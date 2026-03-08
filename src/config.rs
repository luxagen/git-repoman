// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

pub struct RepoSpec
{
    pub remote_rel: String,
    pub local_rel: String,
    pub cfg_param: String,
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

	validate_scalar("CONFIG_FILENAME", &config.config_filename)?;
	if config.config_filename.is_empty() {
		return Err(anyhow!("CONFIG_FILENAME must not be empty"));
	}

	validate_scalar("LIST_FN", &config.list_filename)?;
	if config.list_filename.is_empty() {
		return Err(anyhow!("LIST_FN must not be empty"));
	}
	if config.list_filename.contains('/') || config.list_filename.contains('\\') {
		return Err(anyhow!("LIST_FN must be a filename, not a path: {}", config.list_filename));
	}

	validate_scalar("CONFIG_CMD", &config.config_cmd)?;
	validate_scalar("RECURSE_PREFIX", &config.recurse_prefix)?;
	validate_scalar("TREE_FILTER", &config.tree_filter)?;

	validate_scalar("RLOGIN", &config.rlogin)?;
	validate_scalar("RPATH_BASE", &config.rpath_base)?;
	validate_scalar("RPATH_TEMPLATE", &config.rpath_template)?;
	validate_scalar("LOCAL_DIR", &config.local_dir)?;
	validate_scalar("GM_DIR", &config.gm_dir)?;
	validate_scalar("REMOTE_DIR", &config.remote_dir)?;
	validate_scalar("GIT_ARGS", &config.git_args)?;

	if is_recursive {
		let cwd = std::env::current_dir().context("Failed to get current directory")?;
		let list_path = cwd.join(&config.list_filename);
		if !list_path.exists() {
			return Err(anyhow!(
				"Recursive invocation directory {} does not contain listfile {}",
				cwd.display(),
				config.list_filename
			));
		}
	}

	Ok(())
}

pub struct RepoPaths
{
    pub remote: String,
    pub local: String,
    pub config: String,
}

#[derive(Debug)]
pub struct FullRepoSpec {
    pub remote_path: String,
    pub remote_url: String,
    pub local_path: String,
    pub cfg_param: String, // TODO REMOVE
}

// Typed configuration values with proper types for each setting
annotated_struct!
{
    #[derive(Debug, Clone)]
    pub struct Config {
        /// Configuration filename (.grm.conf by default)
        config_filename: String => "CONFIG_FILENAME",
        /// List filename
        list_filename: String => "LIST_FN",
        /// Whether recursion is enabled (1 by default)
        recurse_enabled: bool => "OPT_RECURSE",
        /// Remote login information (e.g., ssh://user@host)
        rlogin: String => "RLOGIN",
        /// Remote path base directory
        rpath_base: String => "RPATH_BASE",
        /// Remote path template for new repositories
        rpath_template: String => "RPATH_TEMPLATE",
        /// Local base directory for repositories
        local_dir: String => "LOCAL_DIR",
        /// Media base directory
        gm_dir: String => "GM_DIR",
        /// Remote directory
        remote_dir: String => "REMOTE_DIR",
        /// Git arguments when in git mode
        git_args: String => "GIT_ARGS",
        /// Command to execute for configuration
        config_cmd: String => "CONFIG_CMD",
        /// Recurse prefix for path display
        recurse_prefix: String => "RECURSE_PREFIX",
        /// Tree filter path for filtering repositories to current subtree
        tree_filter: String => "TREE_FILTER",
    }
}

impl Config {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            config_filename: ".grm.conf".to_string(),
            list_filename: String::new(),
            recurse_enabled: true,
            rlogin: String::new(),
            rpath_base: String::new(),
            rpath_template: String::new(),
            local_dir: String::new(),
            gm_dir: String::new(),
            remote_dir: String::new(),
            git_args: String::new(),
            config_cmd: String::new(),
            recurse_prefix: String::new(),
            tree_filter: String::new(),
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
                        "CONFIG_FILENAME" | "LIST_FN" | "CONFIG_CMD" => {
                            // These are allowed from environment for root process
                        },
                        _ => {
                            // All other variables are not allowed for root process
                            continue;
                        }
                    }
                }
                
                // Set configuration value
                self.set_by_key(conf_key, value);
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
//		eprintln!("config.load_from_file: {}", path.display());

        let iter = crate::listfile::LineIterator::from_file(path)?;

//		eprintln!("created iterator");

        for line_result in iter
        {
            use crate::listfile::ParsedLine;

			match line_result
			{
				ParsedLine::Config{key, value} => self.set_by_key(key.as_str(), value),
				ParsedLine::RepoSpec {..} => {panic!();}, // TODO proper error
				ParsedLine::Malformed => {panic!();}, // TODO proper error
				_ => {},
			};
        }
        
        Ok(())
    }
}

impl RepoSpec
{
	/// Extract raw repository path components from config file cells
	pub fn from_cells(cells: [&str; 3]) -> Self
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
		
		// Third cell is media relative path, defaults to local_rel if empty or missing
		let media_rel = if cells.len() > 2 && !cells[2].is_empty() {
			cells[2].to_string()
		} else {
			local_rel.clone()
		};
		
		Self {
			remote_rel: remote_rel.to_string(),
			local_rel,
			cfg_param: media_rel,
		}
	}
}

impl RepoPaths
{
	pub fn from_spec(spec: RepoSpec,config: &Config) -> Self
	{
		Self
        {
            remote: cat_paths( // TODO do this in one go?
                &config.rpath_base,
                &cat_paths(&config.remote_dir, &spec.remote_rel)),
            local: cat_paths(&config.local_dir, &spec.local_rel),
            config: cat_paths(&config.gm_dir, &spec.cfg_param),
        }
	}
}

impl FullRepoSpec {
    pub fn from_paths(paths: RepoPaths, config: &Config) -> Self {
        let remote_url = get_remote_url(config, &paths.remote);

        Self
        {
            remote_path: paths.remote,
            remote_url,
            local_path: paths.local,
            cfg_param: paths.config,
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
/// * `rlogin` - Optional remote login info (e.g., "user@host" or "https://github.com")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_remote_url_with_login() {
        let result = build_remote_url("user@github.com","organization/repository.git");
        assert_eq!(result,"user@github.com:organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_without_login() {
        let result = build_remote_url("","organization/repository.git");
        assert_eq!(result,"organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_with_protocol() {
        let result = build_remote_url("https://github.com","organization/repository.git");
        assert_eq!(result,"https://github.com/organization/repository.git");
    }

	#[test]
	fn test_build_remote_url_with_scp_absolute_path() {
		let result = build_remote_url("git@server","/srv/git/repo.git");
		assert_eq!(result,"git@server:/srv/git/repo.git");
	}

	#[test]
	fn test_build_remote_url_with_protocol_leading_slash_is_normalized() {
		let result = build_remote_url("https://github.com","/organization/repository.git");
		assert_eq!(result,"https://github.com/organization/repository.git");
	}

	#[test]
	fn test_build_remote_url_local_absolute_path_is_preserved() {
		let result = build_remote_url("","/tmp/repo.git");
		assert_eq!(result,"/tmp/repo.git");
	}

	#[test]
	fn all_values_does_not_quote_strings() {
		let mut config = Config::new();
		config.list_filename = ".grm.repos".to_string();
		let all = config.all_values();
		let list_fn = all
			.iter()
			.find(|(k, _)| k == "LIST_FN")
			.map(|(_, v)| v.as_str())
			.expect("LIST_FN must be present in all_values()");
		assert_eq!(list_fn, ".grm.repos");
		assert!(!list_fn.starts_with('"') && !list_fn.ends_with('"'));
	}

	#[test]
	fn repo_paths_do_not_compound_recurse_prefix_by_default() {
		let mut config = Config::new();
		config.local_dir = "local".to_string();
		config.gm_dir = "gm".to_string();
		config.rpath_base = "rbase".to_string();
		config.remote_dir = "".to_string();
		config.recurse_prefix = "Ian/Ade/".to_string();

		let spec = RepoSpec {
			remote_rel: "remote".to_string(),
			local_rel: "Guardian_Angel".to_string(),
			cfg_param: "Guardian_Angel".to_string(),
		};

		let paths = RepoPaths::from_spec(spec, &config);
		assert_eq!(paths.local, "local/Guardian_Angel");
		assert_eq!(paths.config, "gm/Guardian_Angel");
	}

	#[test]
	fn remote_url_does_not_duplicate_base_segments_when_remote_dir_empty() {
		let mut config = Config::new();
		config.rlogin = "ssh://git@git.luxagen.net".to_string();
		config.rpath_base = "git/music-projects".to_string();
		config.remote_dir = "".to_string();

		let url = get_remote_url(&config, "git/music-projects/_IDEAS");
		assert_eq!(url, "ssh://git@git.luxagen.net/git/music-projects/_IDEAS");
	}
}