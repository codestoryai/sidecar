//! Contains the types for model selection which we want to use

use llm_client::{
    clients::types::LLMType,
    provider::{AzureOpenAIDeploymentId, LLMProvider, LLMProviderAPIKeys},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agentic::symbol::identifier::LLMProperties;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LLMClientConfig {
    pub slow_model: LLMType,
    pub fast_model: LLMType,
    pub models: HashMap<LLMType, Model>,
    pub providers: Vec<LLMProviderAPIKeys>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LLMClientLoggingConfig {
    pub slow_model: LLMType,
    pub fast_model: LLMType,
    pub models: HashMap<LLMType, LLMProvider>,
    pub providers: Vec<LLMProvider>,
}

impl LLMClientConfig {
    /// Grabs the llm provider which is given by the user input
    pub fn llm_properties_for_slow_model(&self) -> Option<LLMProperties> {
        let model = self.models.get(&self.slow_model);
        if model.is_none() {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        let provider_api_keys = self.providers.iter().find(|p| p.key(provider).is_some());
        if let Some(provider_api_keys) = provider_api_keys {
            Some(LLMProperties::new(
                self.slow_model.clone(),
                provider.clone(),
                provider_api_keys.clone(),
            ))
        } else {
            None
        }
    }
    pub fn provider_for_slow_model(&self) -> Option<&LLMProviderAPIKeys> {
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.models.get(&self.slow_model);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.providers.iter().find(|p| p.key(provider).is_some())
    }

    pub fn provider_for_fast_model(&self) -> Option<&LLMProviderAPIKeys> {
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.models.get(&self.fast_model);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.providers.iter().find(|p| p.key(provider).is_some())
    }

    pub fn fast_model_temperature(&self) -> Option<f32> {
        self.models
            .get(&self.fast_model)
            .map(|model_config| model_config.temperature)
    }

    pub fn provider_config_for_fast_model(&self) -> Option<&LLMProvider> {
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        self.models
            .get(&self.fast_model)
            .map(|model_config| &model_config.provider)
    }

    fn nullify_azure_config(&self) -> AzureOpenAIDeploymentId {
        AzureOpenAIDeploymentId {
            deployment_id: "".to_owned(),
        }
    }

    pub fn logging_config(&self) -> LLMClientLoggingConfig {
        LLMClientLoggingConfig {
            slow_model: self.slow_model.clone(),
            fast_model: self.fast_model.clone(),
            models: self
                .models
                .iter()
                .map(|(model_type, model_config)| {
                    let provider = match model_config.provider.clone() {
                        LLMProvider::Azure(_) => LLMProvider::Azure(self.nullify_azure_config()),
                        _ => model_config.provider.clone(),
                    };
                    (model_type.clone(), provider.clone())
                })
                .collect(),
            providers: self
                .providers
                .iter()
                .map(|provider| provider.provider_type())
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    pub context_length: u32,
    pub temperature: f32,
    pub provider: LLMProvider,
}

#[cfg(test)]
mod tests {
    use llm_client::provider::{
        AzureConfig, CodeStoryLLMTypes, LLMProvider, LLMProviderAPIKeys, OllamaProvider,
    };

    use super::LLMClientConfig;

    #[test]
    fn test_json_should_convert_properly() {
        let data = r#"
        {"slow_model":"MistralInstruct","fast_model":"Gpt4","models":{"Gpt4Turbo":{"context_length":128000,"temperature":0.2,"provider":{"Azure":{"deployment_id":""}}},"Gpt4_32k":{"context_length":32768,"temperature":0.2,"provider":{"Azure":{"deployment_id":""}}},"Gpt4":{"context_length":8192,"temperature":0.2,"provider":{"Azure":{"deployment_id":"gpt4-access"}}},"GPT3_5_16k":{"context_length":16385,"temperature":0.2,"provider":{"Azure":{"deployment_id":"gpt35-turbo-access"}}},"GPT3_5":{"context_length":4096,"temperature":0.2,"provider":{"Azure":{"deployment_id":""}}},"Mixtral":{"context_length":32000,"temperature":0.2,"provider":"TogetherAI"},"MistralInstruct":{"context_length":8000,"temperature":0.2,"provider":"TogetherAI"}},"providers":[{"OpenAI":{"api_key":""}},{"OpenAIAzureConfig":{"deployment_id":"","api_base":"https://codestory-gpt4.openai.azure.com","api_key":"89ca8a49a33344c9b794b3dabcbbc5d0","api_version":"2023-08-01-preview"}},{"TogetherAI":{"api_key":"cc10d6774e67efef2004b85efdb81a3c9ba0b7682cc33d59c30834183502208d"}},{"Ollama":{}}]}
        "#;
        assert!(serde_json::from_str::<LLMClientConfig>(data).is_ok());
    }

    #[test]
    fn test_json_should_convert_properly_codestory() {
        let provider = LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None });
        println!("{:?}", serde_json::to_string(&provider));
        let data = r#"
        {"slow_model":"DeepSeekCoder33BInstruct","fast_model":"DeepSeekCoder33BInstruct","models":{"Gpt4":{"context_length":8192,"temperature":0.2,"provider":"CodeStory"}},"providers":["CodeStory",{"OpenAIAzureConfig":{"deployment_id":"","api_base":"https://codestory-gpt4.openai.azure.com","api_key":"89ca8a49a33344c9b794b3dabcbbc5d0","api_version":"2023-08-01-preview"}},{"TogetherAI":{"api_key":"cc10d6774e67efef2004b85efdb81a3c9ba0b7682cc33d59c30834183502208d"}},{"OpenAICompatible":{"api_key":"somethingelse","api_base":"testendpoint"}},{"Ollama":{}}]}
        "#;
        assert!(serde_json::from_str::<LLMClientConfig>(data).is_ok());
    }

    #[test]
    fn test_gemini_type() {
        let data = r#"
        {"slow_model":"DeepSeekCoder33BInstruct","fast_model":"DeepSeekCoder33BInstruct","models":{"Gpt4":{"context_length":8192,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"GPT3_5_16k":{"context_length":16385,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"CodeLlama7BInstruct":{"context_length":16384,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"DeepSeekCoder33BInstruct":{"context_length":16384,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"DeepSeekCoder1.3BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"DeepSeekCoder6BInstruct":{"context_length":16384,"temperature":0.2,"provider":"Ollama"},"ClaudeSonnet":{"context_length":200000,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"ClaudeHaiku":{"context_length":200000,"temperature":0.2,"provider":{"CodeStory":{"llm_type":null}}},"GeminiPro1.5":{"context_length":1000000,"temperature":0.2,"provider":null}},"providers":["CodeStory",{"Ollama":{}},{"GeminiPro":{"api_key":"ya29.a0Ad52N3-fixk_UEEgKks633q1SaF6YmrY5vt9VroX40j7nqfU5Ny4S_aLH-AzNDXPfAGxOQXGMSD_LTlQYgSYQYEtKlIWIulI1HD9o9wNSkbGP_EawaEhf4UNZe8hwKDQfv9h2727V3on25fLjS_YsRfJTR3Iz9F-ZOQv6tBAfwaCgYKAZMSARESFQHGX2MiNXTyT7SzpY5qnOijIPOc4A0177","api_base":"anton-390822"}}]}
        "#.to_owned();
        let data = serde_json::from_str::<LLMClientConfig>(&data);
        println!("{:?}", data);
        assert!(data.is_ok());
    }

    #[test]
    fn test_custom_llm_type_json() {
        let llm_config = LLMClientConfig {
            slow_model: llm_client::clients::types::LLMType::Custom("slow_model".to_owned()),
            fast_model: llm_client::clients::types::LLMType::Custom("fast_model".to_owned()),
            models: vec![(
                llm_client::clients::types::LLMType::Custom("slow_model".to_owned()),
                super::Model {
                    context_length: 16000,
                    temperature: 0.2,
                    provider: llm_client::provider::LLMProvider::Azure(
                        llm_client::provider::AzureOpenAIDeploymentId {
                            deployment_id: "gpt35-turbo-access".to_owned(),
                        },
                    ),
                },
            )]
            .into_iter()
            .collect(),
            providers: vec![
                LLMProviderAPIKeys::OpenAIAzureConfig(AzureConfig {
                    deployment_id: "gpt35-turbo-access".to_owned(),
                    api_base: "https://codestory-gpt4.openai.azure.com".to_owned(),
                    api_key: "89ca8a49a33344c9b794b3dabcbbc5d0".to_owned(),
                    api_version: "v1".to_owned(),
                }),
                LLMProviderAPIKeys::Ollama(OllamaProvider {}),
            ],
        };
        let client_config_str = serde_json::to_string(&llm_config).expect("to work");
        assert_eq!(client_config_str, "{\"slow_model\":\"slow_model\",\"fast_model\":\"fast_model\",\"models\":{\"slow_model\":{\"context_length\":16000,\"temperature\":0.2,\"provider\":{\"Azure\":{\"deployment_id\":\"gpt35-turbo-access\"}}}},\"providers\":[{\"OpenAIAzureConfig\":{\"deployment_id\":\"gpt35-turbo-access\",\"api_base\":\"https://codestory-gpt4.openai.azure.com\",\"api_key\":\"89ca8a49a33344c9b794b3dabcbbc5d0\",\"api_version\":\"v1\"}}]}");
    }
}
