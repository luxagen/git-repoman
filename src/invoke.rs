// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::io::Write;
use std::process::{Command, Output, Stdio};
use anyhow::{Context, Result, anyhow};

pub struct CapturedOutput {
	pub exit_code: i32,
	pub stdout: Vec<u8>,
    #[allow(dead_code)]
	pub stderr: Vec<u8>,
}

/// Run a command in a specific directory
pub fn run_in_dir(dir: &str, cmd: &[&str]) -> Result<i32> {
    if cmd.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    let program = cmd[0];
    let arguments = &cmd[1..];
    
    let output = Command::new(program)
        .args(arguments)
        .current_dir(dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd))?;
    
    let exit_code = output.status.code().unwrap_or(-1);
    
    // Only report non-zero exit codes
    if !output.status.success() {
        eprintln!("Command {:?} in {} exited with code: {}", cmd, dir, exit_code);
    }
    
    Ok(exit_code)
}

pub fn run_in_dir_status(dir: &str, program: &str, args: &[&str]) -> Result<i32> {
	let status = Command::new(program)
		.args(args)
		.current_dir(dir)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, args))?;

	Ok(status.code().unwrap_or(-1))
}

pub fn run_with_stdin_inherited(dir: &str, program: &str, args: &[&str], stdin_bytes: &[u8]) -> Result<i32> {
	let mut child = Command::new(program)
		.args(args)
		.current_dir(dir)
		.stdin(Stdio::piped())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.spawn()
		.with_context(|| format!("Failed to spawn command in {}: {:?}", dir, args))?;

	if let Some(mut stdin) = child.stdin.take() {
		stdin.write_all(stdin_bytes)?;
	}

	let status = child.wait()?;
	Ok(status.code().unwrap_or(-1))
}

fn run_command_output(dir: &str, program: &str, arguments: &[&str], stdin: Stdio, stdout: Stdio, stderr: Stdio) -> Result<Output> {
	Command::new(program)
		.args(arguments)
		.current_dir(dir)
		.stdin(stdin)
		.stdout(stdout)
		.stderr(stderr)
		.output()
		.map_err(|err| err.into())
}

pub fn run_in_dir_capture(dir: &str, cmd: &[&str]) -> Result<CapturedOutput> {
	if cmd.is_empty() {
		return Err(anyhow!("No command specified"));
	}

	let program = cmd[0];
	let arguments = &cmd[1..];

	let output = run_command_output(dir, program, arguments, Stdio::null(), Stdio::piped(), Stdio::piped())
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd))?;

	let exit_code = output.status.code().unwrap_or(-1);
	Ok(CapturedOutput {
		exit_code,
		stdout: output.stdout,
		stderr: output.stderr,
	})
}

/// Run a command in a specific directory, capturing output but not displaying it
/// Returns the exit code
pub fn run_command_silent(dir: &str, cmd: &[&str]) -> Result<i32> {
    // Early validation
    if cmd.is_empty() {
        return Err(anyhow!("No command specified"));
    }
    
    let program = cmd[0];
    let arguments = &cmd[1..];
    
    let output = run_command_output(dir, program, arguments, Stdio::null(), Stdio::null(), Stdio::null())
        .with_context(|| format!("Failed to execute command: {:?}", cmd))?;
    
    // Get exit code, which is None if process was terminated by a signal
    let exit_code = output.status.code().unwrap_or(-1);
    
    Ok(exit_code)
}

pub fn run_git_status(dir: &str, args: &[&str]) -> Result<i32> {
	run_in_dir_status(dir, "git", args)
}

pub fn run_git_capture(local_path: &str, args: &[&str]) -> Result<CapturedOutput> {
	let mut cmd_args = vec!["git"];
	cmd_args.extend(args);
	run_in_dir_capture(local_path, &cmd_args)
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