use std::collections::HashMap;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::provider::{LLMProvider, LLMProviderAPIKeys};

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMClientMessage, LLMClientRole, LLMType,
};

pub struct GoogleAIStdioClient {
    client: reqwest::Client,
}

impl GoogleAIStdioClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn count_tokens_endpoint(&self, model: &str, api_key: &str) -> String {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:countTokens?key={api_key}").to_owned()
    }

    // we cannot use the streaming endpoint yet since the data returned is not
    // a new line json which you would expect from a data stream
    pub fn get_api_endpoint(&self, model: &str, api_key: &str) -> String {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse&key={api_key}").to_owned()
    }

    fn model(&self, model: &LLMType) -> Option<String> {
        match model {
            LLMType::GeminiPro => Some("gemini-1.5-pro".to_owned()),
            LLMType::GeminiProFlash => Some("gemini-1.5-flash".to_owned()),
            LLMType::Gemini2_0FlashExperimental => Some("gemini-2.0-flash-exp".to_owned()),
            LLMType::Gemini2_0FlashThinkingExperimental => {
                Some("gemini-2.0-flash-thinking-exp-1219".to_owned())
            }
            LLMType::Custom(llm_name) => Some(llm_name.to_owned()),
            _ => None,
        }
    }

    fn get_system_message(&self, messages: &[LLMClientMessage]) -> Option<SystemInstruction> {
        messages
            .iter()
            .find(|m| m.role().is_system())
            .map(|m| SystemInstruction {
                role: "MODEL".to_owned(),
                parts: vec![HashMap::from([("text".to_owned(), m.content().to_owned())])],
            })
    }

    fn get_role(&self, role: &LLMClientRole) -> Option<String> {
        match role {
            LLMClientRole::System => Some("model".to_owned()),
            LLMClientRole::User => Some("user".to_owned()),
            LLMClientRole::Assistant => Some("model".to_owned()),
            _ => None,
        }
    }

    fn get_generation_config(&self, request: &LLMClientCompletionRequest) -> GenerationConfig {
        GenerationConfig {
            temperature: request.temperature(),
            // this is the maximum limit of gemini-pro-1.5
            max_output_tokens: 8192,
            candidate_count: 1,
            top_p: None,
            top_k: None,
        }
    }

    fn get_messages(&self, messages: &[LLMClientMessage]) -> Vec<Content> {
        let messages = messages
            .iter()
            .filter(|m| !m.role().is_system())
            .collect::<Vec<_>>();
        let mut previous_role = None;
        let mut accumulated_messages = vec![];
        let mut final_messages = vec![];
        for message in messages.into_iter() {
            if previous_role.is_none() {
                previous_role = Some(message.role().clone());
                accumulated_messages.push(message);
            } else {
                let previous_role_expected = previous_role.clone().expect("to work");
                let current_role = message.role();
                if &previous_role_expected == current_role {
                    accumulated_messages.push(message);
                } else {
                    let previous_role_str = self.get_role(&previous_role_expected);
                    if let Some(previous_role_str) = previous_role_str {
                        final_messages.push(Content {
                            role: previous_role_str,
                            parts: accumulated_messages
                                .iter()
                                .map(|message| {
                                    HashMap::from([(
                                        "text".to_owned(),
                                        message.content().to_owned(),
                                    )])
                                })
                                .collect(),
                        });
                    }
                    accumulated_messages = vec![message];
                    previous_role = Some(current_role.clone());
                }
            }
        }
        // Add the last group of messages
        if !accumulated_messages.is_empty() {
            if let Some(last_role) = previous_role {
                if let Some(last_role_str) = self.get_role(&last_role) {
                    final_messages.push(Content {
                        role: last_role_str,
                        parts: accumulated_messages
                            .iter()
                            .map(|message| {
                                HashMap::from([("text".to_owned(), message.content().to_owned())])
                            })
                            .collect(),
                    });
                }
            }
        }
        final_messages
    }

    fn get_api_key(&self, api_key: &LLMProviderAPIKeys) -> Option<String> {
        match api_key {
            LLMProviderAPIKeys::GoogleAIStudio(api_key) => Some(api_key.api_key.to_owned()),
            _ => None,
        }
    }

    pub async fn count_tokens(
        &self,
        context: &str,
        api_key: &str,
        model: &str,
    ) -> Result<String, LLMClientError> {
        let token_count_request = GeminiProTokenCountRequestBody {
            contents: vec![Content {
                role: "user".to_owned(),
                parts: vec![HashMap::from([("text".to_owned(), context.to_owned())])],
            }],
        };
        let count_tokens = self
            .client
            .post(self.count_tokens_endpoint(model, api_key))
            .header("Content-Type", "application/json")
            .json(&token_count_request)
            .send()
            .await?;
        let count_tokens_result = count_tokens
            .bytes()
            .await
            .map(|bytes| String::from_utf8(bytes.to_vec()));
        Ok(count_tokens_result.expect("to work").expect("to work"))
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    temperature: f32,
    top_p: Option<f32>,
    top_k: Option<u32>,
    max_output_tokens: u32,
    candidate_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Content {
    role: String,
    // the only parts we will be providing is "text": "content"
    parts: Vec<HashMap<String, String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SystemInstruction {
    role: String,
    // the only parts we will be providing is "text": "content"
    parts: Vec<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiSafetySetting {
    #[serde(rename = "category")]
    category: String,
    #[serde(rename = "threshold")]
    threshold: String,
}

impl GeminiSafetySetting {
    pub fn new(category: String, threshold: String) -> Self {
        Self {
            category,
            threshold,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiProRequestBody {
    contents: Vec<Content>,
    system_instruction: Option<SystemInstruction>,
    generation_config: GenerationConfig,
    safety_settings: Vec<GeminiSafetySetting>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiProTokenCountRequestBody {
    // system_instructions: Option<SystemInstruction>,
    contents: Vec<Content>,
    // tools: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiProResponse {
    candidates: Vec<GeminiProCandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiProCandidate {
    content: Content,
    // safety_ratings: Vec<GeminiProSafetyRating>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiProSafetyRating {
    category: String,
    probability: String,
    probability_score: f32,
    severity: String,
    severity_score: f32,
}
#[async_trait]
impl LLMClient for GoogleAIStdioClient {
    fn client(&self) -> &LLMProvider {
        &LLMProvider::GeminiPro
    }

    async fn stream_completion(
        &self,
        provider_api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let model = self.model(request.model());
        if model.is_none() {
            return Err(LLMClientError::UnSupportedModel);
        }
        let model = model.unwrap();
        let system_message = self.get_system_message(request.messages());
        let messages = self.get_messages(request.messages());
        let generation_config = self.get_generation_config(&request);
        let request = GeminiProRequestBody {
            contents: messages.to_vec(),
            system_instruction: system_message.clone(),
            generation_config,
            safety_settings: vec![
                GeminiSafetySetting::new(
                    "HARM_CATEGORY_HATE_SPEECH".to_string(),
                    "BLOCK_ONLY_HIGH".to_string(),
                ),
                GeminiSafetySetting::new(
                    "HARM_CATEGORY_DANGEROUS_CONTENT".to_string(),
                    "BLOCK_ONLY_HIGH".to_string(),
                ),
                GeminiSafetySetting::new(
                    "HARM_CATEGORY_SEXUALLY_EXPLICIT".to_string(),
                    "BLOCK_ONLY_HIGH".to_string(),
                ),
                GeminiSafetySetting::new(
                    "HARM_CATEGORY_HARASSMENT".to_string(),
                    "BLOCK_ONLY_HIGH".to_string(),
                ),
            ],
        };
        let _token_count_request = GeminiProTokenCountRequestBody {
            // system_instructions: system_message,
            contents: messages,
            // tools: vec![],
        };
        let api_key = self.get_api_key(&provider_api_key);
        if api_key.is_none() {
            return Err(LLMClientError::WrongAPIKeyType);
        }
        let api_key = api_key.expect("to be present");

        // now we need to send a request to the gemini pro api here
        let response = self
            .client
            .post(self.get_api_endpoint(&model, &api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await?;

            eprintln!("HTTP Error: {} {}", status.as_u16(), status.as_str());
            eprintln!("Response body: {}", error_body);
            return Err(LLMClientError::FailedToGetResponse);
        }

        let mut buffered_string = "".to_owned();
        let mut response_stream = response.bytes_stream().eventsource();
        while let Some(event) = response_stream.next().await {
            if let Ok(event) = event {
                let parsed_event =
                    serde_json::from_slice::<GeminiProResponse>(event.data.as_bytes())?;
                if let Some(text_part) = parsed_event.candidates[0].content.parts[0].get("text") {
                    buffered_string = buffered_string + text_part;
                    sender.send(LLMClientCompletionResponse::new(
                        buffered_string.clone(),
                        Some(text_part.to_owned()),
                        model.to_owned(),
                    ))?;
                }
            }
        }
        Ok(buffered_string)
    }

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        self.stream_completion(api_key, request, sender).await
    }

    async fn stream_prompt_completion(
        &self,
        _api_key: LLMProviderAPIKeys,
        _request: LLMClientCompletionStringRequest,
        _sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        Err(LLMClientError::GeminiProDoesNotSupportPromptCompletion)
    }
}
