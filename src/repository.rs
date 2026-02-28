use std::io::Write;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;
use crate::process;

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

/// Helper for fetching from a remote
fn git_fetch(local_path: &str, remote: &str) -> Result<()> {
    run_git_command_with_warning(local_path, &["fetch", remote], "fetch")
}

/// Clone a repository without checking it out
pub fn clone_repo_no_checkout(local_path: &str, remote_url: &str) -> Result<()> {
    println!("Cloning repository \"{}\" into \"{}\"", remote_url, local_path);
    
    // Run git clone without a working directory
    // Pass the local_path as a Path to avoid shell escaping issues
    let status = Command::new("git")
        .arg("clone")
        .arg("--no-checkout")
        .arg(remote_url)
        .arg(Path::new(local_path))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit()) 
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute clone: {}", remote_url))?;
    
    if !status.success() {
        return Err(anyhow!("Git clone failed with exit code: {:?}", status));
    }
    
    Ok(())
}

/// Configure a repository using the provided command
pub fn configure_repo(local_path: &str, media_path: &str, config: &Config) -> Result<()> {
    execute_config_cmd(local_path, media_path, config)
}

/// Update the remote URL for a repository
pub fn set_remote(local_path: &str, remote_url: &str) -> Result<()> {
    // Try to update the remote first, suppressing output
    // We use run_git_cmd_silent to avoid printing errors if this fails
    let status = process::run_command_silent(local_path, &["git", "remote", "set-url", "origin", remote_url])?;
    
    // If remote update failed with exit code 2 (non-existent remote), try to add it
    // This matches the Perl version's check for 512 (which is 2 << 8 in Perl's exit code handling)
    if status == 2 {
        println!("Adding remote origin");
        run_git_cmd_internal(local_path, &["remote", "add", "-f", "origin", remote_url])?;
    } else if status != 0 {
        // Other non-zero exit codes are still errors
        return Err(anyhow!("Failed to set remote with exit code: {}", status));
    }
    
    Ok(())
}

/// Checkout the default branch after cloning
pub fn check_out(local_path: &str) -> Result<()> {
    println!("Checking out repository at \"{}\"", local_path);
    
    // Reset to get the working directory in sync with remote
    run_git_command_with_warning(local_path, &["reset", "--hard"], "reset")?;
    
    Ok(())
}

/// Add a git remote - used for new repositories
fn add_git_remote(local_path: &str, remote_url: &str) -> Result<()> {
    println!("Adding remote origin");
    run_git_cmd_internal(local_path, &["remote", "add", "-f", "origin", remote_url])?;
    
    Ok(())
}

/// Create a new repository 
pub fn create_new(local_path: &str, remote_rel_path: Option<&str>, config: &Config) -> Result<()> {
    let remote_rel_path_str = remote_rel_path.unwrap_or("");
    println!("Creating new repository at \"{}\" with remote \"{}\"", local_path, remote_rel_path_str);
    
    // Check required configuration
    let rpath_template = config.rpath_template
        .as_ref()
        .ok_or_else(|| anyhow!("RPATH_TEMPLATE not set in configuration"))?;
    
    let rlogin = config.rlogin
        .as_ref()
        .ok_or_else(|| anyhow!("RLOGIN not set in configuration"))?;
    
    let rpath_base = config.rpath_base
        .as_ref()
        .ok_or_else(|| anyhow!("RPATH_BASE not set in configuration"))?;
    
    // Parse SSH host
    let (ssh_host, effective_login) = if rlogin.is_empty() {
        ("localhost", "ssh://localhost".to_string())
    } else if let Some(host) = rlogin.strip_prefix("ssh://") {
        (host, rlogin.clone())
    } else {
        return Err(anyhow!("RLOGIN must be in format 'ssh://[user@]host' for SSH remote creation"));
    };
    
    // Check if current directory is already a git repo
    let is_virgin_repo = !Path::new(local_path).join(".git").exists();
    
    // Construct remote path with .git extension
    let mut grm_rpath = format!("{}/{}", rpath_base, remote_rel_path_str);
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
        return Ok(());
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
    
    // Use the helper function to generate the media path
    let media_path = crate::get_media_repo_path(config, remote_rel_path);
    
    // Configure the repository
    execute_config_cmd(local_path, &media_path, config)?;
    
    // Git remote URL
    let git_remote = format!("{}{}", effective_login, grm_rpath);
    
    // Add the remote
    add_git_remote(local_path, &git_remote)?;
    
    // Checkout master if this was a new repository
    if is_virgin_repo {
        run_git_cmd_internal(local_path, &["checkout", "master"])?;
    }
    
    println!("Repository created successfully");
    Ok(())
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
fn execute_config_cmd(local_path: &str, media_path: &str, config: &Config) -> Result<()> {
    let config_cmd = match &config.config_cmd {
        Some(cmd) if !cmd.is_empty() => cmd,
        _ => return Ok(()), // No command to execute
    };
    
    // Append the media_path to the config command as a command-line argument
    let full_command = format!("{} {}", config_cmd, media_path);
    
    // Try to detect the shell environment
    let shell_cmd = detect_shell_command(&full_command)?;
    
    // Execute through the detected shell
    let status = Command::new(shell_cmd.executable)
        .args(&shell_cmd.args)
        .current_dir(local_path)
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
