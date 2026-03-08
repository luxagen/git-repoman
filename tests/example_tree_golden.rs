use std::fs;
use std::path::PathBuf;

use rstest::rstest;

fn first_diff_line(expected: &str, actual: &str) -> Option<(usize, String, String)> {
	for (i, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
		if e != a {
			return Some((i + 1, e.to_string(), a.to_string()));
		}
	}
	let expected_lines = expected.lines().count();
	let actual_lines = actual.lines().count();
	if expected_lines != actual_lines {
		return Some((expected_lines.min(actual_lines) + 1, "<EOF>".to_string(), "<EOF>".to_string()));
	}
	None
}

fn normalize_newlines(bytes: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'\r' {
			if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
				out.push(b'\n');
				i += 2;
				continue;
			}
			out.push(b'\n');
			i += 1;
			continue;
		}
		out.push(bytes[i]);
		i += 1;
	}
	out
}

fn repo_root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[rstest]
#[case::list_lrel("test-fixtures/listing","list-lrel")]
#[case::list_rrel("test-fixtures/listing","list-rrel")]
#[case::list_rurl("test-fixtures/listing","list-rurl")]
#[case::set_remote("test-fixtures/listing","set-remote")]
fn example_tree_list_outputs_match_golden_files(#[case] fixture_dir: &str,#[case] op: &str) {
	let golden_name = format!("{}.txt", op);
	let fixture_dir = repo_root().join(fixture_dir);
	let golden_path = fixture_dir.join(golden_name);
	let expected = fs::read(&golden_path)
		.unwrap_or_else(|e| panic!("failed to read golden file {}: {}", golden_path.display(), e));

	let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("grm");
	cmd.current_dir(&fixture_dir);
	cmd.arg(op);

	let output = cmd.output().unwrap_or_else(|e| panic!("failed to run grm {}: {}", op, e));
	assert!(output.status.success(), "grm {} exited with {:?}", op, output.status.code());
	assert!(output.stderr.is_empty(), "grm {} wrote to stderr: {}", op, String::from_utf8_lossy(&output.stderr));

	let actual_norm = normalize_newlines(&output.stdout);
	let expected_norm = normalize_newlines(&expected);

	let actual_text = String::from_utf8_lossy(&actual_norm);
	let expected_text = String::from_utf8_lossy(&expected_norm);

	if actual_text != expected_text {
		let mut detail = String::new();
		if let Some((line, expected_line, actual_line)) = first_diff_line(&expected_text, &actual_text) {
			detail = format!(
				"first differing line {}\nexpected: {}\n  actual: {}\n",
				line,
				expected_line,
				actual_line
			);
		}
		panic!(
			"stdout mismatch for op {} in fixture {}\n{}",
			op,
			fixture_dir.display(),
			detail
		);
	}
}
