#![allow(unused_imports)]

use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::collections::HashMap;
use regex::Regex;
use url::Url;

mod process;
mod recursive;
mod repository;
mod mode;
mod config;
mod remote_url;

use mode::{PrimaryMode, initialize_operations, get_operations};
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

/// Process a single repository
fn process_repo(config: &Config, repo: &RepoTriple) -> Result<()> {
    // Use the recurse prefix directly from the config
    let prefixed_local_path = format!("{}{}", config.recurse_prefix, repo.local);

    // Get operations
    let operations = get_operations();
    
    // Get local path info
    let path = Path::new(repo.local);
    
    // Flag to determine if we need to checkout master at the end
    let mut needs_checkout = false;
    
    // Handle 'new' operation first - it's mutually exclusive with all others
    if operations.new {
        // Check if path exists and is a directory
        if !path.exists() {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }
        
        if !path.is_dir() {
            eprintln!("ERROR: {} is not a directory", prefixed_local_path);
            return Ok(());
        }
        
        // Check if it's already a repository
        let is_repo = match repository::is_dir_repo_root(repo.local) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("Error checking if {} is a Git repository: {}", repo.local, err);
                return Ok(());
            }
        };
        
        if is_repo {
            eprintln!("{} already exists (skipping)", prefixed_local_path);
            return Ok(());
        }
        
        // Create new repo
        eprintln!("Creating new Git repository in {}", prefixed_local_path);
        needs_checkout = repository::create_new(repo, config)?;
        
        // The operations.configure and operations.set_remote flags are already set
        // via the mode->operations translation in mode.rs for 'new' mode
        eprintln!("{} created", prefixed_local_path);
    }
    
    // Handle list_rrel first since it needs the original repo.remote
    if operations.list_rrel {
        println!("{}", repo.remote); // Use original repo.remote for relative path
        return Ok(());
    }
    
    // For all other operations, use the remote URL instead of the relative path
    // Redefine repo to use the URL version for other operations
    let repo = RepoTriple {
        remote: &get_remote_url(config, repo.remote),
        local: repo.local,
        media: repo.media,
    };

    if operations.list_lrel {
        println!("{}", prefixed_local_path);
        return Ok(());
    }

    if operations.list_rurl {
        println!("{}", repo.remote); // Use repo.remote which now contains the URL
        return Ok(());
    }
    
    // Skip processing for listing modes
    if operations.is_listing_mode() {
        return Ok(());
    }
    
    let mut needs_configure = false;

    // Why isn't this running for Moonlighting?!

    // Process based on path state
    if !path.exists() {
        // Only clone if clone operation is enabled
        if !operations.clone {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }

        // Clone, configure, and checkout
        repository::clone_repo_no_checkout(&repo)?;
        needs_configure = true;
        needs_checkout = true;
    }
    
    // Check if path is a directory
    if !path.is_dir() {
        eprintln!("ERROR: {} is not a directory", prefixed_local_path);
        return Ok(());
    }
    
    // Check if directory is a git repository
    let is_repo = match repository::is_dir_repo_root(repo.local) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Error checking if {} is a Git repository: {}", repo.local, err);
            return Ok(());
        }
    };

    if !is_repo {
        // If we get here, the directory exists but isn't a git repository
        eprintln!("ERROR: {} is not a Git repository", prefixed_local_path);
        return Ok(());
    }

    // Get remote URL
    if !operations.new { // Don't print this for new repos (already did)
        eprintln!("{} exists", prefixed_local_path);
    }

    // Configure first, then update remote
    if operations.configure || needs_configure {
        repository::configure_repo(&repo, config)?;
    }

    if operations.set_remote {
        repository::set_remote(&repo)?;
    }

    // Checkout master if needed (for new repositories)
    if needs_checkout {
        repository::check_out(repo.local)?;
    }

    if operations.git {
        // Execute git commands in the repository
        if !config.git_args.is_empty() {
            repository::run_git_command(repo.local, &config.git_args)?;
        }
    }

    return Ok(());
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
        if let Err(err) = recursive::recurse_listfiles(parent_dir, config, &get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
}

/// Get the current mode string
fn get_mode_string() -> String {
    let operations = get_operations();
    if operations.list_lrel { return "list-lrel".to_string(); }
    if operations.list_rrel { return "list-rrel".to_string(); }
    if operations.list_rurl { return "list-rurl".to_string(); }
    if operations.clone { return "clone".to_string(); }
    if operations.configure { return "configure".to_string(); }
    if operations.set_remote { return "set-remote".to_string(); }
    if operations.git { return "git".to_string(); }
    if operations.new { return "new".to_string(); }
    "status".to_string() // default
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
    
    // Get RepoTriple from cells and process it
    let repo_spec = get_repo_triple(&cells)?;
    
    // Filter out repositories that are not in or below the current directory
    if !passes_tree_filter(&config.tree_filter, &repo_spec.local) {
        return Ok(());
    }
    
    if get_operations().debug {
        eprintln!("Potential target: {}", &repo_spec.local);
    }
    
    // Process the repository using the RepoTriple directly
    if let Err(err) = process_repo(config, &repo_spec) {
        eprintln!("Error processing {}: {}", &repo_spec.local, err);
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

fn get_repo_triple<'a>(cells: &'a Vec<String>) -> Result<RepoTriple<'a>> {
    // First cell is always the remote relative path
    let remote_rel = &cells[0];
    
    // Second cell is local relative path, defaults to repo_name if empty or missing
    let local_rel = if cells.len() > 1 && !cells[1].is_empty() {
        &cells[1]
    } else {
        // Extract repo name from remote path for default values
        let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
        match re.captures(&remote_rel) {
            Some(caps) => caps.get(1).map_or("", |m| m.as_str()),
            None => "",
        }
    };
    
    // Third cell is media relative path, defaults to local_rel if empty or missing
    let media_rel = if cells.len() > 2 && !cells[2].is_empty() {
        &cells[2]
    } else {
        local_rel
    };
    
    Ok(RepoTriple { remote: &remote_rel, local: &local_rel, media: &media_rel })
}

/// Get remote repository path from config and relative path
pub fn get_remote_repo_path(config: &Config, repo_path: &str) -> String {
    let remote_dir = &config.remote_dir;
    if !remote_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", remote_dir, repo_path);
        }
        return remote_dir.to_string();
    }
    repo_path.to_string()
}

/// Generate a complete media repository path by combining gm_dir and repo_path
pub fn get_media_repo_path(config: &Config, repo_path: &str) -> String {
    let gm_dir = &config.gm_dir;
    if !gm_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", gm_dir, repo_path);
        }
        return gm_dir.to_string();
    }
    repo_path.to_string()
}

/// Generate a complete local repository path by combining local_dir and repo_path
pub fn get_local_repo_path(config: &Config, repo_path: &str) -> String {
    let local_dir = &config.local_dir;
    if !local_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", local_dir, repo_path);
        }
        return local_dir.to_string();
    }
    repo_path.to_string()
}

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
    // Get the base path, defaulting to empty string if not set
    let base_path = &config.rpath_base;
    
    // Use the remote repo path function to handle paths consistently
    let full_repo_path = get_remote_repo_path(config, remote_rel_path);
    
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
