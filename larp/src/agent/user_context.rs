//! We are going to implement how tht agent is going to use user context

use futures::stream;
use futures::StreamExt;
use llm_client::clients::types::LLMType;
use llm_client::tokenizer::tokenizer::LLMTokenizerInput;
use llm_prompts::reranking::types::CodeSpan as LLMCodeSpan;
use llm_prompts::reranking::types::ReRankCodeSpanRequest;
use tracing::info;

use crate::agent::llm_funcs;
use crate::agent::types::CodeSpan;
use crate::user_context::types::FileContentValue;
use crate::user_context::types::VariableInformation;
use crate::webserver::agent::ActiveWindowData;

use super::types::Agent;
use super::types::AgentAnswerStreamEvent;

impl Agent {
    pub async fn answer_context_using_user_data(
        &mut self,
        messages: Vec<llm_funcs::llm::Message>,
        sender: tokio::sync::mpsc::UnboundedSender<AgentAnswerStreamEvent>,
    ) -> anyhow::Result<()> {
        // multiple steps here so lets break it down for now:
        // - we get all the variables which are mentioned
        // - we get the file paths and the path aliases for them
        // - we perform the reranking on them
        // - we save the updated code snippets in the conversation message as context
        let reranking_model = self.fast_llm_model();
        info!(
            event_name = "answer_context_using_user_data",
            model = %reranking_model,
        );
        let slow_model = self.slow_llm_model();
        let query = query_from_messages(messages.as_slice());
        let answer_model = self.chat_broker.get_answer_model(slow_model)?;

        let history_token_limit = answer_model.history_tokens_limit;
        let mut prompt_token_limit = answer_model.prompt_tokens_limit;

        // we look at our messages and check how many more tokens we can save
        // and send over
        let history_tokens_in_use = self.llm_tokenizer.count_tokens(
            slow_model,
            LLMTokenizerInput::Messages(
                messages
                    .iter()
                    .map(|message| message.try_into())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )? as i64;

        // we get additional breathing room if we are using less history tokens
        // than required
        // This is the max amount of tokens we can fit from the context provided
        prompt_token_limit = prompt_token_limit + history_token_limit - history_tokens_in_use;

        // TODO(skcd): Pick up from here, we have to pass the context of the terminal output
        // to the chat somehow, figure out how to change the prompts and add things here
        let user_context = self
            .user_context
            .as_ref()
            .map(|user_context| user_context.clone())
            .unwrap_or_default();

        let open_file_data = self
            .get_last_conversation_message_immutable()
            .get_active_window();

        // TODO(skcd): Finish up this part by making sure that the chunking happens
        // both for the user selected code and the open files
        // so we get the code spans for which we want to get the context about
        let mut code_spans = if user_context.is_empty() {
            // We have no user context provided, so we should just use the open
            // file as the selection context
            match open_file_data {
                Some(active_window) => {
                    let file_tokens_len = self
                        .llm_tokenizer
                        .count_tokens_using_tokenizer(slow_model, &active_window.file_content)?
                        as i64;
                    if file_tokens_len <= prompt_token_limit {
                        // we are good, no need to filter things out here
                        let split_content = active_window.file_content.lines().collect::<Vec<_>>();
                        let code_span = LLMCodeSpan::new(
                            active_window.file_path.clone(),
                            0,
                            split_content.len().try_into().expect("to not fail"),
                            active_window.file_content.clone(),
                        );
                        vec![code_span]
                    } else {
                        // First we check if the file section in view is relevant
                        // to us, and if it is we are good we can use that as context
                        // for a faster answer
                        let file_content_in_view_len =
                            self.llm_tokenizer.count_tokens_using_tokenizer(
                                slow_model,
                                &active_window.visible_range_content,
                            )? as i64;
                        if file_content_in_view_len <= prompt_token_limit {
                            vec![LLMCodeSpan::new(
                                active_window.file_path.clone(),
                                active_window
                                    .start_line
                                    .try_into()
                                    .expect("conversation to work"),
                                active_window
                                    .end_line
                                    .try_into()
                                    .expect("conversation to work"),
                                active_window.visible_range_content.to_owned(),
                            )]
                        } else {
                            // we are not good, need to truncate here until we fit
                            // in the prompt limit
                            let files = vec![FileContentValue {
                                file_path: active_window.file_path.clone(),
                                file_content: active_window.file_content.clone(),
                                language: active_window.language.clone(),
                            }];
                            // We have to do something here to handle this properly?
                            // we have to truncate the files here if required
                            let file_code_spans = self
                                .truncate_files(
                                    prompt_token_limit,
                                    reranking_model,
                                    &query,
                                    files,
                                    sender,
                                )
                                .await?;
                            LLMCodeSpan::merge_consecutive_spans(file_code_spans)
                        }
                    }
                }
                None => {
                    vec![]
                }
            }
        } else {
            // we have user context here, so we need to create the code spans
            // and perform the truncation here
            let user_selected_variables = match open_file_data {
                Some(active_file) => {
                    merge_active_window_with_user_variables(active_file, user_context.variables)
                }
                None => user_context.variables,
            };
            info!(
                event_name = "user_selected_variables",
                user_selected_variables = ?user_selected_variables,
            );
            // let user_selected_files = user_context.file_content_map;
            // we prioritize filling the context with the user selected variables
            // first and then we can iterate over the files
            let user_selected_code_spans = self
                .truncate_user_selected_variables(
                    prompt_token_limit as i64,
                    &slow_model,
                    &query,
                    user_selected_variables,
                    sender,
                )
                .await?;
            LLMCodeSpan::merge_consecutive_spans(user_selected_code_spans)
        };

        // add the terminal selection if it exists here
        if let Some(terminal_selection) = self
            .user_context
            .as_ref()
            .map(|user_selection| user_selection.terminal_selection.as_ref())
            .flatten()
        {
            code_spans.push(LLMCodeSpan::from_terminal_selection(
                terminal_selection.to_owned(),
            ));
        }

        // we want to include the files here as well somehow
        // TODO(skcd): figure out how to get the files over here
        if let Some(folder_selection) = self
            .user_context
            .as_ref()
            .map(|user_context| user_context.folder_paths())
        {
            // we get the values from the file selection in parallel
            // and then filter out the errors
            let folder_code_spans = stream::iter(folder_selection)
                .map(|folder_selection| {
                    LLMCodeSpan::from_folder_selection(folder_selection.to_owned())
                })
                .buffer_unordered(1)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .filter_map(|value| value.ok())
                .collect::<Vec<_>>();
            code_spans.extend(folder_code_spans)
        };

        // Now we update the code spans which we have selected
        let _ = self.save_code_snippets_response(
            &query,
            code_spans
                .into_iter()
                .map(|code_span| {
                    let agent_code_span = CodeSpan::new(
                        code_span.file_path().to_owned(),
                        0,
                        code_span.start_line().try_into().expect("to not fail"),
                        code_span.end_line().try_into().expect("to not fail"),
                        code_span.data().to_owned(),
                        None,
                    );
                    agent_code_span
                })
                .collect(),
        );
        // We also retroactively save the last conversation to the database
        if let Some(last_conversation) = self.conversation_messages.last() {
            // save the conversation to the DB
            let _ = last_conversation
                .save_to_db(self.sql_db.clone(), self.reporef().clone())
                .await;
            // send it over the sender
            let _ = self.sender.send(last_conversation.clone()).await;
        }
        Ok(())
    }

    pub async fn truncate_files(
        &self,
        remaining_tokens: i64,
        reranking_model: &LLMType,
        user_query: &str,
        file_content_map: Vec<FileContentValue>,
        sender: tokio::sync::mpsc::UnboundedSender<AgentAnswerStreamEvent>,
    ) -> anyhow::Result<Vec<LLMCodeSpan>> {
        sender.send(AgentAnswerStreamEvent::ReRankingStarted)?;
        // we do the same magic as before, its just that we have a teeny tiny
        // less amount of tokens to work with, but thats fine too
        let provider_keys = self
            .provider_for_llm(reranking_model)
            .ok_or(anyhow::anyhow!("no provider keys found for slow model"))?;
        let provider_config = self
            .provider_config_for_llm(reranking_model)
            .ok_or(anyhow::anyhow!("no provider config found for slow model"))?;
        let language_parsing = self.application.language_parsing.clone();
        let code_spans = file_content_map
            .into_iter()
            .map(|file_content| {
                language_parsing
                    .chunk_file(
                        &file_content.file_path,
                        &file_content.file_content,
                        None,
                        Some(&file_content.language),
                    )
                    .into_iter()
                    .filter(|span| span.data.is_some())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|span| {
                        let code_span = LLMCodeSpan::new(
                            file_content.file_path.to_owned(),
                            span.start.try_into().expect("to not fail"),
                            span.end.try_into().expect("to not fail"),
                            span.data.expect("data to be present"),
                        );
                        code_span
                    })
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<Vec<LLMCodeSpan>>();

        // Now that we have the code spans, we just ask the LLM to rerank these
        // for us with the remaining tokens we have
        let rerank_request = ReRankCodeSpanRequest::new(
            user_query.to_owned(),
            10,
            remaining_tokens,
            code_spans,
            self.reranking_strategy(),
            reranking_model.clone(),
        );
        info!(
            event_name = "reranking_files_started",
            model = %reranking_model,
        );
        let start_time = std::time::Instant::now();
        // Let the reranker do it's magic
        let code_spans = self
            .reranker
            .rerank(
                provider_keys.clone(),
                provider_config.clone(),
                rerank_request,
                self.llm_broker.clone(),
                self.llm_tokenizer.clone(),
            )
            .await?;
        info!(
            event_name = "reranking_files_ended",
            model = %reranking_model,
            time_taken = ?start_time.elapsed(),
        );
        sender.send(AgentAnswerStreamEvent::ReRankingFinished)?;

        // We then merge the code spans together which are consecutive
        Ok(LLMCodeSpan::merge_consecutive_spans(code_spans))
    }

    pub async fn truncate_user_selected_variables(
        &self,
        remaining_tokens: i64,
        reranking_model: &LLMType,
        user_query: &str,
        user_selected_variables: Vec<VariableInformation>,
        sender: tokio::sync::mpsc::UnboundedSender<AgentAnswerStreamEvent>,
    ) -> anyhow::Result<Vec<LLMCodeSpan>> {
        let provider_keys = self
            .provider_for_llm(reranking_model)
            .ok_or(anyhow::anyhow!("no provider keys found for slow model"))?;
        let provider_config = self
            .provider_config_for_llm(reranking_model)
            .ok_or(anyhow::anyhow!("no provider config found for slow model"))?;

        let language_parsing = self.application.language_parsing.clone();
        let code_spans = user_selected_variables
            .into_iter()
            .map(|variable| {
                let start_line = variable.start_position.line();
                language_parsing
                    .chunk_file(
                        &variable.fs_file_path,
                        &variable.content,
                        None,
                        Some(&variable.language),
                    )
                    .into_iter()
                    .filter(|span| span.data.is_some())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|span| {
                        let code_span = LLMCodeSpan::new(
                            variable.fs_file_path.to_owned(),
                            // We fix the start lines here because the line numbers
                            // of the chunks will be relative to this chunk, but
                            // we want them relative to the file
                            (span.start + start_line).try_into().expect("to work"),
                            (span.end + start_line).try_into().expect("to work"),
                            span.data.expect("data to be present"),
                        );
                        code_span
                    })
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<Vec<LLMCodeSpan>>();

        // Now that we have the code spans, we just ask the LLM to rerank these
        // for us with the remaining tokens we have
        let rerank_request = ReRankCodeSpanRequest::new(
            user_query.to_owned(),
            10,
            remaining_tokens,
            code_spans,
            self.reranking_strategy(),
            reranking_model.clone(),
        );
        info!(
            event_name = "reranking_user_variable_started",
            model = %reranking_model,
        );
        let start_time = std::time::Instant::now();
        sender.send(AgentAnswerStreamEvent::ReRankingStarted)?;
        // Let the reranker do it's magic
        let code_spans = self
            .reranker
            .rerank(
                provider_keys.clone(),
                provider_config.clone(),
                rerank_request,
                self.llm_broker.clone(),
                self.llm_tokenizer.clone(),
            )
            .await?;
        info!(
            event_name = "reranking_user_variable_ended",
            model = %reranking_model,
            time_taken = ?start_time.elapsed(),
        );
        sender.send(AgentAnswerStreamEvent::ReRankingFinished)?;
        // We then merge the code spans together which are consecutive
        Ok(LLMCodeSpan::merge_consecutive_spans(code_spans))
    }
}

/// Takes a slice of `llm_funcs::llm::Message` and returns a string containing the content of the messages from the user and assistant roles.
///
/// # Arguments
///
/// * `messages` - A slice of `llm_funcs::llm::Message` representing the messages to query.
///
/// # Returns
///
/// A string containing the content of the messages from the user and assistant roles, with each message separated by a newline character.
fn query_from_messages(messages: &[llm_funcs::llm::Message]) -> String {
    messages
        .iter()
        .map(|message| match message {
            llm_funcs::llm::Message::PlainText {
                role: llm_funcs::llm::Role::User,
                content,
            } => {
                format!("User: {}", content)
            }
            llm_funcs::llm::Message::PlainText {
                role: llm_funcs::llm::Role::Assistant,
                content,
            } => {
                format!("Assistant: {}", content)
            }
            _ => "".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn merge_active_window_with_user_variables(
    active_window: &ActiveWindowData,
    user_variables: Vec<VariableInformation>,
) -> Vec<VariableInformation> {
    let mut user_variables = user_variables;
    // First we check if the has any selected variable which falls in the range
    // of the active window of the user, if it does, we do not include that and
    // just include the active window selection, if thats not the case, then
    // we add the active window selection as a user variable
    user_variables = user_variables
        .into_iter()
        .filter(|user_variable| {
            if user_variable.variable_type.selection()
                && user_variable.start_position.line() >= active_window.start_line
                && user_variable.end_position.line() <= active_window.end_line
                && active_window.file_path == user_variable.fs_file_path
            {
                false
            } else {
                true
            }
        })
        .collect();
    user_variables.push(VariableInformation::from_user_active_window(active_window));
    user_variables
}
