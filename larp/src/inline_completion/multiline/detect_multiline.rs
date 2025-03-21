//! We try to detect if we can trigger a multiline completion over here
//! Some pointers we can use: https://github.com/sourcegraph/cody/blob/3cd602dce56b96f42be3f31183c31980ea616ab4/vscode/src/completions/detect-multiline.ts#L57

use std::sync::Arc;

use crate::{
    chunking::{editor_parsing::EditorParsing, text_document::Position},
    inline_completion::helpers::split_on_lines_editor_compatiable,
};

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // /^(function|def|fn)/g
    static ref FUNCTION_KEYWORDS: Vec<String> = vec!["function".to_owned(), "def".to_owned(), "fn".to_owned()];
    static ref FUNCTION_OR_METHOD_INVOCATION_REGEX: Regex = Regex::new(r"\b[^()]+\((.*)\)$").unwrap();
    static ref BLOCK_START: Regex = Regex::new(r"([(\[{])$").unwrap();
}

struct PrefixAndSuffx {
    prefix: String,
    suffix: String,
}

fn current_line_prefix_and_suffix(line: &str, charcter_position: usize) -> PrefixAndSuffx {
    let characters = line
        .chars()
        .into_iter()
        .map(|char| char.to_string())
        .collect::<Vec<_>>();
    let prefix = characters[0..charcter_position].join("");
    let suffix = characters[charcter_position..].join("");
    PrefixAndSuffx { prefix, suffix }
}

/// Given a line, this function will return the indentation level of the line
///
/// # Arguments
///
/// * `line` - The line to get the indentation level of
/// * `tab_size` - The size of a tab
///
/// # Returns
///
/// The indentation level of the line
fn indentation(line: Option<String>, tab_size: usize) -> usize {
    if line.is_none() {
        return 0;
    }
    let line = line.expect("if is_none to hold");
    let mut count = 0;
    for c in line.chars() {
        match c {
            ' ' => count += 1,
            '\t' => count += tab_size,
            _ => break,
        }
    }
    count
}

fn next_non_empty_line(lines: &Vec<String>, line_number: usize) -> Option<String> {
    // check the line numbers after the current one which are non-empty
    let next_line_number = line_number + 1;
    for line in lines[next_line_number..].iter() {
        if !line.is_empty() {
            return Some(line.to_owned());
        }
    }
    None
}

pub fn is_multiline_completion(
    cursor_position: Position,
    file_content: String,
    editor_parsing: Arc<EditorParsing>,
    file_path: &str,
) -> bool {
    let language_config = editor_parsing.for_file_path(file_path);
    let block_start = language_config
        .map(|lang| lang.block_start.as_ref())
        .flatten();
    if let None = block_start {
        return false;
    }
    let block_start = block_start.expect("if let None above to work");
    let lines = split_on_lines_editor_compatiable(&file_content);
    let next_non_empty_line = next_non_empty_line(&lines, cursor_position.line());
    let previous_non_empty_line = if cursor_position.line() == 0 {
        None
    } else {
        Some(lines[cursor_position.line() - 1].to_owned())
    };
    let prefix_and_suffix =
        current_line_prefix_and_suffix(&lines[cursor_position.line()], cursor_position.column());
    let prefix = &prefix_and_suffix.prefix;
    let suffix = &prefix_and_suffix.suffix;
    let ends_with_block_start = prefix.trim_end().ends_with(block_start);

    let is_current_line_invocation = !prefix.trim().is_empty()
        && BLOCK_START.is_match(prefix.trim_end())
        && (indentation(Some(prefix.to_owned()), 1) >= indentation(next_non_empty_line.clone(), 1));

    let is_new_line_opening_bracket = prefix.trim().is_empty()
        && suffix.trim().is_empty()
        && BLOCK_START.is_match(prefix.trim_end())
        // now we do the indentation checks for the previous line and make sure that we are in a block
        // so we have a higher indentation at the current line
        && {
            match previous_non_empty_line.as_ref() {
                Some(previous_non_empty_line) => {
                    indentation(Some(previous_non_empty_line.to_owned()), 1) < indentation(Some(prefix.to_owned()), 1)
                }
                None => true,
            }
        }
        // now we check the next line if it has a greater indentation than the current line
        && {
            match next_non_empty_line.as_ref() {
                Some(next_non_empty_line) => {
                    indentation(Some(next_non_empty_line.to_owned()), 1) > indentation(Some(prefix.to_owned()), 1)
                }
                None => true,
            }
        };

    if is_current_line_invocation || is_new_line_opening_bracket {
        return true;
    }

    let is_non_empty_line_ends_with_block_start = !prefix.is_empty() && ends_with_block_start && {
        match next_non_empty_line.as_ref() {
            Some(next_non_empty_line) => {
                indentation(Some(next_non_empty_line.to_owned()), 1)
                    <= indentation(Some(prefix.to_owned()), 1)
            }
            None => true,
        }
    };
    let is_empty_line_after_block_start =
        prefix.is_empty() && suffix.is_empty() && ends_with_block_start && {
            let indentation_check = {
                match previous_non_empty_line {
                    Some(previous_non_empty_line) => {
                        indentation(Some(previous_non_empty_line), 1)
                            < indentation(Some(prefix.to_owned()), 1)
                    }
                    None => true,
                }
            }
            // now we check the next line if it has a greater indentation than the current line
            &&{
                match next_non_empty_line.as_ref() {
                    Some(next_non_empty_line) => {
                        indentation(Some(next_non_empty_line.to_owned()), 1)
                            > indentation(Some(prefix.to_owned()), 1)
                    }
                    None => true,
                }
            };
            indentation_check
        };
    if is_non_empty_line_ends_with_block_start || is_empty_line_after_block_start {
        true
    } else {
        false
    }
}
