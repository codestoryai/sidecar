//! Generates the hot streak for the session which allows us to optimistically
//! provide the NEXT most important step for the users
//! The goal is to keep the user in flow by informing them of the next most
//! important thing to do

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

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

use super::chat::{SessionChatMessage, SessionChatRole};

#[derive(Debug, Clone)]
pub struct SessionHotStreakResponse {
    reply: String,
}

impl SessionHotStreakResponse {
    pub fn new(reply: String) -> Self {
        Self { reply }
    }

    pub fn reply(&self) -> &str {
        &self.reply
    }
}

#[derive(Debug, Clone)]
pub struct SessionHotStreakRequest {
    diff_recent_edits: DiffRecentChanges,
    user_context: UserContext,
    previous_messages: Vec<SessionChatMessage>,
    query: String,
    repo_ref: RepoRef,
    project_labels: Vec<String>,
    session_id: String,
    exchange_id: String,
    ui_sender: UnboundedSender<UIEventWithID>,
    cancellation_token: tokio_util::sync::CancellationToken,
    llm_properties: LLMProperties,
}

impl SessionHotStreakRequest {
    pub fn new(
        diff_recent_edits: DiffRecentChanges,
        user_context: UserContext,
        previous_messages: Vec<SessionChatMessage>,
        query: String,
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
            query,
            repo_ref,
            project_labels,
            session_id,
            exchange_id,
            ui_sender,
            cancellation_token,
            llm_properties,
        }
    }
}

pub struct SessionHotStreakClient {
    llm_client: Arc<LLMBroker>,
}

impl SessionHotStreakClient {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn system_message(&self, context: &SessionHotStreakRequest) -> String {
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
            r#"You are an expert software engineer who is going to help the user figure out the next "edit" they should perform.
If no such edits are required then help the user out by giving them feedback on that has been performed (focussing on the recent edits which have happened).
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
    Here the codeblock has line numbers 1 and 2, do not write the line numbers in the codeblock

# How to help the user with the next "edit"
- You will see a lot of diagnostic errors which are present in the editor along with how to fix them (these are provided by the language server running in the editor)
- Since the developer's focus is of paramout importance, you will select the most high quality "edit" which they should perform.
- Ideally your first step towards fixing errors should be about fixing type errors (since they are the most annoying ones)
- Remember you can suggest edits in at most 2 files right now, keep your edits concise and list out the files which you want to edit in full
- Put a single line or more of reasoning on what you want to fix (this will help the user approve the changes you are suggesting later on)
- The code blocks which you generate for the edits should be of very high quality and small, extensively use `// Rest of the code..` and help the user understand how to fix the problem.
- If you want more information ask the user explictly for it, this is helpful to the developer as well.
- You HAVE A SINGLE CHANCE to suggest "edits" to the user, so use it wisely"#
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
    async fn user_message(&self, context: SessionHotStreakRequest) -> Vec<LLMClientMessage> {
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
                .map(|previous_message| match previous_message.role() {
                    SessionChatRole::User => {
                        LLMClientMessage::user(previous_message.message().to_owned())
                    }
                    SessionChatRole::Assistant => {
                        LLMClientMessage::assistant(previous_message.message().to_owned())
                    }
                }),
        );
        let query = context.query.to_owned();
        let location = context
            .repo_ref
            .local_path()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        messages.push(LLMClientMessage::user(format!(
            r#"I can see the following diagnostic errors:
{query}

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
    Here the codeblock has line numbers 1 and 2, do not write the line numbers in the codeblock

# How to help the user with the next "edit"
- You will see a lot of diagnostic errors which are present in the editor along with how to fix them (these are provided by the language server running in the editor)
- Since the developer's focus is of paramout importance, you will select the most high quality "edit" which they should perform.
- Ideally your first step towards fixing errors should be about fixing type errors (since they are the most annoying ones)
- Remember you can suggest edits in at most 2 files right now, keep your edits concise and list out the files which you want to edit in full
- Put a single line or more of reasoning on what you want to fix (this will help the user approve the changes you are suggesting later on)
- The code blocks which you generate for the edits should be of very high quality and small, extensively use `// Rest of the code..` and help the user understand how to fix the problem.
- You HAVE A SINGLE CHANCE to suggest "edits" to the user, so use it wisely
- If you want more information ask the user explictly for it, this is helpful to the developer as well.
- Only reply in natural language and do not reply back in the format of a plan."#
        )));
        messages
    }
}

#[async_trait]
impl Tool for SessionHotStreakClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_context_driven_hot_streak_reply()?;
        let cancellation_token = context.cancellation_token.clone();
        let ui_sender = context.ui_sender.clone();
        let root_id = context.session_id.to_owned();
        let exchange_id = context.exchange_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message(&context));
        let user_messages = self.user_message(context).await;

        let mut messages = vec![system_message];
        messages.extend(user_messages);

        let request =
            LLMClientCompletionRequest::new(llm_properties.llm().clone(), messages, 0.2, None);

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

        let response = llm_response.await;
        println!("session_hot_streak_client::response::({:?})", &response);
        let answer_up_until_now = polling_llm_response.await;

        match (response, answer_up_until_now) {
            (Some(Ok(Ok(_))), Ok(response)) => Ok(ToolOutput::context_driven_hot_streak_reply(
                SessionHotStreakResponse::new(response),
            )),
            (Some(Ok(Err(e))), _) => Err(ToolError::LLMClientError(e)),
            (Some(Err(_)), _) | (None, _) => Err(ToolError::UserCancellation),
            (_, Err(_)) => Err(ToolError::UserCancellation),
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
