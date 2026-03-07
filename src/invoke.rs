// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, RwLock};
use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;

pub enum OutputMode {
	Inherit,
	Capture,
	Silent,
}

pub struct CmdResult {
	pub exit_code: i32,
	pub stdout: Option<Vec<u8>>,
	pub stderr: Option<Vec<u8>>,
}

pub trait CommandRunner: Send + Sync {
	fn run_cmd(&self, dir: &str, cmd: &[&str], mode: OutputMode) -> Result<CmdResult>;
	fn run_with_stdin_inherited(&self, dir: &str, cmd: &[&str], stdin_bytes: &[u8]) -> Result<i32>;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
	fn run_cmd(&self, dir: &str, cmd: &[&str], mode: OutputMode) -> Result<CmdResult> {
		if cmd.is_empty() {
			return Err(anyhow!("No command specified"));
		}

		let program = cmd[0];
		let arguments = &cmd[1..];

		match mode {
			OutputMode::Inherit => {
				let status = Command::new(program)
					.args(arguments)
					.current_dir(dir)
					.stdin(Stdio::inherit())
					.stdout(Stdio::inherit())
					.stderr(Stdio::inherit())
					.status()?;

				Ok(CmdResult {
					exit_code: status.code().unwrap_or(-1),
					stdout: None,
					stderr: None,
				})
			},
			OutputMode::Capture => {
				let output = Command::new(program)
					.args(arguments)
					.current_dir(dir)
					.stdin(Stdio::null())
					.stdout(Stdio::piped())
					.stderr(Stdio::piped())
					.output()?;

				Ok(CmdResult {
					exit_code: output.status.code().unwrap_or(-1),
					stdout: Some(output.stdout),
					stderr: Some(output.stderr),
				})
			},
			OutputMode::Silent => {
				let output = Command::new(program)
					.args(arguments)
					.current_dir(dir)
					.stdin(Stdio::null())
					.stdout(Stdio::null())
					.stderr(Stdio::null())
					.output()?;

				Ok(CmdResult {
					exit_code: output.status.code().unwrap_or(-1),
					stdout: None,
					stderr: None,
				})
			},
		}
	}

	fn run_with_stdin_inherited(&self, dir: &str, cmd: &[&str], stdin_bytes: &[u8]) -> Result<i32> {
		if cmd.is_empty() {
			return Err(anyhow!("No command specified"));
		}

		let program = cmd[0];
		let arguments = &cmd[1..];

		let mut child = Command::new(program)
			.args(arguments)
			.current_dir(dir)
			.stdin(Stdio::piped())
			.stdout(Stdio::inherit())
			.stderr(Stdio::inherit())
			.spawn()
			.with_context(|| format!("Failed to spawn command in {}: {:?}", dir, cmd))?;

		if let Some(mut stdin) = child.stdin.take() {
			stdin.write_all(stdin_bytes)?;
		}

		let status = child.wait()?;
		Ok(status.code().unwrap_or(-1))
	}
}

static COMMAND_RUNNER: Lazy<RwLock<Arc<dyn CommandRunner>>> = Lazy::new(|| RwLock::new(Arc::new(RealCommandRunner)));

pub struct CommandRunnerGuard {
	previous: Arc<dyn CommandRunner>,
}

impl Drop for CommandRunnerGuard {
	fn drop(&mut self) {
		let mut runner = COMMAND_RUNNER.write().expect("COMMAND_RUNNER poisoned");
		*runner = self.previous.clone();
	}
}

pub fn set_command_runner_for_test(runner: Arc<dyn CommandRunner>) -> CommandRunnerGuard {
	let mut slot = COMMAND_RUNNER.write().expect("COMMAND_RUNNER poisoned");
	let previous = slot.clone();
	*slot = runner;
	CommandRunnerGuard { previous }
}

pub fn run_cmd(dir: &str, cmd: &[&str], mode: OutputMode) -> Result<CmdResult> {
	let runner = COMMAND_RUNNER.read().expect("COMMAND_RUNNER poisoned");
	runner.run_cmd(dir, cmd, mode)
}

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

pub fn run_in_dir_status(dir: &str, cmd: &[&str]) -> Result<i32> {
	let result = run_cmd(dir, cmd, OutputMode::Inherit)
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd))?;
	Ok(result.exit_code)
}


pub fn run_with_stdin_inherited(dir: &str, cmd: &[&str], stdin_bytes: &[u8]) -> Result<i32> {
	let runner = COMMAND_RUNNER.read().expect("COMMAND_RUNNER poisoned");
	runner.run_with_stdin_inherited(dir, cmd, stdin_bytes)
}

pub fn run_in_dir_capture(dir: &str, cmd: &[&str]) -> Result<CapturedOutput> {
	let result = run_cmd(dir, cmd, OutputMode::Capture)
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd))?;
	Ok(CapturedOutput {
		exit_code: result.exit_code,
		stdout: result.stdout.unwrap_or_default(),
		stderr: result.stderr.unwrap_or_default(),
	})
}

/// Run a command in a specific directory, capturing output but not displaying it
/// Returns the exit code
pub fn run_command_silent(dir: &str, cmd: &[&str]) -> Result<i32> {
	// Early validation
	if cmd.is_empty() {
		return Err(anyhow!("No command specified"));
	}

	let output = run_cmd(dir, cmd, OutputMode::Silent)
		.with_context(|| format!("Failed to execute command: {:?}", cmd))?;

	Ok(output.exit_code)
}

pub fn run_git_status(dir: &str, args: &[&str]) -> Result<i32> {
	let mut cmd_args = vec!["git"];
	cmd_args.extend(args);
	let output = run_cmd(dir, &cmd_args, OutputMode::Inherit)
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd_args))?;
	Ok(output.exit_code)
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