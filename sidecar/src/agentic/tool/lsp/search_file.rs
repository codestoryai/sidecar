//! Searches the files in a given directory given a regex
//! Can be used by the agent to grep for this in the repository or in a sub-directory

use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;
use tokio::{io::BufReader, process::Command};

use crate::agentic::tool::r#type::ToolRewardScale;
use crate::agentic::tool::{errors::ToolError, input::ToolInput, output::ToolOutput, r#type::Tool};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Magic number which came into existence to not break LLM context windows
/// This limits the number of results to 250 hits, if its more than that, the LLM
/// or the human needs to be more specific
const MAX_RESULTS: usize = 250;

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
enum RipgrepEvent {
    #[serde(rename = "match")]
    Match {
        path: RipgrepPath,
        lines: RipgrepLines,
        line_number: usize,
    },
    #[serde(rename = "context")]
    Context {
        lines: RipgrepLines,
        line_number: usize,
    },
}

#[derive(Debug, serde::Deserialize)]
struct RipgrepPath {
    text: String,
}

#[derive(Debug, serde::Deserialize)]
struct RipgrepLines {
    text: String,
}

#[derive(Debug)]
struct SearchResult {
    file: String,
    line: usize,
    match_line: String,
    before_context: Vec<String>,
    after_context: Vec<String>,
}

impl SearchResult {}

#[derive(Debug, Clone)]
pub struct SearchFileContentWithRegexOutput {
    formatted_response: String,
}

impl SearchFileContentWithRegexOutput {
    pub fn response(&self) -> &str {
        &self.formatted_response
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchFileContentInputPartial {
    directory_path: String,
    regex_pattern: String,
    file_pattern: Option<String>,
}

impl SearchFileContentInputPartial {
    pub fn new(
        directory_path: String,
        regex_pattern: String,
        file_pattern: Option<String>,
    ) -> Self {
        Self {
            directory_path,
            regex_pattern,
            file_pattern,
        }
    }

    pub fn directory_path(&self) -> &str {
        &self.directory_path
    }

    pub fn regex_pattern(&self) -> &str {
        &self.regex_pattern
    }

    pub fn file_pattern(&self) -> Option<&str> {
        self.file_pattern.as_deref()
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<search_files>
<directory_path>
{}
</directory_path>
<regex_pattern>
{}
</regex_pattern>
<file_pattern>
{}
</file_pattern>
</search_files>"#,
            self.directory_path,
            self.regex_pattern,
            self.file_pattern
                .clone()
                .unwrap_or("not provided".to_owned())
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "search_files",
            "description": "Request to perform a regex search across files in a specified directory, providing context-rich results.\nThis tool searches for patterns or specific content across multiple files, displaying each match with encapsulating context.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "directory_path": {
                        "type": "string",
                        "description": "(required) The absolute path of the directory to search in. This directory will be recursively searched.",
                    },
                    "regex_pattern": {
                        "type": "string",
                        "description": "(required) The regular expression pattern to search for. Uses Rust regex syntax.",
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "(optional) Glob pattern to filter files (e.g., '*.ts' for TypeScript files). If not provided, it will search all files (*).",
                    },
                },
                "required": ["directory_path", "regex_pattern"],
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct SearchFileContentInput {
    directory_path: String,
    regex_pattern: String,
    file_pattern: Option<String>,
    editor_url: String,
}

impl SearchFileContentInput {
    pub fn new(
        directory_path: String,
        regex_pattern: String,
        file_pattern: Option<String>,
        editor_url: String,
    ) -> Self {
        Self {
            directory_path,
            regex_pattern,
            file_pattern,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct EditorRipGrepPath {
    rip_grep_path: String,
}

pub struct SearchFileContentClient {
    client: reqwest::Client,
}

impl SearchFileContentClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for SearchFileContentClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_search_file_content_with_regex()?;
        // first grab the rip-grep path from the editor
        let endpoint = context.editor_url.to_owned() + "/rip_grep_path";
        let rg_path = if let Ok(response) = self.client.post(endpoint).send().await {
            let response: EditorRipGrepPath = response
                .json()
                .await
                .map_err(|_e| ToolError::SerdeConversionFailed)?;
            response.rip_grep_path
        } else {
            String::from("rg")
        };

        let regex_pattern = &context.regex_pattern;
        let file_pattern = &context
            .file_pattern
            .filter(|x| x != "null")
            .unwrap_or("*".to_owned());
        let args = vec![
            "--follow",
            // enables lookaround
            "--pcre2",
            "-e",
            regex_pattern,
            "--glob",
            file_pattern,
            "--context",
            "1",
            // do not enable multiline over here, from the docs:
            // https://gist.github.com/theskcd/a6369001b3ea3c0212bbc88d8a74211f from
            // rg --help | grep multiline
            // "--multiline",
            &context.directory_path,
        ];

        println!("search_files::args::({:?})", args);

        let mut child = Command::new(rg_path)
            .args(&args)
            .stdout(Stdio::piped())
            // close stdin so rg does not wait for input from the stdin fd
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| ToolError::IOError(e))?;

        // now we can read the output from the child line by line and parse it out properly
        let stdout = child.stdout.take();
        if let None = stdout {
            println!("stdout is empty over here");
            return Err(ToolError::OutputStreamNotPresent);
        }

        let stdout = stdout.expect("Failed to capture stdout");
        let reader = BufReader::new(stdout).lines();

        let mut output = String::new();
        let mut line_count = 0;
        let max_lines = MAX_RESULTS * 4;

        tokio::pin!(reader);

        while let Some(line) = reader.next_line().await? {
            if line_count >= max_lines {
                break;
            }
            output.push_str(&line);
            output.push('\n');
            line_count += 1;
        }

        let _status = child.wait().await?;

        Ok(ToolOutput::search_file_content_with_regex(
            SearchFileContentWithRegexOutput {
                formatted_response: output,
            },
        ))
    }

    fn tool_description(&self) -> String {
        format!(
            r#"### search_files
Request to perform a regex search across files in a specified directory, providing context-rich results.
This tool searches for patterns or specific content across multiple files, displaying each match with encapsulating context."#
        )
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
- directory_path: (required) The absolute path of the directory to search in. This directory will be recursively searched.
- regex_pattern: (required) The regular expression pattern to search for. Uses Rust regex syntax.
- file_pattern: (optional) Glob pattern to filter files (e.g., '*.ts' for TypeScript files). If not provided, it will search all files (*).

Usage:
<search_files>
<directory_path>
Directory path here
</directory_path>
<regex_pattern>
Your regex pattern here
</regex_pattern>
<file_pattern>
file pattern here (optional)
</file_pattern>
</search_files>"#
        )
    }

    fn get_evaluation_criteria(&self, trajectory_length: usize) -> Vec<String> {
        let mut evaluation_criteria = if trajectory_length < 3 {
            vec![
                "Exploratory Actions: Recognize that initial searches and information-gathering steps are essential and should not be heavily penalized if they don't yield immediate results.",
                "Appropriateness of Action: Evaluate if the action is logical given the agent's current knowledge and the early stage of problem-solving.",
            ]
        } else {
            vec![
                "Solution Quality: Assess the logical changes, contextual fit, and overall improvement without introducing new issues.",
                "Progress Assessment: Evaluate the agent's awareness of solution history, detection of repetitive actions, and planned next steps.",
                "Repetitive or Redundant Actions: Detect if the agent is repeating the same unsuccessful or redundant actions without making progress. Pay close attention to the agent's history and outputs indicating lack of progress.",
            ]
        };
        evaluation_criteria.extend(vec![
            "Query Relevance: Evaluate if the search query or parameters are well-defined and likely to find relevant code.",
            "Search Scope Appropriateness: Check if the file patterns and class/function names narrow down the search effectively.",
            "Relevance of Search Results: Assess whether the search results are directly related to the problem and useful for making progress.",
            "Size of Search Results: Ensure that the code context provided is appropriately sized—not too large to overwhelm nor too small to be unhelpful.",
        ]);
        evaluation_criteria
            .into_iter()
            .map(|evaluation_criteria| evaluation_criteria.to_owned())
            .collect()
    }

    fn get_reward_scale(&self, trajectory_length: usize) -> Vec<ToolRewardScale> {
        if trajectory_length < 3 {
            vec![
                ToolRewardScale::new(
                    90,
                    100,
                    "The search action is excellent, with well-defined parameters yielding only highly relevant results.",
                ),
                ToolRewardScale::new(
                    75,
                    89,
                    "The search action is good, with reasonable parameters yielding relevant results.",
                ),
                ToolRewardScale::new(
                    25,
                    74,
                    "The search action have issues with parameters or yields few or no relevant results.",
                ),
                ToolRewardScale::new(
                    0,
                    24,
                    "The action is counterproductive, with search results that are entirely irrelevant or excessively large, causing setbacks.",
                ),
            ]
        } else {
            vec![
                ToolRewardScale::new(
                    90,
                    100,
                    "The search action significantly advances the solution, providing highly relevant and appropriately sized search results.",
                ),
                ToolRewardScale::new(
                    75,
                    89,
                    "The search action contributes positively towards solving the problem, with relevant results and minor issues.",
                ),
                ToolRewardScale::new(
                    50,
                    74,
                    "The search action is acceptable but may have issues with relevance or provides search results that are too large or too small.",
                ),
                ToolRewardScale::new(
                    25,
                    49,
                    "The search action provides results that are not helpful due to relevance or size issues.",
                ),
                ToolRewardScale::new(
                    0,
                    24,
                    "The search action has minimal impact, providing few relevant results.",
                ),
                ToolRewardScale::new(
                    -50,
                    -1,
                    "The action is counterproductive, with search results that are entirely irrelevant or excessively large, causing setbacks.",
                ),
            ]
        }
    }
}
