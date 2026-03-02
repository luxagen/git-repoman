// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use std::fs::File;
use std::io::Read;
use std::path::Path;
use anyhow::{Context, Result, anyhow};

use crate::LIST_SEPARATOR;

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
        
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        
        Ok(Self {
            content,
            position: 0,
        })
    }
}

impl Iterator for LineIterator {
    type Item = Result<Vec<String>>;
    
    fn next(&mut self) -> Option<Self::Item> {
        // If we've reached the end of the content, stop iteration
        if self.position >= self.content.len() {
            return None;
        }
        
        let remainder = &self.content[self.position..];
        let parse_result = parse_config_line(remainder);
        
        match parse_result {
            Ok((cells, new_remainder)) => {
                // Update position for next iteration
                self.position = self.content.len() - new_remainder.len();

				if cells.is_empty() || cells[0].starts_with('#')
				{
					return self.next();
				}

                Some(Ok(cells))
            },
            Err(err) => {
                // Simply propagate the error directly
                Some(Err(err))
            }
        }
    }
}

/// Parse a line into a vector of cells and the remaining unparsed portion.
/// Returns a vector containing each parsed cell and the
/// remaining input after parsing stopped.
/// 
/// The function stops parsing when:
/// - It reaches the end of the input
/// - It can't make progress (current position doesn't change after parsing)
/// - It encounters a delimiter or line ending
///
/// Any line endings (CR, LF, or CRLF) at the end of the line are consumed.
///
/// # Arguments
/// - `input`: The input string to parse
///
/// # Returns
/// A Result containing:
/// - On success: A tuple with parsed cells and remaining input
/// - On error: An error from cell parsing (like trailing backslash)
fn parse_config_line(input: &str) -> Result<(Vec<String>, &str)> {
    // Skip empty lines
    if input.is_empty() {
        return Ok((Vec::new(), input));
    }
    
    // Parse the first cell to check for comments (this will skip whitespace)
    let (first_cell, first_remainder) = parse_config_cell(input)?;
    
    // Start building cells with the first cell we already parsed
    let mut cells = Vec::new();
    cells.push(first_cell);
    
    let mut remainder = first_remainder;
    
    // Parse cells until we can't make progress
    loop {
        // Check if we're at a separator 
        if !remainder.starts_with(LIST_SEPARATOR) {
            break;
        }
        
        // Skip past the separator and continue parsing
        remainder = &remainder[LIST_SEPARATOR.len_utf8()..];
        
        let (cell, new_remainder) = parse_config_cell(remainder)?;

        // Add the cell to our vector
        cells.push(cell);
        
        // If we couldn't make progress, stop parsing
        if remainder == new_remainder {
            break;
        }
        
        remainder = new_remainder;
    }
    
    // Handle line endings
    match remainder.chars().next() {
        None => {} // EOF
        Some('\r') => { // CR or CRLF
            remainder = &remainder['\r'.len_utf8()..];
            // If CRLF, consume the LF too
            if remainder.starts_with('\n') {
                remainder = &remainder['\n'.len_utf8()..];
            }
        }
        Some('\n') => { // Just LF
            remainder = &remainder['\n'.len_utf8()..];
        }
        _ => {} // No line ending but we're done parsing cells
    }

    Ok((cells, remainder))
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
    // Skip leading whitespace
    let input = skip_whitespace(input);
    
    // If we hit a newline, CR, separator, or empty string while skipping whitespace
    if input.is_empty() || input.starts_with('\n') || input.starts_with('\r') || input.starts_with(LIST_SEPARATOR) {
        return Ok((String::new(), input));
    }
    
    // Start building the cell content
    let mut cell = String::new();
    let mut input = input;
    let mut rtrim_pos = 0;
    
    // Process one character at a time, handling escapes
    while !input.is_empty() {
        // First check for line endings or separator character without consuming them
        if input.starts_with('\r') || input.starts_with('\n') || input.starts_with(LIST_SEPARATOR) {
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