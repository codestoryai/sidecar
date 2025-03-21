//! This generates context from the current file
//! We are not going for grandiose limits right now and will start here

use std::sync::Arc;

use llm_client::{clients::types::LLMType, tokenizer::tokenizer::LLMTokenizer};
use tracing::info;

use crate::{
    chunking::{
        editor_parsing::EditorParsing,
        text_document::{Position, Range},
    },
    inline_completion::{
        context::types::{CodeSelection, DocumentLines},
        types::InLineCompletionError,
    },
};

use super::types::CurrentFilePrefixSuffix;

// Grabs the current file context from the cursor position
pub struct CurrentFileContext {
    file_path: String,
    cursor_position: Position,
    token_limit: usize,
    tokenizer: Arc<LLMTokenizer>,
    editor_parsing: Arc<EditorParsing>,
    llm_type: LLMType,
}

impl CurrentFileContext {
    pub fn new(
        file_path: String,
        cursor_position: Position,
        token_limit: usize,
        tokenizer: Arc<LLMTokenizer>,
        editor_parsing: Arc<EditorParsing>,
        llm_type: LLMType,
    ) -> Self {
        Self {
            file_path,
            cursor_position,
            token_limit,
            tokenizer,
            llm_type,
            editor_parsing,
        }
    }

    pub fn generate_context(
        mut self,
        document_lines: &DocumentLines,
    ) -> Result<CurrentFilePrefixSuffix, InLineCompletionError> {
        let current_line_number = self.cursor_position.line() as usize;
        // Get the current line's content from the cursor position
        // TODO: log here if we get the spaces and tabs at the start of it
        info!(
            event_name = "current_line_content",
            content = document_lines.get_line(current_line_number),
        );
        let current_line = document_lines.get_line(current_line_number);
        let prefix_line_part = current_line[..self.cursor_position.column() as usize].to_owned();
        let suffix_line_part = current_line[self.cursor_position.column() as usize..].to_owned();

        // Now we get the prefix end part and the suffix start part
        let prefix_end_part = self.cursor_position.clone();
        let suffix_start_part = self.cursor_position.clone();
        // we want to get the current line prefix and the suffix here
        // so we can send it over to the LLM
        // reduce our token limit by the current line's token count
        let current_line_token_count = self
            .tokenizer
            .count_tokens_using_tokenizer(&self.llm_type, current_line)?;
        self.token_limit -= current_line_token_count;

        // we also want to reserve some tokens for the current path we are in
        // as well as comment
        let mut possible_file_path = None;
        let language_config = self.editor_parsing.for_file_path(&self.file_path).ok_or(
            InLineCompletionError::NoLanguageConfiguration(self.file_path.to_owned()),
        )?;
        let comment_style = language_config.comment_prefix.to_owned();
        let file_path = &self.file_path.to_owned();
        let file_path_content = format!("{comment_style} {file_path}");
        let file_path_token_count = self
            .tokenizer
            .count_tokens_using_tokenizer(&self.llm_type, &file_path_content)?;
        if file_path_token_count <= self.token_limit {
            possible_file_path = Some(file_path_content);
            self.token_limit -= file_path_token_count;
        }

        // First get the current line's content from the cursor position
        // we need to keep track of the position as well, since its important
        // metadata
        // expand until we hit the token limit
        let mut prefix = vec![];
        let mut suffix = vec![];
        let mut current_token_count = 0;

        let mut iteration_number = 0;
        let mut prefix_line: i64 = current_line_number as i64 - 1;
        let mut suffix_line: i64 = current_line_number as i64 + 1;
        while current_token_count < self.token_limit
            && (prefix_line >= 0 || suffix_line < document_lines.len() as i64)
        {
            // we take in the 3:1 ratio, so we prefer strings from the prefix
            // more over strings from the suffix
            if iteration_number % 4 != 0 {
                if prefix_line >= 0 {
                    let line = document_lines.get_line(prefix_line as usize);
                    let tokens = self
                        .tokenizer
                        .count_tokens_using_tokenizer(&self.llm_type, line)?;
                    if current_token_count + tokens > self.token_limit {
                        break;
                    }
                    current_token_count += tokens;
                    prefix.push(line.to_owned());
                    prefix_line -= 1;
                }
            } else {
                if suffix_line < document_lines.len() as i64 {
                    let line = document_lines.get_line(suffix_line as usize);
                    let tokens = self
                        .tokenizer
                        .count_tokens_using_tokenizer(&self.llm_type, line)?;
                    if current_token_count + tokens > self.token_limit {
                        break;
                    }
                    current_token_count += tokens;
                    suffix.push(line.to_owned());
                    suffix_line += 1;
                }
            }
            iteration_number = iteration_number + 1;
        }

        prefix.reverse();
        let prefix_without_current_line = prefix.to_vec();
        // push the current line content to the prefix
        prefix.push(prefix_line_part.to_owned());
        // now check if we have a possible file path,
        // this should only happen if the line in prefix does not starts with
        //  not a ' ' or '/t'
        if let Some(file_path) = possible_file_path {
            if !prefix[0].starts_with(' ') && !prefix[0].starts_with('\t') {
                // TODO(skcd): This can get expensive cause we are reshuffling the array
                prefix.insert(0, file_path);
            }
        }

        // Prefix has the following properties:
        // we keep track of the line number which we want to insert so its always
        // the line number - 1 of the prefix we have in our content
        // to get the start position we need to get: (prefix_line + 1).start_position
        // line 1 <- prefix_line will be here
        // [prefix_line_we_need]==== INSIDE PREFIX ====
        // line 2
        // line 3
        // ..
        // line n ... [cursor_line -1.end()]
        let prefix = CodeSelection::new(
            Range::new(
                document_lines.start_position_at_line((prefix_line + 1) as usize),
                document_lines.end_position_at_line(prefix_end_part.line()),
            ),
            self.file_path.clone(),
            prefix.join("\n"),
        );

        // Suffix has the following properties:
        // we keep track of the line number which we have to insert to the slot
        // so its always the line number + 1 of the suffix we have in our content
        // to get the start position we need to get: (suffix_line - 1).start_position
        // line 1
        // line 2
        // lin[cursor_position]e 3
        // ..
        // line n ... [cursor_line + 1.start()]
        // only insert the suffix part here if its not empty
        if suffix_line_part != "" {
            suffix.insert(0, suffix_line_part);
        }
        let suffix = CodeSelection::new(
            Range::new(
                document_lines.start_position_at_line(suffix_start_part.line()),
                document_lines.end_position_at_line((suffix_line - 1) as usize),
            ),
            self.file_path.clone(),
            suffix.join("\n"),
        );

        // Now that we have the prefix and the suffix we should join them together
        // and get the values for the prompt, we should also store some metadata here
        // about the segments of code we are sending (can be useful for debugging + enriching)
        Ok(CurrentFilePrefixSuffix::new(
            prefix,
            suffix,
            prefix_without_current_line.join("\n"),
            prefix_line_part.to_owned(),
        ))
    }
}
