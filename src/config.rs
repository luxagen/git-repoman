// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

use crate::{remote_url::build_remote_url};

pub struct RepoSpec
{
    pub remote_rel: String,
    pub local_rel: String,
    pub cfg_param: String,
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
    pub fn load_from_env(&mut self) {
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
		eprintln!("config.load_from_file: {}", path.display());

        let iter = crate::listfile::LineIterator::from_file(path)?;

		eprintln!("created iterator");

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

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
	if config.rpath_base.is_empty()
	{
		panic!("RPATH_BASE must exist!");
	}

	if config.remote_dir.is_empty()
	{
		panic!("REMOTE_DIR must exist!");
	}

	if config.rlogin.is_empty()
	{
		panic!("RLOGIN must exist!");
	}

    // Get the base path, defaulting to empty string if not set
    let base_path = &config.rpath_base;

    // Use cat_paths to handle paths consistently
    let full_repo_path = cat_paths(&config.remote_dir, remote_rel_path);
    
    // Choose URL format based on configuration
    if !config.rlogin.is_empty() {
        // We have login information
        build_remote_url(&config.rlogin, base_path, &full_repo_path)
    } else {
        // No login info
        build_remote_url("", base_path, &full_repo_path)
    }
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