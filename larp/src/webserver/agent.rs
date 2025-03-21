use crate::agent::types::VariableInformation as AgentVariableInformation;
use crate::chunking::text_document::Position as DocumentPosition;
use crate::repo::types::RepoRef;
use crate::user_context::types::{UserContext, VariableInformation, VariableType};

impl Into<crate::agent::types::VariableType> for VariableType {
    fn into(self) -> crate::agent::types::VariableType {
        match self {
            VariableType::File => crate::agent::types::VariableType::File,
            VariableType::CodeSymbol => crate::agent::types::VariableType::CodeSymbol,
            VariableType::Selection => crate::agent::types::VariableType::Selection,
        }
    }
}

impl VariableInformation {
    pub fn to_agent_type(self) -> AgentVariableInformation {
        AgentVariableInformation {
            start_position: DocumentPosition::new(
                self.start_position.line(),
                self.start_position.column(),
                0,
            ),
            end_position: DocumentPosition::new(
                self.end_position.line(),
                self.end_position.column(),
                0,
            ),
            fs_file_path: self.fs_file_path,
            name: self.name,
            variable_type: self.variable_type.into(),
            content: self.content,
            language: self.language,
        }
    }

    pub fn from_user_active_window(active_window: &ActiveWindowData) -> Self {
        Self {
            start_position: DocumentPosition::new(
                active_window.start_line.try_into().unwrap(),
                0,
                0,
            ),
            end_position: DocumentPosition::new(
                active_window.end_line.try_into().unwrap(),
                1000,
                0,
            ),
            fs_file_path: active_window.file_path.to_owned(),
            name: "active_window".to_owned(),
            variable_type: VariableType::Selection,
            content: active_window.visible_range_content.to_owned(),
            language: active_window.language.to_owned(),
            patch: None,
            initial_patch: None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ActiveWindowData {
    pub file_path: String,
    pub file_content: String,
    pub language: String,
    pub visible_range_content: String,
    // start line and end line here refer to the range of the active window for
    // the user
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepContextForView {
    pub repo_ref: RepoRef,
    pub precise_context: Vec<PreciseContext>,
    pub cursor_position: Option<CursorPosition>,
    pub current_view_port: Option<CurrentViewPort>,
    pub language: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionSnippet {
    pub context: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreciseContext {
    pub symbol: Symbol,
    pub hover_text: Vec<String>,
    pub definition_snippet: DefinitionSnippet,
    pub fs_file_path: String,
    pub relative_file_path: String,
    pub range: Range,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuzzy_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPosition {
    pub start_position: Position,
    pub end_position: Position,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentViewPort {
    pub start_position: Position,
    pub end_position: Position,
    pub relative_path: String,
    pub fs_file_path: String,
    pub text_on_screen: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_line: usize,
    pub start_character: usize,
    pub end_line: usize,
    pub end_character: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppendPlanRequest {
    user_query: String,
    thread_id: uuid::Uuid,
    editor_url: String,
    user_context: UserContext,
    #[serde(default)]
    is_deep_reasoning: bool,
    #[serde(default)]
    with_lsp_enrichment: bool,
    access_token: String,
}
