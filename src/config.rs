// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

use crate::LIST_SEPARATOR;

/// Typed configuration values with proper types for each setting
#[derive(Debug, Clone)]
pub struct Config {
    /// Configuration filename (.grm.conf by default)
    pub config_filename: String,
    /// List filename
    pub list_filename: String,
    /// Whether recursion is enabled (1 by default)
    pub recurse_enabled: bool,
    /// Remote login information (e.g., ssh://user@host)
    pub rlogin: String,
    /// Remote path base directory
    pub rpath_base: String,
    /// Remote path template for new repositories
    pub rpath_template: String,
    /// Local base directory for repositories
    pub local_dir: String,
    /// Media base directory
    pub gm_dir: String,
    /// Remote directory
    pub remote_dir: String,
    /// Git arguments when in git mode
    pub git_args: String,
    /// Command to execute for configuration
    pub config_cmd: String,
    /// Recurse prefix for path display
    pub recurse_prefix: String,
    /// Tree filter path for filtering repositories to current subtree
    pub tree_filter: String,
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

    // TODO Why to_string()? Tie lifetimes together to use &str everywhere?

    /// Get all configuration values as string key-value pairs (for environment variable passing)
    pub fn all_values(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        
        // Add all values with their string representations
        result.push(("CONFIG_FILENAME".to_string(), self.config_filename.clone()));
        result.push(("LIST_FN".to_string(), self.list_filename.clone()));
        result.push(("OPT_RECURSE".to_string(), if self.recurse_enabled { "1".to_string() } else { String::new() }));
        
        if !self.rlogin.is_empty() {
            result.push(("RLOGIN".to_string(), self.rlogin.clone()));
        }
        
        if !self.rpath_base.is_empty() {
            result.push(("RPATH_BASE".to_string(), self.rpath_base.clone()));
        }
        
        if !self.rpath_template.is_empty() {
            result.push(("RPATH_TEMPLATE".to_string(), self.rpath_template.clone()));
        }
        
        if !self.local_dir.is_empty() {
            result.push(("LOCAL_DIR".to_string(), self.local_dir.clone()));
        }
        
        if !self.gm_dir.is_empty() {
            result.push(("GM_DIR".to_string(), self.gm_dir.clone()));
        }
        
        if !self.remote_dir.is_empty() {
            result.push(("REMOTE_DIR".to_string(), self.remote_dir.clone()));
        }
        
        if !self.git_args.is_empty() {
            result.push(("GIT_ARGS".to_string(), self.git_args.clone()));
        }
        
        if !self.config_cmd.is_empty() {
            result.push(("CONFIG_CMD".to_string(), self.config_cmd.clone()));
        }
        
        if !self.recurse_prefix.is_empty() {
            result.push(("RECURSE_PREFIX".to_string(), self.recurse_prefix.clone()));
        }
        
        if !self.tree_filter.is_empty() {
            result.push(("TREE_FILTER".to_string(), self.tree_filter.clone()));
        }
        
        result
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
                self.set_from_string(conf_key, value);
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

        // TODO sort out this tree

        for line_result in iter {
			eprintln!("A");
            // First handle any parsing errors
            let mut cells = match line_result {
                Ok(cells) => cells,
                Err(err) => return Err(err.context("Error parsing configuration file"))
            };
            
			eprintln!("B");
            // Error if line contains more than 3 cells
            if cells.len() > 3 {
                return Err(anyhow!("Config line has {} columns instead of the required 3", cells.len()));
            }

			if cells.len() < 3 {
			}

//	FFF	comment or repo line, error
//	FF_	comment or repo line
//	F_F	comment or repo line (implicit remote)
//	F__	comment or repo line (implicit remote+media)
//	_FF	config line
//	_F_	config line (empty value)
//	__F	error
//	___	empty line
			eprintln!("E");
            // We need at least 3 cells for key and value
            if cells.len() == 3 {
                // Move both values out of the vector first
                let key = std::mem::replace(&mut cells[1], String::new());
                let value = std::mem::replace(&mut cells[2], String::new());
                
                // Now that we own key, we can get a reference to it
                let key_ref = key.as_str();
                
                self.set_from_string(key_ref, value);
            }

			eprintln!("C");
            // Error if the first cell is not empty (not a config line)
            if !cells[0].is_empty() {
                return Err(anyhow!("Repository specification found in config file: {:?}", cells));
            }

			eprintln!("D");
            // Only need to check that key (cells[1]) is not empty
            // cells[2] can be empty (which means the config value should be emptied)
            if cells[1].is_empty() {
                return Err(anyhow!("Config line has empty key or value: {:?}", cells));
            }
        }
        
        Ok(())
    }

    /// Set a configuration value from string key and value
    pub fn set_from_string(&mut self, key: &str, value: String) {
        match key {
            "CONFIG_FILENAME" => self.config_filename = value,
            "LIST_FN" => self.list_filename = value,
            "OPT_RECURSE" => self.recurse_enabled = !value.is_empty(),
            "RLOGIN" => self.rlogin = value,
            "RPATH_BASE" => self.rpath_base = value,
            "RPATH_TEMPLATE" => self.rpath_template = value,
            "LOCAL_DIR" => self.local_dir = value,
            "GM_DIR" => self.gm_dir = value,
            "REMOTE_DIR" => self.remote_dir = value,
            "GIT_ARGS" => self.git_args = value,
            "CONFIG_CMD" => self.config_cmd = value,
            "RECURSE_PREFIX" => self.recurse_prefix = value,
            "TREE_FILTER" => self.tree_filter = value,
            _ => {} // Ignore unknown keys
        }
    }
}

/// Slice a string from the current position to the end of the line
/// Returns a substring from the current position to the next line ending character,
/// or an empty slice at the end of the string if no line ending is found.
fn slice_to_eol(input: &str) -> &str {
    for (i, c) in input.char_indices() {
        if c == '\r' || c == '\n' {
            return &input[i..];
        }
    }
    &input[input.len()..]
}