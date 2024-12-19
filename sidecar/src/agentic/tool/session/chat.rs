//! Creates a chat tool which can use the session and the previous messages
//! and generates the reply

use std::sync::Arc;

use crate::{
    agentic::{
        symbol::{identifier::LLMProperties, ui_event::UIEventWithID},
        tool::{
            errors::ToolError,
            helpers::{
                cancellation_future::run_with_cancellation, diff_recent_changes::DiffRecentChanges,
            },
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    repo::types::RepoRef,
    user_context::types::UserContext,
};
use async_trait::async_trait;
use futures::StreamExt;
use llm_client::{
    broker::LLMBroker,
    clients::types::{
        LLMClientCompletionRequest, LLMClientMessage, LLMClientMessageImage, LLMClientRole,
        LLMClientToolReturn, LLMClientToolUse,
    },
};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, serde::Serialize)]
pub enum SessionChatRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionChatMessageImage {
    r#type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionChatToolUse {
    name: String,
    id: String,
    schema: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionChatToolReturn {
    tool_use_id: String,
    tool_name: String,
    content: String,
}

impl SessionChatToolReturn {
    pub fn new(tool_use_id: String, tool_name: String, content: String) -> Self {
        Self {
            tool_use_id,
            tool_name,
            content,
        }
    }

    pub fn to_llm_tool_return(&self) -> LLMClientToolReturn {
        LLMClientToolReturn::new(
            self.tool_use_id.to_owned(),
            self.tool_name.to_owned(),
            self.content.to_owned(),
        )
    }
}

impl SessionChatToolUse {
    pub fn new(name: String, id: String, schema: serde_json::Value) -> Self {
        Self { name, id, schema }
    }

    pub fn to_llm_tool_use(&self) -> LLMClientToolUse {
        LLMClientToolUse::new(
            self.name.to_owned(),
            self.id.to_owned(),
            self.schema.clone(),
        )
    }
}

impl SessionChatMessageImage {
    pub fn new(r#type: String, media_type: String, data: String) -> Self {
        Self {
            r#type,
            media_type,
            data,
        }
    }

    pub fn to_llm_image(&self) -> LLMClientMessageImage {
        LLMClientMessageImage::new(
            self.r#type.to_owned(),
            self.media_type.to_owned(),
            self.data.to_owned(),
        )
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionChatMessage {
    message: String,
    images: Vec<SessionChatMessageImage>,
    tool_use: Vec<SessionChatToolUse>,
    tool_return: Vec<SessionChatToolReturn>,
    role: SessionChatRole,
}

impl SessionChatMessage {
    pub fn new(
        role: SessionChatRole,
        message: String,
        images: Vec<SessionChatMessageImage>,
    ) -> Self {
        Self {
            role,
            message,
            images,
            tool_use: vec![],
            tool_return: vec![],
        }
    }

    pub fn images(&self) -> &[SessionChatMessageImage] {
        self.images.as_slice()
    }

    pub fn tool_return(&self) -> &[SessionChatToolReturn] {
        self.tool_return.as_slice()
    }

    pub fn tool_use(&self) -> &[SessionChatToolUse] {
        self.tool_use.as_slice()
    }

    pub fn assistant(message: String, images: Vec<SessionChatMessageImage>) -> Self {
        Self {
            message,
            tool_use: vec![],
            tool_return: vec![],
            role: SessionChatRole::Assistant,
            images,
        }
    }

    pub fn insert_tool_return_value(mut self, tool_return: SessionChatToolReturn) -> Self {
        self.tool_return.push(tool_return);
        self
    }

    pub fn insert_tool_use(mut self, tool_use: SessionChatToolUse) -> Self {
        self.tool_use.push(tool_use);
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn user(message: String, images: Vec<SessionChatMessageImage>) -> Self {
        Self {
            message,
            tool_use: vec![],
            tool_return: vec![],
            role: SessionChatRole::User,
            images,
        }
    }

    pub fn role(&self) -> &SessionChatRole {
        &self.role
    }

    pub fn from_llm_message(llm_message: LLMClientMessage) -> Self {
        let role = llm_message.role();
        let message = llm_message.content();
        let role = match role {
            &LLMClientRole::Assistant => SessionChatRole::Assistant,
            &LLMClientRole::User => SessionChatRole::User,
            _ => SessionChatRole::User,
        };
        let images = llm_message
            .images()
            .into_iter()
            .map(|llm_image| {
                SessionChatMessageImage::new(
                    llm_image.data().to_owned(),
                    llm_image.media().to_owned(),
                    llm_image.data().to_owned(),
                )
            })
            .collect();
        let tool_use = llm_message
            .tool_use_value()
            .into_iter()
            .map(|tool_use| {
                SessionChatToolUse::new(
                    tool_use.name().to_owned(),
                    tool_use.id().to_owned(),
                    tool_use.input().clone(),
                )
            })
            .collect();
        let tool_return = llm_message
            .tool_return_value()
            .into_iter()
            .map(|tool_return_value| {
                SessionChatToolReturn::new(
                    tool_return_value.tool_use_id().to_owned(),
                    tool_return_value.tool_name().to_owned(),
                    tool_return_value.content().to_owned(),
                )
            })
            .collect();
        Self {
            message: message.to_owned(),
            role,
            images,
            tool_use,
            tool_return,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionChatClientRequest {
    diff_recent_edits: DiffRecentChanges,
    user_context: UserContext,
    previous_messages: Vec<SessionChatMessage>,
    repo_ref: RepoRef,
    project_labels: Vec<String>,
    session_id: String,
    exchange_id: String,
    ui_sender: UnboundedSender<UIEventWithID>,
    cancellation_token: tokio_util::sync::CancellationToken,
    llm_properties: LLMProperties,
}

impl SessionChatClientRequest {
    pub fn new(
        diff_recent_edits: DiffRecentChanges,
        user_context: UserContext,
        previous_messages: Vec<SessionChatMessage>,
        repo_ref: RepoRef,
        project_labels: Vec<String>,
        session_id: String,
        exchange_id: String,
        ui_sender: UnboundedSender<UIEventWithID>,
        cancellation_token: tokio_util::sync::CancellationToken,
        llm_properties: LLMProperties,
    ) -> Self {
        Self {
            diff_recent_edits,
            user_context,
            previous_messages,
            session_id,
            exchange_id,
            repo_ref,
            project_labels,
            ui_sender,
            cancellation_token,
            llm_properties,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionChatClientResponse {
    reply: String,
}

impl SessionChatClientResponse {
    pub fn reply(self) -> String {
        self.reply
    }
}

pub struct SessionChatClient {
    llm_client: Arc<LLMBroker>,
}

impl SessionChatClient {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn system_message(&self, context: &SessionChatClientRequest) -> String {
        let location = context
            .repo_ref
            .local_path()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut project_labels_context = vec![];
        context
            .project_labels
            .to_vec()
            .into_iter()
            .for_each(|project_label| {
                if !project_labels_context.contains(&project_label) {
                    project_labels_context.push(project_label.to_string());
                    project_labels_context.push(project_label.to_string());
                }
            });
        let project_labels_str = project_labels_context.join(",");
        let project_labels_context = format!(
            r#"- You are given the following project labels which are associated with the codebase:
{project_labels_str}
"#
        );
        let system_message = format!(
            r#"You are an expert software engineer who is going to help the user with their questions.
Your job is to answer the user query which is a followup to the conversation we have had.

Provide only as much information and code as is necessary to answer the query, but be concise. Keep number of quoted lines to a minimum when possible.
When referring to code, you must provide an example in a code block.

{project_labels_context}

Respect these rules at all times:
- When asked for your name, you must respond with "Aide".
- Follow the user's requirements carefully & to the letter.
- Minimize any other prose.
- Unless directed otherwise, the user is expecting for you to edit their selected code.
- Link ALL paths AND code symbols (functions, methods, fields, classes, structs, types, variables, values, definitions, directories, etc) by embedding them in a markdown link, with the URL corresponding to the full path, and the anchor following the form `LX` or `LX-LY`, where X represents the starting line number, and Y represents the ending line number, if the reference is more than one line.
    - For example, to refer to lines 50 to 78 in a sentence, respond with something like: The compiler is initialized in [`src/foo.rs`]({location}src/foo.rs#L50-L78)
    - For example, to refer to the `new` function on a struct, respond with something like: The [`new`]({location}src/bar.rs#L26-53) function initializes the struct
    - For example, to refer to the `foo` field on a struct and link a single line, respond with something like: The [`foo`]({location}src/foo.rs#L138) field contains foos. Do not respond with something like [`foo`]({location}src/foo.rs#L138-L138)
    - For example, to refer to a folder `foo`, respond with something like: The files can be found in [`foo`]({location}path/to/foo/) folder
- Do not print out line numbers directly, only in a link
- Do not refer to more lines than necessary when creating a line range, be precise
- Do NOT output bare symbols. ALL symbols must include a link
    - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
    - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Link all fields
    - E.g. Do not simply write: "It has one main field: `foo`." Instead, write: "It has one main field: [`foo`]({location}src/foo.rs#L193)."
- Do NOT link external urls not present in the context, do NOT link urls from the internet
- Link all symbols, even when there are multiple in one sentence
    - E.g. Do not simply write: "Bars are [`Foo`]( that return a list filled with `Bar` variants." Instead, write: "Bars are functions that return a list filled with [`Bar`]({location}src/bar.rs#L38-L57) variants."
- Code blocks MUST be displayed to the user using markdown
- Code blocks MUST be displayed to the user using markdown and must NEVER include the line numbers
- If you are going to not edit sections of the code, leave "// rest of code .." as the placeholder string.
- Do NOT write the line number in the codeblock
    - E.g. Do not write:
    ```rust
    1. // rest of code ..
    2. // rest of code ..
    ```
    Here the codeblock has line numbers 1 and 2, do not write the line numbers in the codeblock"#
        );
        system_message
    }

    /// The messages are show as below:
    /// <user_context>
    /// </user_context>
    /// <diff_recent_changes>
    /// </diff_recent_changes>
    /// <messages>
    /// </messages>
    async fn user_message(&self, context: SessionChatClientRequest) -> Vec<LLMClientMessage> {
        let user_context = context
            .user_context
            .to_xml(Default::default())
            .await
            .unwrap_or_default();
        let diff_recent_changes = context.diff_recent_edits.to_llm_client_message();
        // we want to add the user context at the very start of the message
        let mut messages = vec![];
        // add the user context
        messages.push(LLMClientMessage::user(user_context).cache_point());
        messages.extend(diff_recent_changes);
        messages.extend(
            context
                .previous_messages
                .into_iter()
                .map(|previous_message| match previous_message.role {
                    SessionChatRole::User => LLMClientMessage::user(previous_message.message),
                    SessionChatRole::Assistant => {
                        LLMClientMessage::assistant(previous_message.message)
                    }
                }),
        );
        messages
    }
}

#[async_trait]
impl Tool for SessionChatClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_session_context_driven_chat_reply()?;
        let cancellation_token = context.cancellation_token.clone();
        let ui_sender = context.ui_sender.clone();
        let root_id = context.session_id.to_owned();
        let exchange_id = context.exchange_id.to_owned();
        let system_message = LLMClientMessage::system(self.system_message(&context)).cache_point();

        // so now chat will be routed through codestory provider
        let llm_properties = context.llm_properties.clone();
        println!(
            "session_chat_client::llm_properties::({:?})",
            &llm_properties
        );

        let user_messages = self.user_message(context).await;
        let mut messages = vec![system_message];
        messages.extend(user_messages);

        let request =
            LLMClientCompletionRequest::new(llm_properties.llm().clone(), messages, 0.2, None);

        // now we have to poll both the stream which will send deltas and also the one
        // which will poll the future from the stream
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_llm_client = self.llm_client.clone();
        let cloned_root_id = root_id.to_owned();
        let llm_response = run_with_cancellation(
            cancellation_token,
            tokio::spawn(async move {
                cloned_llm_client
                    .stream_completion(
                        llm_properties.api_key().clone(),
                        request,
                        llm_properties.provider().clone(),
                        vec![
                            ("event_type".to_owned(), "session_chat".to_owned()),
                            ("root_id".to_owned(), cloned_root_id),
                        ]
                        .into_iter()
                        .collect(),
                        sender,
                    )
                    .await
            }),
        );

        // now poll from the receiver where we are getting deltas
        let polling_llm_response = tokio::spawn(async move {
            let ui_sender = ui_sender;
            let request_id = root_id;
            let exchange_id = exchange_id;
            let mut answer_up_until_now = "".to_owned();
            let mut delta = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
            while let Some(stream_msg) = delta.next().await {
                answer_up_until_now = stream_msg.answer_up_until_now().to_owned();
                let _ = ui_sender.send(UIEventWithID::chat_event(
                    request_id.to_owned(),
                    exchange_id.to_owned(),
                    stream_msg.answer_up_until_now().to_owned(),
                    stream_msg.delta().map(|delta| delta.to_owned()),
                ));
            }
            answer_up_until_now
        });

        // now wait for the llm response to finsih, which will resolve even if the
        // cancellation token is cancelled in between
        let response = llm_response.await;
        println!("session_chat_client::response::({:?})", &response);
        // wait for the delta streaming to finish
        let answer_up_until_now = polling_llm_response.await;
        match answer_up_until_now {
            Ok(response) => Ok(ToolOutput::context_driven_chat_reply(
                SessionChatClientResponse { reply: response },
            )),
            _ => Err(ToolError::RetriesExhausted),
        }
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
