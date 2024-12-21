use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agentic::symbol::tool_box::ToolBox;
use crate::agentic::tool::code_edit::code_editor::{CodeEditorParameters, EditorCommand};

/// Anthropic code editor use module works on top of FS directly right now
/// and does not make sure of the editor apis we will do the migration
/// once we are done with integrating a first pass with the editor properly
pub struct AnthropicCodeEditorUse {
    tool_thinking: String,
    tool_box: Arc<ToolBox>,
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

impl AnthropicCodeEditorUse {
    pub fn new(tool_thinking: String, tool_box: Arc<ToolBox>) -> Self {
        Self {
            tool_thinking,
            tool_box,
        }
    }
    pub async fn run_command(&self, params: CodeEditorParameters) -> Result<String, String> {
        let path = PathBuf::from(&params.path);
        if let Some(observation) = self.validate_path(&params.command, &path) {
            return Ok(observation);
        }

        match params.command {
            EditorCommand::View => self.view(&path, params.view_range).await,
            EditorCommand::Create => {
                let file_text = params
                    .file_text
                    .ok_or_else(|| "Parameter `file_text` required for `create`.".to_owned())?;
                self.create(&path, &file_text).await
            }
            EditorCommand::StrReplace => {
                let old_str = params
                    .old_str
                    .ok_or_else(|| "Parameter `old_str` required for `str_replace`".to_owned())?;
                let new_str = params.new_str;
                self.str_replace(&path, &old_str, new_str.as_deref()).await
            }
            EditorCommand::Insert => {
                let insert_line = params
                    .insert_line
                    .ok_or_else(|| "Parameter `insert_line` required for `insert`".to_owned())?;
                let new_str = params
                    .new_str
                    .ok_or_else(|| "Parameter `new_str` required for `insert`".to_owned())?;
                self.insert(&path, insert_line, &new_str).await
            }
            EditorCommand::UndoEdit => {
                Ok("undo_edit not supported, use str_replace instead".to_owned())
            }
        }
    }

    fn validate_path(&self, command: &EditorCommand, path: &Path) -> Option<String> {
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
                    return Some(format!("The path {:?} does not exist.", path));
                }
            }
        }

        if path.is_dir() && *command != EditorCommand::View {
            return Some(format!(
                "The path {:?} is a directory and only `list_files` can be used on directories.",
                path
            ));
        }

        None
    }

    async fn create(&self, path: &Path, file_text: &str) -> Result<String, String> {
        self.write_file(path, file_text).await?;

        let message = format!("File created successfully at: {:?}", path);
        Ok(message.to_owned())
    }

    async fn view(&self, path: &Path, view_range: Option<Vec<i32>>) -> Result<String, String> {
        if path.is_dir() {
            if view_range.is_some() {
                return Ok("The `view_range` is not allowed for directories.".to_owned());
            }
            return self.view_directory(path).map(|output| output.to_owned());
        }

        let file_content = self.read_file(path).await?;
        let (content, init_line) = if let Some(range) = view_range {
            if range.len() != 2 {
                return Ok("`view_range` should be a list of two integers.".to_owned());
            }
            let (start, end) = (range[0], range[1]);
            let cloned_file_content = file_content.clone();
            let file_lines: Vec<&str> = cloned_file_content.lines().collect();
            let n_lines = file_lines.len() as i32;

            if start < 1 || start > n_lines {
                return Ok(format!(
                    "Invalid start line {} for range. Should be between 1 and {}.",
                    start, n_lines
                ));
            }

            if end != -1 && (end < start || end > n_lines) {
                return Ok(format!(
                    "Invalid end line {} for range. Should be >= {} and <= {} or -1.",
                    end, start, n_lines
                ));
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
            message,
        )
    }

    fn view_directory(&self, path: &Path) -> Result<String, String> {
        let entries =
            std::fs::read_dir(path).map_err(|e| (format!("Error reading directory: {:?}", e)))?;

        let mut output = format!(
            "Listing files and directories (excluding hidden) in {:?}:\n",
            path
        );

        for entry in entries {
            let entry = entry.map_err(|e| format!("Dir entry error: {:?}", e))?;
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
    ) -> Result<String, String> {
        let file_content = self.read_file(path).await?;
        let occurrences = file_content.matches(old_str).count();

        if occurrences == 0 {
            return Ok(format!(
                "No occurrence of `{}` found in {:?}.",
                old_str, path
            ));
        } else if occurrences > 1 {
            let lines: Vec<_> = file_content
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(old_str))
                .map(|(i, _)| i + 1)
                .collect();

            return Ok(format!(
                "Multiple occurrences of `{}` found in lines {:?}. Please ensure uniqueness.",
                old_str, lines
            ));
        }

        let replacement = new_str.unwrap_or("");
        let new_file_content = file_content.replace(old_str, replacement);
        self.write_file(path, &new_file_content).await?;

        let snippet = self.extract_snippet(&new_file_content, replacement);
        let mut msg = format!("The file {:?} has been edited. ", path);
        msg.push_str(&self.make_output(&snippet, &format!("a snippet of {:?}", path), 1));
        msg.push_str("Review the changes if necessary.");
        Ok(msg.to_owned())
    }

    async fn insert(&self, path: &Path, insert_line: u32, new_str: &str) -> Result<String, String> {
        let file_text = self.read_file(path).await?;
        let file_lines: Vec<&str> = file_text.lines().collect();
        let n_lines = file_lines.len() as u32;

        if insert_line > n_lines {
            return Ok(format!(
                "`insert_line` {} out of range [0, {}].",
                insert_line, n_lines
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
        Ok(msg.to_owned())
    }

    async fn read_file(&self, path: &Path) -> Result<String, String> {
        match tokio::fs::read_to_string(path).await {
            Err(e) => Ok(format!("Error reading file {:?}: {:?}", path, e)),
            Ok(output) => Ok(output),
        }
    }

    async fn write_file(&self, path: &Path, content: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Error creating directories for {:?}: {}", parent, e))?;
        }
        tokio::fs::write(path, content)
            .await
            .map_err(|e| format!("Error writing file {:?}: {}", path, e))
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
