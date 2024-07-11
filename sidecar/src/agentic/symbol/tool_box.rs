use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::{stream, StreamExt};
use llm_client::clients::types::LLMType;
use llm_client::provider::{LLMProvider, LLMProviderAPIKeys};
use tokio::sync::mpsc::UnboundedSender;

use crate::agentic::symbol::helpers::split_file_content_into_parts;
use crate::agentic::symbol::identifier::{Snippet, SymbolIdentifier};
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
    CodeSymbolProbingSummarize, CodeSymbolToAskQuestionsRequest, CodeSymbolUtilityRequest,
    CodeSymbolWithSteps, CodeSymbolWithThinking,
};
use crate::agentic::tool::code_symbol::initial_request_follow::{
    CodeSymbolFollowInitialRequest, CodeSymbolFollowInitialResponse,
};
use crate::agentic::tool::code_symbol::models::anthropic::{
    AskQuestionSymbolHint, CodeSymbolShouldAskQuestionsResponse, CodeSymbolToAskQuestionsResponse,
    ProbeNextSymbol,
};
use crate::agentic::tool::code_symbol::new_sub_symbol::{
    NewSubSymbolRequiredRequest, NewSubSymbolRequiredResponse,
};
use crate::agentic::tool::code_symbol::planning_before_code_edit::PlanningBeforeCodeEditRequest;
use crate::agentic::tool::code_symbol::probe::{
    ProbeEnoughOrDeeperRequest, ProbeEnoughOrDeeperResponse,
};
use crate::agentic::tool::code_symbol::probe_question_for_symbol::ProbeQuestionForSymbolRequest;
use crate::agentic::tool::code_symbol::probe_try_hard_answer::ProbeTryHardAnswerSymbolRequest;
use crate::agentic::tool::editor::apply::{EditorApplyRequest, EditorApplyResponse};
use crate::agentic::tool::errors::ToolError;
use crate::agentic::tool::filtering::broker::{
    CodeToEditFilterRequest, CodeToEditFilterResponse, CodeToEditSymbolRequest,
    CodeToEditSymbolResponse, CodeToProbeFilterResponse, CodeToProbeSubSymbolList,
    CodeToProbeSubSymbolRequest,
};
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
use crate::user_context::types::UserContext;
use crate::{
    agentic::tool::{broker::ToolBroker, input::ToolInput, lsp::open_file::OpenFileRequest},
    inline_completion::symbols_tracker::SymbolTrackerInline,
};

use super::errors::SymbolError;
use super::events::edit::SymbolToEdit;
use super::events::probe::{SubSymbolToProbe, SymbolToProbeRequest};
use super::helpers::{find_needle_position, generate_hyperlink_from_snippet};
use super::identifier::{LLMProperties, MechaCodeSymbolThinking};
use super::tool_properties::ToolProperties;
use super::types::{SymbolEventRequest, SymbolEventResponse};
use super::ui_event::UIEventWithID;

#[derive(Clone)]
pub struct ToolBox {
    tools: Arc<ToolBroker>,
    symbol_broker: Arc<SymbolTrackerInline>,
    editor_parsing: Arc<EditorParsing>,
    editor_url: String,
    ui_events: UnboundedSender<UIEventWithID>,
    root_request_id: String,
}

impl ToolBox {
    pub fn new(
        tools: Arc<ToolBroker>,
        symbol_broker: Arc<SymbolTrackerInline>,
        editor_parsing: Arc<EditorParsing>,
        editor_url: String,
        ui_events: UnboundedSender<UIEventWithID>,
        root_request_id: String,
    ) -> Self {
        Self {
            tools,
            symbol_broker,
            editor_parsing,
            editor_url,
            ui_events,
            root_request_id,
        }
    }

    pub async fn get_last_position_in_file(
        &self,
        fs_file_path: &str,
        request_id: &str,
    ) -> Result<Position, SymbolError> {
        let file_content = self.file_open(fs_file_path.to_owned(), request_id).await?;
        let file_lines = file_content
            .contents_ref()
            .lines()
            .into_iter()
            .collect::<Vec<_>>()
            .len();
        Ok(Position::new(file_lines - 1, 0, 0))
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
                    // are not part of the trait implementation yet, we have to figure
                    // out the trait implementation logic afterwards, for now
                    // we assume its free of the trait implementation, but this will break
                    // for sure cause we have no guarding logic aginst the success case
                    implementation.outline_node_content().is_class_type()
                        && implementation
                            .outline_node_content()
                            .has_trait_implementation()
                            .is_none()
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
        } else {
            Ok(sub_symbol_to_edit)
        }
    }

    /// Finds the symbols which need to be edited from the user context
    pub async fn find_symbols_to_edit_from_context(
        &self,
        context: &str,
        llm_properties: LLMProperties,
        request_id: &str,
    ) -> Result<FindSymbolsToEditInContextResponse, SymbolError> {
        let tool_input =
            ToolInput::FindSymbolsToEditInContext(FindSymbolsToEditInContextRequest::new(
                context.to_owned(),
                llm_properties,
                self.root_request_id.to_owned(),
            ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        request_id: &str,
    ) -> Result<LSPGrepSymbolInCodebaseResponse, SymbolError> {
        let tool_input = ToolInput::GrepSymbolInCodebase(LSPGrepSymbolInCodebaseRequest::new(
            self.editor_url.to_owned(),
            symbol_name.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
        self.tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_lsp_grep_symbols_in_codebase_response()
            .ok_or(SymbolError::WrongToolOutput)
    }

    pub async fn find_file_location_for_new_symbol(
        &self,
        symbol_name: &str,
        fs_file_path: &str,
        code_location: &Range,
        user_query: &str,
        request_id: &str,
    ) -> Result<FindFileForSymbolResponse, SymbolError> {
        // Here there are multiple steps which we need to take to answer this:
        // - Get all the imports in the file which we are interested in
        // - Get the location of the imports which are present in the file (just the file paths)
        let language_config = self
            .editor_parsing
            .for_file_path(fs_file_path)
            .ok_or(SymbolError::FileTypeNotSupported(fs_file_path.to_owned()))?;
        let file_contents = self.file_open(fs_file_path.to_owned(), request_id).await?;
        let source_code = file_contents.contents_ref().as_bytes();
        let hoverable_nodes = language_config.hoverable_nodes(source_code);
        let import_identifiers = language_config.generate_import_identifiers_fresh(source_code);
        // Now we do the dance where we go over the hoverable nodes and only look at the ranges which overlap
        // with the import identifiers
        let clickable_imports = import_identifiers
            .into_iter()
            .filter(|(_, import_range)| {
                hoverable_nodes
                    .iter()
                    .any(|hoverable_range| hoverable_range.contains(import_range))
            })
            .collect::<Vec<_>>();
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
        let import_file_locations = stream::iter(clickable_imports)
            .map(|(_, clickable_import_range)| {
                self.go_to_definition(
                    fs_file_path,
                    clickable_import_range.end_position(),
                    request_id,
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        request_id: &str,
    ) -> Result<String, SymbolError> {
        let tool_input =
            ToolInput::ProbeTryHardAnswerRequest(ProbeTryHardAnswerSymbolRequest::new(
                user_query.to_owned(),
                probe_request.to_owned(),
                symbol_content,
                llm_properties,
                self.root_request_id.to_owned(),
            ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        symbol_content: String,
        llm_properties: LLMProperties,
        user_query: &str,
        plan: String,
        request_id: &str,
    ) -> Result<NewSubSymbolRequiredResponse, SymbolError> {
        let tool_input = ToolInput::NewSubSymbolForCodeEditing(NewSubSymbolRequiredRequest::new(
            user_query.to_owned(),
            plan.to_owned(),
            symbol_content,
            llm_properties,
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
                self.root_request_id.to_owned(),
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
        request_id: &str,
    ) -> Result<ProbeEnoughOrDeeperResponse, SymbolError> {
        let tool_input = ToolInput::ProbeEnoughOrDeeper(ProbeEnoughOrDeeperRequest::new(
            symbol_name,
            xml_string,
            query,
            llm_properties,
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        request_id: &str,
    ) -> Result<CodeToProbeSubSymbolList, SymbolError> {
        let tool_input =
            ToolInput::ProbeFilterSnippetsSingleSymbol(CodeToProbeSubSymbolRequest::new(
                xml_string,
                query,
                llm,
                provider,
                api_key,
                self.root_request_id.to_owned(),
            ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        request_id: &str,
    ) -> Result<CodeSymbolFollowInitialResponse, SymbolError> {
        let tool_input =
            ToolInput::CodeSymbolFollowInitialRequest(CodeSymbolFollowInitialRequest::new(
                code_symbol_content,
                user_query.to_owned(),
                llm,
                provider,
                api_keys,
                self.root_request_id.to_owned(),
            ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
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
        request_id: &str,
    ) -> Result<String, SymbolError> {
        let tool_input = ToolInput::ProbeSummarizeAnswerRequest(request);
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
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
        request_id: &str,
    ) -> Result<ProbeNextSymbol, SymbolError> {
        let tool_input = ToolInput::ProbeFollowAlongSymbol(request);
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
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
        request_id: &str,
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
            .file_open(fs_file_path.to_owned(), request_id)
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
                // LLMs have a trendency to go overboard with this
                dbg!(&line, line.contains(&line_content));
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
        let mut symbol_locations = dbg!(containing_lines)
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
            .go_to_definition(fs_file_path, symbol_location, request_id)
            .await?
            .definitions();

        // interested files
        let files_interested = go_to_definition
            .iter()
            .map(|definition| definition.file_path().to_owned())
            .collect::<HashSet<String>>();

        // open all these files and get back the outline nodes from these
        let _ = stream::iter(files_interested.into_iter())
            .map(|file| async move {
                let file_open = self.file_open(file.to_owned(), request_id).await;
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
        let definitions_to_outline_node = stream::iter(definitions_to_outline_node)
            .map(|(definition, outline_node)| async move {
                // Figure out what to do over here
                let identifier_range = outline_node.identifier_range();
                let fs_file_path = outline_node.fs_file_path().to_owned();
                // we want to initiate another go-to-definition at this position
                // and compare if it lands to the same location as the outline node
                // if it does, then this is correct otherwise we have to change our
                // outline node
                let go_to_definition = self
                    .go_to_definition(&fs_file_path, identifier_range.end_position(), request_id)
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
                                        .file_open(definition_file_path.to_owned(), request_id)
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
        let definition_to_outline_node_name_and_definition =
            stream::iter(definitions_to_outline_node)
                .map(|(definition, outline_node)| async move {
                    let fs_file_path = outline_node.fs_file_path();
                    let symbol_outline = self
                        .outline_nodes_for_symbol(&fs_file_path, outline_node.name(), request_id)
                        .await;
                    (definition, outline_node.name().to_owned(), symbol_outline)
                })
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
        request_id: &str,
    ) -> Result<CodeSymbolToAskQuestionsResponse, SymbolError> {
        let file_contents = self
            .file_open(snippet.file_path().to_owned(), request_id)
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<CodeSymbolShouldAskQuestionsResponse, SymbolError> {
        let file_contents = self
            .file_open(snippet.file_path().to_owned(), request_id)
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<CodeToProbeFilterResponse, SymbolError> {
        let probe_request = request.probe_request();
        let request = ToolInput::ProbeSubSymbol(CodeToEditFilterRequest::new(
            snippets,
            probe_request.to_owned(),
            llm,
            provider,
            api_key,
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<String, SymbolError> {
        // send an open file request here first
        let _ = self.file_open(fs_file_path.to_owned(), request_id).await?;
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
                        request_id,
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
                let _ =
                    stream::iter(file_paths.clone())
                        .map(|fs_file_path| async move {
                            self.file_open(fs_file_path, request_id).await
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
        request_id: &str,
    ) -> Result<OutlineNodeContent, SymbolError> {
        let file_open_response = self
            .file_open(sub_symbol_probe.fs_file_path().to_owned(), request_id)
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

    /// The symbol can move because of some other edit so we have to map it
    /// properly over here and find it using the name as that it is the best
    /// way to achieve this right now
    /// There might be multiple outline nodes with the same name (rust) supports this
    /// so we need to find the outline node either closest to the range we are interested
    /// in or we found a child node
    pub async fn find_sub_symbol_to_edit_with_name(
        &self,
        parent_symbol_name: &str,
        symbol_to_edit: &SymbolToEdit,
        request_id: &str,
    ) -> Result<OutlineNodeContent, SymbolError> {
        let file_open_response = self
            .file_open(symbol_to_edit.fs_file_path().to_owned(), request_id)
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

    pub fn detect_language(&self, fs_file_path: &str) -> Option<String> {
        self.editor_parsing
            .for_file_path(fs_file_path)
            .map(|ts_language_config| ts_language_config.language_str.to_owned())
    }

    pub async fn utlity_symbols_search(
        &self,
        user_query: &str,
        already_collected_definitions: &[&CodeSymbolWithThinking],
        outline_node_content: &OutlineNodeContent,
        fs_file_content: &str,
        fs_file_path: &str,
        user_context: &UserContext,
        language: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
        tool_properties: &ToolProperties,
    ) -> Result<Vec<Option<(CodeSymbolWithThinking, String)>>, SymbolError> {
        // we are going to use the long context search here to check if there are
        // other utility functions we can and should use for implementing this feature
        // In our user-query we tell the LLM about what symbols are already included
        // and we ask the LLM to collect the other utility symbols which are missed

        // we have to create the query here using the outline node we are interested in
        // and the definitions which we already know about
        let request = CodeSymbolUtilityRequest::new(
            user_query.to_owned(),
            already_collected_definitions
                .into_iter()
                .map(|symbol_with_thinking| {
                    let file_path = symbol_with_thinking.file_path();
                    let symbol_name = symbol_with_thinking.code_symbol();
                    // TODO(skcd): This is horribly wrong, we want to get the full symbol
                    // over here and not just the symbol name since that does not make sense
                    // or at the very least the outline for the symbol
                    format!(
                        r#"<snippet>
<file_path>
{file_path}
</file_path>
<symbol_name>
{symbol_name}
</symbol_name>
</snippet>"#
                    )
                })
                .collect::<Vec<_>>(),
            fs_file_path.to_owned(),
            fs_file_content.to_owned(),
            outline_node_content.range().clone(),
            language.to_owned(),
            llm,
            provider,
            api_keys,
            user_context.clone(),
            self.root_request_id.to_owned(),
        );
        let tool_input = ToolInput::CodeSymbolUtilitySearch(request);
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
        // These are the code symbols which are important from the global search
        // we might have some errors over here which we should fix later on, but we
        // will get on that
        // TODO(skcd): Figure out the best way to fix them
        // pick up from here, we need to run some cleanup things over here, to make sure
        // that we dont make mistakes while grabbing the code symbols
        // for now: we can assume that there are no errors over here, we can work with
        // this assumption for now
        let code_symbols = self
            .tools
            .invoke(tool_input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .utility_code_search_response()
            .ok_or(SymbolError::WrongToolOutput)?;

        let file_paths_to_open: Vec<String> = code_symbols
            .symbols()
            .iter()
            .map(|symbol| symbol.file_path().to_owned())
            .collect::<Vec<_>>();
        // We have the file content for the file paths which the retrival
        // engine presented us with
        let file_to_content_mapping = stream::iter(file_paths_to_open)
            .map(|file_to_open| async move {
                let tool_input = ToolInput::OpenFile(OpenFileRequest::new(
                    file_to_open.to_owned(),
                    self.editor_url.to_owned(),
                ));
                (
                    file_to_open,
                    self.tools
                        .invoke(tool_input)
                        .await
                        .map(|tool_output| tool_output.get_file_open_response()),
                )
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(
                |(fs_file_path, open_file_response)| match open_file_response {
                    Ok(Some(response)) => Some((fs_file_path, response.contents())),
                    _ => None,
                },
            )
            .collect::<HashMap<String, String>>();
        // After this we want to grab the symbol definition after looking at where
        // the symbol is in the file
        let symbols_to_grab = code_symbols.remove_symbols();
        let symbol_locations = stream::iter(symbols_to_grab)
            .map(|symbol| async {
                let symbol_name = symbol.code_symbol();
                let fs_file_path = symbol.file_path();
                if let Some(file_content) = file_to_content_mapping.get(fs_file_path) {
                    let location = self
                        .find_symbol_in_file(symbol_name, file_content, request_id)
                        .await;
                    Some((symbol, location))
                } else {
                    None
                }
            })
            .buffer_unordered(100)
            .filter_map(|content| futures::future::ready(content))
            .collect::<Vec<_>>()
            .await;

        // We now have the locations and the symbol as well, we now ask the symbol manager
        // for the outline for this symbol
        let symbol_to_definition = stream::iter(
            symbol_locations
                .into_iter()
                .map(|symbol_location| (symbol_location, hub_sender.clone())),
        )
        .map(|((symbol, location), hub_sender)| async move {
            if let Ok(location) = location {
                // we might not get the position here for some weird reason which
                // is also fine
                let position = location.get_position();
                if let Some(position) = position {
                    let possible_file_path = self
                        .go_to_definition(fs_file_path, position, request_id)
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
                        let _ = hub_sender.send((
                            SymbolEventRequest::outline(
                                SymbolIdentifier::with_file_path(
                                    symbol.code_symbol(),
                                    &definition_file_path,
                                ),
                                tool_properties.clone(),
                            ),
                            uuid::Uuid::new_v4().to_string(),
                            sender,
                        ));
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
        })
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;
        Ok(symbol_to_definition)
    }

    pub async fn check_for_followups(
        &self,
        parent_symbol_name: &str,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
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
        let symbol_to_edit = dbg!(
            self.find_sub_symbol_to_edit_with_name(parent_symbol_name, symbol_edited, request_id)
                .await
        )?;
        // over here we have to check if its a function or a class
        if symbol_to_edit.is_function_type() {
            // we do need to get the references over here for the function and
            // send them over as followups to check wherever they are being used
            let references = dbg!(
                self.go_to_references(
                    symbol_edited.fs_file_path(),
                    &symbol_edited.range().start_position(),
                    request_id,
                )
                .await
            )?;
            let _ = dbg!(
                self.invoke_followup_on_references(
                    symbol_edited,
                    original_code,
                    &symbol_to_edit,
                    references,
                    hub_sender,
                    request_id,
                    tool_properties,
                )
                .await
            );
        } else if symbol_to_edit.is_class_definition() {
            // TODO(skcd): Show the AI the changed parts over here between the original
            // code and the changed node and ask it for the symbols which we should go
            // to references for, that way we are able to do the finer garained changes
            // as and when required
            let _ = dbg!(
                self.invoke_references_check_for_class_definition(
                    symbol_edited,
                    original_code,
                    &symbol_to_edit,
                    language,
                    llm,
                    provider,
                    api_keys,
                    hub_sender.clone(),
                    request_id,
                    tool_properties,
                )
                .await
            );
            let references = dbg!(
                self.go_to_references(
                    symbol_edited.fs_file_path(),
                    &symbol_edited.range().start_position(),
                    request_id,
                )
                .await
            )?;
            let _ = dbg!(
                self.invoke_followup_on_references(
                    symbol_edited,
                    original_code,
                    &symbol_to_edit,
                    references,
                    hub_sender,
                    request_id,
                    tool_properties,
                )
                .await
            );
        } else {
            // something else over here, wonder what it could be
            return Err(SymbolError::NoContainingSymbolFound);
        }
        Ok(())
    }

    // TODO(codestory): This code is not running because it fails on a serde
    // conversion
    async fn invoke_references_check_for_class_definition(
        &self,
        symbol_edited: &SymbolToEdit,
        original_code: &str,
        edited_symbol: &OutlineNodeContent,
        language: String,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
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
            self.root_request_id.to_owned(),
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
        let class_memebers_to_follow = dbg!(
            self.check_class_members_to_follow(request, request_id)
                .await
        )?
        .members();
        // now we need to get the members and schedule a followup along with the refenreces where
        // we might ber using this class
        // Now we have to get the position of the members which we want to follow-along, this is important
        // since we might have multiple members here and have to make sure that we can go-to-refernces for this
        let members_with_position = class_memebers_to_follow
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
            )
        }))
        .map(|(member, position, fs_file_path, hub_sender)| async move {
            let _ = self
                .check_followup_for_member(
                    member,
                    position,
                    &fs_file_path,
                    original_code,
                    symbol_edited,
                    edited_symbol,
                    hub_sender,
                    request_id,
                    tool_properties,
                )
                .await;
        })
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
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        let references = self
            .go_to_references(fs_file_path, &position, request_id)
            .await?;
        let reference_locations = references.locations();
        let file_paths = reference_locations
            .iter()
            .map(|reference| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();
        // we invoke a request to open the file
        let _ = stream::iter(file_paths.clone())
            .map(|fs_file_path| async {
                // get the file content
                let _ = self.file_open(fs_file_path, request_id).await;
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
                        ))
                    } else {
                        None
                    }
                })
                .map(|(_, ranges, hub_sender, outline_nodes, member)| {
                    ranges
                        .into_iter()
                        .map(|range| {
                            (
                                range,
                                hub_sender.clone(),
                                outline_nodes.to_vec(),
                                member.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .flatten(),
        )
        .map(|(range, hub_sender, outline_nodes, member)| async move {
            self.send_request_for_followup_class_member(
                original_code,
                edited_code,
                symbol_edited,
                member,
                range.start_position(),
                outline_nodes,
                hub_sender,
                request_id,
                tool_properties,
            )
            .await
        })
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
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
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
                        request_id,
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
                        let _ = hub_sender.send((
                            SymbolEventRequest::ask_question(
                                SymbolIdentifier::with_file_path(
                                    outline_node.name(),
                                    outline_node.fs_file_path(),
                                ),
                                instruction_prompt,
                                tool_properties.clone(),
                            ),
                            uuid::Uuid::new_v4().to_string(),
                            sender,
                        ));
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
        request_id: &str,
    ) -> Result<ClassSymbolFollowupResponse, SymbolError> {
        let tool_input = ToolInput::ClassSymbolFollowup(request);
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
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
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
        tool_properties: &ToolProperties,
    ) -> Result<(), SymbolError> {
        let reference_locations = references.locations();
        let file_paths = reference_locations
            .iter()
            .map(|reference| reference.fs_file_path().to_owned())
            .collect::<HashSet<String>>();
        // we invoke a request to open the file
        let _ = stream::iter(file_paths.clone())
            .map(|fs_file_path| async {
                // get the file content
                let _ = self.file_open(fs_file_path, request_id).await;
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
                        Some((file_path, ranges, hub_sender.clone(), outline_nodes))
                    } else {
                        None
                    }
                })
                .map(|(_fs_file_path, ranges, hub_sender, outline_nodes)| {
                    ranges
                        .into_iter()
                        .map(|range| (range, hub_sender.clone(), outline_nodes.to_vec()))
                        .collect::<Vec<_>>()
                })
                .flatten(),
        )
        .map(|(range, hub_sender, outline_nodes)| async move {
            self.send_request_for_followup(
                original_code,
                edited_code,
                symbol_edited,
                range.start_position(),
                outline_nodes,
                hub_sender,
                request_id,
                tool_properties,
            )
            .await
        })
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
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        request_id: &str,
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
                        request_id,
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
                        let instruction_prompt = self.create_instruction_prompt_for_followup(
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
                        let _ = hub_sender.send((
                            SymbolEventRequest::ask_question(
                                SymbolIdentifier::with_file_path(
                                    outline_node.name(),
                                    outline_node.fs_file_path(),
                                ),
                                instruction_prompt,
                                tool_properties.clone(),
                            ),
                            uuid::Uuid::new_v4().to_string(),
                            sender,
                        ));
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

    async fn go_to_references(
        &self,
        fs_file_path: &str,
        position: &Position,
        request_id: &str,
    ) -> Result<GoToReferencesResponse, SymbolError> {
        let input = ToolInput::GoToReference(GoToReferencesRequest::new(
            fs_file_path.to_owned(),
            position.clone(),
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            input.clone(),
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
        request_id: &str,
    ) -> Result<SWEBenchTestRepsonse, SymbolError> {
        let tool_input =
            ToolInput::SWEBenchTest(SWEBenchTestRequest::new(swe_bench_test_endpoint.to_owned()));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            tool_input.clone(),
        ));
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
        request_id: &str,
        tool_properties: &ToolProperties,
        llm_properties: LLMProperties,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
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
            .find_symbols_to_edit_from_context(thinking, llm_properties.clone(), request_id)
            .await?;

        let symbols_to_edit_list = symbols_to_edit.symbol_list();

        if symbols_to_edit_list.is_empty() {
            return Ok(false);
        }

        // TODO(skcd+zi): Can we run this in full parallelism??
        // answer: yes we can, but lets get it to crawl before it runs
        for symbol_to_edit in symbols_to_edit_list.into_iter() {
            let symbol_to_find = symbol_to_edit.to_owned();
            let symbol_locations = self
                .grep_symbols_in_ide(&symbol_to_find, request_id)
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
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let _ = hub_sender.send((
                    SymbolEventRequest::initial_request(
                        SymbolIdentifier::with_file_path(symbol_name, symbol_file_path),
                        request.to_owned(),
                        tool_properties.clone(),
                    ),
                    request_id.to_owned(),
                    sender,
                ));
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
                let _ = hub_sender.send((
                    SymbolEventRequest::initial_request(
                        SymbolIdentifier::with_file_path(symbol_to_edit, fs_file_path),
                        request.to_owned(),
                        tool_properties.clone(),
                    ),
                    request_id.to_owned(),
                    sender,
                ));
                let _ = receiver.await;
            }
        }
        Ok(true)
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
        request_id: &str,
        tool_properties: &ToolProperties,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
    ) -> Result<(), SymbolError> {
        println!("==========too_box::check_code_correctness==========");
        println!(
            "tool_box::check_code_correctness::symbol_edited:\n{:?}",
            &symbol_edited
        );
        println!(
            "tool_box::check_code_correctness::original_code:\n{}",
            original_code
        );
        println!(
            "tool_box::check_code_correctness::edited_code:\n{}",
            edited_code
        );
        println!(
            "tool_box::check_code_correctness::code_edit_extra_context:\n{}",
            code_edit_extra_context
        );
        println!(
            "tool_box::check_code_correctness::tool_properties:{:?}",
            tool_properties
        );
        println!("====================");
        // code correction looks like this:
        // - apply the edited code to the original selection
        // - look at the code actions which are available
        // - take one of the actions or edit code as required
        // - once we have no LSP errors or anything we are good
        let instructions = symbol_edited.instructions().join("\n");
        let fs_file_path = symbol_edited.fs_file_path();
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

            let symbol_to_edit_range = self
                .find_sub_symbol_to_edit_with_name(parent_symbol_name, symbol_edited, request_id)
                .await
                .map(|outline_node| outline_node.range().clone())
                // If its a new symbol we still do not have it in our outline yet, so
                // we should grab it from the range position provided in the edit request
                .unwrap_or(symbol_edited.range().clone());
            let _fs_file_content = self
                .file_open(fs_file_path.to_owned(), request_id)
                .await?
                .contents();

            // The range of the symbol before doing the edit
            let edited_range = symbol_to_edit_range;
            let lsp_request_id = uuid::Uuid::new_v4().to_string();
            let _editor_response = dbg!(
                self.apply_edits_to_editor(fs_file_path, &edited_range, &updated_code, request_id)
                    .await
            )?;

            // after applying the edits to the editor, we will need to get the file
            // contents and the symbol again
            let symbol_to_edit = dbg!(
                self.find_sub_symbol_to_edit_with_name(
                    parent_symbol_name,
                    symbol_edited,
                    request_id
                )
                .await
            )?;
            let fs_file_content = self
                .file_open(fs_file_path.to_owned(), request_id)
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
                let swe_bench_test_output = dbg!(
                    self.swe_bench_test_tool(&swe_bench_test_endpoint, request_id)
                        .await
                )?;
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
                                request_id,
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
                                request_id,
                            )
                            .await?;

                        // Now that we have the corrected code, we should again apply
                        // it to the file
                        let _ = self
                            .apply_edits_to_editor(
                                fs_file_path,
                                symbol_to_edit.range(),
                                &corrected_code,
                                request_id,
                            )
                            .await;

                        // return over here since we are done with the code correction flow
                        // since this only happens in case of swe-bench
                        continue;
                    }
                }
            }

            // Now we check for LSP diagnostics
            let lsp_diagnostics = dbg!(
                self.get_lsp_diagnostics(fs_file_path, &edited_range, request_id)
                    .await
            )?;

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
                    request_id,
                )
                .await?
                .remove_options();

            // now we can send over the request to the LLM to select the best tool
            // for editing the code out
            let selected_action = dbg!(
                self.code_correctness_action_selection(
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
                    request_id,
                )
                .await
            )?;

            // Now that we have the selected action, we can chose what to do about it
            // there might be a case that we have to re-write the code completely, since
            // the LLM thinks that the best thing to do, or invoke one of the quick-fix actions
            let selected_action_index = selected_action.index();
            let tool_use_thinking = selected_action.thinking();
            let _ = self.ui_events.send(UIEventWithID::code_correctness_action(
                request_id.to_owned(),
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
                let fixed_code = dbg!(
                    self.code_correctness_with_edits(
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
                        request_id,
                    )
                    .await
                )?;

                let _ = self.ui_events.send(UIEventWithID::edited_code(
                    request_id.to_owned(),
                    symbol_identifier.clone(),
                    edited_range.clone(),
                    fs_file_path.to_owned(),
                    fixed_code.to_owned(),
                ));

                // after this we have to apply the edits to the editor again and being
                // the loop again
                let _ = dbg!(
                    self.apply_edits_to_editor(
                        fs_file_path,
                        &edited_range,
                        &fixed_code,
                        request_id
                    )
                    .await
                )?;
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
                        request_id,
                        tool_properties,
                        LLMProperties::new(llm.clone(), provider.clone(), api_keys.clone()),
                        hub_sender.clone(),
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
                let response = dbg!(
                    self.invoke_quick_action(selected_action_index, &lsp_request_id, request_id)
                        .await
                )?;
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
        request_id: &str,
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            input.clone(),
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
        request_id: &str,
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            code_editing_error_request.clone(),
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
        request_id: &str,
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_correctness_action()
            .ok_or(SymbolError::WrongToolOutput)
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
        request_id: &str,
        swe_bench_initial_edit: bool,
        is_new_sub_symbol: Option<String>,
    ) -> Result<String, SymbolError> {
        println!("============tool_box::code_edit============");
        println!("tool_box::code_edit::fs_file_path:{}", fs_file_path);
        // println!("tool_box::code_edit::file_content:{}", file_content);
        println!("tool_box::code_edit::selection_range:{:?}", selection_range);
        // println!("tool_box::code_edit::extra_context:{}", &extra_context);
        // println!("tool_box::code_edit::instruction:{}", &instruction);
        // println!(
        //     "tool_box::code_edit::llm_properties: {:?}, {:?}, {:?}",
        //     &llm, &api_keys, &provider
        // );
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
            is_new_sub_symbol,
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_code_edit_output()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn invoke_quick_action(
        &self,
        quick_fix_index: i64,
        lsp_request_id: &str,
        request_id: &str,
    ) -> Result<LSPQuickFixInvocationResponse, SymbolError> {
        let request = ToolInput::QuickFixInvocationRequest(LSPQuickFixInvocationRequest::new(
            lsp_request_id.to_owned(),
            quick_fix_index,
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            String,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        // we get back here the defintion outline along with the reasoning on why
        // we need to look at the symbol
        request_id: &str,
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
                let location = self
                    .find_symbol_in_file(symbol_name, file_content, request_id)
                    .await;
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
        let symbol_to_definition = stream::iter(
            symbol_locations
                .into_iter()
                .map(|symbol_location| (symbol_location, hub_sender.clone())),
        )
        .map(|((symbol, location), hub_sender)| async move {
            if let Ok(location) = location {
                // we might not get the position here for some weird reason which
                // is also fine
                let position = location.get_position();
                if let Some(position) = position {
                    let possible_file_path = self
                        .go_to_definition(fs_file_path, position, request_id)
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
                        let _ = hub_sender.send((
                            SymbolEventRequest::outline(
                                SymbolIdentifier::with_file_path(
                                    symbol.code_symbol(),
                                    &definition_file_path,
                                ),
                                tool_properties.clone(),
                            ),
                            uuid::Uuid::new_v4().to_string(),
                            sender,
                        ));
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
        })
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
        tool_use_request_id: &str,
    ) -> Result<GetQuickFixResponse, SymbolError> {
        let request = ToolInput::QuickFixRequest(GetQuickFixRequest::new(
            fs_file_path.to_owned(),
            self.editor_url.to_owned(),
            range.clone(),
            request_id,
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            tool_use_request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<LSPDiagnosticsOutput, SymbolError> {
        let input = ToolInput::LSPDiagnostics(LSPDiagnosticsInput::new(
            fs_file_path.to_owned(),
            range.clone(),
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            input.clone(),
        ));
        self.tools
            .invoke(input)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_lsp_diagnostics()
            .ok_or(SymbolError::WrongToolOutput)
    }

    async fn apply_edits_to_editor(
        &self,
        fs_file_path: &str,
        range: &Range,
        updated_code: &str,
        request_id: &str,
    ) -> Result<EditorApplyResponse, SymbolError> {
        let input = ToolInput::EditorApplyChange(EditorApplyRequest::new(
            fs_file_path.to_owned(),
            updated_code.to_owned(),
            range.clone(),
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            input.clone(),
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
        request_id: &str,
    ) -> Result<FindInFileResponse, SymbolError> {
        // Here we are going to get the position of the symbol
        let request = ToolInput::GrepSingleFile(FindInFileRequest::new(
            file_contents.to_owned(),
            symbol_name.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<CodeToEditSymbolResponse, SymbolError> {
        let request =
            ToolInput::FilterCodeSnippetsForEditingSingleSymbols(CodeToEditSymbolRequest::new(
                xml_string,
                query,
                llm,
                provider,
                api_keys,
                self.root_request_id.to_owned(),
            ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .code_to_edit_in_symbol()
            .ok_or(SymbolError::WrongToolOutput)
    }

    /// We want to generate the outline for the symbol
    async fn _get_outline_for_symbol_identifier(
        &self,
        fs_file_path: &str,
        symbol_name: &str,
        hub_sender: UnboundedSender<(
            SymbolEventRequest,
            tokio::sync::oneshot::Sender<SymbolEventResponse>,
        )>,
        tool_properties: &ToolProperties,
    ) -> Result<String, SymbolError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let _ = hub_sender.send((
            SymbolEventRequest::outline(
                SymbolIdentifier::with_file_path(symbol_name, fs_file_path),
                tool_properties.clone(),
            ),
            sender,
        ));
        let response = receiver
            .await
            .map(|response| response.to_string())
            .map_err(|e| SymbolError::RecvError(e));
        // this gives us the outline we need for the outline of the symbol which
        // we are interested in
        response
    }

    pub async fn get_outline_nodes_grouped(&self, fs_file_path: &str) -> Option<Vec<OutlineNode>> {
        self.symbol_broker.get_symbols_outline(fs_file_path).await
    }

    pub async fn get_outline_nodes(
        &self,
        fs_file_path: &str,
        request_id: &str,
    ) -> Option<Vec<OutlineNodeContent>> {
        let file_open_result = self.file_open(fs_file_path.to_owned(), request_id).await;
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

    // TODO(skcd): Use this to ask the LLM for the code snippets which need editing
    pub async fn filter_code_for_editing(
        &self,
        snippets: Vec<Snippet>,
        query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_key: LLMProviderAPIKeys,
        request_id: &str,
    ) -> Result<CodeToEditFilterResponse, SymbolError> {
        let request = ToolInput::FilterCodeSnippetsForEditing(CodeToEditFilterRequest::new(
            snippets,
            query,
            llm,
            provider,
            api_key,
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .code_to_edit_filter()
            .ok_or(SymbolError::WrongToolOutput)
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
        request_id: &str,
    ) -> Result<OpenFileResponse, SymbolError> {
        let request = ToolInput::OpenFile(OpenFileRequest::new(
            fs_file_path,
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<FindInFileResponse, SymbolError> {
        let request = ToolInput::GrepSingleFile(FindInFileRequest::new(file_content, symbol));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
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
        request_id: &str,
    ) -> Result<GoToDefinitionResponse, SymbolError> {
        let request = ToolInput::GoToDefinition(GoToDefinitionRequest::new(
            fs_file_path.to_owned(),
            self.editor_url.to_owned(),
            position,
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
        ));
        self.tools
            .invoke(request)
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .get_go_to_definition()
            .ok_or(SymbolError::WrongToolOutput)
    }

    // This helps us find the snippet for the symbol in the file, this is the
    // best way to do this as this is always exact and we never make mistakes
    // over here since we are using the LSP as well
    pub async fn find_snippet_for_symbol(
        &self,
        fs_file_path: &str,
        symbol_name: &str,
        request_id: &str,
    ) -> Result<Snippet, SymbolError> {
        // we always open the document before asking for an outline
        let file_open_result = self.file_open(fs_file_path.to_owned(), request_id).await?;
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
                let file_data = self.file_open(fs_file_path.to_owned(), request_id).await?;
                let file_content = file_data.contents();
                // now we parse it and grab the outline nodes
                let find_in_file = self
                    .find_in_file(file_content, symbol_name.to_owned(), request_id)
                    .await
                    .map(|find_in_file| find_in_file.get_position())
                    .ok()
                    .flatten();
                // now that we have a poition, we can ask for go-to-definition
                if let Some(file_position) = find_in_file {
                    let definition = self
                        .go_to_definition(fs_file_path, file_position, request_id)
                        .await?;
                    // let definition_file_path = definition.file_path().to_owned();
                    let snippet_node = self
                        .grab_symbol_content_from_definition(symbol_name, definition, request_id)
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

    /// If we cannot find the symbol using normal mechanisms we just search
    /// for the symbol by hand in the file and grab the outline node which contains
    /// the symbols
    pub async fn grab_symbol_using_search(
        &self,
        important_symbols: CodeSymbolImportantResponse,
        user_context: UserContext,
        request_id: &str,
    ) -> Result<Vec<MechaCodeSymbolThinking>, SymbolError> {
        let ordered_symbols = important_symbols.ordered_symbols();
        stream::iter(
            ordered_symbols
                .iter()
                .map(|ordered_symbol| ordered_symbol.file_path().to_owned()),
        )
        .for_each(|file_path| async move {
            let file_open_response = self.file_open(file_path.to_owned(), request_id).await;
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

        let mut mecha_code_symbols: Vec<MechaCodeSymbolThinking> = vec![];
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
                    mecha_code_symbols.push(MechaCodeSymbolThinking::new(
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
                        user_context.clone(),
                        Arc::new(self.clone()),
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
        request_id: &str,
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
        let file_content_map = stream::iter(final_paths)
            .map(|path| async move {
                let file_open = self.file_open(path.to_owned(), request_id).await;
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
            self.root_request_id.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        user_context: UserContext,
        request_id: &str,
    ) -> Result<Vec<MechaCodeSymbolThinking>, SymbolError> {
        let symbols = important_symbols.symbols();
        // let ordered_symbols = important_symbols.ordered_symbols();
        // there can be overlaps between these, but for now its fine
        // let mut new_symbols: HashSet<String> = Default::default();
        // let mut symbols_to_visit: HashSet<String> = Default::default();
        // let mut final_code_snippets: HashMap<String, MechaCodeSymbolThinking> = Default::default();
        stream::iter(
            symbols
                .iter()
                .map(|ordered_symbol| ordered_symbol.file_path().to_owned()),
        )
        .for_each(|file_path| async move {
            let file_open_response = self.file_open(file_path.to_owned(), request_id).await;
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

        let mut bounding_symbol_to_instruction: HashMap<
            OutlineNodeContent,
            Vec<(usize, &CodeSymbolWithThinking)>,
        > = Default::default();
        let mut unbounded_symbols: Vec<&CodeSymbolWithThinking> = Default::default();
        for (idx, symbol) in symbols.iter().enumerate() {
            let file_path = symbol.file_path();
            let symbol_name = symbol.code_symbol();
            let outline_nodes = self.symbol_broker.get_symbols_outline(file_path).await;
            if let Some(outline_nodes) = outline_nodes {
                let mut bounding_symbols =
                    self.grab_bounding_symbol_for_symbol(outline_nodes, symbol_name);
                if bounding_symbols.is_empty() {
                    // well this is weird, we have not outline nodes here
                    unbounded_symbols.push(symbol);
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
                user_context.clone(),
                Arc::new(self.clone()),
            );
            let mut ordered_values = order_vec
                .into_iter()
                .map(|(idx, _)| idx)
                .collect::<Vec<_>>();
            // sort by the increasing values of orderes
            ordered_values.sort();
            if ordered_values.is_empty() {
                continue;
            } else {
                mecha_code_symbols.push((ordered_values.remove(0), mecha_code_symbol_thinking));
            }
        }

        // Now we iterate over all the values in the array and then sort them via the first key
        mecha_code_symbols.sort_by_key(|(idx, _)| idx.clone());
        Ok(mecha_code_symbols
            .into_iter()
            .map(|(_, symbol)| symbol)
            .collect())
    }

    async fn go_to_implementations_exact(
        &self,
        fs_file_path: &str,
        position: &Position,
        request_id: &str,
    ) -> Result<GoToImplementationResponse, SymbolError> {
        let _ = self.file_open(fs_file_path.to_owned(), request_id).await?;
        let request = ToolInput::SymbolImplementations(GoToImplementationRequest::new(
            fs_file_path.to_owned(),
            position.clone(),
            self.editor_url.to_owned(),
        ));
        let _ = self.ui_events.send(UIEventWithID::from_tool_event(
            request_id.to_owned(),
            request.clone(),
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
        request_id: &str,
    ) -> Result<GoToImplementationResponse, SymbolError> {
        // LSP requies the EXACT symbol location on where to click go-to-implementation
        // since thats the case we can just open the file and then look for the
        // first occurance of the symbol and grab the location
        let file_content = self.file_open(fs_file_path.to_owned(), request_id).await?;
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
                self.editor_url.to_owned(),
            ));
            let _ = self.ui_events.send(UIEventWithID::from_tool_event(
                request_id.to_owned(),
                request.clone(),
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
        request_id: &str,
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
            .file_open(definition.file_path().to_owned(), request_id)
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
        request_id: &str,
    ) -> Result<OutlineNode, SymbolError> {
        println!(
            "tool_box::get_outline_node_from_snippet::snippet::({:?})",
            snippet.range()
        );
        let fs_file_path = snippet.file_path();
        let file_open_request = self.file_open(fs_file_path.to_owned(), request_id).await?;
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

        println!(
            "tool_box::get_outline_node_from_snippet::nodes({:?})",
            outline_nodes_with_distance
                .iter()
                .map(|(distance, outline_node)| (distance, outline_node.range()))
                .collect::<Vec<_>>()
        );
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
        request_id: &str,
    ) -> Result<OutlineNode, SymbolError> {
        let file_open_request = self.file_open(fs_file_path.to_owned(), request_id).await?;
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
        request_id: String,
    ) -> Result<CodeSymbolImportantResponse, SymbolError> {
        let request_id_ref = &request_id;
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
                    request_id_ref,
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
                    request_id_ref,
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
                    request_id_ref,
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
}
