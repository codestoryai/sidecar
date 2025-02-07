//! Contains types for setting the provider for the LLM, we are going to support
//! 3 things for now:
//! - CodeStory
//! - OpenAI
//! - Ollama
//! - Azure
//! - together.ai

use crate::clients::types::LLMType;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct AzureOpenAIDeploymentId {
    pub deployment_id: String,
}

#[derive(Default, Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct CodeStoryLLMTypes {
    // shoehorning the llm type here so we can provide the correct api keys
    pub llm_type: Option<LLMType>,
}

impl CodeStoryLLMTypes {
    pub fn new() -> Self {
        Self { llm_type: None }
    }
}

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
    Deepseek,
}

impl std::fmt::Display for LLMProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMProvider::OpenAI => write!(f, "OpenAI"),
            LLMProvider::TogetherAI => write!(f, "TogetherAI"),
            LLMProvider::Ollama => write!(f, "Ollama"),
            LLMProvider::LMStudio => write!(f, "LMStudio"),
            LLMProvider::CodeStory(_) => write!(f, "CodeStory"),
            LLMProvider::Azure(_) => write!(f, "Azure"),
            LLMProvider::OpenAICompatible => write!(f, "OpenAICompatible"),
            LLMProvider::Anthropic => write!(f, "Anthropic"),
            LLMProvider::FireworksAI => write!(f, "FireworksAI"),
            LLMProvider::GeminiPro => write!(f, "GeminiPro"),
            LLMProvider::GoogleAIStudio => write!(f, "GoogleAIStudio"),
            LLMProvider::OpenRouter => write!(f, "OpenRouter"),
            LLMProvider::Groq => write!(f, "Groq"),
            LLMProvider::Deepseek => write!(f, "Deepseek"),
        }
    }
}

impl LLMProvider {
    pub fn is_codestory(&self) -> bool {
        matches!(self, LLMProvider::CodeStory(_))
    }

    pub fn is_anthropic_api_key(&self) -> bool {
        matches!(self, LLMProvider::Anthropic)
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum LLMProviderAPIKeys {
    OpenAI(OpenAIProvider),
    TogetherAI(TogetherAIProvider),
    Ollama(OllamaProvider),
    OpenAIAzureConfig(AzureConfig),
    LMStudio(LMStudioConfig),
    OpenAICompatible(OpenAICompatibleConfig),
    CodeStory(CodestoryAccessToken),
    Anthropic(AnthropicAPIKey),
    FireworksAI(FireworksAPIKey),
    GeminiPro(GeminiProAPIKey),
    GoogleAIStudio(GoogleAIStudioKey),
    OpenRouter(OpenRouterAPIKey),
    GroqProvider(GroqProviderAPIKey),
    Deepseek(DeepseekConfig),
}

impl LLMProviderAPIKeys {
    pub fn is_openai(&self) -> bool {
        matches!(self, LLMProviderAPIKeys::OpenAI(_))
    }

    pub fn is_codestory(&self) -> bool {
        matches!(self, LLMProviderAPIKeys::CodeStory(_))
    }

    pub fn provider_type(&self) -> LLMProvider {
        match self {
            LLMProviderAPIKeys::OpenAI(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::TogetherAI(_) => LLMProvider::TogetherAI,
            LLMProviderAPIKeys::Ollama(_) => LLMProvider::Ollama,
            LLMProviderAPIKeys::OpenAIAzureConfig(_) => {
                LLMProvider::Azure(AzureOpenAIDeploymentId {
                    deployment_id: "".to_owned(),
                })
            }
            LLMProviderAPIKeys::LMStudio(_) => LLMProvider::LMStudio,
            LLMProviderAPIKeys::CodeStory(_) => {
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
            }
            LLMProviderAPIKeys::OpenAICompatible(_) => LLMProvider::OpenAICompatible,
            LLMProviderAPIKeys::Anthropic(_) => LLMProvider::Anthropic,
            LLMProviderAPIKeys::FireworksAI(_) => LLMProvider::FireworksAI,
            LLMProviderAPIKeys::GeminiPro(_) => LLMProvider::GeminiPro,
            LLMProviderAPIKeys::GoogleAIStudio(_) => LLMProvider::GoogleAIStudio,
            LLMProviderAPIKeys::OpenRouter(_) => LLMProvider::OpenRouter,
            LLMProviderAPIKeys::GroqProvider(_) => LLMProvider::Groq,
            LLMProviderAPIKeys::Deepseek(_) => LLMProvider::Deepseek,
        }
    }

    // Gets the relevant key from the llm provider
    pub fn key(&self, llm_provider: &LLMProvider) -> Option<Self> {
        match llm_provider {
            LLMProvider::OpenAI => {
                if let LLMProviderAPIKeys::OpenAI(key) = self {
                    Some(LLMProviderAPIKeys::OpenAI(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::TogetherAI => {
                if let LLMProviderAPIKeys::TogetherAI(key) = self {
                    Some(LLMProviderAPIKeys::TogetherAI(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::Ollama => {
                if let LLMProviderAPIKeys::Ollama(key) = self {
                    Some(LLMProviderAPIKeys::Ollama(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::LMStudio => {
                if let LLMProviderAPIKeys::LMStudio(key) = self {
                    Some(LLMProviderAPIKeys::LMStudio(key.clone()))
                } else {
                    None
                }
            }
            // Azure is weird, so we are have to copy the config which we get
            // from the provider keys and then set the deployment id of it
            // properly for the azure provider, if its set to "" that means
            // we do not have a deployment key and we should be returning quickly
            // here.
            // NOTE: We should change this to using the codestory configuration
            // and make calls appropriately, for now this is fine
            LLMProvider::Azure(deployment_id) => {
                if deployment_id.deployment_id == "" {
                    return None;
                }
                if let LLMProviderAPIKeys::OpenAIAzureConfig(key) = self {
                    let mut azure_config = key.clone();
                    azure_config.deployment_id = deployment_id.deployment_id.to_owned();
                    Some(LLMProviderAPIKeys::OpenAIAzureConfig(azure_config))
                } else {
                    None
                }
            }
            // big up codestory provider brrrr
            LLMProvider::CodeStory(_) => {
                if let LLMProviderAPIKeys::CodeStory(access_token) = self {
                    Some(LLMProviderAPIKeys::CodeStory(access_token.clone()))
                } else {
                    None
                }
            }
            LLMProvider::OpenAICompatible => {
                if let LLMProviderAPIKeys::OpenAICompatible(openai_compatible) = self {
                    Some(LLMProviderAPIKeys::OpenAICompatible(
                        openai_compatible.clone(),
                    ))
                } else {
                    None
                }
            }
            LLMProvider::Anthropic => {
                if let LLMProviderAPIKeys::Anthropic(api_key) = self {
                    Some(LLMProviderAPIKeys::Anthropic(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::FireworksAI => {
                if let LLMProviderAPIKeys::FireworksAI(api_key) = self {
                    Some(LLMProviderAPIKeys::FireworksAI(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::GeminiPro => {
                if let LLMProviderAPIKeys::GeminiPro(api_key) = self {
                    Some(LLMProviderAPIKeys::GeminiPro(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::GoogleAIStudio => {
                if let LLMProviderAPIKeys::GoogleAIStudio(api_key) = self {
                    Some(LLMProviderAPIKeys::GoogleAIStudio(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::OpenRouter => {
                if let LLMProviderAPIKeys::OpenRouter(api_key) = self {
                    Some(LLMProviderAPIKeys::OpenRouter(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::Groq => {
                if let LLMProviderAPIKeys::GroqProvider(groq_api_key) = self {
                    Some(LLMProviderAPIKeys::GroqProvider(groq_api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::Deepseek => {
                if let LLMProviderAPIKeys::Deepseek(deepseek_config) = self {
                    Some(LLMProviderAPIKeys::Deepseek(deepseek_config.clone()))
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAIProvider {
    pub api_key: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TogetherAIProvider {
    pub api_key: String,
}

impl TogetherAIProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

/// Groq API key which is used to use an account on Groq
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroqProviderAPIKey {
    pub api_key: String,
}

impl GroqProviderAPIKey {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterAPIKey {
    pub api_key: String,
}

impl OpenRouterAPIKey {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GoogleAIStudioKey {
    pub api_key: String,
}

impl GoogleAIStudioKey {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GeminiProAPIKey {
    pub api_key: String,
    pub api_base: String,
}

impl GeminiProAPIKey {
    pub fn new(api_key: String, api_base: String) -> Self {
        Self { api_key, api_base }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FireworksAPIKey {
    pub api_key: String,
}

impl FireworksAPIKey {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AnthropicAPIKey {
    pub api_key: String,
}

// Named AccessToken for consistency with workOS / ide language
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CodestoryAccessToken {
    pub access_token: String,
}

impl CodestoryAccessToken {
    pub fn new(access_token: String) -> Self {
        Self { access_token }
    }
}

impl AnthropicAPIKey {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAICompatibleConfig {
    pub api_key: String,
    pub api_base: String,
}

impl OpenAICompatibleConfig {
    pub fn new(api_key: String, api_base: String) -> Self {
        Self { api_base, api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OllamaProvider {}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AzureConfig {
    pub deployment_id: String,
    pub api_base: String,
    pub api_key: String,
    pub api_version: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LMStudioConfig {
    pub api_base: String,
}

impl LMStudioConfig {
    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CodeStoryConfig {
    pub llm_type: LLMType,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DeepseekConfig {
    pub api_key: String,
    pub api_base: String,
}

impl DeepseekConfig {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            api_base: "https://api.deepseek.com/v1".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AzureOpenAIDeploymentId, LLMProvider, LLMProviderAPIKeys};

    #[test]
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

    #[test]
    fn test_reading_provider_keys() {
        let provider_keys = LLMProviderAPIKeys::OpenAI(super::OpenAIProvider {
            api_key: "testing".to_owned(),
        });
        let string_provider_keys = serde_json::to_string(&provider_keys).expect("to work");
        assert_eq!(string_provider_keys, "",);
    }
}