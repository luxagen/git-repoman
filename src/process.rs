use std::process::{Command, Stdio};
use anyhow::{Context, Result, anyhow};

/// Execute a command synchronously and redirect all output to stderr.
/// This is equivalent to the Perl run_sync_redir function but with better error handling.
pub fn run_sync_redir(args: &[&str]) -> Result<i32> {
    // Early validation
    if args.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    // For debugging
    eprintln!("Executing: {:?}", args);
    
    let program = args[0];
    let arguments = &args[1..];
    
    // Build and execute the command
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::inherit())  // Use the current stdin
        .stdout(Stdio::inherit()) // Use the current stdout
        .stderr(Stdio::inherit()) // Use the current stderr
        .output()
        .with_context(|| format!("Failed to execute command: {:?}", args))?;
    
    // Get exit code, which is None if process was terminated by a signal
    let exit_code = output.status.code().unwrap_or(-1);
    
    // Only report non-zero exit codes
    if !output.status.success() {
        eprintln!("Command {:?} exited with code: {}", args, exit_code);
    }
    
    Ok(exit_code)
}

/// Run a command in a specific directory
pub fn run_in_dir(dir: &str, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    let program = args[0];
    let arguments = &args[1..];
    
    let output = Command::new(program)
        .args(arguments)
        .current_dir(dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("Failed to execute command in {}: {:?}", dir, args))?;
    
    let exit_code = output.status.code().unwrap_or(-1);
    
    // Only report non-zero exit codes
    if !output.status.success() {
        eprintln!("Command {:?} in {} exited with code: {}", args, dir, exit_code);
    }
    
    Ok(exit_code)
}

/// Execute a git command and return its output
pub fn git_command(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("Failed to execute git command: {:?}", args))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Git command failed: {}", stderr));
    }
    
    let stdout = String::from_utf8(output.stdout)
        .context("Non-UTF8 output from git command")?;
    
    Ok(stdout.trim().to_string())
}

/// Execute a command, capturing its output and checking for success
pub fn capture_command_output(args: &[&str]) -> Result<String> {
    if args.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    let program = args[0];
    let arguments = &args[1..];
    
    let output = Command::new(program)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("Failed to execute command: {:?}", args))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Command failed: {}", stderr));
    }
    
    let stdout = String::from_utf8(output.stdout)
        .context("Non-UTF8 output from command")?;
    
    Ok(stdout.trim().to_string())
}

/// Run a command in a specific directory, capturing output but not displaying it
/// Returns the exit code
pub fn run_command_silent(dir: &str, args: &[&str]) -> Result<i32> {
    // Early validation
    if args.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    let program = args[0];
    let arguments = &args[1..];
    
    // Build and execute the command
    let output = Command::new(program)
        .args(arguments)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("Failed to execute command: {:?}", args))?;
    
    // Get exit code, which is None if process was terminated by a signal
    let exit_code = output.status.code().unwrap_or(-1);
    
    Ok(exit_code)
}
