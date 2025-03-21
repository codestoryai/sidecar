//! Contains the output of a tool which can be used by any of the callers

use crate::agentic::symbol::ui_event::RelevantReference;
use crate::agentic::tool::mcp::integration_tool::McpToolResponse;

use super::{
    code_edit::{
        filter_edit::FilterEditOperationResponse,
        search_and_replace::SearchAndReplaceEditingResponse,
    },
    code_symbol::{
        apply_outline_edit_to_range::ApplyOutlineEditsToRangeResponse,
        correctness::CodeCorrectnessAction,
        find_file_for_new_symbol::FindFileForSymbolResponse,
        find_symbols_to_edit_in_context::FindSymbolsToEditInContextResponse,
        followup::ClassSymbolFollowupResponse,
        important::CodeSymbolImportantResponse,
        initial_request_follow::CodeSymbolFollowInitialResponse,
        models::anthropic::{
            CodeSymbolShouldAskQuestionsResponse, CodeSymbolToAskQuestionsResponse, ProbeNextSymbol,
        },
        new_location::CodeSymbolNewLocationResponse,
        new_sub_symbol::NewSubSymbolRequiredResponse,
        planning_before_code_edit::PlanningBeforeCodeEditResponse,
        probe::ProbeEnoughOrDeeperResponse,
        reranking_symbols_for_editing_context::ReRankingSnippetsForCodeEditingResponse,
        should_edit::ShouldEditCodeSymbolResponse,
    },
    devtools::screenshot::RequestScreenshotOutput,
    editor::apply::EditorApplyResponse,
    feedback::feedback::FeedbackGenerationResponse,
    file::{important::FileImportantResponse, semantic_search::SemanticSearchResponse},
    filtering::broker::{
        CodeToEditFilterResponse, CodeToEditSymbolResponse, CodeToProbeFilterResponse,
        CodeToProbeSubSymbolList,
    },
    git::{diff_client::GitDiffClientResponse, edited_files::EditedFilesResponse},
    grep::file::FindInFileResponse,
    lsp::{
        create_file::CreateFileResponse,
        diagnostics::LSPDiagnosticsOutput,
        file_diagnostics::FileDiagnosticsOutput,
        find_files::FindFilesResponse,
        get_outline_nodes::OutlineNodesUsingEditorResponse,
        go_to_previous_word::GoToPreviousWordResponse,
        gotodefintion::GoToDefinitionResponse,
        gotoimplementations::GoToImplementationResponse,
        gotoreferences::GoToReferencesResponse,
        grep_symbol::LSPGrepSymbolInCodebaseResponse,
        inlay_hints::InlayHintsResponse,
        list_files::ListFilesOutput,
        open_file::OpenFileResponse,
        quick_fix::{GetQuickFixResponse, LSPQuickFixInvocationResponse},
        search_file::SearchFileContentWithRegexOutput,
        subprocess_spawned_output::SubProcessSpanwedPendingOutputResponse,
        undo_changes::UndoChangesMadeDuringExchangeRespnose,
    },
    plan::{generator::StepGeneratorResponse, reasoning::ReasoningResponse},
    repo_map::generator::RepoMapGeneratorResponse,
    rerank::base::ReRankEntriesForBroker,
    reward::client::RewardGenerationResponse,
    session::{
        ask_followup_question::AskFollowupQuestionsResponse,
        attempt_completion::AttemptCompletionClientResponse, chat::SessionChatClientResponse,
        exchange::SessionExchangeNewResponse, hot_streak::SessionHotStreakResponse,
    },
    swe_bench::test_tool::SWEBenchTestRepsonse,
    terminal::terminal::TerminalOutput,
    test_runner::runner::TestRunnerResponse,
};

#[derive(Debug)]
pub struct CodeToEditSnippet {
    start_line: i64,
    end_line: i64,
    thinking: String,
}

impl CodeToEditSnippet {
    pub fn start_line(&self) -> i64 {
        self.start_line
    }

    pub fn end_line(&self) -> i64 {
        self.end_line
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}

#[derive(Debug)]
pub struct CodeToEditToolOutput {
    snipets: Vec<CodeToEditSnippet>,
}

impl CodeToEditToolOutput {
    pub fn new() -> Self {
        CodeToEditToolOutput { snipets: vec![] }
    }

    pub fn add_snippet(&mut self, start_line: i64, end_line: i64, thinking: String) {
        self.snipets.push(CodeToEditSnippet {
            start_line,
            end_line,
            thinking,
        });
    }
}

#[derive(Debug)]
pub enum ToolOutput {
    PlanningBeforeCodeEditing(PlanningBeforeCodeEditResponse),
    CodeEditTool(String),
    LSPDiagnostics(LSPDiagnosticsOutput),
    CodeToEdit(CodeToEditToolOutput),
    ReRankSnippets(ReRankEntriesForBroker),
    ImportantSymbols(CodeSymbolImportantResponse),
    GoToDefinition(GoToDefinitionResponse),
    GoToReference(GoToReferencesResponse),
    FileOpen(OpenFileResponse),
    GrepSingleFile(FindInFileResponse),
    GoToImplementation(GoToImplementationResponse),
    CodeToEditSnippets(CodeToEditFilterResponse),
    CodeToEditSingleSymbolSnippets(CodeToEditSymbolResponse),
    EditorApplyChanges(EditorApplyResponse),
    UtilityCodeSearch(CodeSymbolImportantResponse),
    GetQuickFixList(GetQuickFixResponse),
    LSPQuickFixInvoation(LSPQuickFixInvocationResponse),
    CodeCorrectnessAction(CodeCorrectnessAction),
    CodeEditingForError(String),
    ClassSymbolFollowupResponse(ClassSymbolFollowupResponse),
    // Probe requests
    ProbeCreateQuestionForSymbol(String),
    ProbeEnoughOrDeeper(ProbeEnoughOrDeeperResponse),
    ProbeSubSymbolFiltering(CodeToProbeSubSymbolList),
    ProbePossible(CodeSymbolShouldAskQuestionsResponse),
    ProbeQuestion(CodeSymbolToAskQuestionsResponse),
    ProbeSubSymbol(CodeToProbeFilterResponse),
    ProbeFollowAlongSymbol(ProbeNextSymbol),
    ProbeSummarizationResult(String),
    ProbeTryHardAnswer(String),
    // Repo map result
    RepoMapSearch(CodeSymbolImportantResponse),
    // important files result
    ImportantFilesFinder(FileImportantResponse),
    // Big search result
    BigSearch(CodeSymbolImportantResponse),
    // SWE Bench test output
    SWEBenchTestOutput(SWEBenchTestRepsonse),
    // Test correction output
    TestCorrectionOutput(String),
    // Code Symbol follow for initial request
    CodeSymbolFollowForInitialRequest(CodeSymbolFollowInitialResponse),
    // New sub symbol creation
    NewSubSymbolCreation(NewSubSymbolRequiredResponse),
    // LSP symbol search information
    LSPSymbolSearchInformation(LSPGrepSymbolInCodebaseResponse),
    // Find the file for the symbol
    FindFileForNewSymbol(FindFileForSymbolResponse),
    // Find symbols to edit in the user context
    FindSymbolsToEditInContext(FindSymbolsToEditInContextResponse),
    // the outline nodes which we should use as context for the code editing
    ReRankedCodeSnippetsForCodeEditing(ReRankingSnippetsForCodeEditingResponse),
    // Apply outline edits to the range
    ApplyOutlineEditsToRange(ApplyOutlineEditsToRangeResponse),
    // Filter the edit operations and its reponse
    FilterEditOperation(FilterEditOperationResponse),
    // Keyword search
    KeywordSearch(CodeSymbolImportantResponse),
    // Inlay hints response
    InlayHints(InlayHintsResponse),
    // code symbol new location
    CodeSymbolNewLocation(CodeSymbolNewLocationResponse),
    // should edit the code
    ShouldEditCode(ShouldEditCodeSymbolResponse),
    // search and replace editing
    SearchAndReplaceEditing(SearchAndReplaceEditingResponse),
    // git diff response
    GitDiff(GitDiffClientResponse),
    // outline nodes from the editor
    OutlineNodesUsingEditor(OutlineNodesUsingEditorResponse),
    // filter reference
    ReferencesFilter(Vec<RelevantReference>),
    // edited files with timestamps (git-diff)
    EditedFiles(EditedFilesResponse),
    // reasoning output
    Reasoning(ReasoningResponse),
    // plan update output
    PlanUpdater(),
    // Step generator
    StepGenerator(StepGeneratorResponse),
    // File create
    FileCreate(CreateFileResponse),
    // File diagnostics
    FileDiagnostics(FileDiagnosticsOutput),
    // Plan add step
    PlanAddStep(StepGeneratorResponse),
    // Go to previous word
    GoToPreviousWord(GoToPreviousWordResponse),
    // Go to type definition
    GoToTypeDefinition(GoToDefinitionResponse),
    // context driven chat reply
    ContextDriveChatReply(SessionChatClientResponse),
    // creates a new exchange for the session
    NewExchangeDuringSession(SessionExchangeNewResponse),
    // undo changes made during the session
    UndoChangesMadeDuringSession(UndoChangesMadeDuringExchangeRespnose),
    // context drive hot streak reply
    ContextDriveHotStreakReply(SessionHotStreakResponse),
    // terminal command
    TerminalCommand(TerminalOutput),
    // Formatted output after running ripgrep to search for a pattern
    SearchFileContentWithRegex(SearchFileContentWithRegexOutput),
    // Listed out files from a directory traversal
    ListFiles(ListFilesOutput),
    // ask question followup response
    AskFollowupQuestions(AskFollowupQuestionsResponse),
    // attempt completion
    AttemptCompletion(AttemptCompletionClientResponse),
    // Repo map generation response
    RepoMapGeneration(RepoMapGeneratorResponse),
    // spawned subprocess and their output which is pending
    SubProcessSpawnedPendingOutput(SubProcessSpanwedPendingOutputResponse),
    // Test runner
    TestRunner(TestRunnerResponse),
    // Reward generation
    RewardGeneration(RewardGenerationResponse),
    // Feedback generation
    FeedbackGeneration(FeedbackGenerationResponse),
    // Semantic search file level response
    SemanticSearch(SemanticSearchResponse),
    // Find files output
    FindFiles(FindFilesResponse),
    // Request screenshot output
    RequestScreenshot(RequestScreenshotOutput),
    // dynamically configured MCP servers
    McpTool(McpToolResponse),
}

macro_rules! impl_output {
    ($name:ident, $variant:ident, $type:ty) => {
        pub fn $name(self) -> Option<$type> {
            match self {
                ToolOutput::$variant(response) => Some(response),
                _ => None,
            }
        }
    };
}

impl ToolOutput {
    pub fn sub_process_spawned_pending_output(
        response: SubProcessSpanwedPendingOutputResponse,
    ) -> Self {
        ToolOutput::SubProcessSpawnedPendingOutput(response)
    }

    pub fn repo_map_generation_reponse(response: RepoMapGeneratorResponse) -> Self {
        ToolOutput::RepoMapGeneration(response)
    }

    pub fn search_file_content_with_regex(response: SearchFileContentWithRegexOutput) -> Self {
        ToolOutput::SearchFileContentWithRegex(response)
    }

    pub fn context_driven_hot_streak_reply(response: SessionHotStreakResponse) -> Self {
        ToolOutput::ContextDriveHotStreakReply(response)
    }

    pub fn undo_changes_made_during_session(
        response: UndoChangesMadeDuringExchangeRespnose,
    ) -> Self {
        ToolOutput::UndoChangesMadeDuringSession(response)
    }

    pub fn new_exchange_during_session(response: SessionExchangeNewResponse) -> Self {
        ToolOutput::NewExchangeDuringSession(response)
    }

    pub fn context_driven_chat_reply(response: SessionChatClientResponse) -> Self {
        ToolOutput::ContextDriveChatReply(response)
    }

    pub fn go_to_type_definition(response: GoToDefinitionResponse) -> Self {
        ToolOutput::GoToTypeDefinition(response)
    }

    pub fn go_to_previous_word(response: GoToPreviousWordResponse) -> Self {
        ToolOutput::GoToPreviousWord(response)
    }

    pub fn plan_add_step(response: StepGeneratorResponse) -> Self {
        ToolOutput::PlanAddStep(response)
    }

    pub fn reasoning(response: ReasoningResponse) -> Self {
        ToolOutput::Reasoning(response)
    }

    pub fn file_create(response: CreateFileResponse) -> Self {
        ToolOutput::FileCreate(response)
    }

    pub fn edited_files(response: EditedFilesResponse) -> Self {
        ToolOutput::EditedFiles(response)
    }
    pub fn outline_nodes_using_editor(response: OutlineNodesUsingEditorResponse) -> Self {
        ToolOutput::OutlineNodesUsingEditor(response)
    }

    pub fn git_diff_response(response: GitDiffClientResponse) -> Self {
        ToolOutput::GitDiff(response)
    }

    pub fn search_and_replace_editing(response: SearchAndReplaceEditingResponse) -> Self {
        ToolOutput::SearchAndReplaceEditing(response)
    }

    pub fn should_edit_code(response: ShouldEditCodeSymbolResponse) -> Self {
        ToolOutput::ShouldEditCode(response)
    }

    pub fn code_symbol_new_location(response: CodeSymbolNewLocationResponse) -> Self {
        ToolOutput::CodeSymbolNewLocation(response)
    }

    pub fn inlay_hints(response: InlayHintsResponse) -> Self {
        ToolOutput::InlayHints(response)
    }

    pub fn filter_edit_operation(response: FilterEditOperationResponse) -> Self {
        ToolOutput::FilterEditOperation(response)
    }

    pub fn apply_outline_edits_to_range(response: ApplyOutlineEditsToRangeResponse) -> Self {
        ToolOutput::ApplyOutlineEditsToRange(response)
    }

    pub fn re_ranked_code_snippets_for_editing_context(
        response: ReRankingSnippetsForCodeEditingResponse,
    ) -> Self {
        ToolOutput::ReRankedCodeSnippetsForCodeEditing(response)
    }

    pub fn find_symbols_to_edit_in_context(output: FindSymbolsToEditInContextResponse) -> Self {
        ToolOutput::FindSymbolsToEditInContext(output)
    }

    pub fn find_file_for_new_symbol(output: FindFileForSymbolResponse) -> Self {
        ToolOutput::FindFileForNewSymbol(output)
    }

    pub fn lsp_symbol_search_information(output: LSPGrepSymbolInCodebaseResponse) -> Self {
        ToolOutput::LSPSymbolSearchInformation(output)
    }

    pub fn new_sub_symbol_creation(output: NewSubSymbolRequiredResponse) -> Self {
        ToolOutput::NewSubSymbolCreation(output)
    }

    pub fn planning_before_code_editing(output: PlanningBeforeCodeEditResponse) -> Self {
        ToolOutput::PlanningBeforeCodeEditing(output)
    }

    pub fn code_symbol_follow_for_initial_request(output: CodeSymbolFollowInitialResponse) -> Self {
        ToolOutput::CodeSymbolFollowForInitialRequest(output)
    }

    pub fn swe_bench_test_output(output: SWEBenchTestRepsonse) -> Self {
        ToolOutput::SWEBenchTestOutput(output)
    }

    pub fn probe_summarization_result(response: String) -> Self {
        ToolOutput::ProbeSummarizationResult(response)
    }

    pub fn probe_follow_along_symbol(response: ProbeNextSymbol) -> Self {
        ToolOutput::ProbeFollowAlongSymbol(response)
    }

    pub fn probe_sub_symbol(response: CodeToProbeFilterResponse) -> Self {
        ToolOutput::ProbeSubSymbol(response)
    }

    pub fn probe_possible(response: CodeSymbolShouldAskQuestionsResponse) -> Self {
        ToolOutput::ProbePossible(response)
    }

    pub fn go_to_reference(refernece: GoToReferencesResponse) -> Self {
        ToolOutput::GoToReference(refernece)
    }

    pub fn code_correctness_action(output: CodeCorrectnessAction) -> Self {
        ToolOutput::CodeCorrectnessAction(output)
    }

    pub fn quick_fix_invocation_result(output: LSPQuickFixInvocationResponse) -> Self {
        ToolOutput::LSPQuickFixInvoation(output)
    }

    pub fn quick_fix_list(output: GetQuickFixResponse) -> Self {
        ToolOutput::GetQuickFixList(output)
    }

    pub fn code_edit_output(output: String) -> Self {
        ToolOutput::CodeEditTool(output)
    }

    pub fn lsp_diagnostics(diagnostics: LSPDiagnosticsOutput) -> Self {
        ToolOutput::LSPDiagnostics(diagnostics)
    }

    pub fn code_snippets_to_edit(output: CodeToEditToolOutput) -> Self {
        ToolOutput::CodeToEdit(output)
    }

    pub fn rerank_entries(reranked_snippets: ReRankEntriesForBroker) -> Self {
        ToolOutput::ReRankSnippets(reranked_snippets)
    }

    pub fn important_symbols(important_symbols: CodeSymbolImportantResponse) -> Self {
        ToolOutput::ImportantSymbols(important_symbols)
    }

    pub fn utility_code_symbols(important_symbols: CodeSymbolImportantResponse) -> Self {
        ToolOutput::UtilityCodeSearch(important_symbols)
    }

    pub fn go_to_definition(go_to_definition: GoToDefinitionResponse) -> Self {
        ToolOutput::GoToDefinition(go_to_definition)
    }

    pub fn file_open(file_open: OpenFileResponse) -> Self {
        ToolOutput::FileOpen(file_open)
    }

    pub fn go_to_implementation(go_to_implementation: GoToImplementationResponse) -> Self {
        ToolOutput::GoToImplementation(go_to_implementation)
    }

    pub fn get_quick_fix_actions(self) -> Option<GetQuickFixResponse> {
        match self {
            ToolOutput::GetQuickFixList(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_lsp_diagnostics(self) -> Option<LSPDiagnosticsOutput> {
        match self {
            ToolOutput::LSPDiagnostics(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_editor_apply_response(self) -> Option<EditorApplyResponse> {
        match self {
            ToolOutput::EditorApplyChanges(output) => Some(output),
            _ => None,
        }
    }

    /// Grabs the output of filter edit operations from the ToolOutput
    pub fn get_filter_edit_operation_output(self) -> Option<FilterEditOperationResponse> {
        match self {
            ToolOutput::FilterEditOperation(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_code_edit_output(self) -> Option<String> {
        match self {
            ToolOutput::CodeEditTool(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_important_symbols(self) -> Option<CodeSymbolImportantResponse> {
        match self {
            ToolOutput::ImportantSymbols(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_file_open_response(self) -> Option<OpenFileResponse> {
        match self {
            ToolOutput::FileOpen(file_open) => Some(file_open),
            _ => None,
        }
    }

    pub fn grep_single_file(self) -> Option<FindInFileResponse> {
        match self {
            ToolOutput::GrepSingleFile(grep_single_file) => Some(grep_single_file),
            _ => None,
        }
    }

    pub fn get_go_to_definition(self) -> Option<GoToDefinitionResponse> {
        match self {
            ToolOutput::GoToDefinition(go_to_definition) => Some(go_to_definition),
            _ => None,
        }
    }

    pub fn get_go_to_implementation(self) -> Option<GoToImplementationResponse> {
        match self {
            ToolOutput::GoToImplementation(result) => Some(result),
            _ => None,
        }
    }

    pub fn code_to_edit_filter(self) -> Option<CodeToEditFilterResponse> {
        match self {
            ToolOutput::CodeToEditSnippets(code_to_edit_filter) => Some(code_to_edit_filter),
            _ => None,
        }
    }

    pub fn code_to_edit_in_symbol(self) -> Option<CodeToEditSymbolResponse> {
        match self {
            ToolOutput::CodeToEditSingleSymbolSnippets(response) => Some(response),
            _ => None,
        }
    }

    pub fn utility_code_search_response(self) -> Option<CodeSymbolImportantResponse> {
        match self {
            ToolOutput::UtilityCodeSearch(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_test_correction_output(self) -> Option<String> {
        match self {
            ToolOutput::TestCorrectionOutput(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_code_correctness_action(self) -> Option<CodeCorrectnessAction> {
        match self {
            ToolOutput::CodeCorrectnessAction(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_quick_fix_invocation_result(self) -> Option<LSPQuickFixInvocationResponse> {
        match self {
            ToolOutput::LSPQuickFixInvoation(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_references(self) -> Option<GoToReferencesResponse> {
        match self {
            ToolOutput::GoToReference(output) => Some(output),
            _ => None,
        }
    }

    pub fn code_editing_for_error_fix(self) -> Option<String> {
        match self {
            ToolOutput::CodeEditingForError(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_swe_bench_test_output(self) -> Option<SWEBenchTestRepsonse> {
        match self {
            ToolOutput::SWEBenchTestOutput(output) => Some(output),
            _ => None,
        }
    }

    pub fn class_symbols_to_followup(self) -> Option<ClassSymbolFollowupResponse> {
        match self {
            ToolOutput::ClassSymbolFollowupResponse(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_probe_summarize_result(self) -> Option<String> {
        match self {
            ToolOutput::ProbeSummarizationResult(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_probe_sub_symbol(self) -> Option<CodeToProbeFilterResponse> {
        match self {
            ToolOutput::ProbeSubSymbol(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_should_probe_symbol(self) -> Option<CodeSymbolShouldAskQuestionsResponse> {
        match self {
            ToolOutput::ProbePossible(request) => Some(request),
            _ => None,
        }
    }

    pub fn get_probe_symbol_deeper(self) -> Option<CodeSymbolToAskQuestionsResponse> {
        match self {
            ToolOutput::ProbeQuestion(request) => Some(request),
            _ => None,
        }
    }

    pub fn get_should_probe_next_symbol(self) -> Option<ProbeNextSymbol> {
        match self {
            ToolOutput::ProbeFollowAlongSymbol(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_code_symbol_follow_for_initial_request(
        self,
    ) -> Option<CodeSymbolFollowInitialResponse> {
        match self {
            ToolOutput::CodeSymbolFollowForInitialRequest(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_code_to_probe_sub_symbol_list(self) -> Option<CodeToProbeSubSymbolList> {
        match self {
            ToolOutput::ProbeSubSymbolFiltering(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_probe_enough_or_deeper(self) -> Option<ProbeEnoughOrDeeperResponse> {
        match self {
            ToolOutput::ProbeEnoughOrDeeper(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_probe_create_question_for_symbol(self) -> Option<String> {
        match self {
            ToolOutput::ProbeCreateQuestionForSymbol(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_plan_before_code_editing(self) -> Option<PlanningBeforeCodeEditResponse> {
        match self {
            ToolOutput::PlanningBeforeCodeEditing(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_new_sub_symbol_required(self) -> Option<NewSubSymbolRequiredResponse> {
        match self {
            ToolOutput::NewSubSymbolCreation(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_probe_try_harder_answer(self) -> Option<String> {
        match self {
            ToolOutput::ProbeTryHardAnswer(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_find_file_for_symbol_response(self) -> Option<FindFileForSymbolResponse> {
        match self {
            ToolOutput::FindFileForNewSymbol(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_lsp_grep_symbols_in_codebase_response(
        self,
    ) -> Option<LSPGrepSymbolInCodebaseResponse> {
        match self {
            ToolOutput::LSPSymbolSearchInformation(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_code_symbols_to_edit_in_context(self) -> Option<FindSymbolsToEditInContextResponse> {
        match self {
            ToolOutput::FindSymbolsToEditInContext(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_apply_edits_to_range_response(self) -> Option<ApplyOutlineEditsToRangeResponse> {
        match self {
            ToolOutput::ApplyOutlineEditsToRange(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_reranked_outline_nodes_for_code_editing(
        self,
    ) -> Option<ReRankingSnippetsForCodeEditingResponse> {
        match self {
            ToolOutput::ReRankedCodeSnippetsForCodeEditing(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_keyword_search_reply(self) -> Option<CodeSymbolImportantResponse> {
        match self {
            ToolOutput::KeywordSearch(reply) => Some(reply),
            _ => None,
        }
    }

    pub fn get_inlay_hints_response(self) -> Option<InlayHintsResponse> {
        match self {
            ToolOutput::InlayHints(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_code_symbol_new_location(self) -> Option<CodeSymbolNewLocationResponse> {
        match self {
            ToolOutput::CodeSymbolNewLocation(response) => Some(response),
            _ => None,
        }
    }

    pub fn should_edit_code_symbol_full(self) -> Option<ShouldEditCodeSymbolResponse> {
        match self {
            ToolOutput::ShouldEditCode(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_search_and_replace_output(self) -> Option<SearchAndReplaceEditingResponse> {
        match self {
            ToolOutput::SearchAndReplaceEditing(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_git_diff_output(self) -> Option<GitDiffClientResponse> {
        match self {
            ToolOutput::GitDiff(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_outline_nodes_from_editor(self) -> Option<OutlineNodesUsingEditorResponse> {
        match self {
            ToolOutput::OutlineNodesUsingEditor(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_relevant_references(self) -> Option<Vec<RelevantReference>> {
        match self {
            ToolOutput::ReferencesFilter(response) => Some(response),
            _ => None,
        }
    }

    pub fn recently_edited_files(self) -> Option<EditedFilesResponse> {
        match self {
            ToolOutput::EditedFiles(response) => Some(response),
            _ => None,
        }
    }

    pub fn reasoning_output(self) -> Option<ReasoningResponse> {
        match self {
            ToolOutput::Reasoning(response) => Some(response),
            _ => None,
        }
    }

    pub fn step_generator_output(self) -> Option<StepGeneratorResponse> {
        match self {
            ToolOutput::StepGenerator(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_file_create_response(self) -> Option<CreateFileResponse> {
        match self {
            ToolOutput::FileCreate(response) => Some(response),
            _ => None,
        }
    }

    pub fn file_diagnostics(output: FileDiagnosticsOutput) -> Self {
        ToolOutput::FileDiagnostics(output)
    }

    pub fn get_file_diagnostics(self) -> Option<FileDiagnosticsOutput> {
        match self {
            ToolOutput::FileDiagnostics(output) => Some(output),
            _ => None,
        }
    }

    pub fn get_plan_new_steps(self) -> Option<StepGeneratorResponse> {
        match self {
            ToolOutput::PlanAddStep(response) => Some(response),
            _ => None,
        }
    }

    pub fn go_to_previous_word_response(self) -> Option<GoToPreviousWordResponse> {
        match self {
            ToolOutput::GoToPreviousWord(response) => Some(response),
            _ => None,
        }
    }

    pub fn go_to_type_definition_response(self) -> Option<GoToDefinitionResponse> {
        match self {
            ToolOutput::GoToTypeDefinition(response) => Some(response),
            _ => None,
        }
    }

    pub fn new_exchange_response(self) -> Option<SessionExchangeNewResponse> {
        match self {
            ToolOutput::NewExchangeDuringSession(response) => Some(response),
            _ => None,
        }
    }

    pub fn context_drive_chat_reply(self) -> Option<SessionChatClientResponse> {
        match self {
            ToolOutput::ContextDriveChatReply(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_undo_changes_made_during_session(
        self,
    ) -> Option<UndoChangesMadeDuringExchangeRespnose> {
        match self {
            ToolOutput::UndoChangesMadeDuringSession(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_context_driven_hot_streak_reply(self) -> Option<SessionHotStreakResponse> {
        match self {
            ToolOutput::ContextDriveHotStreakReply(response) => Some(response),
            _ => None,
        }
    }

    pub fn terminal_command(self) -> Option<TerminalOutput> {
        match self {
            ToolOutput::TerminalCommand(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_search_file_content_with_regex(self) -> Option<SearchFileContentWithRegexOutput> {
        match self {
            ToolOutput::SearchFileContentWithRegex(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_list_files_directory(self) -> Option<ListFilesOutput> {
        match self {
            ToolOutput::ListFiles(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_test_runner(self) -> Option<TestRunnerResponse> {
        match self {
            ToolOutput::TestRunner(response) => Some(response),
            _ => None,
        }
    }

    pub fn repo_map_generator_response(self) -> Option<RepoMapGeneratorResponse> {
        match self {
            ToolOutput::RepoMapGeneration(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_pending_spawned_process_output(
        self,
    ) -> Option<SubProcessSpanwedPendingOutputResponse> {
        match self {
            ToolOutput::SubProcessSpawnedPendingOutput(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_reward_generation_response(self) -> Option<RewardGenerationResponse> {
        match self {
            ToolOutput::RewardGeneration(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_feedback_generation_response(self) -> Option<FeedbackGenerationResponse> {
        match self {
            ToolOutput::FeedbackGeneration(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_semantic_search_response(self) -> Option<SemanticSearchResponse> {
        match self {
            ToolOutput::SemanticSearch(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_find_files_response(self) -> Option<FindFilesResponse> {
        match self {
            ToolOutput::FindFiles(response) => Some(response),
            _ => None,
        }
    }

    pub fn get_request_screenshot_response(self) -> Option<RequestScreenshotOutput> {
        match self {
            ToolOutput::RequestScreenshot(response) => Some(response),
            _ => None,
        }
    }

    impl_output!(get_mcp_response, McpTool, McpToolResponse);
}
