//! This contains the input context and how we want to execute action on top of it, we are able to
//! convert between different types of inputs.. something like that
//! or we can keep hardcoded actions somewhere.. we will figure it out as we go

use llm_client::{
    clients::types::LLMType,
    provider::{GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    agentic::{
        symbol::{identifier::LLMProperties, ui_event::UIEventWithID},
        tool::{
            code_symbol::{
                important::CodeSymbolImportantWideSearch, repo_map_search::RepoMapSearchQuery,
            },
            input::ToolInput,
            search::big_search::{BigSearchRequest, SearchType},
        },
    },
    user_context::types::UserContext,
};

use super::message_event::SymbolEventMessageProperties;

#[derive(Debug, Clone)]
pub struct SymbolEventRequestId {
    request_id: String,
    root_request_id: String,
}

impl SymbolEventRequestId {
    pub fn new(request_id: String, root_request_id: String) -> Self {
        Self {
            request_id,
            root_request_id,
        }
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn set_request_id(mut self, request_id: String) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Clone)]
pub struct SymbolInputEvent {
    context: UserContext,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    user_query: String,
    request_id: SymbolEventRequestId,
    // Here we have properties for swe bench which we are sending for testing
    swe_bench_test_endpoint: Option<String>,
    repo_map_fs_path: Option<String>,
    swe_bench_id: Option<String>,
    swe_bench_git_dname: Option<String>,
    swe_bench_code_editing: Option<LLMProperties>,
    swe_bench_gemini_api_keys: Option<LLMProperties>,
    swe_bench_long_context_editing: Option<LLMProperties>,
    full_symbol_edit: bool,
    root_directory: Option<String>,
    /// The properties for the llm which does fast and stable
    /// code symbol selection on an initial context, this can be used
    /// when we are not using full codebase context search
    fast_code_symbol_search_llm: Option<LLMProperties>,
    big_search: bool,
    ui_sender: UnboundedSender<UIEventWithID>,
}

impl SymbolInputEvent {
    pub fn new(
        context: UserContext,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        user_query: String,
        request_id: String,
        root_request_id: String,
        swe_bench_test_endpoint: Option<String>,
        repo_map_fs_path: Option<String>,
        swe_bench_id: Option<String>,
        swe_bench_git_dname: Option<String>,
        swe_bench_code_editing: Option<LLMProperties>,
        swe_bench_gemini_api_keys: Option<LLMProperties>,
        swe_bench_long_context_editing: Option<LLMProperties>,
        full_symbol_edit: bool,
        root_directory: Option<String>,
        fast_code_symbol_search_llm: Option<LLMProperties>,
        big_search: bool,
        ui_sender: UnboundedSender<UIEventWithID>,
    ) -> Self {
        Self {
            context,
            llm,
            provider,
            api_keys,
            request_id: SymbolEventRequestId::new(request_id, root_request_id),
            user_query,
            swe_bench_test_endpoint,
            repo_map_fs_path,
            swe_bench_id,
            swe_bench_git_dname,
            swe_bench_code_editing,
            swe_bench_gemini_api_keys,
            swe_bench_long_context_editing,
            full_symbol_edit,
            root_directory,
            fast_code_symbol_search_llm,
            big_search,
            ui_sender,
        }
    }

    pub fn root_request_id(&self) -> &str {
        &self.request_id.root_request_id
    }

    pub fn ui_sender(&self) -> UnboundedSender<UIEventWithID> {
        self.ui_sender.clone()
    }

    pub fn full_symbol_edit(&self) -> bool {
        self.full_symbol_edit
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn get_swe_bench_git_dname(&self) -> Option<String> {
        self.swe_bench_git_dname.clone()
    }

    pub fn get_swe_bench_test_endpoint(&self) -> Option<String> {
        self.swe_bench_test_endpoint.clone()
    }

    pub fn set_swe_bench_id(mut self, swe_bench_id: String) -> Self {
        self.swe_bench_id = Some(swe_bench_id);
        self
    }

    pub fn swe_bench_instance_id(&self) -> Option<String> {
        self.swe_bench_id.clone()
    }

    pub fn provided_context(&self) -> &UserContext {
        &self.context
    }

    pub fn has_repo_map(&self) -> bool {
        self.repo_map_fs_path.is_some()
    }

    pub fn get_fast_code_symbol_llm(&self) -> Option<LLMProperties> {
        self.fast_code_symbol_search_llm.clone()
    }

    pub fn get_swe_bench_code_editing(&self) -> Option<LLMProperties> {
        self.swe_bench_code_editing.clone()
    }

    pub fn get_swe_bench_gemini_llm_properties(&self) -> Option<LLMProperties> {
        self.swe_bench_gemini_api_keys.clone()
    }

    pub fn get_swe_bench_long_context_editing(&self) -> Option<LLMProperties> {
        self.swe_bench_long_context_editing.clone()
    }

    pub fn request_id(&self) -> &str {
        &self.request_id.request_id
    }

    pub fn big_search(&self) -> bool {
        self.big_search
    }

    pub fn set_user_query(mut self, user_query: String) -> Self {
        self.user_query = user_query;
        self
    }

    // here we can take an action based on the state we are in
    // on some states this might be wrong, I find it a bit easier to reason
    // altho fuck complexity we ball
    pub async fn tool_use_on_initial_invocation(
        self,
        recent_edits: String,
        lsp_diagnostics: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<ToolInput> {
        // if its anthropic we purposefully override the llm here to be a better
        // model (if they are using their own api-keys and even the codestory provider)
        let llm_properties_for_symbol_search =
            if let Some(llm_properties) = self.get_fast_code_symbol_llm() {
                llm_properties.clone()
            } else {
                LLMProperties::new(
                    self.llm.clone(),
                    self.provider.clone(),
                    self.api_keys.clone(),
                )
            };
        // TODO(skcd): Toggle the request here depending on if we have the repo map
        if self.has_repo_map() || self.root_directory.is_some() {
            let contents = if self.has_repo_map() {
                tokio::fs::read_to_string(
                    self.repo_map_fs_path
                        .clone()
                        .expect("has_repo_map to not break"),
                )
                .await
                .ok()
            } else {
                None
            };
            match contents {
                Some(contents) => Some(ToolInput::RepoMapSearch(RepoMapSearchQuery::new(
                    contents,
                    self.user_query.to_owned(),
                    LLMType::ClaudeSonnet,
                    LLMProvider::Anthropic,
                    self.api_keys.clone(),
                    None,
                    self.request_id.root_request_id().to_string(),
                ))),
                None => {
                    if let Some(root_directory) = self.root_directory.to_owned() {
                        if self.big_search() {
                            return Some(ToolInput::BigSearch(BigSearchRequest::new(
                                self.user_query.to_string(),
                                // override to the gemini pro flash model over here
                                // for big search
                                LLMType::GeminiProFlash,
                                LLMProvider::GoogleAIStudio,
                                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
                                    "".to_owned(),
                                )),
                                Some(root_directory),
                                self.request_id.root_request_id().to_string(),
                                SearchType::Both,
                                message_properties,
                            )));
                        }
                    }
                    let code_wide_search: CodeSymbolImportantWideSearch =
                        CodeSymbolImportantWideSearch::new(
                            self.context,
                            self.user_query.to_owned(),
                            llm_properties_for_symbol_search.llm().clone(),
                            llm_properties_for_symbol_search.provider().clone(),
                            llm_properties_for_symbol_search.api_key().clone(),
                            self.request_id.root_request_id().to_string(),
                            "".to_owned(),
                            recent_edits,
                            lsp_diagnostics,
                            message_properties,
                        );
                    // just symbol search instead for quick access
                    return Some(ToolInput::RequestImportantSymbolsCodeWide(code_wide_search));
                }
            }
        } else {
            //TODO(codestory+cache): we should cache this part of the call as well
            let code_wide_search: CodeSymbolImportantWideSearch =
                CodeSymbolImportantWideSearch::new(
                    self.context,
                    self.user_query.to_owned(),
                    llm_properties_for_symbol_search.llm().clone(),
                    llm_properties_for_symbol_search.provider().clone(),
                    llm_properties_for_symbol_search.api_key().clone(),
                    self.request_id.root_request_id().to_string(),
                    "".to_owned(),
                    recent_edits,
                    lsp_diagnostics,
                    message_properties,
                );
            // Now we try to generate the tool input for this
            Some(ToolInput::RequestImportantSymbolsCodeWide(code_wide_search))
        }
    }
}
