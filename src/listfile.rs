// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::fs::File;
use std::io::Read;
use std::path::Path;
use anyhow::{Context, Result, anyhow};

pub enum ParsedLine
{
	Empty,
	Whitespace,
	#[allow(dead_code)]
	Comment  {content: String},
	Config   {key: String, value: String},
	RepoSpec {local: String, remote: String, param: String},
	Malformed,
}

/// Iterator over parsed lines from a configuration file or repository file
pub struct LineIterator {
    content: String,
    position: usize,
}

impl LineIterator {
    /// Create a new iterator from a file path
    pub fn from_file(path: &Path) -> Result<Self> {
        // Read the entire file into memory in binary mode
        let mut file = File::open(path)
            .with_context(|| format!("Failed to open file: {}", path.display()))?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        let content = std::str::from_utf8(&bytes)
            .map_err(|_| anyhow!("File is not valid UTF-8: {}", path.display()))?
            .to_string();
        
        Ok(Self {
            content,
            position: 0,
        })
    }
}

impl Iterator for LineIterator {
    type Item = ParsedLine;
    
    fn next(&mut self) -> Option<Self::Item> {
		// If we've reached the end of the content, stop iteration.
		if self.position >= self.content.len() {
			return None;
		}
        
        let remainder = &self.content[self.position..];
		let (line, new_remainder) = parse_config_line(remainder);
        
		// Update position for next iteration
		debug_assert!({
			let base = self.content.as_ptr() as usize;
			let end = base + self.content.len();
			let ptr = new_remainder.as_ptr() as usize;
			ptr >= base && ptr <= end
		});

		let new_pos = self.content.len() - new_remainder.len();
		debug_assert!(self.content.is_char_boundary(new_pos));
		self.position = new_pos;
		Some(line)
    }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_comment_preserves_separators() {
		let input = "   # hello*world * yep\r\nrest";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::Comment{content} => assert_eq!(content, "hello*world * yep"),
			_ => panic!("expected comment"),
		}
		assert_eq!(remainder, "rest");
	}

	#[test]
	fn parse_comment_hash_only() {
		let input = "#\n";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::Comment{content} => assert_eq!(content, ""),
			_ => panic!("expected comment"),
		}
		assert_eq!(remainder, "");
	}

	#[test]
	fn parse_escaped_separator_with_unicode() {
		let input = "α\\*β*γ\n";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::RepoSpec{local, remote, param} => {
				assert_eq!(local, "α*β");
				assert_eq!(remote, "γ");
				assert_eq!(param, "");
			}
			_ => panic!("expected repospec"),
		}
		assert_eq!(remainder, "");
	}

	#[test]
	fn repo_line_defaults_all_fields_one_cell() {
		let input = "foo\n";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::RepoSpec{remote, local, param} => {
				assert_eq!(remote, "foo");
				assert_eq!(local, "foo");
				assert_eq!(param, "foo");
			}
			_ => panic!("expected repospec"),
		}
		assert_eq!(remainder, "");
	}

	#[test]
	fn repo_line_defaults_param_two_cells() {
		let input = "foo*bar\n";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::RepoSpec{remote, local, param} => {
				assert_eq!(remote, "foo");
				assert_eq!(local, "bar");
				assert_eq!(param, "bar");
			}
			_ => panic!("expected repospec"),
		}
		assert_eq!(remainder, "");
	}

	#[test]
	fn repo_line_no_defaults_three_cells() {
		let input = "foo*bar*baz\n";
		let (line, remainder) = parse_config_line(input);
		match line {
			ParsedLine::RepoSpec{remote, local, param} => {
				assert_eq!(remote, "foo");
				assert_eq!(local, "bar");
				assert_eq!(param, "baz");
			}
			_ => panic!("expected repospec"),
		}
		assert_eq!(remainder, "");
	}

	#[test]
	fn parse_trailing_backslash_errors() {
		let err = parse_config_cell("abc\\").unwrap_err();
		assert!(format!("{}", err).contains("Trailing backslash"));
	}
}

/// Consume a line ending (CR, LF, or CRLF) from the start of input.
/// Returns the remaining input after the line ending.
fn consume_line_ending(mut input: &str) -> &str {
	if input.starts_with('\r') {
		input = &input['\r'.len_utf8()..];
		if input.starts_with('\n') {
			input = &input['\n'.len_utf8()..];
		}
	} else if input.starts_with('\n') {
		input = &input['\n'.len_utf8()..];
	}
	input
}

fn parse_config_line(input: &str) -> (ParsedLine, &str) {
	use crate::CELL_SEPARATOR;

	// 1. Check for empty line
	if input.is_empty() {
		return (ParsedLine::Empty, input);
	}

	// 1a. If line begins with optional whitespace and then '#', treat as comment.
	// Preserve separators literally by slicing from the original line rather than using cell parsing.
	let line_end = input.find(|c| c == '\r' || c == '\n').unwrap_or(input.len());
	let line = &input[..line_end];
	let line_remainder = &input[line_end..];
	let ws_skipped = skip_whitespace(line);
	if ws_skipped.starts_with('#') {
		let after_hash = ws_skipped.strip_prefix('#').unwrap_or("");
		let content_str = skip_whitespace(after_hash);
		let remainder = consume_line_ending(line_remainder);
		return (ParsedLine::Comment{content: content_str.to_string()}, remainder);
	}
	
	// 2. Attempt first cell parse
	let (first_cell, mut remainder) = match parse_config_cell(input)
	{
		Ok(p) => p,
		Err(_) => return (ParsedLine::Malformed, ""),
	};

	// 3. Continue parsing cells into vector until EOL
	let mut cells = vec![first_cell];
	while remainder.starts_with(CELL_SEPARATOR)
	{
		remainder = &remainder[CELL_SEPARATOR.len_utf8()..];
		cells.push(
			match parse_config_cell(remainder)
			{
				Err(_) => {panic!();},
				Ok((cell, new_remainder)) =>
				{
					remainder = new_remainder;
					cell
				},
			}
		);
	}
	
	// Consume line ending
	remainder = consume_line_ending(remainder);
	
	// 4. If count>3, malformed
	if cells.len() > 3 {
		return (ParsedLine::Malformed, remainder);
	}
	
	// 5. If first cell empty, config line or whitespace
	if cells[0].is_empty() {
		// Check for whitespace-only line
		if cells.len() == 1 {
			return (ParsedLine::Whitespace, remainder);
		}
		
		// Config line validation
		if cells[1].is_empty() {
			return (ParsedLine::Malformed, remainder);
		}
		return (
			ParsedLine::Config
			{
				key: cells[1].clone(),
				value: cells.get(2).cloned().unwrap_or_default(),
			},
			remainder);
	}
	
	// 6. Otherwise map present values into RepoSpec
	(
		ParsedLine::RepoSpec
		{
			remote: cells[0].clone(),
			local: cells.get(1).cloned().unwrap_or_else(|| cells[0].clone()),
			param: cells.get(2).cloned().unwrap_or_else(|| cells.get(1).cloned().unwrap_or_else(|| cells[0].clone())),
		},
		remainder,
	)
}

/// Parse a single cell from a configuration or repository file line.
/// 
/// This function handles several important aspects of parsing:
/// - Skips leading whitespace
/// - Handles escaped characters (e.g., `\*` doesn't separate fields)
/// - Preserves escaped whitespace 
/// - Stops at unescaped line endings (CR, LF) or separator characters
/// - Trims trailing whitespace from the right
/// - Treats a trailing backslash at end of line as an error
///
/// If the cell cannot be parsed (empty input, immediate delimiter, etc.), 
/// an empty string is returned.
///
/// # Error
/// Returns an error when a trailing backslash is found at the end of the line 
/// with no character to escape.
///
/// Note: Escaped whitespace (e.g., `\ `) is preserved and never trimmed, only unescaped
/// trailing whitespace is removed.
///
/// # Arguments
/// - `input`: The input string to parse
///
/// # Returns
/// A Result containing:
/// - On success: A tuple with the parsed cell and remaining input
/// - On error: An anyhow error explaining the issue
fn parse_config_cell(input: &str) -> Result<(String, &str)> {
	use crate::CELL_SEPARATOR;

    // Skip leading whitespace
    let input = skip_whitespace(input);
    
    // If we hit a newline, CR, separator, or empty string while skipping whitespace
    if input.is_empty() || input.starts_with('\n') || input.starts_with('\r') || input.starts_with(CELL_SEPARATOR) {
        return Ok((String::new(), input));
    }
    
    // Start building the cell content
    let mut cell = String::new();
    let mut input = input;
    let mut rtrim_pos = 0;
    
    // Process one character at a time, handling escapes
    while !input.is_empty() {
        // First check for line endings or separator character without consuming them
        if input.starts_with('\r') || input.starts_with('\n') || input.starts_with(CELL_SEPARATOR) {
            break;
        }
        
        // Get the next character
        let c = input.chars().next().unwrap();
        
        // Advance past the current character
        input = &input[c.len_utf8()..];
        
        // Handle escaping
        if c == '\\' {
            if input.is_empty() {
                // Error: backslash at end of line with nothing to escape
                return Err(anyhow!("Trailing backslash at end of line with nothing to escape"));
            }
            
            // Get the escaped character
            let escaped = input.chars().next().unwrap();
            
            // Add the escaped character to the cell
            cell.push(escaped);
            rtrim_pos = cell.len(); // Escaped chars are never trimmed
            
            // Advance past the escaped character
            input = &input[escaped.len_utf8()..];
        } else {
            // Add to cell
            cell.push(c);
            
            // Update right trim position if not whitespace
            if !c.is_whitespace() {
                rtrim_pos = cell.len();
            }
        }
    }

    // Truncate to the right trim position (after the last non-whitespace)
    cell.truncate(rtrim_pos);
    
    // Return the cell directly, without additional scanning or copying
    Ok((cell, input))
}

/// Skip leading whitespace in the input string (excluding CR and LF).
/// Returns the remaining string starting at the first non-whitespace character, newline, or end of string
fn skip_whitespace(input: &str) -> &str {
    let mut input = input;
    
    // Skip leading whitespace (excluding CR and LF) until we find non-whitespace or newline
    loop {
        input = match input.chars().next() {
            // Found regular whitespace (not CR or LF)
            Some(c) if c.is_whitespace() && c != '\r' && c != '\n' => {
                &input[c.len_utf8()..]
            },
            // Found CR, LF, other non-whitespace, or end of string
            _ => return input,
        };
    }
}