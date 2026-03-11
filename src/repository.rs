// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::borrow::Cow;
use std::io::Write;
use std::path::Path;
use anyhow::{Context, Result, anyhow};
use crate::config::{Config,RepoValues};
use crate::invoke;

pub use crate::config::RepoSpecification;

/// Check if directory is a Git repository root
pub fn is_dir_repo_root(local_path: &str) -> Result<bool> {
    // Use git rev-parse --git-dir which is more efficient for checking repository existence
    // This is a plumbing command that directly checks for the .git directory
	let output = invoke::run_git_capture(local_path, &["rev-parse", "--git-dir"])
		.with_context(|| format!("Failed to check if {} is a git repo root", local_path))?;
    
    // If command succeeds, it's a git repository
	if output.exit_code != 0 {
        return Ok(false);
    }
    
    // Check if we're at the root (.git dir is directly in this directory)
    // If output is just ".git", we're at the repository root
	let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(git_dir == ".git")
}

/// Initialize a git repository
pub fn init_new(local_path: &str) -> Result<()> {
    // Initialize git repository
    let status = invoke::run_in_dir(local_path, &["git", "init", "-q"])?;
    if status != 0 {
        return Err(anyhow!("Git init failed with exit code: {}", status));
    }
    Ok(())
}

/// Clone a repository without checking it out
pub fn clone_repo_no_checkout(repo: &RepoSpecification) -> Result<()> {
    println!("Cloning repository \"{}\" into \"{}\"", &repo.remoteURL, &repo.paths.local);
	let status = invoke::run_git_status(".", &["clone", "--no-checkout", &repo.remoteURL, &repo.paths.local])
		.with_context(|| format!("Failed to execute clone: {}", &repo.remoteURL))?;
	if status != 0 {
		return Err(anyhow!("Git clone failed with exit code: {}", status));
	}
    Ok(())
}

/// Configure a repository using the provided command

pub fn configure_repo(repo: &RepoSpecification, config: &Config) -> Result<()> {
    execute_config_cmd(repo, config)
}

// TODO: figure out whether to always fetch

/// Update the remote URL for a repository
pub fn set_remote(repo: &RepoSpecification) -> Result<()> {
    let status = invoke::run_command_silent(&repo.paths.local, &["git", "remote", "set-url", "origin", &repo.remoteURL])?;
    if status == 2 {
        println!("Adding remote origin");
		crate::invoke::run_git_cmd(&repo.paths.local, &["remote", "add", "-f", "origin", &repo.remoteURL], None)?;
    } else if status != 0 {
        return Err(anyhow!("Failed to set remote with exit code: {}", status));
    }
    Ok(())
}

// TODO: figure out whether this will work for both new and clone

/// Checkout the default branch after cloning
pub fn check_out(local_path: &str) -> Result<()> {
    println!("Checking out repository at \"{}\"", local_path);
    
    // Reset to get the working directory in sync with remote
	crate::invoke::run_git_cmd(local_path, &["checkout"], Some("checkout"))?;
    
    Ok(())
}

// create_remote:
// 0. if RLOGIN protocol is not SSH or local, abort with "cannot auto-create non-SSH remotes" complaint
// 1. else is protocol is SSH, connect and pipe in the shell script below
// 2. else if RLOGIN protocol is local, run the following shell script using the local shell as in execute_config_cmd

// Shell script (note: use return codes to clearly signal termination conditions):
// 1. if remote exists as dir:
//   a. if is a repo, finish (success)
//   b. else abort with "existing dir" complaint
// 2. else if remote exists as file, abort with "existing file" complaint
// 3. else:
//   a. if no template config, mkdir && cd && git init --bare
//   b. else cp -na --reflink=always to create
//   c. finish (success)

/// Create a new repository
/// Returns true if this was a virgin (newly initialized) repository that needs a checkout after the remote is added
pub fn create_remote(repo: &RepoSpecification, config: &Config, is_repo: bool) -> Result<bool> {
    println!("Creating new repository at \"{}\" with remote \"{}\"", repo.paths.local, repo.remoteURL);
    
    // Check required configuration
    let rpath_template = if config.rpath_template.is_empty() {
        return Err(anyhow!("RPATH_TEMPLATE not set in configuration"));
    } else {
        &config.rpath_template
    };

    let rlogin = if config.rlogin.is_empty() {
        return Err(anyhow!("RLOGIN not set in configuration"));
    } else {
        &config.rlogin
    };

    // Parse SSH host
    let ssh_host = if rlogin.is_empty() {
        "localhost"
    } else if let Some(host) = rlogin.strip_prefix("ssh://") {
        host
    } else {
        return Err(anyhow!("RLOGIN must be in format 'ssh://[user@]host' for SSH remote creation"));
    };

    // Construct remote path with .git extension
    let target_path = if !repo.paths.remote.ends_with(".git") {format!("{}.git", repo.paths.remote)} else {repo.paths.remote.to_string()};

    // Prompt for confirmation
    print!("About to create remote repo '{}'; are you sure? (y/n) ", target_path);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if !input.trim().eq_ignore_ascii_case("y") {
        println!("(aborted)");
        return Ok(false);
    }

    // Create remote repo based on template
    // Define unique exit codes
    const EXIT_NOT_REPO: i32 = 90;
    const EXIT_IS_FILE: i32 = 91;
    const EXIT_OTHER_FILETYPE: i32 = 92;
    
    // Script to check and create remote repository
    let script = format!(r##"#!/bin/bash
set -e
TARGET="{target_path}"
TEMPLATE="{rpath_template}"

if [ -d "$TARGET" ]; then
    # Check if it's a repo using proper git plumbing command
    if git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1; then
        # It's a git repo, success
        exit 0
    else
        # Directory exists but isn't a repo
        exit {EXIT_NOT_REPO}
    fi
elif [ -e "$TARGET" ]; then
    # Path exists but isn't a directory
    if [ -f "$TARGET" ]; then
        # Regular file
        exit {EXIT_IS_FILE}
    else
        # Other file type (symlink, device, socket, fifo, etc.)
        exit {EXIT_OTHER_FILETYPE}
    fi
else
    # Doesn't exist, create it
    if [ -z "$TEMPLATE" ]; then
        # No template config, create bare repo
        mkdir -p "$TARGET"
        cd "$TARGET"
        git init --bare -q
    else
        # Use template
        mkdir -p "$(dirname "$TARGET")"
        cp -na --reflink=auto "$TEMPLATE" "$TARGET"
    fi
    exit 0
fi
"##);

    let status = invoke::run_with_stdin_inherited(".", &["ssh", ssh_host, "bash -s"], script.as_bytes())
        .with_context(|| "Failed to spawn SSH command for repository creation")?;

	match status {
		0 => {
			println!("Repository created successfully");
		},
		EXIT_NOT_REPO => {
			return Err(anyhow!("Target directory exists but is not a git repository: {}", target_path));
		},
		EXIT_IS_FILE => {
			return Err(anyhow!("Target path exists as a regular file: {}", target_path));
		},
		EXIT_OTHER_FILETYPE => {
			return Err(anyhow!("Target path exists as a special file (device, pipe, socket, or symlink): {}", target_path));
		},
		_ => {
			return Err(anyhow!("Remote repository creation failed with status: {}", status));
		}
	}

    Ok(!is_repo)
}

/// Run a git command in the repository (public function called from main.rs)
pub fn run_git_command(local_path: &str, args_str: &str) -> Result<()> {
	let args = match shlex::split(args_str) {
		Some(v) => v,
		None => return Err(anyhow!("Invalid git args quoting")),
	};

	let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
	crate::invoke::run_git_cmd(local_path, &args, None)
}

/// Execute the CONFIG_CMD in the specified directory
pub fn execute_config_cmd(repo: &RepoSpecification, config: &Config) -> Result<()> {
    let config_cmd = &config.config_cmd;
    if config_cmd.is_empty() {
        return Ok(()); // No command to execute
    }

    // Use shell-escape crate to robustly escape the media_path argument for shell usage
    let inner_cmd = format!("{config_cmd} {}", shell_escape::unix::escape(Cow::Borrowed(&repo.paths.cfgParam)));

	// We cannot specify the shell's path (e.g. `/bin/bash`) because we might be running on Win32, even if our parent 
	// process is MinGW or Cygwin; we must rely on `sh` being on the path
	let status = invoke::run_in_dir_status(&repo.paths.local, &["sh", "-c", inner_cmd.as_str()])
		.with_context(|| format!("Failed to execute CONFIG_CMD: {}", inner_cmd))?;
	if status != 0 {
		return Err(anyhow!("Config command failed with exit code: {}", status));
	}
	
    Ok(())
}