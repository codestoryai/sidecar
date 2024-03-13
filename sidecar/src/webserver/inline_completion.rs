use std::{collections::HashMap, time::Instant};

use axum::{
    response::{sse, IntoResponse, Sse},
    Extension, Json,
};
use futures::{stream::Abortable, StreamExt};
use tracing::info;

use crate::{
    application::application::Application,
    chunking::text_document::{Position, Range},
    inline_completion::{
        multiline::detect_multiline::is_multiline_completion,
        types::{FillInMiddleCompletionAgent, TypeIdentifier},
    },
};

use super::{
    model_selection::LLMClientConfig,
    types::{ApiResponse, Result},
};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct InlineCompletionRequest {
    pub filepath: String,
    pub language: String,
    pub text: String,
    pub position: Position,
    pub indentation: Option<String>,
    pub model_config: LLMClientConfig,
    pub id: String,
    pub clipboard_content: Option<String>,
    // very badly named field
    pub type_identifiers: Vec<TypeIdentifier>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineCompletion {
    pub insert_text: String,
    pub insert_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

impl InlineCompletion {
    pub fn new(insert_text: String, insert_range: Range, delta: Option<String>) -> Self {
        Self {
            insert_text,
            insert_range,
            delta,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InlineCompletionResponse {
    pub completions: Vec<InlineCompletion>,
    pub prompt: String,
}

impl InlineCompletionResponse {
    pub fn new(completions: Vec<InlineCompletion>, prompt: String) -> Self {
        Self {
            completions,
            prompt,
        }
    }
}

impl ApiResponse for InlineCompletionResponse {}

pub async fn inline_completion(
    Extension(app): Extension<Application>,
    Json(InlineCompletionRequest {
        filepath,
        language,
        text,
        position,
        indentation,
        model_config,
        id,
        clipboard_content,
        type_identifiers,
    }): Json<InlineCompletionRequest>,
) -> Result<impl IntoResponse> {
    info!(event_name = "inline_completion", id = &id,);
    info!(mode_config = ?model_config);
    dbg!("sidecar.inline_completion.webserver.start");
    let request_start = Instant::now();
    let fill_in_middle_state = app.fill_in_middle_state.clone();
    let symbol_tracker = app.symbol_tracker.clone();
    let abort_request = fill_in_middle_state.insert(id.clone());
    let is_multiline = is_multiline_completion(
        position,
        text.to_owned(),
        app.editor_parsing.clone(),
        &filepath,
    );
    dbg!("is_multiline", &id, is_multiline);
    let fill_in_middle_agent = FillInMiddleCompletionAgent::new(
        app.llm_broker.clone(),
        app.llm_tokenizer.clone(),
        app.answer_models.clone(),
        app.fill_in_middle_broker.clone(),
        app.editor_parsing.clone(),
        symbol_tracker,
        is_multiline,
    );
    let completions = fill_in_middle_agent
        .completion(
            InlineCompletionRequest {
                filepath,
                language,
                text,
                position,
                indentation,
                model_config,
                id: id.to_owned(),
                clipboard_content,
                type_identifiers,
            },
            abort_request.handle().clone(),
            request_start,
        )
        .await
        .map_err(|_e| anyhow::anyhow!("error when generating inline completion"))?;
    // this is how we can abort the running stream if the client disconnects
    let stream = Abortable::new(completions, abort_request);
    Ok(Sse::new(Box::pin(stream.filter_map(
        |completion| async move {
            match completion {
                Ok(completion) => Some(
                    sse::Event::default()
                        .json_data(serde_json::to_string(&completion).expect("serde to work")),
                ),
                _ => None,
            }
        },
    ))))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CancelInlineCompletionRequest {
    id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CancelInlineCompletionResponse {}

impl ApiResponse for CancelInlineCompletionResponse {}

pub async fn cancel_inline_completion(
    Extension(app): Extension<Application>,
    Json(CancelInlineCompletionRequest { id }): Json<CancelInlineCompletionRequest>,
) -> Result<impl IntoResponse> {
    dbg!("inline_completion.cancel", &id);
    let fill_in_middle_state = app.fill_in_middle_state.clone();
    fill_in_middle_state.cancel(&id);
    Ok(Json(CancelInlineCompletionResponse {}))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineDocumentOpenRequest {
    file_path: String,
    file_content: String,
    language: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineDocumentOpenResponse {}

impl ApiResponse for InLineDocumentOpenResponse {}

pub async fn inline_document_open(
    Extension(app): Extension<Application>,
    Json(InLineDocumentOpenRequest {
        file_path,
        file_content,
        language,
    }): Json<InLineDocumentOpenRequest>,
) -> Result<impl IntoResponse> {
    let symbol_tracker = app.symbol_tracker.clone();
    dbg!("webserver.add_document", &file_path);
    symbol_tracker
        .add_document(file_path, file_content, language)
        .await;
    Ok(Json(InLineDocumentOpenResponse {}))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextDocumentContentRange {
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextDocumentContentChangeEvent {
    range: TextDocumentContentRange,
    text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineCompletionFileContentChange {
    file_path: String,
    language: String,
    file_content: String,
    events: Vec<TextDocumentContentChangeEvent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineCompletionFileContentChangeResponse {}

impl ApiResponse for InLineCompletionFileContentChangeResponse {}

pub async fn inline_completion_file_content_change(
    Extension(app): Extension<Application>,
    Json(InLineCompletionFileContentChange {
        file_path,
        language,
        file_content,
        events,
    }): Json<InLineCompletionFileContentChange>,
) -> Result<impl IntoResponse> {
    let symbol_tracker = app.symbol_tracker.clone();

    // make the edits to the file
    let events = events
        .into_iter()
        .map(|event| {
            let range = Range::new(
                Position::new(event.range.start_line, event.range.start_column, 0),
                Position::new(event.range.end_line, event.range.end_column, 0),
            );
            (range, event.text)
        })
        .collect::<Vec<_>>();
    symbol_tracker
        .file_content_change(file_path, file_content, language, events)
        .await;
    Ok(Json(InLineCompletionFileContentChangeResponse {}))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineCompletionFileStateRequest {
    file_path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineCompletionFileStateResponse {
    file_content: Option<String>,
}

impl ApiResponse for InlineCompletionFileStateResponse {}

pub async fn inline_completion_file_content(
    Extension(app): Extension<Application>,
    Json(InlineCompletionFileStateRequest { file_path }): Json<InlineCompletionFileStateRequest>,
) -> Result<impl IntoResponse> {
    let symbol_tracker = app.symbol_tracker.clone();
    let content = symbol_tracker.get_file_content(&file_path).await;
    Ok(Json(InlineCompletionFileStateResponse {
        file_content: content,
    }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineCompletionEditedLinesRequest {
    file_path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineCompletionEditedLinesResponse {
    edited_lines: Vec<usize>,
}

impl ApiResponse for InlineCompletionEditedLinesResponse {}

pub async fn inline_completion_edited_lines(
    Extension(app): Extension<Application>,
    Json(InlineCompletionEditedLinesRequest { file_path }): Json<
        InlineCompletionEditedLinesRequest,
    >,
) -> Result<impl IntoResponse> {
    let symbol_tracker = app.symbol_tracker.clone();
    let edited_lines = symbol_tracker.get_file_edited_lines(&file_path).await;
    Ok(Json(InlineCompletionEditedLinesResponse { edited_lines }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineCompletionIdentifierNodesRequest {
    file_path: String,
    language: String,
    file_content: String,
    cursor_line: usize,
    cursor_column: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdentifierNodeResponse {
    pub name: String,
    pub range: Range,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineCompletionIdentifierNodesResponse {
    identifier_nodes: Vec<IdentifierNodeResponse>,
}

impl ApiResponse for InLineCompletionIdentifierNodesResponse {}

pub async fn get_identifier_nodes(
    Extension(app): Extension<Application>,
    Json(InLineCompletionIdentifierNodesRequest {
        file_path,
        language,
        file_content,
        cursor_line,
        cursor_column,
    }): Json<InLineCompletionIdentifierNodesRequest>,
) -> Result<impl IntoResponse> {
    dbg!("sidecar.get_identifier_nodes", &file_path, language);
    let inline_symbol_tracker = app.symbol_tracker.clone();

    let cursor_position = Position::new(cursor_line, cursor_column, 0);

    // For now we will use the language_config directly, later we should use
    // the cached view which is present in the symbol tracker
    let identifier_nodes = inline_symbol_tracker
        .get_identifier_nodes(&file_path, cursor_position)
        .await;
    dbg!("sidecar.identifier_nodes", identifier_nodes.len());
    Ok(Json(InLineCompletionIdentifierNodesResponse {
        identifier_nodes: identifier_nodes
            .into_iter()
            .map(|identifier_node| IdentifierNodeResponse {
                name: identifier_node.0,
                range: identifier_node.1,
            })
            .collect(),
    }))
}
