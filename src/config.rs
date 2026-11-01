// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

/// Validate that a path specifier does not contain . or .. elements
fn validate_path_specifier(value: &str, key: &str) -> Result<()> {
    for component in value.split('/') {
        if component == "." || component == ".." {
            return Err(anyhow!("Path specifier {} contains invalid component '{}'", key, component));
        }
    }

    Ok(())
}

/// Validate that a local repo path does not contain slashes
fn validate_local_repo_path(value: &str, context: &str) -> Result<()> {
    if value.contains('/') || value.contains('\\') {
        return Err(anyhow!("Local repo path {} contains path separator: {}", context, value));
    }
    Ok(())
}

pub struct RepoValues
{
    pub remote: String,
    pub local: String,
    pub cfgParam: String,
}

pub struct RepoPaths
{
    pub remote: String,
    pub local: String,
    pub cfgParam: String,
}

pub struct RepoSpecification {
    pub paths: RepoPaths,
    pub remoteURL: String,
}

// Typed configuration values with proper types for each setting
annotated_struct!
{
    #[derive(Debug, Clone)]
    pub struct Config {
        /// Remote login information (e.g., ssh://user@host)
        rlogin: String => "RLOGIN",
        /// Remote path base directory
        rpath_base: String => "RPATH_BASE",
        /// Remote path template for new repositories
        rpath_template: String => "RPATH_TEMPLATE",
        /// Media base directory
        gm_dir: String => "GM_DIR",
        /// Remote directory
        remote_dir: String => "REMOTE_DIR",
        /// Command to execute for configuration
        config_cmd: String => "CONFIG_CMD",
        /// Recurse prefix for path display
        recurse_prefix: String => "RECURSE_PREFIX",
    }
}

impl Config {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            rlogin: String::new(),
            rpath_base: String::new(),
            rpath_template: String::new(),
            gm_dir: String::new(),
            remote_dir: String::new(),
            config_cmd: String::new(),
            recurse_prefix: String::new(),
        }
    }

    /// Load configuration from environment variables starting with GRM_
    pub fn load_from_env(&mut self) -> Result<()> {
        // Check if this is a recursive invocation and set the recurse_prefix
        if let Ok(prefix) = std::env::var("GRM_RECURSE_PREFIX") {
            self.recurse_prefix = prefix;
        } else {
            self.recurse_prefix = String::new();
        }
        
        // Determine if we are in a recursive call for permission checking
        let is_recursive = !self.recurse_prefix.is_empty();
        
        for (key, value) in std::env::vars() {
            if let Some(conf_key) = key.strip_prefix("GRM_") {
                // For root process, only allow specific variables from environment
                if !is_recursive {
                    match conf_key {
                        "CONFIG_CMD" => {
                            // These are allowed from environment for root process
                        },
                        _ => {
                            // All other variables are not allowed for root process
                            continue;
                        }
                    }
                }
                
                // Set configuration value
                self.set_by_key(conf_key, value.clone());
                validate_path_specifier(&value, conf_key)?;
            }
        }

		validate_env_loaded_config(self, is_recursive)?;
		Ok(())
    }

    // This should:
    // - load the entire file into RAM in binary mode (no translation)
    // - call parse_config_line until the content is exhausted
    // - treat a single-element vector as a key with an empty value
    // - set configuration values using set_from_string
    //
    // - one empty cell: skip
    // - one non-empty cell: repo with local/GM defaulting
    // - two cells: repo with local/GM override
    // - three cells: repo with local/GM override and remote

    // - return an error if the file cannot be opened or read
    // 

    /// Load configuration from a file
    pub fn load_from_file(&mut self, path: &Path) -> Result<()> {
        for line_result in crate::listfile::LineIterator::from_file(path)?
        {
            use crate::listfile::ParsedLine;

			match line_result
			{
				ParsedLine::Config{key, value} => {
					self.set_by_key(key.as_str(), value.clone());
					validate_path_specifier(&value, &key)?;
				},
				ParsedLine::RepoSpec {..} => {panic!();}, // TODO proper error
				ParsedLine::Malformed => {panic!();}, // TODO proper error
				_ => {},
			};
        }
        
        Ok(())
    }
}

impl RepoValues
{
	/// Extract raw repository path components from config file cells
	pub fn new(cells: [&str; 3]) -> Result<Self>
	{
        use regex::Regex;

		// First cell is always the remote relative path
		let remote_rel = cells[0];
		
		// Second cell is local relative path, defaults to repo_name if empty or missing
		let local_rel = if cells.len() > 1 && !cells[1].is_empty() {
			cells[1].to_string()
		} else {
			// Extract repo name from remote path for default values
			let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
			match re.captures(&remote_rel) {
				Some(caps) => caps.get(1).map_or(String::new(), |m| m.as_str().to_string()),
				None => String::new(),
			}
		};
		
		// Validate local repo path does not contain slashes
		validate_local_repo_path(&local_rel, "local")?;
		
		// Third cell is media relative path, defaults to local_rel if empty or missing
		let media_rel = if cells.len() > 2 && !cells[2].is_empty() {
			cells[2].to_string()
		} else {
			local_rel.clone()
		};
		
		// Validate media repo path does not contain slashes
		validate_local_repo_path(&media_rel, "media")?;
		
		Ok(Self {
			remote: remote_rel.to_string(),
			local: local_rel,
			cfgParam: media_rel,
		})
	}
}

impl RepoPaths
{
	pub fn new(spec: RepoValues,config: &Config) -> Self
	{
		Self
        {
            remote: cat_paths( // TODO do this in one go?
                &config.rpath_base,
                &cat_paths(&config.remote_dir, &spec.remote)),
            local: spec.local,
            cfgParam: cat_paths(&config.gm_dir, &spec.cfgParam),
        }
	}
}

impl RepoSpecification {
    pub fn new(paths: RepoPaths, config: &Config) -> Self {
        let remoteURL = get_remote_url(config, &paths.remote);

        Self
        {
            paths,
            remoteURL,
        }
    }
}

/// Get formatted remote URL based on configuration and a fully-compounded remote path
fn get_remote_url(config: &Config, remote_path: &str) -> String {
	if config.rpath_base.is_empty()
	{
		panic!("RPATH_BASE must exist!");
	}

//	if config.remote_dir.is_empty()
//	{
//		panic!("REMOTE_DIR must exist!");
//	}

	if config.rlogin.is_empty()
	{
		panic!("RLOGIN must exist!");
	}

	build_remote_url(&config.rlogin, remote_path)
}

fn cat_paths(base: &str, rel: &str) -> String {
    // Absolute paths remain unchanged
    if rel.starts_with('/') || base.is_empty() {
        return rel.to_string();
    }

    // Relative paths get base prefix if applicable
    if !rel.is_empty() {
        format!("{}/{}", base, rel)
    } else {
        base.to_string()
    }
}

/// Build a Git clone/fetch URL from components
/// 
/// * `rlogin` - Optional remote login info (e.g., "user@host" or "https://example.com")
/// * `remote_dir` - Remote directory path
/// * `repo_path` - Repository path
fn build_remote_url(rlogin: &str, repo_path: &str) -> String {
    if rlogin.is_empty() {
        // Local path - just join
        return repo_path.to_string();
    }

    let login = rlogin.trim_end_matches('/');

    if !login.contains("://")
    {
		return format!("{}:{}",login,repo_path)
    }

    // Protocol-based URL (http://, https://, ssh://, etc)
    let login_parts: Vec<&str> = login.splitn(2, "://").collect();
    let protocol = login_parts[0];
    let domain = login_parts[1].trim_end_matches('/');
    let path = repo_path.trim_start_matches('/');
    match protocol {
        "http" | "https" => {
            let full_url = format!("{}://{}/{}", protocol, domain.trim_end_matches('/'), path);
            // Try to parse and normalize with gix-url
            if let Ok(parsed) = gix_url::parse(full_url.as_bytes().into()) {
                return parsed.to_string();
            }
            // Fall back to simple string formatting if parsing fails
            full_url
        },
        _ => format!("{}://{}/{}", protocol, domain, path)
    }
}

fn validate_env_loaded_config(config: &Config, is_recursive: bool) -> Result<()> {
	fn validate_scalar(name: &str, value: &str) -> Result<()> {
		if value.contains('\0') || value.contains('\r') || value.contains('\n') {
			return Err(anyhow!("{} contains an invalid character", name));
		}
		if value.starts_with('"') || value.ends_with('"') {
			return Err(anyhow!("{} appears to be quoted (possible formatting bug): {}", name, value));
		}
		Ok(())
	}


	validate_scalar("CONFIG_CMD", &config.config_cmd)?;
	validate_scalar("RECURSE_PREFIX", &config.recurse_prefix)?;

	validate_scalar("RLOGIN", &config.rlogin)?;
	validate_scalar("RPATH_BASE", &config.rpath_base)?;
	validate_scalar("RPATH_TEMPLATE", &config.rpath_template)?;
	validate_scalar("GM_DIR", &config.gm_dir)?;
	validate_scalar("REMOTE_DIR", &config.remote_dir)?;

	if is_recursive {
		let cwd = std::env::current_dir().context("Failed to get current directory")?;
		let list_path = cwd.join(crate::LIST_FILENAME);
		if !list_path.exists() {
			return Err(anyhow!(
				"Recursive invocation directory {} does not contain listfile {}",
				cwd.display(),
				crate::LIST_FILENAME
			));
		}
	}

	Ok(())
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_remote_url_with_login() {
        let result = build_remote_url("user@example.com","organization/repository.git");
        assert_eq!(result,"user@example.com:organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_without_login() {
        let result = build_remote_url("","organization/repository.git");
        assert_eq!(result,"organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_with_protocol() {
        let result = build_remote_url("https://example.com","organization/repository.git");
        assert_eq!(result,"https://example.com/organization/repository.git");
    }

	#[test]
	fn test_build_remote_url_with_scp_absolute_path() {
		let result = build_remote_url("git@server","/srv/git/repo.git");
		assert_eq!(result,"git@server:/srv/git/repo.git");
	}

	#[test]
	fn test_build_remote_url_with_protocol_leading_slash_is_normalized() {
		let result = build_remote_url("https://example.com","/organization/repository.git");
		assert_eq!(result,"https://example.com/organization/repository.git");
	}

	#[test]
	fn test_build_remote_url_local_absolute_path_is_preserved() {
		let result = build_remote_url("","/tmp/repo.git");
		assert_eq!(result,"/tmp/repo.git");
	}


	#[test]
	fn repo_paths_do_not_compound_recurse_prefix_by_default() {
		let mut config = Config::new();
		config.gm_dir = "gm".to_string();
		config.rpath_base = "rbase".to_string();
		config.remote_dir = "".to_string();
		config.recurse_prefix = "Example/Prefix/".to_string();

		let spec = RepoValues {
			remote: "remote".to_string(),
			local: "Example_Repo".to_string(),
			cfgParam: "Example_Repo".to_string(),
		};

		let paths = RepoPaths::new(spec, &config);
		assert_eq!(paths.local, "Example_Repo");
		assert_eq!(paths.cfgParam, "gm/Example_Repo");
	}

	#[test]
	fn remote_url_does_not_duplicate_base_segments_when_remote_dir_empty() {
		let mut config = Config::new();
		config.rlogin = "ssh://git@example.com".to_string();
		config.rpath_base = "git/projects".to_string();
		config.remote_dir = "".to_string();

		let url = get_remote_url(&config, "git/projects/_NOTES");
		assert_eq!(url, "ssh://git@example.com/git/projects/_NOTES");
	}
}