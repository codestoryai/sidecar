use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use regex::Regex;

/// List files in a directory
pub fn list_files(dir: &Path, recursive: bool) -> Result<String> {
    let mut result = String::new();
    
    if recursive {
        // Recursively list all files
        visit_dirs(dir, &mut |path| {
            result.push_str(&format!("{}\n", path.display()));
        })?;
    } else {
        // List only top-level files
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            result.push_str(&format!("{}\n", path.display()));
        }
    }
    
    Ok(result)
}

/// Search for files containing a pattern
pub fn search_files(dir: &Path, pattern: &str, file_pattern: Option<&str>) -> Result<String> {
    let mut result = String::new();
    let regex = Regex::new(&pattern.to_lowercase())
        .context("Failed to compile regex pattern")?;
    
    // Create a regex for the file pattern if provided
    let file_regex = if let Some(file_pattern) = file_pattern {
        Some(Regex::new(&format!("^{}$", file_pattern.replace("*", ".*")))
            .context("Failed to compile file pattern regex")?)
    } else {
        None
    };
    
    // Visit all files in the directory
    visit_dirs(dir, &mut |path| {
        // Check if the file matches the file pattern
        if let Some(ref file_regex) = file_regex {
            if let Some(file_name) = path.file_name() {
                if let Some(file_name_str) = file_name.to_str() {
                    if !file_regex.is_match(file_name_str) {
                        return;
                    }
                }
            }
        }
        
        // Read the file content
        if let Ok(content) = fs::read_to_string(&path) {
            // Check if the content matches the pattern
            if regex.is_match(&content.to_lowercase()) {
                // Add the file path to the result
                result.push_str(&format!("{}\n", path.display()));
                
                // Add a preview of the matching content
                let mut preview = String::new();
                for (i, line) in content.lines().enumerate() {
                    if regex.is_match(&line.to_lowercase()) {
                        preview.push_str(&format!("{}:{}: {}\n", path.display(), i + 1, line));
                    }
                }
                
                // Add the preview to the result
                result.push_str(&preview);
                result.push_str("\n");
            }
        }
    })?;
    
    Ok(result)
}

/// Read a file
pub fn read_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).context("Failed to read file")
}

/// Edit a file
pub fn edit_file(path: &Path, content: &str) -> Result<()> {
    // Create parent directories if they don't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create parent directories")?;
    }
    
    // Write the content to the file
    fs::write(path, content).context("Failed to write to file")?;
    
    Ok(())
}

/// Execute a command
pub fn execute_command(command: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .context(format!("Failed to execute command: {} {:?}", command, args))?;
    
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(anyhow::anyhow!(
            "Command failed with exit code {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Find files matching a pattern
pub fn find_files(dir: &Path, pattern: &str) -> Result<String> {
    let mut result = String::new();
    let pattern_regex = Regex::new(&pattern.replace("*", ".*"))
        .context("Failed to compile pattern regex")?;
    
    visit_dirs(dir, &mut |path| {
        if let Some(path_str) = path.to_str() {
            if pattern_regex.is_match(path_str) {
                result.push_str(&format!("{}\n", path.display()));
            }
        }
    })?;
    
    Ok(result)
}

/// Helper function to recursively visit directories
fn visit_dirs(dir: &Path, cb: &mut dyn FnMut(&PathBuf)) -> Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&path);
            }
        }
    }
    
    Ok(())
}