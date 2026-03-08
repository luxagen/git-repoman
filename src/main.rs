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

#[macro_use]
mod annotated_struct;

mod config;
mod invoke;
mod repository;
mod mode;
mod listfile;

use mode::{PrimaryMode, initialize_operations, get_operations, get_mode_string};
pub use config::{RepoPaths,RepoSpec,FullRepoSpec,Config};

/// Separator character used in listfiles
const CELL_SEPARATOR: char = '*';

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
fn process_repo(config: &Config, repo: &FullRepoSpec) -> Result<()> {
//	eprintln!("{}", );
    // Get operations
    let operations = get_operations();

    if operations.list_rrel {
        println!("{}", repo.remote_path); // NEEDS RREL
        return Ok(());
    }
    
    if operations.list_lrel {
		println!("{}{}", config.recurse_prefix, repo.local_path);
        return Ok(());
    }

    if operations.list_rurl {
        println!("{}", repo.remote_url);
        return Ok(());
    }

    let mut state = determine_repo_state(&repo.local_path)?;

    let mut needs_checkout = false;

    println!("{}", &repo.local_path.bright_white());

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
            repository::run_git_command(&repo.local_path, &config.git_args)?;
        }

        if operations.configure {
            repository::configure_repo(&repo, config)?;
        }
    
        if operations.set_remote {
            // fetch?
            repository::set_remote(&repo)?;
        }
    
        if needs_checkout {
            repository::check_out(&repo.local_path)?;
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
			ParsedLine::Malformed => (), // TODO error
			_ => {},
        };
    }
    
    // Process subdirectories if recursion is enabled
    let operations = get_operations();
    if operations.recurse {
        let parent_dir = list_path.parent().unwrap_or(Path::new("."));
		if let Err(err) = recurse_listfiles(parent_dir, config, mode::get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
}

/// Process cells from a repository list file
fn process_repo_line(config: &Config, local: &str, remote: &str, cfg_param: &str) -> Result<()> {
//	eprintln!(
//		"#CONFIG RB_{}_ LD_{}_ GD_{}_ RD_{}_",
//		config.rpath_base,
//		config.local_dir,
//		config.gm_dir,
//		config.remote_dir);

    // Extract raw path components from cells
	let spec = RepoSpec::from_cells([remote, local, cfg_param]);
//	eprintln!("!P1 R:_{}_ L:_{}_ P:_{}_", spec.remote_rel, spec.local_rel, spec.cfg_param);
    let paths = RepoPaths::from_spec(spec, &config);
//	eprintln!("!P2 R:_{}_ L:_{}_ P:_{}_", paths.remote, paths.local, paths.config);
    let full = FullRepoSpec::from_paths(paths,&config);
//	eprintln!("!P3 R:_{}_ L:_{}_ P:_{}_ RU:_{}_", full.remote_path, full.local_path, full.cfg_param, full.remote_url);

    // Filter out repositories that are not in or below the current directory
    if !passes_tree_filter(&config.tree_filter, &full.local_path) {
        return Ok(());
    }
    
    if get_operations().debug {
        eprintln!("Potential target: {}", &full.local_path);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &full) {
        eprintln!("Error processing {}: {}", &full.local_path, err);
    }
    
    Ok(())
}

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

fn main() -> Result<()> {
	unsafe
	{
		std::env::set_var("MSYS_NO_PATHCONV", "1");
	}
    
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
	config.load_from_env()?;

    // Require LIST_FN (list_filename) to be set after config processing
    if config.list_filename.is_empty() {
        return Err(anyhow!("LIST_FN must be set in {}", config.list_filename));
    }
    
    // Initialize operations
    initialize_operations(args.mode);
    
    // Store git command arguments if in git mode
    if args.mode.to_string() == "git" && !args.args.is_empty() {
		let git_args = args.args.iter()
			.map(|arg| shell_escape::unix::escape(Cow::Borrowed(arg.as_str())).to_string())
			.collect::<Vec<String>>()
			.join(" ");
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
        if let Err(err) = process_repofile(&mut config, &list_path) {
            eprintln!("Error processing repofile: {}", err);
        }
    } else {
        eprintln!("No repofile found");
    }
    
    Ok(())
}

/// Find directory containing listfile by walking up from current directory
fn find_listfile_dir(config: &Config) -> Result<PathBuf> {
	let start_dir = env::current_dir()?;
	let mut current_dir = start_dir.clone();
    
    loop {
        let list_path = current_dir.join(&config.list_filename);
        if list_path.exists() {
            return Ok(current_dir);
        }
        
        if !current_dir.pop() {
			return Err(anyhow!(
				"{} Could not find listfile {} in current directory or any ancestor",
				start_dir.display(),
				config.list_filename
			));
        }
    }
}

/// Recursively process subdirectories, spawning new instances of the program
/// for directories containing listfiles
pub fn recurse_listfiles(dir: &Path, config: &Config, mode: &str) -> Result<()> {
    // Check if recursion is enabled
    let operations = get_operations();
    if !operations.recurse {
        return Ok(());
    }
    
    // Clean up the path before processing
    let dir_str = dir.to_string_lossy().to_string();
    let dir_str = dir_str.trim_end_matches('/');
    let dir_path = Path::new(dir_str);
    
    // Read directory entries
    let entries = fs::read_dir(dir_path)
        .with_context(|| format!("Failed to read directory: {}", dir_path.display()))?;
    
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
        
        let list_file_path = path.join(&config.list_filename);
        
        if list_file_path.exists() {
            // Recurse by spawning a new process
            recurse_to_subdirectory(&path, config, mode)?;
            
            // Skip further recursion - the spawned process will handle subdirectories
            continue;
        }
        
        // Continue recursing into this directory
        recurse_listfiles(&path, config, mode)?;
    }
    
    Ok(())
}

/// Spawn a new process to handle a subdirectory with a listfile
fn recurse_to_subdirectory(path: &Path, config: &Config, mode: &str) -> Result<()> {
    // Get relative path for constructing the recurse prefix
    let current_dir = env::current_dir()?;
    let path_rel = if let Ok(rel_path) = path.strip_prefix(&current_dir) {
        rel_path.to_string_lossy().to_string()
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
    cmd.arg(mode)
       .current_dir(path)
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