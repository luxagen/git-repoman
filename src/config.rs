// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

use crate::config;

macro_rules! annotated_struct {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_attr:meta])*
                $field:ident : $ty:ty => $ann:expr
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis struct $name {
            $( $(#[$field_attr])* pub $field: $ty ),*
        }

        impl $name {
            pub const ANNOTATIONS: &'static [(&'static str, &'static str)] = &[
                $( (stringify!($field), $ann) ),*
            ];

            pub fn populate_from_map(mut self, mut map: HashMap<&'static str, String>) -> Self {
                $(
                    if let Some(value) = map.remove($ann) {
                        self.$field = value.parse::<$ty>().unwrap_or_default();
                    }
                )*
                self
            }

            pub fn set_by_key(&mut self, key: &str, value: String) {
                match key {
                    $(
                        $ann => {
                            self.$field = value.parse::<$ty>().unwrap_or_default();
                            return;
                        }
                    )*
                    _ => {}
                }
            }
        }
    };
}

// "CONFIG_FILENAME" => self.config_filename = value,
// "LIST_FN" => self.list_filename = value,
// "OPT_RECURSE" => self.recurse_enabled = !value.is_empty(),
// "RLOGIN" => self.rlogin = value,
// "RPATH_BASE" => self.rpath_base = value,
// "RPATH_TEMPLATE" => self.rpath_template = value,
// "LOCAL_DIR" => self.local_dir = value,
// "GM_DIR" => self.gm_dir = value,
// "REMOTE_DIR" => self.remote_dir = value,
// "GIT_ARGS" => self.git_args = value,
// "CONFIG_CMD" => self.config_cmd = value,
// "RECURSE_PREFIX" => self.recurse_prefix = value,
// "TREE_FILTER" => self.tree_filter = value,

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
				ParsedLine::RepoSpec {local, remote, param} => {panic!();}, // TODO proper error
				ParsedLine::Malformed => {panic!();}, // TODO proper error
				_ => {},
			};
        }
        
        Ok(())
    }
}