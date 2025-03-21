//! Helps the LLM find files which it is interested in
//! We allow the LLM to suggest a regex which we can search for in the directory we are interested in

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

use super::list_files::list_files;

pub struct FindFilesClient {}

impl FindFilesClient {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone)]
pub struct FindFilesRequest {
    pattern: String,
    root_directory: String,
}

impl FindFilesRequest {
    pub fn new(pattern: String, root_directory: String) -> Self {
        Self {
            pattern,
            root_directory,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FindFileInputPartial {
    pattern: String,
}

impl FindFileInputPartial {
    pub fn new(pattern: String) -> Self {
        Self { pattern }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<find_file>
<pattern>
{}
</pattern>
</find_file>"#,
            &self.pattern
        )
    }
}

#[derive(Debug, Clone)]
pub struct FindFilesResponse {
    files: Vec<PathBuf>,
}

impl FindFilesResponse {
    pub fn files(&self) -> &[PathBuf] {
        self.files.as_slice()
    }
}

fn compile_glob_set(patterns: &[String]) -> Result<GlobSet, ToolError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

fn find_files(
    pattern: &str,
    files: &[PathBuf],
    root_directory: &str,
) -> Result<Vec<PathBuf>, ToolError> {
    // Compile include globs
    let include_patterns = vec![pattern.to_owned()];
    let include_set = compile_glob_set(include_patterns.as_slice())?;

    let mut results = Vec::new();

    for file in files {
        // Compute relative path
        let relative_path = match file.strip_prefix(root_directory) {
            Ok(p) => p,
            Err(_) => continue, // Skip files not under the search directory
        };

        // Convert relative path to a string with forward slashes for glob matching
        let rel_path_str = relative_path.to_string_lossy().replace("\\", "/");

        // Check include patterns
        if !include_set.is_match(&rel_path_str) {
            continue;
        }

        // Collect the result
        results.push(file.clone());
    }

    Ok(results)
}

#[async_trait]
impl Tool for FindFilesClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_find_files()?;
        let directory_path = Path::new(&context.root_directory);
        // handwaving a limit of 1M files when running the find operation over here
        let file_list = list_files(&directory_path, true, 1_000_000).0;
        let find_files = find_files(&context.pattern, &file_list, &context.root_directory)?;
        Ok(ToolOutput::FindFiles(FindFilesResponse {
            files: find_files,
        }))
    }

    fn tool_description(&self) -> String {
        r#"### find_file
This tool searches for files and directories within a specified directory, similar to the Linux find command.
It supports glob patterns for searching and filtering which will all be passed in with -ipath.
The patterns provided should match the relative paths from the root directory.
They should use glob patterns with wildcards, for example, **/*.py, **/*_test*.
It will return the absolute path of the file paths if any hits have been found

Parameters:
- pattern: Pattern to search for (required)"#.to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"Parameters:
- pattern: (required): Pattern to search for and should match the relative paths from the root directory

Usage:
<find_file>
<pattern>
Your pattern over here
</pattern>
</find_file>"#.to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
