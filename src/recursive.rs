use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;
use crate::mode::get_operations;

/// Recursively process subdirectories, spawning new instances of the program
/// for directories containing listfiles
pub fn recurse_listfiles(dir: &Path, config: &Config, mode: &str) -> Result<()> {
    // Check if recursion is enabled
    let operations = get_operations();
    if !operations.recurse {
        return Ok(());
    }
    
    // Clean up the path before processing
    let dir_str = dir.to_string_lossy().to_string();
    let dir_str = dir_str.trim_end_matches('/');
    let dir_path = Path::new(dir_str);
    
    // Read directory entries
    let entries = fs::read_dir(dir_path)
        .with_context(|| format!("Failed to read directory: {}", dir_path.display()))?;
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        
        // Skip non-directories and hidden directories
        if !path.is_dir() || path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false) {
            continue;
        }
        
        let list_file_path = path.join(&config.list_filename);
        
        if list_file_path.exists() {
            // Recurse by spawning a new process
            recurse_to_subdirectory(&path, config, mode)?;
            
            // Skip further recursion - the spawned process will handle subdirectories
            continue;
        }
        
        // Continue recursing into this directory
        recurse_listfiles(&path, config, mode)?;
    }
    
    Ok(())
}

/// Spawn a new process to handle a subdirectory with a listfile
fn recurse_to_subdirectory(path: &Path, config: &Config, mode: &str) -> Result<()> {
    // Get relative path for constructing the recurse prefix
    let current_dir = env::current_dir()?;
    let path_rel = if let Ok(rel_path) = path.strip_prefix(&current_dir) {
        rel_path.to_string_lossy().to_string()
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    
    // Generate recurse prefix based on current hierarchy
    let recurse_prefix = if config.recurse_prefix.is_empty() {
        format!("{}/", path_rel)
    } else {
        format!("{}{}/", config.recurse_prefix, path_rel)
    };
    
    // Get path to current executable
    let exe_path = env::current_exe()
        .context("Failed to get path to current executable")?;
    
    // Build command to execute in subdirectory with preserved environment
    let mut cmd = Command::new(exe_path);
    cmd.arg(mode)
       .current_dir(path)
       // Set the recurse prefix for this level
       .env("GRM_RECURSE_PREFIX", recurse_prefix);
    
    // Add all config values with GRM_ prefix
    for (key, value) in config.all_values() {
        if key == "RECURSE_PREFIX" {
            // Don't pass recurse prefix (already handled)
            continue;
        }
        
        // Add GRM_ prefix to all other config variables
        cmd.env(format!("GRM_{}", key), value);
    }
    
    // Execute the command with preserved environment
    let status = cmd.status()
        .with_context(|| format!("Failed to spawn recursive process in: {}", path.display()))?;
    
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        eprintln!("Warning: Recursive instance in {} exited with code: {}", path.display(), code);
    }
    
    Ok(())
}
