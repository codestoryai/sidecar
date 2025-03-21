use crate::{
    agent::{
        llm_funcs, prompts,
        types::{CodeSpan, VariableInformation},
    },
    application::application::Application,
    chunking::editor_parsing::EditorParsing,
    db::sqlite::SqlDb,
    repo::types::RepoRef,
    user_context::types::UserContext,
    webserver::model_selection::LLMClientConfig,
};

/// Here we allow the agent to perform search and answer workflow related questions
/// we will later use this for code planning and also code editing
use super::types::{
    Agent, AgentAnswerStreamEvent, AgentState, ConversationMessage, ExtendedVariableInformation,
};

use anyhow::anyhow;
use anyhow::Result;
use futures::{pin_mut, FutureExt, StreamExt};
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
    tokenizer::tokenizer::{LLMTokenizer, LLMTokenizerInput},
};
use llm_prompts::{
    answer_model::AnswerModel, chat::broker::LLMChatModelBroker, reranking::broker::ReRankBroker,
};
use once_cell::sync::OnceCell;
use rake::StopWords;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info, warn};

use std::{collections::HashMap, path::Path, sync::Arc};

static STOPWORDS: OnceCell<StopWords> = OnceCell::new();
static STOP_WORDS_LIST: &str = include_str!("stopwords.txt");

pub fn stop_words() -> &'static StopWords {
    STOPWORDS.get_or_init(|| {
        let mut sw = StopWords::new();
        for w in STOP_WORDS_LIST.lines() {
            sw.insert(w.to_string());
        }
        sw
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchAction {
    /// A user-provided query.
    Query(String),

    Path {
        query: String,
    },
    #[serde(rename = "none")]
    Answer {
        paths: Vec<usize>,
    },
    Code {
        query: String,
    },
    Proc {
        query: String,
        paths: Vec<usize>,
    },
}

impl Agent {
    pub fn prepare_for_search(
        application: Application,
        reporef: RepoRef,
        session_id: uuid::Uuid,
        query: &str,
        llm_broker: Arc<LLMBroker>,
        conversation_id: uuid::Uuid,
        sql_db: SqlDb,
        mut previous_conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
        editor_parsing: EditorParsing,
        model_config: LLMClientConfig,
        llm_tokenizer: Arc<LLMTokenizer>,
        chat_broker: Arc<LLMChatModelBroker>,
        reranker: Arc<ReRankBroker>,
    ) -> Self {
        // We will take care of the search here, and use that for the next steps
        let conversation_message = ConversationMessage::search_message(
            conversation_id,
            AgentState::Search,
            query.to_owned(),
        );
        previous_conversations.push(conversation_message);
        let agent = Agent {
            application,
            reporef,
            session_id,
            conversation_messages: previous_conversations,
            llm_broker,
            sql_db,
            sender,
            user_context: None,
            project_labels: vec![],
            editor_parsing,
            model_config,
            llm_tokenizer,
            chat_broker,
            reranker,
            system_instruction: None,
        };
        agent
    }

    pub fn prepare_for_followup(
        application: Application,
        reporef: RepoRef,
        session_id: uuid::Uuid,
        llm_broker: Arc<LLMBroker>,
        sql_db: SqlDb,
        conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
        user_context: UserContext,
        project_labels: Vec<String>,
        editor_parsing: EditorParsing,
        model_config: LLMClientConfig,
        llm_tokenizer: Arc<LLMTokenizer>,
        chat_broker: Arc<LLMChatModelBroker>,
        reranker: Arc<ReRankBroker>,
        system_instruction: Option<String>,
    ) -> Self {
        let agent = Agent {
            application,
            reporef,
            session_id,
            conversation_messages: conversations,
            llm_broker,
            sql_db,
            sender,
            user_context: Some(user_context),
            project_labels,
            editor_parsing,
            model_config,
            llm_tokenizer,
            chat_broker,
            reranker,
            system_instruction,
        };
        agent
    }

    pub fn prepare_for_semantic_search(
        application: Application,
        reporef: RepoRef,
        session_id: uuid::Uuid,
        query: &str,
        llm_broker: Arc<LLMBroker>,
        conversation_id: uuid::Uuid,
        sql_db: SqlDb,
        mut previous_conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
        editor_parsing: EditorParsing,
        model_config: LLMClientConfig,
        llm_tokenizer: Arc<LLMTokenizer>,
        chat_broker: Arc<LLMChatModelBroker>,
        reranker: Arc<ReRankBroker>,
    ) -> Self {
        let conversation_message = ConversationMessage::semantic_search(
            conversation_id,
            AgentState::SemanticSearch,
            query.to_owned(),
        );
        previous_conversations.push(conversation_message);
        let agent = Agent {
            application,
            reporef,
            session_id,
            conversation_messages: previous_conversations,
            llm_broker,
            sql_db,
            sender,
            user_context: None,
            project_labels: vec![],
            editor_parsing,
            model_config,
            llm_tokenizer,
            chat_broker,
            reranker,
            system_instruction: None,
        };
        agent
    }

    pub async fn path_search(&mut self, _query: &str) -> Result<String> {
        Ok("".to_owned())
    }

    pub fn update_user_selected_variables(&mut self, user_variables: Vec<VariableInformation>) {
        let last_exchange = self.get_last_conversation_message();
        user_variables.into_iter().for_each(|user_variable| {
            last_exchange.add_user_variable(user_variable);
        })
    }

    pub fn save_extended_code_selection_variables(
        &mut self,
        extended_variable_information: Vec<ExtendedVariableInformation>,
    ) -> anyhow::Result<()> {
        for variable_information in extended_variable_information.iter() {
            let last_exchange = self.get_last_conversation_message();
            last_exchange.add_extended_variable_information(variable_information.clone());
        }
        Ok(())
    }

    pub fn save_code_snippets_response(
        &mut self,
        query: &str,
        code_snippets: Vec<CodeSpan>,
    ) -> anyhow::Result<String> {
        for code_snippet in code_snippets
            .iter()
            .filter(|code_snippet| !code_snippet.is_empty())
        {
            // Update the last conversation context with the code snippets which
            // we got here
            let last_exchange = self.get_last_conversation_message();
            last_exchange.add_code_spans(code_snippet.clone());
        }

        let response = code_snippets
            .iter()
            .filter(|c| !c.is_empty())
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("\n\n");

        // Now we want to also update the step of the exchange to highlight that
        // we did a search here
        let last_exchange = self.get_last_conversation_message();
        last_exchange.add_agent_step(super::types::AgentStep::Code {
            query: query.to_owned(),
            response: response.to_owned(),
            code_snippets: code_snippets
                .into_iter()
                .filter(|code_snippet| !code_snippet.is_empty())
                .collect(),
        });

        // Now that we have done the code search, we need to figure out what we
        // can do next with all the snippets, some ideas here include dedup and
        // also to join snippets together
        Ok(response)
    }

    pub async fn code_search_hybrid(&mut self, _query: &str) -> Result<Vec<CodeSpan>> {
        Ok(vec![])
    }

    /// This code search combines semantic + lexical + git log score
    /// to generate the code snippets which are the most relevant
    pub async fn code_search(&mut self, query: &str) -> Result<String> {
        let code_snippets = self.code_search_hybrid(query).await?;
        self.save_code_snippets_response(query, code_snippets)
    }

    pub async fn process_files(&mut self, _query: &str, _path_aliases: &[usize]) -> Result<String> {
        Ok("".to_owned())
        // const MAX_CHUNK_LINE_LENGTH: usize = 20;
        // const CHUNK_MERGE_DISTANCE: usize = 10;
        // const MAX_TOKENS: usize = 15400;

        // let paths = path_aliases
        //     .iter()
        //     .copied()
        //     .map(|i| self.paths().nth(i).ok_or(i).map(str::to_owned))
        //     .collect::<Result<Vec<_>, _>>()
        //     .map_err(|i| anyhow!("invalid path alias {i}"))?;

        // debug!(?query, ?paths, "processing file");

        // // Immutable reborrow of `self`, to copy freely to async closures.
        // let self_ = &*self;
        // let chunks = futures::stream::iter(paths.clone())
        //     .map(|path| async move {
        //         tracing::debug!(?path, "reading file");

        //         let lines = self_
        //             .get_file_content(&path)
        //             .await?
        //             .with_context(|| format!("path does not exist in the index: {path}"))?
        //             .lines()
        //             .enumerate()
        //             .map(|(i, line)| format!("{} {line}", i + 1))
        //             .collect::<Vec<_>>();

        //         let bpe = tiktoken_rs::get_bpe_from_model("gpt-3.5-turbo")?;

        //         let iter =
        //             tokio::task::spawn_blocking(|| trim_lines_by_tokens(lines, bpe, MAX_TOKENS))
        //                 .await
        //                 .context("failed to split by token")?;

        //         Result::<_>::Ok((iter, path.clone()))
        //     })
        //     // Buffer file loading to load multiple paths at once
        //     .buffered(10)
        //     .map(|result| async {
        //         let (lines, path) = result?;

        //         // The unwraps here should never fail, we generated this string above to always
        //         // have the same format.
        //         let start_line = lines[0]
        //             .split_once(' ')
        //             .unwrap()
        //             .0
        //             .parse::<usize>()
        //             .unwrap()
        //             - 1;

        //         // We store the lines separately, so that we can reference them later to trim
        //         // this snippet by line number.
        //         let contents = lines.join("\n");
        //         let prompt = prompts::file_explanation(query, &path, &contents);

        //         let json = self
        //             .get_llm_client()
        //             .response(
        //                 llm_funcs::llm::OpenAIModel::GPT3_5_16k,
        //                 vec![llm_funcs::llm::Message::system(&prompt)],
        //                 None,
        //                 0.0,
        //                 Some(0.2),
        //             )
        //             .await?;

        //         #[derive(
        //             serde::Deserialize,
        //             serde::Serialize,
        //             PartialEq,
        //             Eq,
        //             PartialOrd,
        //             Ord,
        //             Copy,
        //             Clone,
        //             Debug,
        //         )]
        //         struct Range {
        //             start: usize,
        //             end: usize,
        //         }

        //         #[derive(serde::Serialize)]
        //         struct RelevantChunk {
        //             #[serde(flatten)]
        //             range: Range,
        //             code: String,
        //         }

        //         let mut line_ranges: Vec<Range> = serde_json::from_str::<Vec<Range>>(&json)?
        //             .into_iter()
        //             .filter(|r| r.start > 0 && r.end > 0)
        //             .map(|mut r| {
        //                 r.end = r.end.min(r.start + MAX_CHUNK_LINE_LENGTH); // Cap relevant chunk size by line number
        //                 r
        //             })
        //             .map(|r| Range {
        //                 start: r.start - 1,
        //                 end: r.end,
        //             })
        //             .collect();

        //         line_ranges.sort();
        //         line_ranges.dedup();

        //         let relevant_chunks = line_ranges
        //             .into_iter()
        //             .fold(Vec::<Range>::new(), |mut exps, next| {
        //                 if let Some(prev) = exps.last_mut() {
        //                     if prev.end + CHUNK_MERGE_DISTANCE >= next.start {
        //                         prev.end = next.end;
        //                         return exps;
        //                     }
        //                 }

        //                 exps.push(next);
        //                 exps
        //             })
        //             .into_iter()
        //             .filter_map(|range| {
        //                 Some(RelevantChunk {
        //                     range,
        //                     code: lines
        //                         .get(
        //                             range.start.saturating_sub(start_line)
        //                                 ..=range.end.saturating_sub(start_line),
        //                         )?
        //                         .iter()
        //                         .map(|line| line.split_once(' ').unwrap().1)
        //                         .collect::<Vec<_>>()
        //                         .join("\n"),
        //                 })
        //             })
        //             .collect::<Vec<_>>();

        //         Ok::<_, anyhow::Error>((relevant_chunks, path))
        //     });

        // let processed = chunks
        //     .boxed()
        //     .buffered(5)
        //     .filter_map(|res| async { res.ok() })
        //     .collect::<Vec<_>>()
        //     .await;

        // let mut chunks = processed
        //     .into_iter()
        //     .flat_map(|(relevant_chunks, path)| {
        //         let alias = self.get_path_alias(&path);

        //         relevant_chunks.into_iter().map(move |c| {
        //             CodeSpan::new(
        //                 path.clone(),
        //                 alias,
        //                 c.range.start.try_into().unwrap(),
        //                 c.range.end.try_into().unwrap(),
        //                 c.code,
        //                 None,
        //             )
        //         })
        //     })
        //     .collect::<Vec<_>>();

        // chunks.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.start_line.cmp(&b.start_line)));

        // for chunk in chunks.iter().filter(|c| !c.is_empty()) {
        //     let last_conversation_message = self.get_last_conversation_message();
        //     last_conversation_message.add_code_spans(chunk.clone());
        // }

        // let response = chunks
        //     .iter()
        //     .filter(|c| !c.is_empty())
        //     .map(|c| c.to_string())
        //     .collect::<Vec<_>>()
        //     .join("\n\n");

        // let last_exchange = self.get_last_conversation_message();
        // last_exchange.add_agent_step(AgentStep::Proc {
        //     query: query.to_owned(),
        //     paths,
        //     response: response.to_owned(),
        // });

        // Ok(response)
    }

    pub async fn answer(
        &mut self,
        path_aliases: &[usize],
        sender: tokio::sync::mpsc::UnboundedSender<AgentAnswerStreamEvent>,
    ) -> Result<String> {
        if self.user_context.is_some() {
            let message = self
                .utter_history(Some(2))
                .map(|message| message.to_owned())
                .collect::<Vec<_>>();
            let _ = self
                .answer_context_using_user_data(message, sender.clone())
                .await;
        }
        dbg!("sidecar.generating_context.followup_question");
        let context = self.answer_context(path_aliases).await?;
        let system_prompt = match self.get_last_conversation_message_agent_state() {
            &AgentState::Explain => prompts::explain_article_prompt(
                path_aliases.len() != 1,
                &context,
                &self
                    .reporef()
                    .local_path()
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            // If we are in a followup chat, then we should always use the context
            // from the previous conversation and use that to answer the query
            &AgentState::FollowupChat => {
                let answer_prompt = prompts::followup_chat_prompt(
                    &context,
                    &self
                        .reporef()
                        .local_path()
                        .map(|path| path.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    // If we had more than 1 conversation then this gets counted
                    // as a followup
                    self.conversation_messages_len() > 1,
                    self.user_context.is_some(),
                    &self.project_labels,
                    self.system_instruction.as_ref().map(|s| s.as_str()),
                );
                answer_prompt
            }
            _ => prompts::answer_article_prompt(
                path_aliases.len() != 1,
                &context,
                &self
                    .reporef()
                    .local_path()
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        };
        let system_message = llm_funcs::llm::Message::system(&system_prompt);

        let answer_model = self.chat_broker.get_answer_model(self.slow_llm_model())?;
        let history = {
            let h = self.utter_history(None).collect::<Vec<_>>();
            let system_headroom = self.llm_tokenizer.count_tokens(
                self.slow_llm_model(),
                LLMTokenizerInput::Messages(vec![LLMClientMessage::system(
                    system_prompt.to_owned(),
                )]),
            )? as i64;
            let headroom = answer_model.answer_tokens + system_headroom;
            trim_utter_history(h, headroom, &answer_model, self.llm_tokenizer.clone())?
        };
        dbg!("sidecar.generating_answer.history_complete");
        let messages = Some(system_message)
            .into_iter()
            .chain(history.into_iter())
            .collect::<Vec<_>>();
        let messages_roles = messages
            .iter()
            .map(|message| message.role())
            .collect::<Vec<_>>();
        dbg!("sidecar.generating_answer.messages", &messages_roles);

        let provider_keys = self
            .provider_for_slow_llm()
            .ok_or(anyhow::anyhow!("no provider keys found for slow model"))?;
        let provider_config = self
            .provider_config_for_slow_model()
            .ok_or(anyhow::anyhow!("no provider config found for slow model"))?;

        let request = LLMClientCompletionRequest::new(
            self.slow_llm_model().clone(),
            messages
                .into_iter()
                .map(|message| (&message).try_into())
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?,
            0.1,
            None,
        )
        .set_max_tokens(
            answer_model
                .answer_tokens
                .try_into()
                .expect("i64 is positive"),
        )
        // fixing the message structure here is necessary for anthropic where we are
        // forced to have alternating human and assistant messages.
        .fix_message_structure();

        // dbg!("sidecar.generating_ansewr.fixed_roles", &fixed_roles);
        let (answer_sender, answer_receiver) = tokio::sync::mpsc::unbounded_channel();
        let answer_receiver = UnboundedReceiverStream::new(answer_receiver).map(either::Left);
        let llm_broker = self.llm_broker.clone();
        let reply = llm_broker
            .stream_completion(
                provider_keys.clone(),
                request,
                provider_config.clone(),
                vec![("event_type".to_owned(), "followup_question".to_owned())]
                    .into_iter()
                    .collect(),
                answer_sender,
            )
            .into_stream()
            .map(either::Right);

        let merged_stream = futures::stream::select(answer_receiver, reply);
        let mut final_answer = None;
        pin_mut!(merged_stream);
        while let Some(value) = merged_stream.next().await {
            match value {
                either::Left(llm_answer) => {
                    // we need to send the answer via the stream here
                    let _ = sender.send(AgentAnswerStreamEvent::LLMAnswer(llm_answer));
                }
                either::Right(reply) => {
                    final_answer = Some(reply);
                    break;
                }
            }
        }
        match final_answer {
            Some(Ok(reply)) => {
                let last_message = self.get_last_conversation_message();
                last_message.set_answer(reply.answer_up_until_now().to_owned());
                last_message.set_generated_answer_context(context);
                Ok(reply.answer_up_until_now().to_owned())
            }
            Some(Err(e)) => Err(e.into()),
            None => Err(anyhow::anyhow!("no answer from llm")),
        }
    }

    fn utter_history(
        &self,
        size: Option<usize>,
    ) -> impl Iterator<Item = llm_funcs::llm::Message> + '_ {
        const ANSWER_MAX_HISTORY_SIZE: usize = 10;

        self.conversation_messages
            .iter()
            .rev()
            .take(
                size.map(|size| std::cmp::min(ANSWER_MAX_HISTORY_SIZE, size))
                    .unwrap_or(ANSWER_MAX_HISTORY_SIZE),
            )
            .rev()
            .flat_map(|conversation_message| {
                let query = Some(llm_funcs::llm::Message::PlainText {
                    content: conversation_message.query().to_owned(),
                    role: llm_funcs::llm::Role::User,
                });

                let conclusion = conversation_message.answer().map(|answer| {
                    llm_funcs::llm::Message::PlainText {
                        role: llm_funcs::llm::Role::Assistant,
                        content: answer.answer_up_until_now.to_owned(),
                    }
                });

                query
                    .into_iter()
                    .chain(conclusion.into_iter())
                    .collect::<Vec<_>>()
            })
    }

    fn get_absolute_path(&self, reporef: &RepoRef, path: &str) -> String {
        let repo_location = reporef.local_path();
        match repo_location {
            Some(ref repo_location) => Path::new(&repo_location)
                .join(Path::new(path))
                .to_string_lossy()
                .to_string(),
            None => {
                // We don't have a repo location, so we just use the path
                path.to_string()
            }
        }
    }

    pub async fn followup_chat_context(&mut self) -> Result<Option<String>> {
        if self.conversation_messages.len() > 1 {
            // we want the last to last chat context here
            self.conversation_messages[self.conversation_messages_len() - 2]
                .get_generated_answer_context()
                .map(|context| Some(context.to_owned()))
                .ok_or(anyhow!("no previous chat"))
        } else {
            Ok(None)
        }
    }

    async fn answer_context(&mut self, aliases: &[usize]) -> Result<String> {
        // Here we create the context for the answer, using the aliases and also
        // using the code spans which we have
        // We change the paths here to be absolute so the LLM can stream that
        // properly
        // Here we might be in a weird position that we have to do followup-chats
        // so for that the answer context is totally different and we set it as such
        let mut prompt = "".to_owned();

        let paths = self.paths().collect::<Vec<_>>();
        let mut aliases = aliases
            .iter()
            .copied()
            .filter(|alias| *alias < paths.len())
            .collect::<Vec<_>>();

        aliases.sort();
        aliases.dedup();

        if !aliases.is_empty() {
            prompt += "##### PATHS #####\n";

            for alias in &aliases {
                let path = &paths[*alias];
                // Now we try to get the absolute path here
                let path_for_prompt = self.get_absolute_path(self.reporef(), path);
                prompt += &format!("{path_for_prompt}\n");
            }
        }

        let code_spans = self.dedup_code_spans(aliases.as_slice()).await?;

        // Sometimes, there are just too many code chunks in the context, and deduplication still
        // doesn't trim enough chunks. So, we enforce a hard limit here that stops adding tokens
        // early if we reach a heuristic limit.
        let slow_model = self.slow_llm_model();
        let answer_model = self.chat_broker.get_answer_model(slow_model)?;
        let prompt_tokens_used: i64 =
            self.llm_tokenizer
                .count_tokens_using_tokenizer(slow_model, &prompt)? as i64;
        let mut remaining_prompt_tokens: i64 = answer_model.total_tokens - prompt_tokens_used;

        // we have to show the selected snippets which the user has selected
        // we have to show the selected snippets to the prompt as well
        let extended_user_selected_context = self.get_extended_user_selection_information();
        if let Some(extended_user_selection_context_slice) = extended_user_selected_context {
            let user_selected_context_header = "#### USER SELECTED CONTEXT ####\n";
            let user_selected_context_tokens: i64 = self
                .llm_tokenizer
                .count_tokens_using_tokenizer(slow_model, user_selected_context_header)?
                as i64;
            if user_selected_context_tokens + answer_model.prompt_tokens_limit
                >= remaining_prompt_tokens
            {
                dbg!("skipping_adding_cause_of_context_length_limit");
                info!("we can't set user selected context because of prompt limit");
            } else {
                prompt += user_selected_context_header;
                remaining_prompt_tokens -= user_selected_context_tokens;

                for extended_user_selected_context in
                    extended_user_selection_context_slice.iter().rev()
                {
                    let variable_prompt = extended_user_selected_context.to_prompt();
                    let user_variable_tokens = self
                        .llm_tokenizer
                        .count_tokens_using_tokenizer(slow_model, &variable_prompt)?
                        as i64;
                    if user_variable_tokens + answer_model.prompt_tokens_limit
                        > remaining_prompt_tokens
                    {
                        info!("breaking at {} tokens", remaining_prompt_tokens);
                        break;
                    }
                    prompt += &variable_prompt;
                    remaining_prompt_tokens -= user_variable_tokens;
                }
            }
        }

        // Select as many recent chunks as possible
        let mut recent_chunks = Vec::new();
        for code_span in code_spans.iter().rev() {
            let snippet = code_span
                .data
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{} {line}\n", i + code_span.start_line as usize + 1))
                .collect::<String>();

            let formatted_snippet = format!(
                "### {} ###\n{snippet}\n\n",
                self.get_absolute_path(self.reporef(), &code_span.file_path)
            );

            let snippet_tokens: i64 = self
                .llm_tokenizer
                .count_tokens_using_tokenizer(slow_model, &formatted_snippet)?
                as i64;

            if snippet_tokens >= remaining_prompt_tokens {
                dbg!("skipping_code_span_addition", snippet_tokens);
                info!("breaking at {} tokens", remaining_prompt_tokens);
                break;
            }

            recent_chunks.push((code_span.clone(), formatted_snippet));

            remaining_prompt_tokens -= snippet_tokens;
            debug!("{}", remaining_prompt_tokens);
        }

        // group recent chunks by path alias
        let mut recent_chunks_by_alias: HashMap<_, _> =
            recent_chunks
                .into_iter()
                .fold(HashMap::new(), |mut map, item| {
                    map.entry(item.0.alias).or_insert_with(Vec::new).push(item);
                    map
                });

        // write the header if we have atleast one chunk
        if !recent_chunks_by_alias.values().all(Vec::is_empty) {
            prompt += "\n##### CODE CHUNKS #####\n\n";
        }

        // sort by alias, then sort by lines
        let mut aliases = recent_chunks_by_alias.keys().copied().collect::<Vec<_>>();
        aliases.sort();

        for alias in aliases {
            let chunks = recent_chunks_by_alias.get_mut(&alias).unwrap();
            chunks.sort_by(|a, b| a.0.start_line.cmp(&b.0.start_line));
            for (_, formatted_snippet) in chunks {
                prompt += formatted_snippet;
            }
        }

        Ok(prompt)
    }

    async fn dedup_code_spans(&mut self, aliases: &[usize]) -> anyhow::Result<Vec<CodeSpan>> {
        // Note: The end line number here is *not* inclusive.
        let mut spans_by_path = HashMap::<_, Vec<_>>::new();
        for code_span in self
            .code_spans()
            .into_iter()
            .filter(|code_span| aliases.contains(&code_span.alias))
        {
            spans_by_path
                .entry(code_span.file_path.clone())
                .or_default()
                .push(code_span.start_line..code_span.end_line);
        }

        // debug!(?spans_by_path, "expanding code spans");

        let self_ = &*self;
        // Map of path -> line list
        let lines_by_file = futures::stream::iter(&mut spans_by_path)
            .then(|(path, spans)| async move {
                spans.sort_by_key(|c| c.start);
                dbg!("path_for_answer", &path);

                let lines = self_
                    .get_file_content(path)
                    .await
                    .unwrap()
                    .unwrap_or_else(|| panic!("path did not exist in the index: {path}"))
                    .split("\n")
                    .map(str::to_owned)
                    .collect::<Vec<_>>();

                (path.clone(), lines)
            })
            .collect::<HashMap<_, _>>()
            .await;

        debug!(
            event_name = "selected_spans",
            spans_by_path = ?spans_by_path,
        );

        Ok(spans_by_path
            .into_iter()
            .flat_map(|(path, spans)| spans.into_iter().map(move |s| (path.clone(), s)))
            .map(|(path, span)| {
                let line_start = span.start as usize;
                let mut line_end = span.end as usize;
                if line_end >= lines_by_file.get(&path).unwrap().len() {
                    warn!(
                        "line end is greater than the number of lines in the file {}",
                        path
                    );
                    line_end = lines_by_file.get(&path).unwrap().len() - 1;
                }
                let snippet = lines_by_file.get(&path).unwrap()[line_start..line_end].join("\n");

                let path_alias = self.get_path_alias(&path);
                CodeSpan::new(path, path_alias, span.start, span.end, snippet, None)
            })
            .collect())
    }
}

fn trim_utter_history(
    mut history: Vec<llm_funcs::llm::Message>,
    headroom: i64,
    answer_model: &AnswerModel,
    llm_tokenizer: Arc<LLMTokenizer>,
) -> Result<Vec<llm_funcs::llm::Message>> {
    let model = &answer_model.llm_type;
    let context_length = answer_model.total_tokens;
    let mut llm_messages: Vec<LLMClientMessage> = history
        .iter()
        .map(|m| m.try_into())
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    // remove the earliest messages, one by one, until we can accommodate into prompt
    // what we are getting here is basically context_length - tokens < headroom
    // written another way we can make this look like: context_length < headroom + tokens
    while context_length
        < (headroom as usize
            + llm_tokenizer
                .count_tokens(model, LLMTokenizerInput::Messages(llm_messages.to_vec()))?)
        .try_into()
        .unwrap()
    {
        if !llm_messages.is_empty() {
            llm_messages.remove(0);
            history.remove(0);
        } else {
            return Err(anyhow!("could not find message to trim"));
        }
    }

    Ok(history)
}
