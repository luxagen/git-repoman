// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

#![allow(unused_imports)]

use std::env;
use std::f32::consts::E;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::collections::HashMap;
use regex::Regex;
use url::Url;
use colored::Colorize;

mod process;
mod recursive;
mod repository;
mod mode;
mod config;
mod remote_url;

use mode::{PrimaryMode, initialize_operations, get_operations, get_mode_string};
use config::{Config, ConfigLineIterator};

/// Separator character used in listfiles
const LIST_SEPARATOR: char = '*';

/// Git Repository Manager - Rust implementation
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Mode of operation
    #[clap(value_enum)]
    mode: PrimaryMode,
    
    /// Additional arguments (for git mode)
    #[clap(trailing_var_arg = true)]
    args: Vec<String>,
}

/// Find the nearest configuration file by walking up directories
fn find_conf_file(config: &Config) -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    
    loop {
        let conf_path = current_dir.join(&config.config_filename);
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
fn process_repo(config: &Config, repo: &RepoTriple) -> Result<()> {
    // Get operations
    let operations = get_operations();

    if operations.list_rrel {
        println!("{}", repo.remote_path); // NEEDS RREL
        return Ok(());
    }
    
    if operations.list_lrel {
        println!("{}", repo.local_path);
        return Ok(());
    }

    if operations.list_rurl {
        println!("{}", repo.remote_url);
        return Ok(());
    }

    let mut state = determine_repo_state(&repo.local_path)?;

    let mut needs_checkout = false;

    print!("{}: ", &repo.local_path.bright_white());

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
                repository::init_new(&repo.local_path)?;

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
            repository::run_git_command(repo.local_path, &config.git_args)?;
        }

        if operations.configure {
            repository::configure_repo(&repo, config)?;
        }
    
        if operations.set_remote {
            // fetch?
            repository::set_remote(&repo)?;
        }
    
        if needs_checkout {
            repository::check_out(repo.local_path)?;
        }

        return Ok(()); // Job done
    }
}

/// Process a repository listfile
fn process_listfile(config: &mut Config, list_path: &Path) -> Result<()> {
    // Use ConfigLineIterator to handle file reading and line parsing
    let iter = ConfigLineIterator::from_file(list_path)?;
    
    // Process each parsed line
    for line_result in iter {
        // Handle parsing errors
        let cells = match line_result {
            Ok(cells) => cells,
            Err(err) => {
                eprintln!("Error parsing line: {}", err);
                continue;
            }
        };
        
        // Skip empty lines and comments (already handled by ConfigLineIterator)
        if cells.is_empty() {
            continue;
        }
        
        // Process the repository line cells
        if let Err(err) = process_repo_line(config, cells) {
            eprintln!("Error processing repository line: {}", err);
        }
    }
    
    // Process subdirectories if recursion is enabled
    let operations = get_operations();
    if operations.recurse {
        let parent_dir = list_path.parent().unwrap_or(Path::new("."));
        if let Err(err) = recursive::recurse_listfiles(parent_dir, config, mode::get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
}

/// Process cells from a repository list file
fn process_repo_line(config: &mut Config, cells: Vec<String>) -> Result<()> {
    // Skip empty cell arrays (already handled by ConfigLineIterator)
    if cells.is_empty() {
        return Ok(());
    }
    
    // Skip comment lines where first non-empty cell starts with #
    for cell in &cells {
        if !cell.is_empty() {
            if cell.starts_with('#') {
                return Ok(());
            }
            break; // Found first non-empty cell that doesn't start with #
        }
    }
    
    // Handle config lines (first cell is empty, indicating it starts with separator)
    if cells[0].is_empty() {
        // This is a config line
        if cells.len() >= 3 {
            let key = cells[1].clone();
            let value = cells[2].clone();
            // Format: * KEY * VALUE
            config.set_from_string(&key, value);
        }
        return Ok(());
    }

    // Extract raw path components from cells
    let (remote, local, media) = extract_repo_paths(&cells);
    let (remote, local, media) = qualify_repo_paths(&config, &remote, &local, &media);
    let remote_url = get_remote_url(&config, &remote);

    let rt = RepoTriple::new(
        &remote,
        &local,
        &media,
        &remote_url,
    );
    
    // Filter out repositories that are not in or below the current directory
    if !passes_tree_filter(&config.tree_filter, &rt.local_path) {
        return Ok(());
    }
    
    if get_operations().debug {
        eprintln!("Potential target: {}", &rt.local_path);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &rt) {
        eprintln!("Error processing {}: {}", &rt.local_path, err);
    }
    
    Ok(())
}

// Use the shared RepoTriple from repository.rs
use crate::repository::RepoTriple;

/// Check if a repository local path passes the tree filter
/// Returns true if there is no filter or if the path is within the filter
fn passes_tree_filter(tree_filter: &str, local_path: &str) -> bool {
    // If there's no tree filter, all paths pass
    if tree_filter.is_empty() {
        return true;
    }
    
    // Get the absolute path from the current directory
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let abs_local_path = current_dir.join(local_path);
    let abs_local_str = abs_local_path.to_string_lossy().replace('\\', "/");
    let tree_filter_str = tree_filter.replace('\\', "/");
    
    // Check if the absolute path contains our filter string
    let passes = abs_local_str.contains(&tree_filter_str);
    
    if !passes && get_operations().debug {
        eprintln!("Skipping repository outside tree filter: {} (not in {})", local_path, tree_filter_str);
    }
    
    passes
}

/// Concatenate paths
pub fn cat_paths(base: &str, rel: &str) -> String {
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

/// Extract raw repository path components from config file cells
fn extract_repo_paths(cells: &Vec<String>) -> (String, String, String) {
    // First cell is always the remote relative path
    let remote_rel = cells[0].clone();
    
    // Second cell is local relative path, defaults to repo_name if empty or missing
    let local_rel = if cells.len() > 1 && !cells[1].is_empty() {
        cells[1].clone()
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
        cells[2].clone()
    } else {
        local_rel.clone()
    };
    
    (remote_rel, local_rel, media_rel)
}

/// Qualify repository paths based on configuration
fn qualify_repo_paths(config: &Config, remote: &str, local: &str, media: &str) -> (String, String, String) {
    (
        cat_paths( // TODO do this in one go?
            &config.rpath_base,
            &cat_paths(&config.remote_dir, remote)),
        cat_paths(&config.local_dir, &local),
        cat_paths(&config.gm_dir, &media),
    )
}

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
    // Get the base path, defaulting to empty string if not set
    let base_path = &config.rpath_base;
    
    // Use cat_paths to handle paths consistently
    let full_repo_path = cat_paths(&config.remote_dir, remote_rel_path);
    
    // Choose URL format based on configuration
    if !config.rlogin.is_empty() {
        // We have login information
        remote_url::build_remote_url(&config.rlogin, base_path, &full_repo_path)
    } else {
        // No login info
        remote_url::build_remote_url("", base_path, &full_repo_path)
    }
}

fn main() -> Result<()> {
    std::env::set_var("MSYS_NO_PATHCONV", "1");
    
    // Save the original working directory to use as a tree filter (like $treeFilter in Perl)
    let tree_filter = env::current_dir()?;
    let tree_filter_str = tree_filter.to_string_lossy().to_string();
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Create configuration
    let mut config = Config::new();
    
    // Load configuration from file
    let conf_path = find_conf_file(&config)?;
    config.load_from_file(&conf_path)?;
    
    // Load configuration from environment variables
    config.load_from_env();

    // Require LIST_FN (list_filename) to be set after config processing
    if config.list_filename.is_empty() {
        return Err(anyhow!("LIST_FN must be set in {}", config.list_filename));
    }
    
    // Initialize operations
    initialize_operations(args.mode);
    
    // Store git command arguments if in git mode
    if args.mode.to_string() == "git" && !args.args.is_empty() {
        let git_args = args.args.join(" ");
        config.git_args = git_args;
    }
    
    // Get listfile directory and path
    let list_dir = find_listfile_dir(&config)?;
    let list_path = list_dir.join(&config.list_filename);
    
    // Just like Perl, change to the listfile directory - this simplifies path handling
    env::set_current_dir(&list_dir)?;
    
    // Store original working directory for filtering
    config.tree_filter = tree_filter_str;
    
    // Process listfile
    if list_path.exists() {
        if let Err(err) = process_listfile(&mut config, &list_path) {
            eprintln!("Error processing listfile: {}", err);
        }
    } else {
        eprintln!("No listfile found");
    }
    
    Ok(())
}

/// Find directory containing listfile by walking up from current directory
fn find_listfile_dir(config: &Config) -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    
    loop {
        let list_path = current_dir.join(&config.list_filename);
        if list_path.exists() {
            return Ok(current_dir);
        }
        
        if !current_dir.pop() {
            return Err(anyhow!("Could not find listfile {} in current directory or any ancestor", config.list_filename));
        }
    }
}
