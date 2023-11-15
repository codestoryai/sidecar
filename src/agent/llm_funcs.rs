use std::collections::HashMap;

/// We define all the helper stuff required here for the LLM to be able to do
/// things.
use async_openai::config::AzureConfig;
use async_openai::config::OpenAIConfig;
use async_openai::types::ChatCompletionFunctionCall;
use async_openai::types::ChatCompletionFunctionsArgs;
use async_openai::types::ChatCompletionRequestMessageArgs;
use async_openai::types::CreateChatCompletionRequest;
use async_openai::types::CreateChatCompletionRequestArgs;
use async_openai::types::CreateCompletionRequestArgs;
use async_openai::types::FunctionCall;
use async_openai::Client;
use futures::StreamExt;
use tiktoken_rs::FunctionCall as tiktoken_rs_FunctionCall;
use tracing::debug;
use tracing::error;
use tracing::warn;

use crate::chunking::text_document::DocumentSymbol;
use crate::in_line_agent::types::ContextSelection;
use crate::in_line_agent::types::InLineAgentAnswer;

use super::types::Answer;
use super::types::CompletionItem;

pub mod llm {
    use std::collections::HashMap;

    #[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct FunctionCall {
        pub name: Option<String>,
        pub arguments: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Function {
        pub name: String,
        pub description: String,
        pub parameters: Parameters,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Parameters {
        #[serde(rename = "type")]
        pub _type: String,
        pub properties: HashMap<String, Parameter>,
        pub required: Vec<String>,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    pub enum Role {
        User,
        System,
        Assistant,
        Function,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Parameter {
        #[serde(rename = "type")]
        pub _type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub items: Option<Box<Parameter>>,
    }
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    #[serde(untagged)]
    pub enum Message {
        FunctionReturn {
            role: Role,
            name: String,
            content: String,
        },
        FunctionCall {
            role: Role,
            function_call: FunctionCall,
            content: (),
        },
        // NB: This has to be the last variant as this enum is marked `#[serde(untagged)]`, so
        // deserialization will always try this variant last. Otherwise, it is possible to
        // accidentally deserialize a `FunctionReturn` value as `PlainText`.
        PlainText {
            role: Role,
            content: String,
        },
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Messages {
        pub messages: Vec<Message>,
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Functions {
        pub functions: Vec<Function>,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct Request {
        pub messages: Messages,
        pub functions: Option<Functions>,
        pub provider: Provider,
        pub max_tokens: Option<u32>,
        pub temperature: Option<f32>,
        pub presence_penalty: Option<f32>,
        pub frequency_penalty: Option<f32>,
        pub model: Option<String>,
        #[serde(default)]
        pub extra_stop_sequences: Vec<String>,
        pub session_reference_id: Option<String>,
    }

    #[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Provider {
        OpenAi,
    }

    #[derive(thiserror::Error, Debug, serde::Deserialize, serde::Serialize)]
    pub enum Error {
        #[error("bad OpenAI request")]
        BadOpenAiRequest,

        #[error("incorrect configuration")]
        BadConfiguration,
    }

    #[derive(Debug, Clone)]
    pub enum OpenAIModel {
        GPT4,
        GPT4_32k,
        GPT3_5_16k,
        GPT3_5Instruct,
    }

    impl OpenAIModel {
        pub fn model_name(&self) -> String {
            match self {
                OpenAIModel::GPT3_5_16k => "gpt-3.5-turbo-16k-0613".to_owned(),
                OpenAIModel::GPT4 => "gpt-4-0613".to_owned(),
                OpenAIModel::GPT4_32k => "gpt-4-32k-0613".to_owned(),
                OpenAIModel::GPT3_5Instruct => "gpt-3.5-turbo-instruct".to_owned(),
            }
        }

        pub fn get_model(model_name: &str) -> anyhow::Result<OpenAIModel> {
            if model_name == "gpt-3.5-turbo-16k-0613" {
                Ok(OpenAIModel::GPT3_5_16k)
            } else if model_name == "gpt-4-0613" {
                Ok(OpenAIModel::GPT4)
            } else if model_name == "gpt-4-32k-0613" {
                Ok(OpenAIModel::GPT4_32k)
            } else if model_name == "gpt-3.5-turbo-instruct" {
                Ok(OpenAIModel::GPT3_5Instruct)
            } else {
                Err(anyhow::anyhow!("unknown model name"))
            }
        }
    }

    pub type Result = std::result::Result<String, Error>;
}

impl llm::Message {
    pub fn system(content: &str) -> Self {
        llm::Message::PlainText {
            role: llm::Role::System,
            content: content.to_owned(),
        }
    }

    pub fn user(content: &str) -> Self {
        llm::Message::PlainText {
            role: llm::Role::User,
            content: content.to_owned(),
        }
    }

    pub fn function_call(function_call: llm::FunctionCall) -> Self {
        // This is where the assistant ends up calling the function
        llm::Message::FunctionCall {
            role: llm::Role::Assistant,
            function_call,
            content: (),
        }
    }

    pub fn function_return(name: String, content: String) -> Self {
        // This is where we assume that the function is the one returning
        // the answer to the agent
        llm::Message::FunctionReturn {
            role: llm::Role::Function,
            name,
            content,
        }
    }
}

impl From<&llm::Role> for async_openai::types::Role {
    fn from(role: &llm::Role) -> Self {
        match role {
            llm::Role::User => async_openai::types::Role::User,
            llm::Role::System => async_openai::types::Role::System,
            llm::Role::Assistant => async_openai::types::Role::Assistant,
            llm::Role::Function => async_openai::types::Role::Function,
        }
    }
}

impl llm::Role {
    pub fn to_string(&self) -> String {
        match self {
            llm::Role::Assistant => "assistant".to_owned(),
            llm::Role::Function => "function".to_owned(),
            llm::Role::System => "system".to_owned(),
            llm::Role::User => "user".to_owned(),
        }
    }
}

impl From<&llm::Message> for tiktoken_rs::ChatCompletionRequestMessage {
    fn from(m: &llm::Message) -> tiktoken_rs::ChatCompletionRequestMessage {
        match m {
            llm::Message::PlainText { role, content } => {
                tiktoken_rs::ChatCompletionRequestMessage {
                    role: role.to_string(),
                    content: Some(content.to_owned()),
                    name: None,
                    function_call: None,
                }
            }
            llm::Message::FunctionReturn {
                role,
                name,
                content,
            } => tiktoken_rs::ChatCompletionRequestMessage {
                role: role.to_string(),
                content: Some(content.to_owned()),
                name: Some(name.clone()),
                function_call: None,
            },
            llm::Message::FunctionCall {
                role,
                function_call,
                content: _,
            } => tiktoken_rs::ChatCompletionRequestMessage {
                role: role.to_string(),
                content: None,
                name: None,
                function_call: Some(tiktoken_rs_FunctionCall {
                    name: function_call
                        .name
                        .as_ref()
                        .expect("function_name to exist for function_call")
                        .to_owned(),
                    arguments: function_call.arguments.to_owned(),
                }),
            },
        }
    }
}

pub struct LlmClient {
    gpt4_client: Client<AzureConfig>,
    gpt432k_client: Client<AzureConfig>,
    gpt3_5_client: Client<AzureConfig>,
    gpt3_5_turbo_instruct: Client<OpenAIConfig>,
}
// pub struct LlmClient {
//     gpt4_client: Client<OpenAIConfig>,
//     gpt432k_client: Client<OpenAIConfig>,
//     gpt3_5_client: Client<OpenAIConfig>,
//     gpt3_5_turbo_instruct: Client<OpenAIConfig>,
// }

impl LlmClient {
    pub fn codestory_infra() -> LlmClient {
        let api_base = "https://codestory-gpt4.openai.azure.com".to_owned();
        let api_key = "89ca8a49a33344c9b794b3dabcbbc5d0".to_owned();
        let api_version = "2023-08-01-preview".to_owned();
        let azure_config = AzureConfig::new()
            .with_api_base(api_base)
            .with_api_key(api_key)
            .with_api_version(api_version)
            .with_deployment_id("gpt4-access".to_owned());
        let openai_config = OpenAIConfig::new()
            .with_org_id("org-pKCie8wjobiHKD2874lQP9wR".to_owned())
            .with_api_key("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".to_owned());
        let gpt4_config = azure_config.clone();
        let gpt4_32k_config = azure_config
            .clone()
            .with_deployment_id("gpt4-32k-access".to_owned());
        let gpt3_5_config = azure_config.with_deployment_id("gpt35-turbo-access".to_owned());
        Self {
            gpt4_client: Client::with_config(gpt4_config),
            gpt432k_client: Client::with_config(gpt4_32k_config),
            gpt3_5_client: Client::with_config(gpt3_5_config),
            gpt3_5_turbo_instruct: Client::with_config(openai_config),
        }
        // Self {
        //     gpt4_client: Client::with_config(openai_config.clone()),
        //     gpt432k_client: Client::with_config(openai_config.clone()),
        //     gpt3_5_client: Client::with_config(openai_config.clone()),
        //     gpt3_5_turbo_instruct: Client::with_config(openai_config),
        // }
    }

    pub async fn stream_response(
        &self,
        model: llm::OpenAIModel,
        messages: Vec<llm::Message>,
        functions: Option<Vec<llm::Function>>,
        temperature: f32,
        frequency_penalty: Option<f32>,
        sender: tokio::sync::mpsc::UnboundedSender<Answer>,
    ) -> anyhow::Result<String> {
        let client = self.get_model(&model);
        if client.is_none() {
            return Err(anyhow::anyhow!("model not found"));
        }
        let request =
            self.create_request(model, messages, functions, temperature, frequency_penalty);
        dbg!(&request);

        const TOTAL_CHAT_RETRIES: usize = 5;

        'retry_loop: for _ in 0..TOTAL_CHAT_RETRIES {
            let mut buf = String::new();
            let stream = client
                .expect("is_none to work")
                .chat()
                .create_stream(request.clone())
                .await;
            if stream.is_err() {
                continue 'retry_loop;
            }
            let unwrap_stream = stream.expect("is_err check above to work");
            tokio::pin!(unwrap_stream);

            loop {
                match unwrap_stream.next().await {
                    None => break,
                    Some(Ok(s)) => {
                        let delta = &s
                            .choices
                            .get(0)
                            .map(|choice| choice.delta.content.clone())
                            .flatten()
                            .unwrap_or("".to_owned());
                        buf += delta;
                        sender
                            .send(Answer {
                                answer_up_until_now: buf.to_owned(),
                                delta: Some(delta.to_owned()),
                            })
                            .expect("sending answer should not fail");
                    }
                    Some(Err(e)) => {
                        warn!(?e, "openai stream error, retrying");
                        continue 'retry_loop;
                    }
                }
            }

            return Ok(buf);
        }
        Err(anyhow::anyhow!(
            "failed to get response from openai".to_owned()
        ))
    }

    // TODO(skcd): This needs to move somewhere else, cause we are x-poulluting
    // things between the agent and the in-line agent
    pub async fn stream_response_inline_agent(
        &self,
        model: llm::OpenAIModel,
        messages: Vec<llm::Message>,
        functions: Option<Vec<llm::Function>>,
        temperature: f32,
        frequency_penalty: Option<f32>,
        sender: tokio::sync::mpsc::UnboundedSender<InLineAgentAnswer>,
        document_symbol: Option<DocumentSymbol>,
        context_selection: Option<ContextSelection>,
    ) -> anyhow::Result<String> {
        let client = self.get_model(&model);
        if client.is_none() {
            return Err(anyhow::anyhow!("model not found"));
        }
        let request =
            self.create_request(model, messages, functions, temperature, frequency_penalty);

        const TOTAL_CHAT_RETRIES: usize = 5;

        'retry_loop: for _ in 0..TOTAL_CHAT_RETRIES {
            let mut buf = String::new();
            let stream = client
                .expect("is_none to work")
                .chat()
                .create_stream(request.clone())
                .await;
            if stream.is_err() {
                continue 'retry_loop;
            }
            let unwrap_stream = stream.expect("is_err check above to work");
            tokio::pin!(unwrap_stream);

            loop {
                match unwrap_stream.next().await {
                    None => break,
                    Some(Ok(s)) => {
                        let delta = &s
                            .choices
                            .get(0)
                            .map(|choice| choice.delta.content.clone())
                            .flatten()
                            .unwrap_or("".to_owned());
                        buf += delta;
                        sender
                            .send(InLineAgentAnswer {
                                answer_up_until_now: buf.to_owned(),
                                delta: Some(delta.to_owned()),
                                state: Default::default(),
                                document_symbol: document_symbol.clone(),
                                context_selection: context_selection.clone(),
                            })
                            .expect("sending answer should not fail");
                    }
                    Some(Err(e)) => {
                        warn!(?e, "openai stream error, retrying");
                        continue 'retry_loop;
                    }
                }
            }

            return Ok(buf);
        }
        Err(anyhow::anyhow!(
            "failed to get response from openai".to_owned()
        ))
    }

    pub async fn response(
        &self,
        model: llm::OpenAIModel,
        messages: Vec<llm::Message>,
        functions: Option<Vec<llm::Function>>,
        temperature: f32,
        frequency_penalty: Option<f32>,
    ) -> anyhow::Result<String> {
        let client = self.get_model(&model);
        if client.is_none() {
            return Err(anyhow::anyhow!("model not found"));
        }
        let request =
            self.create_request(model, messages, functions, temperature, frequency_penalty);

        dbg!(&request);
        const TOTAL_CHAT_RETRIES: usize = 5;

        'retry_loop: for _ in 0..TOTAL_CHAT_RETRIES {
            let mut buf = String::new();
            let stream = client
                .expect("is_none to work")
                .chat()
                .create_stream(request.clone())
                .await;
            if stream.is_err() {
                continue 'retry_loop;
            }
            let unwrap_stream = stream.expect("is_err check above to work");
            tokio::pin!(unwrap_stream);

            loop {
                match unwrap_stream.next().await {
                    None => break,
                    Some(Ok(s)) => {
                        buf += &s
                            .choices
                            .get(0)
                            .map(|choice| choice.delta.content.clone())
                            .flatten()
                            .unwrap_or("".to_owned())
                    }
                    Some(Err(e)) => {
                        warn!(?e, "openai stream error, retrying");
                        continue 'retry_loop;
                    }
                }
            }

            return Ok(buf);
        }
        Err(anyhow::anyhow!(
            "failed to get response from openai".to_owned()
        ))
    }

    pub async fn stream_completion_call(
        &self,
        model: llm::OpenAIModel,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<CompletionItem>,
        logit_bias: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<String> {
        let client = self.get_model_openai(&model);
        if client.is_none() {
            return Err(anyhow::anyhow!("model not found"));
        }
        const TOTAL_CHAT_RETRIES: usize = 5;

        'retry_loop: for _ in 0..TOTAL_CHAT_RETRIES {
            let mut buf = "".to_owned();
            let completion_request = if let Some(ref logit_bias) = logit_bias {
                CreateCompletionRequestArgs::default()
                    .stream(true)
                    .model(model.model_name())
                    .temperature(0.1)
                    .prompt(prompt)
                    .logprobs(1)
                    .logit_bias(logit_bias.clone())
                    .build()
                    .unwrap()
            } else {
                CreateCompletionRequestArgs::default()
                    .stream(true)
                    .model(model.model_name())
                    .temperature(0.1)
                    .prompt(prompt)
                    .logprobs(1)
                    .build()
                    .unwrap()
            };
            let completion_stream = client
                .expect("is_none")
                .completions()
                .create_stream(completion_request)
                .await;
            if completion_stream.is_err() {
                continue 'retry_loop;
            }
            let unwrap_stream = completion_stream.expect("is_err check above to work");
            tokio::pin!(unwrap_stream);

            loop {
                match unwrap_stream.next().await {
                    None => break,
                    Some(Ok(value)) => {
                        let delta = &value
                            .choices
                            .get(0)
                            .map(|choice| choice.text.clone())
                            .unwrap_or("".to_owned());
                        let logprobs = &value
                            .choices
                            .get(0)
                            .map(|choice| choice.logprobs.clone())
                            .flatten();
                        buf += &value
                            .choices
                            .get(0)
                            .map(|choice| choice.text.clone())
                            .unwrap_or("".to_owned());
                        let generated_logprobs = logprobs
                            .as_ref()
                            .map(|logprob| logprob.token_logprobs.to_vec());
                        sender
                            .send(CompletionItem {
                                answer_up_until_now: buf.to_owned(),
                                delta: Some(delta.to_owned()),
                                logprobs: generated_logprobs,
                            })
                            .expect("sending answer should not fail");
                    }
                    Some(Err(e)) => {
                        warn!(?e, "openai stream error, retrying");
                        continue 'retry_loop;
                    }
                }
            }

            return Ok(buf);
        }
        Err(anyhow::anyhow!(
            "failed to get response from openai".to_owned()
        ))
    }

    pub async fn stream_function_call(
        &self,
        model: llm::OpenAIModel,
        messages: Vec<llm::Message>,
        functions: Vec<llm::Function>,
        temperature: f32,
        frequency_penalty: Option<f32>,
    ) -> anyhow::Result<Option<llm::FunctionCall>> {
        let client = self.get_model(&model);
        if client.is_none() {
            return Err(anyhow::anyhow!("model not found"));
        }
        let mut request = self.create_request(
            model,
            messages,
            Some(functions),
            temperature,
            frequency_penalty,
        );
        dbg!(&request);
        let mut final_function_call = llm::FunctionCall::default();

        const TOTAL_CHAT_RETRIES: usize = 5;
        'retry_loop: for _ in 0..TOTAL_CHAT_RETRIES {
            let mut cloned_request = request.clone();
            cloned_request.stream = Some(false);
            let data = client
                .expect("is_none to work")
                .chat()
                .create(cloned_request)
                .await;
            match data {
                Ok(mut data_okay) => {
                    let message = data_okay.choices.remove(0).message;
                    let function_call = message.function_call;
                    if let Some(function_call) = function_call {
                        final_function_call.name = Some(function_call.name);
                        final_function_call.arguments = function_call.arguments;
                        return Ok(Some(final_function_call));
                    }
                    request.temperature = Some(0.1);
                    continue 'retry_loop;
                }
                Err(e) => {
                    debug!("errored out with client create");
                    error!(?e);
                    continue 'retry_loop;
                }
            }
        }
        Ok(None)
    }

    fn get_model(&self, model: &llm::OpenAIModel) -> Option<&Client<AzureConfig>> {
        let client = match model {
            llm::OpenAIModel::GPT4 => &self.gpt4_client,
            llm::OpenAIModel::GPT4_32k => &self.gpt432k_client,
            llm::OpenAIModel::GPT3_5_16k => &self.gpt3_5_client,
            llm::OpenAIModel::GPT3_5Instruct => return None,
        };
        Some(client)
    }

    fn get_model_openai(&self, model: &llm::OpenAIModel) -> Option<&Client<OpenAIConfig>> {
        let client = match model {
            llm::OpenAIModel::GPT4 => return None,
            llm::OpenAIModel::GPT4_32k => return None,
            llm::OpenAIModel::GPT3_5_16k => return None,
            llm::OpenAIModel::GPT3_5Instruct => &self.gpt3_5_turbo_instruct,
        };
        Some(client)
    }

    fn create_request(
        &self,
        model: llm::OpenAIModel,
        messages: Vec<llm::Message>,
        functions: Option<Vec<llm::Function>>,
        temperature: f32,
        frequency_penalty: Option<f32>,
    ) -> CreateChatCompletionRequest {
        let request_messages: Vec<_> = messages
            .into_iter()
            .map(|message| match message {
                llm::Message::PlainText { role, content } => {
                    ChatCompletionRequestMessageArgs::default()
                        .role::<async_openai::types::Role>((&role).into())
                        .content(content)
                        .build()
                        .unwrap()
                }
                llm::Message::FunctionCall {
                    role,
                    function_call,
                    content: _,
                } => ChatCompletionRequestMessageArgs::default()
                    .role::<async_openai::types::Role>((&role).into())
                    .function_call(FunctionCall {
                        name: function_call
                            .name
                            .as_ref()
                            .expect("function_name to exist for function call")
                            .to_owned(),
                        arguments: function_call.arguments,
                    })
                    .build()
                    .unwrap(),
                llm::Message::FunctionReturn {
                    role,
                    name,
                    content,
                } => ChatCompletionRequestMessageArgs::default()
                    .role::<async_openai::types::Role>((&role).into())
                    .name(name)
                    .content(content)
                    .build()
                    .unwrap(),
            })
            .collect();
        let mut request_builder_args = CreateChatCompletionRequestArgs::default();
        let mut request_args_builder = request_builder_args
            .messages(request_messages)
            .temperature(temperature)
            .model(model.model_name())
            .stream(true);
        if let Some(functions) = functions {
            let function_calling: Vec<_> = functions
                .into_iter()
                .map(|function| {
                    ChatCompletionFunctionsArgs::default()
                        .name(function.name)
                        .description(function.description)
                        .parameters(
                            serde_json::to_value(function.parameters)
                                .expect("serde_json::unable_to_convert"),
                        )
                        .build()
                        .unwrap()
                })
                .collect();
            request_args_builder = request_args_builder
                .functions(function_calling)
                .function_call(ChatCompletionFunctionCall::String("auto".to_owned()));
        }
        if let Some(frequency_penalty) = frequency_penalty {
            request_args_builder = request_args_builder.frequency_penalty(frequency_penalty);
        }
        request_args_builder.build().unwrap()
    }
}
