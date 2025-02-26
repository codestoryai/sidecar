use axum::{
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    TogetherAI,
    Google,
    Cohere,
    Mistral,
    Meta,
}

#[derive(Debug, Clone)]
pub enum LLMType {
    // OpenAI Models
    Gpt4_32k,
    Gpt4_Preview,
    Gpt4,
    GPT3_5_16k,
    GPT3_5_Turbo,
    
    // Anthropic Models
    ClaudeOpus,
    Claude3Sonnet,
    Claude3Haiku,
    
    // Together AI Models
    CodeLLama70BInstruct,
    CodeLLama34BInstruct,
    CodeLLama13BInstruct,
    LLaMA2_70B,
    LLaMA2_13B,
    
    // Google Models
    GeminiPro,
    GeminiUltra,
    
    // Cohere Models
    CommandR,
    Command,
    
    // Mistral Models
    MistralLarge,
    MistralMedium,
    MistralSmall,
    
    // Meta Models
    LLaMA3_70B,
    LLaMA3_13B,
}

impl ToString for LLMType {
    fn to_string(&self) -> String {
        match self {
            // OpenAI
            Self::Gpt4_32k => "gpt-4-32k".to_string(),
            Self::Gpt4_Preview => "gpt-4-preview".to_string(),
            Self::Gpt4 => "gpt-4".to_string(),
            Self::GPT3_5_16k => "gpt-3.5-turbo-16k".to_string(),
            Self::GPT3_5_Turbo => "gpt-3.5-turbo".to_string(),
            
            // Anthropic
            Self::ClaudeOpus => "claude-3-opus".to_string(),
            Self::Claude3Sonnet => "claude-3-sonnet".to_string(),
            Self::Claude3Haiku => "claude-3-haiku".to_string(),
            
            // Together AI
            Self::CodeLLama70BInstruct => "codellama-70b-instruct".to_string(),
            Self::CodeLLama34BInstruct => "codellama-34b-instruct".to_string(),
            Self::CodeLLama13BInstruct => "codellama-13b-instruct".to_string(),
            Self::LLaMA2_70B => "llama2-70b".to_string(),
            Self::LLaMA2_13B => "llama2-13b".to_string(),
            
            // Google
            Self::GeminiPro => "gemini-pro".to_string(),
            Self::GeminiUltra => "gemini-ultra".to_string(),
            
            // Cohere
            Self::CommandR => "command-r".to_string(),
            Self::Command => "command".to_string(),
            
            // Mistral
            Self::MistralLarge => "mistral-large".to_string(),
            Self::MistralMedium => "mistral-medium".to_string(),
            Self::MistralSmall => "mistral-small".to_string(),
            
            // Meta
            Self::LLaMA3_70B => "llama3-70b".to_string(),
            Self::LLaMA3_13B => "llama3-13b".to_string(),
        }
    }
}

impl LLMType {
    fn provider(&self) -> LLMProvider {
        match self {
            Self::Gpt4_32k | Self::Gpt4_Preview | Self::Gpt4 | 
            Self::GPT3_5_16k | Self::GPT3_5_Turbo => LLMProvider::OpenAI,
            
            Self::ClaudeOpus | Self::Claude3Sonnet | 
            Self::Claude3Haiku => LLMProvider::Anthropic,
            
            Self::CodeLLama70BInstruct | Self::CodeLLama34BInstruct |
            Self::CodeLLama13BInstruct | Self::LLaMA2_70B |
            Self::LLaMA2_13B => LLMProvider::TogetherAI,
            
            Self::GeminiPro | Self::GeminiUltra => LLMProvider::Google,
            
            Self::CommandR | Self::Command => LLMProvider::Cohere,
            
            Self::MistralLarge | Self::MistralMedium |
            Self::MistralSmall => LLMProvider::Mistral,
            
            Self::LLaMA3_70B | Self::LLaMA3_13B => LLMProvider::Meta,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    temperature: f32,
    max_tokens: u32,
    top_p: f32,
    frequency_penalty: f32,
    presence_penalty: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    name: String,
    status: ModelStatus,
    config: ModelConfig,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Available,
    Busy,
    Offline,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigureRequest {
    model_name: String,
    config: ModelConfig,
}

pub fn router() -> Router<Arc<ModelState>> {
    Router::new()
        .route("/models/list", get(list_models))
        .route("/models/configure", post(configure_model))
        .route("/models/status", get(check_status))
}

async fn list_models() -> Json<Vec<String>> {
    let models = state.list_available_models().await;
    Json(models)
}

// Example model configuration file documentation
/// ```json
/// {
///   "config_path": "/path/to/config",
///   "model_overrides": {
///     "gpt-4": {
///       "config": {
///         "temperature": 0.7,
///         "max_tokens": 4096,
///         "top_p": 1.0,
///         "frequency_penalty": 0.0,
///         "presence_penalty": 0.0
///       },
///       "enabled": true,
///       "endpoint": "https://custom-endpoint/v1"
///     }
///   },
///   "enabled_providers": ["OpenAI", "Anthropic"],
///   "provider_endpoints": {
///     "OpenAI": "https://api.openai.com/v1",
///     "Anthropic": "https://api.anthropic.com/v1"
///   }
/// }
/// ```

fn get_default_models() -> Vec<String> {
    vec![
        // OpenAI
        LLMType::Gpt4_32k.to_string(),
        LLMType::Gpt4_Preview.to_string(),
        LLMType::Gpt4.to_string(),
        LLMType::GPT3_5_16k.to_string(),
        LLMType::GPT3_5_Turbo.to_string(),
        
        // Anthropic
        LLMType::ClaudeOpus.to_string(),
        LLMType::Claude3Sonnet.to_string(),
        LLMType::Claude3Haiku.to_string(),
        
        // Together AI
        LLMType::CodeLLama70BInstruct.to_string(),
        LLMType::CodeLLama34BInstruct.to_string(),
        LLMType::CodeLLama13BInstruct.to_string(),
        LLMType::LLaMA2_70B.to_string(),
        LLMType::LLaMA2_13B.to_string(),
        
        // Google
        LLMType::GeminiPro.to_string(),
        LLMType::GeminiUltra.to_string(),
        
        // Cohere
        LLMType::CommandR.to_string(),
        LLMType::Command.to_string(),
        
        // Mistral
        LLMType::MistralLarge.to_string(),
        LLMType::MistralMedium.to_string(),
        LLMType::MistralSmall.to_string(),
        
        // Meta
        LLMType::LLaMA3_70B.to_string(),
        LLMType::LLaMA3_13B.to_string(),
    ];
    Json(models)
}

async fn configure_model(
    State(state): State<Arc<ModelState>>,
    Json(req): Json<ConfigureRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut configs = state.configs.write().await;
    configs.insert(req.model_name, req.config);
    Ok(StatusCode::OK)
}

async fn check_status(
    State(state): State<Arc<ModelState>>,
) -> Json<HashMap<String, ModelInfo>> {
    let mut status = HashMap::new();
    let configs = state.configs.read().await;
    let status_cache = state.status_cache.read().await;

    // Check each provider's status
    for provider in state.broker.providers.keys() {
        match provider {
            LLMProvider::OpenAI => {
                add_model_status(&mut status, &LLMType::Gpt4_32k.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::Gpt4_Preview.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::Gpt4.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::GPT3_5_16k.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::GPT3_5_Turbo.to_string(), &configs, &status_cache);
            }
            LLMProvider::Anthropic => {
                add_model_status(&mut status, &LLMType::ClaudeOpus.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::Claude3Sonnet.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::Claude3Haiku.to_string(), &configs, &status_cache);
            }
            LLMProvider::TogetherAI => {
                add_model_status(&mut status, &LLMType::CodeLLama70BInstruct.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::CodeLLama34BInstruct.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::CodeLLama13BInstruct.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::LLaMA2_70B.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::LLaMA2_13B.to_string(), &configs, &status_cache);
            }
            LLMProvider::Google => {
                add_model_status(&mut status, &LLMType::GeminiPro.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::GeminiUltra.to_string(), &configs, &status_cache);
            }
            LLMProvider::Cohere => {
                add_model_status(&mut status, &LLMType::CommandR.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::Command.to_string(), &configs, &status_cache);
            }
            LLMProvider::Mistral => {
                add_model_status(&mut status, &LLMType::MistralLarge.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::MistralMedium.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::MistralSmall.to_string(), &configs, &status_cache);
            }
            LLMProvider::Meta => {
                add_model_status(&mut status, &LLMType::LLaMA3_70B.to_string(), &configs, &status_cache);
                add_model_status(&mut status, &LLMType::LLaMA3_13B.to_string(), &configs, &status_cache);
            }
        }
    }

    Json(status)
}

fn add_model_status(
    status: &mut HashMap<String, ModelInfo>,
    model_name: &str,
    configs: &HashMap<String, ModelConfig>,
    status_cache: &HashMap<String, ModelStatus>,
) {
    let default_config = ModelConfig {
        temperature: 0.7,
        max_tokens: 2048,
        top_p: 1.0,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
    };

    let config = configs.get(model_name).cloned().unwrap_or(default_config);
    let model_status = status_cache
        .get(model_name)
        .cloned()
        .unwrap_or(ModelStatus::Available);

    status.insert(
        model_name.to_string(),
        ModelInfo {
            name: model_name.to_string(),
            status: model_status,
            config,
        },
    );
}