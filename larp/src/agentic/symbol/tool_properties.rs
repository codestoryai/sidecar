//! This contains the configuration for the tools which can be used by the agent

use super::identifier::LLMProperties;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolProperties {
    swe_bench_test_endpoint: Option<String>,
    swe_bench_code_editing_llm: Option<LLMProperties>,
    swe_bench_reranking_llm: Option<LLMProperties>,
    swe_bench_long_context_editing_llm: Option<LLMProperties>,
    full_symbol_request: bool,
    fast_code_symbol_search: Option<LLMProperties>,
    // plan for the task instance this contains the overall plan we are going to
    // be following while making the edits
    plan_for_input: Option<String>,
    apply_edits_directly: bool,
}

impl ToolProperties {
    pub fn new() -> Self {
        Self {
            swe_bench_test_endpoint: None,
            swe_bench_code_editing_llm: None,
            swe_bench_reranking_llm: None,
            swe_bench_long_context_editing_llm: None,
            full_symbol_request: false,
            fast_code_symbol_search: None,
            plan_for_input: None,
            apply_edits_directly: false,
        }
    }

    pub fn should_apply_edits_directly(&self) -> bool {
        self.apply_edits_directly
    }

    pub fn set_apply_edits_directly(mut self) -> Self {
        self.apply_edits_directly = true;
        self
    }

    pub fn get_plan_for_input(&self) -> Option<String> {
        self.plan_for_input.clone()
    }

    /// Sets the plan for the input in the tool properties so we can refer to it
    /// while doing code correction or code editing if required
    pub fn set_plan_for_input(mut self, plan_for_input: Option<String>) -> Self {
        self.plan_for_input = plan_for_input;
        self
    }

    pub fn set_fast_code_symbol_search(
        mut self,
        fast_code_symbol_search_llm: Option<LLMProperties>,
    ) -> Self {
        self.fast_code_symbol_search = fast_code_symbol_search_llm;
        self
    }

    pub fn fast_code_symbol_search(&self) -> Option<LLMProperties> {
        self.fast_code_symbol_search.clone()
    }

    pub fn get_full_symbol_request(&self) -> bool {
        self.full_symbol_request
    }

    pub fn set_full_symbol_request(mut self, full_symbol_edit: bool) -> Self {
        self.full_symbol_request = full_symbol_edit;
        self
    }

    pub fn set_long_context_editing_llm(
        mut self,
        swe_bench_long_context_editing_llm: Option<LLMProperties>,
    ) -> Self {
        self.swe_bench_long_context_editing_llm = swe_bench_long_context_editing_llm;
        self
    }

    pub fn get_long_context_editing_llm(&self) -> Option<LLMProperties> {
        self.swe_bench_long_context_editing_llm.clone()
    }

    pub fn set_swe_bench_reranking_llm(
        mut self,
        swe_bench_reranking_llm: Option<LLMProperties>,
    ) -> Self {
        self.swe_bench_reranking_llm = swe_bench_reranking_llm;
        self
    }

    pub fn get_swe_bench_reranking_llm(&self) -> Option<LLMProperties> {
        self.swe_bench_reranking_llm.clone()
    }

    pub fn set_swe_bench_code_editing_llm(
        mut self,
        swe_bench_code_editing_llm: Option<LLMProperties>,
    ) -> Self {
        self.swe_bench_code_editing_llm = swe_bench_code_editing_llm;
        self
    }

    pub fn set_swe_bench_endpoint(mut self, swe_bench_test_endpoint: Option<String>) -> Self {
        self.swe_bench_test_endpoint = swe_bench_test_endpoint;
        self
    }

    pub fn get_swe_bench_test_endpoint(&self) -> Option<String> {
        self.swe_bench_test_endpoint.clone()
    }

    pub fn get_swe_bench_code_editing_llm(&self) -> Option<LLMProperties> {
        self.swe_bench_code_editing_llm.clone()
    }
}
