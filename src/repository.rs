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
	let result = invoke::run_cmd(local_path, &["git", "rev-parse", "--git-dir"], None, invoke::OutputMode::Capture, invoke::OnExitCode::None)
		.with_context(|| format!("Failed to check if {} is a git repo root", local_path))?;
    
    // If command succeeds, it's a git repository
	if result.exit_code != 0 {
        return Ok(false);
    }
    
    // Check if we're at the root (.git dir is directly in this directory)
    // If output is just ".git", we're at the repository root
	let git_dir = String::from_utf8_lossy(&result.stdout).trim().to_string();
    Ok(git_dir == ".git")
}

/// Initialize a git repository
pub fn init_new(local_path: &str) -> Result<()> {
    // Initialize git repository
    invoke::run_cmd(local_path, &["git", "init", "-q"], None, invoke::OutputMode::Inherit, invoke::OnExitCode::Error)?;
    Ok(())
}

/// Clone a repository without checking it out
pub fn clone_repo_no_checkout(repo: &RepoSpecification) -> Result<()> {
	invoke::run_cmd(".", &["git", "clone", "--no-checkout", &repo.remoteURL, &repo.paths.local], None, invoke::OutputMode::Inherit, invoke::OnExitCode::Error)?;

    Ok(())
}

/// Configure a repository using the provided command

pub fn configure_repo(repo: &RepoSpecification, config: &Config) -> Result<()> {
    let config_cmd = &config.config_cmd;
    if config_cmd.is_empty() {
        return Ok(()); // No command to execute
    }

    // Use shell-escape crate to robustly escape the media_path argument for shell usage
    let inner_cmd = format!("{config_cmd} {}", shell_escape::unix::escape(Cow::Borrowed(&repo.paths.cfgParam)));

	// We cannot specify the shell's path (e.g. `/bin/bash`) because we might be running on Win32, even if our parent 
	// process is MinGW or Cygwin; we must rely on `sh` being on the path
	invoke::run_cmd(&repo.paths.local, &["sh", "-c", inner_cmd.as_str()], None, invoke::OutputMode::Inherit, invoke::OnExitCode::Error)?;	
    Ok(())
}

/// Update the remote URL for a repository
pub fn set_remote(repo: &RepoSpecification) -> Result<()> {
    if 2 == invoke::run_cmd(&repo.paths.local, &["git", "remote", "set-url", "origin", &repo.remoteURL], None, invoke::OutputMode::Silent, invoke::OnExitCode::Error)?.exit_code {
		crate::invoke::run_cmd(&repo.paths.local, &["git", "remote", "add", "-f", "origin", &repo.remoteURL], None, invoke::OutputMode::Inherit, invoke::OnExitCode::Error)?;
    }
    Ok(())
}

/// Checkout the default branch after cloning
pub fn check_out(local_path: &str) -> Result<()> {
    println!("Checking out repository at \"{}\"", local_path);
    
    // Reset to get the working directory in sync with remote
	crate::invoke::run_cmd(local_path, &["git", "checkout"], None, invoke::OutputMode::Inherit, invoke::OnExitCode::Warning)?;
    Ok(())
}

const EXIT_REPO_EXISTS: i32 = 89;
const EXIT_NOT_REPO: i32 = 90;
const EXIT_IS_FILE: i32 = 91;
const EXIT_OTHER_FILETYPE: i32 = 92;
const EXIT_MISSING: i32 = 93;
const EXIT_REPO_CREATED: i32 = 94;

enum RemoteStatus {
    Created,
    Exists,
    Missing,
}

fn remote_status_from_exit_code(exit_code: i32, stderr: &[u8], target_path: &str) -> Result<RemoteStatus> {
    Ok(match exit_code {
        EXIT_REPO_CREATED => RemoteStatus::Created,
        EXIT_REPO_EXISTS => RemoteStatus::Exists,
        EXIT_MISSING => RemoteStatus::Missing,
        EXIT_NOT_REPO => return Err(anyhow!("Target directory exists but is not a git repository: {}", target_path)),
        EXIT_IS_FILE => return Err(anyhow!("Target path exists as a regular file: {}", target_path)),
        EXIT_OTHER_FILETYPE => return Err(anyhow!("Target path exists as a special file (device, pipe, socket, or symlink): {}", target_path)),
        code => {
            return Err(anyhow!(
                "Remote repository status check failed with status {} for {}: {}",
                code,
                target_path,
                String::from_utf8_lossy(stderr).trim()
            ));
        }
    })
}

fn probe_remote_repo(ssh_host: &str, target_path: &str) -> Result<RemoteStatus> {
    let script = format!(r##"#!/bin/bash
TARGET="{target_path}"

if [ -d "$TARGET" ]; then
    if git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1; then
        exit {EXIT_REPO_EXISTS}
    fi
    exit {EXIT_NOT_REPO}
elif [ -e "$TARGET" ]; then
    if [ -f "$TARGET" ]; then
        exit {EXIT_IS_FILE}
    else
        exit {EXIT_OTHER_FILETYPE}
    fi
else
    exit {EXIT_MISSING}
fi
"##);

    let status = invoke::run_cmd(
        ".",
        &["ssh", ssh_host, "bash -s"],
        Some(script.as_bytes()),
        invoke::OutputMode::Capture,
        invoke::OnExitCode::None,
    )
    .with_context(|| "Failed to probe remote repository")?;

    remote_status_from_exit_code(status.exit_code, &status.stderr, target_path)
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
    // Check required configuration
    if config.rpath_template.is_empty() {
        return Err(anyhow!("RPATH_TEMPLATE not set in configuration"));
    }

    // Compound the template path with RPATH_BASE (absolute templates bypass the base)
    let template_path = crate::config::cat_paths(&config.rpath_base, &config.rpath_template);

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

    match probe_remote_repo(ssh_host, &target_path)? {
        RemoteStatus::Exists => {
            return Ok(!is_repo);
        }
        RemoteStatus::Missing => {}
        RemoteStatus::Created => {
            return Err(anyhow!("Remote repository probe unexpectedly reported creation: {}", target_path));
        }
    }

    println!("Creating new repository at \"{}\" with remote \"{}\"", repo.paths.local, repo.remoteURL);

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
    // Script to check and create remote repository
    let script = format!(r##"#!/bin/bash
set -e
TARGET="{target_path}"
TEMPLATE="{template_path}"

if [ -d "$TARGET" ]; then
    # Check if it's a repo using proper git plumbing command
    if git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1; then
        # It's a git repo, success
        exit {EXIT_REPO_EXISTS}
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
        # Use template via bare clone (uses hardlinks on same filesystem)
        mkdir -p "$(dirname "$TARGET")"
        git clone --bare -q "$TEMPLATE" "$TARGET"
    fi
    exit {EXIT_REPO_CREATED}
fi
"##);

    let status = invoke::run_cmd(".", &["ssh", ssh_host, "bash -s"], Some(script.as_bytes()), invoke::OutputMode::Capture, invoke::OnExitCode::None)
        .with_context(|| "Failed to spawn SSH command for repository creation")?;

    match remote_status_from_exit_code(status.exit_code, &status.stderr, &target_path)? {
        RemoteStatus::Created => {
            println!("Repository created successfully");
        }
        RemoteStatus::Exists => (),
        RemoteStatus::Missing => {
            return Err(anyhow!("Remote repository disappeared before creation: {}", target_path));
        }
    }

    Ok(!is_repo)
}