// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

#![allow(unused_imports)]

use std::env;
use std::f32::consts::E;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::borrow::Cow;
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::collections::HashMap;
use url::Url;
use colored::Colorize;
use once_cell::sync::OnceCell;
use anstream::{eprintln,println};

#[macro_use]
mod annotated_struct;

mod config;
mod invoke;
mod repository;
mod mode;
mod listfile;

use mode::{PrimaryMode, initialise_operations, get_operations, get_mode_string};
pub use config::{RepoPaths,RepoValues,RepoSpecification,Config};

/// Separator character used in listfiles
const CELL_SEPARATOR: char = '*';

/// Configuration filename
const CONFIG_FILENAME: &str = ".grm.conf";

/// List filename
const LIST_FILENAME: &str = ".grm.repos";

/// Git Repository Manager - Rust implementation
#[derive(Parser, Debug)]
#[clap(author, version, about)]
pub struct Args {
    /// Mode of operation
    #[clap(value_enum)]
    pub mode: PrimaryMode,
    
    /// Additional arguments (for git mode)
    #[clap(trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Global ARGS initialized once at startup
static ARGS: OnceCell<Args> = OnceCell::new();

/// Initialize the global args - call this ONCE at startup
fn initialise_args() {
    ARGS.set(Args::parse()).expect("ARGS already initialized");
}

/// Get a reference to the args
/// Panics if initialize_args wasn't called first
pub fn get_args() -> &'static Args {
    ARGS.get().expect("ARGS not initialized")
}

/// Find the nearest configuration file by walking up directories
fn find_conf_file() -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    
    loop {
        let conf_path = current_dir.join(CONFIG_FILENAME);
        if conf_path.exists() {
            return Ok(conf_path);
        }
        
        if !current_dir.pop() {
            break;
        }
    }
    
    Err(anyhow!("Configuration file not found"))
}

enum RepoState {
    Missing,
    File,
    Directory,
    Repo,
}

fn determine_repo_state(path: &str) -> Result<RepoState> {
    let path = Path::new(path);

    if !path.exists() {
        return Ok(RepoState::Missing);
    }

    if !path.is_dir() {
        return Ok(RepoState::File);
    }

    match repository::is_dir_repo_root(path.to_str().unwrap()) {
        Ok(result) => Ok(if result { RepoState::Repo } else { RepoState::Directory }),
        Err(err) => Err(err)
    }
}    

/// Process a single repository
#[allow(unused_assignments)] // Stupid compiler
fn process_repo(config: &Config, repo: &RepoSpecification) -> Result<()> {
//	eprintln!("{}", );
    // Get operations
    let operations = get_operations();

    if operations.list_rrel {
        println!("{}", repo.paths.remote);
        return Ok(());
    }
    
    if operations.list_lrel {
		println!("{}{}", config.recurse_prefix, repo.paths.local);
        return Ok(());
    }

    if operations.list_rurl {
        println!("{}", repo.remoteURL);
        return Ok(());
    }

    let mut state = determine_repo_state(&repo.paths.local)?;

    let mut needs_checkout = false;

    println!("* {}{}", config.recurse_prefix.bright_white(), repo.paths.local.bright_white());

    // State machine for the repository
    loop {
        let _ = state;
        state = match state {
            RepoState::File => {
                return Ok(()); // Terminal
            }
            RepoState::Missing => {
                if !operations.clone {
                    return Ok(()); // Terminal
                }

                repository::clone_repo_no_checkout(&repo)?;
                needs_checkout = true;
                RepoState::Repo // New state
            }
            RepoState::Directory => {
                if !operations.new {
                    return Ok(()); // Terminal
                }

                // Initialize git repository
                repository::init_new(&repo.paths.local)?;

                // dir: create_remote (is not repo)
                needs_checkout = repository::create_remote(&repo, config, false)?;
                RepoState::Repo // New state
            }
            RepoState::Repo => {
                if operations.new {
                    needs_checkout = repository::create_remote(&repo, config, true)?;
                }

                RepoState::Repo // Unchanged
            }
        };

        if operations.git {
            repository::run_git_command(&repo.paths.local, crate::get_args().args.as_slice())?;
            println!();
        }

        if operations.configure {
            repository::configure_repo(&repo, config)?;
        }
    
        if operations.set_remote {
            // fetch?
            repository::set_remote(&repo)?;
        }
    
        if needs_checkout {
            repository::check_out(&repo.paths.local)?;
        }

        return Ok(()); // Job done
    }
}

/// Process a repository listfile
fn process_repofile(config: &mut Config, list_path: &Path) -> Result<()> {
    use listfile::ParsedLine;

    // Use ConfigLineIterator to handle file reading and line parsing
    let iter = listfile::LineIterator::from_file(list_path)?;
    
    // Process each parsed line
    for line_result in iter {
        // Handle parsing errors
        match line_result
		{
			ParsedLine::Config{key, value} => config.set_by_key(&key, value),
			ParsedLine::RepoSpec{local, remote, param} => process_repo_line(config, &local, &remote, &param)?,
			ParsedLine::Malformed => return Err(anyhow!("Malformed line")),
			_ => continue,
        };
    }
    
    Ok(())
}

/// Process cells from a repository list file
fn process_repo_line(config: &Config, local: &str, remote: &str, cfg_param: &str) -> Result<()> {

    // Extract raw path components from cells
	let spec = RepoValues::new([remote, local, cfg_param])?;
    let paths = RepoPaths::new(spec, &config);
    let full = RepoSpecification::new(paths,&config);

    if get_operations().debug {
        eprintln!("Potential target: {}", &full.paths.local);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &full) {
        eprintln!("Error processing {}: {}", &full.paths.local, err);
    }
    
    Ok(())
}

fn main() -> Result<()> {
	unsafe
	{
		std::env::set_var("MSYS_NO_PATHCONV", "1");
	}
    
    // Parse command line arguments and initialize globals
    initialise_args();
    
    // Create configuration
    let mut config = Config::new();
    
    // Load configuration from file
    let conf_path = find_conf_file()?;
    config.load_from_file(&conf_path)?;
    
    // Load configuration from environment variables
	config.load_from_env()?;
    
    // Initialize operations
    initialise_operations(get_args().mode);
    
    // Check for listfile in current directory
    let current_dir = env::current_dir()?;
    let list_path = current_dir.join(LIST_FILENAME);
    
    // Process listfile if it exists
    if list_path.exists() {
        if let Err(err) = process_repofile(&mut config, &list_path) {
            eprintln!("Error processing repofile: {}", err);
        }
    }
    
    // Recursively process subdirectories for additional listfiles
    if let Err(err) = recurse_listfiles(&current_dir, &config, mode::get_mode_string()) {
        eprintln!("Error during recursion: {}", err);
    }
    
    Ok(())
}

/// Recursively process subdirectories, spawning new instances of the program
/// for directories containing listfiles
pub fn recurse_listfiles(dir: &Path, config: &Config, mode: &str) -> Result<()> {
    // Check if recursion is enabled
    let operations = get_operations();
    if !operations.recurse {
        return Ok(());
    }
    
    // Read directory entries
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        
        // Skip non-directories and hidden directories
        if !path.is_dir() || path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false) {
            continue;
        }
        
        if path.join(LIST_FILENAME).is_file() {
            // Recurse by spawning a new process, which will recurse that directory itself
            fork_for_subdirectory(&path, config, mode)?;
            continue;
        }
        
        // Continue recursing into this directory
        recurse_listfiles(&path, config, mode)?;
    }
    
    Ok(())
}

/// Spawn a new process to handle a subdirectory with a listfile
fn fork_for_subdirectory(path: &Path, config: &Config, mode: &str) -> Result<()> {
    // Get relative path for constructing the recurse prefix
    let current_dir = env::current_dir()?;
    let path_rel = if let Ok(rel_path) = path.strip_prefix(&current_dir) {
        rel_path.to_string_lossy().replace('\\', "/")
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    
    // Generate recurse prefix based on current hierarchy
    let recurse_prefix = if config.recurse_prefix.is_empty() {
        format!("{}/", path_rel)
    } else {
        format!("{}{}/", config.recurse_prefix, path_rel)
    };
    
    // Get path to current executable
    let exe_path = env::current_exe()
        .context("Failed to get path to current executable")?;
    
    // Build command to execute in subdirectory with preserved environment
    let mut cmd = std::process::Command::new(exe_path);
    cmd.arg(mode);
    
    // Pass git arguments if in git mode
    if mode == "git" {
        for arg in &get_args().args {
            cmd.arg(arg);
        }
    }
    
    cmd.current_dir(path)
       // Set the recurse prefix for this level
       .env("GRM_RECURSE_PREFIX", recurse_prefix);
    
    // Add all config values with GRM_ prefix
    for (key, value) in config.all_values() {
        if key == "RECURSE_PREFIX" {
            // Don't pass recurse prefix (already handled)
            continue;
        }
        
        // Add GRM_ prefix to all other config variables
        cmd.env(format!("GRM_{}", key), value);
    }
    
    // Execute the command with preserved environment
    let status = cmd.status()
        .with_context(|| format!("Failed to spawn recursive process in: {}", path.display()))?;
    
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        eprintln!("Warning: Recursive instance in {} exited with code: {}", path.display(), code);
    }
    
    Ok(())
}