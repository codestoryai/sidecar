use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use futures::{stream, StreamExt};
use llm_client::clients::types::LLMType;
use llm_client::provider::{
    AnthropicAPIKey, FireworksAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::agentic::symbol::helpers::{
    apply_inlay_hints_to_code, search_and_replace_generator, split_file_content_into_parts,
};
use crate::agentic::symbol::identifier::{Snippet, SymbolIdentifier};
use crate::agentic::tool::code_edit::filter_edit::{
    FilterEditOperationRequest, FilterEditOperationResponse,
};
use crate::agentic::tool::code_edit::search_and_replace::SearchAndReplaceEditingRequest;
use crate::agentic::tool::code_edit::test_correction::TestOutputCorrectionRequest;
use crate::agentic::tool::code_edit::types::CodeEdit;
use crate::agentic::tool::code_symbol::apply_outline_edit_to_range::ApplyOutlineEditsToRangeRequest;
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
use crate::agentic::tool::code_symbol::should_edit::ShouldEditCodeSymbolRequest;
use crate::agentic::tool::editor::apply::{EditorApplyRequest, EditorApplyResponse};
use crate::agentic::tool::errors::ToolError;
use crate::agentic::tool::filtering::broker::{
    CodeToEditFilterRequest, CodeToEditSymbolRequest, CodeToEditSymbolResponse,
    CodeToProbeFilterResponse, CodeToProbeSubSymbolList, CodeToProbeSubSymbolRequest,
};
use crate::agentic::tool::git::diff_client::{GitDiffClientRequest, GitDiffClientResponse};
use crate::agentic::tool::grep::file::{FindInFileRequest, FindInFileResponse};
use crate::agentic::tool::lsp::diagnostics::{
    Diagnostic, LSPDiagnosticsInput, LSPDiagnosticsOutput,
};
use crate::agentic::tool::lsp::gotodefintion::{
    DefinitionPathAndRange, GoToDefinitionRequest, GoToDefinitionResponse,
};
use crate::agentic::tool::lsp::gotoimplementations::{
    GoToImplementationRequest, GoToImplementationResponse,
};
use crate::agentic::tool::lsp::gotoreferences::{GoToReferencesRequest, GoToReferencesResponse};
use crate::agentic::tool::lsp::grep_symbol::{
    LSPGrepSymbolInCodebaseRequest, LSPGrepSymbolInCodebaseResponse,
};
use crate::agentic::tool::lsp::inlay_hints::InlayHintsRequest;
use crate::agentic::tool::lsp::open_file::OpenFileResponse;
use crate::agentic::tool::lsp::quick_fix::{
    GetQuickFixRequest, GetQuickFixResponse, LSPQuickFixInvocationRequest,
    LSPQuickFixInvocationResponse, QuickFixOption,
};
use crate::agentic::tool::r#type::Tool;
use crate::agentic::tool::swe_bench::test_tool::{SWEBenchTestRepsonse, SWEBenchTestRequest};
use crate::chunking::editor_parsing::EditorParsing;
use crate::chunking::text_document::{Position, Range};
use crate::chunking::types::{OutlineNode, OutlineNodeContent};
use crate::repomap::tag::TagIndex;
use crate::repomap::types::RepoMap;
use crate::user_context::types::UserContext;
use crate::{
    agentic::tool::{broker::ToolBroker, input::ToolInput, lsp::open_file::OpenFileRequest},
    inline_completion::symbols_tracker::SymbolTrackerInline,
};

use super::errors::SymbolError;
use super::events::edit::{SymbolToEdit, SymbolToEditRequest};
use super::events::initial_request::{SymbolEditedItem, SymbolRequestHistoryItem};
use super::events::message_event::{SymbolEventMessage, SymbolEventMessageProperties};
use super::events::probe::{SubSymbolToProbe, SymbolToProbeRequest};
use super::helpers::{find_needle_position, generate_hyperlink_from_snippet};
use super::identifier::{LLMProperties, MechaCodeSymbolThinking};
use super::tool_properties::ToolProperties;
use super::toolbox::helpers::{SymbolChangeSet, SymbolChanges};
use super::types::SymbolEventRequest;
use super::ui_event::UIEventWithID;

#[derive(Clone)]
pub struct ToolBox {
    tools: Arc<ToolBroker>,
    symbol_broker: Arc<SymbolTrackerInline>,
    editor_parsing: Arc<EditorParsing>,
}

impl ToolBox {
    pub fn new(
        tools: Arc<ToolBroker>,
        symbol_broker: Arc<SymbolTrackerInline>,
        editor_parsing: Arc<EditorParsing>,
    ) -> Self {
        Self {
            tools,
            symbol_broker,
            editor_parsing,
        }
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
        // send an open file request here first
        let _ = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let outline_node_possible = self
            .symbol_broker
            .get_symbols_outline(fs_file_path)
            .await
            .ok_or(SymbolError::ExpectedFileToExist)?
            .into_iter()
            .find(|outline_node| outline_node.name() == symbol_name);
        if let Some(outline_node) = outline_node_possible {
            // we check for 2 things here:
            // - its either a function or a class like symbol
            // - if its a function no need to check for implementations
            // - if its a class then we still need to check for implementations
            if outline_node.is_funciton() {
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
                // we need to check for implementations as well and then return it
                let identifier_position = outline_node.identifier_range();
                // now we go to the implementations using this identifier node
                let identifier_node_positions = self
                    .go_to_implementations_exact(
                        fs_file_path,
                        &identifier_position.start_position(),
                        message_properties.clone(),
                    )
                    .await?
                    .remove_implementations_vec();
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
                // Now all files are opened so we have also parsed them in the symbol broker
                // so we can grab the appropriate outlines properly over here
                let file_path_to_outline_nodes = stream::iter(file_paths)
                    .map(|fs_file_path| async move {
                        let symbols = self.symbol_broker.get_symbols_outline(&fs_file_path).await;
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
            .filter(|outline_node| outline_node.name() == parent_symbol_name)
            .collect::<Vec<_>>();

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
                    return Ok(outline_nodes_with_distance.remove(0).1.content().clone());
                }
            }
        }
    }

    pub fn detect_language(&self, fs_file_path: &str) -> Option<String> {
        self.editor_parsing
            .for_file_path(fs_file_path)
            .map(|ts_language_config| ts_language_config.language_str.to_owned())
    }

    pub async fn check_for_followups(
        &self,
        parent_symbol_name: &str,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        // very early return over here for now
        return Ok(());
        println!(
            "tool_box::check_for_followups::start::symbol({})",
            parent_symbol_name
        );
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
            "tool_box::check_for_followups::find_sub_symbol_edited::({})::({})",
            parent_symbol_name,
            symbol_edited.symbol_name()
        );
        let outline_node = self
            .find_sub_symbol_to_edit_with_name(
                parent_symbol_name,
                symbol_edited,
                message_properties.clone(),
            )
            .await?;
        println!(
            "tool_box::check_for_followups::found_sub_symbol_edited::parent_symbol_name({})::symbol_edited({})",
            parent_symbol_name,
            symbol_edited.symbol_name(),
        );
        // over here we have to check if its a function or a class
        if outline_node.is_function_type() {
            println!(
                "tool_box::check_for_followups::is_function_type::parent_symbol_name({})::symbol_to_edit({})",
                parent_symbol_name,
                outline_node.name(),
            );
            // we do need to get the references over here for the function and
            // send them over as followups to check wherever they are being used
            let references = self
                .go_to_references(
                    symbol_edited.fs_file_path(),
                    &outline_node.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?;
            let _ = self
                .invoke_followup_on_references(
                    symbol_edited,
                    original_code,
                    &outline_node,
                    references,
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
            // TODO(skcd): There are several cases here which we can handle:
            // - we might have changed the definition of the symbol itself
            // -  we could have an enum over here and added a new entry at this point, so
            // we have to follow the symbol itself and go to tht references for it
            // - so the hard part here is deciding between the following:
            // - do we follow individual parts of the class type definitions
            // - or do we follow the symbol itself and check things out
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
                    symbol_edited.fs_file_path(),
                    &outline_node.identifier_range().start_position(),
                    message_properties.clone(),
                )
                .await?;

            println!(
                "references from go_to_references: \n\n{}",
                references
                    .clone()
                    .locations()
                    .iter()
                    .map(|loc| format!("{}", loc.fs_file_path()))
                    .collect::<Vec<String>>()
                    .join("\n")
            );

            let _ = self
                .invoke_followup_on_references(
                    symbol_edited,
                    original_code,
                    &outline_node,
                    references,
                    hub_sender,
                    message_properties,
                    tool_properties,
                )
                .await;
        } else {
            println!(
                "tool_box::check_for_followups::found_sub_symbol_edited::no_branch::({})::({}:{:?})",
                parent_symbol_name,
                outline_node.name(),
                outline_node.outline_node_type()
            );
            // something else over here, wonder what it could be
            return Err(SymbolError::NoContainingSymbolFound);
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
            .go_to_references(fs_file_path, &position, message_properties.clone())
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
                        let event = SymbolEventMessage::new(
                            SymbolEventRequest::ask_question(
                                SymbolIdentifier::with_file_path(
                                    outline_node.name(),
                                    outline_node.fs_file_path(),
                                ),
                                instruction_prompt,
                                tool_properties.clone(),
                            ),
                            message_properties
                                .request_id()
                                .clone()
                                .set_request_id(uuid::Uuid::new_v4().to_string()),
                            message_properties.ui_sender(),
                            sender,
                            message_properties.editor_url(),
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

    async fn invoke_followup_on_references(
        &self,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        original_symbol: &OutlineNodeContent,
        references: GoToReferencesResponse,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        println!("============");
        println!(
            "toolbox::invoke_foolowup_on_references: {}",
            symbol_edited.symbol_name()
        );
        println!("============");

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

        let edited_code = original_symbol.content();
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
                            message_properties.clone(),
                        ))
                    } else {
                        None
                    }
                })
                .map(
                    |(_fs_file_path, ranges, hub_sender, outline_nodes, message_properties)| {
                        ranges
                            .into_iter()
                            .map(|range| {
                                (
                                    range,
                                    hub_sender.clone(),
                                    outline_nodes.to_vec(),
                                    message_properties.clone(),
                                )
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .flatten(),
        )
        .map(
            |(range, hub_sender, outline_nodes, message_properties)| async move {
                self.send_request_for_followup(
                    original_code,
                    edited_code,
                    symbol_edited,
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

    fn create_instruction_prompt_for_followup(
        &self,
        original_code: &str,
        edited_code: &str,
        symbol_edited: &SymbolToEdit,
        child_symbol: &OutlineNodeContent,
        file_path_for_followup: &str,
        symbol_content_with_highlight: String,
    ) -> String {
        let symbol_edited_name = symbol_edited.symbol_name();
        let symbol_fs_file_path = symbol_edited.fs_file_path();
        let instructions = symbol_edited.instructions().join("\n");
        let child_symbol_name = child_symbol.name();
        format!(
            r#"Another engineer has changed the code for `{symbol_edited_name}` which is present in `{symbol_fs_file_path}`
The original code for `{symbol_edited_name}` is given below along with the new code and the instructions for why the change was done:
<old_code>
{original_code}
</old_code>

<new_code>
{edited_code}
</new_code>

<instructions_for_change>
{instructions}
</instructions_for_change>

The `{symbol_edited_name}` is being used in `{child_symbol_name}` in the following line:
<file_path>
{file_path_for_followup}
</file_path>
<content>
{symbol_content_with_highlight}
</content>

There might be need for futher changes to the `{child_symbol_name}`
Please handle these changes as required."#
        )
    }

    // we need to search for the smallest node which contains this position or range
    async fn send_request_for_followup(
        &self,
        original_code: &str,
        edited_code: &str,
        symbol_to_edit: &SymbolToEdit,
        position_to_search: Position,
        // This is pretty expensive to copy again and again
        outline_nodes: Vec<OutlineNode>,
        // this is becoming annoying now cause we will need a drain for this while
        // writing a unit-test for this
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        println!("=====================");
        println!(
            "sending request for follow up. Symbol to edit: {}",
            symbol_to_edit.symbol_name()
        );
        println!("=====================");
        let outline_node_possible = outline_nodes.into_iter().find(|outline_node| {
            // we need to check if the outline node contains the range we are interested in
            outline_node.range().contains(&Range::new(
                position_to_search.clone(),
                position_to_search.clone(),
            ))
        });
        match outline_node_possible {
            Some(outline_node) => {
                println!("outline node found: {}", &outline_node.name());
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
                if let Some(_definition) = definitions.definitions().get(0) {
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
                        let _instruction_prompt = self.create_instruction_prompt_for_followup(
                            original_code,
                            edited_code,
                            symbol_to_edit,
                            &child_node,
                            &format!(
                                "{}-{}:{}",
                                child_node.fs_file_path(),
                                child_node.range().start_line(),
                                child_node.range().end_line()
                            ),
                            symbol_content_to_send,
                        );
                        // now we can send it over to the hub sender for handling the change
                        let (sender, receiver) = tokio::sync::oneshot::channel();

                        println!("original code: \n{}", original_code);
                        println!("=========");
                        println!("edited code: \n{}", edited_code);

                        let event = SymbolEventMessage::message_with_properties(
                            SymbolEventRequest::initial_request(
                                SymbolIdentifier::with_file_path(
                                    outline_node.name(),
                                    outline_node.fs_file_path(),
                                ),
                                // todo(skcd) can we do sthing heree...
                                format!(
                                    "This class has changed.\nFrom: {}\nTo: {}\n\nGiven that you depend on it, ensure necessary changes are made to accommodate the changes.",
                                    original_code, edited_code
                                ), // original user query - should be some custom prompt?
                                message_properties.request_id_str().to_string(),
                                vec![], // history...
                                tool_properties.clone(),
                                Some(vec![]),
                                false,
                            ),
                            message_properties.clone(),
                            sender,
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
                println!(
                    "could not find outline node for {}",
                    &symbol_to_edit.symbol_name()
                );
                return Err(SymbolError::NoOutlineNodeSatisfyPosition);
            }
        }
    }

    pub async fn go_to_references(
        &self,
        fs_file_path: &str,
        position: &Position,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<GoToReferencesResponse, SymbolError> {
        let input = ToolInput::GoToReference(GoToReferencesRequest::new(
            fs_file_path.to_owned(),
            position.clone(),
            message_properties.editor_url().to_owned(),
        ));

        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_references()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn swe_bench_test_tool(
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

    /// Check if we need to make changes more than just the code addition
    /// This implies that we are going to make changes around the codebase for the code
    /// addition, instead of just adding code to a particular part of the symbol
    async fn should_apply_code_changes_code_addition(
        &self,
        edited_code: &str,
        symbol_edited: &SymbolToEdit,
        parent_symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<bool, SymbolError> {
        let fs_file_path = symbol_edited.fs_file_path();
        // we are going to parse the edited code and get the outline nodes for it
        let ts_language_parsing = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .ok_or(SymbolError::FileTypeNotSupported(fs_file_path.to_owned()))?;
        let edited_code_outline_nodes =
            ts_language_parsing.generate_outline_fresh(edited_code.as_bytes(), fs_file_path);

        // Here is where things get tricky, we have a parent node we want to apply
        // changes to and various other nodes which might or might not be present
        // in the scope of the symbol which needs to edited
        // our assumption right now is that:
        // no matter the nodes which are generated we do the insertions or the apply the changes
        // as directed by the edited code and let the symbols self-heal cause they will get invoked
        // as part of the code-correctness or the plan
        // code-addition never generates code which is wrong or partial and only complete code

        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let file_outline_nodes = ts_language_parsing
            .generate_outline_fresh(file_content.contents_ref().as_bytes(), fs_file_path);

        // 2 step process now with various cases:
        // - if the edited code has a new new outline node, we need to insert it always (new outline node and for the children as well)
        // - if the edited code has any outline nodes which already exist, we want to overwrite it

        // outline nodes which require a child insertion
        let outline_node_child_addition = edited_code_outline_nodes
            .iter()
            .filter_map(|edited_code_outline_node| {
                let matching_file_outline_node = file_outline_nodes
                    .iter()
                    .filter(|file_outline_node| {
                        file_outline_node.name() == edited_code_outline_node.name()
                            && file_outline_node.is_class()
                    })
                    .collect::<Vec<_>>();
                if !matching_file_outline_node.is_empty() {
                    let new_added_nodes = edited_code_outline_node
                        .children()
                        .iter()
                        .filter(|child_node| {
                            !matching_file_outline_node
                                .iter()
                                .map(|file_outline_node| file_outline_node.children())
                                .flatten()
                                .any(|file_child_node| file_child_node.name() == child_node.name())
                        })
                        .collect::<Vec<_>>();
                    Some((matching_file_outline_node, new_added_nodes))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // outline nodes which have changed now
        // we do not consider child node changes over here, only the main symbol
        // which changed
        // one of the tricks which we can use here is:
        // the content of the changed node should not be empty or an empty string
        // we can comapre it on the length because we could have deletions which happened
        // to the definition, but since this is addition (and only addition) we can be pretty
        // sure that our length of the outline node will be > (going for strictly greater) original length of the outline node
        let outline_nodes_which_changed_vec = edited_code_outline_nodes
            .iter()
            .filter_map(|edited_code_outline_node| {
                let matching_file_outline_node =
                    file_outline_nodes.iter().find(|file_outline_node| {
                        // find the class definition
                        file_outline_node.name() == edited_code_outline_node.name()
                            && file_outline_node.is_class_definition()
                    });
                if let Some(file_outline_node) = matching_file_outline_node {
                    if edited_code_outline_node.content().content().len()
                        > file_outline_node.content().content().len()
                    {
                        Some((file_outline_node, edited_code_outline_node))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // check the state here because we will start applying changes to the editor and
        // if we changed the node content for an outline node
        let child_added = outline_node_child_addition
            .iter()
            .any(|outline_node_child_add| {
                let parent_symbol_name_any = outline_node_child_add
                    .0
                    .iter()
                    .any(|outline_node| outline_node.name() == parent_symbol_name);
                parent_symbol_name_any
            });
        let outline_node_changed = outline_nodes_which_changed_vec
            .iter()
            .any(|outline_node_changed| outline_node_changed.0.name() == parent_symbol_name);

        // we want something related to the parent symbol to change
        Ok(child_added || outline_node_changed)
    }

    /// Takes as input the edited code and the symbol which needs to be edited
    /// and applies the changes
    /// This can lead to multiple outline nodes which need to be checked but for now
    /// we will focus only on the symbols which are part of the parent symbol
    async fn apply_code_changes_code_addition(
        &self,
        edited_code: &str,
        symbol_edited: &SymbolToEdit,
        parent_symbol_name: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Option<OutlineNodeContent>, SymbolError> {
        let fs_file_path = symbol_edited.fs_file_path();
        // we are going to parse the edited code and get the outline nodes for it
        let ts_language_parsing = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .ok_or(SymbolError::FileTypeNotSupported(fs_file_path.to_owned()))?;
        let edited_code_outline_nodes =
            ts_language_parsing.generate_outline_fresh(edited_code.as_bytes(), fs_file_path);

        // Here is where things get tricky, we have a parent node we want to apply
        // changes to and various other nodes which might or might not be present
        // in the scope of the symbol which needs to edited
        // our assumption right now is that:
        // no matter the nodes which are generated we do the insertions or the apply the changes
        // as directed by the edited code and let the symbols self-heal cause they will get invoked
        // as part of the code-correctness or the plan
        // code-addition never generates code which is wrong or partial and only complete code

        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let file_outline_nodes = ts_language_parsing
            .generate_outline_fresh(file_content.contents_ref().as_bytes(), fs_file_path);

        // 2 step process now with various cases:
        // - if the edited code has a new new outline node, we need to insert it always (new outline node and for the children as well)
        // - if the edited code has any outline nodes which already exist, we want to overwrite it

        // this will have a bunch of bugs:
        // - we have functions which need to be implemented and added for outline nodes
        // - there are repetitions of the outline nodes
        let outline_nodes_which_are_fresh = edited_code_outline_nodes
            .iter()
            .filter(|edited_code_outline_node| {
                let node_name = edited_code_outline_node.name();
                // check if outline node does not exist
                !file_outline_nodes
                    .iter()
                    .any(|file_outline_node| file_outline_node.name() == node_name)
            })
            .collect::<Vec<_>>();

        // outline nodes which require a child insertion
        let outline_node_child_addition = edited_code_outline_nodes
            .iter()
            .filter_map(|edited_code_outline_node| {
                let matching_file_outline_node = file_outline_nodes
                    .iter()
                    .filter(|file_outline_node| {
                        file_outline_node.name() == edited_code_outline_node.name()
                            && file_outline_node.is_class()
                    })
                    .collect::<Vec<_>>();
                if !matching_file_outline_node.is_empty() {
                    let new_added_nodes = edited_code_outline_node
                        .children()
                        .iter()
                        .filter(|child_node| {
                            !matching_file_outline_node
                                .iter()
                                .map(|file_outline_node| file_outline_node.children())
                                .flatten()
                                .any(|file_child_node| file_child_node.name() == child_node.name())
                        })
                        .collect::<Vec<_>>();
                    Some((matching_file_outline_node, new_added_nodes))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // outline nodes which have changed now
        // we do not consider child node changes over here, only the main symbol
        // which changed
        // one of the tricks which we can use here is:
        // the content of the changed node should not be empty or an empty string
        // we can comapre it on the length because we could have deletions which happened
        // to the definition, but since this is addition (and only addition) we can be pretty
        // sure that our length of the outline node will be > (going for strictly greater) original length of the outline node
        let outline_nodes_which_changed_vec = edited_code_outline_nodes
            .iter()
            .filter_map(|edited_code_outline_node| {
                let matching_file_outline_node =
                    file_outline_nodes.iter().find(|file_outline_node| {
                        // find the class definition
                        file_outline_node.name() == edited_code_outline_node.name()
                            && file_outline_node.is_class_definition()
                    });
                if let Some(file_outline_node) = matching_file_outline_node {
                    if edited_code_outline_node.content().content().len()
                        > file_outline_node.content().content().len()
                    {
                        Some((file_outline_node, edited_code_outline_node))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // check the state here because we will start applying changes to the editor and
        // if we changed the node content for an outline node
        let child_added = outline_node_child_addition
            .iter()
            .any(|outline_node_child_add| {
                let parent_symbol_name_any = outline_node_child_add
                    .0
                    .iter()
                    .any(|outline_node| outline_node.name() == parent_symbol_name);
                parent_symbol_name_any
            });
        let outline_node_changed = outline_nodes_which_changed_vec
            .iter()
            .any(|outline_node_changed| outline_node_changed.0.name() == parent_symbol_name);

        // we want something related to the parent symbol to change
        if !child_added && !outline_node_changed {
            println!(
                "tool_box::apply_code_changes_code_addition::no_parent_symbol_changed::({})",
                parent_symbol_name
            );
            return Ok(None);
        }

        // we start the code insertions now
        // adding the fresh nodes to the end of the file
        for fresh_outline_node in outline_nodes_which_are_fresh.into_iter() {
            let fs_file_path = fresh_outline_node.fs_file_path();
            let file_content = self
                .file_open(
                    fresh_outline_node.fs_file_path().to_owned(),
                    message_properties.clone(),
                )
                .await?;
            let file_length = file_content
                .contents()
                .lines()
                .into_iter()
                .collect::<Vec<_>>()
                .len();
            let range = Range::new(
                Position::new(file_length, 0, 0),
                Position::new(file_length, 0, 0),
            );
            println!(
                "tool_box::apply_code_changes_code_addition::fresh_outline_node::({})::({})",
                parent_symbol_name,
                fresh_outline_node.name()
            );
            let _ = self
                .apply_edits_to_editor(
                    fs_file_path,
                    &range,
                    fresh_outline_node.content().content(),
                    false,
                    message_properties.clone(),
                )
                .await;
        }

        let mut sub_symbols_added = vec![];

        for outline_node_child_addition in outline_node_child_addition.into_iter() {
            let parent_outline_nodes = outline_node_child_addition.0;
            let child_nodes = outline_node_child_addition.1;
            // find position where we want to add children
            let parent_outline_node = parent_outline_nodes.get(0).map(|outline_node| outline_node);
            if let None = parent_outline_node {
                continue;
            } else {
                let parent_outline_node = parent_outline_node.expect("if let None");
                if parent_outline_node.name() == parent_symbol_name {
                    sub_symbols_added = child_nodes
                        .iter()
                        .map(|child_node| child_node.name().to_owned())
                        .collect::<Vec<_>>();
                }
                let fs_file_path = parent_outline_node.fs_file_path();
                let file_content = self
                    .file_open(fs_file_path.to_owned(), message_properties.clone())
                    .await?;
                let file_outline_nodes_maybe =
                    self.editor_parsing
                        .for_file_path(fs_file_path)
                        .map(|ts_language_parsing| {
                            ts_language_parsing.generate_outline_fresh(
                                file_content.contents_ref().as_bytes(),
                                fs_file_path,
                            )
                        });
                if let None = file_outline_nodes_maybe {
                    continue;
                }
                let file_outline_nodes = file_outline_nodes_maybe.expect("if let None to hold");
                let insert_position = file_outline_nodes
                    .into_iter()
                    .filter(|outline_node| {
                        outline_node.name() == parent_outline_node.name()
                            && outline_node.is_class()
                            // we do not want to edit the trait implementations on addition yet
                            // this is wrong tho, since we could have changed the trait itself and need
                            // to add a new function
                            && outline_node.content().has_trait_implementation().is_none()
                    })
                    .next()
                    .map(|outline_node| outline_node.range().end_position());
                if let Some(end_position) = insert_position {
                    println!("tool_box::apply_code_changes_code_addition::outline_node_child_added::({})::({})", parent_symbol_name, child_nodes.iter().map(|child_node| child_node.name()).collect::<Vec<_>>().join(","));
                    let _ = self
                        .apply_edits_to_editor(
                            fs_file_path,
                            &Range::new(end_position.clone(), end_position),
                            &child_nodes
                                .into_iter()
                                .map(|child_node| child_node.content())
                                .collect::<Vec<_>>()
                                .join("\n"),
                            false,
                            message_properties.clone(),
                        )
                        .await;
                }
            }
        }

        for outline_node_which_changed in outline_nodes_which_changed_vec.into_iter() {
            let edited_node = outline_node_which_changed.1;
            let fs_file_path = edited_node.fs_file_path().to_owned();
            let file_content = self
                .file_open(fs_file_path.to_owned(), (&message_properties).clone())
                .await?;
            let ts_language_config = self.editor_parsing.for_file_path(&fs_file_path);
            if let None = ts_language_config {
                continue;
            }
            let ts_language_config = ts_language_config.expect("if let None to hold");
            let implementation_outline_node_maybe = ts_language_config
                .generate_outline_fresh(file_content.contents_ref().as_bytes(), &fs_file_path)
                .into_iter()
                .find(|outline_node| {
                    outline_node.name() == edited_node.name() && outline_node.is_class()
                });
            if let None = implementation_outline_node_maybe {
                continue;
            }
            let implementation_outline_node =
                implementation_outline_node_maybe.expect("if let None to hold");
            println!(
                "tool_box::apply_code_changes_code_addition::outline_node_which_changed::({})::({})",
                parent_symbol_name,
                implementation_outline_node.name(),
            );
            let _ = self
                .apply_edits_to_editor(
                    &fs_file_path,
                    implementation_outline_node.range(),
                    edited_node.content().content(),
                    false,
                    message_properties.clone(),
                )
                .await;
        }

        // At the end of this function we have to get back the outline nodes contained
        // in the parent symbol name which changed and belongs to the parent node
        // since there can be multiple symbols and we DO NOT support that flow
        // right now we will pick the first symbol and run with it
        if sub_symbols_added.is_empty() {
            Ok(None)
        } else {
            let file_content = self
                .file_open(fs_file_path.to_owned(), message_properties)
                .await?;
            let ts_language_config = self.editor_parsing.for_file_path(fs_file_path);
            if ts_language_config.is_none() {
                // return early over here
                return Ok(None);
            }
            let ts_language_config = ts_language_config.expect("is_none to hold");
            let child_outline_node = ts_language_config
                .generate_outline_fresh(file_content.contents_ref().as_bytes(), fs_file_path)
                .into_iter()
                .filter(|outline_node| {
                    outline_node.name() == parent_symbol_name && outline_node.is_class()
                })
                .filter_map(|outline_node| {
                    let outline_node_maybe = outline_node
                        .children()
                        .iter()
                        .find(|children_node| {
                            if sub_symbols_added
                                .iter()
                                .any(|sub_symbol_name| sub_symbol_name == children_node.name())
                            {
                                true
                            } else {
                                false
                            }
                        })
                        .map(|outline_node_content| outline_node_content.clone());
                    outline_node_maybe
                })
                .next();
            Ok(child_outline_node)
        }
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
        original_code: &str,
        // This is the edited code we are applying to the editor
        edited_code: &str,
        // this is the context from the code edit which we want to keep using while
        // fixing
        code_edit_extra_context: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        tool_properties: &ToolProperties,
        history: Vec<SymbolRequestHistoryItem>,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        // code correction looks like this:
        // - apply the edited code to the original selection
        // - look at the code actions which are available
        // - take one of the actions or edit code as required
        // - once we have no LSP errors or anything we are good
        let instructions = symbol_edited.instructions().join("\n");
        let fs_file_path = symbol_edited.fs_file_path();
        let extra_symbol_list = tool_properties.get_plan_for_input();
        let extra_symbol_list_ref = extra_symbol_list.as_deref();
        let symbol_name = symbol_edited.symbol_name();
        let mut updated_code = edited_code.to_owned();
        let mut tries = 0;
        let max_tries = 5;
        loop {
            // keeping a try counter
            if tries >= max_tries {
                break;
            }
            tries = tries + 1;

            let should_apply_code_changes_for_addition = symbol_edited.is_new()
                && self
                    .should_apply_code_changes_code_addition(
                        edited_code,
                        symbol_edited,
                        parent_symbol_name,
                        message_properties.clone(),
                    )
                    .await?;

            let lsp_request_id = uuid::Uuid::new_v4().to_string();

            if should_apply_code_changes_for_addition {
                let _ = self
                    .apply_code_changes_code_addition(
                        edited_code,
                        symbol_edited,
                        parent_symbol_name,
                        message_properties.clone(),
                    )
                    .await;
            }

            // TODO(codestory): just make this branch false so we always use
            // the previous flow
            let symbol_to_edit = {
                let symbol_to_edit_range = self
                    .find_sub_symbol_to_edit_with_name(
                        parent_symbol_name,
                        symbol_edited,
                        message_properties.clone(),
                    )
                    .await
                    .map(|outline_node| outline_node.range().clone())
                    // If its a new symbol we still do not have it in our outline yet, so
                    // we should grab it from the range position provided in the edit request
                    .unwrap_or(symbol_edited.range().clone());
                let _fs_file_content = self
                    .file_open(fs_file_path.to_owned(), message_properties.clone())
                    .await?
                    .contents();

                // The range of the symbol before doing the edit
                let edited_range = symbol_to_edit_range;
                let _editor_response = self
                    .apply_edits_to_editor(
                        fs_file_path,
                        &edited_range,
                        &updated_code,
                        false,
                        message_properties.clone(),
                    )
                    .await?;

                // after applying the edits to the editor, we will need to get the file
                // contents and the symbol again
                let symbol_to_edit = self
                    .find_sub_symbol_to_edit_with_name(
                        parent_symbol_name,
                        symbol_edited,
                        message_properties.clone(),
                    )
                    .await;
                symbol_to_edit
            }?;

            let fs_file_content = self
                .file_open(fs_file_path.to_owned(), message_properties.clone())
                .await?
                .contents();

            // After applying the changes we get the new range for the symbol
            let edited_range = symbol_to_edit.range().clone();
            // In case we have swe-bench-tooling enabled over here we should run
            // the tests first, since we get enough value out if to begin with
            // TODO(skcd): Use the test output for debugging over here
            let test_output_maybe = if let Some(swe_bench_test_endpoint) =
                tool_properties.get_swe_bench_test_endpoint()
            {
                let swe_bench_test_output =
                    self.swe_bench_test_tool(&swe_bench_test_endpoint).await?;
                // Pass the test output through for checking the correctness of
                // this code
                Some(swe_bench_test_output)
            } else {
                None
            };

            // TODO(skcd): Figure out what should we do over here for tracking
            // the range of the symbol
            if let Some(test_output) = test_output_maybe {
                // Check the action to take using the test output here or should
                // we also combine the lsp diagnostics over here as well???
                let test_output_logs = test_output.test_output();
                let tests_passed = test_output.passed();
                if tests_passed && test_output_logs.is_none() {
                    // We passed! we can keep going
                    println!("tool_box::check_code_correctness::test_passed");
                    return Ok(());
                } else {
                    if let Some(test_output_logs) = test_output_logs {
                        let corrected_code = self
                            .fix_tests_by_editing(
                                fs_file_path,
                                &fs_file_content,
                                &edited_range,
                                instructions.to_owned(),
                                code_edit_extra_context,
                                original_code,
                                self.detect_language(fs_file_path).unwrap_or("".to_owned()),
                                test_output_logs,
                                llm.clone(),
                                provider.clone(),
                                api_keys.clone(),
                                message_properties.clone(),
                            )
                            .await;

                        if corrected_code.is_err() {
                            println!("tool_box::check_code_correctness::missing_xml_tag");
                            continue;
                        }
                        let corrected_code = corrected_code.expect("is_err above to hold");
                        // update our edited code
                        updated_code = corrected_code.to_owned();
                        // grab the symbol again since the location might have
                        // changed between invocations of the code
                        let symbol_to_edit = self
                            .find_sub_symbol_to_edit_with_name(
                                parent_symbol_name,
                                symbol_edited,
                                message_properties.clone(),
                            )
                            .await?;

                        // Now that we have the corrected code, we should again apply
                        // it to the file
                        let _ = self
                            .apply_edits_to_editor(
                                fs_file_path,
                                symbol_to_edit.range(),
                                &corrected_code,
                                false,
                                message_properties.clone(),
                            )
                            .await;

                        // return over here since we are done with the code correction flow
                        // since this only happens in case of swe-bench
                        continue;
                    }
                }
            }

            // Now we check for LSP diagnostics
            let lsp_diagnostics = self
                .get_lsp_diagnostics(fs_file_path, &edited_range, message_properties.clone())
                .await?;

            // We also give it the option to edit the code as required
            if lsp_diagnostics.get_diagnostics().is_empty() {
                break;
            }

            // TODO(skcd): We should format the diagnostics properly over here
            // with some highlight from the lines above and below so we can show
            // a more detailed output to the model

            // Now we get all the quick fixes which are available in the editor
            let quick_fix_actions = self
                .get_quick_fix_actions(
                    fs_file_path,
                    &edited_range,
                    lsp_request_id.to_owned(),
                    message_properties.clone(),
                )
                .await?
                .remove_options();

            // now we can send over the request to the LLM to select the best tool
            // for editing the code out
            let selected_action = self
                .code_correctness_action_selection(
                    fs_file_path,
                    &fs_file_content,
                    &edited_range,
                    symbol_name,
                    &instructions,
                    original_code,
                    lsp_diagnostics.remove_diagnostics(),
                    quick_fix_actions.to_vec(),
                    llm.clone(),
                    provider.clone(),
                    api_keys.clone(),
                    extra_symbol_list_ref,
                    message_properties.clone(),
                )
                .await?;

            // Now that we have the selected action, we can chose what to do about it
            // there might be a case that we have to re-write the code completely, since
            // the LLM thinks that the best thing to do, or invoke one of the quick-fix actions
            let selected_action_index = selected_action.index();
            let tool_use_thinking = selected_action.thinking();
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::code_correctness_action(
                    message_properties.request_id_str().to_owned(),
                    symbol_identifier.clone(),
                    edited_range.clone(),
                    fs_file_path.to_owned(),
                    tool_use_thinking.to_owned(),
                ));

            // TODO(skcd): This needs to change because we will now have 3 actions which can
            // happen
            // code edit is a special operation which is not present in the quick-fix
            // but is provided by us, the way to check this is by looking at the index and seeing
            // if its >= length of the quick_fix_actions (we append to it internally in the LLM call)
            if selected_action_index == quick_fix_actions.len() as i64 {
                let fixed_code = self
                    .code_correctness_with_edits(
                        fs_file_path,
                        &fs_file_content,
                        symbol_to_edit.range(),
                        code_edit_extra_context.to_owned(),
                        selected_action.thinking(),
                        &instructions,
                        original_code,
                        llm.clone(),
                        provider.clone(),
                        api_keys.clone(),
                        message_properties.clone(),
                    )
                    .await?;

                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::edited_code(
                        message_properties.request_id_str().to_owned(),
                        symbol_identifier.clone(),
                        edited_range.clone(),
                        fs_file_path.to_owned(),
                        fixed_code.to_owned(),
                    ));

                // after this we have to apply the edits to the editor again and being
                // the loop again
                let _ = self
                    .apply_edits_to_editor(
                        fs_file_path,
                        &edited_range,
                        &fixed_code,
                        false,
                        message_properties.clone(),
                    )
                    .await?;
            } else if selected_action_index == quick_fix_actions.len() as i64 + 1 {
                // over here we want to ping the other symbols and send them requests, there is a search
                // step with some thinking involved, can we illicit this behavior somehow in the previous invocation
                // or maybe we should keep it separate
                // TODO(skcd): Figure this part out
                // 1. First we figure out if the code symbol exists in the codebase
                // 2. If it does exist then we know the action we want to  invoke on it
                // 3. If the symbol does not exist, then we need to go through the creation loop
                // where should that happen?
                println!("tool_box::check_code_correctness::changes_to_codebase");
                let edit_request_sent = self
                    .code_correctness_changes_to_codebase(
                        parent_symbol_name,
                        fs_file_path,
                        &edited_range,
                        &updated_code,
                        &tool_use_thinking,
                        tool_properties,
                        LLMProperties::new(llm.clone(), provider.clone(), api_keys.clone()),
                        history.to_vec(),
                        hub_sender.clone(),
                        message_properties.clone(),
                    )
                    .await;
                // if no edits were done to the codebase, then we can break from the
                // code correction loop and move forward as there is no more action to take
                if let Ok(false) = edit_request_sent {
                    break;
                }
            } else if selected_action_index == quick_fix_actions.len() as i64 + 2 {
                println!("tool_box::check_code_correctness::no_changes_required");
                break;
            } else {
                // invoke the code action over here with the editor
                let response = self
                    .invoke_quick_action(
                        selected_action_index,
                        &lsp_request_id,
                        message_properties.clone(),
                    )
                    .await?;
                if response.is_success() {
                    // great we have a W
                } else {
                    // boo something bad happened, we should probably log and do something about this here
                    // for now we assume its all Ws
                }
            }
        }
        Ok(())
    }

    /// We are going to edit out the code depending on the test output
    async fn fix_tests_by_editing(
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

    async fn code_correctness_with_edits(
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
        fs_file_path: &str,
        fs_file_content: &str,
        edited_range: &Range,
        symbol_name: &str,
        instruction: &str,
        previous_code: &str,
        diagnostics: Vec<Diagnostic>,
        quick_fix_actions: Vec<QuickFixOption>,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        extra_symbol_plan: Option<&str>,
        message_properties: SymbolEventMessageProperties,
        // TODO(skcd): a history parameter and play with the prompt over here so
        // the LLM does not over index on the history of the symbols which were edited
    ) -> Result<CodeCorrectnessAction, SymbolError> {
        let (code_above, code_below, code_in_selection) =
            split_file_content_into_parts(fs_file_content, edited_range);
        let request = ToolInput::CodeCorrectnessAction(CodeCorrectnessRequest::new(
            fs_file_content.to_owned(),
            fs_file_path.to_owned(),
            code_above,
            code_below,
            code_in_selection,
            symbol_name.to_owned(),
            instruction.to_owned(),
            diagnostics,
            quick_fix_actions,
            previous_code.to_owned(),
            llm,
            provider,
            api_keys,
            extra_symbol_plan.map(|plan| plan.to_owned()),
            message_properties.root_request_id().to_owned(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
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
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        println!("============tool_box::code_edit_search_and_replace============");
        println!(
            "tool_box::code_edit_search_and_replace::fs_file_path({})::symbol_name({})",
            fs_file_path,
            sub_symbol.symbol_name(),
        );
        println!("============");
        let language = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .map(|language_config| language_config.get_language())
            .flatten()
            .unwrap_or("".to_owned());
        let (above, below, mut in_range_selection) =
            split_file_content_into_parts(file_content, selection_range);
        let original_in_range_selection = in_range_selection.to_owned();
        in_range_selection = self
            .apply_inlay_hints(
                fs_file_path,
                &in_range_selection,
                selection_range,
                message_properties.clone(),
            )
            .await?;
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
        let request = ToolInput::SearchAndReplaceEditing(SearchAndReplaceEditingRequest::new(
            fs_file_path.to_owned(),
            selection_range.clone(),
            in_range_selection,
            above,
            below,
            extra_context,
            LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::Anthropic,
                LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("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".to_owned())),
            ),
            language,
            symbols_to_edit,
            instruction,
            message_properties.root_request_id().to_owned(),
            symbol_identifier.clone(),
            uuid::Uuid::new_v4().to_string(),
            message_properties.ui_sender().clone(),
        ));
        println!(
            "tool_box::code_edit_outline::start::symbol_name({})",
            sub_symbol.symbol_name()
        );
        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_search_and_replace_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        let updated_code = search_and_replace_generator(
            response.response(),
            &original_in_range_selection,
            selection_range,
        );

        // Now we can apply the edits to the editor
        let _ = self
            .apply_edits_to_editor(
                fs_file_path,
                selection_range,
                &updated_code,
                false,
                message_properties.clone(),
            )
            .await?;
        println!(
            "tool_box::search_and_replace::finish::symbol_name({})",
            sub_symbol.symbol_name()
        );
        Ok(updated_code)
    }

    /// This uses a more powerful LLM to plan out the changes and generate
    /// an outline of the edits which need to happen and then a weaker model
    /// to apply those edits to the range we are interested in
    ///
    /// - Use anything >= GPT-4 level intelligence to make the changes over here
    /// - Use a weaker model to start applying the changes
    pub async fn code_edit_outline(
        &self,
        sub_symbol: &SymbolToEdit,
        symbol_identifier: &SymbolIdentifier,
        fs_file_path: &str,
        file_content: &str,
        selection_range: &Range,
        extra_context: String,
        instruction: String,
        llm_properties: LLMProperties,
        symbol_to_edit: Option<String>,
        symbols_edited_list: Option<Vec<SymbolEditedItem>>,
        // if the outline edit is an addition, this implies that we can directly
        // stream the output from the slow-llm instead of waiting on the slow
        // llm to rewrite the whole block
        is_addition: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        println!("============tool_box::code_edit_outline============");
        println!("tool_box::code_edit_outline::fs_file_path:{}", fs_file_path);
        println!(
            "tool_box::code_edit_outline::selection_range:{:?}",
            selection_range
        );
        println!("============");
        let language = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .map(|language_config| language_config.get_language())
            .flatten()
            .unwrap_or("".to_owned());
        let (above, below, mut in_range_selection) =
            split_file_content_into_parts(file_content, selection_range);
        in_range_selection = self
            .apply_inlay_hints(
                fs_file_path,
                &in_range_selection,
                selection_range,
                message_properties.clone(),
            )
            .await?;
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
        let request = ToolInput::CodeEditing(CodeEdit::new(
            above,
            below,
            fs_file_path.to_owned(),
            in_range_selection.to_owned(),
            extra_context,
            language.to_owned(),
            instruction.to_owned(),
            llm_properties.llm().clone(),
            llm_properties.api_key().clone(),
            llm_properties.provider().clone(),
            false,
            symbol_to_edit,
            // pass the symbol which we want to edit over here
            if sub_symbol.is_new() {
                Some(sub_symbol.symbol_name().to_owned())
            } else {
                None
            },
            message_properties.root_request_id().to_owned(),
            selection_range.clone(),
            // we want an outline edit
            true,
            symbols_to_edit,
            // if its addition then we should stream the code edits at this point
            is_addition,
            symbol_identifier.clone(),
            message_properties.ui_sender().clone(),
        ));
        println!(
            "tool_box::code_edit_outline::start::symbol_name({})",
            sub_symbol.symbol_name()
        );
        let response = self
            .tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_edit_output()
            .ok_or(SymbolError::WrongToolOutput)?;
        println!(
            "tool_box::code_edit_outline::finish::symbol_name({})",
            sub_symbol.symbol_name()
        );

        println!(
            "tool_box::code_edit_outline::apply_outline_edit_to_range::start::({})",
            sub_symbol.symbol_name()
        );
        if is_addition {
            Ok(response)
        } else {
            let request = ToolInput::ApplyOutlineEditToRange(ApplyOutlineEditsToRangeRequest::new(
                instruction,
                symbol_identifier.clone(),
                fs_file_path.to_owned(),
                in_range_selection,
                response,
                message_properties.root_request_id().to_owned(),
                selection_range.clone(),
                LLMProperties::new(
                    // why are we using gpt4omini over here which is slow as shit, lets at the very
                    // least move over to gemini-flash
                    LLMType::GeminiProFlash,
                    LLMProvider::GoogleAIStudio,
                    LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
                        "AIzaSyCMkKfNkmjF8rTOWMg53NiYmz0Zv6xbfsE".to_owned(),
                    )),
                ),
                // this is the special request id sent along with every edit which we want to make
                uuid::Uuid::new_v4().to_string(),
                message_properties.ui_sender(),
            ));
            let response = self
                .tools
                .invoke(request)
                .await
                .map_err(|e| SymbolError::ToolError(e))?
                .get_apply_edits_to_range_response()
                .ok_or(SymbolError::WrongToolOutput)?;
            println!(
                "tool_box::code_edit_outline::apply_outline_edit_to_range::finish::({})",
                sub_symbol.symbol_name()
            );
            Ok(response.code())
        }
    }

    pub async fn code_edit(
        &self,
        fs_file_path: &str,
        file_content: &str,
        selection_range: &Range,
        extra_context: String,
        instruction: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        swe_bench_initial_edit: bool,
        symbol_to_edit: Option<String>,
        is_new_sub_symbol: Option<String>,
        symbol_edited_list: Option<Vec<SymbolEditedItem>>,
        symbol_identifier: &SymbolIdentifier,
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
        let request = ToolInput::CodeEditing(CodeEdit::new(
            above,
            below,
            fs_file_path.to_owned(),
            in_range_selection,
            extra_context,
            language.to_owned(),
            instruction,
            llm,
            api_keys,
            provider,
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
        stream::iter(file_paths)
            .map(|file_path| async move {
                let outline_nodes = self.get_outline_nodes_grouped(&file_path).await;
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
        message_properties: SymbolEventMessageProperties,
    ) -> Result<LSPQuickFixInvocationResponse, SymbolError> {
        let request = ToolInput::QuickFixInvocationRequest(LSPQuickFixInvocationRequest::new(
            lsp_request_id.to_owned(),
            quick_fix_index,
            message_properties.editor_url(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_quick_fix_invocation_result()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn get_file_content(&self, fs_file_path: &str) -> Result<String, SymbolError> {
        self.symbol_broker
            .get_file_content(fs_file_path)
            .await
            .ok_or(SymbolError::UnableToReadFileContent)
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

    pub async fn get_outline_nodes_grouped(&self, fs_file_path: &str) -> Option<Vec<OutlineNode>> {
        self.symbol_broker.get_symbols_outline(fs_file_path).await
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

    pub async fn symbol_in_range(
        &self,
        fs_file_path: &str,
        range: &Range,
    ) -> Option<Vec<OutlineNode>> {
        self.symbol_broker
            .get_symbols_in_range(fs_file_path, range)
            .await
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

    pub async fn file_open(
        &self,
        fs_file_path: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<OpenFileResponse, SymbolError> {
        let request = ToolInput::OpenFile(OpenFileRequest::new(
            fs_file_path.to_owned(),
            message_properties.editor_url().to_owned(),
        ));
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::open_file_event(
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
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
                    "AIzaSyCMkKfNkmjF8rTOWMg53NiYmz0Zv6xbfsE".to_owned(),
                )),
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
        // we always open the document before asking for an outline
        let file_open_result = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let language = file_open_result.language().to_owned();
        // we add the document for parsing over here
        self.symbol_broker
            .force_add_document(
                file_open_result.fs_file_path().to_owned(),
                file_open_result.contents(),
                language,
            )
            .await;

        // we grab the outlines over here
        let outline_nodes = self.symbol_broker.get_symbols_outline(fs_file_path).await;

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
        // LSP requies the EXACT symbol location on where to click go-to-implementation
        // since thats the case we can just open the file and then look for the
        // first occurance of the symbol and grab the location
        let file_content = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let language = file_content.language().to_owned();
        let _ = self
            .symbol_broker
            .force_add_document(fs_file_path.to_owned(), file_content.contents(), language)
            .await;
        // Now we need to find the outline node which corresponds to the symbol we are
        // interested in and use that as the position to ask for the implementations
        let position_from_outline_node = self
            .symbol_broker
            .get_symbols_outline(fs_file_path)
            .await
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
                if node.is_class() {
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

    /// Gets the changed contents of the file using git-diff
    pub async fn get_file_changes(
        &self,
        root_directory: &str,
        fs_file_path: &str,
    ) -> Result<GitDiffClientResponse, SymbolError> {
        let tool_input = ToolInput::GitDiff(GitDiffClientRequest::new(
            root_directory.to_owned(),
            fs_file_path.to_owned(),
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
    pub async fn grab_changed_symbols_in_file(
        &self,
        root_directory: &str,
        fs_file_path: &str,
    ) -> Result<SymbolChangeSet, SymbolError> {
        let file_changes = self.get_file_changes(root_directory, fs_file_path).await?;

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
        language_config
            .generate_outline_fresh(file_changes.old_version().as_bytes(), fs_file_path)
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
            .generate_outline_fresh(file_changes.new_version().as_bytes(), fs_file_path)
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
                                    || new_outline_nodes[0].is_funciton()
                                {
                                    older_outline_nodes
                                        .into_iter()
                                        .find(|outline_node| {
                                            outline_node.is_class_definition()
                                                || outline_node.is_funciton()
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
                                                ), original_content.to_owned()))
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
                                ), original_content.to_owned())])
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
                symbol_edits.map(|edits| (symbol_identifier.symbol_name().to_owned(), edits))
            })
            .collect::<Vec<_>>();

        let symbol_change_set = changed_nodes_followups
            .into_iter()
            .map(|(symbol_name, changes)| SymbolChanges::new(symbol_name, changes))
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
    ) -> Result<Vec<(SymbolIdentifier, Vec<String>)>, SymbolError> {
        let selection_variable = user_context
            .variables
            .iter()
            .find(|variable| variable.is_selection());
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
            return Ok(vec![]);
        }
        let language_config = language_config.expect("is_none to hold");
        let file_contents = self
            .file_open(fs_file_path.to_owned(), message_properties.clone())
            .await?;
        let outline_nodes = language_config
            .generate_outline_fresh(file_contents.contents_ref().as_bytes(), &fs_file_path);

        // now I have the outline nodes, I want to see which of them intersect with the range we are interested in
        let intersecting_outline_nodes = outline_nodes
            .into_iter()
            .filter(|outline_node| {
                outline_node
                    .range()
                    .intersects_without_byte(&selection_range)
            })
            .collect::<Vec<_>>();

        let anchored_nodes = intersecting_outline_nodes
            .into_iter()
            .map(|outline_node| {
                if outline_node.is_funciton() || outline_node.is_class_definition() {
                    // then its a single unit of work, so its a bit easier
                    (
                        SymbolIdentifier::with_file_path(
                            outline_node.name(),
                            outline_node.fs_file_path(),
                        ),
                        vec![outline_node.name().to_owned()],
                    )
                } else {
                    // we need to look at the children node and figure out where we are going to be making the edits
                    let children_nodes = outline_node
                        .children()
                        .into_iter()
                        .filter(|children| {
                            children.range().intersects_without_byte(&selection_range)
                        })
                        .map(|child_outline_node| child_outline_node.name().to_owned())
                        .collect::<Vec<_>>();
                    (
                        SymbolIdentifier::with_file_path(
                            outline_node.name(),
                            outline_node.fs_file_path(),
                        ),
                        children_nodes,
                    )
                }
            })
            .collect::<Vec<_>>();

        // Now that we have the intersecting outline nodes we can create our own request types on top of this
        Ok(anchored_nodes)
    }

    /// Uses the anchored symbols to grab the symbols which require editing
    pub async fn symbol_to_edit_request(
        &self,
        anchored_symbols: Vec<(SymbolIdentifier, Vec<String>)>,
        user_query: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<SymbolToEditRequest>, SymbolError> {
        let mut symbol_to_edit_request = vec![];
        for (symbol_identifier, child_symbols) in anchored_symbols.into_iter() {
            // if no file path is present we should keep moving forward
            let fs_file_path = symbol_identifier.fs_file_path();
            if fs_file_path.is_none() {
                continue;
            }
            let fs_file_path = fs_file_path.expect("is_none to hold");
            let file_content = self
                .file_open(fs_file_path.to_owned(), message_properties.clone())
                .await?;
            let language_config = self.editor_parsing.for_file_path(&fs_file_path);
            if language_config.is_none() {
                continue;
            }
            let language_config = language_config.expect("is_none to hold");
            let outline_nodes = language_config
                .generate_outline_fresh(file_content.contents_ref().as_bytes(), &fs_file_path)
                .into_iter()
                .filter(|outline_node| outline_node.name() == symbol_identifier.symbol_name())
                .collect::<Vec<_>>();

            // Now we loop over the children and try to find the matching entries
            // for each one of them along with the range
            // the child might share the same name as the outline-node in which case
            // its a function or a class definition
            let symbols_to_edit = child_symbols
                .into_iter()
                .filter_map(|child_symbol| {
                    if child_symbol == symbol_identifier.symbol_name() {
                        // this is a spcial case where the child symbol is of the same name as the symbol name
                        // representing a class definition or function
                        let possible_outline_node = outline_nodes.iter().find(|outline_node| {
                            outline_node.is_class_definition() || outline_node.is_funciton()
                        });
                        if let Some(outline_node) = possible_outline_node {
                            Some(SymbolToEdit::new(
                                child_symbol.to_owned(),
                                outline_node.range().clone(),
                                outline_node.fs_file_path().to_owned(),
                                vec![user_query.to_owned()],
                                false,
                                false,
                                false,
                                user_query.to_owned(),
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
                                        false, // is full edit is set to false
                                        user_query.to_owned(),
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
            symbol_to_edit_request.push(SymbolToEditRequest::new(
                symbols_to_edit,
                symbol_identifier,
                vec![],
            ))
        }
        Ok(symbol_to_edit_request)
    }
}
