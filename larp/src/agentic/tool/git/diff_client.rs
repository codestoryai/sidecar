//! Has the git-diff client which takes as input the root_directory
//! and the file we are interested in and spits out a plain old git diff
//! with the previous and the current version of the file
//!
use std::fs::File as StdFile;
use std::path::Path;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::agentic::tool::r#type::ToolRewardScale;
use crate::agentic::tool::{errors::ToolError, input::ToolInput, output::ToolOutput, r#type::Tool};
use crate::agentic::tool::git::operation_id::extract_operation_id;
use async_trait::async_trait;

// TODO(skcd): We want to talk to the editor to get the git-diff of the recently
// edited files here so we can use it for agentic and anchored editing
pub struct GitDiffClient {}

impl GitDiffClient {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone)]
pub struct GitDiffClientRequest {
    root_directory: String,
    fs_file_path: String,
    // exapnded implies that its `git diff -u 1000` the full view and not `git diff`
    expanded: bool,
}

impl GitDiffClientRequest {
    pub fn new(root_directory: String, fs_file_path: String, expanded: bool) -> Self {
        Self {
            root_directory,
            fs_file_path,
            expanded,
        }
    }

    pub fn root_directory(&self) -> &str {
        &self.root_directory
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }
}

#[derive(Debug, Clone)]
pub struct GitDiffClientResponse {
    fs_file_path: String,
    old_version: String,
    new_version: String,
    operation_id: Option<String>,
}

impl GitDiffClientResponse {
    pub fn old_version(&self) -> &str {
        &self.old_version
    }

    pub fn new_version(&self) -> &str {
        &self.new_version
    }
    
    pub fn operation_id(&self) -> Option<&str> {
        self.operation_id.as_deref()
    }
}

async fn run_command(
    root_directory: &str,
    fs_file_path: &str,
    expanded: bool,
) -> Result<GitDiffClientResponse, ToolError> {
    // Create a temporary file
    let tmpfile = NamedTempFile::new_in("/tmp").map_err(|e| ToolError::IOError(e))?;
    let tmpfile_path = tmpfile.path().to_path_buf();
    println!("tmpfile: {:?}", &tmpfile_path);

    // Run the git diff command, directing stdout to the temporary file
    // if we are in expanded mode, we want to get all the lines of the files in the diff
    let status = if expanded {
        Command::new("git")
            .current_dir(root_directory)
            .arg("diff")
            .arg("--no-prefix")
            .arg("-U8000")
            .stdout(Stdio::from(StdFile::create(&tmpfile_path)?))
            .status()
            .await?
    } else {
        // if we are in normal mode then we just want to get the git diff of the filepath
        // we do want to order it by time somewhat, to make it better
        Command::new("git")
            .current_dir(root_directory)
            .arg("diff")
            .stdout(Stdio::from(StdFile::create(&tmpfile_path)?))
            .status()
            .await?
    };

    if !status.success() {
        println!("{:?}", status.code());
        return Err(ToolError::RetriesExhausted);
    }

    // Asynchronously read the temporary file and stream the contentletfile = File::open(tmpfile_path)?;
    let file = File::open(tmpfile_path).await?;
    let mut reader = BufReader::new(file).lines();

    let mut output = "".to_owned();

    while let Some(line) = reader.next_line().await? {
        output.push_str(&line);
        output.push('\n');
    }

    if !expanded {
        // Try to extract operation ID from the git diff output
        let operation_id = extract_operation_id(&output);
        
        return Ok(GitDiffClientResponse {
            fs_file_path: "".to_owned(),
            old_version: "".to_owned(),
            // we just present the git diff over here in the new version
            // feeling too lazy to create a new tool so this is implict understood
            // behavior
            new_version: output,
            operation_id,
        });
    }

    // now we parse the git-diff in a very very hacky way, bear with me
    // example output:
    // println!("git_diff_output\n{}", &output);
    let results = parse_git_diff_output_full_length(&output, root_directory);
    Ok(results
        .into_iter()
        .find(|result| result.fs_file_path == fs_file_path)
        .ok_or(ToolError::RetriesExhausted)?)
}

// exmaple output:
// diff --git a/sidecar/src/bin/git_diff.rs b/sidecar/src/bin/git_diff.rs
// index 1c5d775c..c78ff82f 100644
// --- a/sidecar/src/bin/git_diff.rs
// +++ b/sidecar/src/bin/git_diff.rs
// @@ -50,5 +50,8 @@ async fn run_command(
//          output.push('\n');
//      }

// +    // now we parse the git-diff in a very very hacky way, bear with me
// +    //
// +
//      Ok(())
//  }

fn parse_git_diff_output_full_length(
    git_diff: &str,
    root_directory: &str,
) -> Vec<GitDiffClientResponse> {
    let mut diff_outputs = Vec::new();
    let git_diff_lines = git_diff.lines().into_iter().collect::<Vec<_>>();
    println!("git_diff_lines_len({})", git_diff_lines.len());
    // let sections = git_diff.split("diff --git").skip(1);
    let mut idx = 0;

    loop {
        if idx > git_diff_lines.len() {
            return diff_outputs;
        }
        // let lines: Vec<&str> = section.lines().collect();
        if !is_valid_diff_section(idx, &git_diff_lines) {
            idx = idx + 1;
            continue;
        }

        // now we want to go until we find the next such index
        let index_limit = (idx + 1..=git_diff_lines.len() - 1)
            .find(|idx| is_valid_diff_section(*idx, &git_diff_lines));
        // println!("index_limit({:?})::idx({})", index_limit, idx);
        let section_limit = if index_limit.is_none() {
            // this implies the end of the section
            git_diff_lines.len() - 1
        } else {
            index_limit.expect("is_none check above to hold") - 1
        };

        let fs_file_path = extract_file_path(&git_diff_lines[idx]);
        let joined_path = Path::new(root_directory).join(fs_file_path);
        let joined_path_str = joined_path.to_str().expect("to work");
        // we start after the diff git -- and index {crap} lines
        let slice_limits = &git_diff_lines[idx + 2..=section_limit];
        let (old_version, new_version) = extract_versions(slice_limits);
        
        // Extract operation ID from the content (try both old and new versions)
        let operation_id = extract_operation_id(&old_version)
            .or_else(|| extract_operation_id(&new_version));

        diff_outputs.push(GitDiffClientResponse {
            fs_file_path: joined_path_str.to_owned(),
            old_version,
            new_version,
            operation_id,
        });
        idx = section_limit;
    }
}

fn is_valid_diff_section(line_index: usize, git_diff_lines: &Vec<&str>) -> bool {
    line_index + 1 < git_diff_lines.len()
        && git_diff_lines[line_index].starts_with("diff --git")
        && git_diff_lines[line_index + 1].starts_with("index")
}

fn extract_file_path(line: &str) -> String {
    // println!("extract_file_path::({})", line);
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 3 {
        if parts[2].starts_with("a/") {
            parts[2].trim_start_matches("a/").to_owned()
        } else {
            parts[2].trim().to_owned()
        }
    } else {
        String::new()
    }
}

fn extract_versions(lines: &[&str]) -> (String, String) {
    let mut old_version = String::new();
    let mut new_version = String::new();
    for line in lines {
        if line.starts_with("@@") {
            continue;
        }
        if line.starts_with("---") {
            continue;
        }
        if line.starts_with("+++") {
            continue;
        }

        if line.starts_with('-') {
            old_version.push_str(&line[1..]);
            old_version.push('\n');
        } else if line.starts_with('+') {
            new_version.push_str(&line[1..]);
            new_version.push('\n');
        } else {
            old_version.push_str(&line[1..]);
            old_version.push_str("\n");
            new_version.push_str(&line[1..]);
            new_version.push_str("\n");
        }
    }

    (
        old_version.trim().to_string(),
        new_version.trim().to_string(),
    )
}

#[async_trait]
impl Tool for GitDiffClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.should_git_diff()?;
        let parsed_response = run_command(
            context.root_directory(),
            context.fs_file_path(),
            context.expanded(),
        )
        .await?;
        let git_diff = ToolOutput::git_diff_response(parsed_response);
        Ok(git_diff)
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}