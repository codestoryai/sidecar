//! This tool allows us to list the files which are present in the directory
//! in a BFS style fashion of iteration

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use ignore::WalkBuilder;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

/// Handwaving this number into existence, no promises offered here and this is just
/// a rough estimation of the context window
const FILES_LIMIT: usize = 250;

fn is_root_or_home(dir_path: &Path) -> bool {
    // Get root directory
    let root_dir = if cfg!(windows) {
        dir_path
            .components()
            .next()
            .map(|c| PathBuf::from(c.as_os_str()))
    } else {
        Some(PathBuf::from("/"))
    };
    let is_root = root_dir.map_or(false, |r| dir_path == r.as_path());

    // Get home directory
    let home_dir = dirs::home_dir();
    let is_home = home_dir.map_or(false, |h| dir_path == h.as_path());

    is_root || is_home
}

pub fn list_files(dir_path: &Path, recursive: bool, limit: usize) -> (Vec<PathBuf>, bool) {
    // Check if dir_path is root or home directory
    if is_root_or_home(dir_path) {
        return (vec![dir_path.to_path_buf()], false);
    }

    let mut results = Vec::new();
    let mut limit_reached = false;

    // Start time for timeout
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10); // Timeout after 10 seconds

    // BFS traversal
    let mut queue = VecDeque::new();
    queue.push_back(dir_path.to_path_buf());

    // Keep track of visited directories to avoid loops
    let mut visited_dirs = HashSet::new();

    // Define the ignore list
    let ignore_names: HashSet<&str> = [
        // js/ts pulled in files
        "node_modules",
        // cache from python
        "__pycache__",
        // env and venv belong to python
        "env",
        "venv",
        // rust like garbage which we don't want to look at
        "target",
        ".target",
        "build",
        // output directories for compiled code
        "dist",
        "out",
        "bundle",
        "vendor",
        // ignore tmp and temp which are common placeholders for temporary files
        "tmp",
        "temp",
        "deps",
        "pkg",
    ]
    .iter()
    .cloned()
    .collect();

    while let Some(current_dir) = queue.pop_front() {
        // Check for timeout
        if start_time.elapsed() > timeout {
            eprintln!("Traversal timed out, returning partial results");
            break;
        }

        // Check if we've reached the limit
        if results.len() >= limit {
            limit_reached = true;
            break;
        }

        // Check if we have visited this directory before
        if !visited_dirs.insert(current_dir.clone()) {
            continue; // Skip already visited directories
        }

        // Build a walker for the current directory
        let mut builder = WalkBuilder::new(&current_dir);
        builder
            // Apply .gitignore and other standard ignore files
            .standard_filters(true)
            // Do not ignore hidden files/directories
            .hidden(false)
            // Only immediate entries
            .max_depth(Some(1))
            // Follow symbolic links
            .follow_links(true);

        // For non-recursive traversal, disable standard filters
        if !recursive {
            builder.standard_filters(false);
        }

        // Clone ignore_names for the closure
        let ignore_names = ignore_names.clone();

        // Set filter_entry to skip ignored directories and files
        builder.filter_entry(move |entry| {
            if let Some(name) = entry.file_name().to_str() {
                // Skip ignored names
                if ignore_names.contains(name) {
                    return false;
                }
                // Do not traverse into hidden directories but include them in the results
                if entry.depth() > 0 && name.starts_with('.') {
                    if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        // Skip traversal into hidden directories
                        return false;
                    }
                }
            }
            true
        });

        let walk = builder.build();

        for result in walk {
            match result {
                Ok(entry) => {
                    let path = entry.path().to_path_buf();
                    // Skip the directory itself
                    if path == current_dir {
                        continue;
                    }
                    // Check if we've reached the limit
                    if results.len() >= limit {
                        limit_reached = true;
                        break;
                    }
                    results.push(path.clone());
                    // If recursive and it's a directory, enqueue it
                    if recursive && path.is_dir() {
                        queue.push_back(path);
                    }
                }
                Err(err) => eprintln!("Error: {}", err),
            }
        }
        if limit_reached {
            break;
        }
    }
    (results, limit_reached)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListFilesInput {
    directory_path: String,
    recursive: bool,
}

impl ListFilesInput {
    pub fn new(directory_path: String, recursive: bool) -> Self {
        Self {
            directory_path,
            recursive,
        }
    }

    pub fn directory_path(&self) -> &str {
        &self.directory_path
    }

    pub fn recursive(&self) -> bool {
        self.recursive
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<thinking>
...
</thinking>
<list_files>
<directory_path>
{}
</directory_path>
<recursive>
{}
</recursive>
</list_files>"#,
            self.directory_path, self.recursive
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "list_files",
            "description": r#"Request to list files and directories within the specified directory.
If recursive is true, it will list all files and directories recursively.
If recursive is false, it will only list the top-level contents.
Do not use this tool to confirm the existence of files you may have created, as the user will let you know if the files were created successfully or not."#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "directory_path": {
                        "type": "string",
                        "description": "(required) The absolute path of the directory to list contents for."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "(required) Whether to list files recursively. Use true for recursive listing, false for top-level only.",
                    }
                },
                "required": ["directory_path", "recursive"],
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListFilesOutput {
    files: Vec<PathBuf>,
}

impl ListFilesOutput {
    pub fn files(&self) -> &[PathBuf] {
        self.files.as_slice()
    }
}

pub struct ListFilesClient {}

impl ListFilesClient {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Tool for ListFilesClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_list_files()?;
        let directory = context.directory_path;
        let is_recursive = context.recursive;
        let output = list_files(Path::new(&directory), is_recursive, FILES_LIMIT);
        Ok(ToolOutput::ListFiles(ListFilesOutput { files: output.0 }))
    }

    fn tool_description(&self) -> String {
        r#"### list_files
Request to list files and directories within the specified directory.
If recursive is true, it will list all files and directories recursively.
If recursive is false, it will only list the top-level contents.
Do not use this tool to confirm the existence of files you may have created, as the user will let you know if the files were created successfully or not."#.to_owned()
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
- directory_path: (required) The absolute path of the directory to list contents for.
- recursive: (required) Whether to list files recursively. Use true for recursive listing, false for top-level only.

Usage:
<list_files>
<directory_path>
Directory path here
</directory_path>
<recursive>
true or false
</recursive>
</list_files>
"#
        )
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "Directory Path Validity: Ensure the requested directory path exists and is valid.",
            "Usefulness: Assess if listing the directory contents is helpful for the current task.",
            "Efficiency: Evaluate if the action is being used at an appropriate time in the workflow.",
        ].into_iter().map(|evaluation_criteria| evaluation_criteria.to_owned()).collect()
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new(
                5,
                0,
                "The action significantly advances the solution.",
            ),
            ToolRewardScale::new(
                0,
                4,
                "The action contributes positively towards solving the problem.",
            ),
            ToolRewardScale::new(
                5,
                9,
                "The action is acceptable but may have some issues.",
            ),
            ToolRewardScale::new(
                0,
                4,
                "The action has minimal impact or minor negative consequences.",
            ),
            ToolRewardScale::new(
                -49,
                -1,
                "The code change is inappropriate, unhelpful, introduces new issues, or redundantly repeats previous changes without making further progress. The Git diff does not align with instructions or is unnecessary.",
            ),
            ToolRewardScale::new(
                -100,
                -50,
                "The code change is counterproductive, causing significant setbacks or demonstrating persistent repetition without learning. The agent fails to recognize completed tasks and continues to attempt redundant actions.",
            ),
        ]
    }
}
