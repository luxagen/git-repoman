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
use regex::Regex;
use url::Url;
use colored::Colorize;

mod invoke;
mod recursive;
mod repository;
mod mode;
mod config;
mod remote_url;
mod listfile;

use mode::{PrimaryMode, initialize_operations, get_operations, get_mode_string};
use config::Config;

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

// Use the shared RepoTriple from repository.rs
use crate::repository::FullRepoSpec;

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
        println!("{}", repo.local_path);
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
        if let Err(err) = recursive::recurse_listfiles(parent_dir, config, mode::get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
}

/// Process cells from a repository list file
fn process_repo_line(config: &Config, local: &str, remote: &str, cfg_param: &str) -> Result<()> {
	eprintln!(
		"#CONFIG RB_{}_ LD_{}_ GD_{}_ RD_{}_",
		config.rpath_base,
		config.local_dir,
		config.gm_dir,
		config.remote_dir);

    // Extract raw path components from cells
	let spec = RepoSpec::from_cells([remote, local, cfg_param]);
	eprintln!("!P1 R:_{}_ L:_{}_ P:_{}_", spec.remote_rel, spec.local_rel, spec.cfg_param);
    let paths = RepoPaths::from_spec(spec, &config);
	eprintln!("!P2 R:_{}_ L:_{}_ P:_{}_", paths.remote, paths.local, paths.config);

    let remote_url = get_remote_url(&config, &paths.remote);
    let full = FullRepoSpec::new(
        paths.remote,
        paths.local,
        paths.config,
        remote_url,
    );
    
	eprintln!("!P3 R:_{}_ L:_{}_ P:_{}_ U:_{}_", full.remote_path, full.local_path, full.cfg_param, full.remote_url);

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

struct RepoSpec
{
    remote_rel: String,
    local_rel: String,
    cfg_param: String,
}

impl RepoSpec
{
	/// Extract raw repository path components from config file cells
	fn from_cells(cells: [&str; 3]) -> Self
	{
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

struct RepoPaths
{
    remote: String,
    local: String,
    config: String,
}

impl RepoPaths
{
	fn from_spec(spec: RepoSpec,config: &Config) -> Self
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
        remote_url::build_remote_url(&config.rlogin, base_path, &full_repo_path)
    } else {
        // No login info
        remote_url::build_remote_url("", base_path, &full_repo_path)
    }
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
    config.load_from_env();

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
