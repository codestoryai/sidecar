use llm_client::{
    clients::{
        fireworks::FireworksAIClient,
        types::{LLMClient, LLMClientCompletionRequest, LLMClientMessage, LLMType},
    },
    provider::{FireworksAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use sidecar::agentic::symbol::identifier::LLMProperties;

#[tokio::main]
async fn main() {
    let system_message = r#"You are an expert software eningeer who never writes incorrect code and is tasked with selecting code symbols whose definitions you can use for editing.
The editor has stopped working for you, so we get no help with auto-complete when writing code, hence we want to make sure that we select all the code symbols which are necessary.
As a first step before making changes, you are tasked with collecting all the definitions of the various code symbols whose methods or parameters you will be using when editing the code in the selection.
- You will be given the original user query in <user_query>
- You will be provided the code snippet you will be editing in <code_snippet_to_edit> section.
- The various definitions of the class, method or function (just the high level outline of it) will be given to you as a list in <code_symbol_outline_list>. When writing code you will reuse the methods from here to make the edits, so be very careful when selecting the symbol outlines you are interested in.
- Pay attention to the <code_snippet_to_edit> section and select code symbols accordingly, do not select symbols which we will not be using for making edits.
- Each code_symbol_outline entry is in the following format:
```
<code_symbol>
<name>
{name of the code symbol over here}
</name>
<content>
{the outline content for the code symbol over here}
</content>
</code_symbol>
```
- You have to decide which code symbols you will be using when doing the edits and select those code symbols.
Your reply should be in the following format:
<reply>
<thinking>
</thinking>
<code_symbol_outline_list>
<code_symbol>
<name>
</name>
<file_path>
</file_path>
</code_symbol>
... more code_symbol sections over here as per your requirement
</code_symbol_outline_list>
<reply>"#;
    let user_message = r#"<user_query>
Implement a new function to handle the code editing stop request, similar to probe_request_stop
</user_query>

<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>



<code_snippet_to_edit>

</code_snippet_to_edit>

<code_symbol_outline_list>
<code_symbol>
<name>
LLMProvider
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub enum LLMProvider {
    OpenAI,
    TogetherAI,
    Ollama,
    LMStudio,
    CodeStory(CodeStoryLLMTypes),
    Azure(AzureOpenAIDeploymentId),
    OpenAICompatible,
    Anthropic,
    FireworksAI,
    GeminiPro,
    GoogleAIStudio,
    OpenRouter,
    Groq,
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProvider
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl std::fmt::Display for LLMProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProvider
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl LLMProvider {
    pub fn is_codestory(&self) -> bool {
    pub fn is_anthropic_api_key(&self) -> bool {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProviderAPIKeys
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum LLMProviderAPIKeys {
    OpenAI(OpenAIProvider),
    TogetherAI(TogetherAIProvider),
    Ollama(OllamaProvider),
    OpenAIAzureConfig(AzureConfig),
    LMStudio(LMStudioConfig),
    OpenAICompatible(OpenAICompatibleConfig),
    CodeStory,
    Anthropic(AnthropicAPIKey),
    FireworksAI(FireworksAPIKey),
    GeminiPro(GeminiProAPIKey),
    GoogleAIStudio(GoogleAIStudioKey),
    OpenRouter(OpenRouterAPIKey),
    GroqProvider(GroqProviderAPIKey),
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProviderAPIKeys
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl LLMProviderAPIKeys {
    // Gets the relevant key from the llm provider
    pub fn is_openai(&self) -> bool {
    pub fn provider_type(&self) -> LLMProvider {
    pub fn key(&self, llm_provider: &LLMProvider) -> Option<Self> {
}
</content>
</code_symbol>
<code_symbol>
<name>
OpenAIProvider
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAIProvider {
    pub api_key: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
OpenAIProvider
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
GoogleAIStudioKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GoogleAIStudioKey {
    pub api_key: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
GoogleAIStudioKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl GoogleAIStudioKey {
    pub fn new(api_key: String) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
FireworksAPIKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FireworksAPIKey {
    pub api_key: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
FireworksAPIKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl FireworksAPIKey {
    pub fn new(api_key: String) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
AnthropicAPIKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AnthropicAPIKey {
    pub api_key: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
AnthropicAPIKey
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/provider.rs
impl AnthropicAPIKey {
    pub fn new(api_key: String) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
Result
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/types.rs
pub type Result<T, E = Error> = std::result::Result<T, E>;
</content>
</code_symbol>
<code_symbol>
<name>
json
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/types.rs
pub(crate) fn json<'a, T>(val: T) -> Json<Response<'a>>
where
    Response<'a>: From<T>,
{
</content>
</code_symbol>
<code_symbol>
<name>
ToolBrokerConfiguration
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
pub struct ToolBrokerConfiguration {
    editor_agent: Option<LLMProperties>,
    apply_edits_directly: bool,
}
</content>
</code_symbol>
<code_symbol>
<name>
ToolBrokerConfiguration
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
impl ToolBrokerConfiguration {
    pub fn new(editor_agent: Option<LLMProperties>, apply_edits_directly: bool) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
ToolBroker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
pub struct ToolBroker {
    tools: HashMap<ToolType, Box<dyn Tool + Send + Sync>>,
}
</content>
</code_symbol>
<code_symbol>
<name>
ToolBroker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
impl ToolBroker {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        code_edit_broker: Arc<CodeEditBroker>,
        symbol_tracking: Arc<SymbolTrackerInline>,
        language_broker: Arc<TSLanguageParsing>,
        tool_broker_config: ToolBrokerConfiguration,
        // Use this if the llm we were talking to times out or does not produce
        // outout which is coherent
        // we should have finer control over the fail-over llm but for now
        // a global setting like this is fine
        fail_over_llm: LLMProperties,
    ) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
ToolBroker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/broker.rs
#[async_trait]
impl Tool for ToolBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolInputEvent
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/events/input.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/events/input.rs
#[derive(Clone, Debug, serde::Serialize)]
pub struct SymbolInputEvent {
    context: UserContext,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    user_query: String,
    request_id: String,
    // Here we have properties for swe bench which we are sending for testing
    swe_bench_test_endpoint: Option<String>,
    repo_map_fs_path: Option<String>,
    gcloud_access_token: Option<String>,
    swe_bench_id: Option<String>,
    swe_bench_git_dname: Option<String>,
    swe_bench_code_editing: Option<LLMProperties>,
    swe_bench_gemini_api_keys: Option<LLMProperties>,
    swe_bench_long_context_editing: Option<LLMProperties>,
    full_symbol_edit: bool,
    codebase_search: bool,
    root_directory: Option<String>,
    /// The properties for the llm which does fast and stable
    /// code symbol selection on an initial context, this can be used
    /// when we are not using full codebase context search
    fast_code_symbol_search_llm: Option<LLMProperties>,
    file_important_search: bool, // todo: this currently conflicts with repomap search
    big_search: bool,
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolInputEvent
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/events/input.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/events/input.rs
impl SymbolInputEvent {
    // here we can take an action based on the state we are in
    // on some states this might be wrong, I find it a bit easier to reason
    // altho fuck complexity we ball
    pub fn new(
        context: UserContext,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        user_query: String,
        request_id: String,
        swe_bench_test_endpoint: Option<String>,
        repo_map_fs_path: Option<String>,
        gcloud_access_token: Option<String>,
        swe_bench_id: Option<String>,
        swe_bench_git_dname: Option<String>,
        swe_bench_code_editing: Option<LLMProperties>,
        swe_bench_gemini_api_keys: Option<LLMProperties>,
        swe_bench_long_context_editing: Option<LLMProperties>,
        full_symbol_edit: bool,
        codebase_search: bool,
        root_directory: Option<String>,
        fast_code_symbol_search_llm: Option<LLMProperties>,
        file_important_search: bool,
        big_search: bool,
    ) -> Self {
    pub fn full_symbol_edit(&self) -> bool {
    pub fn user_query(&self) -> &str {
    pub fn get_swe_bench_git_dname(&self) -> Option<String> {
    pub fn get_swe_bench_test_endpoint(&self) -> Option<String> {
    pub fn set_swe_bench_id(mut self, swe_bench_id: String) -> Self {
    pub fn swe_bench_instance_id(&self) -> Option<String> {
    pub fn provided_context(&self) -> &UserContext {
    pub fn has_repo_map(&self) -> bool {
    pub fn get_fast_code_symbol_llm(&self) -> Option<LLMProperties> {
    pub fn get_swe_bench_code_editing(&self) -> Option<LLMProperties> {
    pub fn get_swe_bench_gemini_llm_properties(&self) -> Option<LLMProperties> {
    pub fn get_swe_bench_long_context_editing(&self) -> Option<LLMProperties> {
    pub fn request_id(&self) -> &str {
    pub fn big_search(&self) -> bool {
    pub async fn tool_use_on_initial_invocation(
        self,
        tool_box: Arc<ToolBox>,
        _request_id: &str,
    ) -> Option<ToolInput> {
}
</content>
</code_symbol>
<code_symbol>
<name>
CodeEditBroker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/code_edit/models/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/code_edit/models/broker.rs
pub struct CodeEditBroker {
    models: HashMap<LLMType, Box<dyn CodeEditPromptFormatters + Send + Sync>>,
}
</content>
</code_symbol>
<code_symbol>
<name>
CodeEditBroker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/code_edit/models/broker.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/tool/code_edit/models/broker.rs
impl CodeEditBroker {
    pub fn new() -> Self {
    pub fn format_prompt(
        &self,
        context: &CodeEdit,
    ) -> Result<LLMClientCompletionRequest, ToolError> {
    pub fn find_code_section_to_edit(
        &self,
        context: &CodeSnippetForEditing,
    ) -> Result<LLMClientCompletionRequest, ToolError> {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMClientConfig
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/model_selection.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/model_selection.rs
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LLMClientConfig {
    pub slow_model: LLMType,
    pub fast_model: LLMType,
    pub models: HashMap<LLMType, Model>,
    pub providers: Vec<LLMProviderAPIKeys>,
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMClientConfig
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/model_selection.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/model_selection.rs
impl LLMClientConfig {
    pub fn provider_for_slow_model(&self) -> Option<&LLMProviderAPIKeys> {
    pub fn provider_for_fast_model(&self) -> Option<&LLMProviderAPIKeys> {
    pub fn fast_model_temperature(&self) -> Option<f32> {
    pub fn provider_config_for_fast_model(&self) -> Option<&LLMProvider> {
    fn nullify_azure_config(&self) -> AzureOpenAIDeploymentId {
    pub fn logging_config(&self) -> LLMClientLoggingConfig {
}
</content>
</code_symbol>
<code_symbol>
<name>
UserContext
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agent.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agent.rs
impl UserContext {
    fn merge_from_previous(mut self, previous: Option<&UserContext>) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolManager
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/manager.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/manager.rs
pub struct SymbolManager {
    _sender: UnboundedSender<(
        SymbolEventRequest,
        String,
        tokio::sync::oneshot::Sender<SymbolEventResponse>,
    )>,
    // this is the channel where the various symbols will use to talk to the manager
    // which in turn will proxy it to the right symbol, what happens if there are failures
    // each symbol has its own receiver which is being used
    symbol_locker: SymbolLocker,
    tools: Arc<ToolBroker>,
    _symbol_broker: Arc<SymbolTrackerInline>,
    _editor_parsing: Arc<EditorParsing>,
    ts_parsing: Arc<TSLanguageParsing>,
    tool_box: Arc<ToolBox>,
    _editor_url: String,
    llm_properties: LLMProperties,
    ui_sender: UnboundedSender<UIEventWithID>,
    long_context_cache: LongContextSearchCache,
    root_request_id: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolManager
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/manager.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/manager.rs
impl SymbolManager {
    // This is just for testing out the flow for single input events
    // once we have the initial request, which we will go through the initial request
    // mode once, we have the symbols from it we can use them to spin up sub-symbols as well
    pub fn new(
        tools: Arc<ToolBroker>,
        symbol_broker: Arc<SymbolTrackerInline>,
        editor_parsing: Arc<EditorParsing>,
        editor_url: String,
        ui_sender: UnboundedSender<UIEventWithID>,
        llm_properties: LLMProperties,
        // This is a hack and not a proper one at that, we obviously want to
        // do better over here
        user_context: UserContext,
        request_id: String,
    ) -> Self {
    pub async fn probe_request_from_user_context(
        &self,
        query: String,
        user_context: UserContext,
    ) -> Result<(), SymbolError> {
    pub async fn probe_request(&self, input_event: SymbolEventRequest) -> Result<(), SymbolError> {
    pub async fn initial_request(&self, input_event: SymbolInputEvent) -> Result<(), SymbolError> {
}
</content>
</code_symbol>
<code_symbol>
<name>
UserContext
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/user_context/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/user_context/types.rs
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct UserContext {
    pub variables: Vec<VariableInformation>,
    pub file_content_map: Vec<FileContentValue>,
    pub terminal_selection: Option<String>,
    // These paths will be absolute and need to be used to get the
    // context of the folders here, we will output it properly
    folder_paths: Vec<String>,
}
</content>
</code_symbol>
<code_symbol>
<name>
UserContext
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/user_context/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/user_context/types.rs
impl UserContext {
    // generats the full xml for the input context so the llm can query from it
    pub fn new(
        variables: Vec<VariableInformation>,
        file_content_map: Vec<FileContentValue>,
        terminal_selection: Option<String>,
        folder_paths: Vec<String>,
    ) -> Self {
    pub fn update_file_content_map(
        mut self,
        file_path: String,
        file_content: String,
        language: String,
    ) -> Self {
    pub fn folder_paths(&self) -> Vec<String> {
    pub fn is_empty(&self) -> bool {
    pub async fn to_xml(
        self,
        file_extension_filters: HashSet<String>,
    ) -> Result<String, UserContextError> {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum LLMType {
    Mixtral,
    MistralInstruct,
    Gpt4,
    GPT3_5_16k,
    Gpt4_32k,
    Gpt4O,
    Gpt4OMini,
    Gpt4Turbo,
    DeepSeekCoder1_3BInstruct,
    DeepSeekCoder33BInstruct,
    DeepSeekCoder6BInstruct,
    DeepSeekCoderV2,
    CodeLLama70BInstruct,
    CodeLlama13BInstruct,
    CodeLlama7BInstruct,
    Llama3_8bInstruct,
    Llama3_1_8bInstruct,
    Llama3_1_70bInstruct,
    ClaudeOpus,
    ClaudeSonnet,
    ClaudeHaiku,
    PPLXSonnetSmall,
    CohereRerankV3,
    GeminiPro,
    GeminiProFlash,
    Custom(String),
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
impl Serialize for LLMType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
impl<'de> Deserialize<'de> for LLMType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            fn visit_str<E>(self, value: &str) -> Result<LLMType, E>
            where
                E: de::Error,
            {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
impl LLMType {
    pub fn is_openai(&self) -> bool {
    pub fn is_custom(&self) -> bool {
    pub fn is_anthropic(&self) -> bool {
    pub fn is_openai_gpt4o(&self) -> bool {
    pub fn is_gemini_model(&self) -> bool {
    pub fn is_gemini_pro(&self) -> bool {
    pub fn is_togetherai_model(&self) -> bool {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/types.rs
impl fmt::Display for LLMType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
}
</content>
</code_symbol>
<code_symbol>
<name>
Application
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/application/application.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/application/application.rs
#[derive(Clone)]
pub struct Application {
    // Arc here because its shared by many things and is the consistent state
    // for the application
    pub config: Arc<Configuration>,
    pub repo_pool: RepositoryPool,
    pub indexes: Arc<Indexes>,
    pub semantic_client: Option<SemanticClient>,
    /// Background & maintenance tasks are executed on a separate
    /// executor
    pub sync_queue: SyncQueue,
    /// We also want to keep the language parsing functionality here
    pub language_parsing: Arc<TSLanguageParsing>,
    pub sql: SqlDb,
    pub posthog_client: Arc<PosthogClient>,
    pub user_id: String,
    pub llm_broker: Arc<LLMBroker>,
    pub inline_prompt_edit: Arc<InLineEditPromptBroker>,
    pub llm_tokenizer: Arc<LLMTokenizer>,
    pub chat_broker: Arc<LLMChatModelBroker>,
    pub fill_in_middle_broker: Arc<FillInMiddleBroker>,
    pub reranker: Arc<ReRankBroker>,
    pub answer_models: Arc<LLMAnswerModelBroker>,
    pub editor_parsing: Arc<EditorParsing>,
    pub fill_in_middle_state: Arc<FillInMiddleState>,
    pub symbol_tracker: Arc<SymbolTrackerInline>,
    pub probe_request_tracker: Arc<ProbeRequestTracker>,
}
</content>
</code_symbol>
<code_symbol>
<name>
Application
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/application/application.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/application/application.rs
impl Application {
    pub async fn initialize(mut config: Configuration) -> anyhow::Result<Self> {
    pub fn install_logging(config: &Configuration) {
    pub fn write_index(&self) -> BoundSyncQueue {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMType
</name>
<file_path>
/Users/skcd/test_repo/sidecar/llm_client/src/clients/ollama.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/llm_client/src/clients/ollama.rs
impl LLMType {
    pub fn to_ollama_model(&self) -> Result<String, LLMClientError> {
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProperties
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/identifier.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/identifier.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMProperties {
    llm: LLMType,
    provider: LLMProvider,
    api_key: LLMProviderAPIKeys,
}
</content>
</code_symbol>
<code_symbol>
<name>
LLMProperties
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/identifier.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/identifier.rs
impl LLMProperties {
    pub fn new(llm: LLMType, provider: LLMProvider, api_keys: LLMProviderAPIKeys) -> Self {
    pub fn llm(&self) -> &LLMType {
    pub fn provider(&self) -> &LLMProvider {
    pub fn api_key(&self) -> &LLMProviderAPIKeys {
    pub fn upgrade_llm_to_gemini_pro(mut self) -> Self {
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeRequestTracker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone)]
pub struct ProbeRequestTracker {
    pub running_requests: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeRequestTracker
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
impl ProbeRequestTracker {
    pub fn new() -> Self {
    async fn track_new_request(&self, request_id: &str, join_handle: JoinHandle<()>) {
    async fn cancel_request(&self, request_id: &str) {
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeRequestActiveWindow
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeRequestActiveWindow {
    file_path: String,
    file_content: String,
    language: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeRequest
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeRequest {
    request_id: String,
    editor_url: String,
    model_config: LLMClientConfig,
    user_context: UserContext,
    query: String,
    active_window_data: Option<ProbeRequestActiveWindow>,
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeStopRequest
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeStopRequest {
    request_id: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
ProbeStopResponse
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeStopResponse {
    done: bool,
}
</content>
</code_symbol>
<code_symbol>
<name>
AgenticCodeEditingStopRequest
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticCodeEditingStopRequest {
    request_id: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
AgenticCodeEditingStopResponse
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticCodeEditingStopResponse {
    done: bool,
}
</content>
</code_symbol>
<code_symbol>
<name>
probe_request_stop
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
pub async fn probe_request_stop(
    Extension(app): Extension<Application>,
    Json(ProbeStopRequest { request_id }): Json<ProbeStopRequest>,
) -> Result<impl IntoResponse> {
</content>
</code_symbol>
<code_symbol>
<name>
probe_request
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
pub async fn probe_request(
    Extension(app): Extension<Application>,
    Json(ProbeRequest {
        request_id,
        editor_url,
        model_config,
        mut user_context,
        query,
        active_window_data,
    }): Json<ProbeRequest>,
) -> Result<impl IntoResponse> {
</content>
</code_symbol>
<code_symbol>
<name>
SWEBenchRequest
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SWEBenchRequest {
    git_dname: String,
    problem_statement: String,
    editor_url: String,
    test_endpoint: String,
    // This is the file path with the repo map present in it
    repo_map_file: Option<String>,
    gcloud_access_token: String,
    swe_bench_id: String,
}
</content>
</code_symbol>
<code_symbol>
<name>
swe_bench
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
pub async fn swe_bench(
    axumQuery(SWEBenchRequest {
        git_dname,
        problem_statement,
        editor_url,
        test_endpoint,
        repo_map_file,
        gcloud_access_token,
        swe_bench_id,
    }): axumQuery<SWEBenchRequest>,
    Extension(app): Extension<Application>,
) -> Result<impl IntoResponse> {
</content>
</code_symbol>
<code_symbol>
<name>
AgenticCodeEditing
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticCodeEditing {
    user_query: String,
    editor_url: String,
    request_id: String,
    user_context: UserContext,
    active_window_data: Option<ProbeRequestActiveWindow>,
    root_directory: String,
    codebase_search: bool,
}
</content>
</code_symbol>
<code_symbol>
<name>
code_editing
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
<content>
FILEPATH: /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
pub async fn code_editing(
    Extension(app): Extension<Application>,
    Json(AgenticCodeEditing {
        user_query,
        editor_url,
        request_id,
        mut user_context,
        active_window_data,
        root_directory,
        codebase_search,
    }): Json<AgenticCodeEditing>,
) -> Result<impl IntoResponse> {
</content>
</code_symbol>
</code_symbol_outline_list>"#;
    // let gemini_llm_prperties = LLMProperties::new(
    //     LLMType::GeminiPro,
    //     LLMProvider::GoogleAIStudio,
    //     LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
    //         "".to_owned(),
    //     )),
    // );
    let fireworks_ai = LLMProperties::new(
        LLMType::Llama3_1_8bInstruct,
        LLMProvider::FireworksAI,
        LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
            "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
        )),
    );
    let few_shot_user_instruction = r#"<user_query>
We want to implement a new method on symbol event which exposes the initial request question
</user_query>
<code_snippet_to_edit>
```rust
#[derive(Debug, Clone, serde::Serialize)]
pub enum SymbolEvent {
    InitialRequest(InitialRequestData),
    AskQuestion(AskQuestionRequest),
    UserFeedback,
    Delete,
    Edit(SymbolToEditRequest),
    Outline,
    // Probe
    Probe(SymbolToProbeRequest),
}
```
</code_snippet_to_edit>
<code_symbol_outline_list>
<code_symbol>
<name>
InitialRequestData
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/initial_request.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct InitialRequestData {
    original_question: String,
    plan_if_available: Option<String>,
    history: Vec<SymbolRequestHistoryItem>,
    /// We operate on the full symbol instead of the
    full_symbol_request: bool,
}

impl InitialRequestData {
    pub fn new(
        original_question: String,
        plan_if_available: Option<String>,
        history: Vec<SymbolRequestHistoryItem>,
        full_symbol_request: bool,
    ) -> Self
    
    pub fn full_symbol_request(&self) -> bool

    pub fn get_original_question(&self) -> &str

    pub fn get_plan(&self) -> Option<String>

    pub fn history(&self) -> &[SymbolRequestHistoryItem]
}
</content>
</code_symbol>
<code_symbol>
<name>
AskQuestionRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/edit.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskQuestionRequest {
    question: String,
}

impl AskQuestionRequest {
    pub fn new(question: String) -> Self

    pub fn get_question(&self) -> &str
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolToEditRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/edit.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToEditRequest {
    symbols: Vec<SymbolToEdit>,
    symbol_identifier: SymbolIdentifier,
    history: Vec<SymbolRequestHistoryItem>,
}

impl SymbolToEditRequest {
    pub fn new(
        symbols: Vec<SymbolToEdit>,
        identifier: SymbolIdentifier,
        history: Vec<SymbolRequestHistoryItem>,
    ) -> Self

    pub fn symbols(self) -> Vec<SymbolToEdit>

    pub fn symbol_identifier(&self) -> &SymbolIdentifier

    pub fn history(&self) -> &[SymbolRequestHistoryItem]
}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolToProbeRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/probe.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToProbeRequest {
    symbol_identifier: SymbolIdentifier,
    probe_request: String,
    original_request: String,
    original_request_id: String,
    history: Vec<SymbolToProbeHistory>,
}

impl SymbolToProbeRequest {
    pub fn new(
        symbol_identifier: SymbolIdentifier,
        probe_request: String,
        original_request: String,
        original_request_id: String,
        history: Vec<SymbolToProbeHistory>,
    ) -> Self

    pub fn symbol_identifier(&self) -> &SymbolIdentifier

    pub fn original_request_id(&self) -> &str

    pub fn original_request(&self) -> &str

    pub fn probe_request(&self) -> &str

    pub fn history_slice(&self) -> &[SymbolToProbeHistory]

    pub fn history(&self) -> String
}
</content>
</code_symbol>
</code_symbol_outline_list>"#;
    let few_shot_output = r#"<reply>
<thinking>
The request talks about implementing new methods for the initial request data, so we need to include the initial request data symbol in the context when trying to edit the code.
</thinking>
<code_symbol_outline_list>
<code_symbol>
<name>
InitialRequestData
</name>
<file_path>
/Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/initial_request.rs
</file_path>
</code_symbol>
</code_symbol_outline_list>
</reply>"#;
    let llm_request = LLMClientCompletionRequest::new(
        fireworks_ai.llm().clone(),
        vec![
            LLMClientMessage::system(system_message.to_owned()),
            LLMClientMessage::user(few_shot_user_instruction.to_owned()),
            LLMClientMessage::assistant(few_shot_output.to_owned()),
            LLMClientMessage::user(user_message.to_owned()),
        ],
        0.0,
        None,
    );
    // let client = GoogleAIStdioClient::new();
    let client = FireworksAIClient::new();
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let start_instant = std::time::Instant::now();
    let response = client
        .stream_completion(fireworks_ai.api_key().clone(), llm_request, sender)
        .await;
    println!(
        "response {}:\n{:?}",
        start_instant.elapsed().as_millis(),
        response.expect("to work always")
    );
}
