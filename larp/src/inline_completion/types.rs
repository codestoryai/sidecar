use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use futures::stream::AbortHandle;
use futures::{stream, StreamExt};
use futures::{FutureExt, Stream};
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    tokenizer::tokenizer::{LLMTokenizer, LLMTokenizerError},
};
use llm_prompts::fim::types::FillInMiddleRequest;
use llm_prompts::{answer_model::LLMAnswerModelBroker, fim::types::FillInMiddleBroker};

use crate::chunking::languages::TSLanguageConfig;
use crate::chunking::text_document::Range;
use crate::chunking::types::OutlineNode;
use crate::inline_completion::context::clipboard_context::{
    ClipboardContext, ClipboardContextString,
};
use crate::inline_completion::helpers::{fix_model_for_sidecar_provider, get_indentation_string};
use crate::{
    chunking::editor_parsing::EditorParsing,
    webserver::inline_completion::{
        InlineCompletion, InlineCompletionRequest, InlineCompletionResponse,
    },
};

use super::context::codebase_context::CodeBaseContext;
use super::symbols_tracker::SymbolTrackerInline;
use super::{
    context::{current_file::CurrentFileContext, types::DocumentLines},
    helpers::insert_range,
};

const CLIPBOARD_CONTEXT: usize = 50;
const CODEBASE_CONTEXT: usize = 3000;
const ANTHROPIC_CODEBASE_CONTEXT: usize = 5_000;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeIdentifierPosition {
    line: usize,
    character: usize,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeIdentifierRange {
    start: TypeIdentifierPosition,
    end: TypeIdentifierPosition,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeIdentifiersNode {
    identifier: String,
    range: TypeIdentifierRange,
}

impl TypeIdentifiersNode {
    pub fn identifier(&self) -> &str {
        &self.identifier
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeIdentifierDefinitionPosition {
    file_path: String,
    range: TypeIdentifierRange,
}

impl TypeIdentifierDefinitionPosition {
    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    fn check_inside_or_outside(&self, range: &Range) -> bool {
        // check if the range for this goto-definition is contained within
        // the outline
        let start_position = range.start_position();
        let end_position = range.end_position();
        let range_start = &self.range.start;
        let range_end = &self.range.end;
        if (start_position.line() <= range_start.line
            || (start_position.line() == range_start.line
                && start_position.column() <= range_start.character))
            && (end_position.line() >= range_end.line
                || (end_position.line() == range_end.line
                    && end_position.column() >= range_end.character))
        {
            true
        } else {
            if (range_start.line <= start_position.line()
                || (range_start.line == start_position.line()
                    && range_start.character <= start_position.column()))
                && (range_end.line >= end_position.line()
                    || (range_end.line == end_position.line()
                        && range_end.character >= end_position.column()))
            {
                true
            } else {
                false
            }
        }
    }

    pub fn get_outline(
        &self,
        outline_nodes: &[OutlineNode],
        language_config: &TSLanguageConfig,
    ) -> Option<String> {
        let filtered_outline_nodes = outline_nodes
            .iter()
            .filter(|outline_node| {
                // check if the range for this goto-definition is contained within
                // the outline or completely outside the outline
                if self.check_inside_or_outside(outline_node.range()) {
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();

        // we are not done yet, we have to also include the nodes which might be
        // part of the implementation of a given struct, so we go for another pass
        // and look at class like objects and grab their implementation context as well
        // ideally we should be getting just a single filtered outline nodes
        let final_outline_nodes = outline_nodes
            .iter()
            .filter(|outline_node| outline_node.is_class())
            .filter_map(|outline_node| {
                let node_name = outline_node.name();
                let name_matches = filtered_outline_nodes
                    .iter()
                    .any(|filtered_outline_node| filtered_outline_node.name() == node_name);
                if name_matches {
                    Some(outline_node)
                } else {
                    None
                }
            })
            .filter_map(|outline_node| outline_node.get_outline())
            .collect::<Vec<_>>();
        if final_outline_nodes.is_empty() {
            None
        } else {
            let comment_prefix = &language_config.comment_prefix;
            let file_path = self.file_path();
            let outline_content = final_outline_nodes
                .join("\n")
                .lines()
                .map(|line| format!("{comment_prefix} {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            // we have to add the filepath at the start and include the outline
            // which we have generated
            Some(format!(
                r#"{comment_prefix} File Path: {file_path}
{outline_content}"#
            ))
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TypeIdentifier {
    node: TypeIdentifiersNode,
    type_definitions: Vec<TypeIdentifierDefinitionPosition>,
    node_type: NodeType,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
/// These types are mapped out in typescript, so we get it from there
pub enum NodeType {
    Identifier,
    FunctionParameter,
    ImportNode,
}

impl TypeIdentifier {
    pub fn node(&self) -> &TypeIdentifiersNode {
        &self.node
    }

    pub fn type_definitions(&self) -> &[TypeIdentifierDefinitionPosition] {
        self.type_definitions.as_slice()
    }
}

#[derive(Debug, Clone)]
pub struct FillInMiddleError {
    _error_count: usize,
    _missing_count: usize,
}

impl FillInMiddleError {
    pub fn new(error_count: usize, missing_count: usize) -> Self {
        Self {
            _error_count: error_count,
            _missing_count: missing_count,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FillInMiddleStreamContext {
    prefix_at_cursor_position: String,
    next_non_empty_line: Option<String>,
}

impl FillInMiddleStreamContext {
    fn new(prefix_at_cursor_position: String, next_non_empty_line: Option<String>) -> Self {
        Self {
            prefix_at_cursor_position,
            next_non_empty_line,
        }
    }
}

pub struct FillInMiddleCompletionAgent {
    llm_broker: Arc<LLMBroker>,
    llm_tokenizer: Arc<LLMTokenizer>,
    fill_in_middle_broker: Arc<FillInMiddleBroker>,
    editor_parsing: Arc<EditorParsing>,
    answer_mode: Arc<LLMAnswerModelBroker>,
    symbol_tracker: Arc<SymbolTrackerInline>,
}

#[derive(thiserror::Error, Debug)]
pub enum InLineCompletionError {
    #[error("LLM type {0} is not supported for inline completion.")]
    LLMNotSupported(LLMType),

    #[error("Language Not supported: {0}")]
    LanguageNotSupported(String),

    #[error("tokenizer formatting error: {0}")]
    LLMTokenizerError(#[from] llm_client::format::types::TokenizerError),

    #[error("tokenizer error: {0}")]
    LLMTokenizationError(#[from] LLMTokenizerError),

    #[error("No language configuration found for path: {0}")]
    NoLanguageConfiguration(String),

    #[error("Fill in middle error: {0}")]
    FillInMiddleError(#[from] llm_prompts::fim::types::FillInMiddleError),

    #[error("Missing provider keys: {0}")]
    MissingProviderKeys(LLMType),

    #[error("LLMClient error: {0}")]
    LLMClientError(#[from] llm_client::clients::types::LLMClientError),

    #[error("terminated streamed completion")]
    InlineCompletionTerminated,

    #[error("Tokenizer not found: {0}")]
    TokenizerNotFound(LLMType),

    #[error("Tokenization error: {0}")]
    TokenizationError(LLMType),

    #[error("Prefix not found for the cursor position")]
    PrefixNotFound,

    #[error("Suffix not found for cursor position")]
    SuffixNotFound,

    #[error("Aborted the handle")]
    AbortedHandle,
}

impl FillInMiddleCompletionAgent {
    pub fn new(
        llm_broker: Arc<LLMBroker>,
        llm_tokenizer: Arc<LLMTokenizer>,
        answer_mode: Arc<LLMAnswerModelBroker>,
        fill_in_middle_broker: Arc<FillInMiddleBroker>,
        editor_parsing: Arc<EditorParsing>,
        symbol_tracker: Arc<SymbolTrackerInline>,
    ) -> Self {
        Self {
            llm_broker,
            llm_tokenizer,
            answer_mode,
            fill_in_middle_broker,
            editor_parsing,
            symbol_tracker,
        }
    }

    pub async fn completion(
        &self,
        completion_request: InlineCompletionRequest,
        abort_handle: AbortHandle,
        _request_start: Instant,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<InlineCompletionResponse, InLineCompletionError>> + Send>>,
        InLineCompletionError,
    > {
        let request_id = completion_request.id.to_owned();
        // Now that we have the position, we want to create the request for the fill
        // in the middle request.
        let model_config = &completion_request.model_config;
        // If we are using the codestory provider, use the only model compatible with the codestory
        // provider.
        let fast_model = match model_config.provider_for_fast_model() {
            Some(provider) => {
                fix_model_for_sidecar_provider(provider, model_config.fast_model.clone())
            }
            None => model_config.fast_model.clone(),
        };
        let fast_model_api_key = model_config
            .provider_for_fast_model()
            .ok_or(InLineCompletionError::MissingProviderKeys(
                fast_model.clone(),
            ))?
            .clone();
        let model_config = self.answer_mode.get_answer_model(&fast_model);
        if let None = model_config {
            return Err(InLineCompletionError::LLMNotSupported(fast_model));
        }
        let model_config = model_config.expect("if let None holds");
        let token_limit = model_config.inline_completion_tokens;
        if let None = token_limit {
            return Err(InLineCompletionError::LLMNotSupported(fast_model));
        }
        let mut token_limit = token_limit.expect("if let None to hold");

        let document_lines = DocumentLines::from_file_content(&completion_request.text);

        if abort_handle.is_aborted() {
            return Err(InLineCompletionError::AbortedHandle);
        }

        let mut prefix = None;
        if let Some(completion_context) = completion_request.clipboard_content {
            let clipboard_context = ClipboardContext::new(
                completion_context,
                self.llm_tokenizer.clone(),
                fast_model.clone(),
                self.editor_parsing.clone(),
                completion_request.filepath.to_owned(),
            )
            .get_clipboard_context(CLIPBOARD_CONTEXT)?;
            match clipboard_context {
                ClipboardContextString::TruncatedToLimit(
                    clipboard_context,
                    clipboard_tokens_used,
                ) => {
                    token_limit = token_limit - clipboard_tokens_used;
                    prefix = Some(clipboard_context);
                }
                _ => {}
            }
        };

        // Now we are going to get the codebase context
        let codebase_context = CodeBaseContext::new(
            self.llm_tokenizer.clone(),
            fast_model.clone(),
            completion_request.filepath.to_owned(),
            completion_request.text.to_owned(),
            completion_request.position.clone(),
            self.symbol_tracker.clone(),
            self.editor_parsing.clone(),
            request_id.to_owned(),
        )
        .generate_context(
            if fast_model.is_anthropic() {
                ANTHROPIC_CODEBASE_CONTEXT
            } else {
                CODEBASE_CONTEXT
            },
            abort_handle.clone(),
        )
        .await?
        .get_prefix_with_tokens();
        match codebase_context {
            Some((codebase_prefix, used_tokens)) => {
                token_limit = token_limit - used_tokens;
                if let Some(previous_prefix) = prefix {
                    prefix = Some(format!("{}\n{}", previous_prefix, codebase_prefix));
                } else {
                    prefix = Some(codebase_prefix);
                }
            }
            None => {}
        }

        // TODO(skcd): We should replace this with making the call from the sidecar
        // to the editor directly instead of proxying it like this and getting the data
        // back on the request
        let definitions_context = self
            .symbol_tracker
            .get_definition_configs(
                &completion_request.filepath,
                completion_request.type_identifiers,
                self.editor_parsing.clone(),
            )
            .await;
        if !definitions_context.is_empty() {
            if let Some(previous_prefix) = prefix {
                prefix = Some(format!(
                    "{}\n{}",
                    previous_prefix,
                    definitions_context.join("\n")
                ));
            } else {
                prefix = Some(definitions_context.join("\n"))
            }
        }
        // TODO(skcd): Can we also grab the context from other functions which might be useful for the completion.
        // TODO(skcd): We also want to grab the recent edits which might be useful for the completion.

        // Now we are going to grab the current line prefix
        let cursor_prefix = Arc::new(FillInMiddleStreamContext::new(
            document_lines.prefix_at_line(completion_request.position)?,
            document_lines.next_non_empty_line(completion_request.position),
        ));

        // Now we generate the prefix and the suffix here
        let completion_context = CurrentFileContext::new(
            completion_request.filepath,
            completion_request.position,
            token_limit as usize,
            self.llm_tokenizer.clone(),
            self.editor_parsing.clone(),
            fast_model.clone(),
        )
        .generate_context(&document_lines)?;

        let stop_words = model_config
            .get_stop_words_inline_completion()
            .unwrap_or_default();

        // We are keeping the current line to use it for later
        let current_line = completion_context.current_line_content.to_owned();
        // with anthropic models we have an issue with the way completions
        // are generated. If it has only whitespace then the model also
        // generates the whitespace and indents things properly
        // if there is content before on the line, then the indentation is broken
        // completely, which sucks
        // so what we want to do is the following:
        // - remove the indentation of the current line since it will always
        // be from the cursor position
        // - and add indentation to the followup lines since those require it
        // as its lost otherwise
        let is_current_line_whitespace = current_line.trim().is_empty();
        let current_line_indentation = get_indentation_string(&current_line);

        let llm_request = self.fill_in_middle_broker.format_context(
            match prefix {
                Some(prefix) => FillInMiddleRequest::new(
                    format!(
                        "{}\n{}",
                        prefix,
                        if fast_model.is_anthropic() {
                            completion_context.prefix_without_current_line.to_owned()
                        } else {
                            completion_context.prefix.content().to_owned()
                        }
                    ),
                    completion_context.suffix.content().to_owned(),
                    fast_model.clone(),
                    stop_words,
                    model_config.inline_completion_tokens,
                    completion_context.current_line_content.to_owned(),
                    is_current_line_whitespace,
                    current_line_indentation.to_owned(),
                ),
                None => FillInMiddleRequest::new(
                    if fast_model.is_anthropic() {
                        completion_context.prefix_without_current_line.to_owned()
                    } else {
                        completion_context.prefix.content().to_owned()
                    },
                    completion_context.suffix.content().to_owned(),
                    fast_model.clone(),
                    stop_words,
                    model_config.inline_completion_tokens,
                    completion_context.current_line_content.to_owned(),
                    is_current_line_whitespace,
                    current_line_indentation.to_owned(),
                ),
            },
            &fast_model,
        )?;

        let arced_document_lines = Arc::new(document_lines);

        // Now we send a request over to our provider and get a response for this
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        // convert this to a stream where we are yielding new lines
        let completion_receiver_stream =
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver).map(either::Left);
        // pin_mut!(merged_stream);

        let llm_broker = self.llm_broker.clone();
        let should_end_stream = Arc::new(std::sync::Mutex::new(false));
        Ok(Box::pin({
            let cursor_prefix = cursor_prefix.clone();
            let should_end_stream = should_end_stream.clone();
            // ugly, ugly, ugly, but type-safe so yay :))
            let completion = LLMBroker::stream_string_completion_owned(
                llm_broker,
                fast_model_api_key,
                llm_request,
                vec![("event_type".to_owned(), "fill_in_middle".to_owned())]
                    .into_iter()
                    .collect(),
                sender,
                if fast_model.is_anthropic() {
                    Some("<code_inserted>".to_owned())
                } else {
                    None
                },
                is_current_line_whitespace,
                current_line_indentation,
                fast_model.clone(),
            )
            .into_stream()
            .map(either::Right);

            let merged_stream = stream::select(completion_receiver_stream, completion);
            merged_stream
                .map(move |item| {
                    (
                        item,
                        arced_document_lines.clone(),
                        cursor_prefix.clone(),
                        should_end_stream.clone(),
                        fast_model.clone(),
                    )
                })
                .map(
                    move |(item, document_lines, cursor_prefix, should_end_stream, fast_model)| {
                        match item {
                            either::Left(response) => Ok((
                                InlineCompletionResponse::new(vec![InlineCompletion::new(
                                    // TODO(skcd): Remove this later on, we are testing it out over here
                                    response.answer_up_until_now().to_owned(),
                                    insert_range(
                                        completion_request.position,
                                        &document_lines,
                                        response.answer_up_until_now(),
                                    ),
                                    response.delta().map(|v| v.to_owned()),
                                )]),
                                cursor_prefix.clone(),
                                should_end_stream.clone(),
                                fast_model,
                            )),
                            either::Right(Ok(response)) => {
                                // for anthropic models we do not want to look
                                // at the final answer and process it, unlike
                                // other providers we get a weird </code_inserted>
                                // at the very end, the real bug has to do  with the
                                // checks we have for termination which we should fix first.
                                Ok((
                                    InlineCompletionResponse::new(
                                        // this gets sent at the very end
                                        vec![InlineCompletion::new(
                                            response.answer_up_until_now().to_owned(),
                                            insert_range(
                                                completion_request.position,
                                                &document_lines,
                                                response.answer_up_until_now(),
                                            ),
                                            None,
                                        )],
                                    ),
                                    cursor_prefix,
                                    should_end_stream.clone(),
                                    fast_model,
                                ))
                            }
                            either::Right(Err(e)) => {
                                println!("{:?}", e);
                                Err(InLineCompletionError::InlineCompletionTerminated)
                            }
                        }
                    },
                )
                // this is used to decide the termination of the stream
                .take_while(
                    |inline_completion_response| match inline_completion_response {
                        Ok((
                            inline_completion_response,
                            cursor_prefix,
                            should_end_stream,
                            fast_model,
                        )) => {
                            // Now we can check if we should still be sending the item over, and we work independently over here on a state
                            // basis and not the stream basis
                            {
                                // we are going ot early bail here if we have reached the end of the stream
                                if let Ok(value) = should_end_stream.lock() {
                                    if *value {
                                        return futures::future::ready(false);
                                    }
                                }
                            }
                            let inserted_text_delta = inline_completion_response
                                .completions
                                .get(0)
                                .map(|completion| completion.delta.clone())
                                .flatten();
                            let inserted_text = inline_completion_response
                                .completions
                                .get(0)
                                .map(|completion| completion.insert_text.to_owned());
                            let inserted_range = inline_completion_response
                                .completions
                                .get(0)
                                .map(|completion| completion.insert_range.clone());
                            match (inserted_text, inserted_range) {
                                (Some(inserted_text), Some(inserted_range)) => {
                                    // we go for immediate termination now
                                    let terminating_condition = immediate_terminating_condition(
                                        inserted_text.clone(),
                                        inserted_text_delta.clone(),
                                        &inserted_range,
                                        cursor_prefix.clone(),
                                        fast_model.clone(),
                                    );
                                    // dbg!(
                                    //     "sidecar.termination_condition",
                                    //     &inserted_text,
                                    //     &inserted_text_delta,
                                    //     &terminating_condition,
                                    // );
                                    match terminating_condition {
                                        TerminationCondition::Immediate => {
                                            if let Ok(mut value) = should_end_stream.lock() {
                                                *value = true;
                                            }
                                            // terminate NOW
                                            futures::future::ready(false)
                                        }
                                        TerminationCondition::Next => {
                                            if let Ok(mut value) = should_end_stream.lock() {
                                                *value = true;
                                            }
                                            // terminate on next
                                            futures::future::ready(true)
                                        }
                                        TerminationCondition::Not => futures::future::ready(true),
                                    }
                                }
                                _ => futures::future::ready(true),
                            }
                        }
                        Err(_) => futures::future::ready(false),
                    },
                )
                .map(|item| match item {
                    Ok((inline_completion, _cursor_prefix, _should_end_stream, _llm_type)) => {
                        Ok(inline_completion)
                    }
                    Err(e) => Err(e),
                })
        }))
    }
}

fn indentation_at_position(line_content: &str) -> usize {
    let mut indentation = 0;
    // indentation is consistent so we do not have to worry about counting
    // the spaces which tabs will take
    for c in line_content.chars() {
        if c == ' ' {
            indentation += 1;
        } else if c == '\t' {
            indentation += 1;
        } else {
            break;
        }
    }
    indentation
}

#[derive(Debug)]
enum TerminationCondition {
    /// terminate the stream immediately and do not send the current line
    Immediate,
    /// send the current line and stop the stream after
    Next,
    /// we do not have to terminate
    Not,
}

// TODO(skcd): We have to fix this properly, we can use the same condition as
// what cody/continue is doing for now
fn immediate_terminating_condition(
    inserted_text: String,
    inserted_text_delta: Option<String>,
    _inserted_range: &Range,
    context: Arc<FillInMiddleStreamContext>,
    fast_model: LLMType,
) -> TerminationCondition {
    // for anthropic models, since they are smart and do not ramble we can terminate
    // immediately
    let next_line = context.next_non_empty_line.as_ref();
    if fast_model.is_anthropic() {
        if inserted_text_delta == Some("</code_inserted>".to_owned()) {
            return TerminationCondition::Immediate;
        }
    }

    // Check if the indentation of the inserted text is greater than the line we are on
    // if that's the case, then we should not be inserting it and stop, we are going out
    // of bounds
    // The reason here is that we need the whole line and not just the prefix of the
    // inserted text, since it can be partial, so we grab that as well and figure out
    // the line
    // check if this is the same line as the cursor position, then its okay to always include this
    // and skip the indentaiton check
    if let Some(inserted_text_delta) = inserted_text_delta.as_ref() {
        // early failsafe when the inserted text is a prefix for the inserted text delta
        if inserted_text.starts_with(inserted_text_delta) {
            return TerminationCondition::Not;
        }
        let inserted_text_indentation = indentation_at_position(&inserted_text_delta);
        let prefix_indentation = indentation_at_position(&context.prefix_at_cursor_position);
        if inserted_text_indentation < prefix_indentation {
            return TerminationCondition::Immediate;
        }
        // if the indents are equal we should probably stop at the next line
        // this is not perfect cause I can think of if else kind of blocks where
        // this will break
        // but this will protect against a lot of cases which we are seeing in prod
        if inserted_text_indentation == prefix_indentation {
            return TerminationCondition::Next;
        }
    }

    if fast_model.is_anthropic() {
        return TerminationCondition::Not;
    }
    // First we check if the next line is similar to the line we are going to insert
    // if that's the case, then we CAN STOP
    // if let (Some(next_line), Some(inserted_text)) = (next_line, inserted_text_delta.as_ref()) {
    //     let distance: usize = *str_distance(
    //         next_line,
    //         inserted_text.trim(),
    //         str_distance::Levenshtein::default(),
    //     );
    //     if inserted_text.len() > 4
    //         && next_line.len() > 4
    //         // comparision between the distance
    //         && (((distance / next_line.trim().len()) as f32) < 0.1)
    //     {
    //         dbg!("sidecar.inline_autocomplete.stop.next_line_similarity");
    //         return TerminationCondition::Immediate;
    //     }
    // }

    // Next we check if this is a closing bracket condition
    let closing_brackets = vec![")", "]", "}", "`", "\"\"\"", ";"]
        .into_iter()
        .map(|s| s.to_owned())
        .collect::<Vec<String>>();
    let opening_brackets = vec!["(", "[", "{"]
        .into_iter()
        .map(|s| s.to_owned())
        .collect::<Vec<String>>();

    // We check if the next line is completely closing bracket types
    if let (Some(next_line), Some(inserted_text)) = (next_line, inserted_text_delta.as_ref()) {
        let next_line_closing_count = next_line
            .chars()
            .into_iter()
            .filter(|char| closing_brackets.contains(&char.to_string()))
            .count() as i64;
        let next_line_opening_count = next_line
            .chars()
            .into_iter()
            .filter(|char| opening_brackets.contains(&char.to_string()))
            .count() as i64;
        let is_inserted_text_closing = inserted_text
            .chars()
            .into_iter()
            .all(|char| closing_brackets.contains(&char.to_string()));
        if next_line_closing_count > next_line_opening_count && is_inserted_text_closing {
            return TerminationCondition::Immediate;
        }
    }

    // check if the next line is a prefix of the inserted line, this can happen
    // when we are inserting )}; kind of values and the editor already has )}
    if let (Some(next_line), Some(inserted_text)) = (next_line, inserted_text_delta.as_ref()) {
        if inserted_text.starts_with(next_line) {
            return TerminationCondition::Immediate;
        }
    }

    // Now we are going to check if the inserted text is just ending brackets and nothing
    // else, if that's the case we probably want to stop at this point and stop generation
    // this will help us avoid the case where we are inserting a line after the closing
    // brackets
    if let Some(inserted_text_delta) = inserted_text_delta.as_ref() {
        let is_inserted_text_closing = inserted_text_delta
            // trim here to remove the whitespace
            .trim()
            .chars()
            .into_iter()
            .all(|char| closing_brackets.contains(&char.to_string()));
        if is_inserted_text_closing {
            return TerminationCondition::Next;
        }
    }

    TerminationCondition::Not
}
