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
    let system_message = r#"You are an expert software engineer who is tasked with finding the right location to place new code.
- We will be presented a list of sections of the code section where the number of the section is mentioned in <idx {number}>
- The user has asked you to find the location where we should add this new code, you have to reply in the following format:
<reply>
<thinking>
{your thoughts on how it should work}
</thinking>
<section>
{the number of the section where we should be adding the code, we will add the code at the top of that section}
</section>
</reply>
- You will first think for a bit, use 2 or less sentences to plan out where we should add the new code and then reply with the section number where we should add it, we are adding the code at the top of the section.
- The edge case when you want to add the code at the end of the file, just give back the last empty section number which is empty and we can add it to that"#;
    let user_message = r#"<user_instruction>
we want to also track the deployment name as AzureOpenAIDeploymentName
</user_instruction>

<file_outline>
<idx 0>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct AzureOpenAIDeploymentId {
    pub deployment_id: String,
}
</idx>
<idx 1>
#[derive(Default, Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct CodeStoryLLMTypes {
    // shoehorning the llm type here so we can provide the correct api keys
    pub llm_type: Option<LLMType>,
}
</idx>
<idx 2>
impl CodeStoryLLMTypes {
    pub fn new() -> Self {
}
</idx>
<idx 3>
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
</idx>
<idx 4>
impl std::fmt::Display for LLMProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
}
</idx>
<idx 5>
impl LLMProvider {
    pub fn is_codestory(&self) -> bool {

    pub fn is_anthropic_api_key(&self) -> bool {
}
</idx>
<idx 6>
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
</idx>
<idx 7>
impl LLMProviderAPIKeys {
    pub fn is_openai(&self) -> bool {

    pub fn provider_type(&self) -> LLMProvider {

    // Gets the relevant key from the llm provider
    pub fn key(&self, llm_provider: &LLMProvider) -> Option<Self> {
}
</idx>
<idx 8>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAIProvider {
    pub api_key: String,
}
</idx>
<idx 9>
impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 10>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TogetherAIProvider {
    pub api_key: String,
}
</idx>
<idx 11>
impl TogetherAIProvider {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 12>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroqProviderAPIKey {
    pub api_key: String,
}
</idx>
<idx 13>
impl GroqProviderAPIKey {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 14>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterAPIKey {
    pub api_key: String,
}
</idx>
<idx 15>
impl OpenRouterAPIKey {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 16>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GoogleAIStudioKey {
    pub api_key: String,
}
</idx>
<idx 17>
impl GoogleAIStudioKey {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 18>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GeminiProAPIKey {
    pub api_key: String,
    pub api_base: String,
}
</idx>
<idx 19>
impl GeminiProAPIKey {
    pub fn new(api_key: String, api_base: String) -> Self {
}
</idx>
<idx 20>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FireworksAPIKey {
    pub api_key: String,
}
</idx>
<idx 21>
impl FireworksAPIKey {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 22>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AnthropicAPIKey {
    pub api_key: String,
}
</idx>
<idx 23>
impl AnthropicAPIKey {
    pub fn new(api_key: String) -> Self {
}
</idx>
<idx 24>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAICompatibleConfig {
    pub api_key: String,
    pub api_base: String,
}
</idx>
<idx 25>
impl OpenAICompatibleConfig {
    pub fn new(api_key: String, api_base: String) -> Self {
}
</idx>
<idx 26>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OllamaProvider {}
</idx>
<idx 27>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AzureConfig {
    pub deployment_id: String,
    pub api_base: String,
    pub api_key: String,
    pub api_version: String,
}
</idx>
<idx 28>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LMStudioConfig {
    pub api_base: String,
}
</idx>
<idx 29>
impl LMStudioConfig {
    pub fn api_base(&self) -> &str {
}
</idx>
<idx 30>
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CodeStoryConfig {
    pub llm_type: LLMType,
}
</idx>
<idx 31>
    fn test_reading_from_string_for_provider() {
        let provider = LLMProvider::Azure(AzureOpenAIDeploymentId {
            deployment_id: "testing".to_owned(),
        });
        let string_provider = serde_json::to_string(&provider).expect("to work");
        assert_eq!(
            string_provider,
            "{\"Azure\":{\"deployment_id\":\"testing\"}}"
        );
        let provider = LLMProvider::Ollama;
        let string_provider = serde_json::to_string(&provider).expect("to work");
        assert_eq!(string_provider, "\"Ollama\"");
    }
</idx>
<idx 32>
    fn test_reading_provider_keys() {
        let provider_keys = LLMProviderAPIKeys::OpenAI(super::OpenAIProvider {
            api_key: "testing".to_owned(),
        });
        let string_provider_keys = serde_json::to_string(&provider_keys).expect("to work");
        assert_eq!(string_provider_keys, "",);
    }
</idx>
<idx 33>
    fn test_reading_from_string_for_provider_keys() {
        let provider_keys = LLMProviderAPIKeys::CodeStory;
        let string_provider_keys = serde_json::to_string(&provider_keys).expect("to work");
        assert_eq!(string_provider_keys, "\"CodeStory\"");
    }
</idx>
<idx 34>
</idx>
</file_outline>

Your reply should be in the following format:
<reply>
<thinking>
{{your thinking behind selecting the section where we want to add this code}}
</thinking>
<section>
{{the section id where we should be writing the code}}
</section>
</reply>"#;
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
    let _few_shot_user_instruction = r#"<code_in_selection>
```py
def add_values(a, b):
    return a + b

def subtract(a, b):
    return a - b
```
</code_in_selection>

<code_changes_outline>
def add_values(a, b, logger):
    logger.info(a, b)
    # rest of the code

def subtract(a, b, logger):
    logger.info(a, b)
    # rest of the code
</code_changes_outline>"#;
    let _few_shot_output = r#"<reply>
```py
def add_values(a, b, logger):
    logger.info(a, b)
    return a + b

def subtract(a, b, logger):
    logger.info(a, b)
    return a - b
```
</reply>"#;
    let llm_request = LLMClientCompletionRequest::new(
        fireworks_ai.llm().clone(),
        vec![
            LLMClientMessage::system(system_message.to_owned()),
            // LLMClientMessage::user(few_shot_user_instruction.to_owned()),
            // LLMClientMessage::assistant(few_shot_output.to_owned()),
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
