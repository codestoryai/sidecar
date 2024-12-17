use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    agentic::tool::code_edit::code_editor::{CodeEditorParameters, EditorCommand},
    mcts::action_node::ActionObservation,
    agentic::{
        symbol::{
            identifier::SymbolIdentifier,
            ui_event::{EditedCodeStreamingRequest, UIEventWithID},
        },
        tool::{
            code_edit::{
                search_and_replace::StreamedEditingForEditor,
            },
            errors::ToolError,
        },
    },
};

use super::error::AnthropicEditorError;

/// Editor implementation for Anthropic models with computer use capabilities
pub struct AnthropicCodeEditor {
    tool_thinking: String,
    computer_use_enabled: bool,
    streaming_enabled: bool,
    editor_url: String,
}

fn maybe_truncate(s: &str) -> String {
    let max_lines = 200; // arbitrary limit to mimic Python code’s truncation
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() > max_lines {
        let mut truncated = lines[..max_lines].join("\n");
        truncated.push_str("\n... [truncated] ...\n");
        truncated
    } else {
        s.to_string()
    }
}

impl AnthropicCodeEditor {
    pub fn new(tool_thinking: String, editor_url: String) -> Self {
        Self {
            tool_thinking,
            computer_use_enabled: true,  // Enable by default for compatibility
            streaming_enabled: true,     // Enable streaming by default
            editor_url,
        }
    }

    pub async fn run_command(
        &self,
        params: CodeEditorParameters,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        let path = PathBuf::from(&params.path);
        if let Some(observation) = self.validate_path(&params.command, &path) {
            return Ok(observation);
        }

        match params.command {
            EditorCommand::View => self.view(&path, params.view_range).await,
            EditorCommand::Create => {
                let file_text = params.file_text.ok_or_else(|| {
                    AnthropicEditorError::InputParametersMissing(
                        "Parameter `file_text` required for `create`.".to_owned(),
                    )
                })?;
                self.create(&path, &file_text).await
            }
            EditorCommand::StrReplace => {
                let old_str = params.old_str.ok_or_else(|| {
                    AnthropicEditorError::InputParametersMissing(
                        "Parameter `old_str` required for `str_replace`".to_owned(),
                    )
                })?;
                let new_str = params.new_str;
                self.str_replace(&path, &old_str, new_str.as_deref()).await
            }
            EditorCommand::Insert => {
                let insert_line = params.insert_line.ok_or_else(|| {
                    AnthropicEditorError::InputParametersMissing(
                        "Parameter `insert_line` required for `insert`".to_owned(),
                    )
                })?;
                let new_str = params.new_str.ok_or_else(|| {
                    AnthropicEditorError::InputParametersMissing(
                        "Parameter `new_str` required for `insert`".to_owned(),
                    )
                })?;
                self.insert(&path, insert_line, &new_str).await
            }
            EditorCommand::UndoEdit => Ok(ActionObservation::errored(
                "undo_edit not supported, use str_replace instead".to_owned(),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            )),
            EditorCommand::ComputerUse => {
                if !self.computer_use_enabled {
                    return Ok(ActionObservation::errored(
                        "Computer use operations are not enabled".to_owned(),
                        Some(self.tool_thinking.to_owned()),
                        true,
                        false,
                    ));
                }
                self.handle_computer_use(params).await
            }
        }
    }

    /// Returns whether computer use operations are enabled
    pub fn computer_use_enabled(&self) -> bool {
        self.computer_use_enabled
    }

    /// Returns whether streaming is enabled for operations
    pub fn streaming_enabled(&self) -> bool {
        self.streaming_enabled
    }

    /// Returns the editor URL
    pub fn editor_url(&self) -> &str {
        &self.editor_url
    }

    fn validate_path(&self, command: &EditorCommand, path: &Path) -> Option<ActionObservation> {
        match command {
            EditorCommand::Create => {
                // Disable this check since we are okay with creating files which
                // already exist
                // if path.exists() {
                //     return Some(ActionObservation::errored(
                //         format!(
                //             "File already exists at: {:?}. Cannot overwrite with `create`.",
                //             path
                //         ),
                //         true,
                //         false,
                //     ));
                // }
            }
            _ => {
                if !path.exists() {
                    return Some(ActionObservation::errored(
                        format!("The path {:?} does not exist.", path),
                        Some(self.tool_thinking.to_owned()),
                        true,
                        false,
                    ));
                }
            }
        }

        if path.is_dir() && *command != EditorCommand::View {
            return Some(ActionObservation::errored(
                format!(
                    "The path {:?} is a directory and only `list_files` can be used on directories.",
                    path
                ),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            ));
        }

        None
    }

    async fn create(
        &self,
        path: &Path,
        file_text: &str,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        self.write_file(path, file_text).await?;

        let message = format!("File created successfully at: {:?}", path);
        Ok(ActionObservation::new(
            message.to_owned(),
            message,
            Some(self.tool_thinking.to_owned()),
            false,
        )
        .file_content_updated(path.to_string_lossy().to_string(), file_text.to_owned()))
    }

    async fn view(
        &self,
        path: &Path,
        view_range: Option<Vec<i32>>,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        if path.is_dir() {
            if view_range.is_some() {
                return Err(AnthropicEditorError::ViewCommandError(
                    "The `view_range` is not allowed for directories.".to_owned(),
                ));
            }
            return self.view_directory(path).map(|output| {
                ActionObservation::new(
                    output.to_owned(),
                    output.to_owned(),
                    Some(self.tool_thinking.to_owned()),
                    false,
                )
            });
        }

        let file_content = self.read_file(path).await?;
        let (content, init_line) = if let Some(range) = view_range {
            if range.len() != 2 {
                return Err(AnthropicEditorError::ViewCommandError(
                    "`view_range` should be a list of two integers.".to_owned(),
                ));
            }
            let (start, end) = (range[0], range[1]);
            let cloned_file_content = file_content.clone();
            let file_lines: Vec<&str> = cloned_file_content.lines().collect();
            let n_lines = file_lines.len() as i32;

            if start < 1 || start > n_lines {
                return Err(AnthropicEditorError::ViewCommandError(format!(
                    "Invalid start line {} for range. Should be between 1 and {}.",
                    start, n_lines
                )));
            }

            if end != -1 && (end < start || end > n_lines) {
                return Err(AnthropicEditorError::ViewCommandError(format!(
                    "Invalid end line {} for range. Should be >= {} and <= {} or -1.",
                    end, start, n_lines
                )));
            }

            let slice = if end == -1 {
                &file_lines[(start - 1) as usize..]
            } else {
                &file_lines[(start - 1) as usize..end as usize]
            };

            (slice.join("\n"), start)
        } else {
            (file_content.to_owned(), 1)
        };

        let message = self.make_output(&content, &format!("{:?}", path), init_line);
        Ok(
            // always send the fact that we were able to view the file
            // at this point, since the patch we will create will be on top of this
            ActionObservation::new(
                message.to_owned(),
                message.to_owned(),
                Some(self.tool_thinking.to_owned()),
                false,
            )
            .file_content_updated(path.to_string_lossy().to_string(), file_content),
        )
    }

    fn view_directory(&self, path: &Path) -> Result<String, AnthropicEditorError> {
        let entries = std::fs::read_dir(path).map_err(|e| {
            AnthropicEditorError::ViewCommandError(format!("Error reading directory: {:?}", e))
        })?;

        let mut output = format!(
            "Listing files and directories (excluding hidden) in {:?}:\n",
            path
        );

        for entry in entries {
            let entry = entry.map_err(|e| {
                AnthropicEditorError::ViewCommandError(format!("Dir entry error: {:?}", e))
            })?;
            let file_name = entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| "<non-UTF8>".to_string());
            if file_name.starts_with('.') {
                continue; // Skip hidden files
            }
            output.push_str(&format!("{}/{}\n", path.display(), file_name));
        }
        Ok(output)
    }

    async fn str_replace(
        &self,
        path: &Path,
        old_str: &str,
        new_str: Option<&str>,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        let file_content = self.read_file(path).await?;
        let occurrences = file_content.matches(old_str).count();

        if occurrences == 0 {
            return Ok(ActionObservation::errored(
                format!("No occurrence of `{}` found in {:?}.", old_str, path),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            ));
        } else if occurrences > 1 {
            let lines: Vec<_> = file_content
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(old_str))
                .map(|(i, _)| i + 1)
                .collect();

            return Ok(ActionObservation::errored(
                format!(
                    "Multiple occurrences of `{}` found in lines {:?}. Please ensure uniqueness.",
                    old_str, lines
                ),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            ));
        }

        let replacement = new_str.unwrap_or("");
        let new_file_content = file_content.replace(old_str, replacement);
        self.write_file(path, &new_file_content).await?;

        let snippet = self.extract_snippet(&new_file_content, replacement);
        let mut msg = format!("The file {:?} has been edited. ", path);
        msg.push_str(&self.make_output(&snippet, &format!("a snippet of {:?}", path), 1));
        msg.push_str("Review the changes if necessary.");
        Ok(ActionObservation::new(
            msg.to_owned(),
            msg.to_owned(),
            Some(self.tool_thinking.to_owned()),
            false,
        )
        .file_content_updated(
            path.to_string_lossy().to_string(),
            new_file_content.to_owned(),
        ))
    }

    async fn insert(
        &self,
        path: &Path,
        insert_line: u32,
        new_str: &str,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        let file_text = self.read_file(path).await?;
        let file_lines: Vec<&str> = file_text.lines().collect();
        let n_lines = file_lines.len() as u32;

        if insert_line > n_lines {
            return Ok(ActionObservation::errored(
                format!(
                    "`insert_line` {} out of range [0, {}].",
                    insert_line, n_lines
                ),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            ));
        }

        let new_str_lines: Vec<&str> = new_str.lines().collect();
        let mut new_file_lines = Vec::new();
        new_file_lines.extend_from_slice(&file_lines[..insert_line as usize]);
        new_file_lines.extend_from_slice(&new_str_lines[..]);
        new_file_lines.extend_from_slice(&file_lines[insert_line as usize..]);

        let new_file_text = new_file_lines.join("\n");
        self.write_file(path, &new_file_text).await?;

        let snippet_lines = self.make_insert_snippet(&file_lines, &new_str_lines, insert_line);
        let mut msg = format!("The file {:?} has been edited. ", path);
        msg.push_str(&self.make_output(
            &snippet_lines.join("\n"),
            "a snippet of the edited file",
            ((insert_line as i32) - 4).max(1) as i32,
        ));
        msg.push_str(
            "Review the changes and make sure they are correct (indentation, no duplicates, etc).",
        );
        Ok(ActionObservation::new(
            msg.to_owned(),
            msg.to_owned(),
            Some(self.tool_thinking.to_owned()),
            false,
        )
        .file_content_updated(path.to_string_lossy().to_string(), new_file_text))
    }

    async fn read_file(&self, path: &Path) -> Result<String, AnthropicEditorError> {
        match tokio::fs::read_to_string(path).await {
            Err(e) => Err(AnthropicEditorError::ReadingFileError(format!(
                "Error reading file {:?}: {:?}",
                path, e
            ))),
            Ok(output) => Ok(output),
        }
    }

    async fn write_file(&self, path: &Path, content: &str) -> Result<(), AnthropicEditorError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AnthropicEditorError::InputParametersMissing(format!(
                    "Error creating directories for {:?}: {}",
                    parent, e
                ))
            })?;
        }
        tokio::fs::write(path, content).await.map_err(|e| {
            AnthropicEditorError::InputParametersMissing(format!(
                "Error writing file {:?}: {}",
                path, e
            ))
        })
    }

    fn make_output(&self, file_content: &str, file_descriptor: &str, init_line: i32) -> String {
        let truncated = maybe_truncate(file_content);
        let mut numbered = String::new();
        for (i, line) in truncated.lines().enumerate() {
            writeln!(&mut numbered, "{:6}\t{}", i as i32 + init_line, line).ok();
        }

        format!(
            "Here's the result of `cat -n` on {}:\n{}\n",
            file_descriptor, numbered
        )
    }

    fn extract_snippet(&self, new_file_content: &str, new_str: &str) -> String {
        // Find the position of new_str in new_file_content
        let pos = match new_file_content.find(new_str) {
            Some(p) => p,
            None => {
                // new_str not found, return something indicative or handle error
                return String::from("new_str not found in new_file_content");
            }
        };

        // Determine which line new_str was on
        let prefix = &new_file_content[..pos];
        let snippet_line = prefix.lines().count();

        // Extract a snippet around that line
        let lines: Vec<&str> = new_file_content.lines().collect();
        let snippet_lines = 4; // number of lines to show around the match
        let start_line = snippet_line.saturating_sub(snippet_lines);
        let end_line = (snippet_line + snippet_lines + new_str.lines().count()).min(lines.len());
        lines[start_line..end_line].join("\n")
    }

    /// Handles computer use operations from Anthropic models
    async fn handle_computer_use(
        &self,
        params: CodeEditorParameters,
    ) -> Result<ActionObservation, AnthropicEditorError> {
        if !self.computer_use_enabled {
            return Ok(ActionObservation::errored(
                "Computer use operations are not enabled".to_owned(),
                Some(self.tool_thinking.to_owned()),
                true,
                false,
            ));
        }

        let file_text = params.file_text.ok_or_else(|| {
            AnthropicEditorError::InputParametersMissing(
                "Parameter `file_text` required for computer use operations".to_owned(),
            )
        })?;

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut accumulator = ComputerUseAccumulator::new(sender.clone(), self);

        // Set up streaming for UI updates
        let streamed_editor = StreamedEditingForEditor::new();

        // Add the operation to accumulator
        accumulator.add_delta(file_text).await;

        // Process the computer use operation
        let mut operation_result = String::new();
        while let Some(delta) = receiver.recv().await {
            match delta {
                ComputerUseDelta::OperationStarted => {
                    if self.streaming_enabled {
                        streamed_editor
                            .send_edit_event(
                                self.editor_url.clone(),
                                EditedCodeStreamingRequest::start_edit(
                                    "computer_use".to_owned(),
                                    "computer_use".to_owned(),
                                    Range::default(),
                                    None,
                                ),
                            )
                            .await;
                    }
                }
                ComputerUseDelta::OperationProgress(status) => {
                    operation_result = status.clone();
                    if self.streaming_enabled {
                        streamed_editor
                            .send_edit_event(
                                self.editor_url.clone(),
                                EditedCodeStreamingRequest::delta(
                                    "computer_use".to_owned(),
                                    "computer_use".to_owned(),
                                    Range::default(),
                                    Some(status),
                                    None,
                                    None,
                                ),
                            )
                            .await;
                    }
                }
                ComputerUseDelta::OperationComplete => {
                    if self.streaming_enabled {
                        streamed_editor
                            .send_edit_event(
                                self.editor_url.clone(),
                                EditedCodeStreamingRequest::end(
                                    "computer_use".to_owned(),
                                    "computer_use".to_owned(),
                                    Range::default(),
                                    None,
                                ),
                            )
                            .await;
                    }
                    break;
                }
            }
        }

        Ok(ActionObservation::new(
            "Computer use operation completed successfully".to_owned(),
            operation_result,
            Some(self.tool_thinking.to_owned()),
            false,
        ))
    }

    fn make_insert_snippet(
        &self,
        original_lines: &[&str],
        inserted_lines: &[&str],
        insert_line: u32,
    ) -> Vec<String> {
        let snippet_lines = 4;

        let start = insert_line.saturating_sub(snippet_lines);
        let end = (insert_line + snippet_lines).min(original_lines.len() as u32);

        let mut snippet = Vec::new();
        // Lines before
        snippet.extend_from_slice(&original_lines[start as usize..insert_line as usize]);
        // Inserted lines
        snippet.extend_from_slice(inserted_lines);
        // Lines after
        snippet.extend_from_slice(&original_lines[insert_line as usize..end as usize]);

        snippet.into_iter().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Clone)]
pub enum ComputerUseDelta {
    OperationStarted,
    OperationProgress(String),
    OperationComplete,
}

#[derive(Debug)]
pub enum ComputerUseOperation {
    ReadFile(String),
    WriteFile(String, String),
    CreateDirectory(String),
    DeleteFile(String),
    Invalid(String),
}

pub struct ComputerUseAccumulator {
    answer_up_until_now: String,
    operation_status: String,
    sender: UnboundedSender<ComputerUseDelta>,
    editor: &'static AnthropicCodeEditor,
}

impl ComputerUseAccumulator {
    pub fn new(sender: UnboundedSender<ComputerUseDelta>, editor: &'static AnthropicCodeEditor) -> Self {
        Self {
            answer_up_until_now: String::new(),
            operation_status: String::new(),
            sender,
            editor,
        }
    }

    pub async fn add_delta(&mut self, delta: String) {
        self.answer_up_until_now.push_str(&delta);
        self.process_operation().await;
    }

    async fn process_operation(&mut self) {
        let _ = self.sender.send(ComputerUseDelta::OperationStarted);

        // Parse and validate the operation
        match self.parse_operation() {
            Some(operation) => {
                match operation {
                    ComputerUseOperation::ReadFile(path) => {
                        self.operation_status = format!("Reading file: {}", path);
                        let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                            self.operation_status.clone(),
                        ));

                        // Use existing editor read_file method
                        match self.editor.read_file(std::path::Path::new(&path)).await {
                            Ok(_content) => {
                                self.operation_status = format!("Successfully read file: {}", path);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                            Err(e) => {
                                self.operation_status = format!("Error reading file {}: {}", path, e);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                        }
                    }
                    ComputerUseOperation::WriteFile(path, content) => {
                        self.operation_status = format!("Writing to file: {}", path);
                        let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                            self.operation_status.clone(),
                        ));

                        // Use existing editor write_file method
                        match self.editor.write_file(std::path::Path::new(&path), &content).await {
                            Ok(_) => {
                                self.operation_status = format!("Successfully wrote to file: {}", path);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                            Err(e) => {
                                self.operation_status = format!("Error writing to file {}: {}", path, e);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                        }
                    }
                    ComputerUseOperation::CreateDirectory(path) => {
                        self.operation_status = format!("Creating directory: {}", path);
                        let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                            self.operation_status.clone(),
                        ));

                        // Use tokio's create_dir_all for directory creation
                        match tokio::fs::create_dir_all(&path).await {
                            Ok(_) => {
                                self.operation_status = format!("Successfully created directory: {}", path);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                            Err(e) => {
                                self.operation_status = format!("Error creating directory {}: {}", path, e);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                        }
                    }
                    ComputerUseOperation::DeleteFile(path) => {
                        self.operation_status = format!("Deleting file: {}", path);
                        let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                            self.operation_status.clone(),
                        ));

                        // Use tokio's remove_file for file deletion
                        match tokio::fs::remove_file(&path).await {
                            Ok(_) => {
                                self.operation_status = format!("Successfully deleted file: {}", path);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                            Err(e) => {
                                self.operation_status = format!("Error deleting file {}: {}", path, e);
                                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                                    self.operation_status.clone(),
                                ));
                            }
                        }
                    }
                    ComputerUseOperation::Invalid(reason) => {
                        self.operation_status = format!("Invalid operation: {}", reason);
                        let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                            self.operation_status.clone(),
                        ));
                    }
                }
            }
            None => {
                self.operation_status = "No valid operation found".to_owned();
                let _ = self.sender.send(ComputerUseDelta::OperationProgress(
                    self.operation_status.clone(),
                ));
            }
        }

        let _ = self.sender.send(ComputerUseDelta::OperationComplete);
    }

    fn parse_operation(&self) -> Option<ComputerUseOperation> {
        // Parse the operation from the accumulated response
        // Expected format: <operation>:<parameters>
        // Example: read_file:/path/to/file
        let input = self.answer_up_until_now.trim();
        let parts: Vec<&str> = input.split(':').collect();

        match parts.as_slice() {
            [operation, path] => {
                let path = path.trim();
                match *operation {
                    "read_file" => Some(ComputerUseOperation::ReadFile(path.to_owned())),
                    "write_file" if parts.len() >= 3 => {
                        let content = parts[2..].join(":");
                        Some(ComputerUseOperation::WriteFile(path.to_owned(), content))
                    }
                    "create_directory" => Some(ComputerUseOperation::CreateDirectory(path.to_owned())),
                    "delete_file" => Some(ComputerUseOperation::DeleteFile(path.to_owned())),
                    _ => Some(ComputerUseOperation::Invalid(format!(
                        "Unknown operation: {}",
                        operation
                    ))),
                }
            }
            _ => Some(ComputerUseOperation::Invalid(
                "Invalid operation format".to_owned(),
            )),
        }
    }
}
