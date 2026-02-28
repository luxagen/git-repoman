use std::io::Write;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;
use crate::process;

// Shared repository specification struct
#[derive(Debug, Clone)]
pub struct RepoTriple<'a> {
    pub remote: &'a str,
    pub local: &'a str,
    pub media: &'a str,
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
fn init_git_repository(local_path: &str) -> Result<()> {
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
    println!("Cloning repository \"{}\" into \"{}\"", repo.remote, repo.local);
    let status = Command::new("git")
        .arg("clone")
        .arg("--no-checkout")
        .arg(repo.remote)
        .arg(Path::new(repo.local))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit()) 
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute clone: {}", repo.remote))?;
    if !status.success() {
        return Err(anyhow!("Git clone failed with exit code: {:?}", status));
    }
    Ok(())
}

/// Configure a repository using the provided command

pub fn configure_repo(repo: &RepoTriple, config: &Config) -> Result<()> {
    execute_config_cmd(repo, config)
}

/// Update the remote URL for a repository
pub fn set_remote(repo: &RepoTriple) -> Result<()> {
    let status = process::run_command_silent(repo.local, &["git", "remote", "set-url", "origin", repo.remote])?;
    if status == 2 {
        println!("Adding remote origin");
        run_git_cmd_internal(repo.local, &["remote", "add", "-f", "origin", repo.remote])?;
    } else if status != 0 {
        return Err(anyhow!("Failed to set remote with exit code: {}", status));
    }
    Ok(())
}

/// Checkout the default branch after cloning
pub fn check_out(local_path: &str) -> Result<()> {
    println!("Checking out repository at \"{}\"", local_path);
    
    // Reset to get the working directory in sync with remote
    run_git_command_with_warning(local_path, &["checkout"], "checkout")?;
    
    Ok(())
}

/// Create a new repository
/// Returns true if this was a virgin (newly initialized) repository that needs a checkout after the remote is added
pub fn create_new(repo: &RepoTriple, config: &Config) -> Result<bool> {
    println!("Creating new repository at \"{}\" with remote \"{}\"", repo.local, repo.remote);
    let local_path = repo.local;
    let remote_rel_path = repo.remote;
    
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

    let rpath_base = if config.rpath_base.is_empty() {
        return Err(anyhow!("RPATH_BASE not set in configuration"));
    } else {
        &config.rpath_base
    };
    
    // Parse SSH host
    let ssh_host = if rlogin.is_empty() {
        "localhost"
    } else if let Some(host) = rlogin.strip_prefix("ssh://") {
        host
    } else {
        return Err(anyhow!("RLOGIN must be in format 'ssh://[user@]host' for SSH remote creation"));
    };
    
    // Check if current directory is already a git repo
    let is_virgin_repo = !Path::new(local_path).join(".git").exists();
    
    // Construct remote path with .git extension
    let mut grm_rpath = format!("{}/{}", rpath_base, remote_rel_path);
    if !grm_rpath.ends_with(".git") {
        grm_rpath.push_str(".git");
    }
    
    // Prompt for confirmation
    print!("About to create remote repo '{}'; are you sure? (y/n) ", grm_rpath);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("(aborted)");
        return Ok(false);
    }
    
    // Create remote repo based on template
    let ssh_cmd = format!("xargs -0 -n 1 -- cp -na --reflink=auto '{}/{}'", rpath_base, rpath_template);
    
    let mut child = Command::new("ssh")
        .args([ssh_host, &ssh_cmd])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "Failed to spawn SSH command for repository creation")?;
    
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(format!("{}\0", grm_rpath).as_bytes())?;
    }
    
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("Remote repository creation failed with status: {:?}", status));
    }
    
    // Initialize git repository
    init_git_repository(local_path)?;
    
    println!("Repository created successfully");
    Ok(is_virgin_repo)
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
    // Append the media_path to the config command as a command-line argument
    let full_command = format!("{} {}", config_cmd, repo.media);
    
    // Try to detect the shell environment
    let shell_cmd = detect_shell_command(&full_command)?;
    
    // Execute through the detected shell
    let status = Command::new(shell_cmd.executable)
        .args(&shell_cmd.args)
        .current_dir(repo.local)
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