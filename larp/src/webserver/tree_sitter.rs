use axum::{response::IntoResponse, Extension, Json};
use quick_xml::events::Event;

use crate::{application::application::Application, chunking::text_document::Range};

use super::{
    in_line_agent::TextDocumentWeb,
    types::{ApiResponse, Result},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractDocumentationStringRequest {
    language: String,
    source: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ExtractDocumentationStringResponse {
    documentation: Vec<String>,
}

impl ApiResponse for ExtractDocumentationStringResponse {}

pub async fn extract_documentation_strings(
    Extension(app): Extension<Application>,
    Json(ExtractDocumentationStringRequest { language, source }): Json<
        ExtractDocumentationStringRequest,
    >,
) -> Result<impl IntoResponse> {
    let language_parsing = app.language_parsing.clone();
    let documentation_strings = language_parsing.parse_documentation(&source, &language);
    Ok(Json(ExtractDocumentationStringResponse {
        documentation: documentation_strings,
    }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractDiagnosticsRangeQuery {
    range: Range,
    text_document_web: TextDocumentWeb,
    threshold_to_expand: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractDiagnosticRangeReply {
    range: Range,
}

pub async fn extract_diagnostics_range(
    Extension(app): Extension<Application>,
    Json(ExtractDiagnosticsRangeQuery {
        range,
        text_document_web,
        threshold_to_expand,
    }): Json<ExtractDiagnosticsRangeQuery>,
) -> Result<impl IntoResponse> {
    let language_parsing = app.language_parsing.clone();
    let expanded_range = language_parsing.get_fix_range(
        &text_document_web.text,
        &text_document_web.language,
        &range,
        threshold_to_expand,
    );
    Ok(Json(ExtractDiagnosticRangeReply {
        range: expanded_range.unwrap_or(range),
    }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeSitterValidRequest {
    language: String,
    source: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeSitterValidResponse {
    valid: bool,
}

pub async fn tree_sitter_node_check(
    Extension(app): Extension<Application>,
    Json(TreeSitterValidRequest { language, source }): Json<TreeSitterValidRequest>,
) -> Result<impl IntoResponse> {
    let language_parsing = app.language_parsing.clone();
    let tree_sitter = language_parsing.for_lang(&language);
    let valid = match tree_sitter {
        Some(tree_sitter) => {
            let grammar = tree_sitter.grammar;
            let mut parser = tree_sitter::Parser::new();
            let _ = parser.set_language(grammar());
            let node = parser.parse(&source, None);
            match node {
                Some(node) => node.root_node().has_error(),
                None => false,
            }
        }
        None => false,
    };
    Ok(Json(TreeSitterValidResponse { valid }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckValidXMLRequest {
    input: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckValidXMLResponse {
    valid: bool,
}

fn validate_xml(xml_data: &str) -> bool {
    let mut reader = quick_xml::Reader::from_str(xml_data);
    reader.trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(_) => (),
            Err(_) => return false,
        }
    }
    true
}

pub async fn check_valid_xml(
    Extension(_app): Extension<Application>,
    Json(CheckValidXMLRequest { input }): Json<CheckValidXMLRequest>,
) -> Result<impl IntoResponse> {
    println!("we are getting a ping over here");
    println!("Input: {}", &input);
    let valid = validate_xml(&input);
    Ok(Json(CheckValidXMLResponse { valid }))
}
