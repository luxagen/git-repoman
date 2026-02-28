use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

use crate::LIST_SEPARATOR;

/// Typed configuration values with proper types for each setting
#[derive(Debug, Clone)]
pub struct Config {
    /// Configuration filename (.grm.conf by default)
    pub config_filename: String,
    /// List filename
    pub list_filename: String,
    /// Whether recursion is enabled (1 by default)
    pub recurse_enabled: bool,
    /// Remote login information (e.g., ssh://user@host)
    pub rlogin: String,
    /// Remote path base directory
    pub rpath_base: String,
    /// Remote path template for new repositories
    pub rpath_template: String,
    /// Local base directory for repositories
    pub local_dir: String,
    /// Media base directory
    pub gm_dir: String,
    /// Remote directory
    pub remote_dir: String,
    /// Git arguments when in git mode
    pub git_args: String,
    /// Command to execute for configuration
    pub config_cmd: String,
    /// Recurse prefix for path display
    pub recurse_prefix: String,
    /// Tree filter path for filtering repositories to current subtree
    pub tree_filter: String,
}

impl Config {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            config_filename: ".grm.conf".to_string(),
            list_filename: String::new(),
            recurse_enabled: true,
            rlogin: String::new(),
            rpath_base: String::new(),
            rpath_template: String::new(),
            local_dir: String::new(),
            gm_dir: String::new(),
            remote_dir: String::new(),
            git_args: String::new(),
            config_cmd: String::new(),
            recurse_prefix: String::new(),
            tree_filter: String::new(),
        }
    }
    
    // TODO Why to_string()? Tie lifetimes together to use &str everywhere?

    /// Get all configuration values as string key-value pairs (for environment variable passing)
    pub fn all_values(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        
        // Add all values with their string representations
        result.push(("CONFIG_FILENAME".to_string(), self.config_filename.clone()));
        result.push(("LIST_FN".to_string(), self.list_filename.clone()));
        result.push(("OPT_RECURSE".to_string(), if self.recurse_enabled { "1".to_string() } else { String::new() }));
        
        if !self.rlogin.is_empty() {
            result.push(("RLOGIN".to_string(), self.rlogin.clone()));
        }
        
        if !self.rpath_base.is_empty() {
            result.push(("RPATH_BASE".to_string(), self.rpath_base.clone()));
        }
        
        if !self.rpath_template.is_empty() {
            result.push(("RPATH_TEMPLATE".to_string(), self.rpath_template.clone()));
        }
        
        if !self.local_dir.is_empty() {
            result.push(("LOCAL_DIR".to_string(), self.local_dir.clone()));
        }
        
        if !self.gm_dir.is_empty() {
            result.push(("GM_DIR".to_string(), self.gm_dir.clone()));
        }
        
        if !self.remote_dir.is_empty() {
            result.push(("REMOTE_DIR".to_string(), self.remote_dir.clone()));
        }
        
        if !self.git_args.is_empty() {
            result.push(("GIT_ARGS".to_string(), self.git_args.clone()));
        }
        
        if !self.config_cmd.is_empty() {
            result.push(("CONFIG_CMD".to_string(), self.config_cmd.clone()));
        }
        
        if !self.recurse_prefix.is_empty() {
            result.push(("RECURSE_PREFIX".to_string(), self.recurse_prefix.clone()));
        }
        
        if !self.tree_filter.is_empty() {
            result.push(("TREE_FILTER".to_string(), self.tree_filter.clone()));
        }
        
        result
    }
    
    /// Load configuration from environment variables starting with GRM_
    pub fn load_from_env(&mut self) {
        // Check if this is a recursive invocation and set the recurse_prefix
        if let Ok(prefix) = std::env::var("GRM_RECURSE_PREFIX") {
            self.recurse_prefix = prefix;
        } else {
            self.recurse_prefix = String::new();
        }
        
        // Determine if we are in a recursive call for permission checking
        let is_recursive = !self.recurse_prefix.is_empty();
        
        for (key, value) in std::env::vars() {
            if let Some(conf_key) = key.strip_prefix("GRM_") {
                // For root process, only allow specific variables from environment
                if !is_recursive {
                    match conf_key {
                        "CONFIG_FILENAME" | "LIST_FN" | "CONFIG_CMD" => {
                            // These are allowed from environment for root process
                        },
                        _ => {
                            // All other variables are not allowed for root process
                            continue;
                        }
                    }
                }
                
                // Set configuration value
                self.set_from_string(conf_key, value);
            }
        }
    }

    // This should:
    // - load the entire file into RAM in binary mode (no translation)
    // - call parse_config_line until the content is exhausted
    // - treat a single-element vector as a key with an empty value
    // - set configuration values using set_from_string
    //
    // - one empty cell: skip
    // - one non-empty cell: repo with local/GM defaulting
    // - two cells: repo with local/GM override
    // - three cells: repo with local/GM override and remote

    // - return an error if the file cannot be opened or read
    // 

    /// Load configuration from a file
    pub fn load_from_file(&mut self, path: &Path) -> Result<()> {
        let iter = ConfigLineIterator::from_file(path)?;

        // TODO sort out this tree

        for line_result in iter {
            // First handle any parsing errors
            let mut cells = match line_result {
                Ok(cells) => cells,
                Err(err) => return Err(err.context("Error parsing configuration file"))
            };
            
            // Error if line contains more than 3 cells
            if cells.len() != 3 {
                return Err(anyhow!("Config line has {} columns instead of the required 3", cells.len()));
            }

            // Error if the first cell is not empty (not a config line)
            if !cells[0].is_empty() {
                return Err(anyhow!("Repository specification found in config file: {:?}", cells));
            }

            // Only need to check that key (cells[1]) is not empty
            // cells[2] can be empty (which means the config value should be emptied)
            if cells[1].is_empty() {
                return Err(anyhow!("Config line has empty key or value: {:?}", cells));
            }
            
            // We need at least 3 cells for key and value
            if cells.len() == 3 {
                // Move both values out of the vector first
                let key = std::mem::replace(&mut cells[1], String::new());
                let value = std::mem::replace(&mut cells[2], String::new());
                
                // Now that we own key, we can get a reference to it
                let key_ref = key.as_str();
                
                self.set_from_string(key_ref, value);
            }
        }
        
        Ok(())
    }

    /// Set a configuration value from string key and value
    pub fn set_from_string(&mut self, key: &str, value: String) {
        match key {
            "CONFIG_FILENAME" => self.config_filename = value,
            "LIST_FN" => self.list_filename = value,
            "OPT_RECURSE" => self.recurse_enabled = !value.is_empty(),
            "RLOGIN" => self.rlogin = value,
            "RPATH_BASE" => self.rpath_base = value,
            "RPATH_TEMPLATE" => self.rpath_template = value,
            "LOCAL_DIR" => self.local_dir = value,
            "GM_DIR" => self.gm_dir = value,
            "REMOTE_DIR" => self.remote_dir = value,
            "GIT_ARGS" => self.git_args = value,
            "CONFIG_CMD" => self.config_cmd = value,
            "RECURSE_PREFIX" => self.recurse_prefix = value,
            "TREE_FILTER" => self.tree_filter = value,
            _ => {} // Ignore unknown keys
        }
    }
}

/// Iterator over parsed lines from a configuration file or repository file
pub struct ConfigLineIterator {
    content: String,
    position: usize,
}

impl ConfigLineIterator {
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

impl Iterator for ConfigLineIterator {
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
                
                // Skip empty lines and comments (they return empty cell vectors)
                if cells.is_empty() {
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
pub fn parse_config_cell(input: &str) -> Result<(String, &str)> {
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
pub fn parse_config_line(input: &str) -> Result<(Vec<String>, &str)> {
    // Skip empty lines
    if input.is_empty() {
        return Ok((Vec::new(), input));
    }
    
    // Parse the first cell to check for comments (this will skip whitespace)
    let (first_cell, first_remainder) = parse_config_cell(input)?;
    
    // Check if it's a comment after skipping whitespace
    if first_cell.starts_with('#') {
        return Ok((Vec::new(), input));
    }
    
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
        
        let parse_result = parse_config_cell(remainder);
        
        // Handle errors in cell parsing
        if let Err(err) = parse_result {
            return Err(err);
        }
        
        let (cell, new_remainder) = parse_result?;
        
        // Check if this cell is a comment
        if cell.starts_with('#') {
            // For comments, skip to end of line (or input)
            remainder = slice_to_eol(new_remainder);
            break;
        }
        
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

/// Slice a string from the current position to the end of the line
/// Returns a substring from the current position to the next line ending character,
/// or an empty slice at the end of the string if no line ending is found.
fn slice_to_eol(input: &str) -> &str {
    for (i, c) in input.char_indices() {
        if c == '\r' || c == '\n' {
            return &input[i..];
        }
    }
    &input[input.len()..]
}