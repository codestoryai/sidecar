use crate::{
    agent::{
        llm_funcs::{
            self,
            llm::{self, Message},
        },
        prompts,
        types::{AgentStep, CodeSpan, VariableInformation},
    },
    application::application::Application,
    db::sqlite::SqlDb,
    git::commit_statistics::GitLogScore,
    repo::types::RepoRef,
    webserver::agent::UserContext,
};

/// Here we allow the agent to perform search and answer workflow related questions
/// we will later use this for code planning and also code editing
use super::{
    llm_funcs::LlmClient,
    model,
    types::{Agent, AgentState, Answer, ConversationMessage},
};

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use futures::StreamExt;
use once_cell::sync::OnceCell;
use rake::StopWords;
use tiktoken_rs::CoreBPE;
use tokio::sync::mpsc::Sender;
use tracing::{debug, info};

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::Path,
    sync::Arc,
};

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

const PATH_LIMIT: u64 = 30;
const PATH_LIMIT_USIZE: usize = 30;

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
        llm_client: Arc<LlmClient>,
        conversation_id: uuid::Uuid,
        sql_db: SqlDb,
        mut previous_conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
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
            llm_client,
            model: model::GPT_4,
            sql_db,
            sender,
            user_context: None,
        };
        agent
    }

    pub fn prepare_for_followup(
        application: Application,
        reporef: RepoRef,
        session_id: uuid::Uuid,
        llm_client: Arc<LlmClient>,
        sql_db: SqlDb,
        conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
        user_context: UserContext,
    ) -> Self {
        let agent = Agent {
            application,
            reporef,
            session_id,
            conversation_messages: conversations,
            llm_client,
            model: model::GPT_4,
            sql_db,
            sender,
            user_context: Some(user_context),
        };
        agent
    }

    pub fn prepare_for_semantic_search(
        application: Application,
        reporef: RepoRef,
        session_id: uuid::Uuid,
        query: &str,
        llm_client: Arc<LlmClient>,
        conversation_id: uuid::Uuid,
        sql_db: SqlDb,
        mut previous_conversations: Vec<ConversationMessage>,
        sender: Sender<ConversationMessage>,
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
            llm_client,
            model: model::GPT_4,
            sql_db,
            sender,
            user_context: None,
        };
        agent
    }

    pub async fn path_search(&mut self, query: &str) -> Result<String> {
        // Here we first take the user query and perform a lexical search
        // on all the paths which are present
        let mut path_matches = self
            .application
            .indexes
            .file
            .fuzzy_path_match(self.reporef(), query, PATH_LIMIT_USIZE)
            .await
            .map(|c| c.relative_path)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        // Now we try semantic search on the same query
        if self.application.semantic_client.is_some() && path_matches.is_empty() {
            path_matches = self
                .application
                .semantic_client
                .as_ref()
                .expect("is_some to hold above")
                .search(query, self.reporef(), PATH_LIMIT, 0, 0.0, true)
                .await?
                .into_iter()
                .map(|payload| payload.relative_path)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
        }

        // This also updates the path in the last exchange which has happened
        // with the agent
        let mut paths = path_matches
            .iter()
            .map(|p| (self.get_path_alias(p), p.to_string()))
            .collect::<Vec<_>>();
        paths.sort_by(|a: &(usize, String), b| a.0.cmp(&b.0));

        let response = paths
            .iter()
            .map(|(alias, path)| format!("{}: {}", alias, path))
            .collect::<Vec<_>>()
            .join("\n");

        // Now we want to update the path in agent
        let last_exchange = self.get_last_conversation_message();
        last_exchange.add_agent_step(super::types::AgentStep::Path {
            query: query.to_owned(),
            response: response.to_owned(),
            paths: paths
                .into_iter()
                .map(|path_with_alias| path_with_alias.1)
                .collect(),
        });

        Ok(response)
    }

    pub fn update_user_selected_variables(&mut self, user_variables: Vec<VariableInformation>) {
        let last_exchange = self.get_last_conversation_message();
        user_variables.into_iter().for_each(|user_variable| {
            last_exchange.add_user_variable(user_variable);
        })
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

    pub async fn code_search_hybrid(&mut self, query: &str) -> Result<Vec<CodeSpan>> {
        const CODE_SEARCH_LIMIT: u64 = 10;
        if self.application.semantic_client.is_none() {
            return Err(anyhow::anyhow!("no semantic client defined"));
        }
        let mut results_semantic = self
            .application
            .semantic_client
            .as_ref()
            .expect("is_none to hold")
            .search(query, self.reporef(), CODE_SEARCH_LIMIT, 0, 0.0, true)
            .await?;
        let hyde_snippets = self.hyde(query).await?;
        if !hyde_snippets.is_empty() {
            let hyde_snippets = hyde_snippets.first().unwrap();
            let hyde_search = self
                .application
                .semantic_client
                .as_ref()
                .expect("is_none to hold")
                .search(
                    hyde_snippets,
                    self.reporef(),
                    CODE_SEARCH_LIMIT,
                    0,
                    0.3,
                    true,
                )
                .await?;
            results_semantic.extend(hyde_search);
        }

        // Now we do a lexical search as well this is to help figure out which
        // snippets are relevant
        let lexical_search_code_snippets = self
            .application
            .indexes
            .code_snippet
            .lexical_search(
                self.reporef(),
                query,
                CODE_SEARCH_LIMIT
                    .try_into()
                    .expect("conversion to not fail"),
            )
            .await
            .unwrap_or(vec![]);

        // Now we get the statistics from the git log and use that for scoring
        // as well
        let git_log_score =
            GitLogScore::generate_git_log_score(self.reporef.clone(), self.application.sql.clone())
                .await;

        let mut code_snippets_semantic = results_semantic
            .into_iter()
            .map(|result| {
                let path_alias = self.get_path_alias(&result.relative_path);
                // convert it to a code snippet here
                let code_span = CodeSpan::new(
                    result.relative_path,
                    path_alias,
                    result.start_line,
                    result.end_line,
                    result.text,
                    result.score,
                );
                code_span
            })
            .collect::<Vec<_>>();

        let code_snippets_lexical_score: HashMap<String, (f32, CodeSpan)> =
            lexical_search_code_snippets
                .into_iter()
                .map(|lexical_code_snippet| {
                    let path_alias = self.get_path_alias(&lexical_code_snippet.relative_path);
                    // convert it to a code snippet here
                    let code_span = CodeSpan::new(
                        lexical_code_snippet.relative_path,
                        path_alias,
                        lexical_code_snippet.line_start,
                        lexical_code_snippet.line_end,
                        lexical_code_snippet.content,
                        Some(lexical_code_snippet.score),
                    );
                    (
                        code_span.get_unique_key(),
                        (lexical_code_snippet.score, code_span),
                    )
                })
                .collect();

        // Now that we have the git log score, lets use that to score the results
        // Lets first get the lexical scores for the code snippets which we are getting from the search
        code_snippets_semantic = code_snippets_semantic
            .into_iter()
            .map(|mut code_snippet| {
                let unique_key = code_snippet.get_unique_key();
                // If we don't get anything here we just return 0.3
                let lexical_score = code_snippets_lexical_score
                    .get(&unique_key)
                    .map(|v| &v.0)
                    .unwrap_or(&0.3);
                let git_log_score = git_log_score.get_score_for_file(&code_snippet.file_path);
                if let Some(semantic_score) = code_snippet.score {
                    code_snippet.score = Some(semantic_score + 2.5 * lexical_score + git_log_score);
                } else {
                    code_snippet.score = Some(2.5 * lexical_score + git_log_score);
                }
                code_snippet
            })
            .collect::<Vec<_>>();

        // We should always include the results from the lexical search, since
        // we have hits for the keywords so they are worth a lot of points
        let code_snippet_semantic_keys: HashSet<String> = code_snippets_semantic
            .iter()
            .map(|c| c.get_unique_key())
            .collect();
        // Now check with the lexical set which are not included in the result
        // and add them
        code_snippets_lexical_score
            .into_iter()
            .for_each(|(_, mut code_snippet_with_score)| {
                // if we don't have it, it makes sense to add the results here and give
                // it a semantic score of 0.3 or something (which is our threshold)
                let unique_key_for_code_snippet = code_snippet_with_score.1.get_unique_key();
                if !code_snippet_semantic_keys.contains(&unique_key_for_code_snippet) {
                    let git_log_score =
                        git_log_score.get_score_for_file(&code_snippet_with_score.1.file_path);
                    code_snippet_with_score.1.score =
                        Some(0.3 + git_log_score + 2.5 * code_snippet_with_score.0);
                    code_snippets_semantic.push(code_snippet_with_score.1);
                }
            });
        code_snippets_semantic.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        Ok(code_snippets_semantic
            .into_iter()
            .take(
                (CODE_SEARCH_LIMIT * 2)
                    .try_into()
                    .expect("20u64 to usize should not fail"),
            )
            .collect())
    }

    /// This code search combines semantic + lexical + git log score
    /// to generate the code snippets which are the most relevant
    pub async fn code_search(&mut self, query: &str) -> Result<String> {
        let code_snippets = self.code_search_hybrid(query).await?;
        self.save_code_snippets_response(query, code_snippets)
    }

    /// This just uses the semantic search and nothing else
    pub async fn code_search_pure_semantic(&mut self, query: &str) -> Result<String> {
        const CODE_SEARCH_LIMIT: u64 = 10;
        // If we don't have semantic client we skip this one
        if self.application.semantic_client.is_none() {
            return Ok("".to_string());
        }
        let mut results = self
            .application
            .semantic_client
            .as_ref()
            .expect("is_none to hold")
            .search(query, self.reporef(), CODE_SEARCH_LIMIT, 0, 0.0, true)
            .await?;
        let hyde_snippets = self.hyde(query).await?;
        if !hyde_snippets.is_empty() {
            let hyde_snippet = hyde_snippets.first().unwrap();
            let hyde_search = self
                .application
                .semantic_client
                .as_ref()
                .expect("is_none to hold")
                .search(
                    hyde_snippet,
                    self.reporef(),
                    CODE_SEARCH_LIMIT,
                    0,
                    0.3,
                    true,
                )
                .await?;
            results.extend(hyde_search);
        }

        let mut code_snippets = results
            .into_iter()
            .map(|result| {
                let path_alias = self.get_path_alias(&result.relative_path);
                // convert it to a code snippet here
                let code_span = CodeSpan::new(
                    result.relative_path,
                    path_alias,
                    result.start_line,
                    result.end_line,
                    result.text,
                    result.score,
                );
                code_span
            })
            .collect::<Vec<_>>();

        code_snippets.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.start_line.cmp(&b.start_line)));

        self.save_code_snippets_response(query, code_snippets)
    }

    async fn hyde(&self, query: &str) -> Result<Vec<String>> {
        let prompt = vec![Message::system(&prompts::hypothetical_document_prompt(
            query,
        ))];

        let response = self
            .get_llm_client()
            .response(llm::OpenAIModel::GPT3_5_16k, prompt, None, 0.0, None)
            .await?;

        debug!("hyde response");

        let documents = prompts::try_parse_hypothetical_documents(&response);

        for doc in documents.iter() {
            info!(?doc, "hyde generated snippet");
        }

        Ok(documents)
    }

    pub async fn process_files(&mut self, query: &str, path_aliases: &[usize]) -> Result<String> {
        const MAX_CHUNK_LINE_LENGTH: usize = 20;
        const CHUNK_MERGE_DISTANCE: usize = 10;
        const MAX_TOKENS: usize = 15400;

        let paths = path_aliases
            .iter()
            .copied()
            .map(|i| self.paths().nth(i).ok_or(i).map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|i| anyhow!("invalid path alias {i}"))?;

        debug!(?query, ?paths, "processing file");

        // Immutable reborrow of `self`, to copy freely to async closures.
        let self_ = &*self;
        let chunks = futures::stream::iter(paths.clone())
            .map(|path| async move {
                tracing::debug!(?path, "reading file");

                let lines = self_
                    .get_file_content(&path)
                    .await?
                    .with_context(|| format!("path does not exist in the index: {path}"))?
                    .lines()
                    .enumerate()
                    .map(|(i, line)| format!("{} {line}", i + 1))
                    .collect::<Vec<_>>();

                let bpe = tiktoken_rs::get_bpe_from_model("gpt-3.5-turbo")?;

                let iter =
                    tokio::task::spawn_blocking(|| trim_lines_by_tokens(lines, bpe, MAX_TOKENS))
                        .await
                        .context("failed to split by token")?;

                Result::<_>::Ok((iter, path.clone()))
            })
            // Buffer file loading to load multiple paths at once
            .buffered(10)
            .map(|result| async {
                let (lines, path) = result?;

                // The unwraps here should never fail, we generated this string above to always
                // have the same format.
                let start_line = lines[0]
                    .split_once(' ')
                    .unwrap()
                    .0
                    .parse::<usize>()
                    .unwrap()
                    - 1;

                // We store the lines separately, so that we can reference them later to trim
                // this snippet by line number.
                let contents = lines.join("\n");
                let prompt = prompts::file_explanation(query, &path, &contents);

                let json = self
                    .get_llm_client()
                    .response(
                        llm_funcs::llm::OpenAIModel::GPT3_5_16k,
                        vec![llm_funcs::llm::Message::system(&prompt)],
                        None,
                        0.0,
                        Some(0.2),
                    )
                    .await?;

                #[derive(
                    serde::Deserialize,
                    serde::Serialize,
                    PartialEq,
                    Eq,
                    PartialOrd,
                    Ord,
                    Copy,
                    Clone,
                    Debug,
                )]
                struct Range {
                    start: usize,
                    end: usize,
                }

                #[derive(serde::Serialize)]
                struct RelevantChunk {
                    #[serde(flatten)]
                    range: Range,
                    code: String,
                }

                let mut line_ranges: Vec<Range> = serde_json::from_str::<Vec<Range>>(&json)?
                    .into_iter()
                    .filter(|r| r.start > 0 && r.end > 0)
                    .map(|mut r| {
                        r.end = r.end.min(r.start + MAX_CHUNK_LINE_LENGTH); // Cap relevant chunk size by line number
                        r
                    })
                    .map(|r| Range {
                        start: r.start - 1,
                        end: r.end,
                    })
                    .collect();

                line_ranges.sort();
                line_ranges.dedup();

                let relevant_chunks = line_ranges
                    .into_iter()
                    .fold(Vec::<Range>::new(), |mut exps, next| {
                        if let Some(prev) = exps.last_mut() {
                            if prev.end + CHUNK_MERGE_DISTANCE >= next.start {
                                prev.end = next.end;
                                return exps;
                            }
                        }

                        exps.push(next);
                        exps
                    })
                    .into_iter()
                    .filter_map(|range| {
                        Some(RelevantChunk {
                            range,
                            code: lines
                                .get(
                                    range.start.saturating_sub(start_line)
                                        ..=range.end.saturating_sub(start_line),
                                )?
                                .iter()
                                .map(|line| line.split_once(' ').unwrap().1)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        })
                    })
                    .collect::<Vec<_>>();

                Ok::<_, anyhow::Error>((relevant_chunks, path))
            });

        let processed = chunks
            .boxed()
            .buffered(5)
            .filter_map(|res| async { res.ok() })
            .collect::<Vec<_>>()
            .await;

        let mut chunks = processed
            .into_iter()
            .flat_map(|(relevant_chunks, path)| {
                let alias = self.get_path_alias(&path);

                relevant_chunks.into_iter().map(move |c| {
                    CodeSpan::new(
                        path.clone(),
                        alias,
                        c.range.start.try_into().unwrap(),
                        c.range.end.try_into().unwrap(),
                        c.code,
                        None,
                    )
                })
            })
            .collect::<Vec<_>>();

        chunks.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.start_line.cmp(&b.start_line)));

        for chunk in chunks.iter().filter(|c| !c.is_empty()) {
            let last_conversation_message = self.get_last_conversation_message();
            last_conversation_message.add_code_spans(chunk.clone());
        }

        let response = chunks
            .iter()
            .filter(|c| !c.is_empty())
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("\n\n");

        let last_exchange = self.get_last_conversation_message();
        last_exchange.add_agent_step(AgentStep::Proc {
            query: query.to_owned(),
            paths,
            response: response.to_owned(),
        });

        Ok(response)
    }

    pub async fn answer(
        &mut self,
        path_aliases: &[usize],
        sender: tokio::sync::mpsc::UnboundedSender<Answer>,
    ) -> Result<String> {
        if self.user_context.is_some() {
            let message = self
                .utter_history(Some(2))
                .map(|message| message.to_owned())
                .collect::<Vec<_>>();
            let _ = self.truncate_user_context(message).await;
        }
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
        let history = {
            let h = self.utter_history(None).collect::<Vec<_>>();
            let system_headroom = tiktoken_rs::num_tokens_from_messages(
                self.model.tokenizer,
                &[(&system_message).into()],
            )?;
            let headroom = self.model.answer_tokens + system_headroom;
            trim_utter_history(h, headroom, &self.model)?
        };
        let messages = Some(system_message)
            .into_iter()
            .chain(history.into_iter())
            .collect::<Vec<_>>();

        let reply = self
            .llm_client
            .clone()
            .stream_response(
                llm::OpenAIModel::get_model(self.model.model_name)?,
                messages,
                None,
                0.1,
                None,
                sender,
            )
            .await?;
        dbg!(&reply);

        let last_message = self.get_last_conversation_message();
        last_message.set_answer(reply.to_owned());
        last_message.set_generated_answer_context(context);

        Ok(reply)
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

        let code_spans = self.dedup_code_spans(aliases.as_slice()).await;

        // Sometimes, there are just too many code chunks in the context, and deduplication still
        // doesn't trim enough chunks. So, we enforce a hard limit here that stops adding tokens
        // early if we reach a heuristic limit.
        let bpe = tiktoken_rs::get_bpe_from_model(self.model.tokenizer)?;
        let mut remaining_prompt_tokens =
            tiktoken_rs::get_completion_max_tokens(self.model.tokenizer, &prompt)?;

        // we have to show the selected snippets which the user has selected
        let user_variables = self.get_user_variables();
        if let Some(user_variables_slice) = user_variables {
            let user_selected_context = "#### USER SELECTED CONTEXT ####\n";
            let user_selected_context_tokens = bpe.encode_ordinary(&user_selected_context).len();
            if user_selected_context_tokens
                >= remaining_prompt_tokens - self.model.prompt_tokens_limit
            {
                info!("we can't set user selected context because of prompt limit");
            } else {
                prompt += "#### USER SELECTED CONTEXT ####\n";
                remaining_prompt_tokens -= user_selected_context_tokens;

                for user_variable in user_variables_slice.iter() {
                    let variable_prompt = user_variable.to_prompt();
                    let user_variable_tokens = bpe.encode_ordinary(&variable_prompt).len();
                    if user_variable_tokens
                        > remaining_prompt_tokens - self.model.prompt_tokens_limit
                    {
                        info!("breaking at {} tokens", remaining_prompt_tokens);
                        break;
                    }
                    prompt += &variable_prompt;
                    remaining_prompt_tokens -= user_variable_tokens;
                }
            }
        }

        // If we have definition snippets we also add them to the mix here
        let definition_snippets = self.get_definition_snippets();
        if let Some(definition_snippets_slice) = definition_snippets {
            let definition_snippets_context = "#### DEFINITION SNIPPETS ####\n";
            let definition_snippets_context_tokens =
                bpe.encode_ordinary(&definition_snippets_context).len();
            if definition_snippets_context_tokens
                >= remaining_prompt_tokens - self.model.prompt_tokens_limit
            {
                info!("we can't set definition snippets because of prompt limit");
            } else {
                prompt += "#### DEFINITION SNIPPETS ####\n";
                remaining_prompt_tokens -= definition_snippets_context_tokens;

                for definition_snippet_context in definition_snippets_slice.iter() {
                    let definition_snippet_tokens =
                        bpe.encode_ordinary(&definition_snippet_context).len();
                    if definition_snippet_tokens
                        > remaining_prompt_tokens - self.model.prompt_tokens_limit
                    {
                        info!("breaking at {} tokens", remaining_prompt_tokens);
                        break;
                    }
                    prompt += &definition_snippet_context;
                    remaining_prompt_tokens -= definition_snippet_tokens;
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

            let snippet_tokens = bpe.encode_ordinary(&formatted_snippet).len();

            if snippet_tokens >= remaining_prompt_tokens - self.model.prompt_tokens_limit {
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

    async fn dedup_code_spans(&mut self, aliases: &[usize]) -> Vec<CodeSpan> {
        /// The ratio of code tokens to context size.
        ///
        /// Making this closure to 1 means that more of the context is taken up by source code.
        const CONTEXT_CODE_RATIO: f32 = 0.5;

        let bpe = tiktoken_rs::get_bpe_from_model(self.model.tokenizer).unwrap();
        let context_size = tiktoken_rs::model::get_context_size(self.model.tokenizer);
        let max_tokens = (context_size as f32 * CONTEXT_CODE_RATIO) as usize;

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

        debug!(?spans_by_path, "expanding code spans");

        let self_ = &*self;
        // Map of path -> line list
        let lines_by_file = futures::stream::iter(&mut spans_by_path)
            .then(|(path, spans)| async move {
                spans.sort_by_key(|c| c.start);

                let lines = self_
                    .get_file_content(path)
                    .await
                    .unwrap()
                    .unwrap_or_else(|| panic!("path did not exist in the index: {path}"))
                    // we should be using .lines here instead, but this is okay
                    // for now
                    .split("\n")
                    .map(str::to_owned)
                    .collect::<Vec<_>>();

                (path.clone(), lines)
            })
            .collect::<HashMap<_, _>>()
            .await;

        // Total number of lines to try and expand by, per loop iteration.
        const TOTAL_LINE_INC: usize = 100;

        // We keep track of whether any spans were changed below, so that we know when to break
        // out of this loop.
        let mut changed = true;

        while !spans_by_path.is_empty() && changed {
            changed = false;

            let tokens = spans_by_path
                .iter()
                .flat_map(|(path, spans)| spans.iter().map(move |s| (path, s)))
                .map(|(path, span)| {
                    let line_start = span.start as usize;
                    let line_end = span.end as usize;
                    let range = line_start..line_end;
                    let snippet = lines_by_file.get(path).unwrap()[range].join("\n");
                    bpe.encode_ordinary(&snippet).len()
                })
                .sum::<usize>();

            // First, we grow the spans if possible.
            if tokens < max_tokens {
                // NB: We divide TOTAL_LINE_INC by 2, because we expand in 2 directions.
                let range_step = (TOTAL_LINE_INC / 2)
                    / spans_by_path
                        .values()
                        .map(|spans| spans.len())
                        .sum::<usize>()
                        .max(1);

                let range_step = range_step.max(1);

                for (path, span) in spans_by_path
                    .iter_mut()
                    .flat_map(|(path, spans)| spans.iter_mut().map(move |s| (path, s)))
                {
                    let file_lines = lines_by_file.get(path.as_str()).unwrap().len();

                    let old_span = span.clone();

                    span.start = span.start.saturating_sub(range_step as u64);

                    // Expand the end line forwards, capping at the total number of lines.
                    span.end += range_step as u64;
                    span.end = span.end.min(file_lines as u64);

                    if *span != old_span {
                        changed = true;
                    }
                }
            }

            // Next, we merge any overlapping spans.
            for spans in spans_by_path.values_mut() {
                *spans = std::mem::take(spans)
                    .into_iter()
                    .fold(Vec::new(), |mut a, next| {
                        // There is some rightward drift here, which could be fixed once if-let
                        // chains are stabilized.
                        if let Some(prev) = a.last_mut() {
                            if let Some(next) = merge_overlapping(prev, next) {
                                a.push(next);
                            } else {
                                changed = true;
                            }
                        } else {
                            a.push(next);
                        }

                        a
                    });
            }
        }

        debug!(?spans_by_path, "expanded spans");

        spans_by_path
            .into_iter()
            .flat_map(|(path, spans)| spans.into_iter().map(move |s| (path.clone(), s)))
            .map(|(path, span)| {
                let line_start = span.start as usize;
                let line_end = span.end as usize;
                let snippet = lines_by_file.get(&path).unwrap()[line_start..line_end].join("\n");

                let path_alias = self.get_path_alias(&path);
                CodeSpan::new(path, path_alias, span.start, span.end, snippet, None)
            })
            .collect()
    }
}

fn trim_lines_by_tokens(lines: Vec<String>, bpe: CoreBPE, max_tokens: usize) -> Vec<String> {
    let line_tokens = lines
        .iter()
        .map(|line| bpe.encode_ordinary(line).len())
        .collect::<Vec<_>>();

    let mut trimmed_lines = Vec::new();

    // Push lines to `trimmed_lines` until we reach the maximum number of tokens.
    let mut i = 0usize;
    let mut tokens = 0usize;
    while i < lines.len() && tokens < max_tokens {
        tokens += line_tokens[i];
        trimmed_lines.push(lines[i].clone());
        i += 1;
    }

    trimmed_lines
}

fn merge_overlapping(a: &mut Range<u64>, b: Range<u64>) -> Option<Range<u64>> {
    if a.end >= b.start {
        // `b` might be contained in `a`, which allows us to discard it.
        if a.end < b.end {
            a.end = b.end;
        }

        None
    } else {
        Some(b)
    }
}

fn trim_utter_history(
    mut history: Vec<llm_funcs::llm::Message>,
    headroom: usize,
    model: &model::AnswerModel,
) -> Result<Vec<llm_funcs::llm::Message>> {
    let mut tiktoken_msgs: Vec<tiktoken_rs::ChatCompletionRequestMessage> =
        history.iter().map(|m| m.into()).collect::<Vec<_>>();

    // remove the earliest messages, one by one, until we can accommodate into prompt
    while tiktoken_rs::get_chat_completion_max_tokens(model.tokenizer, &tiktoken_msgs)? < headroom {
        if !tiktoken_msgs.is_empty() {
            tiktoken_msgs.remove(0);
            history.remove(0);
        } else {
            return Err(anyhow!("could not find message to trim"));
        }
    }

    Ok(history)
}
