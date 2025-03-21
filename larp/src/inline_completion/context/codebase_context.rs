use std::{collections::HashMap, sync::Arc};

use futures::stream::AbortHandle;
use llm_client::{
    clients::types::LLMType,
    tokenizer::tokenizer::{LLMTokenizer, LLMTokenizerInput},
};

use crate::{
    chunking::{editor_parsing::EditorParsing, text_document::Position},
    inline_completion::{
        document::content::SnippetInformationWithScore, symbols_tracker::SymbolTrackerInline,
        types::InLineCompletionError,
    },
};

/// Creates the codebase context which we want to use
/// for generating inline-completions
pub struct CodeBaseContext {
    tokenizer: Arc<LLMTokenizer>,
    llm_type: LLMType,
    file_path: String,
    file_content: String,
    cursor_position: Position,
    symbol_tracker: Arc<SymbolTrackerInline>,
    editor_parsing: Arc<EditorParsing>,
    _request_id: String,
}

/// The Codebase context helps truncate the context in the clipboard into the limit of the
/// tokenizer.
#[derive(Debug, Clone)]
pub enum CodebaseContextString {
    TruncatedToLimit(String, i64),
    UnableToTruncate,
}

impl CodebaseContextString {
    pub fn get_prefix_with_tokens(self) -> Option<(String, i64)> {
        match self {
            CodebaseContextString::TruncatedToLimit(prefix, used_tokens) => {
                Some((prefix, used_tokens))
            }
            CodebaseContextString::UnableToTruncate => None,
        }
    }
}

impl CodeBaseContext {
    pub fn new(
        tokenizer: Arc<LLMTokenizer>,
        llm_type: LLMType,
        file_path: String,
        file_content: String,
        cursor_position: Position,
        symbol_tracker: Arc<SymbolTrackerInline>,
        editor_parsing: Arc<EditorParsing>,
        request_id: String,
    ) -> Self {
        Self {
            tokenizer,
            llm_type,
            file_path,
            file_content,
            cursor_position,
            symbol_tracker,
            editor_parsing,
            _request_id: request_id,
        }
    }

    pub fn get_context_window_from_current_file(&self) -> String {
        let current_line = self.cursor_position.line();
        let lines = self.file_content.lines().collect::<Vec<_>>();
        let start_line = if current_line >= 50 {
            current_line - 50
        } else {
            0
        };
        let end_line = current_line;
        let context_lines = lines[start_line..end_line].join("\n");
        context_lines
    }

    pub async fn generate_context(
        &self,
        token_limit: usize,
        abort_handle: AbortHandle,
    ) -> Result<CodebaseContextString, InLineCompletionError> {
        let language_config = self.editor_parsing.for_file_path(&self.file_path).ok_or(
            InLineCompletionError::LanguageNotSupported("not_supported".to_owned()),
        )?;
        let current_window_context = self.get_context_window_from_current_file();
        // Now we try to get the context from the symbol tracker
        let history_files = self.symbol_tracker.get_document_history().await;
        // since these history files are sorted in the order of priority, we can
        // safely assume that the first one is the most recent one

        let mut relevant_snippets: Vec<SnippetInformationWithScore> = vec![];
        // println!(
        //     "history files: {:?}",
        //     history_files
        //         .iter()
        //         .map(|history_file| history_file.to_owned())
        //         .collect::<Vec<_>>()
        // );
        // println!("current file: {}", &self.file_path);
        // println!("current window context: {}", &current_window_context);
        // TODO(skcd): hate hate hate, but there's a mutex lock so this is fine ❤️‍🔥
        for history_file in history_files.into_iter() {
            let skip_line = if history_file == self.file_path {
                Some(self.cursor_position.line())
            } else {
                None
            };
            if abort_handle.is_aborted() {
                return Err(InLineCompletionError::AbortedHandle);
            }
            let snippet_information = self
                .symbol_tracker
                .get_document_lines(&history_file, &current_window_context, skip_line)
                .await;
            if let Some(mut snippet_information) = snippet_information {
                relevant_snippets.append(&mut snippet_information);
            }
        }
        relevant_snippets.sort_by(|a, b| {
            b.score()
                .partial_cmp(&a.score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        //         println!("======================================");
        //         println!("{}", &relevant_snippets.len());
        //         relevant_snippets
        //             .iter()
        //             .enumerate()
        //             .for_each(|(idx, snippet)| {
        //                 let file_path = snippet.file_path();
        //                 let content = snippet.snippet_information().snippet();
        //                 let printable_snippet = format!(
        //                     r#"<code_snippet>
        // <id>{idx}</id>
        // <file_path>{file_path}</file_path>
        // <content>
        // {content}
        // </content>
        // </code_snippet>"#
        //                 );
        //                 println!("{}", printable_snippet);
        //             });
        //         println!("======================================");

        // Now that we have the relevant snippets we can generate the context
        let comment_prefix = &language_config.comment_prefix;
        let mut running_context: Vec<String> = vec![];
        let mut inlcuded_snippet_from_files: HashMap<String, usize> = HashMap::new();
        let mut total_tokens_used_by_snippets = 0;
        for snippet in relevant_snippets {
            let file_path = snippet.file_path();
            let current_count: usize =
                *inlcuded_snippet_from_files.get(file_path).unwrap_or(&0) + 1;
            inlcuded_snippet_from_files.insert(file_path.to_owned(), current_count);

            // we have a strict limit of 10 snippets from each file, if we exceed that we break
            // this prevents a big file from putting in too much context
            if current_count > 10 {
                continue;
            }
            let snippet_context = snippet
                .snippet_information()
                .snippet()
                .split("\n")
                .map(|snippet| format!("{} {}", comment_prefix, snippet))
                .collect::<Vec<_>>()
                .join("\n");
            let file_path_header = format!("{} Path: {}", comment_prefix, file_path,);
            let joined_snippet_context = format!("{}\n{}", file_path_header, snippet_context);
            let current_snippet_tokens = self.tokenizer.count_tokens_approx(
                &self.llm_type,
                LLMTokenizerInput::Prompt(joined_snippet_context.to_owned()),
                // adding + 1 here for the \n at the end
            )? + 1;
            total_tokens_used_by_snippets = total_tokens_used_by_snippets + current_snippet_tokens;
            running_context.push(joined_snippet_context);
            let current_context = running_context.join("\n");

            // This is a compute intensive operation, so we want to abort if we are aborted
            if abort_handle.is_aborted() {
                return Err(InLineCompletionError::AbortedHandle);
            }
            if total_tokens_used_by_snippets > token_limit {
                return Ok(CodebaseContextString::TruncatedToLimit(
                    current_context,
                    total_tokens_used_by_snippets as i64,
                ));
            }
        }

        let prefix_context = running_context.join(&format!("\n{comment_prefix}\n"));
        let used_tokens_for_prefix = self.tokenizer.count_tokens(
            &self.llm_type,
            LLMTokenizerInput::Prompt(prefix_context.to_owned()),
        )?;
        Ok(CodebaseContextString::TruncatedToLimit(
            prefix_context,
            used_tokens_for_prefix as i64,
        ))
    }
}
