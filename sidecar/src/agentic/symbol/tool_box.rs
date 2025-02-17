use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use futures::{stream, StreamExt};
use llm_client::clients::types::LLMType;
use llm_client::provider::{
    AnthropicAPIKey, FireworksAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::agentic::symbol::events::context_event::SelectionContextEvent;
use crate::agentic::symbol::helpers::{apply_inlay_hints_to_code, split_file_content_into_parts};
use crate::agentic::symbol::identifier::{Snippet, SymbolIdentifier};
use crate::agentic::tool::code_edit::filter_edit::{
    FilterEditOperationRequest, FilterEditOperationResponse,
};
use crate::agentic::tool::code_edit::search_and_replace::SearchAndReplaceEditingRequest;
use crate::agentic::tool::code_edit::test_correction::TestOutputCorrectionRequest;
use crate::agentic::tool::code_edit::types::CodeEdit;
use crate::agentic::tool::code_symbol::correctness::{
    CodeCorrectnessAction, CodeCorrectnessRequest,
};
use crate::agentic::tool::code_symbol::error_fix::CodeEditingErrorRequest;
use crate::agentic::tool::code_symbol::find_file_for_new_symbol::{
    FindFileForSymbolRequest, FindFileForSymbolResponse,
};
use crate::agentic::tool::code_symbol::find_symbols_to_edit_in_context::{
    FindSymbolsToEditInContextRequest, FindSymbolsToEditInContextResponse,
};
use crate::agentic::tool::code_symbol::followup::{
    ClassSymbolFollowupRequest, ClassSymbolFollowupResponse, ClassSymbolMember,
};
use crate::agentic::tool::code_symbol::important::{
    CodeSymbolFollowAlongForProbing, CodeSymbolImportantRequest, CodeSymbolImportantResponse,
    CodeSymbolProbingSummarize, CodeSymbolToAskQuestionsRequest, CodeSymbolWithSteps,
    CodeSymbolWithThinking,
};
use crate::agentic::tool::code_symbol::initial_request_follow::{
    CodeSymbolFollowInitialRequest, CodeSymbolFollowInitialResponse,
};
use crate::agentic::tool::code_symbol::models::anthropic::{
    AskQuestionSymbolHint, CodeSymbolShouldAskQuestionsResponse, CodeSymbolToAskQuestionsResponse,
    ProbeNextSymbol,
};
use crate::agentic::tool::code_symbol::new_location::CodeSymbolNewLocationRequest;
use crate::agentic::tool::code_symbol::new_sub_symbol::{
    NewSubSymbolRequiredRequest, NewSubSymbolRequiredResponse,
};
use crate::agentic::tool::code_symbol::planning_before_code_edit::PlanningBeforeCodeEditRequest;
use crate::agentic::tool::code_symbol::probe::{
    ProbeEnoughOrDeeperRequest, ProbeEnoughOrDeeperResponse,
};
use crate::agentic::tool::code_symbol::probe_question_for_symbol::ProbeQuestionForSymbolRequest;
use crate::agentic::tool::code_symbol::probe_try_hard_answer::ProbeTryHardAnswerSymbolRequest;
use crate::agentic::tool::code_symbol::reranking_symbols_for_editing_context::{
    ReRankingCodeSnippetSymbolOutline, ReRankingSnippetsForCodeEditingRequest,
};
use crate::agentic::tool::code_symbol::scratch_pad::{
    ScratchPadAgentEdits, ScratchPadAgentHumanMessage, ScratchPadAgentInput,
    ScratchPadAgentInputType, ScratchPadDiagnosticSignal,
};
use crate::agentic::tool::code_symbol::should_edit::ShouldEditCodeSymbolRequest;
use crate::agentic::tool::editor::apply::{EditorApplyRequest, EditorApplyResponse};
use crate::agentic::tool::errors::ToolError;
use crate::agentic::tool::filtering::broker::{
    CodeToEditFilterRequest, CodeToEditSymbolRequest, CodeToEditSymbolResponse,
    CodeToProbeFilterResponse, CodeToProbeSubSymbolList, CodeToProbeSubSymbolRequest,
};
use crate::agentic::tool::git::diff_client::{GitDiffClientRequest, GitDiffClientResponse};
use crate::agentic::tool::git::edited_files::EditedFilesRequest;
use crate::agentic::tool::grep::file::{FindInFileRequest, FindInFileResponse};
use crate::agentic::tool::helpers::diff_recent_changes::{DiffFileContent, DiffRecentChanges};
use crate::agentic::tool::lsp::create_file::CreateFileRequest;
use crate::agentic::tool::lsp::diagnostics::{
    DiagnosticWithSnippet, LSPDiagnosticsInput, LSPDiagnosticsOutput,
};
use crate::agentic::tool::lsp::file_diagnostics::{FileDiagnosticsInput, FileDiagnosticsOutput};
use crate::agentic::tool::lsp::get_outline_nodes::{
    OutlineNodesUsingEditorRequest, OutlineNodesUsingEditorResponse,
};
use crate::agentic::tool::lsp::go_to_previous_word::GoToPreviousWordRequest;
use crate::agentic::tool::lsp::gotodefintion::{
    DefinitionPathAndRange, GoToDefinitionRequest, GoToDefinitionResponse,
};
use crate::agentic::tool::lsp::gotoimplementations::{
    GoToImplementationRequest, GoToImplementationResponse,
};
use crate::agentic::tool::lsp::gotoreferences::{
    AnchoredReference, GoToReferencesRequest, GoToReferencesResponse, ReferenceLocation,
};
use crate::agentic::tool::lsp::grep_symbol::{
    LSPGrepSymbolInCodebaseRequest, LSPGrepSymbolInCodebaseResponse,
};
use crate::agentic::tool::lsp::inlay_hints::InlayHintsRequest;
use crate::agentic::tool::lsp::open_file::OpenFileResponse;
use crate::agentic::tool::lsp::quick_fix::{
    GetQuickFixRequest, GetQuickFixResponse, LSPQuickFixInvocationRequest,
    LSPQuickFixInvocationResponse,
};
use crate::agentic::tool::lsp::subprocess_spawned_output::SubProcessSpawnedPendingOutputRequest;
use crate::agentic::tool::lsp::undo_changes::UndoChangesMadeDuringExchangeRequest;
use crate::agentic::tool::plan::add_steps::PlanAddRequest;
use crate::agentic::tool::plan::generator::{StepGeneratorRequest, StepSenderEvent};
use crate::agentic::tool::plan::plan_step::PlanStep;
use crate::agentic::tool::plan::reasoning::ReasoningRequest;
use crate::agentic::tool::r#type::{Tool, ToolType};
use crate::agentic::tool::ref_filter::ref_filter::ReferenceFilterRequest;
use crate::agentic::tool::session::chat::SessionChatMessage;
use crate::agentic::tool::session::exchange::SessionExchangeNewRequest;
use crate::agentic::tool::swe_bench::test_tool::{SWEBenchTestRepsonse, SWEBenchTestRequest};
use crate::agentic::tool::terminal::terminal::{TerminalInput, TerminalOutput};
use crate::chunking::editor_parsing::EditorParsing;
use crate::chunking::text_document::{Position, Range};
use crate::chunking::types::{OutlineNode, OutlineNodeContent};
use crate::repomap::tag::TagIndex;
use crate::repomap::types::RepoMap;
use crate::user_context::types::{UserContext, VariableInformation};
use crate::{
    agentic::tool::{broker::ToolBroker, input::ToolInput, lsp::open_file::OpenFileRequest},
};

fn new_dummy_message_properties() -> SymbolEventMessageProperties {
    let (dummy_sender, _dummy_receiver) = tokio::sync::mpsc::unbounded_channel();
    SymbolEventMessageProperties::new(
        SymbolEventRequestId::new("dummy".to_owned()),
        dummy_sender,
        "dummy_editor_url".to_owned(),
        tokio_util::sync::CancellationToken::new(),
        LLMProperties::new(
            LLMType::Llama3_1_8bInstruct,
            LLMProvider::FireworksAI,
            LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new("dummy_key".to_owned()))
        )
    )
}

use super::anchored::AnchoredSymbol;
use super::errors::SymbolError;
use super::events::context_event::ContextGatheringEvent;
use super::events::edit::{SymbolToEdit, SymbolToEditRequest};
use super::events::initial_request::{SymbolEditedItem, SymbolRequestHistoryItem};
use super::events::lsp::LSPDiagnosticError;
use super::events::message_event::{SymbolEventMessage, SymbolEventMessageProperties};
use super::events::probe::{SubSymbolToProbe, SymbolToProbeRequest};
use super::helpers::{find_needle_position, generate_hyperlink_from_snippet, SymbolFollowupBFS};
use super::identifier::{LLMProperties, MechaCodeSymbolThinking};
use super::tool_properties::ToolProperties;
use super::toolbox::helpers::{SymbolChangeSet, SymbolChanges};
use super::types::SymbolEventRequest;
use super::ui_event::UIEventWithID;

#[derive(Clone)]
pub struct ToolBox {
    tools: Arc<ToolBroker>,
    editor_parsing: Arc<EditorParsing>,
}

impl ToolBox {
    pub fn new(
        tools: Arc<ToolBroker>,
        editor_parsing: Arc<EditorParsing>,
    ) -> Self {
        Self {
            tools,
            editor_parsing,
        }
    }

    pub fn tools(&self) -> Arc<ToolBroker> {
        self.tools.clone()
    }

    pub fn mcp_tools(&self) -> Box<[ToolType]> {
        self.tools.mcp_tools.clone()
    }

    /// sends the user query to the scratch-pad agent
    pub async fn scratch_pad_agent_human_request(
        &self,
        scratch_pad_path: String,
        user_query: String,
        file_paths_interested: Vec<String>,
        user_context_files: Vec<String>,
        user_code_selected: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let scratch_pad_content = self
            .file_open(scratch_pad_path.to_owned(), message_properties.clone())
            .await;
        println!(
            "tool_box::scratch_pad_agent_human_request::scratch_pad_present::({})",
            scratch_pad_content.is_ok()
        );
        let recent_diff_with_priority = self
            .recently_edited_files(
                file_paths_interested.into_iter().collect(),
                message_properties.clone(),
            )
            .await
            .ok();
        if let Ok(scratch_pad_content) = scratch_pad_content {
            let _ = self
                .tools
                .invoke(ToolInput::ScratchPadInput(ScratchPadAgentInput::new(
                    user_context_files.to_vec(),
                    "".to_owned(),
                    ScratchPadAgentInputType::UserMessage(ScratchPadAgentHumanMessage::new(
                        user_code_selected,
                        user_context_files,
                        user_query,
                    )),
                    scratch_pad_content.contents(),
                    scratch_pad_path.to_owned(),
                    message_properties.root_request_id().to_owned(),
                    recent_diff_with_priority,
                    message_properties.ui_sender(),
                    message_properties.editor_url(),
                    message_properties.request_id_str().to_owned(),
                )))
                .await;
        }
        Ok(())
    }

    /// sends the diagnostic message to the scratch-pad agent
    pub async fn scratch_pad_diagnostics(
        &self,
        scratch_pad_path: &str,
        diagnostics: Vec<String>,
        file_paths_interested: Vec<String>,
        files_context: Vec<String>,
        extra_context: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("tool_box::scratch_pad_diagnostics");
        let scratch_pad_content = self
            .file_open(scratch_pad_path.to_owned(), message_properties.clone())
            .await;
        let recently_edited_files_with_priority = self
            .recently_edited_files(
                file_paths_interested.into_iter().collect(),
                message_properties.clone(),
            )
            .await
            .ok();
        if let Ok(scratch_pad_content) = scratch_pad_content {
            let _ = self
                .tools
                .invoke(ToolInput::ScratchPadInput(ScratchPadAgentInput::new(
                    files_context,
                    extra_context,
                    ScratchPadAgentInputType::LSPDiagnosticMessage(
                        ScratchPadDiagnosticSignal::new(diagnostics),
                    ),
                    scratch_pad_content.contents(),
                    scratch_pad_path.to_owned(),
                    message_properties.root_request_id().to_owned(),
                    recently_edited_files_with_priority,
                    message_properties.ui_sender(),
                    message_properties.editor_url(),
                    message_properties.request_id_str().to_owned(),
                )))
                .await;
        }
        Ok(())
    }

    /// send the edits which have been made to the scratch-pad agent
    pub async fn scratch_pad_edits_made(
        &self,
        scratch_pad_path: &str,
        user_query: &str,
        extra_context: &str,
        files_interested: Vec<String>,
        edits_made: Vec<String>,
        user_context_files: Vec<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("tool_box::scratch_pad_edits_made");
        println!(
            "tool_box::scratch_pad_edits_made::opening_file::file({})",
            scratch_pad_path
        );
        let scratch_pad_content = self
            .file_open(scratch_pad_path.to_owned(), message_properties.clone())
            .await;
        let recently_edited_files = self
            .recently_edited_files(
                files_interested.into_iter().collect(),
                message_properties.clone(),
            )
            .await
            .ok();
        if let Ok(scratch_pad_content) = scratch_pad_content {
            println!("tool_box::scratch_pad_edits_made::edits_made_tool_invocation::scratch_pad_content({})", scratch_pad_content.contents_ref());
            let _ = self
                .tools
                .invoke(ToolInput::ScratchPadInput(ScratchPadAgentInput::new(
                    user_context_files.to_vec(),
                    extra_context.to_owned(),
                    ScratchPadAgentInputType::EditsMade(ScratchPadAgentEdits::new(
                        edits_made,
                        user_query.to_owned(),
                    )),
                    scratch_pad_content.contents(),
                    scratch_pad_path.to_owned(),
                    message_properties.root_request_id().to_owned(),
                    recently_edited_files,
                    message_properties.ui_sender().clone(),
                    message_properties.editor_url(),
                    message_properties.request_id_str().to_owned(),
                )))
                .await;
        } else {
            println!("tool_box::scratch_pad_edits_made::file_open_error");
        }
        Ok(())
    }

    /// Inserts a new line at the locatiaon we want to, this is a smart way to
    /// make space for the new symbol by looking at the line we want to insert it
    /// and if we want to insert it at the start of the line or at the end of the line
    pub async fn add_empty_line_at_line(
        &self,
        fs_file_path: &str,
        line_number: usize,
        at_start: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Position, SymbolError> {
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let file_lines = file_contents
            .contents_ref()
            .lines()
            .into_iter()
            .collect::<Vec<_>>();
        // if we are inserting at the start of the line, then we can just insert
        // a \n at the start of the line and it will be safe always
        if at_start {
            let _ = self
                .apply_edits_to_editor(
                    fs_file_path,
                    &Range::new(
                        Position::new(line_number, 0, 0),
                        Position::new(line_number, 0, 0),
                    ),
                    "\n",
                    true,
                    message_properties.clone(),
                )
                .await;
            Ok(Position::new(line_number, 0, 0))
        } else {
            match file_lines.get(line_number) {
                Some(line_content) => {
                    if line_content.is_empty() {
                        Ok(Position::new(line_number, 0, 0))
                    } else {
                        let edit_range = Range::new(
                            Position::new(line_number, 0, 0),
                            Position::new(line_number, line_content.chars().count(), 0),
                        );
                        let _ = self
                            .apply_edits_to_editor(
                                &fs_file_path,
                                &edit_range,
                                &format!("{}\n", line_content),
                                true,
                                message_properties.clone(),
                            )
                            .await;
                        Ok(Position::new(line_number + 1, 0, 0))
                    }
                }
                // none here might refer to the fact that the line does not exist
                // this almost always happens for empty files, so for now insert an empty
                // line at 0, 0
                None => {
                    let _ = self
                        .apply_edits_to_editor(
                            fs_file_path,
                            &Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0)),
                            "",
                            true,
                            message_properties.clone(),
                        )
                        .await?;
                    Ok(Position::new(0, 0, 0))
                }
            }
        }
    }

    /// Adds a new empty line at the end of the file and returns the start position
    /// of the newly added line
    pub async fn add_empty_line_end_of_file(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Position, SymbolError> {
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let file_lines = file_contents
            .contents_ref()
            .lines()
            .into_iter()
            .collect::<Vec<_>>();
        match file_lines.last() {
            Some(last_line) => {
                if last_line.is_empty() {
                    // if we have an empty line then its safe to just keep this
                    Ok(Position::new(file_lines.len() - 1, 0, 0))
                } else {
                    // we want to insert a new line over here
                    let edit_range = Range::new(
                        Position::new(file_lines.len() - 1, 0, 0),
                        Position::new(file_lines.len() - 1, last_line.chars().count(), 0),
                    );
                    let _ = self
                        .apply_edits_to_editor(
                            &fs_file_path,
                            &edit_range,
                            &format!("{}\n", last_line),
                            true,
                            message_properties.clone(),
                        )
                        .await;
                    // now that we have inserted a new line it should be safe to send
                    // over the file_lines.len() as the position where we want to
                    // insert the code
                    Ok(Position::new(file_lines.len(), 0, 0))
                }
            }
            None => {
                // if we have no lines in the file, then we can just add an empty
                // string over here
                let _ = self
                    .apply_edits_to_editor(
                        fs_file_path,
                        &Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0)),
                        "",
                        true,
                        message_properties.clone(),
                    )
                    .await?;
                Ok(Position::new(0, 0, 0))
            }
        }
    }

    /// Returns the position of the last line and also returns if this line is
    /// empty (which would make it safe for code editing)
    pub async fn get_last_position_in_file(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Position, SymbolError> {
        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let file_lines = file_content
            .contents_ref()
            .lines()
            .into_iter()
            .collect::<Vec<_>>();
        Ok(Position::new(file_lines.len() - 1, 0, 0))
    }

    // TODO(codestory): This needs more love, the position we are getting back
    // is completely broken in this case
    pub async fn find_implementation_block_for_sub_symbol(
        &self,
        mut sub_symbol_to_edit: SymbolToEdit,
        implementations: &[Snippet],
    ) -> Result<SymbolToEdit, SymbolError> {
        // Find the right implementation where we want to insert this sub-symbol
        let language_config = self
            .editor_parsing
            .for_file_path(sub_symbol_to_edit.fs_file_path());
        if let None = language_config {
            return Err(SymbolError::FileTypeNotSupported(
                sub_symbol_to_edit.fs_file_path().to_owned(),
            ));
        }
        let language_config = language_config.expect("if let None to hold");
        if language_config.language_str == "rust" {
            let valid_position: Option<(String, Position)> = implementations
                .into_iter()
                .filter(|implementation| {
                    // only those implementations which are of class type and
                    implementation.outline_node_content().is_class_type()
                })
                .filter_map(|implementation| {
                    // check if the implementation config contains `impl ` just this
                    // in the content, this is a big big hack but this will work
                    if implementation
                        .outline_node_content()
                        .content()
                        .contains("impl ")
                    {
                        Some((
                            implementation
                                .outline_node_content()
                                .fs_file_path()
                                .to_owned(),
                            implementation.outline_node_content().range().end_position(),
                        ))
                    } else {
                        None
                    }
                })
                .next();
            match valid_position {
                Some((fs_file_path, end_position)) => {
                    sub_symbol_to_edit.set_fs_file_path(fs_file_path);
                    sub_symbol_to_edit
                        .set_range(Range::new(end_position.clone(), end_position.clone()));
                }
                None => {
                    // TODO(codestory): Handle the none case here when we do not find
                    // any implementation block and have to create one
                }
            };
            Ok(sub_symbol_to_edit)
        } else if language_config.language_str == "typescript"
            || language_config.language_str == "javascript"
        {
            // for languages like typescript and javascript we want to fix the range
            // for the new sub-symbol which we want to edit
            let valid_position = implementations
                .into_iter()
                // reversing it here so we get the largest class implementation
                // block
                .rev()
                .filter(|implementation| implementation.outline_node_content().is_class_type())
                .map(|implementation| {
                    let outline_node_content = implementation.outline_node_content();
                    (
                        outline_node_content.fs_file_path().to_owned(),
                        implementation.range().end_position(),
                    )
                })
                .next();
            match valid_position {
                Some((fs_file_path, end_position)) => {
                    sub_symbol_to_edit.set_fs_file_path(fs_file_path);
                    sub_symbol_to_edit.set_range(Range::new(end_position.clone(), end_position));
                }
                None => {}
            };
            Ok(sub_symbol_to_edit)
        } else {
            Ok(sub_symbol_to_edit)
        }
    }

    /// Finds the symbols which need to be edited from the user context
    pub async fn find_symbols_to_edit_from_context(
        &self,
        context: &str,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<FindSymbolsToEditInContextResponse, SymbolError> {
        let tool_input =
            ToolInput::FindSymbolsToEditInContext(FindSymbolsToEditInContextRequest::new(
                context.to_owned(),
                llm_properties,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_symbols_to_edit_in_context()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn grep_symbols_in_ide(
        &self,
        symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<LSPGrepSymbolInCodebaseResponse, SymbolError> {
        let tool_input = ToolInput::GrepSymbolInCodebase(LSPGrepSymbolInCodebaseRequest::new(
            message_properties.editor_url(),
            symbol_name.to_owned(),
        ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_lsp_grep_symbols_in_codebase_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn find_import_nodes(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<(String, Range)>, SymbolError> {
        let language_config = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .ok_or(SymbolError::FileTypeNotSupported(fs_file_path.to_owned()))?;
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let source_code = file_contents.contents_ref().as_bytes();
        let hoverable_nodes = language_config.hoverable_nodes(source_code);
        let import_identifiers = language_config.generate_import_identifiers_fresh(source_code);
        // Now we do the dance where we go over the hoverable nodes and only look at the ranges which overlap
        // with the import identifiers
        let clickable_imports = hoverable_nodes
            .into_iter()
            .filter(|hoverable_node| {
                import_identifiers
                    .iter()
                    .any(|(_, import_identifier)| import_identifier.contains(&hoverable_node))
            })
            .filter_map(|hoverable_node_range| {
                file_contents
                    .content_in_ranges_exact(&hoverable_node_range)
                    .map(|content| (content, hoverable_node_range))
            })
            .collect::<Vec<_>>();
        Ok(clickable_imports)
    }

    /// Grabs all the files which are imported by the current file we are interested
    /// in
    ///
    /// Use this for getting a sense of code graph and where we are loacted in the code
    async fn get_imported_files(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
        // returns the Vec<files_on_the_local_graph>, Vec<outline_nodes_symbol_filter>)
        // this allows us to get the local graph of the files which are related
        // to the current file and the outline nodes which we can include
    ) -> Result<(Vec<String>, Vec<String>), SymbolError> {
        let mut clickable_import_range = vec![];
        // the name of the outline nodes which we can include when we are looking
        // at the imports, this prevents extra context or nodes from slipping in
        // and only includes the local code graph
        let mut outline_node_name_filter = vec![];
        self.find_import_nodes(fs_file_path, message_properties.clone())
            .await?
            .into_iter()
            .for_each(|(import_node_name, range)| {
                clickable_import_range.push(range);
                outline_node_name_filter.push(import_node_name)
            });
        // Now we execute a go-to-definition request on all the imports
        let definition_files = stream::iter(
            clickable_import_range
                .to_vec()
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(range, message_properties)| async move {
            let go_to_definition = self
                .go_to_definition(fs_file_path, range.end_position(), message_properties)
                .await;
            match go_to_definition {
                Ok(go_to_definition) => go_to_definition
                    .definitions()
                    .into_iter()
                    .map(|definition| definition.file_path().to_owned())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    println!("get_imported_files::error({:?})", e);
                    vec![]
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        let implementation_files = stream::iter(
            clickable_import_range
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(range, message_properties)| async move {
            let go_to_implementations = self
                .go_to_implementations_exact(
                    fs_file_path,
                    &range.end_position(),
                    message_properties,
                )
                .await;
            match go_to_implementations {
                Ok(go_to_implementations) => go_to_implementations
                    .get_implementation_locations_vec()
                    .into_iter()
                    .map(|implementation| implementation.fs_file_path().to_owned())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    println!("get_imported_files::go_to_implementation::error({:?})", e);
                    vec![]
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        // combine the definition and implementation files together to get the local
        // code graph
        Ok((
            definition_files
                .into_iter()
                .flatten()
                .chain(implementation_files.into_iter().flatten())
                .collect::<HashSet<String>>()
                .into_iter()
                .collect(),
            outline_node_name_filter,
        ))
    }

    /// Applies the inlay hints if we are able to get that from the editor
    ///
    /// If the inlay-hints hook is not working, we fallback to the original string
    async fn apply_inlay_hints(
        &self,
        fs_file_path: &str,
        code_in_selection: &str,
        range: &Range,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let inlay_hint_request = ToolInput::InlayHints(InlayHintsRequest::new(
            fs_file_path.to_owned(),
            range.clone(),
            message_properties.editor_url().to_owned(),
        ));
        let inlay_hints = self
            .tools
            .invoke(inlay_hint_request)
            .await
            .map_err(|e| SymbolError::ToolError(e));

        match inlay_hints {
            Ok(inlay_hints) => {
                if let Some(inlay_hints) = inlay_hints.get_inlay_hints_response() {
                    Ok(apply_inlay_hints_to_code(
                        code_in_selection,
                        range,
                        inlay_hints,
                    ))
                } else {
                    Ok(code_in_selection.to_owned())
                }
            }
            Err(_e) => Ok(code_in_selection.to_owned()),
        }
    }

    /// Compresses the symbol by removing function content if its present
    /// and leaves an outline which we can work on top of
    pub fn get_compressed_symbol_view(&self, content: &str, file_path: &str) -> String {
        let language_parsing = self.editor_parsing.for_file_path(file_path);
        if let None = language_parsing {
            return content.to_owned();
        }
        let language_parsing = language_parsing.expect("if let None to hold");
        let outlines = language_parsing.generate_outline_fresh(content.as_bytes(), file_path);
        if outlines.is_empty() {
            return content.to_owned();
        }
        let compressed_outlines = outlines
            .into_iter()
            .filter_map(|outline| outline.get_outline_node_compressed())
            .collect::<Vec<_>>();
        if compressed_outlines.is_empty() {
            return content.to_owned();
        }
        compressed_outlines.join("\n")
    }

    /// ReRanking the outline nodes which we have to gather context for the code
    /// editing
    pub async fn rerank_outline_nodes_for_code_editing(
        &self,
        sub_symbol: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<String>, SymbolError> {
        let current_file_outline_nodes: Vec<_> = self
            .get_outline_nodes(sub_symbol.fs_file_path(), message_properties.clone())
            .await
            .unwrap_or_default();

        // Keep a list of symbol names which we should include and filter the outline
        // nodes against this
        let symbol_filter = current_file_outline_nodes
            .iter()
            .map(|outline_node| outline_node.name().to_owned())
            .collect::<Vec<_>>();

        // Always include the current file in the surrounding files from symbols in file
        // this is necessary to make sure that the LLM can gather the context from the current
        // file as well (even if its just outline, preventing it from assuming some functions
        // do not exist)
        let surrounding_files_from_symbols_in_file = stream::iter(
            current_file_outline_nodes
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(outline_node, message_properties)| async move {
            let definitions = self
                .go_to_definition(
                    outline_node.fs_file_path(),
                    outline_node.range().end_position(),
                    message_properties.clone(),
                )
                .await;
            let implementations = self
                .go_to_implementations_exact(
                    outline_node.fs_file_path(),
                    &outline_node.range().end_position(),
                    message_properties,
                )
                .await;
            let mut files_to_visit = vec![];
            if let Ok(definitions) = definitions {
                files_to_visit.extend(
                    definitions
                        .definitions()
                        .into_iter()
                        .map(|definition| definition.file_path().to_owned()),
                );
            }
            if let Ok(implementations) = implementations {
                files_to_visit.extend(
                    implementations
                        .get_implementation_locations_vec()
                        .into_iter()
                        .map(|implementation| implementation.fs_file_path().to_owned()),
                );
            }
            files_to_visit
        })
        .buffer_unordered(4)
        .collect::<HashSet<_>>()
        .await
        .into_iter()
        .flatten()
        .chain(vec![sub_symbol.fs_file_path().to_owned()])
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

        // we also want to check the implementations for the outline nodes which we are getting here
        // so we have the total picture of the nodes which we should be using
        let local_code_graph = self
            .local_code_graph(
                sub_symbol.fs_file_path(),
                surrounding_files_from_symbols_in_file,
                symbol_filter,
                message_properties.clone(),
            )
            .await?;

        let outline_nodes_for_query = local_code_graph
            .iter()
            .filter_map(|outline_node| {
                let outline_node_compressed = outline_node.get_outline_node_compressed();
                if let Some(outline_node_compressed) = outline_node_compressed {
                    Some(ReRankingCodeSnippetSymbolOutline::new(
                        outline_node.name().to_owned(),
                        outline_node.fs_file_path().to_owned(),
                        outline_node_compressed,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let file_contents = self
            .file_open(
                sub_symbol.fs_file_path().to_owned(),
                message_properties.clone(),
            )
            .await?;
        let file_contents = file_contents.contents();
        let range = sub_symbol.range();
        // we might not have a range here to select from when we are adding a new
        // symbol over here
        let (_, _, in_selection) = split_file_content_into_parts(&file_contents, range);
        let selected_code_with_typehints = self
            .apply_inlay_hints(
                sub_symbol.fs_file_path(),
                &in_selection,
                range,
                message_properties.clone(),
            )
            .await?;
        let user_query = sub_symbol.instructions().join("\n");
        let tool_input = ToolInput::ReRankingCodeSnippetsForEditing(
            ReRankingSnippetsForCodeEditingRequest::new(
                outline_nodes_for_query.to_vec(),
                None,
                None,
                selected_code_with_typehints,
                sub_symbol.fs_file_path().to_owned(),
                user_query,
                LLMProperties::new(
                    LLMType::Llama3_1_8bInstruct,
                    LLMProvider::FireworksAI,
                    LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
                        "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
                    )),
                ),
                message_properties.root_request_id().to_owned(),
            ),
        );

        let code_symbol_outline_list = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_reranked_outline_nodes_for_code_editing()
            .ok_or(SymbolError::WrongToolOutput)?
            .code_symbol_outline_list();

        // Now we pick the outline nodes which are part of the response and join
        // them together with \n on the compressed view
        let filetered_outline_node = outline_nodes_for_query
            .into_iter()
            .filter(|outline_node| {
                let name = outline_node.name();
                let fs_file_path = outline_node.fs_file_path();
                if code_symbol_outline_list
                    .iter()
                    .any(|outline_node_selected| {
                        outline_node_selected.name() == name
                            && outline_node_selected.fs_file_path() == fs_file_path
                    })
                {
                    true
                } else {
                    false
                }
            })
            // TODO(skcd): boooo bad ownership here, we should be able to use a reference
            // over here if we use the outline_nodes_for_query as a slice
            .map(|outline_node| outline_node.content().to_owned())
            .collect::<Vec<_>>();
        Ok(filetered_outline_node)
    }

    /// Grab the outline nodes for the files which are imported
    pub async fn local_code_graph(
        &self,
        fs_file_path: &str,
        extra_files_to_include: Vec<String>,
        symbol_filter: Vec<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<OutlineNode>, SymbolError> {
        let (imported_files, outline_nodes_filter) = self
            .get_imported_files(fs_file_path, message_properties.clone())
            .await?;

        let final_outline_nodes_filter = outline_nodes_filter
            .into_iter()
            .chain(symbol_filter.into_iter())
            .collect::<HashSet<String>>();
        let outline_nodes = stream::iter(
            imported_files
                .into_iter()
                .chain(extra_files_to_include)
                .collect::<HashSet<String>>()
                .into_iter()
                .map(|imported_file| (imported_file, message_properties.clone())),
        )
        .map(|(imported_file, message_properties)| async move {
            let file_open_response = self
                .file_open(imported_file.to_owned(), message_properties)
                .await;
            if let Ok(file_open_response) = file_open_response {
                let _ = self
                    .force_add_document(
                        &imported_file,
                        file_open_response.contents_ref(),
                        file_open_response.language(),
                    )
                    .await;
            }
            self.get_outline_nodes_grouped(&imported_file).await
        })
        .buffer_unordered(5)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .flatten()
        // filter to only include the ouline nodes which we know of, not including
        // the whole world
        .filter(|outline_node| final_outline_nodes_filter.contains(outline_node.name()))
        .collect::<Vec<_>>();
        Ok(outline_nodes)
    }

    pub async fn find_file_location_for_new_symbol(
        &self,
        symbol_name: &str,
        fs_file_path: &str,
        code_location: &Range,
        user_query: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<FindFileForSymbolResponse, SymbolError> {
        // Here there are multiple steps which we need to take to answer this:
        // - Get all the imports in the file which we are interested in
        // - Get the location of the imports which are present in the file (just the file paths)
        let clickable_imports = self
            .find_import_nodes(fs_file_path, message_properties.clone())
            .await?;
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        // grab the lines which contain the imports, this will be unordered
        let mut import_line_numbers = clickable_imports
            .iter()
            .map(|(_, range)| (range.start_line()..=range.end_line()))
            .flatten()
            .collect::<HashSet<usize>>()
            .into_iter()
            .collect::<Vec<usize>>();
        // sort the line numbers in increasing order
        import_line_numbers.sort();

        // grab the lines from the file which fall in the import line range
        let import_lines = file_contents
            .contents_ref()
            .lines()
            .enumerate()
            .filter_map(|(line_number, content)| {
                if import_line_numbers
                    .iter()
                    .any(|import_line_number| import_line_number == &line_number)
                {
                    Some(content)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Get all the file locations which are imported by this file, we use
        // this as a hint to figure out the file where we should be adding the code to
        // TODO(skcd+zi): We should show a bit more data over here for the file, maybe
        // the hotspots in the file which the user has scrolled over and a preview
        // of the file
        let import_file_locations = stream::iter(
            clickable_imports
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|((_, clickable_import_range), message_properties)| {
            self.go_to_definition(
                fs_file_path,
                clickable_import_range.end_position(),
                message_properties,
            )
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|go_to_definition_output| match go_to_definition_output {
            Ok(go_to_definition) => Some(
                go_to_definition
                    .definitions()
                    .into_iter()
                    .map(|go_to_definition| go_to_definition.file_path().to_owned())
                    .collect::<Vec<_>>(),
            ),
            Err(_e) => None,
        })
        .flatten()
        .collect::<HashSet<String>>()
        .into_iter()
        .collect::<Vec<String>>();

        let code_content_in_range = file_contents
            .contents_ref()
            .lines()
            .enumerate()
            .filter_map(|(line_number, content)| {
                if code_location.start_line() <= line_number
                    && line_number <= code_location.end_line()
                {
                    Some(content)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let tool_input = ToolInput::FindFileForNewSymbol(FindFileForSymbolRequest::new(
            fs_file_path.to_owned(),
            symbol_name.to_owned(),
            import_lines,
            import_file_locations,
            user_query.to_owned(),
            code_content_in_range,
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_find_file_for_symbol_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Tries to answer the probe query if it can
    pub async fn probe_try_hard_answer(
        &self,
        symbol_content: String,
        llm_properties: LLMProperties,
        user_query: &str,
        probe_request: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let tool_input =
            ToolInput::ProbeTryHardAnswerRequest(ProbeTryHardAnswerSymbolRequest::new(
                user_query.to_owned(),
                probe_request.to_owned(),
                symbol_content,
                llm_properties,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_try_harder_answer()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Helps us understand if we need to generate new symbols to satisfy
    /// the user request
    pub async fn check_new_sub_symbols_required(
        &self,
        symbol_name: &str,
        symbol_content: String,
        llm_properties: LLMProperties,
        user_query: &str,
        plan: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<NewSubSymbolRequiredResponse, SymbolError> {
        let tool_input = ToolInput::NewSubSymbolForCodeEditing(NewSubSymbolRequiredRequest::new(
            user_query.to_owned(),
            plan.to_owned(),
            symbol_name.to_owned(),
            symbol_content,
            llm_properties,
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_new_sub_symbol_required()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// We gather the snippets along with the questions we want to ask and
    /// generate the final question which we want to send over to the next symbol
    pub async fn probe_query_generation_for_symbol(
        &self,
        current_symbol_name: &str,
        next_symbol_name: &str,
        next_symbol_name_file_path: &str,
        original_query: &str,
        history: Vec<String>,
        ask_question_with_snippet: Vec<(Snippet, AskQuestionSymbolHint)>,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        // generate the hyperlinks using the ask_question_with_snippet
        // send these hyperlinks to the query
        let tool_input =
            ToolInput::ProbeCreateQuestionForSymbol(ProbeQuestionForSymbolRequest::new(
                current_symbol_name.to_owned(),
                next_symbol_name.to_owned(),
                next_symbol_name_file_path.to_owned(),
                ask_question_with_snippet
                    .into_iter()
                    .map(|(snippet, ask_question)| {
                        generate_hyperlink_from_snippet(&snippet, ask_question)
                    })
                    .collect::<Vec<_>>(),
                history,
                original_query.to_owned(),
                llm_properties,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_create_question_for_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Checks if we have enough information to answer the user query
    /// or do we need to probe deeper into some of the symbol
    pub async fn probe_enough_or_deeper(
        &self,
        query: String,
        xml_string: String,
        symbol_name: String,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<ProbeEnoughOrDeeperResponse, SymbolError> {
        let tool_input = ToolInput::ProbeEnoughOrDeeper(ProbeEnoughOrDeeperRequest::new(
            symbol_name,
            xml_string,
            query,
            llm_properties,
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_enough_or_deeper()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// This takes the original symbol and the generated xml out of it
    /// and gives back snippets which we should be probing into
    ///
    /// This is used to decide if the symbol is too long where all we want to
    /// focus our efforts on
    pub async fn filter_code_snippets_subsymbol_for_probing(
        &self,
        xml_string: String,
        query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeToProbeSubSymbolList, SymbolError> {
        let tool_input =
            ToolInput::ProbeFilterSnippetsSingleSymbol(CodeToProbeSubSymbolRequest::new(
                xml_string,
                query,
                llm,
                provider,
                api_key,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_to_probe_sub_symbol_list()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// This generates additional requests from the initial query
    /// which we can mark as initial query (for now, altho it should be
    /// more of a ask question) but the goal is figure out if we need to make
    /// changes elsewhere in the codebase by following a symbol
    pub async fn follow_along_initial_query(
        &self,
        code_symbol_content: Vec<String>,
        user_query: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeSymbolFollowInitialResponse, SymbolError> {
        let tool_input =
            ToolInput::CodeSymbolFollowInitialRequest(CodeSymbolFollowInitialRequest::new(
                code_symbol_content,
                user_query.to_owned(),
                llm,
                provider,
                api_keys,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_symbol_follow_for_initial_request()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Sends the request to summarize the probing results
    pub async fn probing_results_summarize(
        &self,
        request: CodeSymbolProbingSummarize,
    ) -> Result<String, SymbolError> {
        let tool_input = ToolInput::ProbeSummarizeAnswerRequest(request);
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_summarize_result()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Sends the request to figure out if we need to go ahead and probe the
    /// next symbol for a reply
    pub async fn next_symbol_should_probe_request(
        &self,
        request: CodeSymbolFollowAlongForProbing,
    ) -> Result<ProbeNextSymbol, SymbolError> {
        let tool_input = ToolInput::ProbeFollowAlongSymbol(request);
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_should_probe_next_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Takes a file path and the line content and the symbol to search for
    /// in the file
    /// This way we are able to go to the definition of the file which contains
    /// this symbol and send the appropriate request to it
    pub async fn go_to_definition_using_symbol(
        &self,
        snippet_range: &Range,
        fs_file_path: &str,
        // Line content here can be multi-line because LLMs are dumb machines
        // which do not follow the instructions provided to them
        // in which case we have to split the lines on \n and then find the line
        // which will contain the symbol_to_search
        line_content: &str,
        symbol_to_search: &str,
        // The first thing we are returning here is the highlight of the symbol
        // along with the content
        // we are returning the definition path and range along with the symbol where the go-to-definition belongs to
        // along with the outline of the symbol containing the go-to-definition
        message_properties: SymbolEventMessageProperties,
    ) -> Result<
        (
            String,
            // The position in the file where we are clicking
            Range,
            Vec<(DefinitionPathAndRange, String, String)>,
        ),
        SymbolError,
    > {
        // TODO(skcd): This is wrong, becausae both the lines can contain the symbol we want
        // to search for
        let line_content_contains_symbol = line_content
            .lines()
            .any(|line| line.contains(symbol_to_search));
        // If none of them contain it, then we need to return the error over here ASAP
        if !line_content_contains_symbol {
            return Err(SymbolError::SymbolNotFoundInLine(
                symbol_to_search.to_owned(),
                line_content.to_owned(),
            ));
        }
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?
            .contents();
        let hoverable_ranges = self.get_hoverable_nodes(file_contents.as_str(), fs_file_path)?;
        let selection_range = snippet_range;
        let file_with_lines = file_contents.lines().enumerate().collect::<Vec<_>>();
        let mut containing_lines = file_contents
            .lines()
            .enumerate()
            .into_iter()
            .map(|(index, line)| (index as i64, line))
            .filter_map(|(index, line)| {
                let start_line = selection_range.start_line() as i64;
                let end_line = selection_range.end_line() as i64;
                let minimum_distance =
                    std::cmp::min(i64::abs(start_line - index), i64::abs(end_line - index));
                // we also need to make sure that the selection we are making is in
                // the range of the snippet we are selecting the symbols in
                // LLMs have a trendency to go overboard with this;
                if line_content
                    .lines()
                    .any(|line_content_to_search| line.contains(line_content_to_search))
                    && start_line <= index
                    && index <= end_line
                {
                    Some((minimum_distance, (line.to_owned(), index)))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        // sort it by the smallest distance from the range we are interested in first
        containing_lines.sort_by(|a, b| a.0.cmp(&b.0));
        // Now iterate over all the lines containing this symbol and find the one which we can hover over
        // and select the first one possible here
        let mut symbol_locations = containing_lines
            .into_iter()
            .filter_map(|containing_line| {
                let (line, line_index) = containing_line.1;
                let position = find_needle_position(&line, symbol_to_search).map(|column| {
                    Position::new(
                        (line_index).try_into().expect("i64 to usize to work"),
                        column,
                        0,
                    )
                });
                match position {
                    Some(position) => {
                        if hoverable_ranges
                            .iter()
                            .any(|hoverable_range| hoverable_range.contains_position(&position))
                        {
                            Some(position)
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            })
            .collect::<Vec<Position>>();
        if symbol_locations.is_empty() {
            return Err(SymbolError::NoOutlineNodeSatisfyPosition);
        }
        let symbol_location = symbol_locations.remove(0);

        let symbol_range = Range::new(
            symbol_location.clone(),
            symbol_location.clone().shift_column(
                symbol_to_search
                    .chars()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .len(),
            ),
        );

        // Grab the 4 lines before and 4 lines after from the file content and show that as the highlight
        let position_line_number = symbol_location.line() as i64;
        // symbol link to send
        let symbol_link = file_with_lines
            .into_iter()
            .filter_map(|(line_number, line_content)| {
                if line_number as i64 <= position_line_number + 4
                    && line_number as i64 >= position_line_number - 4
                {
                    if line_number as i64 == position_line_number {
                        // if this is the line number we are interested in then we have to highlight
                        // this for the LLM
                        Some(format!(
                            r#"<line_with_reference>
{line_content}
</line_with_reference>"#
                        ))
                    } else {
                        Some(line_content.to_owned())
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Now we can invoke a go-to-definition on the symbol over here and get back
        // the containing symbol which has this symbol we are interested in visiting
        let go_to_definition = self
            .go_to_definition(fs_file_path, symbol_location, message_properties.clone())
            .await?
            .definitions();

        // interested files
        let files_interested = go_to_definition
            .iter()
            .map(|definition| definition.file_path().to_owned())
            .collect::<HashSet<String>>();

        // open all these files and get back the outline nodes from these
        let _ = stream::iter(
            files_interested
                .into_iter()
                .map(|file| (file, message_properties.clone())),
        )
        .map(|(file, message_properties)| async move {
            let file_open = self.file_open(file.to_owned(), message_properties).await;
            // now we also add it to the symbol tracker forcefully
            if let Ok(file_open) = file_open {
                let language = file_open.language().to_owned();
                self.symbol_broker
                    .force_add_document(file, file_open.contents(), language)
                    .await;
            }
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;
        // Now check in the outline nodes for a given file which biggest symbol contains this range
        let definitions_to_outline_node =
            stream::iter(go_to_definition.into_iter().map(|definition| {
                let file_path = definition.file_path().to_owned();
                (definition, file_path)
            }))
            .map(|(definition, fs_file_path)| async move {
                let outline_nodes = self.symbol_broker.get_symbols_outline(&fs_file_path).await;
                (definition, outline_nodes)
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|(definition, outline_nodes_maybe)| {
                if let Some(outline_node) = outline_nodes_maybe {
                    Some((definition, outline_node))
                } else {
                    None
                }
            })
            .filter_map(|(definition, outline_nodes)| {
                let possible_outline_node = outline_nodes.into_iter().find(|outline_node| {
                    // one of the problems we have over here is that the struct
                    // might be bigger than the parsing we have done because
                    // of decorators etc
                    outline_node
                        .range()
                        .contains_check_line_column(definition.range())
                        // I do not trust this check, it will probably come bite
                        // us in the ass later on
                        || definition.range().contains_check_line_column(outline_node.range()
                    )
                });
                if let Some(outline_node) = possible_outline_node {
                    Some((definition, outline_node))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Take another pass here over the definitions with thier outline nodes
        // to verify we are not pointing to an implementation but the actual
        // definition (common case with rust where implementations are in different files)
        let definitions_to_outline_node = stream::iter(definitions_to_outline_node.into_iter().map(|data| (data, message_properties.clone())))
            .map(|((definition, outline_node), message_properties)| async move {
                // Figure out what to do over here
                let identifier_range = outline_node.identifier_range();
                let fs_file_path = outline_node.fs_file_path().to_owned();
                // we want to initiate another go-to-definition at this position
                // and compare if it lands to the same location as the outline node
                // if it does, then this is correct otherwise we have to change our
                // outline node
                let go_to_definition = self
                    .go_to_definition(&fs_file_path, identifier_range.end_position(), message_properties.clone())
                    .await;
                match go_to_definition {
                    Ok(go_to_definition) => {
                        let definitions = go_to_definition.definitions();
                        let points_to_same_symbol = definitions.iter().any(|definition| {
                            let definition_range = definition.range();
                            if identifier_range.contains_check_line(&definition_range) {
                                true
                            } else {
                                false
                            }
                        });
                        if points_to_same_symbol {
                            Some((definition, outline_node))
                        } else {
                            // it does not point to the same outline node
                            // which is common for languages like rust and typescript
                            // so we try to find the symbol where we can follow this to
                            if definitions.is_empty() {
                                None
                            } else {
                                // Filter out the junk which we already know about
                                // example: rustup files which are internal libraries
                                let most_probable_definition = definitions
                                    .into_iter()
                                    .find(|definition| !definition.file_path().contains("rustup"));
                                if most_probable_definition.is_none() {
                                    None
                                } else {
                                    let most_probable_definition =
                                        most_probable_definition.expect("is_none to hold");
                                    let definition_file_path = most_probable_definition.file_path();
                                    let file_open_response = self
                                        .file_open(definition_file_path.to_owned(), message_properties.clone())
                                        .await;
                                    if file_open_response.is_err() {
                                        return None;
                                    }
                                    let file_open_response =
                                        file_open_response.expect("is_err to hold");
                                    let _ = self
                                        .force_add_document(
                                            &definition_file_path,
                                            file_open_response.contents_ref(),
                                            file_open_response.language(),
                                        )
                                        .await;
                                    // Now we want to grab the outline nodes from here
                                    let outline_nodes = self
                                        .symbol_broker
                                        .get_symbols_outline(definition_file_path)
                                        .await;
                                    if outline_nodes.is_none() {
                                        return None;
                                    }
                                    let outline_nodes = outline_nodes.expect("is_none to hold");
                                    let possible_outline_node = outline_nodes.into_iter().find(|outline_node| {
                                        // one of the problems we have over here is that the struct
                                        // might be bigger than the parsing we have done because
                                        // of decorators etc
                                        outline_node
                                            .range()
                                            .contains_check_line_column(definition.range())
                                            // I do not trust this check, it will probably come bite
                                            // us in the ass later on
                                            || definition.range().contains_check_line_column(outline_node.range()
                                        )
                                    });
                                    possible_outline_node.map(|outline_node| (definition, outline_node))
                                }
                            }
                        }
                    }
                    Err(_) => Some((definition, outline_node)),
                }
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<Vec<_>>();

        // // Now we want to go from the definitions we are interested in to the snippet
        // // where we will be asking the question and also get the outline(???) for it
        let definition_to_outline_node_name_and_definition = stream::iter(
            definitions_to_outline_node
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(
            |((definition, outline_node), message_properties)| async move {
                let fs_file_path = outline_node.fs_file_path();
                let symbol_outline = self
                    .outline_nodes_for_symbol(
                        &fs_file_path,
                        outline_node.name(),
                        message_properties,
                    )
                    .await;
                (definition, outline_node.name().to_owned(), symbol_outline)
            },
        )
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(
            |(definition, symbol_name, symbol_outline)| match symbol_outline {
                Ok(symbol_outline) => Some((definition, symbol_name, symbol_outline)),
                Err(_) => None,
            },
        )
        .collect::<Vec<_>>();

        // // Now that we have the outline node which we are interested in, we need to
        // // find the outline node which we can use to guarantee that this works
        // // Once we have the definition we can figure out the symbol which contains this
        // // Now we try to find the line which is closest the the snippet or contained
        // // within it for a lack of better word
        Ok((
            symbol_link,
            symbol_range,
            definition_to_outline_node_name_and_definition,
        ))
    }

    pub async fn probe_deeper_in_symbol(
        &self,
        snippet: &Snippet,
        reason: &str,
        history: &str,
        query: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeSymbolToAskQuestionsResponse, SymbolError> {
        let file_contents = self
            .file_open(snippet.file_path().to_owned(), message_properties.clone())
            .await?;
        let file_contents = file_contents.contents();
        let range = snippet.range();
        let (above, below, in_selection) = split_file_content_into_parts(&file_contents, range);
        let request = ToolInput::ProbeQuestionAskRequest(CodeSymbolToAskQuestionsRequest::new(
            history.to_owned(),
            snippet.symbol_name().to_owned(),
            snippet.file_path().to_owned(),
            snippet.language().to_owned(),
            "".to_owned(),
            above,
            below,
            in_selection,
            llm,
            provider,
            api_keys,
            format!(
                r#"The user has asked the following query:
{query}

We also believe this symbol needs to be looked at more closesly because:
{reason}"#
            ),
            message_properties.root_request_id().to_owned(),
        ));
        // This is broken because of the types over here
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_symbol_deeper()
            .ok_or(SymbolError::WrongToolOutput)
    }

    // This is used to ask which sub-symbols we are going to follow deeper
    pub async fn should_follow_subsymbol_for_probing(
        &self,
        snippet: &Snippet,
        reason: &str,
        history: &str,
        query: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeSymbolShouldAskQuestionsResponse, SymbolError> {
        let file_contents = self
            .file_open(snippet.file_path().to_owned(), message_properties.clone())
            .await?;
        let file_contents = file_contents.contents();
        let range = snippet.range();
        let (above, below, in_selection) = split_file_content_into_parts(&file_contents, range);
        let request = ToolInput::ProbePossibleRequest(CodeSymbolToAskQuestionsRequest::new(
            history.to_owned(),
            snippet.symbol_name().to_owned(),
            snippet.file_path().to_owned(),
            snippet.language().to_owned(),
            "".to_owned(),
            above,
            below,
            in_selection,
            llm,
            provider,
            api_key,
            // Here we can join the queries we get from the reason to the real user query
            format!(
                r"#The original user query is:
{query}

We also believe this symbol needs to be probed because of:
{reason}#"
            ),
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_should_probe_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn probe_sub_symbols(
        &self,
        snippets: Vec<Snippet>,
        request: &SymbolToProbeRequest,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeToProbeFilterResponse, SymbolError> {
        let probe_request = request.probe_request();
        let request = ToolInput::ProbeSubSymbol(CodeToEditFilterRequest::new(
            snippets,
            probe_request.to_owned(),
            llm,
            provider,
            api_key,
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_probe_sub_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn outline_nodes_for_symbol(
        &self,
        fs_file_path: &str,
        symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let outline_node_possible = self
            .get_ouline_nodes_grouped_fresh(fs_file_path, message_properties.clone())
            .await
            .ok_or(SymbolError::WrongToolOutput)?
            .into_iter()
            .find(|outline_node| outline_node.name() == symbol_name);
        if let Some(outline_node) = outline_node_possible {
            // we check for 2 things here:
            // - its either a function or a class like symbol
            // - if its a function no need to check for implementations
            // - if its a class then we still need to check for implementations
            if outline_node.is_function() {
                // just return this over here
                let fs_file_path = format!(
                    "{}-{}:{}",
                    outline_node.fs_file_path(),
                    outline_node.range().start_line(),
                    outline_node.range().end_line()
                );
                let content = outline_node.content().content();
                Ok(format!(
                    "<outline_list>
<outline>
<symbol_name>
{symbol_name}
</symbol_name>
<file_path>
{fs_file_path}
</file_path>
<content>
{content}
</content>
</outline>
</outline_list>"
                ))
            } else {
                let start = Instant::now();
                // we need to check for implementations as well and then return it
                let identifier_position = outline_node.identifier_range();
                // now we go to the implementations using this identifier node (this can take some time)
                let identifier_node_positions = self
                    .go_to_implementations_exact(
                        fs_file_path,
                        &identifier_position.start_position(),
                        message_properties.clone(),
                    )
                    .await?
                    .remove_implementations_vec();

                println!(
                    "go_to_implementations_exact::elapsed({:?})",
                    start.elapsed()
                );
                // Now that we have the identifier positions we want to grab the
                // remaining implementations as well
                let file_paths = identifier_node_positions
                    .into_iter()
                    .map(|implementation| implementation.fs_file_path().to_owned())
                    .collect::<HashSet<String>>();
                // send a request to open all these files

                let _ = stream::iter(
                    file_paths
                        .clone()
                        .into_iter()
                        .map(|fs_file_path| (fs_file_path, message_properties.clone())),
                )
                .map(|(fs_file_path, message_properties)| async move {
                    self.file_open(fs_file_path, message_properties).await
                })
                .buffer_unordered(100)
                .collect::<Vec<_>>()
                .await;

                // this shit takes forever!
                let start = Instant::now();
                // Now all files are opened so we have also parsed them in the symbol broker
                // so we can grab the appropriate outlines properly over here
                let file_path_to_outline_nodes = stream::iter(file_paths)
                    .map(|fs_file_path| async move {
                        let start = Instant::now();
                        let symbols = self.symbol_broker.get_symbols_outline(&fs_file_path).await;
                        println!("get_symbol_outline::elapsed({:?}", start.elapsed());
                        (fs_file_path, symbols)
                    })
                    .buffer_unordered(100)
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .filter_map(
                        |(fs_file_path, outline_nodes_maybe)| match outline_nodes_maybe {
                            Some(outline_nodes) => Some((fs_file_path, outline_nodes)),
                            None => None,
                        },
                    )
                    .filter_map(|(fs_file_path, outline_nodes)| {
                        match outline_nodes
                            .into_iter()
                            .find(|outline_node| outline_node.name() == symbol_name)
                        {
                            Some(outline_node) => Some((fs_file_path, outline_node)),
                            None => None,
                        }
                    })
                    .collect::<HashMap<String, OutlineNode>>();

                println!("file_path_to_outline_nodes::elapsed({:?})", start.elapsed());

                // we need to get the outline for the symbol over here
                let mut outlines = vec![];
                for (_, outline_node) in file_path_to_outline_nodes.into_iter() {
                    // Fuck it we ball, let's return the full outline here we need to truncate it later on
                    let fs_file_path = format!(
                        "{}-{}:{}",
                        outline_node.fs_file_path(),
                        outline_node.range().start_line(),
                        outline_node.range().end_line()
                    );
                    let outline = outline_node.get_outline_short();
                    outlines.push(format!(
                        r#"<outline>
<symbol_name>
{symbol_name}
</symbol_name>
<file_path>
{fs_file_path}
</file_path>
<content>
{outline}
</content>
</outline>"#
                    ))
                }

                // now add the identifier node which we are originally looking at the implementations for
                let fs_file_path = format!(
                    "{}-{}:{}",
                    outline_node.fs_file_path(),
                    outline_node.range().start_line(),
                    outline_node.range().end_line()
                );
                let outline = outline_node.get_outline_short();
                outlines.push(format!(
                    r#"<outline>
<symbol_name>
{symbol_name}
</symbol_name>
<file_path>
{fs_file_path}
</file_path>
<content>
{outline}
</content>
</outline>"#
                ));
                let joined_outlines = outlines.join("\n");
                Ok(format!(
                    r#"<outline_list>
{joined_outlines}
</outline_line>"#
                ))
            }
        } else {
            // we did not find anything here so skip this part
            Err(SymbolError::OutlineNodeNotFound(symbol_name.to_owned()))
        }
    }

    pub async fn find_sub_symbol_to_probe_with_name(
        &self,
        parent_symbol_name: &str,
        sub_symbol_probe: &SubSymbolToProbe,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNodeContent, SymbolError> {
        let file_open_response = self
            .file_open(
                sub_symbol_probe.fs_file_path().to_owned(),
                message_properties,
            )
            .await;
        match file_open_response {
            Ok(file_open_response) => {
                let _ = self
                    .force_add_document(
                        sub_symbol_probe.fs_file_path(),
                        file_open_response.contents_ref(),
                        file_open_response.language(),
                    )
                    .await;
            }
            Err(e) => {
                println!("tool_box::find_sub_symbol_to_probe_with_name::err({:?})", e);
            }
        }
        let outline_nodes = self
            .get_outline_nodes_grouped(sub_symbol_probe.fs_file_path())
            .await
            .ok_or(SymbolError::ExpectedFileToExist)?
            .into_iter()
            .filter(|outline_node| outline_node.name() == parent_symbol_name)
            .collect::<Vec<_>>();

        if sub_symbol_probe.is_outline() {
            // we can ignore this for now
            Err(SymbolError::OutlineNodeEditingNotSupported)
        } else {
            let child_node = outline_nodes
                .iter()
                .map(|outline_node| outline_node.children())
                .flatten()
                .find(|child_node| child_node.name() == sub_symbol_probe.symbol_name());
            if let Some(child_node) = child_node {
                Ok(child_node.clone())
            } else {
                outline_nodes
                    .iter()
                    .find(|outline_node| outline_node.name() == sub_symbol_probe.symbol_name())
                    .map(|outline_node| outline_node.content().clone())
                    .ok_or(SymbolError::NoOutlineNodeSatisfyPosition)
            }
        }
    }

    /// Finds the symbol which we want to edit and is closest to the range
    /// we are interested in
    ///
    /// This gives us back the full symbol instead of the sub-symbol we are interested
    /// in, this is a bit broken right now and we have to figure out how to fix this:
    /// when editing a struct and its impl right below, if the struct edits make it start
    /// overlapping with the impl edits then we get in a bad state over here
    pub async fn find_symbol_to_edit_closest_to_range(
        &self,
        symbol_to_edit: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNodeContent, SymbolError> {
        let file_open_response = self
            .file_open(symbol_to_edit.fs_file_path().to_owned(), message_properties)
            .await?;
        let _ = self
            .force_add_document(
                symbol_to_edit.fs_file_path(),
                file_open_response.contents_ref(),
                file_open_response.language(),
            )
            .await;
        let outline_nodes = self
            .get_outline_nodes_grouped(symbol_to_edit.fs_file_path())
            .await
            .ok_or(SymbolError::ExpectedFileToExist)?
            .into_iter()
            .filter(|outline_node| outline_node.name() == symbol_to_edit.symbol_name())
            .collect::<Vec<_>>();

        if outline_nodes.is_empty() {
            return Err(SymbolError::NoOutlineNodeSatisfyPosition);
        }
        // Now we want to get the outline node which is closest in the range to all
        // the outline nodes which we are getting
        let mut outline_nodes_with_distance = outline_nodes
            .into_iter()
            .map(|outline_node| {
                let outline_node_range = outline_node.range();
                let symbol_to_edit_range = symbol_to_edit.range();
                let range_distance = outline_node_range.minimal_line_distance(symbol_to_edit_range);
                (range_distance, outline_node)
            })
            .collect::<Vec<_>>();
        outline_nodes_with_distance.sort_by_key(|(distance, _)| *distance);

        // grab the first outline node over here
        if outline_nodes_with_distance.is_empty() {
            return Err(SymbolError::NoOutlineNodeSatisfyPosition);
        } else {
            // grabs the outline node which is at the lowest distance from the
            // range we are interested in
            Ok(outline_nodes_with_distance.remove(0).1.content().clone())
        }
    }

    /// The symbol can move because of some other edit so we have to map it
    /// properly over here and find it using the name as that it is the best
    /// way to achieve this right now
    /// There might be multiple outline nodes with the same name (rust) supports this
    /// so we need to find the outline node either closest to the range we are interested
    /// in or we found a child node
    ///
    /// Another consideration here is that the symbol might be an outline node
    /// so we have to be careful with the selection, this happens in languages
    /// which are not rust like: python, typescript etc
    pub async fn find_sub_symbol_to_edit_with_name(
        &self,
        parent_symbol_name: &str,
        symbol_to_edit: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNodeContent, SymbolError> {
        let outline_nodes = self
            .get_outline_nodes_from_editor(
                symbol_to_edit.fs_file_path(),
                message_properties.clone(),
            )
            .await
            .unwrap_or_default();
        if outline_nodes.is_empty() {
            return Err(SymbolError::NoOutlineNodeSatisfyPosition);
        }
        if symbol_to_edit.is_outline() {
            // grab the outline node which belongs to the symbol we want to edit
            let outline_node_for_symbol = outline_nodes
                .into_iter()
                .find(|outline_node| outline_node.name() == symbol_to_edit.symbol_name())
                .ok_or(SymbolError::NoOutlineNodeSatisfyPosition)?;

            // Now we grab the outline over here by removing all the line ranges which belong
            // to the functions
            let outline_content = outline_node_for_symbol
                .content()
                .get_non_overlapping_content(
                    outline_node_for_symbol
                        .children()
                        .into_iter()
                        .map(|outline_node_child| outline_node_child.range())
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
                .ok_or(SymbolError::NoOutlineNodeSatisfyPosition)?;

            // update the range and the content of the outline node content
            let outline_content_range = outline_content.1;
            let outline_node_content = outline_content.0;
            let outline_node = outline_node_for_symbol
                .content()
                .clone()
                .set_range(outline_content_range)
                .set_content(outline_node_content);
            Ok(outline_node)
        } else {
            let child_node = outline_nodes
                .iter()
                .filter(|outline_node| outline_node.name() == parent_symbol_name)
                .map(|outline_node| outline_node.children())
                .flatten()
                .into_iter()
                .find(|child_node| child_node.name() == symbol_to_edit.symbol_name());

            if let Some(child_node) = child_node {
                Ok(child_node.clone())
            } else {
                // if no children match, then we have to find out which symbol we want to select
                // and use those, this one will be the closest one to the range we are interested
                // in
                let mut outline_nodes_with_distance = outline_nodes
                    .into_iter()
                    .filter(|outline_node| outline_node.name() == symbol_to_edit.symbol_name())
                    .map(|outline_node| {
                        (
                            outline_node
                                .range()
                                .minimal_line_distance(symbol_to_edit.range()),
                            outline_node,
                        )
                    })
                    .collect::<Vec<_>>();

                outline_nodes_with_distance.sort_by_key(|(distance, _)| *distance);
                if outline_nodes_with_distance.is_empty() {
                    Err(SymbolError::NoOutlineNodeSatisfyPosition)
                } else {
                    let selected_outline_node = outline_nodes_with_distance.remove(0);
                    if selected_outline_node.1.is_file() {
                        // if our range is within the range of the outline node, that
                        // means we are doing an anchored edit
                        // File range: [S........................E]
                        // Our range : [...........S...........E..]
                        let outline_node_content = selected_outline_node.1.consume_content();
                        if outline_node_content
                            .range()
                            .contains_check_line(symbol_to_edit.range())
                        {
                            println!("selection_range_selected::{:?}", &symbol_to_edit.range());
                            Ok(outline_node_content.set_range(symbol_to_edit.range().clone()))
                        } else {
                            println!(
                                "selection_range_selected_not_selected::{:?})",
                                &outline_node_content.range()
                            );
                            Ok(outline_node_content)
                        }
                    } else {
                        return Ok(outline_nodes_with_distance.remove(0).1.content().clone());
                    }
                }
            }
        }
    }

    pub fn detect_language(&self, fs_file_path: &str) -> Option<String> {
        self.editor_parsing
            .for_file_path(fs_file_path)
            .map(|ts_language_config| ts_language_config.language_str.to_owned())
    }

    /// We want to check for followups on the functions which implies that we can
    /// simply look at the places where these functions are being used and then just
    /// do go-to-reference on it
    async fn check_for_followups_on_functions(
        &self,
        outline_node: OutlineNodeContent,
        symbol_edited: &SymbolToEdit,
        symbol_followup_bfs: &SymbolFollowupBFS,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: ToolProperties,
    ) -> Result<Vec<SymbolFollowupBFS>, SymbolError> {
        let mut reference_locations = vec![];
        println!(
            "tool_box::check_for_followups::is_function_type_edit::({})",
            outline_node.name()
        );
        // for functions its very easy, we have to just get the references which
        // are using this function somewhere in their code
        let references = self
            .go_to_references(
                symbol_edited.fs_file_path().to_owned(),
                outline_node.identifier_range().start_position(),
                message_properties.clone(),
            )
            .await;
        if references.is_ok() {
            reference_locations.extend(references.expect("is_ok to hold").locations());
        }

        // Now that we have the reference locations we want to execute changes to the outline nodes containing the reference
        let outline_nodes_to_edit = stream::iter(
            reference_locations
                .iter()
                .map(|refernece_location| refernece_location.fs_file_path().to_owned())
                .collect::<HashSet<String>>()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            self.get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .flatten()
        .filter(|outline_node| {
            let outline_node_range = outline_node.range();
            reference_locations.iter().any(|reference_location| {
                outline_node_range.contains_check_line_column(reference_location.range())
            })
        })
        .collect::<Vec<_>>();

        let function_name = symbol_followup_bfs.symbol_edited().symbol_name();
        let function_file_path = symbol_followup_bfs.symbol_edited().fs_file_path();
        let original_code = symbol_followup_bfs.original_code();
        let edited_code = symbol_followup_bfs.edited_code();
        for outline_node_to_edit in outline_nodes_to_edit.to_vec().into_iter() {
            let _ = self
                .send_edit_instruction_to_outline_node(
                    outline_node_to_edit,
                    format!(r#"A dependency of this code has changed. You are given the list of changes below:
<dependency>
<name>
{function_name}
</name>
<fs_file_path>
{function_file_path}
</fs_file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#),
                    hub_sender.clone(),
                    message_properties.clone(),
                    tool_properties.clone(),
                )
                .await;
        }

        // Now that we have sent the edit requests, we want to grab these outline
        // nodes again after they have changed and create the followup requests
        let mut symbol_followup_bfs = vec![];
        for outline_node_to_edit in outline_nodes_to_edit.into_iter() {
            let new_outline_node = self
                .find_sub_symbol_to_edit_with_name(
                    outline_node_to_edit.name(),
                    &SymbolToEdit::new(
                        outline_node.name().to_owned(),
                        outline_node.range().clone(),
                        outline_node.fs_file_path().to_owned(),
                        vec![],
                        false,
                        false,
                        false,
                        "".to_owned(),
                        None,
                        false,
                        None,
                        true,
                        None,
                        vec![],
                        None,
                    ),
                    message_properties.clone(),
                )
                .await?;
            symbol_followup_bfs.push(SymbolFollowupBFS::new(
                SymbolToEdit::new(
                    new_outline_node.name().to_owned(),
                    new_outline_node.range().clone(),
                    new_outline_node.fs_file_path().to_ascii_lowercase(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                SymbolIdentifier::with_file_path(
                    new_outline_node.name(),
                    new_outline_node.fs_file_path(),
                ),
                outline_node_to_edit.content().content().to_owned(),
                new_outline_node.content().to_owned(),
            ));
        }
        Ok(symbol_followup_bfs)
    }

    /// We want to check for followups on the class definitions which implies
    /// that we want to change any implementation of the class which might have
    /// changed
    async fn check_for_followups_class_definitions(
        &self,
        class_outline_node: OutlineNodeContent,
        symbol_edited: &SymbolToEdit,
        class_symbol_followup: &SymbolFollowupBFS,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: ToolProperties,
    ) -> Result<Vec<SymbolFollowupBFS>, SymbolError> {
        let mut reference_locations = vec![];
        println!(
            "tool_box::check_for_followups::is_class_definition::({})",
            class_outline_node.name()
        );

        let language_config = self
            .editor_parsing
            .for_file_path(class_outline_node.fs_file_path());
        if language_config.is_none() {
            return Ok(vec![]);
        }
        let language_config = language_config.expect("is_none to hold");

        let original_code = class_symbol_followup.original_code();
        let edited_code = class_symbol_followup.edited_code();
        let class_symbol_name = class_outline_node.name();
        let class_fs_file_path = class_outline_node.fs_file_path();
        // if this is a class definitions then we have to be a bit more careful
        // and look at where this class definition is being used and follow those
        // reference
        let references = self
            .go_to_references(
                symbol_edited.fs_file_path().to_owned(),
                class_outline_node.identifier_range().start_position(),
                message_properties.clone(),
            )
            .await;

        if references.is_ok() {
            reference_locations.extend(
                references
                    .expect("is_ok to hold")
                    .locations()
                    .into_iter()
                    .map(|location| (location, class_symbol_followup.clone())),
            );
        }

        // Now we have to do the following for completeness:
        // - find all the fucntions which belong to this class
        // - order them in a topological sort order and then go about making changes
        // - the challenge here is that we might have dependencies which might be spread
        // across different implementation blocks so we have to carefully craft this out
        // - the more they are present in the same block the better it is
        // having said all of this, the dumb way is the best way
        // the dumb way is to show the whole symbol implementation blocks and ask
        // the model to make any changes required (especially if its a single block or all in a single file
        // which is the majority case in our codebase, if there are multiple files which have this
        // then we can do it per file)
        let file_paths = reference_locations
            .iter()
            .map(|(reference_location, _)| reference_location.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        // outline nodes which contain any children which contains a reference
        // to the original symbol
        let outline_nodes = stream::iter(
            file_paths
                .into_iter()
                .map(|file_path| (file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            self.get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .flatten()
        .filter(|outline_node| {
            // now check which outline node belongs to the references
            let outline_node_range = outline_node.range();
            reference_locations.iter().any(|(reference_location, _)| {
                outline_node_range.contains_check_line_column(reference_location.range())
            })
        })
        .filter(|outline_node| outline_node.name() == class_symbol_name)
        .filter(|outline_node| {
            outline_node.children().into_iter().any(|child_node| {
                let child_range = child_node.range();
                reference_locations.iter().any(|(reference_location, _)| {
                    child_range.contains_check_line_column(reference_location.range())
                })
            })
        })
        .collect::<Vec<_>>();

        // now we can execute the edits on each of these files
        let prompt = format!(
            r#"A dependency of this code has changed. You are given the list of changes below:
<dependency>
<name>
{class_symbol_name}
</name>
<fs_file_path>
{class_fs_file_path}
</fs_file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#
        );

        println!(
            "tool_box::check_for_followups_class_definitions::symbol_name({})::outline_nodes({})",
            class_symbol_name,
            outline_nodes.len()
        );
        let _ = stream::iter(outline_nodes.to_vec().into_iter().map(|data| {
            (
                data,
                hub_sender.clone(),
                message_properties.clone(),
                tool_properties.clone(),
                prompt.to_owned(),
            )
        }))
        .map(
            |(outline_node, hub_sender, message_properties, tool_properties, prompt)| async move {
                self.send_edit_instruction_to_outline_node(
                    outline_node,
                    prompt,
                    hub_sender,
                    message_properties,
                    tool_properties,
                )
                .await
            },
        )
        .buffer_unordered(1)
        .collect::<Vec<_>>()
        .await;

        // TODO(skcd): now we want to capture the methods which have changed since those are the
        // ones which we want follow after changing the symbol (the methods over here)
        // the symbol might have moved, so we want to make sure that we do it correctly
        let mut references_to_symbol_followup = vec![];

        for outline_node in outline_nodes.into_iter() {
            let symbol_name = outline_node.name();
            let original_code = outline_node.content().content();
            let new_outline_node = self
                .find_sub_symbol_to_edit_with_name(
                    symbol_name,
                    &SymbolToEdit::new(
                        outline_node.name().to_owned(),
                        outline_node.range().clone(),
                        outline_node.fs_file_path().to_owned(),
                        vec![],
                        false,
                        false,
                        false,
                        "".to_owned(),
                        None,
                        false,
                        None,
                        true,
                        None,
                        vec![],
                        None,
                    ),
                    message_properties.clone(),
                )
                .await;

            // Now we want to compare the changed functions which are present in the new outline node and the older outline node
            // which is where we want to go for references
            if let Ok(new_outline_node) = new_outline_node {
                // the ranges here are all messed up since we are computing relative to
                // Position::new(0, 0, 0) ...ugh
                let edited_code = new_outline_node.content();
                let older_outline_nodes = language_config
                    .generate_outline_fresh(original_code.as_bytes(), symbol_edited.fs_file_path())
                    .into_iter()
                    .find(|outline_node| outline_node.name() == class_symbol_name);
                let newer_outline_nodes = language_config
                    .generate_outline_fresh(edited_code.as_bytes(), symbol_edited.fs_file_path())
                    .into_iter()
                    .find(|outline_node| outline_node.name() == class_symbol_name);

                // our outline nodes are matching up over here
                if let (Some(new_outline_nodes), Some(old_outline_node)) =
                    (newer_outline_nodes, older_outline_nodes)
                {
                    // now find the child nodes which are also present on the old outline nodes
                    let changed_function_nodes = new_outline_nodes
                        .children()
                        .into_iter()
                        .filter_map(|new_child_outline_node| {
                            let old_child_outline_node =
                                old_outline_node.children().iter().find(|old_child_node| {
                                    old_child_node.name() == new_child_outline_node.name()
                                });
                            if let Some(old_child_outline_node) = old_child_outline_node {
                                if old_child_outline_node.content()
                                    != new_child_outline_node.content()
                                {
                                    Some((old_child_outline_node.content(), new_child_outline_node))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();

                    let function_names = changed_function_nodes
                        .iter()
                        .map(|function_node| function_node.1.name())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!(
                        "tool_box::check_for_followups::symbol_name({})::functions_changed::({})",
                        class_symbol_name, function_names
                    );

                    // for the changed function nodes we want to go over the references
                    // and only find the references which are not part of the current symbol
                    for (old_content, changed_function_node) in changed_function_nodes.into_iter() {
                        let references_for_functions = self
                            .go_to_references(
                                changed_function_node.fs_file_path().to_owned(),
                                changed_function_node
                                    .identifier_range()
                                    .start_position()
                                    .move_lines(new_outline_node.range().start_line()),
                                message_properties.clone(),
                            )
                            .await?
                            .locations();
                        // the interesting part here is that we are showing the changed
                        // function content but not idea on which class symbol it is
                        // part of
                        references_to_symbol_followup.extend(
                            references_for_functions.into_iter().map(|reference| {
                                (
                                    reference,
                                    SymbolFollowupBFS::new(
                                        SymbolToEdit::new(
                                            changed_function_node.name().to_owned(),
                                            changed_function_node.range().clone(),
                                            changed_function_node.fs_file_path().to_owned(),
                                            vec![],
                                            false,
                                            false,
                                            false,
                                            "".to_owned(),
                                            None,
                                            false,
                                            None,
                                            true,
                                            None,
                                            vec![],
                                            None,
                                        ),
                                        SymbolIdentifier::with_file_path(
                                            class_symbol_name,
                                            class_fs_file_path,
                                        ),
                                        old_content.to_owned(),
                                        changed_function_node.content().to_owned(),
                                    ),
                                )
                            }),
                        )
                    }
                }
            }
        }

        // we send the request for editing the full outline node where the reference location is contained
        // and only if it does not belong to the same class block, and just edit it out
        // once we have done the edit we need to grab what has changed and send it over again
        let class_symbol_new_location = self
            .find_sub_symbol_to_edit_with_name(
                class_symbol_name,
                &SymbolToEdit::new(
                    class_outline_node.name().to_owned(),
                    class_outline_node.range().clone(),
                    class_outline_node.fs_file_path().to_owned(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                message_properties.clone(),
            )
            .await;

        if let Ok(class_symbol_new_location) = class_symbol_new_location {
            // we have the new location, now we go to the references for this
            let references = self
                .go_to_references(
                    class_symbol_new_location.fs_file_path().to_owned(),
                    class_symbol_new_location.range().start_position(),
                    message_properties.clone(),
                )
                .await?
                .locations();
            references_to_symbol_followup.extend(
                references
                    .into_iter()
                    .map(|reference| (reference, class_symbol_followup.clone())),
            );
        }

        println!("tool_box::check_for_followups_class_definitions::symbol_name({})::references_locations({})", class_symbol_name, references_to_symbol_followup.iter().map(|(reference_location, _)| reference_location.fs_file_path().to_owned()).collect::<Vec<_>>().join(","));

        // Now that we have the references for functions, we need to filter out which do not belong
        // to the current class which is getting edited
        let file_paths = references_to_symbol_followup
            .iter()
            .map(|(reference, _)| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();
        let outline_nodes_to_file_paths = stream::iter(
            file_paths
                .into_iter()
                .map(|file_path| (file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes = self
                .get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await;
            outline_nodes.map(|outline_nodes| (fs_file_path, outline_nodes))
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<_, _>>();

        // Now that we have the outline nodes for each file path, we want to find the outline nodes
        // which interesect with the references we are interested in and create the prompt for editing
        // them out
        let editable_outline_nodes = outline_nodes_to_file_paths
            .into_iter()
            .map(|(_, outline_nodes)| {
                outline_nodes
                    .into_iter()
                    .filter(|outline_node| outline_node.name() != class_symbol_name)
                    .filter(|outline_node| {
                        let outline_node_range = outline_node.range();
                        references_to_symbol_followup
                            .iter()
                            .any(|(reference_location, _)| {
                                outline_node_range
                                    .contains_check_line_column(reference_location.range())
                                    // check if the reference is on the same file as the outline node
                                    && reference_location.fs_file_path()
                                        == outline_node.fs_file_path()
                            })
                    })
            })
            .flatten()
            .collect::<Vec<_>>();

        // Now that we have the outline nodes which require editing, we can send an edit request
        // for editing them
        for editable_outline_node in editable_outline_nodes.to_vec().into_iter() {
            // first get the symbol-followup-bfs requests which belong to this outline node
            let editable_outline_node_range = editable_outline_node.range();
            let prompt_for_editing = references_to_symbol_followup
                .iter()
                .filter_map(|(reference_location, symbol_followup_bfs)| {
                    if editable_outline_node_range
                        .contains_check_line_column(reference_location.range())
                    {
                        Some(symbol_followup_bfs)
                    } else {
                        None
                    }
                })
                .map(|symbol_followup_bfs| {
                    let name = symbol_followup_bfs.symbol_edited().symbol_name();
                    let parent_symbol_name = symbol_followup_bfs
                        .symbol_identifier()
                        .symbol_name()
                        .to_owned();
                    let name = if parent_symbol_name != name {
                        if language_config.is_rust() {
                            format!("{}::{}", parent_symbol_name, name)
                        } else {
                            format!("{}.{}", parent_symbol_name, name)
                        }
                    } else {
                        name.to_owned()
                    };
                    let fs_file_path = symbol_followup_bfs.symbol_edited().fs_file_path();
                    let original_code = symbol_followup_bfs.original_code();
                    let edited_code = symbol_followup_bfs.edited_code();
                    format!(
                        r#"<dependency>
<name>
{name}
</name>
<file_path>
{fs_file_path}
</file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !prompt_for_editing.trim().is_empty() {
                let _ = self
                    .send_edit_instruction_to_outline_node(
                        editable_outline_node,
                        format!(
                            r#"A dependency of this code has changed. You are given the list of changes below:
{prompt_for_editing}
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#
                        ),
                        hub_sender.clone(),
                        message_properties.clone(),
                        tool_properties.clone(),
                    )
                    .await;
            }
        }

        // now that we have edited all the outline nodes we are interested in
        // we need to look at the nodes again and see what has changed to generate
        // the followup queries or we can just send the request as is and take care
        // of it during the followup depending on anything changing or not (which is simpler)
        let mut final_followup_requests = vec![];
        for editable_outline_node in editable_outline_nodes.into_iter() {
            let outline_node_new_content = self
                .find_sub_symbol_to_edit_with_name(
                    editable_outline_node.name(),
                    &SymbolToEdit::new(
                        editable_outline_node.name().to_owned(),
                        editable_outline_node.range().clone(),
                        editable_outline_node.fs_file_path().to_owned(),
                        vec![],
                        false,
                        false,
                        true,
                        "".to_owned(),
                        None,
                        false,
                        None,
                        true,
                        None,
                        vec![],
                        None,
                    ),
                    message_properties.clone(),
                )
                .await?;
            final_followup_requests.push(SymbolFollowupBFS::new(
                SymbolToEdit::new(
                    outline_node_new_content.name().to_owned(),
                    outline_node_new_content.range().clone(),
                    outline_node_new_content.fs_file_path().to_owned(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                SymbolIdentifier::with_file_path(
                    outline_node_new_content.name(),
                    outline_node_new_content.fs_file_path(),
                ),
                editable_outline_node.content().content().to_owned(),
                outline_node_new_content.content().to_owned(),
            ));
        }
        Ok(final_followup_requests)
    }

    /// We want to make sure that class implementation chagnes follow the following
    /// state machine:
    /// class-implementation -> changes class definitions
    /// if class-definitions change:
    /// change everything about the class implementations and also the functions
    /// which previous changed
    /// once this is done get the functions which have chagned along with any
    /// places which might be referencing the class itself
    async fn check_for_followups_class_implementation(
        &self,
        class_implementation_outline_node: OutlineNodeContent,
        symbol_followup: &SymbolFollowupBFS,
        original_code: &str,
        edited_code: &str,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<Vec<SymbolFollowupBFS>, SymbolError> {
        println!(
            "tool_box::check_for_followups::is_class_implementation_type::({})",
            class_implementation_outline_node.name()
        );
        let language_config = self
            .editor_parsing
            .for_file_path(class_implementation_outline_node.fs_file_path());
        if language_config.is_none() {
            return Ok(vec![]);
        }
        let language_config = language_config.expect("is_none to hold");
        let class_implementation_name = class_implementation_outline_node.name();
        let outline_node_file_path = class_implementation_outline_node.fs_file_path();
        // we should always trigger an edit on the class symbol definition by itself
        // just to make sure that if any changes are required to it, they are completed
        // and managed accordingly
        // the state-machine when we chagne the definition is as follows:
        // class-implementation -> check if class-definition needs change
        // class-definition changed -> check if any of the class-implementation reference blocks point to class definition and trigger a search and replace block
        let mut definitions = self
            .go_to_definition(
                class_implementation_outline_node.fs_file_path(),
                class_implementation_outline_node
                    .identifier_range()
                    .start_position(),
                message_properties.clone(),
            )
            .await?
            .definitions();

        // changed content for the class definition
        // is being tracked over here
        let mut class_implementations_interested_references = vec![];
        if !definitions.is_empty() {
            let first_definition = definitions.remove(0);
            // if the definition does not belong to the outline nodo where
            // we are focussed on right now, then skip the step
            if first_definition.file_path() != class_implementation_outline_node.fs_file_path()
                || !class_implementation_outline_node
                    .range()
                    .contains_check_line_column(class_implementation_outline_node.range())
            {
                let outline_node = self
                    .get_outline_node_for_range(
                        first_definition.range(),
                        first_definition.file_path(),
                        message_properties.clone(),
                    )
                    .await?;

                let original_definition_code = outline_node.content().content().to_owned();

                println!("tool_box::check_for_followup_bfs::class_implementation::definition_check::({})::({})", outline_node.name(), outline_node.fs_file_path());
                // Now send over an edit request to this outline node
                // TODO(skcd): This is heavily unoptimised right now, since we are not changing just the changes
                // but the whole symbol together so it slows down the whole pipeline
                let _ = self.send_edit_instruction_to_outline_node(
                outline_node,
                {
                    let name = symbol_followup.symbol_edited().symbol_name();
                    let fs_file_path = symbol_followup.symbol_edited().fs_file_path();
                    let parent_symbol_name = symbol_followup.symbol_identifier().symbol_name();
                    let name = if parent_symbol_name != name {
                        if language_config.is_rust() {
                            format!("{}::{}", parent_symbol_name, name)
                        } else {
                            format!("{}.{}", parent_symbol_name, name)
                        }
                    } else {
                        name.to_owned()
                    };
                    format!(r#"A dependency of this code has changed. You are given the list of changes below:
<dependency>
<name>
{name}
</name>
<fs_file_path>
{fs_file_path}
</fs_file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#)},
                hub_sender.clone(),
                message_properties.clone(),
                tool_properties.clone(),
            )
            .await;
                // now we want to check if the definition has changed over here
                let changed_outline_node = self
                    .get_outline_nodes_grouped(first_definition.file_path())
                    .await
                    .map(|outline_nodes| {
                        let mut filtered_outline_nodes = outline_nodes
                            .into_iter()
                            .filter(|outline_node| outline_node.is_class_definition())
                            .filter(|outline_node| outline_node.name() == outline_node.name())
                            .collect::<Vec<_>>();
                        if filtered_outline_nodes.is_empty() {
                            None
                        } else {
                            Some(filtered_outline_nodes.remove(0))
                        }
                    })
                    .flatten();
                if let Some(changed_outline_node) = changed_outline_node {
                    if changed_outline_node.content().content().trim()
                        != original_definition_code.trim()
                    {
                        // we also want to get the references for the class definition node
                        // which point to self
                        let class_definition_references = self
                            .go_to_references(
                                changed_outline_node.fs_file_path().to_owned(),
                                changed_outline_node.range().start_position(),
                                message_properties.clone(),
                            )
                            .await;
                        if let Ok(class_definition_references) = class_definition_references {
                            class_implementations_interested_references.extend(
                                class_definition_references.locations().into_iter().map(
                                    |location| {
                                        (
                                            location,
                                            SymbolFollowupBFS::new(
                                                SymbolToEdit::new(
                                                    changed_outline_node.name().to_owned(),
                                                    changed_outline_node.range().clone(),
                                                    changed_outline_node.fs_file_path().to_owned(),
                                                    vec![],
                                                    false,
                                                    false,
                                                    true,
                                                    "".to_owned(),
                                                    None,
                                                    false,
                                                    None,
                                                    true,
                                                    None,
                                                    vec![],
                                                    None,
                                                ),
                                                SymbolIdentifier::with_file_path(
                                                    changed_outline_node.name(),
                                                    changed_outline_node.fs_file_path(),
                                                ),
                                                original_definition_code.to_owned(),
                                                changed_outline_node.content().content().to_owned(),
                                            ),
                                        )
                                    },
                                ),
                            )
                        }
                    }
                }
            }
        }

        // if class-definition change has happened, we have to go through all the
        // implementation blocks which contain this class definition along with
        // any changed functions and their references
        // the ranges here are all messed up since we are computing relative to
        // Position::new(0, 0, 0) ...ugh
        let older_outline_nodes = language_config
            .generate_outline_fresh(original_code.as_bytes(), outline_node_file_path)
            .into_iter()
            .find(|outline_node| outline_node.name() == class_implementation_name);
        let newer_outline_nodes = language_config
            .generate_outline_fresh(edited_code.as_bytes(), outline_node_file_path)
            .into_iter()
            .find(|outline_node| outline_node.name() == class_implementation_name);

        let class_implementation_block_new_location = self
            .find_sub_symbol_to_edit_with_name(
                class_implementation_outline_node.name(),
                &SymbolToEdit::new(
                    class_implementation_outline_node.name().to_owned(),
                    class_implementation_outline_node.range().clone(),
                    class_implementation_outline_node.fs_file_path().to_owned(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                message_properties.clone(),
            )
            .await?;

        let start_line = class_implementation_block_new_location
            .range()
            .start_position()
            .line();

        // our outline nodes are matching up over here
        if let (Some(new_outline_node), Some(old_outline_node)) =
            (newer_outline_nodes, older_outline_nodes)
        {
            // now find the child nodes which are also present on the old outline nodes
            let changed_function_nodes = new_outline_node
                .children()
                .into_iter()
                .filter_map(|new_child_outline_node| {
                    let old_child_outline_node =
                        old_outline_node.children().iter().find(|old_child_node| {
                            old_child_node.name() == new_child_outline_node.name()
                        });
                    if let Some(old_child_outline_node) = old_child_outline_node {
                        if old_child_outline_node.content() != new_child_outline_node.content() {
                            Some((old_child_outline_node.content(), new_child_outline_node))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            // now that we have the changed function nodes we can try and find the references where we want to go for this
            // and the ones which belong to the same class (that is very important we are still in the first branch over here)
            for (old_content, changed_function_node) in changed_function_nodes.into_iter() {
                let references_for_functions = self
                    .go_to_references(
                        changed_function_node.fs_file_path().to_owned(),
                        changed_function_node
                            .identifier_range()
                            .start_position()
                            // we want to get the accurate line because when we are parsing on top of just
                            // the changed content we will start with 0 index since we do not have the file content
                            .move_lines(start_line),
                        message_properties.clone(),
                    )
                    .await?
                    .locations();
                class_implementations_interested_references.extend(
                    references_for_functions.into_iter().map(|reference| {
                        (
                            reference,
                            SymbolFollowupBFS::new(
                                SymbolToEdit::new(
                                    changed_function_node.name().to_owned(),
                                    changed_function_node.range().clone(),
                                    changed_function_node.fs_file_path().to_owned(),
                                    vec![],
                                    false,
                                    false,
                                    false,
                                    "".to_owned(),
                                    None,
                                    false,
                                    None,
                                    true,
                                    None,
                                    vec![],
                                    None,
                                ),
                                SymbolIdentifier::with_file_path(
                                    class_implementation_name,
                                    outline_node_file_path,
                                ),
                                old_content.to_owned(),
                                changed_function_node.content().to_owned(),
                            ),
                        )
                    }),
                );
            }
        }

        // now that we have all the references and where we want to go towards, we have to find the outline nodes which these referneces belong to
        // and make sure that the edit request we are sending is only for the outline nodes which belong to the class
        let file_paths = class_implementations_interested_references
            .iter()
            .map(|(reference_location, _)| reference_location.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        let outline_nodes_which_belong_to_class = stream::iter(
            file_paths
                .into_iter()
                .map(|file_path| (file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            self.get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .flatten()
        // only worry about outline nodes which are part of the class
        .filter(|outline_node| outline_node.name() == class_implementation_name)
        // only look at the class implementation blocks and not on the definition again
        .filter(|outline_node| outline_node.is_class())
        .collect::<Vec<_>>();

        // now we want to send an edit request with the changes which have happend and have a reference to this outline node
        for outline_nodes_to_change in outline_nodes_which_belong_to_class.to_vec().into_iter() {
            let prompt_for_editing = class_implementations_interested_references
                .iter()
                .filter_map(|(reference_location, symbol_followup_bfs)| {
                    // this will by definition include all the nodes since we might have the changed class definition
                    if outline_nodes_to_change
                        .children()
                        .into_iter()
                        .any(|children| {
                            let child_range = children.range();
                            child_range.contains(reference_location.range())
                        })
                    {
                        Some(symbol_followup_bfs)
                    } else {
                        None
                    }
                })
                .map(|symbol_followup_bfs| {
                    let name = symbol_followup_bfs.symbol_edited().symbol_name();
                    let fs_file_path = symbol_followup_bfs.symbol_edited().fs_file_path();
                    let original_code = symbol_followup_bfs.original_code();
                    let edited_code = symbol_followup_bfs.edited_code();
                    let parent_symbol_name = symbol_followup_bfs.symbol_identifier().symbol_name();
                    let name = if parent_symbol_name != name {
                        if language_config.is_rust() {
                            format!("{}::{}", parent_symbol_name, name)
                        } else {
                            format!("{}.{}", parent_symbol_name, name)
                        }
                    } else {
                        name.to_owned()
                    };
                    format!(
                        r#"<dependency>
<name>
{name}
</name>
<file_path>
{fs_file_path}
</file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !prompt_for_editing.trim().is_empty() {
                let _ = self.send_edit_instruction_to_outline_node(
                    outline_nodes_to_change,
                    format!(r#"A dependency of this code has changed. You are given the list of changes below:
{prompt_for_editing}
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#),
                    hub_sender.clone(),
                    message_properties.clone(),
                    tool_properties.clone(),
                )
                .await;
            }
        }

        let mut references_to_check_for_followups = vec![];
        // now we want to grab the function nodes along with the class definition references which has changed and then send an edit
        // request to the outline node which is outside the class
        for outline_node_belonging_to_class in outline_nodes_which_belong_to_class.into_iter() {
            // First get the new outline node for this one
            let new_outline_node = self
                .find_sub_symbol_to_edit_with_name(
                    outline_node_belonging_to_class.name(),
                    &SymbolToEdit::new(
                        outline_node_belonging_to_class.name().to_owned(),
                        outline_node_belonging_to_class.range().clone(),
                        outline_node_belonging_to_class.fs_file_path().to_owned(),
                        vec![],
                        false,
                        false,
                        true,
                        "".to_owned(),
                        None,
                        false,
                        None,
                        true,
                        None,
                        vec![],
                        None,
                    ),
                    message_properties.clone(),
                )
                .await?;

            // Now we want to get the changed symbols over here and compare it to the older version and get the delta between the old and new
            let older_outline_nodes = language_config
                .generate_outline_fresh(
                    outline_node_belonging_to_class
                        .content()
                        .content()
                        .as_bytes(),
                    outline_node_file_path,
                )
                .into_iter()
                .find(|outline_node| outline_node.name() == class_implementation_name);
            let newer_outline_nodes = language_config
                .generate_outline_fresh(
                    new_outline_node.content().as_bytes(),
                    outline_node_file_path,
                )
                .into_iter()
                .find(|outline_node| outline_node.name() == class_implementation_name);

            // Once we have these, we have to find the children which are different in both these blocks and only keep track of those references
            if let (Some(new_outline_node), Some(old_outline_node)) =
                (newer_outline_nodes, older_outline_nodes)
            {
                // now find the child nodes which are also present on the old outline nodes
                let changed_function_nodes = new_outline_node
                    .children()
                    .into_iter()
                    .filter_map(|new_child_outline_node| {
                        let old_child_outline_node =
                            old_outline_node.children().iter().find(|old_child_node| {
                                old_child_node.name() == new_child_outline_node.name()
                            });
                        if let Some(old_child_outline_node) = old_child_outline_node {
                            if old_child_outline_node.content() != new_child_outline_node.content()
                            {
                                Some((old_child_outline_node.content(), new_child_outline_node))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                // now that we have the changed function nodes we can try and find the references where we want to go for this
                // and the ones which belong to the same class (that is very important we are still in the first branch over here)
                for (old_content, changed_function_node) in changed_function_nodes.into_iter() {
                    let references_for_functions = self
                        .go_to_references(
                            changed_function_node.fs_file_path().to_owned(),
                            changed_function_node
                                .identifier_range()
                                .start_position()
                                // we want to get the accurate line because when we are parsing on top of just
                                // the changed content we will start with 0 index since we do not have the file content
                                .move_lines(start_line),
                            message_properties.clone(),
                        )
                        .await?
                        .locations();
                    references_to_check_for_followups.extend(
                        references_for_functions.into_iter().map(|reference| {
                            (
                                reference,
                                SymbolFollowupBFS::new(
                                    SymbolToEdit::new(
                                        changed_function_node.name().to_owned(),
                                        changed_function_node.range().clone(),
                                        changed_function_node.fs_file_path().to_owned(),
                                        vec![],
                                        false,
                                        false,
                                        false,
                                        "".to_owned(),
                                        None,
                                        false,
                                        None,
                                        true,
                                        None,
                                        vec![],
                                        None,
                                    ),
                                    SymbolIdentifier::with_file_path(
                                        class_implementation_name,
                                        outline_node_file_path,
                                    ),
                                    old_content.to_owned(),
                                    changed_function_node.content().to_owned(),
                                ),
                            )
                        }),
                    );
                }
            }
        }

        let file_paths = references_to_check_for_followups
            .iter()
            .map(|(reference, _)| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        // outline nodes which require editing
        let outline_nodes_to_file_paths = stream::iter(
            file_paths
                .into_iter()
                .map(|file_path| (file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes = self
                .get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await;
            outline_nodes
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .flatten()
        // we do not want to follow the class outline nodes
        .filter(|outline_node| outline_node.name() != class_implementation_name)
        .filter(|outline_node| {
            // make sure that the outline node contains one of the references
            let outline_node_range = outline_node.range();
            references_to_check_for_followups
                .iter()
                .any(|(reference, _)| {
                    outline_node_range.contains_check_line_column(reference.range())
                })
        })
        .collect::<Vec<_>>();

        // now we want to grab the symbolfollowup request on each of these outline nodes where we sent the request
        for outline_node_to_edit in outline_nodes_to_file_paths.to_vec().into_iter() {
            let prompt_for_editing = references_to_check_for_followups
                .iter()
                .filter_map(|(reference_location, symbol_followup_bfs)| {
                    // this will by definition include all the nodes since we might have the changed class definition
                    if outline_node_to_edit.children().into_iter().any(|children| {
                        let child_range = children.range();
                        child_range.contains(reference_location.range())
                    }) {
                        Some(symbol_followup_bfs)
                    } else {
                        None
                    }
                })
                .map(|symbol_followup_bfs| {
                    let name = symbol_followup_bfs.symbol_edited().symbol_name();
                    let fs_file_path = symbol_followup_bfs.symbol_edited().fs_file_path();
                    let original_code = symbol_followup_bfs.original_code();
                    let edited_code = symbol_followup_bfs.edited_code();
                    let parent_symbol_name = symbol_followup_bfs.symbol_identifier().symbol_name();
                    let name = if parent_symbol_name != name {
                        if language_config.is_rust() {
                            format!("{}::{}", parent_symbol_name, name)
                        } else {
                            format!("{}.{}", parent_symbol_name, name)
                        }
                    } else {
                        name.to_owned()
                    };
                    format!(
                        r#"<dependency>
<name>
{name}
</name>
<file_path>
{fs_file_path}
</file_path>
<original_implementation>
{original_code}
</original_implementation>
<updated_implementation>
{edited_code}
</updated_implementation>
</dependency>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !prompt_for_editing.trim().is_empty() {
                let _ = self
                    .send_edit_instruction_to_outline_node(
                        outline_node_to_edit,
                        format!(r#"A dependency of this code has changed. You are given the list of changes below:
{prompt_for_editing}
Please update this code to accommodate these changes. Consider:
1. Method signature changes (parameters, return types)
2. Behavioural changes in the dependency
3. Potential side effects or new exceptions
4. Deprecated features that should no longer be used
5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required."#),
                        hub_sender.clone(),
                        message_properties.clone(),
                        tool_properties.clone(),
                    )
                    .await;
            }
        }

        let mut final_followup_requests = vec![];
        for outline_node_to_follow in outline_nodes_to_file_paths.into_iter() {
            let outline_node_new_content = self
                .find_sub_symbol_to_edit_with_name(
                    outline_node_to_follow.name(),
                    &SymbolToEdit::new(
                        outline_node_to_follow.name().to_owned(),
                        outline_node_to_follow.range().clone(),
                        outline_node_to_follow.fs_file_path().to_owned(),
                        vec![],
                        false,
                        false,
                        true,
                        "".to_owned(),
                        None,
                        false,
                        None,
                        true,
                        None,
                        vec![],
                        None,
                    ),
                    message_properties.clone(),
                )
                .await?;
            final_followup_requests.push(SymbolFollowupBFS::new(
                SymbolToEdit::new(
                    outline_node_new_content.name().to_owned(),
                    outline_node_new_content.range().clone(),
                    outline_node_new_content.fs_file_path().to_owned(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                SymbolIdentifier::with_file_path(
                    outline_node_new_content.name(),
                    outline_node_new_content.fs_file_path(),
                ),
                outline_node_to_follow.content().content().to_owned(),
                outline_node_new_content.content().to_owned(),
            ));
        }

        Ok(final_followup_requests)
    }

    /// To get the followups working we have to do the following:
    /// we are going to heal the codebase in waves:
    /// - we have a set of symbols which have been edited and want to do followups
    /// on top of that
    /// - we go to references and deduplicate and then do the editing
    /// once we are done with it, we again need to go to references
    /// TODO(skcd): heavily unoptimised code
    pub async fn check_for_followups_bfs(
        &self,
        mut symbol_followups: Vec<SymbolFollowupBFS>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        println!("tool_box::check_for_followups_bfs::start");
        // first we want to detect if there were changes and if there were what
        // those changes are about
        // we want to track the reference location along with the changed symbol_followup
        // so we can pass the correct git-diff to it
        let instant = std::time::Instant::now();
        loop {
            if symbol_followups.is_empty() {
                // break when we have no more followups to do
                break;
            }
            // empty the reference locations at the start of the invocation as it
            // will get populated down the line
            let reference_locations = stream::iter(symbol_followups.into_iter().map(|reference_location| (reference_location, message_properties.clone(), hub_sender.clone()))).map(|(symbol_followup, message_properties, hub_sender)| async move {
                let mut reference_locations = vec![];
                let symbol_edited = symbol_followup.symbol_edited();
                let symbol_identifier = symbol_followup.symbol_identifier();
                let original_code = symbol_followup.original_code();
                let edited_code = symbol_followup.edited_code();
                println!(
                    "tool_box::check_for_followup_bfs::symbol_name({})",
                    symbol_identifier.symbol_name()
                );
                if original_code.trim() == edited_code.trim() {
                    println!(
                        "tool_box::check_for_followup_bfs::same_code_original_edited::({})",
                        symbol_identifier.symbol_name()
                    );
                    return vec![];
                }
                let outline_node = self
                    .find_sub_symbol_to_edit_with_name(
                        symbol_identifier.symbol_name(),
                        symbol_edited,
                        message_properties.clone(),
                    )
                    .await;
                if outline_node.is_err() {
                    return vec![];
                }

                let outline_node = outline_node.expect("is_err to not fail above");

                if outline_node.is_function_type() {
                    println!("tool_box::check_for_followups_bfs::is_function_type::symbol_name({})::fs_file_path({})", outline_node.name(), outline_node.fs_file_path());
                    reference_locations.extend(
                        self.check_for_followups_on_functions(
                            outline_node,
                            symbol_edited,
                            &symbol_followup,
                            hub_sender.clone(),
                            message_properties.clone(),
                            tool_properties.clone(),
                        )
                        .await.unwrap_or_default(),
                    );
                } else if outline_node.is_class_definition() {
                    println!(
                        "tool_box::check_for_followups_bfs::class_definition::symbol_name({})::fs_file_path({})",
                        outline_node.name(),
                        outline_node.fs_file_path(),
                    );
                    reference_locations.extend(
                        self.check_for_followups_class_definitions(
                            outline_node,
                            symbol_edited,
                            &symbol_followup,
                            hub_sender.clone(),
                            message_properties.clone(),
                            tool_properties.clone(),
                        )
                        .await.unwrap_or_default()
                    );
                } else {
                    println!(
                        "tool_box::check_for_followups_bfs:class_implementation::symbol_name({})::fs_file_path({})",
                        outline_node.name(),
                        outline_node.fs_file_path(),
                    );
                    reference_locations.extend(
                        self.check_for_followups_class_implementation(
                            outline_node,
                            &symbol_followup,
                            original_code,
                            edited_code,
                            hub_sender.clone(),
                            message_properties.clone(),
                            tool_properties,
                        )
                        .await.unwrap_or_default()
                    );
                }
                reference_locations
            }).buffer_unordered(100).collect::<Vec<_>>().await.into_iter().flatten().collect::<Vec<_>>();
            // create a dedup here for the references which we are sending
            // making sure that we are not repeating the followup request with the same data
            let mut already_seen_followup: HashSet<String> = Default::default();
            symbol_followups = vec![];
            for reference_location in reference_locations.into_iter() {
                let symbol_identifier = reference_location.symbol_identifier();
                let symbol_to_edit = reference_location.symbol_edited().symbol_name();
                let hash_key = format!(
                    "{}::{:?}::{}",
                    symbol_identifier.symbol_name(),
                    symbol_identifier.fs_file_path(),
                    symbol_to_edit
                );
                if already_seen_followup.contains(&hash_key) {
                    continue;
                }
                already_seen_followup.insert(hash_key);
                // helps us dedup the reference locations
                symbol_followups.push(reference_location);
            }
        }
        println!(
            "tool_box::check_for_followups_bfs::complete::time_taken({:?}ms)",
            instant.elapsed().as_millis()
        );
        Ok(())
    }

    pub async fn check_for_followups(
        &self,
        parent_symbol_name: &str,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        edited_code: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        // followups here are made for checking the references or different symbols
        // or if something has changed
        // first do we show the agent the chagned data and then ask it to decide
        // where to go next or should we do something else?
        // another idea here would be to use the definitions or the references
        // of various symbols to find them and then navigate to them
        let language = self
            .editor_parsing
            .for_file_path(symbol_edited.fs_file_path())
            .map(|language_config| language_config.language_str.to_owned())
            .unwrap_or("".to_owned());
        println!(
            "tool_box::check_for_followups::find_sub_symbol_edited::({})::({})::range({:?})",
            parent_symbol_name,
            symbol_edited.symbol_name(),
            symbol_edited.range(),
        );
        let outline_node = self
            .find_sub_symbol_to_edit_with_name(
                parent_symbol_name,
                symbol_edited,
                message_properties.clone(),
            )
            .await?;

        // the line number relative to which we are computing our positions
        let edited_code_start_line = outline_node.range().start_line();

        println!(
            "tool_box::check_for_followups::found_sub_symbol_edited::parent_symbol_name({})::symbol_edited({})::outline_node_type({:?})",
            parent_symbol_name,
            symbol_edited.symbol_name(),
            outline_node.outline_node_type(),
        );
        // over here we have to check if its a function or a class
        if outline_node.is_function_type() {
            println!(
                "tool_box::check_for_followups::is_function_type::parent_symbol_name({})::symbol_to_edit({})",
                parent_symbol_name,
                outline_node.name(),
            );

            // this should no longer be needed - use metadata!

            // we do need to get the references over here for the function and
            // send them over as followups to check wherever they are being used
            let references = self
                .go_to_references(
                    symbol_edited.fs_file_path().to_owned(),
                    outline_node.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?;

            let references = references.prioritize_and_deduplicate(symbol_edited.fs_file_path());

            let _ = self
                .invoke_followup_on_references(
                    original_code,
                    &outline_node,
                    references.locations(),
                    hub_sender,
                    message_properties.clone(),
                    tool_properties,
                )
                .await;
        } else if outline_node.is_class_definition() {
            println!(
                "tool_box::check_for_followups::is_class_definition::parent_symbol_name({})::symbol_to_edit({})",
                parent_symbol_name,
                &outline_node.name()
            );
            // this flow only happens for rust/golang type of languages
            // so the new flow which we should take is the following:
            // - go to our implementation blocks and check if there is something which needs
            // changing over here
            // - once we have changed the implementation blocks trigger the followups for the functions
            // which changed as well as the class definition node itself
            let _ = self
                .invoke_references_check_for_class_definition(
                    symbol_edited,
                    original_code,
                    &outline_node,
                    language,
                    llm,
                    provider,
                    api_keys,
                    hub_sender.clone(),
                    message_properties.clone(),
                    tool_properties,
                )
                .await;

            // this returns positions with byte_offset 0, which fks .contains
            let references = self
                .go_to_references(
                    symbol_edited.fs_file_path().to_owned(),
                    outline_node.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?;

            let references = references.prioritize_and_deduplicate(symbol_edited.fs_file_path());

            println!(
                "check_for_followups::go_to_references::({})",
                references
                    .clone()
                    .locations()
                    .iter()
                    .map(|loc| loc.fs_file_path())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            let _ = self
                .invoke_followup_on_references(
                    original_code,
                    &outline_node,
                    references.locations(),
                    hub_sender,
                    message_properties,
                    tool_properties,
                )
                .await;
        } else {
            // we are always editing the symbol in full and are interested in the
            // fields which are present in the code
            // we want to go over the child nodes which have changed and then invoke
            // followups on that
            // we can compare each of the child nodes and figure out what changes were made
            // to each of the child
            let language_config = self
                .editor_parsing
                .for_file_path(symbol_edited.fs_file_path());
            if language_config.is_none() {
                return Ok(());
            }
            let language_config = language_config.expect("to be present");

            // the ranges here are all messed up since we are computing relative to
            // Position::new(0, 0, 0) ...ugh
            let older_outline_nodes = language_config
                .generate_outline_fresh(original_code.as_bytes(), symbol_edited.fs_file_path())
                .into_iter()
                .find(|outline_node| outline_node.name() == parent_symbol_name);
            let newer_outline_nodes = language_config
                .generate_outline_fresh(edited_code.as_bytes(), symbol_edited.fs_file_path())
                .into_iter()
                .find(|outline_node| outline_node.name() == parent_symbol_name);

            // our outline nodes are matching up over here
            if let (Some(new_outline_node), Some(old_outline_node)) =
                (newer_outline_nodes, older_outline_nodes)
            {
                // now find the child nodes which are also present on the old outline nodes
                let changed_function_nodes = new_outline_node
                    .children()
                    .into_iter()
                    .filter_map(|new_child_outline_node| {
                        let old_child_outline_node =
                            old_outline_node.children().iter().find(|old_child_node| {
                                old_child_node.name() == new_child_outline_node.name()
                            });
                        if let Some(old_child_outline_node) = old_child_outline_node {
                            if old_child_outline_node.content() != new_child_outline_node.content()
                            {
                                Some(new_child_outline_node)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                let function_names = changed_function_nodes
                    .iter()
                    .map(|function_node| function_node.name())
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "tool_box::check_for_followups::symbol_name({})::functions_changed::({})",
                    parent_symbol_name, function_names
                );

                // go over the children now and go to the references for each of them which have changed, these will
                // be our reference locations
                let fs_file_path = symbol_edited.fs_file_path();
                let mut reference_locations = vec![];
                for outline_node in changed_function_nodes.into_iter() {
                    let references = self
                        .go_to_references(
                            fs_file_path.to_owned(),
                            // we have to fix the range here relative to the
                            // position in the current document
                            outline_node
                                .identifier_range()
                                .start_position()
                                .move_lines(edited_code_start_line),
                            message_properties.clone(),
                        )
                        .await;

                    if let Ok(references) = references {
                        let locations = references.clone().locations();
                        println!(
                            "tool_box::check_for_followups::symbol_name({})::fs_file_path({})::range({:?})::references::({})",
                            outline_node.name(),
                            outline_node.fs_file_path(),
                            outline_node.range(),
                            locations
                                .as_slice()
                                .iter()
                                .map(|location| location.fs_file_path())
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        let references =
                            references.prioritize_and_deduplicate(symbol_edited.fs_file_path());
                        reference_locations.extend(references.locations());
                    } else {
                        println!(
                            "tool_box::refenreces_error::({:?})",
                            references.expect("if let Ok to hold")
                        );
                    }
                }

                let _ = self
                    .invoke_followup_on_references(
                        original_code,
                        &outline_node,
                        reference_locations,
                        hub_sender,
                        message_properties,
                        tool_properties,
                    )
                    .await;
            }
        }
        Ok(())
    }

    async fn invoke_references_check_for_class_definition(
        &self,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        edited_symbol: &OutlineNodeContent,
        language: String,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        println!("toolbox::invoke_references_check_for_class_definition");
        // we need to first ask the LLM for the class properties if any we have
        // to followup on if they changed
        let request = ClassSymbolFollowupRequest::new(
            symbol_edited.fs_file_path().to_owned(),
            original_code.to_owned(),
            language,
            edited_symbol.content().to_owned(),
            symbol_edited.instructions().join("\n"),
            llm,
            provider,
            api_key,
            message_properties.root_request_id().to_owned(),
        );
        let fs_file_path = edited_symbol.fs_file_path().to_owned();
        let start_line = edited_symbol.range().start_line();
        let content_lines = edited_symbol
            .content()
            .lines()
            .enumerate()
            .into_iter()
            .map(|(index, line)| (index + start_line, line.to_owned()))
            .collect::<Vec<_>>();
        let class_members_to_follow = self.check_class_members_to_follow(request).await?.members();
        // now we need to get the members and schedule a followup along with the refenreces where
        // we might ber using this class
        // Now we have to get the position of the members which we want to follow-along, this is important
        // since we might have multiple members here and have to make sure that we can go-to-refernces for this
        let members_with_position = class_members_to_follow
            .into_iter()
            .filter_map(|member| {
                // find the position in the content where we have this member and keep track of that
                let inner_symbol = member.line();
                let found_line = content_lines
                    .iter()
                    .find(|(_, line)| line.contains(inner_symbol));
                if let Some((line_number, found_line)) = found_line {
                    let column_index = found_line.find(member.name());
                    if let Some(column_index) = column_index {
                        Some((member, Position::new(*line_number, column_index, 0)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        stream::iter(members_with_position.into_iter().map(|(member, position)| {
            (
                member,
                position,
                fs_file_path.to_owned(),
                hub_sender.clone(),
                message_properties.clone(),
            )
        }))
        .map(
            |(member, position, fs_file_path, hub_sender, message_properties)| async move {
                let _ = self
                    .check_followup_for_member(
                        member,
                        position,
                        &fs_file_path,
                        original_code,
                        symbol_edited,
                        edited_symbol,
                        hub_sender,
                        message_properties,
                        tool_properties,
                    )
                    .await;
            },
        )
        // run all these futures in parallel
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;
        // now we have the members and their positions along with the class defintion which we want to check anyways
        // we initial go-to-refences on all of these and try to see what we are getting

        // we also want to do a reference check for the class identifier itself, since this is also important and we
        // want to make sure that we are checking all the places where its being used
        Ok(())
    }

    async fn check_followup_for_member(
        &self,
        member: ClassSymbolMember,
        position: Position,
        // This is the file path where we want to check for the references
        fs_file_path: &str,
        original_code: &str,
        symbol_edited: &SymbolToEdit,
        edited_symbol: &OutlineNodeContent,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        let references = self
            .go_to_references(
                fs_file_path.to_owned(),
                position.clone(),
                message_properties.clone(),
            )
            .await?;
        let reference_locations = references.locations();
        let file_paths = reference_locations
            .iter()
            .map(|reference| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();
        // we invoke a request to open the file
        let _ = stream::iter(
            file_paths
                .clone()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async {
            // get the file content
            let _ = self.file_open(fs_file_path, message_properties).await;
            ()
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;

        // next we ask the symbol manager for all the symbols in the file and try
        // to locate our symbol inside one of the symbols?
        // once we have the outline node, we can try to understand which symbol
        // the position is part of and use that for creating the containing scope
        // of the symbol
        let mut file_path_to_outline_nodes = stream::iter(file_paths.clone())
            .map(|fs_file_path| async {
                self.get_outline_nodes_grouped(&fs_file_path)
                    .await
                    .map(|outline_nodes| (fs_file_path, outline_nodes))
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<HashMap<String, Vec<OutlineNode>>>();

        // now we have to group the files along with the positions/ranges of the references
        let mut file_paths_to_locations: HashMap<String, Vec<Range>> = Default::default();
        reference_locations.iter().for_each(|reference| {
            let file_path = reference.fs_file_path();
            let range = reference.range().clone();
            if let Some(file_pointer) = file_paths_to_locations.get_mut(file_path) {
                file_pointer.push(range);
            } else {
                file_paths_to_locations.insert(file_path.to_owned(), vec![range]);
            }
        });

        let edited_code = edited_symbol.content();
        stream::iter(
            file_paths_to_locations
                .into_iter()
                .filter_map(|(file_path, ranges)| {
                    if let Some(outline_nodes) = file_path_to_outline_nodes.remove(&file_path) {
                        Some((
                            file_path,
                            ranges,
                            hub_sender.clone(),
                            outline_nodes,
                            member.clone(),
                            message_properties.clone(),
                        ))
                    } else {
                        None
                    }
                })
                .map(
                    |(_, ranges, hub_sender, outline_nodes, member, message_properties)| {
                        ranges
                            .into_iter()
                            .map(|range| {
                                (
                                    range,
                                    hub_sender.clone(),
                                    outline_nodes.to_vec(),
                                    member.clone(),
                                    message_properties.clone(),
                                )
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .flatten(),
        )
        .map(
            |(range, hub_sender, outline_nodes, member, message_properties)| async move {
                self.send_request_for_followup_class_member(
                    original_code,
                    edited_code,
                    symbol_edited,
                    member,
                    range.start_position(),
                    outline_nodes,
                    hub_sender,
                    message_properties,
                    tool_properties,
                )
                .await
            },
        )
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;
        Ok(())
    }

    async fn send_request_for_followup_class_member(
        &self,
        original_code: &str,
        edited_code: &str,
        symbol_edited: &SymbolToEdit,
        member: ClassSymbolMember,
        position_to_search: Position,
        outline_nodes: Vec<OutlineNode>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        let outline_node_possible = outline_nodes.into_iter().find(|outline_node| {
            // we need to check if the outline node contains the range we are interested in
            outline_node.range().contains(&Range::new(
                position_to_search.clone(),
                position_to_search.clone(),
            ))
        });
        match outline_node_possible {
            Some(outline_node) => {
                // we try to find the smallest node over here which contains the position
                let child_node_possible =
                    outline_node
                        .children()
                        .into_iter()
                        .find(|outline_node_content| {
                            outline_node_content.range().contains(&Range::new(
                                position_to_search.clone(),
                                position_to_search.clone(),
                            ))
                        });

                let outline_node_fs_file_path = outline_node.content().fs_file_path();
                let outline_node_identifier_range = outline_node.content().identifier_range();
                // we can go to definition of the node and then ask the symbol for the outline over
                // here so the symbol knows about everything
                let definitions = self
                    .go_to_definition(
                        outline_node_fs_file_path,
                        outline_node_identifier_range.start_position(),
                        message_properties.clone(),
                    )
                    .await?;
                if let Some(definition) = definitions.definitions().get(0) {
                    let _ = definition.file_path();
                    let _ = outline_node.name();
                    if let Some(child_node) = child_node_possible {
                        // we need to get a few lines above and below the place where the defintion is present
                        // so we can show that to the LLM properly and ask it to make changes
                        let start_line = child_node.range().start_line();
                        let content_with_line_numbers = child_node
                            .content()
                            .lines()
                            .enumerate()
                            .map(|(index, line)| (index + start_line, line.to_owned()))
                            .collect::<Vec<_>>();
                        // Now we collect 4 lines above and below the position we are interested in
                        let position_line_number = position_to_search.line() as i64;
                        let symbol_content_to_send = content_with_line_numbers
                            .into_iter()
                            .filter_map(|(line_number, line_content)| {
                                if line_number as i64 <= position_line_number + 4
                                    && line_number as i64 >= position_line_number - 4
                                {
                                    if line_number as i64 == position_line_number {
                                        // if this is the line number we are interested in then we have to highlight
                                        // this for the LLM
                                        Some(format!(
                                            r#"<line_with_reference>
{line_content}
</line_with_reference>"#
                                        ))
                                    } else {
                                        Some(line_content)
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let instruction_prompt = self
                            .create_instruction_prompt_for_followup_class_member_change(
                                original_code,
                                edited_code,
                                &child_node,
                                &format!(
                                    "{}-{}:{}",
                                    child_node.fs_file_path(),
                                    child_node.range().start_line(),
                                    child_node.range().end_line()
                                ),
                                symbol_content_to_send,
                                member,
                                &symbol_edited,
                            );
                        // now we can send it over to the hub sender for handling the change
                        let (sender, receiver) = tokio::sync::oneshot::channel();

                        let symbol_identifier = SymbolIdentifier::with_file_path(
                            outline_node.name(),
                            outline_node.fs_file_path(),
                        );

                        let symbol_to_edit_with_updated_instruction = symbol_edited
                            .clone_with_instructions(&vec![instruction_prompt.clone()]);

                        // go hardcore, straight up edit, no faffing around
                        let symbol_event_request_for_edit = SymbolEventRequest::simple_edit_request(
                            symbol_identifier.to_owned(),
                            symbol_to_edit_with_updated_instruction,
                            tool_properties.to_owned(),
                        );

                        let event = SymbolEventMessage::new(
                            symbol_event_request_for_edit,
                            message_properties
                                .request_id()
                                .clone()
                                .set_request_id(uuid::Uuid::new_v4().to_string()),
                            message_properties.ui_sender(),
                            sender,
                            message_properties.cancellation_token(),
                            message_properties.editor_url(),
                            message_properties.llm_properties().clone(),
                        );
                        let _ = hub_sender.send(event);
                        // Figure out what to do with the receiver over here
                        let _ = receiver.await;
                        // this also feels a bit iffy to me, since this will block
                        // the other requests from happening unless we do everything in parallel
                        Ok(())
                    } else {
                        // honestly this might be the case that the position where we got the reference is in some global zone
                        // which is hard to handle right now, we can just return and error and keep going
                        return Err(SymbolError::SymbolNotContainedInChild);
                    }
                    // This is now perfect since we have the symbol outline which we
                    // want to send over as context
                    // along with other metadata to create the followup-request required
                    // for making the edits as required
                } else {
                    // if there are no defintions, this is bad since we do require some kind
                    // of definition to be present here
                    return Err(SymbolError::DefinitionNotFound(
                        outline_node.name().to_owned(),
                    ));
                }
            }
            None => {
                // if there is no such outline node, then what should we do? cause we still
                // need an outline of sorts
                return Err(SymbolError::NoOutlineNodeSatisfyPosition);
            }
        }
    }

    async fn check_class_members_to_follow(
        &self,
        request: ClassSymbolFollowupRequest,
    ) -> Result<ClassSymbolFollowupResponse, SymbolError> {
        let tool_input = ToolInput::ClassSymbolFollowup(request);
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .class_symbols_to_followup()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Invokes followups on the references based on certain assumptions:
    /// prioritizes and de-duplicates which can lead to self-loops this is important
    /// as it can happen anytime
    ///
    /// How do we detect this:
    /// - we need to look at which node level we are on and transition/filter out
    /// based on that
    /// - if we are on a function, then its trivial to just keep going over all the references (no
    /// prioritiztion required)
    /// - if we are on a class definition, then we for sure need to go to the implementations
    /// if the references point to that
    /// - if we are on a class implementation block, then we do not need to go further into the
    /// implementation block since that will be already covered by our current run for reference checks
    async fn invoke_followup_on_references(
        &self,
        original_code: &str,
        // this will be a class, function or the class implementation block
        original_symbol: &OutlineNodeContent,
        // references here might be from everywhere: functions in the class, implementation block
        // or even the function
        reference_locations: Vec<ReferenceLocation>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        let file_paths = reference_locations
            .iter()
            .map(|reference| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        println!(
            "invoke_followup_on_references::file_paths::({})",
            file_paths
                .iter()
                .map(|fs_file_path| fs_file_path.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        // we invoke a request to open the file
        let _ = stream::iter(
            file_paths
                .clone()
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            // get the file content
            let file_contents = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_contents) = file_contents {
                let _ = self
                    .force_add_document(
                        // this is critical to avoiding race conditions
                        &fs_file_path,
                        file_contents.contents_ref(),
                        file_contents.language(),
                    )
                    .await;
            }
            ()
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;

        // next we ask the symbol manager for all the symbols in the file and try
        // to locate our symbol inside one of the symbols?
        // once we have the outline node, we can try to understand which symbol
        // the position is part of and use that for creating the containing scope
        // of the symbol
        let file_path_to_outline_nodes = stream::iter(file_paths.clone())
            .map(|fs_file_path| async {
                self.get_outline_nodes_grouped(&fs_file_path)
                    .await
                    .map(|outline_nodes| (fs_file_path, outline_nodes))
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<HashMap<String, Vec<OutlineNode>>>();

        let mut sorted_file_path_to_outline_nodes: HashMap<String, Vec<OutlineNode>> =
            file_path_to_outline_nodes
                .into_iter()
                .map(|(fs_file_path, mut outline_nodes)| {
                    let original_symbol_name = original_symbol.name();

                    outline_nodes.sort_by(|a, b| {
                        let a_match = a.name() == original_symbol_name;
                        let b_match = b.name() == original_symbol_name;
                        b_match.cmp(&a_match)
                    });

                    println!(
                        "toolbox::invoke_followup_on_references::sorted_file_path_to_outline_nodes({})::outline_nodes({})", fs_file_path,
                        outline_nodes
                            .iter()
                            .map(|n| n.name())
                            .collect::<Vec<&str>>()
                            .join(",")
                    );

                    (fs_file_path, outline_nodes)
                })
                .collect::<HashMap<String, Vec<OutlineNode>>>();

        // now we have to group the files along with the positions/ranges of the references
        let mut file_paths_to_locations: HashMap<String, Vec<Range>> = Default::default();
        reference_locations.iter().for_each(|reference| {
            let file_path = reference.fs_file_path();
            let range = reference.range().clone();
            if let Some(file_pointer) = file_paths_to_locations.get_mut(file_path) {
                file_pointer.push(range);
            } else {
                file_paths_to_locations.insert(file_path.to_owned(), vec![range]);
            }
        });

        let edited_code = original_symbol.content();

        // now we have to deuplicate the outline nodes which we want to change based on the ranges
        let outline_nodes_for_followups = file_paths_to_locations
            .into_iter()
            .filter_map(|(file_path, ranges)| {
                if let Some(outline_nodes) = sorted_file_path_to_outline_nodes.remove(&file_path) {
                    // figure out what to put over here
                    let outline_nodes_containing_references = outline_nodes
                        .into_iter()
                        .filter(|outline_node| {
                            ranges
                                .iter()
                                .any(|range| outline_node.content().range().contains(&range))
                        })
                        .collect::<Vec<_>>();
                    Some(outline_nodes_containing_references)
                } else {
                    None
                }
            })
            .flatten()
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|outline_node_for_reference| {
                // here we can follow a simple paradigm and use it
                if original_symbol.is_function_type() {
                    true
                } else if original_symbol.is_class_definition() {
                    // we can go to an implementation block from here but not back to
                    // the class itself (only if the classes line up)
                    if outline_node_for_reference.is_class_definition()
                        && outline_node_for_reference.name() == original_symbol.name()
                    {
                        false
                    } else {
                        true
                    }
                } else {
                    // this is a class implementation block, can we go back to another class implementation block
                    // yes, but only if its not the same as the outline node itself
                    // this does not feel right to me tho....
                    if outline_node_for_reference.content().content() == edited_code {
                        false
                    } else {
                        true
                    }
                }
            })
            .map(|outline_node| (outline_node, hub_sender.clone(), message_properties.clone()))
            .collect::<Vec<_>>();

        println!(
            "tool_box::invoke_followup_on_references::outline_nodes_for_followups::outline_nodes_for_followups({})",
            outline_nodes_for_followups
                .iter()
                .map(|(node, _, _)| format!("{} - {:?}", node.name(), node.identifier_range()))
                .collect::<Vec<String>>()
                .join(",")
        );

        stream::iter(outline_nodes_for_followups)
            .map(
                |(outline_node, hub_sender, message_properties)| async move {
                    self.send_request_for_followup(
                        original_code,
                        edited_code,
                        outline_node,
                        hub_sender,
                        message_properties,
                        tool_properties,
                    )
                    .await
                },
            )
            .buffer_unordered(1)
            .collect::<Vec<_>>()
            .await;
        // not entirely convinced that this is the best way to do this, but I think
        // it makes sense to do it this way
        Ok(())
    }

    fn create_instruction_prompt_for_followup_class_member_change(
        &self,
        original_code: &str,
        edited_code: &str,
        child_symbol: &OutlineNodeContent,
        file_path_for_followup: &str,
        symbol_content_with_highlight: String,
        class_memeber_change: ClassSymbolMember,
        symbol_to_edit: &SymbolToEdit,
    ) -> String {
        let member_name = class_memeber_change.name();
        let symbol_fs_file_path = symbol_to_edit.fs_file_path();
        let instructions = symbol_to_edit.instructions().join("\n");
        let child_symbol_name = child_symbol.name();
        let original_symbol_name = symbol_to_edit.symbol_name();
        let thinking = class_memeber_change.thinking();
        format!(
            r#"Another engineer has changed the member `{member_name}` in `{original_symbol_name} which is present in `{symbol_fs_file_path}
The original code for `{original_symbol_name}` is given in the <old_code> section below along with the new code which is present in <new_code> and the instructions for why the change was done in <instructions_for_change> section:
<old_code>
{original_code}
</old_code>

<new_code>
{edited_code}
</new_code>

<instructions_for_change>
{instructions}
</instructions_for_change>

The `{member_name}` is being used in `{child_symbol_name}` in the following line:
<file_path>
{file_path_for_followup}
</file_path>
<content>
{symbol_content_with_highlight}
</content>

The member for `{original_symbol_name}` which was changed is `{member_name}` and the reason we think it needs a followup change in `{child_symbol_name}` is given below:
{thinking}

Make the necessary changes if required making sure that nothing breaks"#
        )
    }

    async fn send_edit_instruction_to_outline_node(
        &self,
        outline_node: OutlineNode,
        instruction: String,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: ToolProperties,
    ) -> Result<(), SymbolError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();

        // the symbol representing the reference
        let symbol_identifier =
            SymbolIdentifier::with_file_path(outline_node.name(), outline_node.fs_file_path());

        let symbol_to_edit = SymbolToEdit::new(
            outline_node.name().to_string(),
            outline_node.range().to_owned(),
            outline_node.fs_file_path().to_string(),
            vec![instruction],
            false,
            false, // is_new could be true...
            true,
            "".to_string(),
            None,
            false,
            None,
            true, // disable any kind of followups or correctness check
            None,
            vec![],
            None,
        );

        let event = SymbolEventMessage::message_with_properties(
            SymbolEventRequest::simple_edit_request(
                symbol_identifier,
                symbol_to_edit.to_owned(),
                tool_properties,
            ),
            message_properties,
            sender,
        );
        let start = Instant::now();
        let _ = hub_sender.send(event);
        let _ = receiver.await;
        println!(
            "tool_box::send_edit_instruction_to_outline_node::SymbolEventRequest::time:({:?})",
            start.elapsed()
        );
        Ok(())
    }

    // we need to search for the smallest node which contains this position or range
    async fn send_request_for_followup(
        &self,
        original_code: &str,
        edited_code: &str,
        // This is pretty expensive to copy again and again
        outline_node: OutlineNode,
        // this is becoming annoying now cause we will need a drain for this while
        // writing a unit-test for this
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        println!("=====================");
        println!(
            "sending request for follow up. Symbol to edit: {}",
            outline_node.name()
        );
        println!("=====================");
        // we try to find the smallest node over here which contains the position

        let outline_node_fs_file_path = outline_node.content().fs_file_path();
        let outline_node_identifier_range = outline_node.content().identifier_range();
        // we can go to definition of the node and then ask the symbol for the outline over
        // here so the symbol knows about everything

        let start = Instant::now();
        let definitions = self
            .go_to_definition(
                outline_node_fs_file_path,
                outline_node_identifier_range.start_position(),
                message_properties.clone(),
            )
            .await?;

        println!(
            "tool_box::send_request_for_followup::go_to_definition::time: {:?}",
            start.elapsed()
        );

        if let Some(_definition) = definitions.definitions().get(0) {
            // we need to get a few lines above and below the place where the defintion is present
            // so we can show that to the LLM properly and ask it to make changes
            // now we can send it over to the hub sender for handling the change
            let (sender, receiver) = tokio::sync::oneshot::channel();

            println!("original code: \n{}", original_code);
            println!("=========");
            println!("edited code: \n{}", edited_code);

            let prompt = format!(
                "A dependency of this code has changed.\n\
                             Dependent class/method: {}\n\
                             Original implementation:\n```\n{}\n```\n\
                             Updated implementation:\n```\n{}\n```\n\n\
                             Please update this code to accommodate these changes. Consider:\n\
                             1. Method signature changes (parameters, return types)\n\
                             2. Behavioural changes in the dependency\n\
                             3. Potential side effects or new exceptions\n\
                             4. Deprecated features that should no longer be used\n\
                             5. If no changes are required, do not make any changes to the code! I do not want to review code if no changes are required.\n\
                             Explain your changes and any assumptions you make.",
                outline_node.name(),
                original_code,
                edited_code
            );

            // the symbol representing the reference
            let symbol_identifier =
                SymbolIdentifier::with_file_path(outline_node.name(), outline_node.fs_file_path());

            let symbol_to_edit = SymbolToEdit::new(
                outline_node.name().to_string(),
                outline_node.range().to_owned(),
                outline_node.fs_file_path().to_string(),
                vec![prompt],
                false,
                false, // is_new could be true...
                true,
                "".to_string(),
                None,
                false,
                None,
                true,
                None,
                vec![],
                None,
            );

            let event = SymbolEventMessage::message_with_properties(
                SymbolEventRequest::simple_edit_request(
                    symbol_identifier,
                    symbol_to_edit.to_owned(),
                    tool_properties.to_owned(),
                ),
                message_properties.clone(),
                sender,
            );
            let start = Instant::now();
            let _ = hub_sender.send(event);
            // Figure out what to do with the receiver over here
            let _ = receiver.await;
            println!(
                "tool_box::send_request_for_followup::SymbolEventRequest::time: {:?}",
                start.elapsed()
            );
            // this also feels a bit iffy to me, since this will block
            // the other requests from happening unless we do everything in parallel
            Ok(())
            // This is now perfect since we have the symbol outline which we
            // want to send over as context
            // along with other metadata to create the followup-request required
            // for making the edits as required
        } else {
            // if there are no defintions, this is bad since we do require some kind
            // of definition to be present here
            return Err(SymbolError::DefinitionNotFound(
                outline_node.name().to_owned(),
            ));
        }
    }

    pub async fn go_to_references(
        &self,
        fs_file_path: String,
        position: Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToReferencesResponse, SymbolError> {
        let input = ToolInput::GoToReference(GoToReferencesRequest::new(
            fs_file_path.to_owned(),
            position.clone(),
            message_properties.editor_url().to_owned(),
        ));

        println!(
            "too_box::go_to_references::fs_file_path({:?})::position({:?})",
            &fs_file_path, &position
        );

        let reference_locations = self
            .tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_references()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(reference_locations.filter_out_same_position_location(&fs_file_path, &position))
    }

    async fn _swe_bench_test_tool(
        &self,
        swe_bench_test_endpoint: &str,
    ) -> Result<SWEBenchTestRepsonse, SymbolError> {
        let tool_input =
            ToolInput::SWEBenchTest(SWEBenchTestRequest::new(swe_bench_test_endpoint.to_owned()));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_swe_bench_test_output()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// We can make edits to a different part of the codebase when
    /// doing code correction, returns if any edits were done to the codebase
    /// outside of the selected range (true or false accordingly)
    /// NOTE: Not running in full parallelism yet, we will enable that
    /// and missing creating new files etc
    pub async fn code_correctness_changes_to_codebase(
        &self,
        parent_symbol_name: &str,
        fs_file_path: &str,
        _edited_range: &Range,
        _edited_code: &str,
        thinking: &str,
        tool_properties: &ToolProperties,
        llm_properties: LLMProperties,
        history: Vec<SymbolRequestHistoryItem>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<bool, SymbolError> {
        // over here we want to ping the other symbols and send them requests, there is a search
        // step with some thinking involved, can we illicit this behavior somehow in the previous invocation
        // or maybe we should keep it separate
        // TODO(skcd): Figure this part out
        // 1. First we figure out if the code symbol exists in the codebase
        // 2. If it does exist then we know the action we want to  invoke on it
        // 3. If the symbol does not exist, then we need to go through the creation loop
        // where should that happen?
        let symbols_to_edit = self
            .find_symbols_to_edit_from_context(
                thinking,
                llm_properties.clone(),
                message_properties.clone(),
            )
            .await?;

        let symbols_to_edit_list = symbols_to_edit.symbol_list();

        if symbols_to_edit_list.is_empty() {
            return Ok(false);
        }

        // TODO(skcd+zi): Can we run this in full parallelism??
        // answer: yes we can, but lets get it to crawl before it runs
        for (symbol_to_edit, message_properties) in symbols_to_edit_list
            .into_iter()
            .map(|symbol| (symbol, message_properties.clone()))
        {
            let symbol_to_find = symbol_to_edit.to_owned();
            let symbol_locations = self
                .grep_symbols_in_ide(&symbol_to_find, message_properties.clone())
                .await?;
            let found_symbol = symbol_locations
                .locations()
                .into_iter()
                .find(|location| location.name() == symbol_to_edit);
            let request = format!(
                r#"The instruction contains information about multiple symbols, but we are going to focus only on {symbol_to_edit}
instruction:
{thinking}"#
            );
            if let Some(symbol_to_edit) = found_symbol {
                let symbol_file_path = symbol_to_edit.fs_file_path();
                let symbol_name = symbol_to_edit.name();
                // if the reference is to some part of the symbol itself
                // that's already covered in the plans, so we can choose to
                // not make an initial request at this point
                // this check is very very weak, we need to do better over here
                // TODO(skcd): This check does not take into consideration the changes
                // we have made across the codebase, we need to have a better way to
                // handle this
                if symbol_name == parent_symbol_name {
                    println!("tool_box::code_correctness_changes_to_codebase::skipping::self_symbol::({})", &parent_symbol_name);
                    continue;
                } else {
                    println!("tool_box::code_correctness_chagnes_to_codebase::following_along::self_symbol({})::symbol_to_edit({})", &parent_symbol_name, symbol_name);
                }

                // skip sending the request here if we have already done the work
                // in history (ideally we use an LLM but right now we just guard
                // easily)
                if history.iter().any(|history| {
                    history.symbol_name() == symbol_name
                        && history.fs_file_path() == symbol_file_path
                }) {
                    println!("tool_box::code_correctness_changes_to_codebase::skipping::symbol_in_history::({})", symbol_name);
                    continue;
                }
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let event = SymbolEventMessage::message_with_properties(
                    SymbolEventRequest::initial_request(
                        SymbolIdentifier::with_file_path(symbol_name, symbol_file_path),
                        thinking.to_owned(),
                        request.to_owned(),
                        history.to_vec(),
                        tool_properties.clone(),
                        None,
                        false,
                    ),
                    message_properties,
                    sender,
                );
                let _ = hub_sender.send(event);
                // we should pass back this response to the caller for sure??
                let _ = receiver.await;
            } else {
                // no matching symbols, F in the chat for the LLM
                // TODO(skcd): Figure out how to handle this properly, for now we can just skip on this
                // since the code correction call will be blocked anyways on this
                // hack we are taking: we are putting the symbol at the end of the file
                // it breaks: code organisation and semantic location (we can fix it??)
                // we are going for end to end loop
                println!("tool_box::code_correctness_changes_to_codebase::new_symbol");
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let event = SymbolEventMessage::message_with_properties(
                    SymbolEventRequest::initial_request(
                        SymbolIdentifier::with_file_path(symbol_to_edit, fs_file_path),
                        thinking.to_owned(),
                        request.to_owned(),
                        history.to_vec(),
                        tool_properties.clone(),
                        None,
                        false,
                    ),
                    message_properties,
                    sender,
                );
                let _ = hub_sender.send(event);
                let _ = receiver.await;
            }
        }
        Ok(true)
    }

    /// Generate the repo map for the tools
    pub async fn load_repo_map(
        &self,
        repo_map_path: &String,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<String> {
        let tag_index = TagIndex::from_path(Path::new(repo_map_path)).await;

        // TODO(skcd): Should have proper construct time sharing (we only create it once) over here
        let request_id = message_properties.request_id().request_id();
        println!("tool_box::load_repo_map::start({})", &request_id);
        let repo_map = RepoMap::new().with_map_tokens(10_000);

        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::repo_map_gen_start(request_id.to_owned()));
        let result = repo_map.get_repo_map(&tag_index).await.ok();

        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::repo_map_gen_end(request_id.to_owned()));
        println!("tool_box::load_repo_map::end({})", &request_id);
        result
    }

    pub async fn check_code_correctness(
        &self,
        parent_symbol_name: &str,
        symbol_edited: &SymbolToEdit,
        symbol_identifier: SymbolIdentifier,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        tool_properties: &ToolProperties,
        _history: Vec<SymbolRequestHistoryItem>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let instructions = symbol_edited.instructions().join("\n");
        let fs_file_path = symbol_edited.fs_file_path();
        let extra_symbol_list = tool_properties.get_plan_for_input();
        let extra_symbol_list_ref = extra_symbol_list.as_deref().map(String::from);
        let symbol_name = symbol_edited.symbol_name();

        let lsp_request_id = uuid::Uuid::new_v4().to_string(); // todo(zi): request_id is what quick fix list is stored against.
                                                               // we then parallelise calls using the same lsp_request_id?
                                                               // this is dangerous. todo(zi)!!

        let edited_symbol_outline_node_content = self
            .find_sub_symbol_to_edit_with_name(
                parent_symbol_name,
                symbol_edited,
                message_properties.clone(),
            )
            .await?;

        // After applying the changes we get the new range for the symbol
        let edited_range = edited_symbol_outline_node_content.range().to_owned();
        // In case we have swe-bench-tooling enabled over here we should run
        // the tests first, since we get enough value out if to begin with
        // TODO(skcd): Use the test output for debugging over here
        // let test_output_maybe = if let Some(swe_bench_test_endpoint) =
        //     tool_properties.get_swe_bench_test_endpoint()
        // {
        //     let swe_bench_test_output = self.swe_bench_test_tool(&swe_bench_test_endpoint).await?;
        //     // Pass the test output through for checking the correctness of
        //     // this code
        //     Some(swe_bench_test_output)
        // } else {
        //     None
        // };

        // // TODO(skcd): Figure out what should we do over here for tracking
        // // the range of the symbol
        // if let Some(test_output) = test_output_maybe {
        //     // Check the action to take using the test output here or should
        //     // we also combine the lsp diagnostics over here as well???
        //     let test_output_logs = test_output.test_output();
        //     let tests_passed = test_output.passed();
        //     if tests_passed && test_output_logs.is_none() {
        //         // We passed! we can keep going
        //         println!("tool_box::check_code_correctness::test_passed");
        //         return Ok(());
        //     } else {
        //         if let Some(test_output_logs) = test_output_logs {
        //             let corrected_code = self
        //                 .fix_tests_by_editing(
        //                     fs_file_path,
        //                     &fs_file_content,
        //                     &edited_range,
        //                     instructions.to_owned(),
        //                     code_edit_extra_context,
        //                     original_code,
        //                     self.detect_language(fs_file_path).unwrap_or("".to_owned()),
        //                     test_output_logs,
        //                     llm.clone(),
        //                     provider.clone(),
        //                     api_keys.clone(),
        //                     message_properties.clone(),
        //                 )
        //                 .await;

        //             if corrected_code.is_err() {
        //                 println!("tool_box::check_code_correctness::missing_xml_tag");
        //                 continue;
        //             }
        //             let corrected_code = corrected_code.expect("is_err above to hold");
        //             // update our edited code
        //             updated_code = corrected_code.to_owned();
        //             // grab the symbol again since the location might have
        //             // changed between invocations of the code
        //             let symbol_to_edit = self
        //                 .find_sub_symbol_to_edit_with_name(
        //                     parent_symbol_name,
        //                     symbol_edited,
        //                     message_properties.clone(),
        //                 )
        //                 .await?;

        //             // Now that we have the corrected code, we should again apply
        //             // it to the file
        //             let _ = self
        //                 .apply_edits_to_editor(
        //                     fs_file_path,
        //                     symbol_to_edit.range(),
        //                     &corrected_code,
        //                     false,
        //                     message_properties.clone(),
        //                 )
        //                 .await;

        //             // return over here since we are done with the code correction flow
        //             // since this only happens in case of swe-bench
        //             continue;
        //         }
        //     }
        // }

        // Now we check for LSP diagnostics
        let lsp_diagnostics_output = self
            .get_lsp_diagnostics(fs_file_path, &edited_range, message_properties.to_owned())
            .await?;

        let diagnostics = lsp_diagnostics_output.get_diagnostics().to_owned();

        // Means a) no issues b) issues, but LSP didn't respond in time. Early return.
        if diagnostics.is_empty() {
            println!(
                "tool_box::check_code_correctness::get_diagnostics::is_empty(true) - no diagnostics found"
            );

            return Ok(());
        }

        let diagnostics_log = diagnostics
            .iter()
            .enumerate()
            .map(|(i, d)| format!("{}: {}", i, d.message()))
            .collect::<Vec<_>>()
            .join("\n");

        println!(
            "======\ntool_box::check_code_correctness::diagnostics\n======\n{diagnostics_log}"
        );

        // we open the file once, using it as reference to find snippets for diagnostics
        let fs_file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.to_owned())
            .await?
            .contents();

        // This is defensive - a single error from parsing snippets from diagnostic ranges will Error the entire iterator
        // Should never happen, so should be loud when if it does.
        let diagnostics_with_snippets = diagnostics
            .into_iter()
            .map(|d| d.with_snippet_from_contents(&fs_file_contents, fs_file_path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SymbolError::DiagnosticSnippetError(e))?;

        // parallel process diagnostics
        let _res = stream::iter(diagnostics_with_snippets.into_iter().map(|diagnostic_with_snippet| {
            (
                diagnostic_with_snippet,
                edited_symbol_outline_node_content.content().to_owned(),
                lsp_request_id.to_owned(),
                message_properties.to_owned(),
                instructions.to_owned(),
                llm.to_owned(),
                provider.to_owned(),
                api_keys.to_owned(),
                extra_symbol_list_ref.to_owned(),
                symbol_identifier.to_owned(),
                hub_sender.to_owned(),
            )
        }))
        .map(
            |(
                diagnostic_with_snippet,
                edited_symbol_content,
                lsp_request_id,
                message_properties,
                instructions,
                llm,
                provider,
                api_keys,
                extra_symbol_list_ref,
                symbol_identifier,
                hub_sender,
            )| async move {
                // get quick actions for diagnostics range
                let quick_fix_actions = self
                    .get_quick_fix_actions(
                        &fs_file_path.to_owned(),
                        diagnostic_with_snippet.range(),
                        lsp_request_id.to_owned(),
                        message_properties.to_owned(),
                    )
                    .await?
                    .remove_options(); // todo(zi: limit) why is this necessary?

                let quick_fix_actions_log = &quick_fix_actions.iter().enumerate()
                    .map(|(i, action)| format!("{}: {}", i, action.label()))
                    .collect::<Vec<_>>()
                    .join("\n");

                println!("======\ntoolbox::check_code_correctness::quick_fix_actions\n======\n{quick_fix_actions_log}");

                let request = CodeCorrectnessRequest::new(
                    edited_symbol_content,
                    symbol_name.to_owned(),
                    instructions,
                    diagnostic_with_snippet,
                    quick_fix_actions.to_vec(),
                    llm.clone(),
                    provider.clone(),
                    api_keys.clone(),
                    extra_symbol_list_ref,
                    message_properties.root_request_id().to_owned(),
                );

                // now we can send over the request to the LLM to select the best tool
                // for editing the code out
                let selected_action = self
                    .code_correctness_action_selection(request)
                    .await?;

                let selected_action_index = selected_action.index();
                let correctness_tool_thinking = selected_action.thinking();

                println!(
                    "tool_box::check_code_correctness::invoke_quick_action::index({})\nThinking: {}",
                    &selected_action_index, &correctness_tool_thinking
                );

                let ui_event_with_id = UIEventWithID::code_correctness_action(
                    message_properties.request_id_str().to_owned(),
                    symbol_identifier.clone(),
                    edited_range.clone(),
                    fs_file_path.to_owned(),
                    correctness_tool_thinking.to_owned(),
                );

                // IDE doesn't react to this atm.
                let _ =
                    message_properties
                        .ui_sender()
                        .send(ui_event_with_id);

                let _ = self
                    .handle_selected_action(
                        selected_action_index,
                        quick_fix_actions.len() as i64, // todo(zi): may panic?
                        correctness_tool_thinking,
                        &lsp_request_id,
                        message_properties.to_owned(),
                        tool_properties.to_owned(),
                        symbol_identifier.to_owned(),
                        hub_sender.to_owned(),
                        symbol_edited.to_owned(),
                    )
                    .await?;

                Ok(())
            },
        )
        .buffer_unordered(20) // 20 at a time for now todo(zi);
        .collect::<Vec<Result<(), SymbolError>>>()
        .await;
        Ok(())
    }

    async fn _quick_action_simple_edit(
        &self,
        symbol_to_edit: SymbolToEdit,
        symbol_identifier: SymbolIdentifier,
        tool_properties: ToolProperties,
        message_properties: SymbolEventMessageProperties,
        hub_sender: UnboundedSender<SymbolEventMessage>,
    ) -> Result<(), SymbolError> {
        println!("tool_box::check_code_correctness::code_correctness_with_edits (edit self)");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let symbol_event_message = SymbolEventMessage::message_with_properties(
            SymbolEventRequest::simple_edit_request(
                symbol_identifier,
                symbol_to_edit,
                tool_properties,
            ),
            message_properties,
            sender,
        );

        let _ = hub_sender
            .send(symbol_event_message)
            .map_err(|e| SymbolError::SymbolEventSendError(e))?;

        println!("tool_box::check_code_correctness::simple_edit_request::waiting");
        let _ = receiver.await.map_err(|e| SymbolError::RecvError(e))?;
        println!("tool_box::check_code_correctness::simple_edit_request::complete");

        Ok(())
    }

    /// Logic for processing selected actions
    async fn handle_selected_action(
        &self,
        action_index: i64,
        total_actions_len: i64,
        _correctness_tool_thinking: &str,
        lsp_request_id: &str,
        message_properties: SymbolEventMessageProperties,
        _tool_properties: ToolProperties,
        _symbol_identifier: SymbolIdentifier,
        _hub_sender: UnboundedSender<SymbolEventMessage>,
        symbol_edited: SymbolToEdit,
    ) -> Result<(), SymbolError> {
        // TODO(skcd): This needs to change because we will now have 3 actions which can
        // happen
        // code edit is a special operation which is not present in the quick-fix
        // but is provided by us, the way to check this is by looking at the index and seeing
        // if its >= length of the quick_fix_actions (we append to it internally in the LLM call)
        match action_index {
            // this used to be quick_action_simple_edit,
            // Since this may have been the culprit of deeper, unnecessary edits, temporarily removing this ability
            // this arm is now do nothing option
            i if i == total_actions_len => {
                println!("tool_box::check_code_correctness::no_changes_required");
                // consider sending UI event here
                Ok(())
            }
            i if i < total_actions_len => {
                let symbol_path = symbol_edited.fs_file_path();
                // invoke the code action over here with the editor
                let response = self
                    .invoke_quick_action(
                        action_index,
                        &lsp_request_id,
                        symbol_path,
                        message_properties,
                    )
                    .await?;
                if response.is_success() {
                    println!("tool_box::check_code_correctness::invoke_quick_action::is_success()");
                    // great we have a W
                } else {
                    // boo something bad happened, we should probably log and do something about this here
                    // for now we assume its all Ws
                    println!(
                        "tool_box::check_code_correctness::invoke_quick_action::fail::DO_SOMETHING_ABOUT_THIS"
                    );
                }

                Ok(())
            }
            _ => {
                // Handle unexpected index
                println!("Unexpected action index: {}", action_index);
                Ok(())
            }
        }
    }

    /// We are going to edit out the code depending on the test output
    async fn _fix_tests_by_editing(
        &self,
        fs_file_path: &str,
        fs_file_content: &str,
        symbol_to_edit_range: &Range,
        user_instructions: String,
        code_edit_extra_context: &str,
        original_code: &str,
        language: String,
        test_output: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let (code_above, code_below, code_in_selection) =
            split_file_content_into_parts(fs_file_content, symbol_to_edit_range);
        let input = ToolInput::TestOutputCorrection(TestOutputCorrectionRequest::new(
            fs_file_path.to_owned(),
            fs_file_content.to_owned(),
            user_instructions,
            code_above,
            code_below,
            code_in_selection,
            original_code.to_owned(),
            language,
            test_output,
            llm,
            provider,
            api_keys,
            code_edit_extra_context.to_owned(),
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_test_correction_output()
            .ok_or(SymbolError::WrongToolOutput)
    }

    // todo(zi): consider using search and replace here?

    // TODO(codestory): This part of the puzzle is still messed up since we are rewriting the whole
    // code over here which is not correct
    async fn _code_correctness_with_edits(
        &self,
        fs_file_path: &str,
        fs_file_content: &str,
        edited_range: &Range,
        extra_context: String,
        error_instruction: &str,
        instructions: &str,
        previous_code: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let (code_above, code_below, code_in_selection) =
            split_file_content_into_parts(fs_file_content, edited_range);
        let code_editing_error_request = ToolInput::CodeEditingError(CodeEditingErrorRequest::new(
            fs_file_path.to_owned(),
            code_above,
            code_below,
            code_in_selection,
            extra_context,
            previous_code.to_owned(),
            error_instruction.to_owned(),
            instructions.to_owned(),
            llm,
            provider,
            api_keys,
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(code_editing_error_request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .code_editing_for_error_fix()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn code_correctness_action_selection(
        &self,
        request: CodeCorrectnessRequest,
    ) -> Result<CodeCorrectnessAction, SymbolError> {
        let tool_input = ToolInput::CodeCorrectnessAction(request);

        self.tools
            .invoke(tool_input)
            .await
            .map_err(SymbolError::ToolError)?
            .get_code_correctness_action()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// This uses the search and replace mechanism to make edits
    ///
    /// This works really well for long symbols and symbols in general where
    /// we want to make edits to the wider codebase
    ///
    /// - Use anything >= sonnet3.5 level of intelligence or perf on diff-style
    /// editing for this mode
    pub async fn code_editing_with_search_and_replace(
        &self,
        sub_symbol: &SymbolToEdit,
        fs_file_path: &str,
        file_content: &str,
        selection_range: &Range,
        extra_context: String,
        instruction: String,
        symbol_identifier: &SymbolIdentifier,
        symbols_edited_list: Option<Vec<SymbolEditedItem>>,
        user_provided_context: Option<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        println!("============tool_box::code_edit_search_and_replace============");
        println!(
            "tool_box::code_edit_search_and_replace::fs_file_path({})::symbol_name({})",
            fs_file_path,
            sub_symbol.symbol_name(),
        );
        println!("============");
        println!(
            "tool_box::code_edit_search_and_replace::instructions({})",
            &instruction
        );
        let (_, _, in_range_selection) =
            split_file_content_into_parts(file_content, selection_range);
        // TODO(skcd): This might not be the perfect place to get cache-hits we might
        // want to send over the static list of edits at the start of each iteration?
        let recent_edits = self
            .recently_edited_files(
                vec![fs_file_path.to_owned()].into_iter().collect(),
                message_properties.clone(),
            )
            .await
            .ok();
        let symbols_to_edit = symbols_edited_list.map(|symbols| {
            symbols
                .into_iter()
                .filter(|symbol| symbol.is_new())
                .map(|symbol| {
                    let fs_file_path = symbol.fs_file_path();
                    let symbol_name = symbol.name();
                    format!(
                        r#"<symbol>
FILEPATH: {fs_file_path}
{symbol_name}
</symbol>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        let lsp_diagnostic_with_content = self
            .get_lsp_diagnostics_with_content(
                sub_symbol.fs_file_path(),
                selection_range,
                message_properties.clone(),
            )
            .await
            .unwrap_or_default();

        let session_id = message_properties.root_request_id().to_owned();
        let exchange_id = message_properties.request_id_str().to_owned();
        let llm_properties = message_properties.llm_properties().clone();
        println!("code_editing_with_search_and_replace::session_id({})::exchange_id({})::file_content_len({})", &session_id, &exchange_id, file_content.lines().into_iter().collect::<Vec<_>>().len());

        let request = ToolInput::SearchAndReplaceEditing(SearchAndReplaceEditingRequest::new(
            fs_file_path.to_owned(),
            selection_range.clone(),
            in_range_selection,
            // we are passing the full file content
            file_content.to_owned(),
            extra_context,
            llm_properties,
            symbols_to_edit,
            instruction,
            message_properties.root_request_id().to_owned(),
            symbol_identifier.clone(),
            uuid::Uuid::new_v4().to_string(),
            message_properties.ui_sender().clone(),
            user_provided_context,
            message_properties.editor_url(),
            recent_edits,
            sub_symbol.previous_user_queries().to_vec(),
            lsp_diagnostic_with_content,
            false,
            session_id,
            exchange_id,
            sub_symbol.plan_step_id(),
            sub_symbol.previous_message(),
            message_properties.cancellation_token(),
            sub_symbol.aide_rules(),
            sub_symbol.should_stream(),
        ));
        println!(
            "tool_box::code_edit_outline::start::symbol_name({})",
            sub_symbol.symbol_name()
        );

        let start = Instant::now();

        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_search_and_replace_output()
            .ok_or(SymbolError::WrongToolOutput)?;

        println!(
            "code_editing_with_search_and_replace::time: {:?}",
            start.elapsed()
        );

        let updated_code = response.updated_code();
        println!(
            "search_and_replacing::code_editing_lines::({})",
            updated_code.lines().into_iter().collect::<Vec<_>>().len()
        );

        println!(
            "tool_box::search_and_replace::finish::symbol_name({})",
            sub_symbol.symbol_name()
        );
        Ok(updated_code.to_owned())
    }

    pub async fn code_edit(
        &self,
        fs_file_path: &str,
        file_content: &str,
        selection_range: &Range,
        extra_context: String,
        instruction: String,
        swe_bench_initial_edit: bool,
        symbol_to_edit: Option<String>,
        is_new_sub_symbol: Option<String>,
        symbol_edited_list: Option<Vec<SymbolEditedItem>>,
        symbol_identifier: &SymbolIdentifier,
        user_provided_context: Option<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        println!("============tool_box::code_edit============");
        println!("tool_box::code_edit::fs_file_path:{}", fs_file_path);
        println!("tool_box::code_edit::selection_range:{:?}", selection_range);
        println!("============");
        // we need to get the range above and then below and then in the selection
        let language = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .map(|language_config| language_config.get_language())
            .flatten()
            .unwrap_or("".to_owned());
        let (above, below, in_range_selection) =
            split_file_content_into_parts(file_content, selection_range);

        let new_symbols_edited = symbol_edited_list.map(|symbol_list| {
            symbol_list
                .into_iter()
                .filter(|symbol| symbol.is_new())
                .map(|symbol| {
                    let fs_file_path = symbol.fs_file_path();
                    let symbol_name = symbol.name();
                    format!(
                        r#"<symbol>
FILEPATH: {fs_file_path}
{symbol_name}
</symbol>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        let session_id = message_properties.root_request_id().to_owned();
        let exchange_id = message_properties.request_id_str().to_owned();
        let llm_properties = message_properties.llm_properties().clone();
        let request = ToolInput::CodeEditing(CodeEdit::new(
            above,
            below,
            fs_file_path.to_owned(),
            in_range_selection,
            extra_context,
            language.to_owned(),
            instruction,
            llm_properties.clone(),
            swe_bench_initial_edit,
            symbol_to_edit,
            is_new_sub_symbol,
            message_properties.root_request_id().to_owned(),
            selection_range.clone(),
            // we want a complete edit over here
            false,
            new_symbols_edited,
            // should we stream the edits we are making over here
            true,
            symbol_identifier.clone(),
            message_properties.ui_sender(),
            true, // disable thinking by default
            user_provided_context,
            session_id,
            exchange_id,
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_edit_output()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// We get the outline of the files which are mentioned in the user context
    /// along with the variables (excluding any selection)
    pub async fn outline_for_user_context(
        &self,
        user_context: &UserContext,
        message_properties: SymbolEventMessageProperties,
    ) -> String {
        // we need to traverse the files and the folders which are mentioned over here
        let file_paths = user_context
            .file_content_map
            .iter()
            .map(|file_content_value| file_content_value.file_path.to_owned())
            .into_iter()
            .collect::<HashSet<String>>();
        let _ = stream::iter(
            file_paths
                .clone()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(file_path, message_properties)| async move {
            let file_open_response = self
                .file_open(file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_open_response) = file_open_response {
                // force add the document
                let _ = self
                    .force_add_document(
                        &file_path,
                        file_open_response.contents_ref(),
                        file_open_response.language(),
                    )
                    .await;
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

        // now we want to parse the files which we are getting
        stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(file_path, message_properties)| async move {
            let outline_nodes = self
                .get_outline_nodes_from_editor(&file_path, message_properties.clone())
                .await;
            (file_path, outline_nodes)
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|(_, outline_nodes)| outline_nodes)
        .map(|outline_nodes| {
            outline_nodes
                .into_iter()
                .filter_map(|outline_node| outline_node.get_outline_node_compressed())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
    }

    async fn invoke_quick_action(
        &self,
        quick_fix_index: i64,
        lsp_request_id: &str,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<LSPQuickFixInvocationResponse, SymbolError> {
        let request = ToolInput::QuickFixInvocationRequest(LSPQuickFixInvocationRequest::new(
            lsp_request_id.to_owned(),
            quick_fix_index,
            message_properties.editor_url(),
            fs_file_path.to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_quick_fix_invocation_result()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn get_file_content(&self, fs_file_path: &str) -> Result<String, SymbolError> {
        self.file_open(fs_file_path.to_owned(), new_dummy_message_properties())
            .await
            .map(|response| response.contents())
    }

    pub async fn gather_important_symbols_with_definition(
        &self,
        fs_file_path: &str,
        file_content: &str,
        selection_range: &Range,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        query: &str,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<Vec<Option<(CodeSymbolWithThinking, String)>>, SymbolError> {
        let language = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .map(|language_config| language_config.get_language())
            .flatten()
            .unwrap_or("".to_owned());
        let request = ToolInput::RequestImportantSymbols(CodeSymbolImportantRequest::new(
            None,
            vec![],
            fs_file_path.to_owned(),
            file_content.to_owned(),
            selection_range.clone(),
            llm,
            provider,
            api_keys,
            language,
            query.to_owned(),
            message_properties.root_request_id().to_owned(),
        ));
        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_important_symbols()
            .ok_or(SymbolError::WrongToolOutput)?;
        let symbols_to_grab = response
            .symbols()
            .into_iter()
            .map(|symbol| symbol.clone())
            .collect::<Vec<_>>();
        let symbol_locations = stream::iter(symbols_to_grab)
            .map(|symbol| async move {
                let symbol_name = symbol.code_symbol();
                let location = self.find_symbol_in_file(symbol_name, file_content).await;
                (symbol, location)
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;

        // we want to grab the defintion of these symbols over here, so we can either
        // ask the hub and get it back or do something else... asking the hub is the best
        // thing to do over here
        // we now need to go to the definitions of these symbols and then ask the hub
        // manager to grab the outlines
        let symbol_to_definition =
            stream::iter(symbol_locations.into_iter().map(|symbol_location| {
                (
                    symbol_location,
                    hub_sender.clone(),
                    message_properties.clone(),
                )
            }))
            .map(
                |((symbol, location), hub_sender, message_properties)| async move {
                    if let Ok(location) = location {
                        // we might not get the position here for some weird reason which
                        // is also fine
                        let position = location.get_position();
                        if let Some(position) = position {
                            let possible_file_path = self
                                .go_to_definition(
                                    fs_file_path,
                                    position,
                                    message_properties.clone(),
                                )
                                .await
                                .map(|position| {
                                    // there are multiple definitions here for some
                                    // reason which I can't recall why, but we will
                                    // always take the first one and run with it cause
                                    // we then let this symbol agent take care of things
                                    // TODO(skcd): The symbol needs to be on the
                                    // correct file path over here
                                    let symbol_file_path = position
                                        .definitions()
                                        .first()
                                        .map(|definition| definition.file_path().to_owned());
                                    symbol_file_path
                                })
                                .ok()
                                .flatten();
                            if let Some(definition_file_path) = possible_file_path {
                                let (sender, receiver) = tokio::sync::oneshot::channel();
                                // we have the possible file path over here
                                let event = SymbolEventMessage::message_with_properties(
                                    SymbolEventRequest::outline(
                                        SymbolIdentifier::with_file_path(
                                            symbol.code_symbol(),
                                            &definition_file_path,
                                        ),
                                        tool_properties.clone(),
                                    ),
                                    message_properties
                                        .set_request_id(uuid::Uuid::new_v4().to_string()),
                                    sender,
                                );
                                let _ = hub_sender.send(event);
                                receiver
                                    .await
                                    .map(|response| response.to_string())
                                    .ok()
                                    .map(|definition_outline| (symbol, definition_outline))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                },
            )
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;
        Ok(symbol_to_definition)
    }

    pub async fn get_quick_fix_actions(
        &self,
        fs_file_path: &str,
        range: &Range,
        request_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GetQuickFixResponse, SymbolError> {
        let request = ToolInput::QuickFixRequest(GetQuickFixRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            range.clone(),
            request_id,
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_quick_fix_actions()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn get_lsp_diagnostics(
        &self,
        fs_file_path: &str,
        range: &Range,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<LSPDiagnosticsOutput, SymbolError> {
        let input = ToolInput::LSPDiagnostics(LSPDiagnosticsInput::new(
            fs_file_path.to_owned(),
            range.clone(),
            message_properties.editor_url().to_owned(),
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_lsp_diagnostics()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn use_terminal_command(
        &self,
        command: &str,
        wait_for_exit: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<TerminalOutput, SymbolError> {
        let input = ToolInput::TerminalCommand(TerminalInput::new(
            command.to_owned(),
            message_properties.editor_url().to_owned(),
            wait_for_exit.to_owned(),
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .terminal_command()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Grabs full workspace diagnostics
    pub async fn grab_workspace_diagnostics(
        &self,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(Vec<LSPDiagnosticError>, Vec<VariableInformation>), SymbolError> {
        let input = ToolInput::FileDiagnostics(FileDiagnosticsInput::new(
            "".to_owned(),
            message_properties.editor_url().to_owned(),
            false,
            None,
            true,
        ));
        let file_diagnostics_output = self
            .tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_file_diagnostics()
            .ok_or(SymbolError::WrongToolOutput)?;
        let file_paths = file_diagnostics_output
            .remove_diagnostics()
            .into_iter()
            .map(|file_diagnostic_output| file_diagnostic_output.fs_file_path().to_owned())
            .collect::<HashSet<String>>()
            .into_iter()
            .collect::<Vec<_>>();
        // read each of the files and make it a variables
        let extra_variables = stream::iter(
            file_paths
                .to_vec()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let file_open_response = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await;
            file_open_response.map(|file_open_response| {
                VariableInformation::create_file(
                    file_open_response.full_range(),
                    fs_file_path.to_owned(),
                    file_open_response.fs_file_path().to_owned(),
                    file_open_response.contents_ref().to_string(),
                    file_open_response.language().to_owned(),
                )
            })
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<Vec<_>>();
        let file_signals = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let diagnostics = self
                .get_file_diagnostics(&fs_file_path, message_properties.clone(), true)
                .await;
            let file_contents = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_contents) = file_contents {
                diagnostics.map(|diagnostics| {
                    diagnostics
                        .remove_diagnostics()
                        .into_iter()
                        .filter_map(|diagnostic| {
                            DiagnosticWithSnippet::from_diagnostic_and_contents(
                                diagnostic,
                                file_contents.contents_ref(),
                                fs_file_path.to_owned(),
                            )
                            .ok()
                        })
                        .collect::<Vec<_>>()
                })
            } else {
                diagnostics.map(|diagnostics| {
                    diagnostics
                        .remove_diagnostics()
                        .into_iter()
                        .map(|diagnostic| {
                            DiagnosticWithSnippet::new(
                                diagnostic.message().to_owned(),
                                diagnostic.range().clone(),
                                "".to_owned(),
                                fs_file_path.to_owned(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            }
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|data| data.ok())
        .flatten()
        .map(|diagnostic| {
            LSPDiagnosticError::new(
                diagnostic.range().clone(),
                diagnostic.snippet().to_owned(),
                diagnostic.fs_file_path().to_owned(),
                diagnostic.message().to_owned(),
                diagnostic.quick_fix_labels().to_owned(),
                diagnostic.parameter_hints().to_owned(),
            )
        })
        .collect::<Vec<_>>();
        Ok((file_signals, extra_variables))
    }

    /// Grabs the hover information from the editor
    pub async fn get_hover_information(
        &self,
        fs_file_path: &str,
        position: &Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<FileDiagnosticsOutput, SymbolError> {
        let input = ToolInput::FileDiagnostics(FileDiagnosticsInput::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            false,
            Some(position.clone()),
            false,
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_file_diagnostics()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn get_file_diagnostics(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
        with_enrichment: bool,
    ) -> Result<FileDiagnosticsOutput, SymbolError> {
        let input = ToolInput::FileDiagnostics(FileDiagnosticsInput::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            with_enrichment,
            None,
            false,
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_file_diagnostics()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// gets diagnostics and their snippets
    pub async fn get_lsp_diagnostics_for_files(
        &self,
        file_paths: Vec<String>,
        message_properties: SymbolEventMessageProperties,
        with_enrichment: bool,
    ) -> Result<Vec<LSPDiagnosticError>, SymbolError> {
        let file_signals = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let diagnostics = self
                .get_file_diagnostics(&fs_file_path, message_properties.clone(), with_enrichment)
                .await;
            let file_contents = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_contents) = file_contents {
                diagnostics.map(|diagnostics| {
                    diagnostics
                        .remove_diagnostics()
                        .into_iter()
                        .filter_map(|diagnostic| {
                            DiagnosticWithSnippet::from_diagnostic_and_contents(
                                diagnostic,
                                file_contents.contents_ref(),
                                fs_file_path.to_owned(),
                            )
                            .ok()
                        })
                        .collect::<Vec<_>>()
                })
            } else {
                diagnostics.map(|diagnostics| {
                    diagnostics
                        .remove_diagnostics()
                        .into_iter()
                        .map(|diagnostic| {
                            DiagnosticWithSnippet::new(
                                diagnostic.message().to_owned(),
                                diagnostic.range().clone(),
                                "".to_owned(),
                                fs_file_path.to_owned(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            }
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|data| data.ok())
        .flatten()
        .map(|diagnostic| {
            LSPDiagnosticError::new(
                diagnostic.range().clone(),
                diagnostic.snippet().to_owned(),
                diagnostic.fs_file_path().to_owned(),
                diagnostic.message().to_owned(),
                diagnostic.quick_fix_labels().to_owned(),
                diagnostic.parameter_hints().to_owned(),
            )
        })
        .collect::<Vec<_>>();
        Ok(file_signals)
    }

    pub async fn apply_edits_to_editor(
        &self,
        fs_file_path: &str,
        range: &Range,
        updated_code: &str,
        // if we should be applying these edits directly
        apply_directly: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<EditorApplyResponse, SymbolError> {
        let input = ToolInput::EditorApplyChange(EditorApplyRequest::new(
            fs_file_path.to_owned(),
            updated_code.to_owned(),
            range.clone(),
            message_properties.editor_url().to_owned(),
            apply_directly,
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_editor_apply_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn find_symbol_in_file(
        &self,
        symbol_name: &str,
        file_contents: &str,
    ) -> Result<FindInFileResponse, SymbolError> {
        // Here we are going to get the position of the symbol
        let request = ToolInput::GrepSingleFile(FindInFileRequest::new(
            file_contents.to_owned(),
            symbol_name.to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .grep_single_file()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn filter_code_snippets_in_symbol_for_editing(
        &self,
        xml_string: String,
        query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        symbols_to_be_edited: Option<&[SymbolEditedItem]>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeToEditSymbolResponse, SymbolError> {
        let symbols_to_be_edited = symbols_to_be_edited.map(|symbols_to_be_edited| {
            symbols_to_be_edited
                .into_iter()
                .filter(|symbol| symbol.is_new())
                .map(|symbol| {
                    let symbol_name = symbol.name();
                    let fs_file_path = symbol.fs_file_path();
                    format!(
                        r#"<symbol>
FILEPATH: {fs_file_path}
{symbol_name}
</symbol>"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        let request =
            ToolInput::FilterCodeSnippetsForEditingSingleSymbols(CodeToEditSymbolRequest::new(
                xml_string,
                query,
                symbols_to_be_edited,
                llm,
                provider,
                api_keys,
                message_properties.root_request_id().to_owned(),
            ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .code_to_edit_in_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Grabs the location where we should be adding the new symbol
    ///
    /// Instead of putting it at the end of the file, we can add it to the exact
    /// location we are interested in which is detected by the LLM
    pub async fn code_location_for_addition(
        &self,
        fs_file_path: &str,
        symbol_identifer: &SymbolIdentifier,
        add_request: &str,
        message_properties: SymbolEventMessageProperties,
        // returns the line where we want to insert it and if we want to insert it
        // before the line or after the line
        // before case:
        // <we_get_start_position_here>def something():
        //     # pass
        // so here we want to insert a new line at the start of the line and then
        // insert it
        // after case:
        // def something():
        //    # pass <we_get_start_position_here>
        // we just insert a new line at the end of this line and then insert it
        // the boolean here represents if we want to insert it at the start of the line
        // or the end of the line
        // think of this as (Position, at_start)
    ) -> Result<(Position, bool), SymbolError> {
        println!(
            "too_box::code_location_for_addition::start::symbol({})",
            symbol_identifer.symbol_name()
        );
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let _ = self
            .force_add_document(
                fs_file_path,
                file_contents.contents_ref(),
                file_contents.language(),
            )
            .await?;
        let outline_nodes: Vec<_> = self
            .get_outline_nodes_grouped(fs_file_path)
            .await
            .unwrap_or_default();
        let outline_nodes_range = outline_nodes
            .iter()
            .map(|outline_node| outline_node.range().clone())
            .collect::<Vec<_>>();
        let outline_nodes_str = outline_nodes
            .into_iter()
            .map(|outline_node| outline_node.get_outline_for_prompt())
            .collect::<Vec<_>>();
        let request = ToolInput::CodeSymbolNewLocation(CodeSymbolNewLocationRequest::new(
            fs_file_path.to_owned(),
            outline_nodes_str,
            symbol_identifer.symbol_name().to_owned(),
            add_request.to_owned(),
            LLMProperties::new(
                LLMType::Llama3_1_8bInstruct,
                LLMProvider::FireworksAI,
                LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
                    "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
                )),
            ),
            message_properties.root_request_id().to_owned(),
        ));
        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_symbol_new_location()
            .ok_or(SymbolError::WrongToolOutput)?;
        let response_idx = response.idx();
        // if we are at the end of the file, then we can just get the last line
        // after the
        if response_idx == outline_nodes_range.len() {
            if outline_nodes_range.is_empty() {
                Ok((Position::new(0, 0, 0), false))
            } else {
                Ok((
                    outline_nodes_range[outline_nodes_range.len() - 1].end_position(),
                    false,
                ))
            }
        } else {
            let outline_node_selected = outline_nodes_range.get(response_idx);
            if let Some(outline_node) = outline_node_selected {
                let file_content_lines = file_contents
                    .contents_ref()
                    .lines()
                    .enumerate()
                    .collect::<Vec<_>>();
                if outline_node.start_position().line() + 1 <= file_content_lines.len() {
                    // we need to find the first empty line over here or if we have no
                    // empty line, then we can just insert it at 0'th line number
                    let mut check_start_line = outline_node.start_position().line();
                    let start_line;
                    loop {
                        if check_start_line == 0 {
                            start_line = 0;
                            break;
                        }
                        // if line is empty, we are in luck we can start editing here
                        if file_content_lines[check_start_line].1.is_empty() {
                            start_line = check_start_line;
                            break;
                        } else {
                            // line is not empty, so we have to go up a line
                            check_start_line = check_start_line - 1;
                        }
                    }
                    Ok((Position::new(start_line, 0, 0), true))
                } else {
                    // out of bound node position which is weird
                    Err(SymbolError::NoOutlineNodeSatisfyPosition)
                }
            } else {
                Err(SymbolError::OutlineNodeEditingNotSupported)
            }
        }
    }

    /// todo(zi): this is a dead method walking...but a perfect test case for refactoring
    pub async fn get_outline_nodes_grouped(&self, fs_file_path: &str) -> Option<Vec<OutlineNode>> {
        self.symbol_broker.get_symbols_outline(fs_file_path).await
    }

    async fn get_ouline_nodes_grouped_fresh(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<Vec<OutlineNode>> {
        self.get_outline_nodes_from_editor(fs_file_path, message_properties)
            .await
    }

    /// Sends a request to the editor to get the outline nodes
    pub async fn get_outline_nodes_from_editor(
        &self,
        fs_file_path: &str,
    ) -> Option<Vec<OutlineNode>> {
        let input = ToolInput::OutlineNodesUsingEditor(OutlineNodesUsingEditorRequest::new(
            fs_file_path.to_owned(), 
            message_properties.editor_url(),
        ));
        self.tools
            .invoke(input)
            .await
            .ok()
            .map(|response| response.get_outline_nodes_from_editor())
            .flatten()
            .map(|outline_nodes_response| {
                outline_nodes_response.to_outline_nodes(fs_file_path.to_owned())
            })
    }

    /// Returns the outline nodes using the editor instead of relying on tree-sitter
    ///
    /// Use this with caution for now since some concepts don't map 1:1 to how
    /// the world looks like with tree-sitter and sidecar
    /// example: golang class names on functions are not registered when using this
    pub async fn get_outline_nodes_using_editor(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNodesUsingEditorResponse, SymbolError> {
        let request = ToolInput::OutlineNodesUsingEditor(OutlineNodesUsingEditorRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url(),
        ));
        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_outline_nodes_from_editor()
            .ok_or(SymbolError::WrongToolOutput);
        response
    }

    pub async fn get_outline_nodes(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<Vec<OutlineNodeContent>> {
        let file_open_result = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await;
        if let Err(_) = file_open_result {
            return None;
        }
        let file_open = file_open_result.expect("if let Err to hold");
        let _ = self
            .force_add_document(fs_file_path, file_open.contents_ref(), file_open.language())
            .await;
        self.symbol_broker
            .get_symbols_outline(&fs_file_path)
            .await
            .map(|outline_nodes| {
                // class and the functions are included here
                outline_nodes
                    .into_iter()
                    .map(|outline_node| {
                        // let children = outline_node.consume_all_outlines();
                        // outline node here contains the classes and the functions
                        // which we have to edit
                        // so one way would be to ask the LLM to edit it
                        // another is to figure out if we can show it all the functions
                        // which are present inside the class and ask it to make changes
                        let outline_content = outline_node.content().clone();
                        let all_outlines = outline_node.consume_all_outlines();
                        vec![outline_content]
                            .into_iter()
                            .chain(all_outlines)
                            .collect::<Vec<OutlineNodeContent>>()
                    })
                    .flatten()
                    .collect::<Vec<_>>()
            })
    }

    // this can help us find the symbol for a given range!
    pub async fn symbol_in_range(
        &self,
        fs_file_path: &str,
        range: &Range,
    ) -> Option<Vec<OutlineNode>> {
        self.get_outline_nodes_from_editor(fs_file_path)
            .await
            .map(|nodes| nodes.into_iter().filter(|node| node.range().intersects_with_another_range(range)).collect())
    }

    pub async fn force_add_document(
        &self,
        fs_file_path: &str,
        file_contents: &str,
        language: &str,
    ) -> Result<(), SymbolError> {
        let _ = self
            .symbol_broker
            .force_add_document(
                fs_file_path.to_owned(),
                file_contents.to_owned(),
                language.to_owned(),
            )
            .await;
        Ok(())
    }

    pub async fn get_symbol_references(
        &self,
        fs_file_path: String,
        symbol: String,
        possible_range: Range,
        message_properties: SymbolEventMessageProperties,
        _request_id: String,
    ) -> Result<Vec<ReferenceLocation>, SymbolError> {
        let symbol_closest = self
            .find_symbol_to_edit_closest_to_range(
                &SymbolToEdit::new(
                    symbol,
                    possible_range,
                    fs_file_path.to_owned(),
                    vec![],
                    false,
                    false,
                    true,
                    "".to_owned(),
                    None,
                    false,
                    None,
                    true,
                    None,
                    vec![],
                    None,
                ),
                message_properties.clone(),
            )
            .await?;
        let reference_locations = self
            .go_to_references(
                fs_file_path,
                symbol_closest.identifier_range().start_position(),
                message_properties.clone(),
            )
            .await?
            .locations();
        Ok(reference_locations)
    }

    pub async fn file_open(
        &self,
        fs_file_path: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OpenFileResponse, SymbolError> {
        let request = ToolInput::OpenFile(OpenFileRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            None,
            None,
        ));
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::open_file_event(
                message_properties.root_request_id().to_owned(),
                message_properties.request_id_str().to_owned(),
                fs_file_path,
            ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_file_open_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn find_in_file(
        &self,
        file_content: String,
        symbol: String,
    ) -> Result<FindInFileResponse, SymbolError> {
        let request = ToolInput::GrepSingleFile(FindInFileRequest::new(file_content, symbol));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .grep_single_file()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// Allows us to go to type definition for any given symbol
    pub async fn go_to_type_definition(
        &self,
        fs_file_path: &str,
        position: Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToDefinitionResponse, SymbolError> {
        let request = ToolInput::GoToTypeDefinition(GoToDefinitionRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            position,
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .go_to_type_definition_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn go_to_definition(
        &self,
        fs_file_path: &str,
        position: Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToDefinitionResponse, SymbolError> {
        let request = ToolInput::GoToDefinition(GoToDefinitionRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
            position,
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_go_to_definition()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn edits_required_full_symbol(
        &self,
        symbol_content: &str,
        user_request: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<bool, SymbolError> {
        let tool_input = ToolInput::ShouldEditCode(ShouldEditCodeSymbolRequest::new(
            symbol_content.to_owned(),
            user_request.to_owned(),
            LLMProperties::new(
                LLMType::GeminiProFlash,
                LLMProvider::GoogleAIStudio,
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
            ),
            message_properties.root_request_id().to_owned(),
        ));
        let output = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .should_edit_code_symbol_full()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(output.should_edit())
    }

    /// Used to make sure if the edit request should proceed as planned or we
    /// are already finished with the edit request
    pub async fn should_edit_symbol(
        &self,
        symbol_to_edit: &SymbolToEdit,
        symbol_content: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<FilterEditOperationResponse, SymbolError> {
        let tool_input = ToolInput::FilterEditOperation(FilterEditOperationRequest::new(
            symbol_content.to_owned(),
            symbol_to_edit.symbol_name().to_owned(),
            symbol_to_edit.fs_file_path().to_owned(),
            symbol_to_edit.original_user_query().to_owned(),
            symbol_to_edit.instructions().to_vec().join("\n"),
            LLMProperties::new(
                LLMType::Llama3_1_8bInstruct,
                LLMProvider::FireworksAI,
                LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
                    "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
                )),
            ),
            message_properties.root_request_id().to_owned(),
        ));
        let output = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_filter_edit_operation_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(output)
    }

    // This helps us find the snippet for the symbol in the file, this is the
    // best way to do this as this is always exact and we never make mistakes
    // over here since we are using the LSP as well
    pub async fn find_snippet_for_symbol(
        &self,
        fs_file_path: &str,
        symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Snippet, SymbolError> {
        // we grab the outlines over here
        let outline_nodes = self
            .tools
            .invoke(ToolInput::OutlineNodesUsingEditor(
                OutlineNodesUsingEditorRequest::new(
                    fs_file_path.to_owned(),
                    message_properties.editor_url(),
                ),
            ))
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_outline_nodes_from_editor()
            .map(|response| response.to_outline_nodes(fs_file_path.to_owned()));

        // We will either get an outline node or we will get None
        // for today, we will go with the following assumption
        // - if the document has already been open, then its good
        // - otherwise we open the document and parse it again
        if let Some(outline_nodes) = outline_nodes {
            let mut outline_nodes = self.grab_symbols_from_outline(outline_nodes, symbol_name);

            // if there are no outline nodes, then we have to skip this part
            // and keep going
            if outline_nodes.is_empty() {
                // here we need to do go-to-definition
                // first we check where the symbol is present on the file
                // and we can use goto-definition
                // so we first search the file for where the symbol is
                // this will be another invocation to the tools
                // and then we ask for the definition once we find it
                let file_data = self
                    .file_open(fs_file_path.to_owned(), message_properties.clone())
                    .await?;
                let file_content = file_data.contents();
                // now we parse it and grab the outline nodes
                let find_in_file = self
                    .find_in_file(file_content, symbol_name.to_owned())
                    .await
                    .map(|find_in_file| find_in_file.get_position())
                    .ok()
                    .flatten();
                // now that we have a poition, we can ask for go-to-definition
                if let Some(file_position) = find_in_file {
                    let definition = self
                        .go_to_definition(fs_file_path, file_position, message_properties.clone())
                        .await?;
                    // let definition_file_path = definition.file_path().to_owned();
                    let snippet_node = self
                        .grab_symbol_content_from_definition(
                            symbol_name,
                            definition,
                            message_properties,
                        )
                        .await?;
                    Ok(snippet_node)
                } else {
                    Err(SymbolError::SnippetNotFound)
                }
            } else {
                // if we have multiple outline nodes, then we need to select
                // the best one, this will require another invocation from the LLM
                // we have the symbol, we can just use the outline nodes which is
                // the first
                let outline_node = outline_nodes.remove(0);
                Ok(Snippet::new(
                    outline_node.name().to_owned(),
                    outline_node.range().clone(),
                    outline_node.fs_file_path().to_owned(),
                    outline_node.content().to_owned(),
                    outline_node,
                ))
            }
        } else {
            Err(SymbolError::OutlineNodeNotFound(symbol_name.to_owned()))
        }
    }

    /// Finds the changed symbols which are present in the file using simple git-diff
    ///
    /// Coming soon:
    /// - can anchor on a range instead
    /// - has history of the changes
    /// - will be a stream based input instead of ping based as is now
    pub async fn find_changed_symbols(
        &self,
        _file_paths: Vec<String>,
        _request_id: &str,
        _ui_sender: UnboundedSender<UIEventWithID>,
    ) -> Result<Vec<(MechaCodeSymbolThinking, Vec<String>)>, SymbolError> {
        // we raw execute git dif commands here (not recommended but ... whatever)
        todo!();
    }

    /// If we cannot find the symbol using normal mechanisms we just search
    /// for the symbol by hand in the file and grab the outline node which contains
    /// the symbols
    pub async fn grab_symbol_using_search(
        &self,
        important_symbols: CodeSymbolImportantResponse,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<(MechaCodeSymbolThinking, Vec<String>)>, SymbolError> {
        let ordered_symbols = important_symbols.ordered_symbols();
        stream::iter(ordered_symbols.iter().map(|ordered_symbol| {
            (
                ordered_symbol.file_path().to_owned(),
                message_properties.clone(),
            )
        }))
        .for_each(|(file_path, message_properties)| async move {
            let file_open_response = self
                .file_open(file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_open_response) = file_open_response {
                let _ = self
                    .force_add_document(
                        &file_path,
                        file_open_response.contents_ref(),
                        file_open_response.language(),
                    )
                    .await;
            }
        })
        .await;

        let mut mecha_code_symbols: Vec<(MechaCodeSymbolThinking, Vec<String>)> = vec![];
        for symbol in ordered_symbols.into_iter() {
            let file_path = symbol.file_path();
            let symbol_name = symbol.code_symbol();
            let outline_nodes = self.symbol_broker.get_symbols_outline(file_path).await;
            if let Some(outline_nodes) = outline_nodes {
                let possible_outline_nodes = outline_nodes
                    .into_iter()
                    .find(|outline_node| outline_node.content().content().contains(symbol_name));
                if let Some(outline_node) = possible_outline_nodes {
                    let outline_node_content = outline_node.content();
                    mecha_code_symbols.push((
                        MechaCodeSymbolThinking::new(
                            outline_node.name().to_owned(),
                            vec![],
                            false,
                            outline_node.fs_file_path().to_owned(),
                            Some(Snippet::new(
                                outline_node_content.name().to_owned(),
                                outline_node_content.range().clone(),
                                outline_node_content.fs_file_path().to_owned(),
                                outline_node_content.content().to_owned(),
                                outline_node_content.clone(),
                            )),
                            vec![],
                            Arc::new(self.clone()),
                        ),
                        symbol.steps().to_vec(),
                    ));
                }
            }
        }
        Ok(mecha_code_symbols)
    }

    /// Does another COT pass after the original plan was generated but this
    /// time the code is also visibile to the LLM
    pub async fn planning_before_code_editing(
        &self,
        important_symbols: &CodeSymbolImportantResponse,
        user_query: &str,
        llm_properties: LLMProperties,
        // we need to change our prompt when we are doing big search
        _is_big_search: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeSymbolImportantResponse, SymbolError> {
        let ordered_symbol_file_paths = important_symbols
            .ordered_symbols()
            .into_iter()
            .map(|symbol| symbol.file_path().to_owned())
            .collect::<Vec<_>>();
        let symbol_file_path = important_symbols
            .symbols()
            .into_iter()
            .map(|symbol| symbol.file_path().to_owned())
            .collect::<Vec<_>>();
        let final_paths = ordered_symbol_file_paths
            .into_iter()
            .chain(symbol_file_path.into_iter())
            .collect::<HashSet<String>>();
        let file_content_map = stream::iter(
            final_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(path, message_properties)| async move {
            let file_open = self.file_open(path.to_owned(), message_properties).await;
            match file_open {
                Ok(file_open_response) => Some((path, file_open_response.contents())),
                Err(_) => None,
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<String, String>>();
        // create the original plan here
        let original_plan = important_symbols.ordered_symbols_to_plan();
        let request = ToolInput::PlanningBeforeCodeEdit(PlanningBeforeCodeEditRequest::new(
            user_query.to_owned(),
            file_content_map,
            original_plan,
            llm_properties,
            message_properties.root_request_id().to_owned(),
            message_properties.request_id_str().to_owned(),
            message_properties.cancellation_token(),
        ));
        let final_plan_list = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_plan_before_code_editing()
            .ok_or(SymbolError::WrongToolOutput)?
            .final_plan_list();
        let response = CodeSymbolImportantResponse::new(
            final_plan_list
                .iter()
                .map(|plan_item| {
                    CodeSymbolWithThinking::new(
                        plan_item.symbol_name().to_owned(),
                        plan_item.plan().to_owned(),
                        plan_item.file_path().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
            final_plan_list
                .iter()
                .map(|plan_item| {
                    CodeSymbolWithSteps::new(
                        plan_item.symbol_name().to_owned(),
                        vec![plan_item.plan().to_owned()],
                        false,
                        plan_item.file_path().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
        );
        Ok(response)
    }

    /// CodeSymbolImportantResponse gets turned to a list of mechacodesymbolthinking
    /// where each one of them is a file we want to edit
    pub async fn important_symbols_per_file(
        &self,
        important_symbols: &CodeSymbolImportantResponse,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<(MechaCodeSymbolThinking, Vec<String>)>, SymbolError> {
        let symbols = important_symbols.ordered_symbols();
        let mut previous_file_path: Option<String> = None;
        let mut previous_steps = vec![];
        let mut file_paths_and_steps = vec![];
        for symbol in symbols.into_iter() {
            let steps = symbol
                .steps()
                .into_iter()
                .map(|step| format!("- {}", step))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(ref previous_file_path_ref) = previous_file_path.clone() {
                if *previous_file_path_ref == symbol.file_path() {
                    previous_steps.push(steps);
                } else {
                    // add the current symbol fs_file_path and the step over here
                    file_paths_and_steps
                        .push((previous_file_path_ref.to_owned(), previous_steps.to_vec()));
                    previous_file_path = Some(symbol.file_path().to_owned());
                    previous_steps = vec![steps];
                }
            } else {
                previous_file_path = Some(symbol.file_path().to_owned());
                previous_steps.push(steps);
            }
        }

        if let Some(previous_file_path) = previous_file_path {
            file_paths_and_steps.push((previous_file_path, previous_steps));
        }

        // Now that we have the ordered list of file paths and the steps we want to create
        // the mecha code symbol thinking
        let unique_file_paths = file_paths_and_steps
            .iter()
            .map(|(fs_file_path, _)| fs_file_path.to_owned())
            .collect::<HashSet<String>>();
        let file_paths_to_content = stream::iter(
            unique_file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let file_content = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await
                .ok();
            if let Some(file_content) = file_content {
                Some((
                    fs_file_path,
                    (file_content.language().to_owned(), file_content.contents()),
                ))
            } else {
                None
            }
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<String, (String, String)>>();

        let mecha_code_symbol_thinking = file_paths_and_steps
            .into_iter()
            .map(|(fs_file_path, steps)| {
                let (language, file_content) = file_paths_to_content
                    .get(&fs_file_path)
                    .map(|data| data.clone())
                    .unwrap_or(("".to_owned(), "".to_owned()));
                let range = Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0));
                let mecha_code_symbol_thinking = MechaCodeSymbolThinking::new(
                    fs_file_path.to_owned(),
                    vec![],
                    false,
                    fs_file_path.to_owned(),
                    Some(Snippet::new(
                        fs_file_path.to_owned(),
                        range.clone(),
                        fs_file_path.to_owned(),
                        file_content.to_owned(),
                        OutlineNodeContent::file_symbol(
                            fs_file_path.to_owned(),
                            range,
                            file_content.to_owned(),
                            fs_file_path,
                            language,
                        ),
                    )),
                    vec![],
                    Arc::new(self.clone()),
                );
                (mecha_code_symbol_thinking, steps)
            })
            .collect();
        Ok(mecha_code_symbol_thinking)
    }

    // TODO(skcd): We are not capturing the is_new symbols which might become
    // necessary later on
    pub async fn important_symbols(
        &self,
        important_symbols: &CodeSymbolImportantResponse,
        message_properties: SymbolEventMessageProperties,
        // returning the mecha code symbol along with the steps we want to take
        // for that symbol
    ) -> Result<Vec<(MechaCodeSymbolThinking, Vec<String>)>, SymbolError> {
        let symbols = important_symbols.ordered_symbols();
        // let ordered_symbols = important_symbols.ordered_symbols();
        // there can be overlaps between these, but for now its fine
        // let mut new_symbols: HashSet<String> = Default::default();
        // let mut symbols_to_visit: HashSet<String> = Default::default();
        // let mut final_code_snippets: HashMap<String, MechaCodeSymbolThinking> = Default::default();
        stream::iter(symbols.iter().map(|ordered_symbol| {
            (
                ordered_symbol.file_path().to_owned(),
                message_properties.clone(),
            )
        }))
        .for_each(|(file_path, message_properties)| async move {
            let file_open_response = self
                .file_open(file_path.to_owned(), message_properties)
                .await;
            if let Ok(file_open_response) = file_open_response {
                let _ = self
                    .force_add_document(
                        &file_path,
                        file_open_response.contents_ref(),
                        file_open_response.language(),
                    )
                    .await;
            } else {
                println!(
                    "tool_box::important_symbols::file_open_response_error({})",
                    file_path
                );
            }
        })
        .await;

        let mut bounding_symbol_to_instruction: HashMap<
            OutlineNodeContent,
            Vec<(usize, &CodeSymbolWithSteps)>,
        > = Default::default();
        let mut unbounded_symbols: Vec<(usize, &CodeSymbolWithSteps)> = Default::default();
        for (idx, symbol) in symbols.iter().enumerate() {
            let file_path = symbol.file_path();
            let symbol_name = symbol.code_symbol();
            let outline_nodes = self.symbol_broker.get_symbols_outline(file_path).await;
            if let Some(outline_nodes) = outline_nodes {
                let mut bounding_symbols =
                    self.grab_bounding_symbol_for_symbol(outline_nodes, symbol_name);
                if bounding_symbols.is_empty() {
                    // well this is weird, we have not outline nodes here
                    unbounded_symbols.push((idx, symbol));
                } else {
                    let outline_node = bounding_symbols.remove(0);
                    if bounding_symbol_to_instruction.contains_key(&outline_node) {
                        let contained_sub_symbols = bounding_symbol_to_instruction
                            .get_mut(&outline_node)
                            .expect("contains_key to work");
                        contained_sub_symbols.push((idx, symbol));
                    } else {
                        bounding_symbol_to_instruction.insert(outline_node, vec![(idx, symbol)]);
                    }
                }
            }
        }

        // We have categorised the sub-symbols now to belong with their bounding symbols
        // and for the sub-symbols which are unbounded, now we can create the final
        // mecha_code_symbol_thinking
        let mut mecha_code_symbols = vec![];
        for (outline_node, order_vec) in bounding_symbol_to_instruction.into_iter() {
            // code_symbol_with_steps.into_iter().map(|code_symbol_with_step| {
            //     let code_symbol = code_symbol_with_step.code_symbol().to_owned();
            //     let instructions = code_symbol_with_step.steps().to_vec();
            // }).collect::<Vec<_>>();
            let mecha_code_symbol_thinking = MechaCodeSymbolThinking::new(
                outline_node.name().to_owned(),
                vec![],
                false,
                outline_node.fs_file_path().to_owned(),
                Some(Snippet::new(
                    outline_node.name().to_owned(),
                    outline_node.range().clone(),
                    outline_node.fs_file_path().to_owned(),
                    outline_node.content().to_owned(),
                    outline_node.clone(),
                )),
                vec![],
                Arc::new(self.clone()),
            );
            let mut ordered_values = order_vec
                .into_iter()
                .map(|(idx, code_symbol_with_steps)| (idx, code_symbol_with_steps.steps().to_vec()))
                .collect::<Vec<_>>();
            // sort by the increasing values of orderes
            ordered_values.sort();
            if ordered_values.is_empty() {
                continue;
            } else {
                mecha_code_symbols.push((ordered_values.remove(0), mecha_code_symbol_thinking));
            }
        }

        // Now for all the symbols which are new or unbounded by any other symbol right now
        // we need to also add them inside properly
        println!(
            "tool_box::important_symbols::unbounded_symbols::({})::len({})",
            unbounded_symbols
                .iter()
                .map(|(_, symbol)| symbol.code_symbol().to_owned())
                .collect::<Vec<_>>()
                .join(","),
            unbounded_symbols.len()
        );
        unbounded_symbols
            .iter()
            .for_each(|(ordered_value, code_symbol_with_steps)| {
                mecha_code_symbols.push((
                    (*ordered_value, code_symbol_with_steps.steps().to_vec()),
                    MechaCodeSymbolThinking::new(
                        code_symbol_with_steps.code_symbol().to_owned(),
                        code_symbol_with_steps.steps().to_vec(),
                        true,
                        code_symbol_with_steps.file_path().to_owned(),
                        None,
                        vec![],
                        Arc::new(self.clone()),
                    ),
                ));
            });

        // Now we iterate over all the values in the array and then sort them via the first key
        mecha_code_symbols.sort_by_key(|(idx, _)| idx.clone());
        Ok(mecha_code_symbols
            .into_iter()
            .map(|((_, steps), symbol)| (symbol, steps))
            .collect())
    }

    async fn go_to_implementations_exact(
        &self,
        fs_file_path: &str,
        position: &Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToImplementationResponse, SymbolError> {
        let _ = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let request = ToolInput::SymbolImplementations(GoToImplementationRequest::new(
            fs_file_path.to_owned(),
            position.clone(),
            message_properties.editor_url().to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_go_to_implementation()
            .ok_or(SymbolError::WrongToolOutput)
    }

    // TODO(skcd): Implementation is returning the macros on top of the symbols
    // which is pretty bad, because we do not capture it properly in our logic
    pub async fn go_to_implementation(
        &self,
        fs_file_path: &str,
        symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToImplementationResponse, SymbolError> {
        let outline_nodes = self
            .get_outline_nodes_from_editor(fs_file_path, message_properties.clone())
            .await;
        // Now we need to find the outline node which corresponds to the symbol we are
        // interested in and use that as the position to ask for the implementations
        let position_from_outline_node = outline_nodes
            .map(|outline_nodes| {
                outline_nodes
                    .into_iter()
                    .find(|outline_node| outline_node.name() == symbol_name)
                    .map(|outline_node| outline_node.identifier_range().end_position())
            })
            .flatten();
        if let Some(position) = position_from_outline_node {
            let request = ToolInput::SymbolImplementations(GoToImplementationRequest::new(
                fs_file_path.to_owned(),
                position,
                message_properties.editor_url().to_owned(),
            ));
            self.tools
                .invoke(request)
                .await
                .map_err(|e| SymbolError::ToolError(e))?
                .get_go_to_implementation()
                .ok_or(SymbolError::WrongToolOutput)
        } else {
            Err(SymbolError::ToolError(ToolError::SymbolNotFound(
                symbol_name.to_owned(),
            )))
        }
    }

    /// Grabs the symbol content and the range in the file which it is present in
    async fn grab_symbol_content_from_definition(
        &self,
        symbol_name: &str,
        definition: GoToDefinitionResponse,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Snippet, SymbolError> {
        // here we first try to open the file
        // and then read the symbols from it nad then parse
        // it out properly
        // since its very much possible that we get multiple definitions over here
        // we have to figure out how to pick the best one over here
        // TODO(skcd): This will break if we are unable to get definitions properly
        let mut definitions = definition.definitions();
        if definitions.is_empty() {
            return Err(SymbolError::SymbolNotFound);
        }
        let definition = definitions.remove(0);
        let _ = self
            .file_open(definition.file_path().to_owned(), message_properties)
            .await?;
        // grab the symbols from the file
        // but we can also try getting it from the symbol broker
        // because we are going to open a file and send a signal to the signal broker
        // let symbols = self
        //     .editor_parsing
        //     .for_file_path(definition.file_path())
        //     .ok_or(ToolError::NotSupportedLanguage)?
        //     .generate_file_outline_str(file_content.contents().as_bytes());
        let symbols = self
            .symbol_broker
            .get_symbols_outline(definition.file_path())
            .await;
        if let Some(symbols) = symbols {
            let symbols = self.grab_symbols_from_outline(symbols, symbol_name);
            // find the first symbol and grab back its content
            symbols
                .into_iter()
                .find(|symbol| symbol.name() == symbol_name)
                .map(|symbol| {
                    Snippet::new(
                        symbol.name().to_owned(),
                        symbol.range().clone(),
                        definition.file_path().to_owned(),
                        symbol.content().to_owned(),
                        symbol,
                    )
                })
                .ok_or(SymbolError::ToolError(ToolError::SymbolNotFound(
                    symbol_name.to_owned(),
                )))
        } else {
            Err(SymbolError::ToolError(ToolError::SymbolNotFound(
                symbol_name.to_owned(),
            )))
        }
    }

    /// Grabs the bounding symbol for a given sub-symbol or symbol
    /// if we are looking for a function in a class, we get the class back
    /// if its the class itself we get the class back
    /// if its a function itself outside of the class, then we get that back
    fn grab_bounding_symbol_for_symbol(
        &self,
        outline_nodes: Vec<OutlineNode>,
        symbol_name: &str,
    ) -> Vec<OutlineNodeContent> {
        outline_nodes
            .into_iter()
            .filter_map(|node| {
                if node.is_class() {
                    if node.content().name() == symbol_name {
                        Some(vec![node.content().clone()])
                    } else {
                        if node
                            .children()
                            .iter()
                            .any(|node| node.name() == symbol_name)
                        {
                            Some(vec![node.content().clone()])
                        } else {
                            None
                        }
                    }
                } else {
                    if node.content().name() == symbol_name {
                        Some(vec![node.content().clone()])
                    } else {
                        None
                    }
                }
            })
            .flatten()
            .collect::<Vec<_>>()
    }

    fn grab_symbols_from_outline(
        &self,
        outline_nodes: Vec<OutlineNode>,
        symbol_name: &str,
    ) -> Vec<OutlineNodeContent> {
        outline_nodes
            .into_iter()
            .filter_map(|node| {
                if node.is_class() || node.is_file() {
                    // it might either be the class itself
                    // or a function inside it so we can check for it
                    // properly here
                    if node.content().name() == symbol_name {
                        Some(vec![node.content().clone()])
                    } else {
                        Some(
                            node.children()
                                .into_iter()
                                .filter(|node| node.name() == symbol_name)
                                .map(|node| node.clone())
                                .collect::<Vec<_>>(),
                        )
                    }
                } else {
                    // we can just compare the node directly
                    // without looking at the children at this stage
                    if node.content().name() == symbol_name {
                        Some(vec![node.content().clone()])
                    } else {
                        None
                    }
                }
            })
            .flatten()
            .collect::<Vec<_>>()
    }

    /// The outline node contains details about the inner constructs which
    /// might be present in the snippet
    ///
    /// Since a snippet should belong 1-1 with an outline node, we also do a
    /// huristic check to figure out if the symbol name and the outline node
    /// matches up and is the closest to the snippet we are looking at
    pub async fn get_outline_node_from_snippet(
        &self,
        snippet: &Snippet,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNode, SymbolError> {
        let fs_file_path = snippet.file_path();
        let file_open_request = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let _ = self
            .force_add_document(
                fs_file_path,
                file_open_request.contents_ref(),
                file_open_request.language(),
            )
            .await;
        let symbols_outline = self
            .symbol_broker
            .get_symbols_outline(&fs_file_path)
            .await
            .ok_or(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))?
            .into_iter()
            .filter(|outline_node| outline_node.name() == snippet.symbol_name())
            .collect::<Vec<_>>();

        // we want to find the closest outline node to this snippet over here
        // since we can have multiple implementations with the same symbol name
        let mut outline_nodes_with_distance = symbols_outline
            .into_iter()
            .map(|outline_node| {
                let distance: i64 = if outline_node
                    .range()
                    .intersects_without_byte(snippet.range())
                    || snippet
                        .range()
                        .intersects_without_byte(outline_node.range())
                {
                    0
                } else {
                    outline_node.range().minimal_line_distance(snippet.range())
                };
                (distance, outline_node)
            })
            .collect::<Vec<_>>();

        // Now sort it based on the distance in ascending order
        outline_nodes_with_distance.sort_by_key(|outline_node| outline_node.0);
        if outline_nodes_with_distance.is_empty() {
            Err(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))
        } else {
            Ok(outline_nodes_with_distance.remove(0).1)
        }
    }

    /// Grabs the outline node which contains this range in the current file
    pub async fn get_outline_node_for_range(
        &self,
        range: &Range,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OutlineNode, SymbolError> {
        let file_open_request = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let _ = self
            .force_add_document(
                fs_file_path,
                file_open_request.contents_ref(),
                file_open_request.language(),
            )
            .await;
        let symbols_outline = self
            .symbol_broker
            .get_symbols_outline(fs_file_path)
            .await
            .ok_or(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))?
            .into_iter()
            .filter(|outline_node| outline_node.range().contains_check_line(range))
            .collect::<Vec<_>>();
        let mut outline_nodes_with_distance = symbols_outline
            .into_iter()
            .map(|outline_node| {
                let distance: i64 = if outline_node.range().intersects_without_byte(range)
                    || range.intersects_without_byte(outline_node.range())
                {
                    0
                } else {
                    outline_node.range().minimal_line_distance(range)
                };
                (distance, outline_node)
            })
            .collect::<Vec<_>>();
        // Now sort it based on the distance in ascending order
        outline_nodes_with_distance.sort_by_key(|outline_node| outline_node.0);
        if outline_nodes_with_distance.is_empty() {
            Err(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))
        } else {
            Ok(outline_nodes_with_distance.remove(0).1)
        }
    }

    /// Generates the symbol identifiers from the user context if possible:
    /// The generate goal here is be deterministic and quick (sub-millisecond
    /// ttft )
    /// If the user already knows and is smart to do the selection over complete
    /// ranges of sub-symbol (we will also handle cases where its inside a particular
    /// symbol or mentioned etc)
    pub async fn grab_symbols_from_user_context(
        &self,
        query: String,
        user_context: UserContext,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<CodeSymbolImportantResponse, SymbolError> {
        // we have 3 types of variables over here:
        // class, selection and file
        // file will be handled at the go-to-xml stage anyways
        // class and selections are more interesting, we can try and detect
        // smartly over here if the selection or the class is overlapping with
        // some symbol and if it is contained in some symbol completely
        // these are easy trigger points to start the agents
        let symbols = user_context
            .variables
            .iter()
            .filter(|variable| variable.is_code_symbol())
            .collect::<Vec<_>>();
        let selections = user_context
            .variables
            .iter()
            .filter(|variable| variable.is_selection())
            .collect::<Vec<_>>();
        let _ = symbols
            .iter()
            .map(|symbol| symbol.fs_file_path.to_owned())
            .chain(
                selections
                    .iter()
                    .map(|symbol| symbol.fs_file_path.to_owned()),
            )
            .collect::<HashSet<_>>();

        let mut outline_nodes_from_symbols = vec![];

        for symbol in symbols.iter() {
            let outline_node = self
                .get_outline_node_for_range(
                    &Range::new(symbol.start_position.clone(), symbol.end_position.clone()),
                    &symbol.fs_file_path,
                    message_properties.clone(),
                )
                .await;
            if let Ok(outline_node) = outline_node {
                outline_nodes_from_symbols.push(outline_node);
            }
        }
        let mut outline_node_from_symbols = vec![];
        for symbol in symbols.iter() {
            let outline_node = self
                .get_outline_node_for_range(
                    &Range::new(symbol.start_position.clone(), symbol.end_position.clone()),
                    &symbol.fs_file_path,
                    message_properties.clone(),
                )
                .await;
            if let Ok(outline_node) = outline_node {
                outline_node_from_symbols.push(outline_node);
            }
        }

        let mut outline_node_from_selections = vec![];

        for selection in selections.iter() {
            let outline_node = self
                .get_outline_node_for_range(
                    &Range::new(
                        selection.start_position.clone(),
                        selection.end_position.clone(),
                    ),
                    &selection.fs_file_path,
                    message_properties.clone(),
                )
                .await;
            if let Ok(outline_node) = outline_node {
                outline_node_from_selections.push(outline_node);
            }
        }

        // Now we de-duplicate the outline nodes using the symbol_name and fs_file_path
        let symbol_identifiers = outline_node_from_symbols
            .into_iter()
            .chain(outline_node_from_selections)
            .map(|outline_node| {
                SymbolIdentifier::with_file_path(outline_node.name(), outline_node.fs_file_path())
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let symbols = symbol_identifiers
            .iter()
            .filter_map(|symbol_identifier| {
                let symbol_name = symbol_identifier.symbol_name();
                let fs_file_path = symbol_identifier.fs_file_path();
                match fs_file_path {
                    Some(fs_file_path) => Some(CodeSymbolWithThinking::new(
                        symbol_name.to_owned(),
                        query.to_owned(),
                        fs_file_path,
                    )),
                    None => None,
                }
            })
            .collect::<Vec<_>>();
        let ordered_symbols = symbol_identifiers
            .iter()
            .filter_map(|symbol_identifier| {
                let symbol_name = symbol_identifier.symbol_name();
                let fs_file_path = symbol_identifier.fs_file_path();
                match fs_file_path {
                    Some(fs_file_path) => Some(CodeSymbolWithSteps::new(
                        symbol_name.to_owned(),
                        vec![query.to_owned()],
                        false,
                        fs_file_path,
                    )),
                    None => None,
                }
            })
            .collect::<Vec<_>>();
        if ordered_symbols.is_empty() {
            Err(SymbolError::UserContextEmpty)
        } else {
            Ok(CodeSymbolImportantResponse::new(symbols, ordered_symbols))
        }
    }

    /// Grabs the hoverable nodes which are present in the file, especially useful
    /// when figuring out which node to use for cmd+click
    fn get_hoverable_nodes(
        &self,
        source_code: &str,
        file_path: &str,
    ) -> Result<Vec<Range>, SymbolError> {
        let language_parsing = self.editor_parsing.for_file_path(file_path);
        if let None = language_parsing {
            return Err(SymbolError::FileTypeNotSupported(file_path.to_owned()));
        }
        let language_parsing = language_parsing.expect("if let None to hold");
        Ok(language_parsing.hoverable_nodes(source_code.as_bytes()))
    }

    /// Grabs the repo wide `git-diff` which can be used
    /// as input for the agentic trigger
    ///
    /// This is to warmup the cache and set the context for the
    /// agent
    pub async fn get_git_diff(
        &self,
        root_directory: &str,
    ) -> Result<GitDiffClientResponse, SymbolError> {
        let tool_input = ToolInput::GitDiff(GitDiffClientRequest::new(
            root_directory.to_owned(),
            // file path is not required, we just need the
            // root_directory
            "".to_owned(),
            // we want the full view of the git diff changes
            false,
        ));
        let output = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_git_diff_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        // println!("tool_output::{:?}", output);
        Ok(output)
    }

    /// Gets the changed contents of the file using git-diff
    pub async fn get_file_changes(
        &self,
        root_directory: &str,
        fs_file_path: &str,
    ) -> Result<GitDiffClientResponse, SymbolError> {
        let tool_input = ToolInput::GitDiff(GitDiffClientRequest::new(
            root_directory.to_owned(),
            fs_file_path.to_owned(),
            // we want the full view of the git diff changes
            true,
        ));
        let output = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_git_diff_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        // println!("tool_output::{:?}", output);
        Ok(output)
    }

    /// Grabs the changed symbols present in a file:
    /// We get back the symbol identifier and the following
    /// information about it:
    /// - the previous content
    /// - the new content
    /// Some problems: This overwrites when the symbol names are the same, so we need
    /// to account for the range of the symbol as well, its not as straightforward as
    /// comparsing the parent symbol name
    /// imagine this case
    /// struct Something {}
    /// impl Something {}
    /// ^ for both of these we will have the same symbol-identifier ...
    /// but this works when changing the function of a symbol
    ///
    /// All file main nodes are returned, those with non-empty vecs have changes contained within.
    /// Original contents storeed in (_, String)
    pub async fn grab_changed_symbols_in_file_git(
        &self,
        root_directory: &str,
        fs_file_path: &str,
    ) -> Result<SymbolChangeSet, SymbolError> {
        let file_changes = self.get_file_changes(root_directory, fs_file_path).await?;
        self.get_symbol_change_set(
            fs_file_path,
            file_changes.old_version(),
            file_changes.new_version(),
        )
        .await
    }

    pub async fn get_symbol_change_set(
        &self,
        fs_file_path: &str,
        older_content: &str,
        new_content: &str,
    ) -> Result<SymbolChangeSet, SymbolError> {
        // Now we need to parse the new and old version of the files and get the changed
        // nodes which are present in them or which have been completely added or deleted
        // TODO(codestory): This is still wrong btw, the true meaning of a symbol-identifier
        // is where the symbol is really defined, not where its present in a file
        // remember that we can have impls and symbol definitions in different files
        // for now we are testing only on cases where the symbol is assumed to be defined
        // in the file
        let language_config = self.editor_parsing.for_file_path(fs_file_path);
        if language_config.is_none() {
            return Ok(SymbolChangeSet::default());
        }

        let language_config = language_config.expect("is_none to hold");
        let mut older_outline_nodes: HashMap<SymbolIdentifier, Vec<OutlineNode>> =
            Default::default();
        // this function needs to be using the editor instead of the tree-sitter
        // call over here
        language_config
            .generate_outline_fresh(older_content.as_bytes(), fs_file_path)
            .into_iter()
            .for_each(|outline_node| {
                let symbol_identifier =
                    SymbolIdentifier::with_file_path(outline_node.name(), fs_file_path);
                if let Some(outline_nodes_older) = older_outline_nodes.get_mut(&symbol_identifier) {
                    outline_nodes_older.push(outline_node);
                } else {
                    older_outline_nodes.insert(symbol_identifier, vec![outline_node]);
                }
            });
        let mut newer_outline_nodes: HashMap<SymbolIdentifier, Vec<OutlineNode>> =
            Default::default();
        language_config
            .generate_outline_fresh(new_content.as_bytes(), fs_file_path)
            .into_iter()
            .for_each(|outline_node| {
                let symbol_identifier =
                    SymbolIdentifier::with_file_path(outline_node.name(), fs_file_path);
                if let Some(new_outline_nodes) = newer_outline_nodes.get_mut(&symbol_identifier) {
                    new_outline_nodes.push(outline_node);
                } else {
                    newer_outline_nodes.insert(symbol_identifier, vec![outline_node]);
                }
            });

        // instead of figuring out the delta, use a simple huristic to map the outline nodes
        // together based on their properties
        let changed_nodes = newer_outline_nodes
            .into_iter()
            .filter_map(|(symbol_identifier, mut new_outline_nodes)| {
                match older_outline_nodes.get_mut(&symbol_identifier) {
                    Some(older_outline_nodes) => {
                        // if there is a single new node, and if we have many older
                        // nodes or something, then its a case of code-deletion
                        // in this case we want to grab the outline node from older
                        // which is either a class or function node
                        if new_outline_nodes.len() == 1 {
                            if older_outline_nodes.len() == 1 {
                                Some(vec![(
                                    symbol_identifier,
                                    Some(older_outline_nodes.remove(0)),
                                    Some(new_outline_nodes.remove(0)),
                                )])
                            } else {
                                if older_outline_nodes.is_empty() {
                                    return Some(vec![(
                                        symbol_identifier,
                                        None,
                                        Some(new_outline_nodes[0].clone()),
                                    )]);
                                }
                                // if will be either of the following:
                                // is_func and is_class_delcaration are special one
                                // if this is a class then we are in js/py land
                                if new_outline_nodes[0].is_class_definition()
                                    || new_outline_nodes[0].is_function()
                                {
                                    older_outline_nodes
                                        .into_iter()
                                        .find(|outline_node| {
                                            outline_node.is_class_definition()
                                                || outline_node.is_function()
                                        })
                                        .map(|older_outline_node| {
                                            vec![(
                                                symbol_identifier,
                                                Some(older_outline_node.clone()),
                                                Some(new_outline_nodes[0].clone()),
                                            )]
                                        })
                                } else {
                                    // otherwise we are in js/py land so also grab the first
                                    // entry and return it over here
                                    Some(vec![(
                                        symbol_identifier,
                                        Some(older_outline_nodes.remove(0)),
                                        Some(new_outline_nodes.remove(0)),
                                    )])
                                }
                            }
                        } else {
                            // if we have multiple nodes, then we are in rust/golang land
                            // in this case we can easily search for the symbols and make that work
                            // we want to find the node which might be part of the implementation node
                            // we can have multiple implementations in the same file but separated by the trait
                            // so that can act as our key, and for the ones which are the same that should just work
                            let class_definition_new = new_outline_nodes
                                .iter()
                                .find(|outline_node| outline_node.is_class_definition());
                            let class_definition_old = older_outline_nodes
                                .iter()
                                .find(|outline_node| outline_node.is_class_definition());
                            let mut entries = vec![];

                            // if we have a class definition on the new, then we should
                            // get it
                            if let Some(class_definition_new) = class_definition_new {
                                entries.push((
                                    symbol_identifier.clone(),
                                    class_definition_old.map(|data| data.clone()),
                                    Some(class_definition_new.clone()),
                                ));
                            }

                            // now we might have entries over here which are just class types which we want to match
                            // but matching them will be hard, but what we can do instead is match them on trait implementations
                            let new_outline_nodes_implementations = new_outline_nodes
                                .iter()
                                .filter(|outline_node| outline_node.is_class())
                                .collect::<Vec<_>>();
                            let old_outline_nodes_implementations = older_outline_nodes
                                .iter()
                                .filter(|outline_node| outline_node.is_class())
                                .collect::<Vec<_>>();

                            // now we try to match the outline nodes together
                            let changed_nodes = new_outline_nodes_implementations
                                .into_iter()
                                .map(|new_outline_node| {
                                    let trait_implementation =
                                        new_outline_node.content().has_trait_implementation();
                                    let matching_older_nodes = old_outline_nodes_implementations
                                        .iter()
                                        .filter(|outline_node| {
                                            &outline_node.content().has_trait_implementation()
                                                == &trait_implementation
                                        })
                                        .collect::<Vec<_>>();

                                    if matching_older_nodes.is_empty() {
                                        vec![(
                                            symbol_identifier.clone(),
                                            None,
                                            Some(new_outline_node.clone()),
                                        )]
                                    } else if matching_older_nodes.len() == 1 {
                                        vec![(
                                            symbol_identifier.clone(),
                                            Some((*matching_older_nodes[0]).clone()),
                                            Some(new_outline_node.clone()),
                                        )]
                                    } else {
                                        // now we have multiple outline nodes, which makes
                                        // it hard to find where the function overlaps with the
                                        // outline node, an easy test for this is to literlly
                                        // look at the children nodes and see which ones are matching
                                        // up but this is a hard problem cause the functions can move
                                        // around in the outline nodes
                                        // for now we take the easy route and just compare against the
                                        // first older outline node
                                        vec![(
                                            symbol_identifier.clone(),
                                            matching_older_nodes
                                                .get(0)
                                                .map(|outline_node| (**outline_node).clone()),
                                            Some(new_outline_node.clone()),
                                        )]
                                    }
                                })
                                .flatten()
                                .collect::<Vec<_>>();
                            entries.extend(changed_nodes.into_iter());
                            Some(entries)
                        }
                    }
                    None => {
                        // just flat out all the outline nodes and send them over for checking what to do
                        // with the data
                        Some(
                            new_outline_nodes
                                .into_iter()
                                .map(|outline_node| {
                                    (symbol_identifier.clone(), None, Some(outline_node))
                                })
                                .collect::<Vec<_>>(),
                        )
                    }
                }
            })
            .flatten()
            .collect::<Vec<_>>();

        // right now we just focus on the function symbols, we will handle the class
        // memeber symbols differently
        // Vec<(String, Vec<(SymbolToEdit, String)>)>
        // we get back the a vec of parent_symbol_names, the children symbols which need to be edited
        // in case of class it will be the same symbol and the original content
        let changed_nodes_followups = changed_nodes
            .into_iter()
            .filter_map(|changed_node| {
                let symbol_identifier = changed_node.0;
                let original_outline_node = changed_node.1;
                let changed_outline_node = changed_node.2;
                println!("symbol_identifier::({})::original_present({})::changed_present({})", symbol_identifier.symbol_name(), original_outline_node.is_some(), changed_outline_node.is_some());
                let symbol_edits = match (original_outline_node, changed_outline_node) {
                    (None, None) => {
                        // nothing to do, both sides are empty
                        None
                    }
                    (Some(original_node), Some(current_node)) => {
                        // hell yeah, lets gooo
                        if current_node.is_class() {
                            println!("current_node::is_class({})", current_node.name());
                            // in this case we have to send the functions inside for reference checks
                            // cause one of them has changed
                            // look at all the children inside and get them instead over here
                            let symbol_edits_which_happened = current_node
                                .children()
                                .into_iter()
                                .filter_map(|children_node| {
                                    let original_child_present = original_node.children().into_iter().find(|original_child| {
                                        original_child.name() == children_node.name()
                                    });
                                    match original_child_present {
                                        Some(original_child) => {
                                            if original_child.content() == children_node.content() {
                                                None
                                            } else {
                                                // create a symbol to edit request here
                                                let original_content = original_child.content();
                                                let current_content = children_node.content();
                                                Some((SymbolToEdit::new(
                                                    children_node.name().to_owned(),
                                                    children_node.range().clone(),
                                                    fs_file_path.to_owned(),
                                                    vec![format!(
                                                        r#"The following changes were made:
{fs_file_path}
<<<<<<<<
{original_content}
=====
{current_content}
>>>>>>>>"#
                                                    )
                                                    .to_owned()],
                                                    false,
                                                    false,
                                                    true,
                                                    "Edits have happened, you have to understand the reason".to_owned(),
                                                    None,
                                                    true,
                                                    None,
                                                    true, // should we disable followups and correctness check
                                                    None,
                                                    vec![],
                                                    None,
                                                ), original_content.to_owned(), current_content.to_owned()))
                                            }
                                        }
                                        None => {
                                            // this is a new child, no need to trigger followups
                                            None
                                        }
                                    }
                                })
                                .collect::<Vec<_>>();
                            Some(symbol_edits_which_happened)
                        } else {
                            println!("is_class_definition::({})", current_node.name());
                            // in this case, we have to send for reference check the whole class
                            let original_content = original_node.content().content();
                            let current_content = current_node.content().content();
                            if original_content != current_content {
                                Some(vec![(SymbolToEdit::new(
                                    symbol_identifier.symbol_name().to_owned(),
                                    current_node.range().clone(),
                                    fs_file_path.to_owned(),
                                    vec![format!(
                                        r#"The following changes were made:
    {fs_file_path}
    <<<<<<<<
    {original_content}
    =====
    {current_content}
    >>>>>>>>"#
                                    )
                                    .to_owned()],
                                    false,
                                    false,
                                    true,
                                    "Edits have happened, you have to understand the reason".to_owned(),
                                    None,
                                    true,
                                    None,
                                    true, // should we disable followups and correctness check
                                    None,
                                    vec![],
                                    None,
                                ), original_content.to_owned(), current_content.to_owned())])
                            } else {
                                None
                            }
                        }
                    }
                    (None, Some(_current_node)) => {
                        // new symbol which didn't exist before, safe to bet we don't
                        // have followups for it
                        None
                    }
                    (Some(_original_node), None) => {
                        // code deletion, this is another can of worms
                        None
                    }
                };
                symbol_edits.map(|edits| (symbol_identifier, edits))
            })
            .collect::<Vec<_>>();

        let symbol_change_set = changed_nodes_followups
            .into_iter()
            .map(|(symbol_identifier, changes)| SymbolChanges::new(symbol_identifier, changes))
            .collect::<Vec<_>>();

        Ok(SymbolChangeSet::new(symbol_change_set))
    }

    /// Gets a unique identifier or symbol edit request given the range which is selected
    /// and where we want to edit it
    pub async fn symbols_to_anchor(
        &self,
        user_context: &UserContext,
        message_properties: SymbolEventMessageProperties,
        // we return a vector which maps the parent symbol identifier to the children symbols
        // which require editing over here
    ) -> Result<Vec<AnchoredSymbol>, SymbolError> {
        let selection_variable = user_context.variables.iter().find(|variable| {
            variable.is_selection()
                && !(variable.start_position.line() == 0 && variable.end_position.line() == 0)
        });
        if selection_variable.is_none() {
            return Ok(vec![]);
        }
        let selection_variable = selection_variable.expect("is_none to hold above");
        let selection_range = Range::new(
            selection_variable.start_position,
            selection_variable.end_position,
        );
        let fs_file_path = selection_variable.fs_file_path.to_owned();
        let language_config = self.editor_parsing.for_file_path(&fs_file_path);
        if language_config.is_none() {
            println!("tool_box::language_config::is_none");
            return Ok(vec![]);
        }
        let language_config = language_config.expect("is_none to hold");
        let outline_nodes = self
            .get_outline_nodes_from_editor(&fs_file_path, message_properties.clone())
            .await
            .unwrap_or_default();
        println!(
            "tool_box::symbols_to_anchor::file_path({})::outline_nodes_len({})",
            &fs_file_path,
            outline_nodes.len()
        );

        // now I have the outline nodes, I want to see which of them intersect with the range we are interested in
        let mut intersecting_outline_nodes = outline_nodes
            .into_iter()
            .filter(|outline_node| {
                outline_node
                    .range()
                    .intersects_with_another_range(&selection_range)
                    || outline_node.is_file()
            })
            .collect::<Vec<_>>();

        // if we have multiple outline nodes over here and one of them is a file then we should discard the file
        // one, since we have a more precise selection anyways
        let intersecting_outline_nodes_len = intersecting_outline_nodes.len();
        if intersecting_outline_nodes_len > 1 {
            intersecting_outline_nodes = intersecting_outline_nodes
                .into_iter()
                .filter(|outline_node| !outline_node.is_file())
                .collect::<Vec<_>>();
        }

        let anchored_nodes: Vec<AnchoredSymbol> = intersecting_outline_nodes
            .into_iter()
            .map(|outline_node| {
                println!(
                    "tool_box::symbols_to_anchor::({})::({:?})",
                    outline_node.name(),
                    outline_node.outline_node_type()
                );
                if outline_node.is_function()
                    || outline_node.is_class_definition()
                    || outline_node.is_file()
                    || (language_config.is_single_implementation_block_language())
                {
                    // then its a single unit of work, so its a bit easier
                    AnchoredSymbol::new(
                        SymbolIdentifier::with_file_path(
                            outline_node.name(),
                            outline_node.fs_file_path(),
                        ),
                        outline_node.content().content(),
                        &[outline_node.name().to_owned()],
                        outline_node.range().clone(),
                    )
                } else {
                    // first check if we are completly covering the outline node in question
                    // if we are, then we should anchor on just that
                    if outline_node.range().equals_line_range(&selection_range) {
                        AnchoredSymbol::new(
                            SymbolIdentifier::with_file_path(
                                outline_node.name(),
                                outline_node.fs_file_path(),
                            ),
                            outline_node.content().content(),
                            &[outline_node.name().to_owned()],
                            outline_node.range().clone(),
                        )
                    } else {
                        // we need to look at the children node and figure out where we are going to be making the edits
                        let children_nodes = outline_node
                            .children()
                            .into_iter()
                            .filter(|children| {
                                children
                                    .range()
                                    .intersects_with_another_range(&selection_range)
                            })
                            .map(|child_outline_node| child_outline_node.name().to_owned())
                            .collect::<Vec<_>>();
                        AnchoredSymbol::new(
                            SymbolIdentifier::with_file_path(
                                outline_node.name(),
                                outline_node.fs_file_path(),
                            ),
                            outline_node.content().content(),
                            &children_nodes,
                            outline_node.range().clone(),
                        )
                    }
                }
            })
            .collect::<Vec<_>>();

        // Now that we have the intersecting outline nodes we can create our own request types on top of this
        Ok(anchored_nodes)
    }

    /// Uses the anchored symbols to grab the symbols which require editing
    pub async fn symbol_to_edit_request(
        &self,
        anchored_symbols: &[AnchoredSymbol],
        user_query: &str,
        user_provided_context: Option<String>,
        recent_diff_changes: DiffRecentChanges,
        previous_user_queries: Vec<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<SymbolToEditRequest>, SymbolError> {
        println!(
            "tool_box::symbol_to_edit_request::anchored_symbols::({:?})",
            anchored_symbols
        );
        let mut symbol_to_edit_request = vec![];
        for anchored_symbol in anchored_symbols.into_iter() {
            let symbol_identifier = anchored_symbol.identifier().to_owned();
            let child_symbols = anchored_symbol.sub_symbol_names().to_vec();
            println!(
                "tool_box::symbol_to_edit_request::anchored_symbos::child_symbols::({})",
                child_symbols.to_vec().join(",")
            );
            let symbol_range = anchored_symbol.possible_range();
            // if no file path is present we should keep moving forward
            let fs_file_path = symbol_identifier.fs_file_path();
            if fs_file_path.is_none() {
                continue;
            }
            let fs_file_path = fs_file_path.expect("is_none to hold");
            let language_config = self.editor_parsing.for_file_path(&fs_file_path);
            if language_config.is_none() {
                continue;
            }
            let language_config = language_config.expect("is_none to hold");
            let outline_nodes = self
                .get_outline_nodes_from_editor(&fs_file_path, message_properties.clone())
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|outline_node| outline_node.name() == symbol_identifier.symbol_name())
                .collect::<Vec<_>>();

            // if the child symbol is literally just the symbol identifier
            if child_symbols.len() == 1
                && child_symbols.get(0) == Some(&symbol_identifier.symbol_name().to_owned())
            {
                println!(
                    "tool_box::symbol_to_edit_request::full_symbol_edit::symbol_name({})",
                    symbol_identifier.symbol_name()
                );
                // This implies its the symbol itself so we can easily send the edit request quickly
                // over here, since we anyways do full symbol edits
                symbol_to_edit_request.push(SymbolToEditRequest::new(
                    vec![SymbolToEdit::new(
                        symbol_identifier.symbol_name().to_owned(),
                        symbol_range.clone(),
                        fs_file_path.to_owned(),
                        vec![user_query.to_owned()].to_owned(),
                        false,
                        false,
                        true,
                        user_query.to_owned(),
                        None,
                        false, // grab definitions
                        user_provided_context.to_owned(),
                        false, // disable followups - keep false to enable followups
                        Some(recent_diff_changes.clone()),
                        previous_user_queries.to_vec(),
                        None,
                    )],
                    symbol_identifier.clone(),
                    vec![],
                ));
                continue;
            }

            // Now we loop over the children and try to find the matching entries
            // for each one of them along with the range
            // the child might share the same name as the outline-node in which case
            // its a function or a class definition
            // TODO(skcd):
            // whats broken over here?
            // we have a possible range location on the anchor symbol
            // we can use that to figure out where the anchor symbol is present
            // the assumption is that no edits would be going on at the time so we can maybe
            // directly compare the ranges (altho thats wrong cause we can have race conditions)
            let symbols_to_edit = child_symbols
                .into_iter()
                .filter_map(|child_symbol| {
                    if child_symbol == symbol_identifier.symbol_name() {
                        // this is a spcial case where the child symbol is of the same name as the symbol name
                        // representing a class definition or function
                        let possible_outline_node = outline_nodes.iter().find(|outline_node| {
                            outline_node.is_class_definition()
                                || outline_node.is_function()
                                || outline_node.is_file()
                                // if we are in python or js land, then its a single implementation block language
                                || language_config.is_single_implementation_block_language()
                        });
                        if let Some(outline_node) = possible_outline_node {
                            Some(SymbolToEdit::new(
                                child_symbol.to_owned(),
                                outline_node.range().clone(),
                                outline_node.fs_file_path().to_owned(),
                                vec![user_query.to_owned()],
                                false,
                                false,
                                true, // we want to try out the search and replace style editing over here
                                user_query.to_owned(),
                                None,
                                // since these are quick edits we do not want to spend
                                // time gathering context
                                false,
                                user_provided_context.clone(),
                                true, // should we disable followups and correctness check
                                None,
                                vec![],
                                None,
                            ))
                        } else {
                            None
                        }
                    } else {
                        // iterate over the children of the outline nodes and try to find the matching node for us
                        outline_nodes.iter().find_map(|outline_node| {
                            outline_node.children().into_iter().find_map(|child_node| {
                                if child_node.name() == &child_symbol {
                                    Some(SymbolToEdit::new(
                                        child_symbol.to_owned(),
                                        child_node.range().clone(),
                                        child_node.fs_file_path().to_owned(),
                                        vec![user_query.to_owned()],
                                        false,
                                        false,
                                        true, // we want to try out the search and replace style editing over here
                                        user_query.to_owned(),
                                        None,
                                        // since these are quick edits we do not
                                        // want to spend time gathering context
                                        false,
                                        user_provided_context.clone(),
                                        true, // should we disable followups and correctness check
                                        None,
                                        vec![],
                                        None,
                                    ))
                                } else {
                                    None
                                }
                            })
                        })
                    }
                })
                .collect::<Vec<_>>();
            println!(
                "tool_box::symbol_to_edit_request::symbols_len({})",
                symbols_to_edit.len()
            );
            symbol_to_edit_request.push(SymbolToEditRequest::new(
                symbols_to_edit,
                symbol_identifier,
                vec![],
            ))
        }
        Ok(symbol_to_edit_request)
    }

    /// Grabs all the files which are imported by the current file we are interested
    /// in
    ///
    /// Use this for getting a sense of code graph and where we are loacted in the code
    async fn get_imported_files_outline_nodes(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
        // returns the Vec<files_on_the_local_graph>, Vec<outline_nodes_symbol_filter>)
        // this allows us to get the local graph of the files which are related
        // to the current file and the outline nodes which we can include
    ) -> Result<Vec<OutlineNode>, SymbolError> {
        let mut clickable_import_range = vec![];
        // the name of the outline nodes which we can include when we are looking
        // at the imports, this prevents extra context or nodes from slipping in
        // and only includes the local code graph
        let mut outline_node_name_filter = vec![];
        self.find_import_nodes(fs_file_path, message_properties.clone())
            .await?
            .into_iter()
            .for_each(|(import_node_name, range)| {
                clickable_import_range.push(range);
                outline_node_name_filter.push(import_node_name)
            });
        // Now we execute a go-to-definition request on all the imports
        let definition_files = stream::iter(
            clickable_import_range
                .to_vec()
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(range, message_properties)| async move {
            let go_to_definition = self
                .go_to_definition(fs_file_path, range.end_position(), message_properties)
                .await;
            match go_to_definition {
                Ok(go_to_definition) => go_to_definition
                    .definitions()
                    .into_iter()
                    .map(|definition| definition.file_path().to_owned())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    println!("get_imported_files::error({:?})", e);
                    vec![]
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        let implementation_files = stream::iter(
            clickable_import_range
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(range, message_properties)| async move {
            let go_to_implementations = self
                .go_to_implementations_exact(
                    fs_file_path,
                    &range.end_position(),
                    message_properties,
                )
                .await;
            match go_to_implementations {
                Ok(go_to_implementations) => go_to_implementations
                    .get_implementation_locations_vec()
                    .into_iter()
                    .map(|implementation| implementation.fs_file_path().to_owned())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    println!("get_imported_files::go_to_implementation::error({:?})", e);
                    vec![]
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

        let interested_files = definition_files
            .into_iter()
            .flatten()
            .chain(implementation_files.into_iter().flatten())
            .collect::<Vec<_>>();

        let outline_nodes = stream::iter(
            interested_files
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes_for_file = self
                .get_outline_nodes_using_editor(&fs_file_path, message_properties)
                .await;
            match outline_nodes_for_file {
                Ok(outline_nodes) => Some(outline_nodes.to_outline_nodes(fs_file_path)),
                Err(e) => {
                    eprintln!("{:?}", e);
                    None
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|data| data)
        .flatten()
        .collect::<Vec<_>>();

        // now we need to grab the outline nodes which pass the filter we are interested in
        let filtered_outline_nodes = outline_nodes
            .into_iter()
            .filter(|outline_node| {
                outline_node_name_filter.contains(&(outline_node.name().to_owned()))
            })
            .collect::<Vec<_>>();
        // TODO(skcd): We should exclude files which are internal libraries in specific languages like rust and typescript, golang etc
        Ok(filtered_outline_nodes)
    }

    /// We try to get the file contents which are present here along with the local
    /// import graph which is contained by these files
    ///
    /// We have to be smart about how we represent the data so we need to de-duplicate
    /// the nodes which we are interested in and get them to work properly
    pub async fn file_paths_to_user_context(
        &self,
        file_paths: Vec<String>,
        grab_import_nodes: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let mut deduplicated_file_paths = vec![];
        let mut already_seen_files: HashSet<String> = Default::default();
        file_paths.into_iter().for_each(|file_path| {
            if already_seen_files.contains(&file_path) {
                return;
            }
            already_seen_files.insert(file_path.to_owned());
            deduplicated_file_paths.push(file_path);
        });

        if !grab_import_nodes {
            let mut user_context_file_contents = vec![];
            for fs_file_path in deduplicated_file_paths.into_iter() {
                let file_content = self
                    .file_open(fs_file_path.to_owned(), message_properties.clone())
                    .await;
                if let Ok(file_content) = file_content {
                    let file_content_str = file_content.contents_ref();
                    let language_id = file_content.language();
                    user_context_file_contents.push(format!(
                        r#"# FILEPATH: {fs_file_path}
```{language_id}
{file_content_str}
```"#
                    ));
                }
            }
            return Ok(user_context_file_contents.join("\n"));
        }

        // now that we have the file paths, we should get the imports properly over here
        // we might have to deduplicate and tone it down a bit based on how big the imports
        // are getting
        let mut outline_nodes = vec![];
        for fs_file_path in deduplicated_file_paths.iter() {
            let imported_outline_nodes = self
                .get_imported_files_outline_nodes(fs_file_path, message_properties.clone())
                .await?;
            outline_nodes.extend(imported_outline_nodes);
        }

        // now that we have all the outline nodes we want to make sure that this is passed around properly
        // we have to sort them by path and also make sure that they are de-duplicated since that can lead to errors otherwise
        let mut deduplicated_outline_nodes: HashSet<String> = Default::default();
        let mut filtered_outline_nodes = vec![];
        for outline_node in outline_nodes.into_iter() {
            let unique_identifier = outline_node.unique_identifier();
            if deduplicated_outline_nodes.contains(&unique_identifier) {
                continue;
            }
            deduplicated_outline_nodes.insert(unique_identifier);
            filtered_outline_nodes.push(outline_node);
        }

        // now we filter out the outline nodes which we know are part of the core language features, these just add bloat
        // to the code
        filtered_outline_nodes.sort_by_key(|outline_node| outline_node.unique_identifier());
        let mut user_context_file_contents = vec![];
        for fs_file_path in deduplicated_file_paths.into_iter() {
            let file_content = self
                .file_open(fs_file_path.to_owned(), message_properties.clone())
                .await;
            if let Ok(file_content) = file_content {
                let file_content_str = file_content.contents_ref();
                let language_id = file_content.language();
                user_context_file_contents.push(format!(
                    r#"# FILEPATH: {fs_file_path}
```{language_id}
{file_content_str}
```"#
                ));
            }
        }

        // now we want to get the outline nodes processed over here
        let outline_node_strings = filtered_outline_nodes
            .into_iter()
            .filter(|outline_node| {
                let fs_file_path = outline_node.fs_file_path();
                if fs_file_path.contains("node_modules")
                    || fs_file_path.contains(".rustup")
                    || fs_file_path.contains(".cargo")
                {
                    false
                } else {
                    true
                }
            })
            .filter_map(|outline_node| outline_node.get_outline_node_compressed())
            .collect::<Vec<_>>();

        let user_context_file_contents = user_context_file_contents.join("\n");
        let code_outlines = outline_node_strings.join("\n");

        Ok(format!(
            "<files_provided>
{user_context_file_contents}
</files_imported>
<code_outline>
{code_outlines}
</code_outline>"
        ))
    }

    /// We try to warmup the user context over here with the context the user has
    /// added for context, this allows for faster and more accurate edits based
    /// on the model's in-context learning
    ///
    /// To warmup we send over a dummy edit request here with a very simple user instruction
    /// we do not care about how the output is working or what it is generating, just
    /// keep the cache warm
    pub async fn warmup_context(
        &self,
        file_paths: Vec<OpenFileResponse>,
        grab_import_nodes: bool,
        message_properties: SymbolEventMessageProperties,
    ) {
        let file_paths_to_user_context = self
            .file_paths_to_user_context(
                file_paths
                    .into_iter()
                    .map(|file_path| file_path.fs_file_path().to_owned())
                    .collect(),
                grab_import_nodes,
                message_properties.clone(),
            )
            .await
            .ok();
        let sender = message_properties.ui_sender();
        let request_id = message_properties.request_id_str();
        let session_id = message_properties.root_request_id().to_owned();
        let editor_url = message_properties.editor_url();
        let llm_properties = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::Anthropic,
            LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned())),
        );

        // we use the same session id for the exchange id
        let exchange_id = session_id.to_owned();
        // TODO(skcd): Figure out how to maximise the prompt cache hit over here
        let search_and_replace_request = SearchAndReplaceEditingRequest::new(
            "".to_owned(),
            Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0)),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            llm_properties,
            None,
            "".to_owned(),
            request_id.to_owned(),
            SymbolIdentifier::with_file_path("", ""),
            request_id.to_owned(),
            sender,
            file_paths_to_user_context,
            editor_url,
            None,
            vec![],
            vec![],
            true,
            session_id.to_owned(),
            exchange_id,
            None,
            vec![],
            message_properties.cancellation_token(),
            // no aide rules during the warmup phase
            None,
            // do not care about the warmup anyways
            false,
        );
        let search_and_replace = ToolInput::SearchAndReplaceEditing(search_and_replace_request);
        let cloned_tools = self.tools.clone();
        let _join_handle = tokio::spawn(async move {
            let _ = cloned_tools.invoke(search_and_replace).await;
        });
    }

    /// We are going to get the outline nodes without repeting the locations which
    /// have been already included
    pub async fn outline_nodes_for_references(
        &self,
        references: &[ReferenceLocation],
        message_properties: SymbolEventMessageProperties,
    ) -> Vec<OutlineNode> {
        // all file_paths contained in references
        let file_paths = references
            .iter()
            .map(|reference| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        // all outline_nodes within file_paths
        let outline_nodes_by_files = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes = self
                .get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await;
            outline_nodes.map(|outline_nodes| (fs_file_path, outline_nodes))
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .map(|(_, outline_nodes)| outline_nodes)
        .flatten()
        .collect::<Vec<_>>();

        outline_nodes_by_files
            .into_iter()
            .filter(|outline_node| {
                // check if a reference belongs inside this outline node
                let outline_node_range = outline_node.range();
                references.iter().any(|reference| {
                    // checks whether the outline node's range contains a reference's range, and has matching paths
                    outline_node_range.contains_check_line_column(reference.range())
                        && reference.fs_file_path() == outline_node.fs_file_path()
                })
            })
            .collect::<Vec<_>>()
    }

    pub async fn anchored_references_for_locations(
        &self,
        ref_locations: &[ReferenceLocation],
        original_symbol: AnchoredSymbol,
        message_properties: SymbolEventMessageProperties,
    ) -> Vec<AnchoredReference> {
        let file_paths = ref_locations
            .iter()
            .map(|location| location.fs_file_path().to_owned())
            .collect::<HashSet<String>>();

        // all outline_nodes within file_paths
        let outline_nodes_by_files = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes = self
                .get_ouline_nodes_grouped_fresh(&fs_file_path, message_properties)
                .await;
            outline_nodes.map(|outline_nodes| (fs_file_path, outline_nodes))
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .map(|(_, outline_nodes)| outline_nodes)
        .flatten()
        .collect::<Vec<_>>();

        // let anchored_references = ref_locations
        //     .into_iter()
        //     .filter_map(|ref_location| {
        //         let matching_outline_node = outline_nodes_by_files.iter().find(|outline_node| {
        //             let outline_node_range = outline_node.range();
        //             outline_node_range.contains_check_line_column(ref_location.range())
        //                 && ref_location.fs_file_path() == outline_node.fs_file_path()
        //         });

        //         matching_outline_node.map(|outline_node: &OutlineNode| {
        //             AnchoredReference::new(
        //                 original_symbol.to_owned(),
        //                 ref_location.to_owned(),
        //                 outline_node.to_owned(),
        //             )
        //         })
        //     })
        //     .collect::<Vec<_>>();
        outline_nodes_by_files
            .into_iter()
            .filter_map(|outline_node| {
                // check if a reference belongs inside this outline node
                let outline_node_range = outline_node.range();
                let references_inside = ref_locations
                    .iter()
                    .filter(|reference| {
                        outline_node_range.contains_check_line_column(reference.range())
                            && reference.fs_file_path() == outline_node.fs_file_path()
                    })
                    .collect::<Vec<_>>();

                if references_inside.is_empty() {
                    None
                } else {
                    Some(AnchoredReference::new(
                        original_symbol.to_owned(),
                        references_inside
                            .into_iter()
                            .map(|data| data.clone())
                            .collect(),
                        outline_node,
                    ))
                }
            })
            .collect::<Vec<_>>()
    }

    /// Get the diagnostics with content for the files which we are interested in
    /// over here
    ///
    /// This way we are able to prompt the LLM a bit better
    async fn get_lsp_diagnostics_with_content(
        &self,
        fs_file_path: &str,
        range: &Range,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<DiagnosticWithSnippet>, SymbolError> {
        let dignostics = self
            .get_lsp_diagnostics(fs_file_path, range, message_properties.clone())
            .await?;
        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let diagnotic_with_snippet = dignostics
            .remove_diagnostics()
            .into_iter()
            .filter_map(|diagnostic| {
                diagnostic
                    .with_snippet_from_contents(file_content.contents_ref(), &fs_file_path)
                    .ok()
            })
            .collect::<Vec<_>>();
        Ok(diagnotic_with_snippet)
    }

    /// This formats the diagnostics for the prompt and can be used as in as a drop
    /// in replacement when sending lsp diagnostics to the prompt
    async fn get_lsp_diagnostics_with_content_prompt_format(
        &self,
        fs_file_path: &str,
        range: &Range,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let lsp_diagnostics_with_content = self
            .get_lsp_diagnostics_with_content(fs_file_path, range, message_properties)
            .await?;
        let diagnostics = lsp_diagnostics_with_content
            .into_iter()
            .map(|diagnostic| {
                let content = diagnostic.snippet();
                let message = diagnostic.message();
                format!(
                    r#"<diagnostic>
<content>
{content}
</content>
<message>
{message}
</message>
</diagnostic>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            r#"# LSP Diagnostics on file: {fs_file_path}
{diagnostics}"#
        ))
    }

    pub async fn get_lsp_diagnostics_prompt_format(
        &self,
        file_paths: Vec<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let diagnostics = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            self.get_lsp_diagnostics_with_content_prompt_format(
                &fs_file_path,
                &Range::new(Position::new(0, 0, 0), Position::new(100_000, 0, 0)),
                message_properties,
            )
            .await
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<Vec<_>>()
        .join("\n");
        Ok(diagnostics)
    }

    /// Grabs the git-diff sorted by timestamp relative to the content if any
    /// we have seen before
    ///
    /// This allows us to continously understand the changes which have happened
    /// in the editor and even if the final output of git-diff is nothing, the user
    /// might have made a change and then reverted it, but we are able to capture it
    pub async fn recently_edited_files_with_content(
        &self,
        important_file_paths: HashSet<String>,
        diff_content_files: Vec<DiffFileContent>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<DiffRecentChanges, SymbolError> {
        let input = ToolInput::EditedFiles(EditedFilesRequest::new(
            message_properties.editor_url(),
            diff_content_files,
        ));
        let mut recently_edited_files = self
            .tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .recently_edited_files()
            .ok_or(SymbolError::WrongToolOutput)?
            .changed_files();

        let recently_edited_file_content = recently_edited_files
            .iter()
            .map(|recently_edited_file| {
                DiffFileContent::new(
                    recently_edited_file.fs_file_path().to_owned(),
                    recently_edited_file.current_content().to_owned(),
                    // here we throw a hail mary towards the editor, that it will be
                    // able to give us the git-diff when present
                    None,
                )
            })
            .collect::<Vec<_>>();

        // sort it in increasing order based on the updated timestamp
        recently_edited_files.sort_by_key(|edited_file| edited_file.updated_timestamp_ms());
        let (l1_cache, l2_cache): (Vec<_>, Vec<_>) = recently_edited_files
            .into_iter()
            .partition(|edited_file| important_file_paths.contains(edited_file.fs_file_path()));
        let l1_cache = l1_cache
            .into_iter()
            .map(|diff_file| diff_file.diff().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let l2_cache = l2_cache
            .into_iter()
            .map(|diff_file| diff_file.diff().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        // now to create the diff-recent-changes we are going to remove the files
        // which exist in our important file paths and mark them as l1 cache
        // while keeping the other set of files as l2 cache
        Ok(DiffRecentChanges::new(
            l1_cache,
            l2_cache,
            recently_edited_file_content,
        ))
    }

    /// Grabs the git-diff sorted by the timestamp over here
    /// If we are provided a set of files then this set comes at the very end
    /// and is still ordered by the timestamp
    /// Those which are not included in the file-set are arranged in the order
    /// they appear in
    pub async fn recently_edited_files(
        &self,
        important_file_paths: HashSet<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<DiffRecentChanges, SymbolError> {
        let input =
            // we don't have access to the our own state of the world over here so sending
            // over an empty vec instead
            ToolInput::EditedFiles(EditedFilesRequest::new(message_properties.editor_url(), vec![]));
        let mut recently_edited_files = self
            .tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .recently_edited_files()
            .ok_or(SymbolError::WrongToolOutput)?
            .changed_files();

        let recently_edited_file_content = recently_edited_files
            .iter()
            .map(|recently_edited_file| {
                DiffFileContent::new(
                    recently_edited_file.fs_file_path().to_owned(),
                    recently_edited_file.current_content().to_owned(),
                    None,
                )
            })
            .collect::<Vec<_>>();

        // sort it in increasing order based on the updated timestamp
        recently_edited_files.sort_by_key(|edited_file| edited_file.updated_timestamp_ms());
        let (l1_cache, l2_cache): (Vec<_>, Vec<_>) = recently_edited_files
            .into_iter()
            .partition(|edited_file| important_file_paths.contains(edited_file.fs_file_path()));
        let l1_cache = l1_cache
            .into_iter()
            .map(|diff_file| diff_file.diff().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let l2_cache = l2_cache
            .into_iter()
            .map(|diff_file| diff_file.diff().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        // now to create the diff-recent-changes we are going to remove the files
        // which exist in our important file paths and mark them as l1 cache
        // while keeping the other set of files as l2 cache
        Ok(DiffRecentChanges::new(
            l1_cache,
            l2_cache,
            recently_edited_file_content,
        ))
    }

    pub async fn reference_filtering(
        &self,
        user_query: &str,
        references: Vec<AnchoredReference>,
        message_properties: SymbolEventMessageProperties,
    ) {
        let start = std::time::Instant::now();
        let anthropic_api_keys = LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned()));
        let llm_properties = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::Anthropic,
            anthropic_api_keys.clone(),
        );
        let references = references
            .into_iter()
            .take(10) // todo(zi): so we don't go crazy with 1000s of requests
            .collect::<Vec<_>>();
        println!(
            "tool_box:reference_filtering::references.len({:?})",
            &references.len()
        );
        let request = ReferenceFilterRequest::new(
            user_query.to_owned(),
            llm_properties.clone(),
            message_properties.request_id_str().to_owned(),
            message_properties.clone(),
            references.clone(),
        );
        let llm_time = Instant::now();
        let _relevant_references = match self
            .tools
            .invoke(ToolInput::ReferencesFilter(request))
            .await
        {
            Ok(ok_references) => ok_references.get_relevant_references().unwrap_or_default(),
            Err(err) => {
                eprintln!("Failed to filter references: {:?}", err);
                Vec::new()
            }
        };
        println!("ReferenceFilter::invoke::elapsed({:?})", llm_time.elapsed());

        println!(
            "collect references async task total elapsed: {:?}",
            start.elapsed()
        );
    }

    /// We can ask the model to reason here and generate the reasoning trace
    /// as required
    ///
    /// This is helpful to plan out big changes if required and that can be passed
    /// to sonnet3.5 for the next steps
    pub async fn reasoning(
        &self,
        user_query: String,
        file_paths: Vec<String>,
        aide_rules: Option<String>,
        scratch_pad_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let file_contents = stream::iter(
            file_paths
                .to_vec()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            (
                fs_file_path.to_owned(),
                self.file_open(fs_file_path, message_properties).await,
            )
        })
        .buffer_unordered(1)
        .collect::<Vec<_>>()
        .await;

        // now we get the lsp diagnostics which are on the file paths we are looking at
        let lsp_diagnostics = stream::iter(
            file_paths
                .to_vec()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            (
                fs_file_path.to_owned(),
                self.get_lsp_diagnostics_with_content(
                    &fs_file_path,
                    &Range::new(Position::new(0, 0, 0), Position::new(100_000, 0, 0)),
                    message_properties,
                )
                .await,
            )
        })
        .buffer_unordered(1)
        .collect::<Vec<_>>()
        .await;

        // now we get the diff recent changes
        let recente_edits_made = self
            .recently_edited_files(Default::default(), message_properties.clone())
            .await?;

        // leave out code in selection for now
        let code_in_selection = "".to_owned();

        let file_contents = file_contents
            .into_iter()
            .filter_map(|(_, file_open_response)| {
                file_open_response
                    .map(|file_open_response| {
                        let language_id = file_open_response.language();
                        let fs_file_path = file_open_response.fs_file_path();
                        let contents = file_open_response.contents_ref();
                        format!(
                            r#"# FILEPATH: {fs_file_path}
```{language_id}
{contents}
```"#
                        )
                    })
                    .ok()
            })
            .collect::<Vec<_>>()
            .join("\n");

        let lsp_diagnostics = lsp_diagnostics
            .into_iter()
            .filter_map(|(fs_file_path, lsp_diagnostics)| {
                lsp_diagnostics
                    .map(|lsp_diagnostics| {
                        let diagnostics = lsp_diagnostics
                            .into_iter()
                            .map(|diagnostic| {
                                let message = diagnostic.message();
                                let snippet = diagnostic.snippet();
                                format!(
                                    r#"<diagnostic>
<message>
{message}
</message>
<snippet>
{snippet}
</snippet>
</diagnostic>"#
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!(
                            r#"# LSP Diagnostics on file: {fs_file_path}
{diagnostics}"#
                        )
                    })
                    .ok()
            })
            .collect::<Vec<_>>()
            .join("\n");

        // scratch-pad path
        let scratch_pad_content = self
            .file_open(scratch_pad_path.to_owned(), message_properties.clone())
            .await?;

        let session_id = message_properties.root_request_id().to_owned();
        let exchange_id = message_properties.request_id_str().to_owned();
        // for the recent edits made we want to grab the l2 cache over here
        let l2_cache = recente_edits_made.l2_changes();
        let tool_input = ToolInput::Reasoning(ReasoningRequest::new(
            user_query,
            file_contents,
            code_in_selection,
            lsp_diagnostics,
            l2_cache.to_owned(),
            message_properties.root_request_id().to_owned(),
            scratch_pad_path.to_owned(),
            scratch_pad_content.contents(),
            aide_rules.clone(),
            message_properties.editor_url().to_owned(),
            session_id,
            exchange_id,
        ));
        let reasoning_output = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .reasoning_output()
            .ok_or(SymbolError::WrongToolOutput)?
            .response();
        Ok(reasoning_output)
    }

    /// Convert the gathered context from the events to a prompt for the LLM/LRM
    ///
    /// We might get consecutive selection ranges which can mess up the prompt
    /// very quickly, we should be a bit more careful about that
    ///
    /// TODO(skcd): We should deduplicate the outline nodes which are present
    /// since we might be repeating a lot of context, an example is:
    /// https://gist.github.com/theskcd/c710d77f223f21fac1525277d0bfa929
    pub async fn context_recording_to_prompt(
        &self,
        context_events: Vec<ContextGatheringEvent>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let file_paths = context_events
            .iter()
            .map(|context_event| match context_event {
                ContextGatheringEvent::LSPContextEvent(lsp_event) => {
                    match lsp_event.destination_maybe() {
                        Some(destination) => {
                            vec![destination]
                        }
                        None => {
                            vec![]
                        }
                    }
                    .into_iter()
                    .chain(vec![lsp_event.source_fs_file_path().to_owned()])
                    .collect::<Vec<_>>()
                }
                ContextGatheringEvent::OpenFile(open_file) => {
                    vec![open_file.fs_file_path().to_owned()]
                }
                ContextGatheringEvent::Selection(selection) => {
                    vec![selection.fs_file_path().to_owned()]
                }
            })
            .flatten()
            .collect::<HashSet<String>>();

        let outline_nodes = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let outline_nodes = self
                .get_outline_nodes_from_editor(&fs_file_path, message_properties)
                .await;
            outline_nodes.map(|outline_nodes| (fs_file_path, outline_nodes))
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<_, _>>();

        // To handle the LSPEvent and the selection event this is what we want to do:
        // - LSP event: We want to get the outline node where the user performed the clicked and on the destination make sure to include that outline node (if present)
        // - Selection event: If its a selection event we want to make sure that we always keep track of the selection and deduplicate and merge any selections which are consecutive
        // - we never show the full file unless its absolutely necessary

        let mut final_prompt_parts: Vec<String> = vec![];
        for context_event in context_events.into_iter() {
            match context_event {
                ContextGatheringEvent::LSPContextEvent(lsp_context_event) => {
                    let source_file = lsp_context_event.source_fs_file_path();
                    let destination = lsp_context_event.destination_maybe();
                    println!("tool_box::context_recroding_to_prompt::lsp_context_event::source_file({})::destination({:?})", source_file, &destination);
                    let source_outline_nodes = outline_nodes
                        .get(source_file)
                        .map(|outline_nodes| outline_nodes.to_vec())
                        .unwrap_or_default();
                    let destination_outline_nodes = if let Some(destination) = destination {
                        outline_nodes
                            .get(&destination)
                            .map(|outline_nodes| outline_nodes.to_vec())
                            .unwrap_or_default()
                    } else {
                        println!(
                            "tool_box::context_recording_to_prompt::no_destination_outline_node"
                        );
                        vec![]
                    };
                    let prompt = lsp_context_event.lsp_context_event_to_prompt(
                        source_outline_nodes,
                        destination_outline_nodes,
                    );
                    if let Some(prompt) = prompt {
                        final_prompt_parts.push(prompt);
                    }
                }
                ContextGatheringEvent::Selection(selection_event) => {
                    let source_file = selection_event.fs_file_path();
                    let source_outline_nodes = outline_nodes
                        .get(source_file)
                        .map(|outline_nodes| outline_nodes.to_vec())
                        .unwrap_or_default();
                    let prompt = SelectionContextEvent::to_prompt(
                        vec![selection_event],
                        source_outline_nodes,
                    );
                    final_prompt_parts.push(prompt);
                }
                ContextGatheringEvent::OpenFile(_open_file) => {
                    // keep going, we are not responding to these events
                }
            }
        }
        let joined_journey = final_prompt_parts
            .into_iter()
            .enumerate()
            .map(|(idx, prompt)| {
                format!(
                    "## {idx}
{prompt}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        Ok(format!(
            "I am showing you all the steps I took and giving this to you as context:
{joined_journey}"
        ))
    }

    /// Creates a file using the editor endpoint
    pub async fn create_file(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let tool_input = ToolInput::CreateFile(CreateFileRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url(),
        ));
        let _ = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_file_create_response()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(())
    }

    /// Generates the steps for a plan
    pub async fn generate_plan(
        &self,
        user_query: &str,
        previous_queries: Vec<String>,
        user_context: &UserContext,
        aide_rules: Option<String>,
        previous_messages: Vec<SessionChatMessage>,
        is_deep_reasoning: bool,
        step_sender: Option<UnboundedSender<StepSenderEvent>>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<PlanStep>, SymbolError> {
        // pick up from here about passing aiderules everywhere and then look at the tool use agent
        let step_generator_request = StepGeneratorRequest::new(
            user_query.to_owned(),
            previous_queries,
            is_deep_reasoning,
            previous_messages,
            message_properties.root_request_id().to_owned(),
            message_properties.editor_url(),
            message_properties.request_id_str().to_owned(),
            message_properties.ui_sender(),
            aide_rules,
            step_sender,
            message_properties.cancellation_token(),
            message_properties.llm_properties().clone(),
        )
        .with_user_context(user_context);
        println!("tool_box::generate_plan::start");
        let start_instant = std::time::Instant::now();
        let plan_steps = self
            .tools
            .invoke(ToolInput::GenerateStep(step_generator_request))
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .step_generator_output()
            .ok_or(SymbolError::WrongToolOutput)?
            .into_plan_steps();
        println!(
            "tool_box::generate_plan::end::time_taken({}ms)",
            &start_instant.elapsed().as_millis()
        );
        Ok(plan_steps)
    }

    pub async fn generate_new_steps_and_user_help_for_plan(
        &self,
        plan_up_until_now: String,
        initial_user_query: String,
        query: String,
        user_context: UserContext,
        recent_diff_changes: DiffRecentChanges,
        message_properties: SymbolEventMessageProperties,
        is_deep_reasoning: bool,
    ) -> Result<(Vec<PlanStep>, Option<String>), SymbolError> {
        let plan_add_request = PlanAddRequest::new(
            plan_up_until_now,
            user_context,
            initial_user_query,
            query,
            recent_diff_changes,
            message_properties.editor_url(),
            message_properties.root_request_id().to_owned(),
            is_deep_reasoning,
            "".to_owned(),
        )
        .ask_human_for_help();
        let tool_input = ToolInput::PlanStepAdd(plan_add_request);
        println!("tool_box::generate_new_steps_for_plan::start");
        let start_instant = std::time::Instant::now();
        let plan_steps = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_plan_new_steps()
            .ok_or(SymbolError::WrongToolOutput)?;
        let human_help = plan_steps.huamn_help();
        let steps = plan_steps.into_plan_steps();
        println!(
            "tool_box::generate_new_steps_for_plan::end({}ms)",
            start_instant.elapsed().as_millis()
        );
        Ok((steps, human_help))
    }

    /// Creates new steps which can be added to the plan
    pub async fn generate_new_steps_for_plan(
        &self,
        plan_up_until_now: String,
        initial_user_query: String,
        query: String,
        user_context: UserContext,
        recent_diff_changes: DiffRecentChanges,
        message_properties: SymbolEventMessageProperties,
        is_deep_reasoning: bool,
        diagnostics: String,
    ) -> Result<Vec<PlanStep>, SymbolError> {
        let plan_add_request = PlanAddRequest::new(
            plan_up_until_now,
            user_context,
            initial_user_query,
            query,
            recent_diff_changes,
            message_properties.editor_url(),
            message_properties.root_request_id().to_owned(),
            is_deep_reasoning,
            diagnostics,
        );
        let tool_input = ToolInput::PlanStepAdd(plan_add_request);
        println!("tool_box::generate_new_steps_for_plan::start");
        let start_instant = std::time::Instant::now();
        let plan_steps = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_plan_new_steps()
            .ok_or(SymbolError::WrongToolOutput)?
            .into_plan_steps();
        println!(
            "tool_box::generate_new_steps_for_plan::end({}ms)",
            start_instant.elapsed().as_millis()
        );
        Ok(plan_steps)
    }

    /// Captures the deep user context when required by using tree-sitter
    pub async fn generate_deep_user_context(
        &self,
        user_context: UserContext,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<UserContext, SymbolError> {
        let variables = user_context
            .variables
            .iter()
            .filter(|variable| variable.is_selection() || variable.is_code_symbol())
            .collect::<Vec<_>>();
        let file_paths = variables
            .iter()
            .filter(|variable| variable.is_selection() || variable.is_code_symbol())
            .map(|variable| variable.fs_file_path.to_owned())
            .collect::<HashSet<String>>();
        // file path to clickable nodes where we can go deeper to get more context
        let file_paths_to_clickable_nodes = stream::iter(
            file_paths
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(file_path, message_properties)| async move {
            let open_file_path = self
                .file_open(file_path.to_owned(), message_properties.clone())
                .await;
            let clickable_function_nodes = open_file_path.map(|file_open_response| {
                let language_parsing = self
                    .editor_parsing
                    .for_file_path(file_open_response.fs_file_path());
                match language_parsing {
                    Some(language_parsing) => {
                        Some(language_parsing.generate_function_insights(
                            file_open_response.contents_ref().as_bytes(),
                        ))
                    }
                    None => None,
                }
            });
            let flattened_result = clickable_function_nodes.ok().flatten();
            flattened_result.map(|result| (file_path, result))
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<_, _>>();

        // now that we have clickable nodes and then we need to filter it to the variables
        // which we can click on
        let filtered_file_paths_to_clickable_nodes = file_paths_to_clickable_nodes
            .into_iter()
            .map(|(fs_file_path, mut clickable_nodes)| {
                clickable_nodes = clickable_nodes
                    .into_iter()
                    .filter(|(_, clickable_range)| {
                        let variable_contains = variables
                            .iter()
                            .filter(|variable| &variable.fs_file_path == &fs_file_path)
                            .any(|variable| {
                                // check if the variable range contains the clickable node we are interested in
                                Range::new(
                                    variable.start_position.clone(),
                                    variable.end_position.clone(),
                                )
                                .contains_check_line(clickable_range)
                            });
                        variable_contains
                    })
                    .collect::<Vec<_>>();
                (fs_file_path, clickable_nodes)
            })
            .collect::<HashMap<_, _>>();

        // Now we try to get the definition of these nodes we have to click and then
        // go to the implementations of this
        let mut already_seen_outline_nodes: HashSet<String> = Default::default();
        let mut additional_user_variables = vec![];
        for (fs_file_path, clickable_nodes) in filtered_file_paths_to_clickable_nodes.into_iter() {
            for (click_word, click_range) in clickable_nodes.into_iter() {
                // here we want to invoke to go to definition
                // and then we want to invoke go to implementations over here
                // get the outline nodes which we are interested in and then merge them
                // together to deduplicate them
                // and then we want to get the compressed outline nodes over here
                let go_to_definition = self
                    .go_to_definition(
                        &fs_file_path,
                        click_range.start_position(),
                        message_properties.clone(),
                    )
                    .await?;

                // we want to filter out the definitions which might lead to common modules like Vec, Arc etc
                if go_to_definition.is_empty() {
                    continue;
                }
                let first_definition = go_to_definition.definitions().remove(0);
                println!(
                    "tool_box::generate_outline_for_click_word::word({})::definition_path({})",
                    &click_word,
                    first_definition.file_path()
                );
                // try to filter out things here which are very common: Vec, Arc, Box etc, these are present in rustlib
                let is_blocklisted_definition = vec!["rustlib/src"]
                    .into_iter()
                    .any(|path_fragment| first_definition.file_path().contains(&path_fragment));
                if is_blocklisted_definition {
                    println!(
                        "tool_box::generate_outline_for_click_word::word({})::is_blocklisted",
                        &click_word
                    );
                    continue;
                }
                // now we want to get the outline node which contains this definition
                let mut variables_for_clickable_nodes = vec![];
                let outline_node_containing_range = self
                    .get_outline_node_for_range(
                        first_definition.range(),
                        first_definition.file_path(),
                        message_properties.clone(),
                    )
                    .await?;
                if already_seen_outline_nodes
                    .contains(&outline_node_containing_range.unique_identifier())
                {
                    continue;
                }
                already_seen_outline_nodes
                    .insert(outline_node_containing_range.unique_identifier());
                variables_for_clickable_nodes.push(VariableInformation::create_selection(
                    outline_node_containing_range.range().clone(),
                    outline_node_containing_range.fs_file_path().to_owned(),
                    format!("Definition for {}", click_word),
                    outline_node_containing_range.get_outline_short(),
                    outline_node_containing_range
                        .content()
                        .language()
                        .to_owned(),
                ));
                // now we want to get the implementations for the outline node
                let go_to_implementations_response = self
                    .go_to_implementations_exact(
                        outline_node_containing_range.fs_file_path(),
                        &outline_node_containing_range
                            .identifier_range()
                            .start_position(),
                        message_properties.clone(),
                    )
                    .await?;
                // now that we have go-to-implementations we want to find the outline nodes which contain these implementations
                let implementation_locations =
                    go_to_implementations_response.remove_implementations_vec();

                // keep track of already seen outline nodes
                let mut implementation_short_outline_nodes = vec![];
                for implementation_location in implementation_locations.into_iter() {
                    // find the outline node which contains these implementations
                    let implementation_fs_file_path = implementation_location.fs_file_path();
                    let implementation_range = implementation_location.range();
                    let outline_node_containing_range = self
                        .get_outline_node_for_range(
                            implementation_range,
                            implementation_fs_file_path,
                            message_properties.clone(),
                        )
                        .await?;
                    if already_seen_outline_nodes
                        .contains(&outline_node_containing_range.unique_identifier())
                    {
                        continue;
                    }
                    already_seen_outline_nodes
                        .insert(outline_node_containing_range.unique_identifier());
                    implementation_short_outline_nodes.push(outline_node_containing_range);
                }

                // now we have all the outline nodes for the clickable range we are interested in
                variables_for_clickable_nodes.extend(
                    implementation_short_outline_nodes
                        .into_iter()
                        .map(|implementation_outline_node| {
                            VariableInformation::create_selection(
                                implementation_outline_node.range().clone(),
                                implementation_outline_node.fs_file_path().to_owned(),
                                format!("Implementation for {}", click_word),
                                implementation_outline_node.get_outline_short(),
                                implementation_outline_node.content().language().to_owned(),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
                additional_user_variables.extend(variables_for_clickable_nodes);
            }
        }

        println!(
            "tool_box::generate_deeper_user_context::additional_user_varibles::len({})",
            additional_user_variables.len()
        );
        let user_context = user_context.add_variables(additional_user_variables);
        // update the user context with additional variables which we found
        Ok(user_context)
    }

    pub async fn find_associated_files(
        &self,
        primary_file: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<String>, SymbolError> {
        let mut associated_files: HashSet<String> = HashSet::new();

        // // 1. Add files from the local code graph:
        // let import_nodes = self.find_import_nodes(primary_file, message_properties.clone()).await?;
        // for (import_name, range) in import_nodes {
        //     let definitions = self.go_to_definition(primary_file, range.end_position(), message_properties.clone()).await?;
        //     for definition in definitions.definitions() {
        //         associated_files.insert(definition.file_path().to_owned());
        //     }
        //     // Also check implementations for interfaces/traits:
        //     let implementations = self.go_to_implementations_exact(primary_file, &range.end_position(), message_properties.clone()).await?;
        //     for implementation in implementations.get_implementation_locations_vec() {
        //         associated_files.insert(implementation.fs_file_path().to_owned());
        //     }
        // }

        // 2. (Optional) Add files based on references to top-level symbols in primary_file:
        let top_level_symbols = self
            .get_outline_nodes_from_editor(primary_file, message_properties.clone())
            .await
            .unwrap_or_default();
        for symbol in top_level_symbols {
            let references = self
                .go_to_references(
                    primary_file.to_owned(),
                    symbol.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?;
            for reference in references.locations() {
                associated_files.insert(reference.fs_file_path().to_owned());
            }
        }

        Ok(associated_files.into_iter().collect())
    }

    /// This looks at the LSP diagnostics and figures out the type definition to go to
    ///
    /// This is a common mistake on the LLM where it hallucinates a function on a class
    /// so we can just detect the class or the parent caller until where its correct and
    /// dive deeper from there
    /// This function just gives back the range where we can click for the type-definitions
    pub async fn grab_type_definition_worthy_positions_using_diagnostics(
        &self,
        fs_file_path: &str,
        mut lsp_diagnositcs: Vec<LSPDiagnosticError>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<VariableInformation>, SymbolError> {
        let language_properties = self.editor_parsing.for_file_path(fs_file_path);
        if language_properties.is_none() {
            return Ok(vec![]);
        }
        let language_properties = language_properties.expect("is_none to hold");
        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await;
        if file_content.is_err() {
            return Ok(vec![]);
        }
        let file_content = file_content.expect("is_err to hold");
        let function_call_ranges = language_properties
            .generate_function_call_paths(file_content.contents_ref().as_bytes());
        if function_call_ranges.is_none() {
            return Ok(vec![]);
        }
        let function_call_ranges = function_call_ranges.expect("is_none to hold");
        let function_call_ranges_with_diagnostic_range = function_call_ranges
            .into_iter()
            .filter_map(|(click_word, click_range)| {
                lsp_diagnositcs
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, diagnostic)| {
                        if click_range.contains_check_line_column(diagnostic.range()) {
                            Some((idx, diagnostic))
                        } else {
                            None
                        }
                    })
                    .next()
                    .map(|(idx, diagnostic)| (idx, diagnostic.range().clone()))
                    .map(|(idx, diagnostic_range)| (idx, click_word, click_range, diagnostic_range))
            })
            .collect::<Vec<_>>();

        // now that we have filtered the function call ranges, we want to take a step back on the position we are in
        // since the function call diagnostics going wrong is a sign that the LLM does not see the outline node type
        // for the
        let previous_word_click_ranges = stream::iter(
            function_call_ranges_with_diagnostic_range
                .into_iter()
                .map(|function_call_range| (function_call_range, message_properties.clone())),
        )
        .map(|(function_call_range, message_properties)| async move {
            let go_to_previous_word = ToolInput::GoToPreviousWord(GoToPreviousWordRequest::new(
                fs_file_path.to_owned(),
                function_call_range.2.start_position(),
                message_properties.editor_url(),
            ));
            let tool_output = self.tools.invoke(go_to_previous_word).await;
            match tool_output {
                Ok(output) => output
                    .go_to_previous_word_response()
                    .map(|output| output.range())
                    .flatten(),
                Err(_) => None,
            }
            .map(|output| (function_call_range.0, function_call_range.0, output))
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .collect::<Vec<_>>();

        let mut additional_outline_node_ids = vec![];
        let mut outline_nodes: HashMap<String, OutlineNode> = Default::default();
        for (lsp_diagnostic_index, click_word, click_range) in
            previous_word_click_ranges.into_iter()
        {
            let mut current_outline_nodes = vec![];
            // here we want to invoke to go to type definition
            // and then we want to invoke go to implementations over here
            // get the outline nodes which we are interested in and then merge them
            // together to deduplicate them
            // and then we want to get the compressed outline nodes over here
            let go_to_type_definition = self
                .go_to_type_definition(
                    &fs_file_path,
                    click_range.start_position(),
                    message_properties.clone(),
                )
                .await?;

            // we want to filter out the definitions which might lead to common modules like Vec, Arc etc
            if go_to_type_definition.is_empty() {
                continue;
            }
            let first_definition =
                go_to_type_definition
                    .definitions()
                    .into_iter()
                    .find(|type_definition| {
                        // try to filter out things here which are very common: Vec, Arc, Box etc, these are present in rustlib
                        !vec!["rustlib/src"].into_iter().any(|path_fragment| {
                            type_definition.file_path().contains(&path_fragment)
                        })
                    });
            if first_definition.is_none() {
                println!(
                    "tool_box::grab_type_definitions_worthy_using_diagnostics::is_none::word({})",
                    &click_word
                );
                continue;
            }
            let first_definition = first_definition.expect("is_none to hold");
            // try to filter out things here which are very common: Vec, Arc, Box etc, these are present in rustlib
            let is_blocklisted_definition = vec!["rustlib/src"]
                .into_iter()
                .any(|path_fragment| first_definition.file_path().contains(&path_fragment));
            if is_blocklisted_definition {
                println!(
                    "tool_box::generate_outline_for_click_word::word({})::is_blocklisted",
                    &click_word
                );
                continue;
            }
            // now we want to get the outline node which contains this definition
            let outline_node_containing_range = self
                .get_outline_node_for_range(
                    first_definition.range(),
                    first_definition.file_path(),
                    message_properties.clone(),
                )
                .await?;
            if additional_outline_node_ids
                .contains(&outline_node_containing_range.unique_identifier())
            {
                continue;
            }

            // update state variables like outline_nodes hashmap and addition_outline_node_ids which we want
            // to track
            current_outline_nodes.push(outline_node_containing_range.unique_identifier());
            additional_outline_node_ids.push(outline_node_containing_range.unique_identifier());
            outline_nodes.insert(
                outline_node_containing_range.unique_identifier(),
                outline_node_containing_range.clone(),
            );
            // now we want to get the implementations for the outline node
            let go_to_implementations_response = self
                .go_to_implementations_exact(
                    outline_node_containing_range.fs_file_path(),
                    &outline_node_containing_range
                        .identifier_range()
                        .start_position(),
                    message_properties.clone(),
                )
                .await?;
            // now that we have go-to-implementations we want to find the outline nodes which contain these implementations
            let implementation_locations =
                go_to_implementations_response.remove_implementations_vec();

            // keep track of already seen outline nodes
            for implementation_location in implementation_locations.into_iter() {
                // find the outline node which contains these implementations
                let implementation_fs_file_path = implementation_location.fs_file_path();
                let implementation_range = implementation_location.range();
                let outline_node_containing_range = self
                    .get_outline_node_for_range(
                        implementation_range,
                        implementation_fs_file_path,
                        message_properties.clone(),
                    )
                    .await?;

                // do some state management over here, we want to keep track of this for the current
                // lsp diagnostic
                current_outline_nodes.push(outline_node_containing_range.unique_identifier());
                if !additional_outline_node_ids
                    .contains(&outline_node_containing_range.unique_identifier())
                {
                    additional_outline_node_ids
                        .push(outline_node_containing_range.unique_identifier());
                }
                outline_nodes.insert(
                    outline_node_containing_range.unique_identifier(),
                    outline_node_containing_range,
                );
            }

            // now update the lsp diagnostics with the information we generated
            lsp_diagnositcs
                .get_mut(lsp_diagnostic_index)
                .map(|lsp_diagnostic| {
                    lsp_diagnostic.set_user_variables(
                        current_outline_nodes
                            .into_iter()
                            .filter_map(|current_outline_node| {
                                outline_nodes.get(&current_outline_node)
                            })
                            .map(|outline_node| {
                                VariableInformation::from_outline_node(outline_node)
                            })
                            .collect::<Vec<_>>(),
                    );
                });
        }

        // converts everything to a variable information
        // this is the type we use in user context and we keep it structured to reflect
        // it on the UI
        let additional_user_variables = additional_outline_node_ids
            .into_iter()
            .filter_map(|outline_node_id| {
                let outline_node = outline_nodes.remove(&outline_node_id);
                outline_node
                    .map(|outline_node| VariableInformation::from_outline_node(&outline_node))
            })
            .collect::<Vec<_>>();
        Ok(additional_user_variables)
    }

    /// Go-to-references for errors which are happening at the start of the outline
    /// node
    /// The end goal here is to create a prompt which can be shared with the o1-preview
    /// model for background reply and it can come back whenever
    pub async fn broken_references_for_lsp_diagnostics(
        &self,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
        // we return the extra variables and also the prompt which is our query which
        // we generate
    ) -> Result<(Vec<VariableInformation>, String), SymbolError> {
        // Step 1: Get outline nodes and LSP diagnostics
        let outline_nodes = self
            .get_outline_nodes_from_editor(fs_file_path, message_properties.clone())
            .await
            .unwrap_or_default();

        // for each outline node try to get the hover information, if we get
        // more than one then its a sign that we should check the references
        #[derive(Clone)]
        struct LSPDiagnosticErrorClickableRange {
            outline_node_content: OutlineNodeContent,
        }
        let mut clickable_diagnostics: Vec<LSPDiagnosticErrorClickableRange> = vec![];
        for outline_node in outline_nodes.into_iter() {
            if outline_node.is_class_definition() {
                let hover_information = self
                    .get_hover_information(
                        fs_file_path,
                        &outline_node.identifier_range().start_position(),
                        message_properties.clone(),
                    )
                    .await;
                if let Ok(hover_information) = hover_information {
                    if hover_information.remove_diagnostics().len() > 1 {
                        clickable_diagnostics.push(LSPDiagnosticErrorClickableRange {
                            outline_node_content: outline_node.content().clone(),
                        })
                    }
                }
            } else {
                // check in each of the children if the hover information is more than
                // 1
                for child_node in outline_node.children() {
                    if !child_node.is_function_type() {
                        continue;
                    }
                    let hover_information = self
                        .get_hover_information(
                            fs_file_path,
                            &child_node.identifier_range().start_position(),
                            message_properties.clone(),
                        )
                        .await;
                    if let Ok(hover_information) = hover_information {
                        dbg!(child_node.name());
                        if dbg!(hover_information.remove_diagnostics()).len() > 1 {
                            clickable_diagnostics.push(LSPDiagnosticErrorClickableRange {
                                outline_node_content: child_node.clone(),
                            })
                        }
                    }
                }
            }
        }

        println!(
            "tool_box::broken_references_for_lsp_diagnostics::clickable_outline_nodes::({})",
            clickable_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.outline_node_content.name().to_owned())
                .collect::<Vec<_>>()
                .join(",")
        );

        // Step 3: Collect references for outline nodes with diagnostics
        struct LSPDiagnosticErrorOnReference {
            reference_location: ReferenceLocation,
            originating_outline_node: OutlineNodeContent,
        }

        let mut lsp_diagnostic_on_references = Vec::new();
        for clickable_error_places in &clickable_diagnostics {
            let outline_node = &clickable_error_places.outline_node_content;
            let references_for_outline_node = self
                .go_to_references(
                    outline_node.fs_file_path().to_owned(),
                    outline_node.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?
                .locations();

            lsp_diagnostic_on_references.extend(references_for_outline_node.into_iter().map(
                |reference_location| LSPDiagnosticErrorOnReference {
                    reference_location,
                    originating_outline_node: outline_node.clone(),
                },
            ));
        }

        // Step 4: Collect file paths from references to get their diagnostics
        let file_paths_to_diagnostics: HashSet<String> = lsp_diagnostic_on_references
            .iter()
            .map(|diagnostic| diagnostic.reference_location.fs_file_path().to_owned())
            .collect();

        // Step 5: Get diagnostics for reference files
        let diagnostics_on_file_paths = stream::iter(
            file_paths_to_diagnostics
                .clone()
                .into_iter()
                .map(|fs_file_path| (fs_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let diagnostics = self
                .get_lsp_diagnostics_for_files(
                    vec![fs_file_path.to_owned()],
                    message_properties,
                    true,
                )
                .await;
            diagnostics.map(|diagnostics| (fs_file_path, diagnostics))
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<HashMap<String, Vec<LSPDiagnosticError>>>();

        // Step 6: Filter references that correspond to diagnostics
        let lsp_diagnostic_on_references = lsp_diagnostic_on_references
            .into_iter()
            .filter_map(|lsp_diagnostic_on_reference| {
                let reference_location = &lsp_diagnostic_on_reference.reference_location;
                let diagnostics_on_reference_file =
                    diagnostics_on_file_paths.get(reference_location.fs_file_path());

                if let Some(diagnostics_on_reference_file) = diagnostics_on_reference_file {
                    if diagnostics_on_reference_file.iter().any(|diagnostic| {
                        diagnostic
                            .range()
                            .contains_check_line_column(reference_location.range())
                            || reference_location
                                .range()
                                .contains_check_line_column(diagnostic.range())
                    }) {
                        Some(lsp_diagnostic_on_reference)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Step 7: Group by the original outline node
        use std::hash::{Hash, Hasher};

        #[derive(Clone)]
        struct OutlineNodeKey {
            fs_file_path: String,
            identifier_range: Range,
        }

        impl PartialEq for OutlineNodeKey {
            fn eq(&self, other: &Self) -> bool {
                self.fs_file_path == other.fs_file_path
                    && self.identifier_range == other.identifier_range
            }
        }

        impl Eq for OutlineNodeKey {}

        impl Hash for OutlineNodeKey {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.fs_file_path.hash(state);
                self.identifier_range.hash(state);
            }
        }

        let mut lsp_diagnostic_on_references_by_outline_node: HashMap<
            OutlineNodeKey,
            Vec<LSPDiagnosticErrorOnReference>,
        > = HashMap::new();

        for diagnostic_on_reference in lsp_diagnostic_on_references.into_iter() {
            let outline_node = diagnostic_on_reference.originating_outline_node.clone();
            let key = OutlineNodeKey {
                fs_file_path: outline_node.fs_file_path().to_owned(),
                identifier_range: outline_node.identifier_range().clone(),
            };
            lsp_diagnostic_on_references_by_outline_node
                .entry(key)
                .or_insert_with(Vec::new)
                .push(diagnostic_on_reference);
        }

        // Step 8: Create the prompt
        let mut prompt = String::new();

        // create a few new user context variables so we can show that on each step
        // the context we are getting as extra
        let extra_user_variables = stream::iter(
            file_paths_to_diagnostics
                .clone()
                .into_iter()
                .map(|diagnostic_file_path| (diagnostic_file_path, message_properties.clone())),
        )
        .map(|(fs_file_path, message_properties)| async move {
            let file_content = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await;
            file_content.map(|file_content| {
                let language = file_content.language().to_owned();
                VariableInformation::create_file(
                    file_content.full_range(),
                    file_content.fs_file_path().to_owned(),
                    file_content.fs_file_path().to_owned(),
                    file_content.contents(),
                    language,
                )
            })
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<Vec<_>>();

        for (_outline_node_key, lsp_diagnostics) in
            lsp_diagnostic_on_references_by_outline_node.iter()
        {
            let outline_node = lsp_diagnostics
                .first()
                .unwrap()
                .originating_outline_node
                .clone();
            let outline_node_name = outline_node.name();
            let outline_node_type = outline_node.outline_node_type().to_string();
            let outline_node_fs_file_path = outline_node.fs_file_path();

            prompt.push_str(&format!(
                "The {} '{}' in file '{}' has errors in its references:\n",
                outline_node_type, outline_node_name, outline_node_fs_file_path
            ));

            for lsp_diagnostic_on_ref in lsp_diagnostics {
                let reference_location = &lsp_diagnostic_on_ref.reference_location;
                let reference_fs_file_path = reference_location.fs_file_path();
                let reference_range = reference_location.range();

                if let Some(diagnostics) = diagnostics_on_file_paths.get(reference_fs_file_path) {
                    let relevant_diagnostics: Vec<_> = diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic
                                .range()
                                .contains_check_line_column(reference_range)
                                || reference_range.contains_check_line_column(diagnostic.range())
                        })
                        .collect();

                    for diagnostic in relevant_diagnostics {
                        prompt.push_str(&format!(
                            r#"- Error in file '{}' at range L{}-{}:
```
{}
```"#,
                            reference_fs_file_path,
                            diagnostic.range().start_line(),
                            diagnostic.range().end_line(),
                            diagnostic.snippet(),
                        ));
                    }
                }
            }

            prompt.push('\n');
        }
        Ok((extra_user_variables, prompt))
    }

    /// Creates a new exchange for the session, this helps the agent send over
    /// more messages as and when required
    pub async fn create_new_exchange(
        &self,
        session_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let tool_input = ToolInput::NewExchangeDuringSession(SessionExchangeNewRequest::new(
            session_id,
            message_properties.editor_url(),
        ));
        let response = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .new_exchange_response()
            .ok_or(SymbolError::WrongToolOutput)?;
        match response.exchange_id() {
            Some(exchange_id) => Ok(exchange_id),
            // we have a tool output which has gone wrong
            None => Err(SymbolError::WrongToolOutput),
        }
    }

    pub async fn undo_changes_made_during_session(
        &self,
        session_id: String,
        exchange_id: String,
        step_index: Option<usize>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<bool, SymbolError> {
        let tool_input =
            ToolInput::UndoChangesMadeDuringSession(UndoChangesMadeDuringExchangeRequest::new(
                exchange_id,
                session_id,
                step_index,
                message_properties.editor_url(),
            ));
        let response = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_undo_changes_made_during_session()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(response.is_success())
    }

    pub async fn grab_pending_subprocess_output(
        &self,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Option<String>, SymbolError> {
        let tool_input = ToolInput::SubProcessSpawnedPendingOutput(
            SubProcessSpawnedPendingOutputRequest::with_editor_url(message_properties.editor_url()),
        );
        let response = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_pending_spawned_process_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        Ok(response.output())
    }
}