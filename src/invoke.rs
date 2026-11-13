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

pub enum OnExitCode {
	None,
	Warning,
	Error,
}

pub struct CmdResult {
	pub exit_code: i32,
	pub stdout: Option<Vec<u8>>,
	pub stderr: Option<Vec<u8>>,
}

pub trait CommandRunner: Send + Sync {
	fn run_cmd(&self, dir: &str, cmd: &[&str], stdin_bytes: Option<&[u8]>, output_mode: OutputMode) -> Result<CmdResult>;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
	fn run_cmd(&self, dir: &str, cmd: &[&str], stdin_bytes: Option<&[u8]>, output_mode: OutputMode) -> Result<CmdResult> {
		if cmd.is_empty() {
			return Err(anyhow!("No command specified"));
		}

		let program = cmd[0];
		let arguments = &cmd[1..];

		let stdin_mode = if stdin_bytes.is_some() {
			Stdio::piped()
		} else {
			match output_mode {
				OutputMode::Inherit => Stdio::inherit(),
				_ => Stdio::null(),
			}
		};

		let (stdout_mode, stderr_mode) = match output_mode {
			OutputMode::Inherit => (Stdio::inherit(), Stdio::inherit()),
			OutputMode::Silent => (Stdio::null(), Stdio::null()),
			OutputMode::Capture => (Stdio::piped(), Stdio::piped()),
		};

		let mut child = Command::new(program)
			.args(arguments)
			.current_dir(dir)
			.stdin(stdin_mode)
			.stdout(stdout_mode)
			.stderr(stderr_mode)
			.spawn()
			.with_context(|| format!("Failed to spawn command in {}: {:?}", dir, cmd))?;

		if let Some(stdin_bytes) = stdin_bytes {
			if let Some(mut stdin) = child.stdin.take() {
				stdin.write_all(stdin_bytes)?;
			}
		}

		let output = child.wait_with_output()?;

		Ok(CmdResult {
			exit_code: output.status.code().unwrap_or(-1),
			stdout: if output.stdout.is_empty() { None } else { Some(output.stdout) },
			stderr: if output.stderr.is_empty() { None } else { Some(output.stderr) },
		})
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

#[allow(dead_code)]
pub fn set_command_runner_for_test(runner: Arc<dyn CommandRunner>) -> CommandRunnerGuard {
	let mut slot = COMMAND_RUNNER.write().expect("COMMAND_RUNNER poisoned");
	let previous = slot.clone();
	*slot = runner;
	CommandRunnerGuard { previous }
}

pub struct CapturedOutput {
	pub exit_code: i32,
	pub stdout: Vec<u8>,
    #[allow(dead_code)]
	pub stderr: Vec<u8>,
}

pub fn run_cmd(dir: &str, cmd: &[&str], stdin_bytes: Option<&[u8]>, output_mode: OutputMode, on_exit_code: OnExitCode) -> Result<CapturedOutput> {
	let result = COMMAND_RUNNER.read().expect("COMMAND_RUNNER poisoned").run_cmd(dir, cmd, stdin_bytes, output_mode)
		.with_context(|| format!("Failed to execute command in {}: {:?}", dir, cmd))?;

	if result.exit_code != 0 {
		match on_exit_code {
			OnExitCode::None => (),
			OnExitCode::Warning => println!("Warning: `{}` failed with exit code {}", cmd.join(" "), result.exit_code),
			OnExitCode::Error => return Err(anyhow!("Error: '{}' failed with exit code: {}", cmd.join(" "), result.exit_code)),
		}
	}

	Ok(CapturedOutput {
		exit_code: result.exit_code,
		stdout: result.stdout.unwrap_or_default(),
		stderr: result.stderr.unwrap_or_default(),
	})
}