use std::process::{Command, Stdio};
use anyhow::{Context, Result, anyhow};

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
