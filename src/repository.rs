// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::io::Write;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;
use crate::process;

// Shared repository specification struct
#[derive(Debug, Clone)]
pub struct RepoTriple<'a> {
    pub remote_path: &'a str,
    pub remote_url: &'a str,
    pub local_path: &'a str,
    pub media_path: &'a str, // TODO REMOVE
}

impl<'a> RepoTriple<'a> {
    /// Create a new RepoTriple with remote_path, local_path, and media_path; remote_url is initialized to empty
    pub fn new(remote_path: &'a str, local_path: &'a str, media_path: &'a str, remote_url: &'a str) -> Self {
        use crate::get_remote_url;

        Self {
            remote_path,
            remote_url,
            local_path,
            media_path,
        }
    }
}


/// Check if directory is a Git repository root
pub fn is_dir_repo_root(local_path: &str) -> Result<bool> {
    // Use git rev-parse --git-dir which is more efficient for checking repository existence
    // This is a plumbing command that directly checks for the .git directory
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(local_path)
        .output()
        .with_context(|| format!("Failed to check if {} is a git repo root", local_path))?;
    
    // If command succeeds, it's a git repository
    if !output.status.success() {
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
    let status = process::run_in_dir(local_path, &["git", "init", "-q"])?;
    if status != 0 {
        return Err(anyhow!("Git init failed with exit code: {}", status));
    }
    Ok(())
}

/// Run a git command and expect success (internal version)
fn run_git_cmd_internal(local_path: &str, args: &[&str]) -> Result<()> {
    let mut cmd_args = vec!["git"];
    cmd_args.extend(args);
    
    let status = process::run_in_dir(local_path, &cmd_args)?;
    
    if status != 0 {
        return Err(anyhow!("Git command '{}' failed with exit code: {}", 
                           args.join(" "), status));
    }
    
    Ok(())
}

/// Run a git command and print a warning on failure instead of returning an error
fn run_git_command_with_warning(local_path: &str, args: &[&str], operation: &str) -> Result<()> {
    let mut cmd_args = vec!["git"];
    cmd_args.extend(args);
    
    let status = process::run_in_dir(local_path, &cmd_args)?;
    if status != 0 {
        println!("Warning: git {} failed with code {}", operation, status);
    }
    
    Ok(())
}

/// Clone a repository without checking it out
pub fn clone_repo_no_checkout(repo: &RepoTriple) -> Result<()> {
    println!("Cloning repository \"{}\" into \"{}\"", repo.remote_url, repo.local_path);
    let status = Command::new("git")
        .arg("clone")
        .arg("--no-checkout")
        .arg(repo.remote_url)
        .arg(Path::new(repo.local_path))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit()) 
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute clone: {}", repo.remote_url))?;
    if !status.success() {
        return Err(anyhow!("Git clone failed with exit code: {:?}", status));
    }
    Ok(())
}

/// Configure a repository using the provided command

pub fn configure_repo(repo: &RepoTriple, config: &Config) -> Result<()> {
    execute_config_cmd(repo, config)
}

// TODO: figure out whether to always fetch

/// Update the remote URL for a repository
pub fn set_remote(repo: &RepoTriple) -> Result<()> {
    let status = process::run_command_silent(repo.local_path, &["git", "remote", "set-url", "origin", repo.remote_url])?;
    if status == 2 {
        println!("Adding remote origin");
        run_git_cmd_internal(repo.local_path, &["remote", "add", "-f", "origin", repo.remote_url])?;
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
    run_git_command_with_warning(local_path, &["checkout"], "checkout")?;
    
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
pub fn create_remote(repo: &RepoTriple, config: &Config, is_repo: bool) -> Result<bool> {
    println!("Creating new repository at \"{}\" with remote \"{}\"", repo.local_path, repo.remote_url);
    
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
    let target_path = if !repo.remote_path.ends_with(".git") {format!("{}.git", repo.remote_path)} else {repo.remote_path.to_string()};

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

    let mut child = Command::new("ssh")
        .args([ssh_host, "bash -s"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "Failed to spawn SSH command for repository creation")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(script.as_bytes())?;
    }

    let status = child.wait()?;
    
    // Handle exit codes
    match status.code() {
        Some(0) => {
            // Success
            println!("Repository created successfully");
        },
        Some(EXIT_NOT_REPO) => {
            return Err(anyhow!("Target directory exists but is not a git repository: {}", target_path));
        },
        Some(EXIT_IS_FILE) => {
            return Err(anyhow!("Target path exists as a regular file: {}", target_path));
        },
        Some(EXIT_OTHER_FILETYPE) => {
            return Err(anyhow!("Target path exists as a special file (device, pipe, socket, or symlink): {}", target_path));
        },
        _ => {
            return Err(anyhow!("Remote repository creation failed with status: {:?}", status));
        }
    }

    Ok(!is_repo)
}

/// Run a git command in the repository (public function called from main.rs)
pub fn run_git_command(local_path: &str, args_str: &str) -> Result<()> {
    // Split the arguments string into individual arguments
    let args: Vec<&str> = args_str.split_whitespace().collect();
    
    // Use our standardized helper internally
    run_git_cmd_internal(local_path, &args)
}

/// Shell command structure
struct ShellCommand {
    executable: String,
    args: Vec<String>,
}

/// Detect what shell to use based on environment
fn detect_shell_command(cmd: &str) -> Result<ShellCommand> {
    // First, try to use the SHELL environment variable
    if let Ok(shell_path) = std::env::var("SHELL") {
        // User has a SHELL variable defined, use it directly
        return Ok(ShellCommand {
            executable: shell_path,
            args: vec!["-c".to_string(), cmd.to_string()],
        });
    }
    
    // Default for all platforms if SHELL is not set
    Ok(ShellCommand {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
    })
}

/// Execute a CONFIG_CMD in the specified directory
pub fn execute_config_cmd(repo: &RepoTriple, config: &Config) -> Result<()> {
    let config_cmd = &config.config_cmd;
    if config_cmd.is_empty() {
        return Ok(()); // No command to execute
    }

    // Use shell-escape crate to robustly escape the media_path argument for shell usage
    use shell_escape::unix::escape;
    let escaped_media_path = escape(repo.media_path.into());
    let full_command = format!("{} {}", config_cmd, escaped_media_path);
    
    // Try to detect the shell environment
    let shell_cmd = detect_shell_command(&full_command)?;
    
    // Execute through the detected shell
    let status = Command::new(shell_cmd.executable)
        .args(&shell_cmd.args)
        .current_dir(repo.local_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute CONFIG_CMD: {}", full_command))?;
    
    if !status.success() {
        return Err(anyhow!("Config command failed with exit code: {:?}", status));
    }
    
    Ok(())
}