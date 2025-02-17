//! Contains the struct for search and replace style editing

use std::{collections::HashMap, path::Path, sync::Arc};
use async_trait::async_trait;
use futures::{lock::Mutex, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::{
    agentic::{
        symbol::{
            identifier::{LLMProperties, SymbolIdentifier},
            ui_event::{EditedCodeStreamingRequest, UIEventWithID},
        },
        tool::{
            errors::ToolError,
            helpers::{
                cancellation_future::run_with_cancellation,
                diff_recent_changes::DiffRecentChanges,
            },
            input::ToolInput,
            lsp::{diagnostics::DiagnosticWithSnippet, open_file::OpenFileRequest},
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
            session::chat::{SessionChatMessage, SessionChatRole},
        },
    },
    chunking::text_document::{Position, Range},
};

const _SURROUNDING_CONTEXT_LIMIT: usize = 200;

struct DropDetector<T>(T);

impl<T> Drop for DropDetector<T> {
    fn drop(&mut self) {
        println!("DropDetector is being dropped!");
    }
}

#[derive(Debug)]
pub struct SearchAndReplaceEditingResponse {
    updated_code: String,
    response: String,
}

impl SearchAndReplaceEditingResponse {
    pub fn new(updated_code: String, response: String) -> Self {
        Self {
            updated_code,
            response,
        }
    }

    pub fn updated_code(&self) -> &str {
        &self.updated_code
    }

    pub fn response(&self) -> &str {
        &self.response
    }
}

#[derive(Debug)]
pub struct SearchAndReplaceAccumulator {
    code_lines: Vec<String>,
    current_line: usize,
    sender: UnboundedSender<UIEventWithID>,
}

impl SearchAndReplaceAccumulator {
    pub fn new(code: String, id: usize, sender: UnboundedSender<UIEventWithID>) -> Self {
        let code_lines = code.lines().map(|s| s.to_string()).collect();
        Self {
            code_lines,
            current_line: 0,
            sender,
        }
    }

    pub async fn add_delta(&mut self, delta: String) {
        let head = "<<<<<<< SEARCH";
        let divider = "=======";
        let tail = ">>>>>>> REPLACE";
        
        let mut lines = delta.lines();
        while let Some(line) = lines.next() {
            // Skip until we find a code block
            if line.starts_with("```") {
                // Skip the opening fence
                continue;
            }
            
            if line.contains(head) {
                // Found start of search block
                let mut search_block = String::new();
                while let Some(line) = lines.next() {
                    if line == divider {
                        break;
                    }
                    if !search_block.is_empty() {
                        search_block.push('\n');
                    }
                    search_block.push_str(line);
                }
                
                // Found start of replace block
                let mut replace_block = String::new();
                while let Some(line) = lines.next() {
                    if line.contains(tail) {
                        break;
                    }
                    if !replace_block.is_empty() {
                        replace_block.push('\n');
                    }
                    replace_block.push_str(line);
                }
                
                // If search block is empty, this is an insertion
                if search_block.is_empty() {
                    self.code_lines = replace_block.lines().map(|s| s.to_string()).collect();
                    
                    // Send update event
                    let event = EditedCodeStreamingRequest {
                        code: self.join_lines(),
                    };
                    if let Err(e) = self.sender.send(UIEventWithID::EditedCodeStreaming(event)) {
                        eprintln!("Failed to send update: {}", e);
                    }
                } else {
                    // Find and replace the block
                    let search_lines: Vec<_> = search_block.lines().collect();
                    let replace_lines: Vec<_> = replace_block.lines().collect();
                    
                    for i in 0..=self.code_lines.len() - search_lines.len() {
                        if self.code_lines[i..i + search_lines.len()]
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>() == search_lines
                        {
                            self.code_lines.splice(
                                i..i + search_lines.len(),
                                replace_lines.iter().map(|&s| s.to_string()),
                            );
                            
                            // Send update event
                            let event = EditedCodeStreamingRequest {
                                code: self.join_lines(),
                            };
                            if let Err(e) = self.sender.send(UIEventWithID::EditedCodeStreaming(event)) {
                                eprintln!("Failed to send update: {}", e);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    fn join_lines(&self) -> String {
        let mut result = self.code_lines.join("\n");
        if self.has_trailing_newline || !self.code_lines.is_empty() {
            result.push('\n');
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn test_newline_preservation() {
        // Test case 1: File with existing trailing newline
        let original_code = "fn main() {\n    println!(\"Hello\");\n}\n";
        let edits = r#"```rust
<<<<<<< SEARCH
fn main() {
    println!("Hello");
}
=======
fn main() {
    println!("Updated");
}
>>>>>>> REPLACE
```"#;
        let (sender, _receiver) = unbounded_channel();
        let mut accumulator = SearchAndReplaceAccumulator::new(original_code.to_owned(), 0, sender);
        accumulator.add_delta(edits.to_owned()).await;
        let final_code = accumulator.join_lines();
        assert_eq!(final_code, "fn main() {\n    println!(\"Updated\");\n}\n");

        // Test case 2: Empty file getting new content
        let empty_code = "";
        let edits = r#"```rust
<<<<<<< SEARCH
=======
fn test() {
    println!("New content");
}
>>>>>>> REPLACE
```"#;
        let (sender, _receiver) = unbounded_channel();
        let mut accumulator = SearchAndReplaceAccumulator::new(empty_code.to_owned(), 0, sender);
        accumulator.add_delta(edits.to_owned()).await;
        let final_code = accumulator.join_lines();
        assert_eq!(final_code, "fn test() {\n    println!(\"New content\");\n}\n");
    }
}