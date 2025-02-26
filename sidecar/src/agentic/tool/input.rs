use crate::agentic::tool::session::ask_expert::AskExpertRequest;
use super::{
    code_edit::{
        code_editor::CodeEditorParameters,
        filter_edit::FilterEditOperationRequest,
        find::FindCodeSelectionInput,
        search_and_replace::SearchAndReplaceEditingRequest,
        test_correction::TestOutputCorrectionRequest,
        types::{CodeEdit, CodeEditingPartialRequest},
    },
    code_symbol::{
        apply_outline_edit_to_range::ApplyOutlineEditsToRangeRequest,
        correctness::CodeCorrectnessRequest,
        error_fix::CodeEditingErrorRequest,
        find_file_for_new_symbol::FindFileForSymbolRequest,
        find_symbols_to_edit_in_context::FindSymbolsToEditInContextRequest,
        followup::ClassSymbolFollowupRequest,
        important::{
            CodeSymbolFollowAlongForProbing, CodeSymbolImportantRequest,
            CodeSymbolImportantWideSearch, CodeSymbolProbingSummarize,
            CodeSymbolToAskQuestionsRequest, CodeSymbolUtilityRequest,
        },
        initial_request_follow::CodeSymbolFollowInitialRequest,
        new_location::CodeSymbolNewLocationRequest,
        new_sub_symbol::NewSubSymbolRequiredRequest,
        planning_before_code_edit::PlanningBeforeCodeEditRequest,
        probe::ProbeEnoughOrDeeperRequest,
        probe_question_for_symbol::ProbeQuestionForSymbolRequest,
        probe_try_hard_answer::ProbeTryHardAnswerSymbolRequest,
        repo_map_search::RepoMapSearchQuery,
        reranking_symbols_for_editing_context::ReRankingSnippetsForCodeEditingRequest,
        scratch_pad::ScratchPadAgentInput,
        should_edit::ShouldEditCodeSymbolRequest,
    },
    devtools::screenshot::{RequestScreenshotInput, RequestScreenshotInputPartial},
    editor::apply::EditorApplyRequest,
    errors::ToolError,
    feedback::feedback::FeedbackGenerationRequest,
    file::{
        file_finder::ImportantFilesFinderQuery,
        semantic_search::{SemanticSearchParametersPartial, SemanticSearchRequest},
    },
    filtering::broker::{
        CodeToEditFilterRequest, CodeToEditSymbolRequest, CodeToProbeSubSymbolRequest,
    },
    git::{diff_client::GitDiffClientRequest, edited_files::EditedFilesRequest},
    grep::file::FindInFileRequest,
    kw_search::tool::KeywordSearchQuery,
    lsp::{
        create_file::CreateFileRequest,
        diagnostics::LSPDiagnosticsInput,
        file_diagnostics::{FileDiagnosticsInput, WorkspaceDiagnosticsPartial},
        find_files::{FindFileInputPartial, FindFilesRequest},
        get_outline_nodes::OutlineNodesUsingEditorRequest,
        go_to_previous_word::GoToPreviousWordRequest,
        gotodefintion::GoToDefinitionRequest,
        gotoimplementations::GoToImplementationRequest,
        gotoreferences::GoToReferencesRequest,
        grep_symbol::LSPGrepSymbolInCodebaseRequest,
        inlay_hints::InlayHintsRequest,
        list_files::{ListFilesInput, ListFilesInputPartial},
        open_file::{OpenFileRequest, OpenFileRequestPartial},
        quick_fix::{GetQuickFixRequest, LSPQuickFixInvocationRequest},
        search_file::{SearchFileContentInput, SearchFileContentInputPartial},
        subprocess_spawned_output::SubProcessSpawnedPendingOutputRequest,
        undo_changes::UndoChangesMadeDuringExchangeRequest,
    },
    mcp::input::{McpToolInput, McpToolPartial},
    plan::{
        add_steps::PlanAddRequest, generator::StepGeneratorRequest, reasoning::ReasoningRequest,
        updater::PlanUpdateRequest,
    },
    r#type::ToolType,
    ref_filter::ref_filter::ReferenceFilterRequest,
    repo_map::generator::{RepoMapGeneratorRequest, RepoMapGeneratorRequestPartial},
    rerank::base::ReRankEntriesForBroker,
    reward::client::RewardGenerationRequest,
    search::big_search::BigSearchRequest,
    session::{
        ask_followup_question::AskFollowupQuestionsRequest,
        attempt_completion::AttemptCompletionClientRequest,
        chat::SessionChatClientRequest,
        exchange::SessionExchangeNewRequest,
        hot_streak::SessionHotStreakRequest,
        tool_use_agent::{ContextCrunchingInputPartial, ToolUseAgentReasoningParamsPartial},
    },
    swe_bench::test_tool::SWEBenchTestRequest,
    terminal::terminal::{TerminalInput, TerminalInputPartial},
    test_runner::runner::{TestRunnerRequest, TestRunnerRequestPartial},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ToolInputPartial {
    CodeEditing(CodeEditingPartialRequest),
    ListFiles(ListFilesInputPartial),
    SearchFileContentWithRegex(SearchFileContentInputPartial),
    OpenFile(OpenFileRequestPartial),
    LSPDiagnostics(WorkspaceDiagnosticsPartial),
    TerminalCommand(TerminalInputPartial),
    AskFollowupQuestions(AskFollowupQuestionsRequest),
    AskExpert(AskExpertRequest),
    AttemptCompletion(AttemptCompletionClientRequest),
    RepoMapGeneration(RepoMapGeneratorRequestPartial),
    TestRunner(TestRunnerRequestPartial),
    CodeEditorParameters(CodeEditorParameters),
    SemanticSearch(SemanticSearchParametersPartial),
    Reasoning(ToolUseAgentReasoningParamsPartial),
    FindFile(FindFileInputPartial),
    RequestScreenshot(RequestScreenshotInputPartial),
    ContextCrunching(ContextCrunchingInputPartial),
    McpTool(McpToolPartial),
}

impl ToolInputPartial {
    pub fn to_tool_type(&self) -> ToolType {
        match self {
            Self::CodeEditing(_) => ToolType::CodeEditing,
            Self::ListFiles(_) => ToolType::ListFiles,
            Self::SearchFileContentWithRegex(_) => ToolType::SearchFileContentWithRegex,
            Self::OpenFile(_) => ToolType::OpenFile,
            Self::LSPDiagnostics(_) => ToolType::LSPDiagnostics,
            Self::TerminalCommand(_) => ToolType::TerminalCommand,
            Self::AskFollowupQuestions(_) => ToolType::AskFollowupQuestions,
            Self::AskExpert(_) => ToolType::AskExpert,
            Self::AttemptCompletion(_) => ToolType::AttemptCompletion,
            Self::RepoMapGeneration(_) => ToolType::RepoMapGeneration,
            Self::TestRunner(_) => ToolType::TestRunner,
            Self::CodeEditorParameters(_) => ToolType::CodeEditorTool,
            Self::SemanticSearch(_) => ToolType::SemanticSearch,
            Self::Reasoning(_) => ToolType::Reasoning,
            Self::FindFile(_) => ToolType::FindFiles,
            Self::RequestScreenshot(_) => ToolType::RequestScreenshot,
            Self::ContextCrunching(_) => ToolType::ContextCrunching,
            Self::McpTool(partial) => ToolType::McpTool(partial.full_name.clone()),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::CodeEditing(code_editing) => code_editing.to_string(),
            Self::ListFiles(list_files) => list_files.to_string(),
            Self::SearchFileContentWithRegex(search_file_content_with_regex) => {
                search_file_content_with_regex.to_string()
            }
            Self::OpenFile(open_file) => open_file.to_string(),
            Self::LSPDiagnostics(lsp_diagnostics) => lsp_diagnostics.to_string(),
            Self::TerminalCommand(terminal_command) => terminal_command.to_string(),
            Self::AskFollowupQuestions(ask_followup_question) => ask_followup_question.to_string(),
            Self::AskExpert(ask_expert) => ask_expert.to_string(),
            Self::AttemptCompletion(attempt_completion) => attempt_completion.to_string(),
            Self::RepoMapGeneration(repo_map_generator) => repo_map_generator.to_string(),
            Self::TestRunner(test_runner_partial_output) => test_runner_partial_output.to_string(),
            Self::CodeEditorParameters(code_editor_parameters) => {
                code_editor_parameters.to_string()
            }
            Self::SemanticSearch(semantic_search_parameters) => {
                semantic_search_parameters.to_string()
            }
            Self::Reasoning(tool_use_reasoning) => tool_use_reasoning.to_string(),
            Self::FindFile(find_file_partial_input) => find_file_partial_input.to_string(),
            Self::RequestScreenshot(request_screenshot) => request_screenshot.to_string(),
            Self::ContextCrunching(context_crunching) => context_crunching.to_string(),
            Self::McpTool(mcp_partial) => mcp_partial.to_string(),
        }
    }

    pub fn to_json_value(&self) -> Option<serde_json::Value> {
        match self {
            Self::CodeEditing(code_editing) => serde_json::to_value(&code_editing).ok(),
            Self::ListFiles(list_files) => serde_json::to_value(&list_files).ok(),
            Self::SearchFileContentWithRegex(search_file_content_with_regex) => {
                serde_json::to_value(&search_file_content_with_regex).ok()
            }
            Self::OpenFile(open_file) => serde_json::to_value(&open_file).ok(),
            Self::LSPDiagnostics(lsp_diagnostics) => serde_json::to_value(&lsp_diagnostics).ok(),
            Self::TerminalCommand(terminal_command) => serde_json::to_value(&terminal_command).ok(),
            Self::AskFollowupQuestions(ask_followup_question) => {
                serde_json::to_value(&ask_followup_question).ok()
            }
            Self::AttemptCompletion(attempt_completion) => {
                serde_json::to_value(&attempt_completion).ok()
            }
            Self::RepoMapGeneration(repo_map_generator) => {
                serde_json::to_value(&repo_map_generator).ok()
            }
            Self::TestRunner(test_runner_partial_output) => {
                serde_json::to_value(&test_runner_partial_output).ok()
            }
            Self::CodeEditorParameters(code_editor_parameters) => {
                serde_json::to_value(&code_editor_parameters).ok()
            }
            Self::SemanticSearch(semantic_search_parameters) => {
                serde_json::to_value(semantic_search_parameters).ok()
            }
            Self::Reasoning(reasoning_input) => serde_json::to_value(reasoning_input).ok(),
            Self::FindFile(find_file_parameters) => serde_json::to_value(find_file_parameters).ok(),
            Self::RequestScreenshot(request_screenshot) => {
                serde_json::to_value(request_screenshot).ok()
            }
            Self::ContextCrunching(context_crunching) => {
                serde_json::to_value(context_crunching).ok()
            }
            Self::McpTool(mcp_partial) => serde_json::to_value(mcp_partial).ok(),
            Self::AskExpert(ask_expert) => serde_json::to_value(ask_expert).ok(),
        }
    }

    pub fn to_json(tool_type: ToolType) -> Option<serde_json::Value> {
        match tool_type {
            ToolType::CodeEditing => None,
            ToolType::ListFiles => Some(ListFilesInputPartial::to_json()),
            ToolType::SearchFileContentWithRegex => Some(SearchFileContentInputPartial::to_json()),
            ToolType::OpenFile => Some(OpenFileRequestPartial::to_json()),
            ToolType::LSPDiagnostics => None,
            ToolType::TerminalCommand => Some(TerminalInputPartial::to_json()),
            ToolType::AskFollowupQuestions => None,
            ToolType::AttemptCompletion => Some(AttemptCompletionClientRequest::to_json()),
            ToolType::RepoMapGeneration => None,
            ToolType::TestRunner => Some(TestRunnerRequestPartial::to_json()),
            ToolType::CodeEditorTool => Some(CodeEditorParameters::to_json()),
            ToolType::RequestScreenshot => Some(RequestScreenshotInputPartial::to_json()),
            ToolType::McpTool(_name) => None,
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ToolInput {
    CodeEditing(CodeEdit),
    LSPDiagnostics(LSPDiagnosticsInput),
    FindCodeSnippets(FindCodeSelectionInput),
    ReRank(ReRankEntriesForBroker),
    CodeSymbolUtilitySearch(CodeSymbolUtilityRequest),
    RequestImportantSymbols(CodeSymbolImportantRequest),
    RequestImportantSymbolsCodeWide(CodeSymbolImportantWideSearch),
    GoToDefinition(GoToDefinitionRequest),
    GoToReference(GoToReferencesRequest),
    OpenFile(OpenFileRequest),
    GrepSingleFile(FindInFileRequest),
    SymbolImplementations(GoToImplementationRequest),
    FilterCodeSnippetsForEditing(CodeToEditFilterRequest),
    FilterCodeSnippetsForEditingSingleSymbols(CodeToEditSymbolRequest),
    EditorApplyChange(EditorApplyRequest),
    QuickFixRequest(GetQuickFixRequest),
    QuickFixInvocationRequest(LSPQuickFixInvocationRequest),
    CodeCorrectnessAction(CodeCorrectnessRequest),
    CodeEditingError(CodeEditingErrorRequest),
    ClassSymbolFollowup(ClassSymbolFollowupRequest),
    // probe request
    ProbeCreateQuestionForSymbol(ProbeQuestionForSymbolRequest),
    ProbeEnoughOrDeeper(ProbeEnoughOrDeeperRequest),
    ProbeFilterSnippetsSingleSymbol(CodeToProbeSubSymbolRequest),
    ProbeSubSymbol(CodeToEditFilterRequest),
    ProbePossibleRequest(CodeSymbolToAskQuestionsRequest),
    ProbeQuestionAskRequest(CodeSymbolToAskQuestionsRequest),
    ProbeFollowAlongSymbol(CodeSymbolFollowAlongForProbing),
    ProbeSummarizeAnswerRequest(CodeSymbolProbingSummarize),
    ProbeTryHardAnswerRequest(ProbeTryHardAnswerSymbolRequest),
    // repo map query
    RepoMapSearch(RepoMapSearchQuery),
    // important files query
    ImportantFilesFinder(ImportantFilesFinderQuery),
    // SWE Bench tooling
    SWEBenchTest(SWEBenchTestRequest),
    // Test output correction
    TestOutputCorrection(TestOutputCorrectionRequest),
    // Code symbol follow initial request
    CodeSymbolFollowInitialRequest(CodeSymbolFollowInitialRequest),
    // Plan before code editing
    PlanningBeforeCodeEdit(PlanningBeforeCodeEditRequest),
    // New symbols required for code editing
    NewSubSymbolForCodeEditing(NewSubSymbolRequiredRequest),
    // Find the symbol in the codebase which we want to select, this only
    // takes a string as input
    GrepSymbolInCodebase(LSPGrepSymbolInCodebaseRequest),
    // Find file location for the new symbol
    FindFileForNewSymbol(FindFileForSymbolRequest),
    // Find symbol to edit in user context
    FindSymbolsToEditInContext(FindSymbolsToEditInContextRequest),
    // ReRanking outline nodes for code editing context
    ReRankingCodeSnippetsForEditing(ReRankingSnippetsForCodeEditingRequest),
    // Apply the generated code outline to the range we are interested in
    ApplyOutlineEditToRange(ApplyOutlineEditsToRangeRequest),
    // Big search
    BigSearch(BigSearchRequest),
    // checks if the edit operation needs to be performed or is an extra
    FilterEditOperation(FilterEditOperationRequest),
    // Keyword search
    KeywordSearch(KeywordSearchQuery),
    // inlay hints from the lsp/editor
    InlayHints(InlayHintsRequest),
    CodeSymbolNewLocation(CodeSymbolNewLocationRequest),
    // should edit the code symbol
    ShouldEditCode(ShouldEditCodeSymbolRequest),
    // search and replace blocks
    SearchAndReplaceEditing(SearchAndReplaceEditingRequest),
    // git diff request
    GitDiff(GitDiffClientRequest),
    OutlineNodesUsingEditor(OutlineNodesUsingEditorRequest),
    // filters references based on user query
    ReferencesFilter(ReferenceFilterRequest),
    // Scratch pad agent input request
    ScratchPadInput(ScratchPadAgentInput),
    // edited files ordered by timestamp
    EditedFiles(EditedFilesRequest),
    // reasoning with just context
    Reasoning(ReasoningRequest),
    // update plan
    UpdatePlan(PlanUpdateRequest),
    // Generate plan steps
    GenerateStep(StepGeneratorRequest),
    // Create file
    CreateFile(CreateFileRequest),
    FileDiagnostics(FileDiagnosticsInput),
    // Plan step add
    PlanStepAdd(PlanAddRequest),
    // Go to previous word in a document
    GoToPreviousWord(GoToPreviousWordRequest),
    // Go to type definition
    GoToTypeDefinition(GoToDefinitionRequest),
    // Context driven chat reply request
    ContextDrivenChatReply(SessionChatClientRequest),
    // Create new exchange for the session
    NewExchangeDuringSession(SessionExchangeNewRequest),
    // Undo changes made during a session
    UndoChangesMadeDuringSession(UndoChangesMadeDuringExchangeRequest),
    // Context drive hot streak reply
    ContextDriveHotStreakReply(SessionHotStreakRequest),
    // Terminal command
    TerminalCommand(TerminalInput),
    // Search file content with regex
    SearchFileContentWithRegex(SearchFileContentInput),
    // List out the files
    ListFiles(ListFilesInput),
    // Ask the user some question
    AskFollowupQuestions(AskFollowupQuestionsRequest),
    // Ask an expert for help
    AskExpert(AskExpertRequest),
    // Attempt completion of a task
    AttemptCompletion(AttemptCompletionClientRequest),
    // Generates the repo map
    RepoMapGeneration(RepoMapGeneratorRequest),
    // Sub process generation input
    SubProcessSpawnedPendingOutput(SubProcessSpawnedPendingOutputRequest),
    // Run tests
    RunTests(TestRunnerRequest),
    // Reward generation
    RewardGeneration(RewardGenerationRequest),
    // Feedback generation
    FeedbackGeneration(FeedbackGenerationRequest),
    // Semantic search input
    SemanticSearch(SemanticSearchRequest),
    // Find files input
    FindFiles(FindFilesRequest),
    // Request screenshot input
    RequestScreenshot(RequestScreenshotInput),
    // Model Context Protocol tool
    McpTool(McpToolInput),
}

impl ToolInput {
    pub fn tool_type(&self) -> ToolType {
        match self {
            ToolInput::SemanticSearch(_) => ToolType::SemanticSearch,
            ToolInput::CodeEditing(_) => ToolType::CodeEditing,
            ToolInput::LSPDiagnostics(_) => ToolType::LSPDiagnostics,
            ToolInput::FindCodeSnippets(_) => ToolType::FindCodeSnippets,
            ToolInput::ReRank(_) => ToolType::ReRank,
            ToolInput::RequestImportantSymbols(_) => ToolType::RequestImportantSymbols,
            ToolInput::RequestImportantSymbolsCodeWide(_) => ToolType::FindCodeSymbolsCodeBaseWide,
            ToolInput::GoToDefinition(_) => ToolType::GoToDefinitions,
            ToolInput::GoToReference(_) => ToolType::GoToReferences,
            ToolInput::OpenFile(_) => ToolType::OpenFile,
            ToolInput::GrepSingleFile(_) => ToolType::GrepInFile,
            ToolInput::SymbolImplementations(_) => ToolType::GoToImplementations,
            ToolInput::FilterCodeSnippetsForEditing(_) => ToolType::FilterCodeSnippetsForEditing,
            ToolInput::FilterCodeSnippetsForEditingSingleSymbols(_) => {
                ToolType::FilterCodeSnippetsSingleSymbolForEditing
            }
            ToolInput::EditorApplyChange(_) => ToolType::EditorApplyEdits,
            ToolInput::CodeSymbolUtilitySearch(_) => ToolType::UtilityCodeSymbolSearch,
            ToolInput::QuickFixRequest(_) => ToolType::GetQuickFix,
            ToolInput::QuickFixInvocationRequest(_) => ToolType::ApplyQuickFix,
            ToolInput::CodeCorrectnessAction(_) => ToolType::CodeCorrectnessActionSelection,
            ToolInput::CodeEditingError(_) => ToolType::CodeEditingForError,
            ToolInput::ClassSymbolFollowup(_) => ToolType::ClassSymbolFollowup,
            ToolInput::ProbePossibleRequest(_) => ToolType::ProbePossible,
            ToolInput::ProbeQuestionAskRequest(_) => ToolType::ProbeQuestion,
            ToolInput::ProbeSubSymbol(_) => ToolType::ProbeSubSymbol,
            ToolInput::ProbeFollowAlongSymbol(_) => ToolType::ProbeFollowAlongSymbol,
            ToolInput::ProbeSummarizeAnswerRequest(_) => ToolType::ProbeSummarizeAnswer,
            ToolInput::RepoMapSearch(_) => ToolType::RepoMapSearch,
            ToolInput::ImportantFilesFinder(_) => ToolType::ImportantFilesFinder,
            ToolInput::SWEBenchTest(_) => ToolType::SWEBenchToolEndpoint,
            ToolInput::TestOutputCorrection(_) => ToolType::TestCorrection,
            ToolInput::CodeSymbolFollowInitialRequest(_) => {
                ToolType::CodeSymbolsToFollowInitialRequest
            }
            ToolInput::ProbeFilterSnippetsSingleSymbol(_) => ToolType::ProbeSubSymbolFiltering,
            ToolInput::ProbeEnoughOrDeeper(_) => ToolType::ProbeEnoughOrDeeper,
            ToolInput::ProbeCreateQuestionForSymbol(_) => ToolType::ProbeCreateQuestionForSymbol,
            ToolInput::PlanningBeforeCodeEdit(_) => ToolType::PlanningBeforeCodeEdit,
            ToolInput::NewSubSymbolForCodeEditing(_) => ToolType::NewSubSymbolRequired,
            ToolInput::ProbeTryHardAnswerRequest(_) => ToolType::ProbeTryHardAnswer,
            ToolInput::GrepSymbolInCodebase(_) => ToolType::GrepSymbolInCodebase,
            ToolInput::FindFileForNewSymbol(_) => ToolType::FindFileForNewSymbol,
            ToolInput::FindSymbolsToEditInContext(_) => ToolType::FindSymbolsToEditInContext,
            ToolInput::ReRankingCodeSnippetsForEditing(_) => {
                ToolType::ReRankingCodeSnippetsForCodeEditingContext
            }
            ToolInput::ApplyOutlineEditToRange(_) => ToolType::ApplyOutlineEditToRange,
            ToolInput::BigSearch(_) => ToolType::BigSearch,
            ToolInput::FilterEditOperation(_) => ToolType::FilterEditOperation,
            ToolInput::KeywordSearch(_) => ToolType::KeywordSearch,
            ToolInput::InlayHints(_) => ToolType::InLayHints,
            ToolInput::CodeSymbolNewLocation(_) => ToolType::CodeSymbolNewLocation,
            ToolInput::ShouldEditCode(_) => ToolType::ShouldEditCode,
            ToolInput::SearchAndReplaceEditing(_) => ToolType::SearchAndReplaceEditing,
            ToolInput::GitDiff(_) => ToolType::GitDiff,
            ToolInput::OutlineNodesUsingEditor(_) => ToolType::OutlineNodesUsingEditor,
            ToolInput::ReferencesFilter(_) => ToolType::ReferencesFilter,
            ToolInput::ScratchPadInput(_) => ToolType::ScratchPadAgent,
            ToolInput::EditedFiles(_) => ToolType::EditedFiles,
            ToolInput::Reasoning(_) => ToolType::Reasoning,
            ToolInput::UpdatePlan(_) => ToolType::PlanUpdater,
            ToolInput::GenerateStep(_) => ToolType::StepGenerator,
            ToolInput::CreateFile(_) => ToolType::CreateFile,
            ToolInput::FileDiagnostics(_) => ToolType::FileDiagnostics,
            ToolInput::PlanStepAdd(_) => ToolType::PlanStepAdd,
            ToolInput::GoToPreviousWord(_) => ToolType::GoToPreviousWordRange,
            ToolInput::GoToTypeDefinition(_) => ToolType::GoToTypeDefinition,
            ToolInput::ContextDrivenChatReply(_) => ToolType::ContextDrivenChatReply,
            ToolInput::NewExchangeDuringSession(_) => ToolType::NewExchangeDuringSession,
            ToolInput::UndoChangesMadeDuringSession(_) => ToolType::UndoChangesMadeDuringSession,
            ToolInput::ContextDriveHotStreakReply(_) => ToolType::ContextDriveHotStreakReply,
            ToolInput::TerminalCommand(_) => ToolType::TerminalCommand,
            ToolInput::SearchFileContentWithRegex(_) => ToolType::SearchFileContentWithRegex,
            ToolInput::ListFiles(_) => ToolType::ListFiles,
            ToolInput::AskFollowupQuestions(_) => ToolType::AskFollowupQuestions,
            ToolInput::AskExpert(_) => ToolType::AskExpert,
            ToolInput::AttemptCompletion(_) => ToolType::AttemptCompletion,
            ToolInput::RepoMapGeneration(_) => ToolType::RepoMapGeneration,
            ToolInput::SubProcessSpawnedPendingOutput(_) => {
                ToolType::SubProcessSpawnedPendingOutput
            }
            ToolInput::RunTests(_) => ToolType::TestRunner,
            ToolInput::RewardGeneration(_) => ToolType::RewardGeneration,
            ToolInput::FeedbackGeneration(_) => ToolType::FeedbackGeneration,
            ToolInput::FindFiles(_) => ToolType::FindFiles,
            ToolInput::RequestScreenshot(_) => ToolType::RequestScreenshot,
            ToolInput::McpTool(inp) => ToolType::McpTool(inp.partial.full_name.clone()),
        }
    }

    pub fn is_find_files(self) -> Result<FindFilesRequest, ToolError> {
        if let ToolInput::FindFiles(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FindFiles))
        }
    }

    pub fn is_semantic_search(self) -> Result<SemanticSearchRequest, ToolError> {
        if let ToolInput::SemanticSearch(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::SemanticSearch))
        }
    }

    pub fn is_feedback_generation_request(self) -> Result<FeedbackGenerationRequest, ToolError> {
        if let ToolInput::FeedbackGeneration(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FeedbackGeneration))
        }
    }

    pub fn is_reward_generation_request(self) -> Result<RewardGenerationRequest, ToolError> {
        if let ToolInput::RewardGeneration(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::RewardGeneration))
        }
    }

    pub fn is_subprocess_spawn_pending_output(
        self,
    ) -> Result<SubProcessSpawnedPendingOutputRequest, ToolError> {
        if let ToolInput::SubProcessSpawnedPendingOutput(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::SubProcessSpawnedPendingOutput,
            ))
        }
    }

    pub fn is_repo_map_generation(self) -> Result<RepoMapGeneratorRequest, ToolError> {
        if let ToolInput::RepoMapGeneration(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::RepoMapGeneration))
        }
    }

    pub fn is_test_runner(self) -> Result<TestRunnerRequest, ToolError> {
        if let ToolInput::RunTests(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::TestRunner))
        }
    }

    pub fn is_attempt_completion(self) -> Result<AttemptCompletionClientRequest, ToolError> {
        if let ToolInput::AttemptCompletion(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::AttemptCompletion))
        }
    }

    pub fn is_ask_followup_questions(self) -> Result<AskFollowupQuestionsRequest, ToolError> {
        if let ToolInput::AskFollowupQuestions(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::AskFollowupQuestions))
        }
    }

    pub fn is_ask_expert(self) -> Result<AskExpertRequest, ToolError> {
        if let ToolInput::AskExpert(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::AskExpert))
        }
    }

    pub fn is_list_files(self) -> Result<ListFilesInput, ToolError> {
        if let ToolInput::ListFiles(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ListFiles))
        }
    }

    pub fn is_search_file_content_with_regex(self) -> Result<SearchFileContentInput, ToolError> {
        if let ToolInput::SearchFileContentWithRegex(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::SearchFileContentWithRegex,
            ))
        }
    }

    pub fn is_context_driven_hot_streak_reply(self) -> Result<SessionHotStreakRequest, ToolError> {
        if let ToolInput::ContextDriveHotStreakReply(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::ContextDriveHotStreakReply,
            ))
        }
    }

    pub fn is_undo_request_during_session(
        self,
    ) -> Result<UndoChangesMadeDuringExchangeRequest, ToolError> {
        if let ToolInput::UndoChangesMadeDuringSession(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::UndoChangesMadeDuringSession,
            ))
        }
    }

    pub fn is_new_exchange_during_session(self) -> Result<SessionExchangeNewRequest, ToolError> {
        if let ToolInput::NewExchangeDuringSession(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::NewExchangeDuringSession,
            ))
        }
    }

    pub fn is_session_context_driven_chat_reply(
        self,
    ) -> Result<SessionChatClientRequest, ToolError> {
        if let ToolInput::ContextDrivenChatReply(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ContextDrivenChatReply))
        }
    }

    pub fn is_go_to_type_definition(self) -> Result<GoToDefinitionRequest, ToolError> {
        if let ToolInput::GoToTypeDefinition(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GoToTypeDefinition))
        }
    }

    pub fn is_go_to_previous_word_request(self) -> Result<GoToPreviousWordRequest, ToolError> {
        if let ToolInput::GoToPreviousWord(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GoToPreviousWordRange))
        }
    }

    pub fn is_plan_step_add(self) -> Result<PlanAddRequest, ToolError> {
        if let ToolInput::PlanStepAdd(input) = self {
            Ok(input)
        } else {
            Err(ToolError::WrongToolInput(ToolType::PlanStepAdd))
        }
    }

    pub fn is_file_diagnostics(self) -> Result<FileDiagnosticsInput, ToolError> {
        if let ToolInput::FileDiagnostics(input) = self {
            Ok(input)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FileDiagnostics))
        }
    }

    pub fn should_reasoning(self) -> Result<ReasoningRequest, ToolError> {
        if let ToolInput::Reasoning(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::Reasoning))
        }
    }

    pub fn should_edited_files(self) -> Result<EditedFilesRequest, ToolError> {
        if let ToolInput::EditedFiles(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::EditedFiles))
        }
    }

    pub fn should_scratch_pad_input(self) -> Result<ScratchPadAgentInput, ToolError> {
        if let ToolInput::ScratchPadInput(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ScratchPadAgent))
        }
    }

    pub fn should_outline_nodes_using_editor(
        self,
    ) -> Result<OutlineNodesUsingEditorRequest, ToolError> {
        if let ToolInput::OutlineNodesUsingEditor(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::OutlineNodesUsingEditor))
        }
    }

    pub fn should_git_diff(self) -> Result<GitDiffClientRequest, ToolError> {
        if let ToolInput::GitDiff(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GitDiff))
        }
    }

    pub fn should_search_and_replace_editing(
        self,
    ) -> Result<SearchAndReplaceEditingRequest, ToolError> {
        if let ToolInput::SearchAndReplaceEditing(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::SearchAndReplaceEditing))
        }
    }

    pub fn should_edit_code(self) -> Result<ShouldEditCodeSymbolRequest, ToolError> {
        if let ToolInput::ShouldEditCode(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ShouldEditCode))
        }
    }

    pub fn code_symbol_new_location(self) -> Result<CodeSymbolNewLocationRequest, ToolError> {
        if let ToolInput::CodeSymbolNewLocation(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::CodeSymbolNewLocation))
        }
    }

    pub fn inlay_hints_request(self) -> Result<InlayHintsRequest, ToolError> {
        if let ToolInput::InlayHints(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::InLayHints))
        }
    }

    pub fn filter_edit_operation_request(self) -> Result<FilterEditOperationRequest, ToolError> {
        if let ToolInput::FilterEditOperation(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FilterEditOperation))
        }
    }

    pub fn filter_references_request(self) -> Result<ReferenceFilterRequest, ToolError> {
        if let ToolInput::ReferencesFilter(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ReferencesFilter))
        }
    }

    pub fn apply_outline_edits_to_range(
        self,
    ) -> Result<ApplyOutlineEditsToRangeRequest, ToolError> {
        if let ToolInput::ApplyOutlineEditToRange(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ApplyOutlineEditToRange))
        }
    }

    pub fn reranking_code_snippets_for_editing_context(
        self,
    ) -> Result<ReRankingSnippetsForCodeEditingRequest, ToolError> {
        if let ToolInput::ReRankingCodeSnippetsForEditing(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::ReRankingCodeSnippetsForCodeEditingContext,
            ))
        }
    }

    pub fn find_symbols_to_edit_in_context(
        self,
    ) -> Result<FindSymbolsToEditInContextRequest, ToolError> {
        if let ToolInput::FindSymbolsToEditInContext(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::FindSymbolsToEditInContext,
            ))
        }
    }

    pub fn find_file_for_new_symbol(self) -> Result<FindFileForSymbolRequest, ToolError> {
        if let ToolInput::FindFileForNewSymbol(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FindFileForNewSymbol))
        }
    }

    pub fn grep_symbol_in_codebase(self) -> Result<LSPGrepSymbolInCodebaseRequest, ToolError> {
        if let ToolInput::GrepSymbolInCodebase(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GrepSymbolInCodebase))
        }
    }

    pub fn get_probe_try_hard_answer_request(
        self,
    ) -> Result<ProbeTryHardAnswerSymbolRequest, ToolError> {
        if let ToolInput::ProbeTryHardAnswerRequest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeTryHardAnswer))
        }
    }

    pub fn probe_try_hard_answer(request: ProbeTryHardAnswerSymbolRequest) -> Self {
        ToolInput::ProbeTryHardAnswerRequest(request)
    }

    pub fn get_new_sub_symbol_for_code_editing(
        self,
    ) -> Result<NewSubSymbolRequiredRequest, ToolError> {
        if let ToolInput::NewSubSymbolForCodeEditing(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::NewSubSymbolRequired))
        }
    }

    pub fn probe_create_question_for_symbol(request: ProbeQuestionForSymbolRequest) -> Self {
        ToolInput::ProbeCreateQuestionForSymbol(request)
    }

    pub fn get_probe_create_question_for_symbol(
        self,
    ) -> Result<ProbeQuestionForSymbolRequest, ToolError> {
        if let ToolInput::ProbeCreateQuestionForSymbol(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::ProbeCreateQuestionForSymbol,
            ))
        }
    }

    pub fn probe_enough_or_deeper(request: ProbeEnoughOrDeeperRequest) -> Self {
        ToolInput::ProbeEnoughOrDeeper(request)
    }

    pub fn get_probe_enough_or_deeper(self) -> Result<ProbeEnoughOrDeeperRequest, ToolError> {
        if let ToolInput::ProbeEnoughOrDeeper(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeEnoughOrDeeper))
        }
    }

    pub fn probe_filter_snippets_single_symbol(request: CodeToProbeSubSymbolRequest) -> Self {
        ToolInput::ProbeFilterSnippetsSingleSymbol(request)
    }

    pub fn is_probe_filter_snippets_single_symbol(&self) -> bool {
        if let ToolInput::ProbeFilterSnippetsSingleSymbol(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_code_symbol_follow_initial_request(
        self,
    ) -> Result<CodeSymbolFollowInitialRequest, ToolError> {
        if let ToolInput::CodeSymbolFollowInitialRequest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::CodeSymbolsToFollowInitialRequest,
            ))
        }
    }

    pub fn is_test_output(self) -> Result<TestOutputCorrectionRequest, ToolError> {
        if let ToolInput::TestOutputCorrection(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::TestCorrection))
        }
    }

    pub fn is_probe_subsymbol(&self) -> bool {
        if let ToolInput::ProbeSubSymbol(_) = self {
            true
        } else {
            false
        }
    }

    pub fn swe_bench_test(self) -> Result<SWEBenchTestRequest, ToolError> {
        if let ToolInput::SWEBenchTest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::SWEBenchToolEndpoint))
        }
    }

    pub fn repo_map_search_query(self) -> Result<RepoMapSearchQuery, ToolError> {
        if let ToolInput::RepoMapSearch(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::RepoMapSearch))
        }
    }

    pub fn important_files_finder_query(self) -> Result<ImportantFilesFinderQuery, ToolError> {
        if let ToolInput::ImportantFilesFinder(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ImportantFilesFinder))
        }
    }

    pub fn keyword_search_query(self) -> Result<KeywordSearchQuery, ToolError> {
        if let ToolInput::KeywordSearch(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::KeywordSearch))
        }
    }

    pub fn big_search_query(self) -> Result<BigSearchRequest, ToolError> {
        if let ToolInput::BigSearch(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::BigSearch))
        }
    }

    pub fn probe_sub_symbol_filtering(self) -> Result<CodeToProbeSubSymbolRequest, ToolError> {
        if let ToolInput::ProbeFilterSnippetsSingleSymbol(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeSubSymbolFiltering))
        }
    }

    pub fn probe_subsymbol(self) -> Result<CodeToEditFilterRequest, ToolError> {
        if let ToolInput::ProbeSubSymbol(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeSubSymbol))
        }
    }

    pub fn probe_possible_request(self) -> Result<CodeSymbolToAskQuestionsRequest, ToolError> {
        if let ToolInput::ProbePossibleRequest(output) = self {
            Ok(output)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbePossible))
        }
    }

    pub fn probe_question_request(self) -> Result<CodeSymbolToAskQuestionsRequest, ToolError> {
        if let ToolInput::ProbeQuestionAskRequest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeQuestion))
        }
    }

    pub fn probe_follow_along_symbol(self) -> Result<CodeSymbolFollowAlongForProbing, ToolError> {
        if let ToolInput::ProbeFollowAlongSymbol(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeFollowAlongSymbol))
        }
    }

    pub fn probe_summarization_request(self) -> Result<CodeSymbolProbingSummarize, ToolError> {
        if let ToolInput::ProbeSummarizeAnswerRequest(response) = self {
            Ok(response)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ProbeSummarizeAnswer))
        }
    }

    pub fn is_probe_summarization_request(&self) -> bool {
        if let ToolInput::ProbeSummarizeAnswerRequest(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_repo_map_search(&self) -> bool {
        if let ToolInput::RepoMapSearch(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_probe_follow_along_symbol_request(&self) -> bool {
        if let ToolInput::ProbeFollowAlongSymbol(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_probe_possible_request(&self) -> bool {
        if let ToolInput::ProbePossibleRequest(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_probe_question(&self) -> bool {
        if let ToolInput::ProbeQuestionAskRequest(_) = self {
            true
        } else {
            false
        }
    }

    pub fn code_editing_error(self) -> Result<CodeEditingErrorRequest, ToolError> {
        if let ToolInput::CodeEditingError(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::CodeEditingForError))
        }
    }

    pub fn code_correctness_action(self) -> Result<CodeCorrectnessRequest, ToolError> {
        if let ToolInput::CodeCorrectnessAction(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::CodeCorrectnessActionSelection,
            ))
        }
    }

    pub fn quick_fix_invocation_request(self) -> Result<LSPQuickFixInvocationRequest, ToolError> {
        if let ToolInput::QuickFixInvocationRequest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GetQuickFix))
        }
    }

    pub fn quick_fix_request(self) -> Result<GetQuickFixRequest, ToolError> {
        if let ToolInput::QuickFixRequest(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ApplyQuickFix))
        }
    }

    pub fn editor_apply_changes(self) -> Result<EditorApplyRequest, ToolError> {
        if let ToolInput::EditorApplyChange(editor_apply_request) = self {
            Ok(editor_apply_request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::EditorApplyEdits))
        }
    }

    pub fn symbol_implementations(self) -> Result<GoToImplementationRequest, ToolError> {
        if let ToolInput::SymbolImplementations(symbol_implementations) = self {
            Ok(symbol_implementations)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GoToImplementations))
        }
    }

    pub fn reference_request(self) -> Result<GoToReferencesRequest, ToolError> {
        if let ToolInput::GoToReference(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GoToReferences))
        }
    }

    pub fn class_symbol_followup(self) -> Result<ClassSymbolFollowupRequest, ToolError> {
        if let ToolInput::ClassSymbolFollowup(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ClassSymbolFollowup))
        }
    }

    pub fn grep_single_file(self) -> Result<FindInFileRequest, ToolError> {
        if let ToolInput::GrepSingleFile(grep_single_file) = self {
            Ok(grep_single_file)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GrepInFile))
        }
    }

    pub fn is_file_open(self) -> Result<OpenFileRequest, ToolError> {
        if let ToolInput::OpenFile(open_file) = self {
            Ok(open_file)
        } else {
            Err(ToolError::WrongToolInput(ToolType::OpenFile))
        }
    }

    pub fn is_go_to_definition(self) -> Result<GoToDefinitionRequest, ToolError> {
        if let ToolInput::GoToDefinition(definition_request) = self {
            Ok(definition_request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::GoToDefinitions))
        }
    }

    pub fn is_code_edit(self) -> Result<CodeEdit, ToolError> {
        if let ToolInput::CodeEditing(code_edit) = self {
            Ok(code_edit)
        } else {
            Err(ToolError::WrongToolInput(ToolType::CodeEditing))
        }
    }

    pub fn is_lsp_diagnostics(self) -> Result<LSPDiagnosticsInput, ToolError> {
        if let ToolInput::LSPDiagnostics(lsp_diagnostics) = self {
            Ok(lsp_diagnostics)
        } else {
            Err(ToolError::WrongToolInput(ToolType::LSPDiagnostics))
        }
    }

    pub fn is_code_find(self) -> Result<FindCodeSelectionInput, ToolError> {
        if let ToolInput::FindCodeSnippets(find_code_snippets) = self {
            Ok(find_code_snippets)
        } else {
            Err(ToolError::WrongToolInput(ToolType::FindCodeSnippets))
        }
    }

    pub fn is_rerank(self) -> Result<ReRankEntriesForBroker, ToolError> {
        if let ToolInput::ReRank(rerank) = self {
            Ok(rerank)
        } else {
            Err(ToolError::WrongToolInput(ToolType::ReRank))
        }
    }

    pub fn is_utility_code_search(&self) -> bool {
        if let ToolInput::CodeSymbolUtilitySearch(_) = self {
            true
        } else {
            false
        }
    }

    pub fn utility_code_search(self) -> Result<CodeSymbolUtilityRequest, ToolError> {
        if let ToolInput::CodeSymbolUtilitySearch(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::UtilityCodeSymbolSearch))
        }
    }

    pub fn code_symbol_search(
        self,
    ) -> Result<either::Either<CodeSymbolImportantRequest, CodeSymbolImportantWideSearch>, ToolError>
    {
        if let ToolInput::RequestImportantSymbols(request_code_symbol_important) = self {
            Ok(either::Either::Left(request_code_symbol_important))
        } else if let ToolInput::RequestImportantSymbolsCodeWide(request_code_symbol_important) =
            self
        {
            Ok(either::Either::Right(request_code_symbol_important))
        } else {
            Err(ToolError::WrongToolInput(ToolType::UtilityCodeSymbolSearch))
        }
    }

    pub fn filter_code_snippets_for_editing(self) -> Result<CodeToEditFilterRequest, ToolError> {
        if let ToolInput::FilterCodeSnippetsForEditing(filter_code_snippets_for_editing) = self {
            Ok(filter_code_snippets_for_editing)
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::FilterCodeSnippetsForEditing,
            ))
        }
    }

    pub fn filter_code_snippets_request(
        self,
    ) -> Result<either::Either<CodeToEditFilterRequest, CodeToEditSymbolRequest>, ToolError> {
        if let ToolInput::FilterCodeSnippetsForEditing(filter_code_snippets_for_editing) = self {
            Ok(either::Left(filter_code_snippets_for_editing))
        } else if let ToolInput::FilterCodeSnippetsForEditingSingleSymbols(
            filter_code_snippets_for_editing,
        ) = self
        {
            Ok(either::Right(filter_code_snippets_for_editing))
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::FilterCodeSnippetsForEditing,
            ))
        }
    }

    pub fn plan_before_code_editing(self) -> Result<PlanningBeforeCodeEditRequest, ToolError> {
        if let ToolInput::PlanningBeforeCodeEdit(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::PlanningBeforeCodeEdit))
        }
    }

    pub fn plan_updater(self) -> Result<PlanUpdateRequest, ToolError> {
        if let ToolInput::UpdatePlan(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::PlanUpdater))
        }
    }

    pub fn step_generator(self) -> Result<StepGeneratorRequest, ToolError> {
        if let ToolInput::GenerateStep(request) = self {
            Ok(request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::StepGenerator))
        }
    }

    pub fn is_file_create(self) -> Result<CreateFileRequest, ToolError> {
        if let ToolInput::CreateFile(create_file) = self {
            Ok(create_file)
        } else {
            Err(ToolError::WrongToolInput(ToolType::CreateFile))
        }
    }

    pub fn is_terminal_command(self) -> Result<TerminalInput, ToolError> {
        if let ToolInput::TerminalCommand(terminal_command) = self {
            Ok(terminal_command)
        } else {
            Err(ToolError::WrongToolInput(ToolType::TerminalCommand))
        }
    }

    pub fn screenshot_request(self) -> Result<RequestScreenshotInput, ToolError> {
        if let ToolInput::RequestScreenshot(screenshot_request) = self {
            Ok(screenshot_request)
        } else {
            Err(ToolError::WrongToolInput(ToolType::RequestScreenshot))
        }
    }
}