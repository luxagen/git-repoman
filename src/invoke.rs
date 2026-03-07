// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::process::{Command, Stdio};
use anyhow::{Context, Result, anyhow};

pub struct CapturedOutput {
	pub exit_code: i32,
	pub stdout: Vec<u8>,
	pub stderr: Vec<u8>,
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

pub fn run_in_dir_capture(dir: &str, args: &[&str]) -> Result<CapturedOutput> {
	if args.is_empty() {
		return Err(anyhow!("No command specified"));
	}

	let program = args[0];
	let arguments = &args[1..];

	let output = Command::new(program)
		.args(arguments)
		.current_dir(dir)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, args))?;

	let exit_code = output.status.code().unwrap_or(-1);
	Ok(CapturedOutput {
		exit_code,
		stdout: output.stdout,
		stderr: output.stderr,
	})
}

pub fn run_git_capture(local_path: &str, args: &[&str]) -> Result<CapturedOutput> {
	let mut cmd_args = vec!["git"];
	cmd_args.extend(args);
	run_in_dir_capture(local_path, &cmd_args)
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

pub fn run_git_cmd(local_path: &str, args: &[&str], operation_for_warning: Option<&str>) -> Result<()> {
	let mut cmd_args = vec!["git"];
	cmd_args.extend(args);

	let status = run_in_dir(local_path, &cmd_args)?;
	if status != 0 {
		if let Some(operation) = operation_for_warning {
			println!("Warning: git {} failed with code {}", operation, status);
			return Ok(());
		}

		return Err(anyhow!(
			"Git command '{}' failed with exit code: {}",
			args.join(" "),
			status
		));
	}

	Ok(())
}