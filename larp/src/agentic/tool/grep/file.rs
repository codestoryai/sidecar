//! We want to grep over some content in the file and return
//! the first position where we find it
use async_trait::async_trait;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::Position,
};

pub struct FindInFile {}

impl FindInFile {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindInFileRequest {
    file_contents: String,
    file_symbol: String,
}

impl FindInFileRequest {
    pub fn new(file_contents: String, file_symbol: String) -> Self {
        Self {
            file_contents,
            file_symbol,
        }
    }
}

#[derive(Debug)]
pub struct FindInFileResponse {
    position: Option<Position>,
}

impl FindInFileResponse {
    pub fn get_position(self) -> Option<Position> {
        self.position
    }
}

impl FindInFile {
    pub fn get_symbol_location(&self, input: FindInFileRequest) -> Option<Position> {
        let symbol = &input.file_symbol;
        let file_lines = input
            .file_contents
            .lines()
            .enumerate()
            .collect::<Vec<(_, _)>>();

        let positions: Vec<Position> = file_lines
            .into_iter()
            .filter_map(|line| {
                if line.1.contains(symbol) {
                    // then we grab at which character we have a match
                    let column = line
                        .1
                        .chars()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .as_slice()
                        .windows(symbol.chars().into_iter().collect::<Vec<_>>().len())
                        .enumerate()
                        .find(|(_idx, window)| {
                            window
                                .into_iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join("")
                                == symbol.to_owned()
                        })
                        .map(|(idx, _)| idx);
                    if let Some(column) = column {
                        Some(Position::new(line.0, column, 0))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if let Some(position) = positions.first() {
            Some(position.clone())
        } else {
            None
        }
    }
}

#[async_trait]
impl Tool for FindInFile {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.grep_single_file()?;
        let response = self.get_symbol_location(context);
        Ok(ToolOutput::GrepSingleFile(FindInFileResponse {
            position: response,
        }))
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
