use llm_client::{clients::types::LLMType, provider::LLMProviderAPIKeys};
use regex::Regex;

use crate::chunking::text_document::{Position, Range};

use super::context::types::DocumentLines;

/// We are going to fix the range here based on the text document
/// following this convention because the byte offset from vscode
/// is different from the byte offset in rust
pub fn fix_vscode_range(range: Range, text_bytes: &[u8]) -> Range {
    // First we convert from the bytes to the string
    let fixed_range = range.clone();

    let start_position = fix_vscode_position(fixed_range.start_position(), text_bytes);
    let end_position = fix_vscode_position(fixed_range.end_position(), text_bytes);
    Range::new(start_position, end_position)
}

pub fn fix_vscode_position(mut position: Position, text_bytes: &[u8]) -> Position {
    let text_str = String::from_utf8(text_bytes.to_vec()).unwrap_or_default();
    // Now we have to split the text on the new lines
    let re = Regex::new(r"\r\n|\r|\n").unwrap();

    // Split the string using the regex pattern
    let lines: Vec<&str> = re.split(&text_str).collect();
    let position_byte_offset =
        line_column_to_byte_offset(lines.to_vec(), position.line(), position.column());
    if let Some(byte_offset) = position_byte_offset {
        position.set_byte_offset(byte_offset);
    }
    position
}

fn line_column_to_byte_offset(
    lines: Vec<&str>,
    target_line: usize,
    target_column: usize,
) -> Option<usize> {
    // Keep track of the current line and column in the input text
    let mut current_byte_offset = 0;

    for (index, line) in lines.iter().enumerate() {
        if index == target_line {
            let mut current_col = 0;

            // If the target column is at the beginning of the line
            if target_column == 0 {
                return Some(current_byte_offset);
            }

            for char in line.chars() {
                if current_col == target_column {
                    return Some(current_byte_offset);
                }
                current_byte_offset += char.len_utf8();
                current_col += 1;
            }

            // If the target column is exactly at the end of this line
            if current_col == target_column {
                return Some(current_byte_offset); // target_column is at the line break
            }

            // Column requested is beyond the current line length
            return None;
        }

        // Increment the byte offset by the length of the current line and its newline
        current_byte_offset += line.len() + "\n".len(); // add 1 for the newline character
    }

    // Line requested is beyond the input text line count
    None
}

pub fn insert_range(
    current_position: Position,
    document_lines: &DocumentLines,
    text: &str,
) -> Range {
    // so we first get the current line content
    let current_line = document_lines.get_line(current_position.line() as usize);
    let final_string = current_line.to_owned() + text;
    // now we can count the lines and the number of bytes and characters in the inserted snippet
    let lines = final_string.lines().collect::<Vec<_>>();
    let byte_offset = final_string.len();
    if lines.len() >= 1 {
        let final_line_number = (lines.len() - 1 + current_position.line()) as usize;
        let final_column_position = lines[lines.len() - 1].chars().count();
        // else we are going to insert from the current position to the end position
        let new_range = Range::new(
            current_position,
            Position::new(
                final_line_number,
                final_column_position,
                (byte_offset
                    + document_lines
                        .start_position_at_line(current_position.line())
                        .to_byte_offset()) as usize,
            ),
        );
        new_range
    } else {
        // just return the current position
        Range::new(current_position.clone(), current_position)
    }
}

/// This is the editor compatiable version of split_on_lines
/// it will split on \r\n and \n but with a catch
/// on the editor if have the content as just "\n" its really 2 lines
/// line 0: ""
/// line 1: ""
/// our normal lines() will just give us 1 line when we split it
/// so we handle that case explicitly here
pub fn split_on_lines_editor_compatiable(content: &str) -> Vec<String> {
    let extra_line = content.ends_with("\n") || content.ends_with("\r\n");
    let mut lines = content
        .lines()
        .into_iter()
        .map(|content_line| content_line.to_owned())
        .collect::<Vec<_>>();
    if extra_line {
        lines.push("".to_owned());
    }
    lines
}

/// This fixes the model when we are using the sidecar provider
/// we need to change the model to the one that the sidecar provider
pub fn fix_model_for_sidecar_provider(
    provider: &LLMProviderAPIKeys,
    fast_model: LLMType,
) -> LLMType {
    if matches!(provider, LLMProviderAPIKeys::CodeStory(_)) {
        if fast_model.is_togetherai_model() {
            fast_model
        } else {
            LLMType::CodeLlama13BInstruct
        }
    } else {
        fast_model
    }
}

pub fn should_track_file(document_path: &str) -> bool {
    if document_path.contains(".rustup") {
        return false;
    }
    if document_path.contains(".cargo") {
        return false;
    }
    true
}

// check if a string is all whitespaces
pub fn is_whitespace(s: &str) -> bool {
    s.trim().is_empty()
}

pub fn get_indentation_string(s: &str) -> String {
    let mut indentation = "".to_owned();
    for c in s.chars() {
        if c == ' ' {
            indentation += " ";
        } else if c == '\t' {
            indentation += "\t";
        } else {
            break;
        }
    }
    indentation
}
