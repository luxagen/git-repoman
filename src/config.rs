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
//            pub const ANNOTATIONS: &'static [(&'static str, &'static str)] = &[
//                $( (stringify!($field), $ann) ),*
//            ];
//
//            pub fn populate_from_map(mut self, mut map: HashMap<&'static str, String>) -> Self {
//                $(
//                    if let Some(value) = map.remove($ann) {
//                        self.$field = value.parse::<$ty>().unwrap_or_default();
//                    }
//                )*
//                self
//            }

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

            // AUTO-GENERATED: no manual repetition!
            pub fn all_values(&self) -> Vec<(String, String)> {
                let mut result = Vec::new();
                $(
                    result.push(($ann.to_string(), format!("{:?}", self.$field)));
                )*
                result
            }
        }
    };
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