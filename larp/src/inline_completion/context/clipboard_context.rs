use std::sync::Arc;

use llm_client::{clients::types::LLMType, tokenizer::tokenizer::LLMTokenizer};

use crate::{
    chunking::editor_parsing::EditorParsing, inline_completion::types::InLineCompletionError,
};

/// The Clipboard context helps truncate the context in the clipboard into the limit of the
/// tokenizer.
pub struct ClipboardContext {
    clipboard_context: String,
    tokenizer: Arc<LLMTokenizer>,
    llm_type: LLMType,
    editor_parsing: Arc<EditorParsing>,
    file_path: String,
}

/// The Clipboard context helps truncate the context in the clipboard into the limit of the
/// tokenizer.
#[derive(Debug, Clone)]
pub enum ClipboardContextString {
    // this contains the nubmers of tokens which has been used
    TruncatedToLimit(String, i64),
    UnableToTruncate,
}

impl ClipboardContext {
    pub fn new(
        clipboard_context: String,
        tokenizer: Arc<LLMTokenizer>,
        llm_type: LLMType,
        editor_parsing: Arc<EditorParsing>,
        file_path: String,
    ) -> Self {
        Self {
            clipboard_context,
            tokenizer,
            llm_type,
            editor_parsing,
            file_path,
        }
    }

    pub fn get_clipboard_context(
        &self,
        token_limit: usize,
    ) -> Result<ClipboardContextString, InLineCompletionError> {
        let language_config = self.editor_parsing.for_file_path(&self.file_path).ok_or(
            InLineCompletionError::NoLanguageConfiguration(self.file_path.to_owned()),
        )?;
        let comment_style = language_config.comment_prefix.to_owned();
        let tokenizer = self.tokenizer.tokenizers.get(&self.llm_type).ok_or(
            InLineCompletionError::TokenizerNotFound(self.llm_type.to_owned()),
        )?;
        // always include the comment hearder here
        let completion_string = format!("{comment_style} Clipboard:\n");
        let comment_tokens_used = tokenizer
            .encode(completion_string, false)
            .map_err(|_e| InLineCompletionError::TokenizationError(self.llm_type.to_owned()))?;
        if comment_tokens_used.len() >= token_limit {
            return Ok(ClipboardContextString::UnableToTruncate);
        }
        let mut answer = ClipboardContextString::UnableToTruncate;

        // Now we try to add some prefix to this from the clipboard context and see if can fit any
        let mut string_up_until_now = "".to_owned();
        for line in self.clipboard_context.lines() {
            // check if the line has a whitespace at the start
            if line
                .chars()
                .next()
                .map(|char| char.is_whitespace())
                .unwrap_or_default()
            {
                string_up_until_now = string_up_until_now + &format!("\n{comment_style}{line}");
            } else {
                string_up_until_now = string_up_until_now + &format!("\n{comment_style} {line}");
            }
            let completion_string = format!(
                r#"{comment_style} Clipboard:
{string_up_until_now}"#
            );
            let tokens_used = tokenizer
                // we pay the const twice here, once for copying the completion_string here and
                // another time for sending it over
                .encode(completion_string.to_owned(), false)
                .map_err(|_e| InLineCompletionError::TokenizationError(self.llm_type.to_owned()))?;
            if tokens_used.len() >= token_limit {
                break;
            } else {
                answer = ClipboardContextString::TruncatedToLimit(
                    completion_string,
                    tokens_used.len() as i64,
                );
            }
        }
        Ok(answer)
    }
}
