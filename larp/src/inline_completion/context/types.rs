use crate::{
    chunking::text_document::{Position, Range},
    inline_completion::types::InLineCompletionError,
};

/// Different kinds of completions we can have

#[derive(Debug, Clone)]
pub struct DocumentLines {
    lines: Vec<(i64, String)>,
    line_start_position: Vec<Position>,
    line_end_position: Vec<Position>,
}

impl DocumentLines {
    pub fn new(
        lines: Vec<(i64, String)>,
        line_start_position: Vec<Position>,
        line_end_position: Vec<Position>,
    ) -> Self {
        Self {
            lines,
            line_start_position,
            line_end_position,
        }
    }

    /// Returns the content of the next non-empty line after the given position.
    ///
    /// # Arguments
    ///
    /// * `position` - The starting position to search from.
    ///
    /// # Returns
    ///
    /// * `Some(String)` - The content of the next non-empty line.
    /// * `None` - If there are no more non-empty lines after the given position.
    pub fn next_non_empty_line(&self, position: Position) -> Option<String> {
        // Calculate the line number of the next line
        let next_line_number = position.line() + 1;

        // If the next line number is out of bounds, return None
        if next_line_number >= self.lines.len() {
            return None;
        }

        // Iterate through the lines starting from the next line
        for idx in next_line_number..self.lines.len() {
            // Get a reference to the current line
            let line = &self.lines[idx];

            // Get the content of the current line
            let content = &line.1;

            // If the trimmed content is not empty, return it
            if !content.trim().is_empty() {
                return Some(content.to_owned());
            }
        }

        // If no non-empty line was found, return None
        None
    }

    pub fn prefix_at_line(&self, position: Position) -> Result<String, InLineCompletionError> {
        let line_number = position.line();
        if line_number >= self.lines.len() {
            return Err(InLineCompletionError::PrefixNotFound);
        }
        let line = &self.lines[line_number];
        let characters = line
            .1
            .chars()
            .into_iter()
            .map(|char| char.to_string())
            .collect::<Vec<_>>();
        // Now only get the prefix for this from the current line
        let line_prefix = characters[0..position.column() as usize].join("");
        Ok(line_prefix)
    }

    pub fn document_prefix(&self, position: Position) -> Result<String, InLineCompletionError> {
        let line_number = position.line();
        if line_number >= self.lines.len() {
            return Err(InLineCompletionError::PrefixNotFound);
        }
        let line = &self.lines[line_number];
        let characters = line
            .1
            .chars()
            .into_iter()
            .map(|char| char.to_string())
            .collect::<Vec<_>>();
        // Now only get the prefix for this from the current line
        let line_prefix = characters[0..position.column() as usize].join("");
        // Now get the prefix for the previous lines
        let mut lines = vec![];
        for line in &self.lines[0..line_number] {
            lines.push(line.1.to_owned());
        }
        lines.push(line_prefix);
        Ok(lines.join("\n"))
    }

    pub fn document_suffix(&self, position: Position) -> Result<String, InLineCompletionError> {
        let line_number = position.line();
        if line_number >= self.lines.len() {
            return Err(InLineCompletionError::SuffixNotFound);
        }
        let line = &self.lines[line_number];
        // Now only get the suffix for this from the current line
        let line_suffix = if line_number + 1 >= self.lines.len() {
            "".to_owned()
        } else {
            let characters = line
                .1
                .chars()
                .into_iter()
                .map(|char| char.to_string())
                .collect::<Vec<_>>();
            // Now only get the suffix for this from the current line
            characters[position.column() as usize..].join("")
        };
        // Now get the suffix for the next lines
        let mut lines = vec![line_suffix];
        for line in &self.lines[line_number + 1..] {
            lines.push(line.1.to_owned());
        }
        Ok(lines.join("\n"))
    }

    pub fn from_file_content(content: &str) -> Self {
        let mut byte_offset = 0;
        let lines: Vec<_> = content
            .lines()
            .enumerate()
            .map(|(_, line)| {
                // so here we will store the start and end byte position since we can
                // literally count the content size of the line and maintain
                // a running total of things
                let start = byte_offset;
                byte_offset += line.len();
                let end = byte_offset;
                byte_offset += 1; // for the newline
                (start, end)
            })
            .collect();
        let line_start_position: Vec<_> = content
            .lines()
            .enumerate()
            // the first entry is the start position offset, the second is the suffix
            .map(|(idx, _)| Position::new(idx, 0, lines[idx].0))
            .collect();
        let line_end_position: Vec<_> = content
            .lines()
            .enumerate()
            .map(|(idx, line)| Position::new(idx, line.chars().count(), lines[idx].1))
            .collect();
        Self::new(
            content
                .lines()
                .enumerate()
                .map(|(idx, line)| (idx as i64, line.to_string().to_owned()))
                .collect::<Vec<_>>(),
            line_start_position,
            line_end_position,
        )
    }

    pub fn get_line(&self, line_number: usize) -> &str {
        &self.lines[line_number].1
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn start_position_at_line(&self, line_number: usize) -> Position {
        self.line_start_position[line_number]
    }

    pub fn end_position_at_line(&self, line_number: usize) -> Position {
        self.line_end_position[line_number]
    }
}

#[derive(Debug, Clone)]
pub struct CodeSelection {
    _range: Range,
    _file_path: String,
    content: String,
}

impl CodeSelection {
    pub fn new(range: Range, file_path: String, content: String) -> Self {
        Self {
            _range: range,
            _file_path: file_path,
            content,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

pub enum CompletionContext {
    CurrentFile,
}

#[derive(Debug, Clone)]
pub struct CurrentFilePrefixSuffix {
    pub prefix: CodeSelection,
    pub suffix: CodeSelection,
    pub prefix_without_current_line: String,
    pub current_line_content: String,
}

impl CurrentFilePrefixSuffix {
    pub fn new(
        prefix: CodeSelection,
        suffix: CodeSelection,
        prefix_without_current_line: String,
        current_line_content: String,
    ) -> Self {
        Self {
            prefix,
            suffix,
            prefix_without_current_line,
            current_line_content,
        }
    }
}
