use llm_client::clients::types::{LLMClientError, LLMType};
use serde_xml_rs::to_string;
use tokio::sync::mpsc::error::SendError;

use std::time::Instant;
use std::{path::PathBuf, time::Duration};
use thiserror::Error;

use crate::agentic::symbol::ui_event::UIEventWithID;
use crate::{
    agentic::{
        symbol::events::message_event::SymbolEventMessageProperties,
        tool::{
            code_symbol::important::{
                CodeSymbolImportantResponse, CodeSymbolWithSteps, CodeSymbolWithThinking,
            },
            file::types::SerdeError,
            human::{error::CommunicationError, qa::GenerateHumanQuestionResponse},
        },
    },
    repomap::types::RepoMap,
    user_context::types::UserContextError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    big_search::IterativeSearchSeed, decide::DecideResponse, google_studio::GoogleStudioLLM,
    identify::IdentifyResponse, relevant_files::QueryRelevantFilesResponse, repository::Repository,
};

#[derive(Clone, Debug, Serialize)]
pub enum IterativeSearchEvent {
    SearchStarted,
    SeedApplied(Duration),
    SearchQueriesGenerated(Vec<SearchQuery>, Duration),
    SearchExecuted(Vec<SearchResult>, Duration),
    IdentificationCompleted(IdentifyResponse, Duration),
    FileOutlineGenerated(Duration),
    DecisionMade(DecideResponse, Duration),
    LoopCompleted(usize, Duration),
    SearchCompleted(Duration),
}

pub trait IterativeSearchEventHandler: Send + Sync {
    fn handle_event(&self, event: IterativeSearchEvent) -> Result<(), IterativeSearchError>;
}

pub struct UIEventHandler {
    message_properties: SymbolEventMessageProperties,
}

impl UIEventHandler {
    pub fn new(message_properties: SymbolEventMessageProperties) -> Self {
        Self { message_properties }
    }

    pub fn message_properties(&self) -> &SymbolEventMessageProperties {
        &self.message_properties
    }
}

impl IterativeSearchEventHandler for UIEventHandler {
    fn handle_event(&self, event: IterativeSearchEvent) -> Result<(), IterativeSearchError> {
        match event {
            _ => Ok(self.message_properties().ui_sender().send(
                UIEventWithID::search_iteration_event(
                    self.message_properties()
                        .request_id()
                        .root_request_id() // todo(zi) or should this just be request id?
                        .to_owned(),
                    event,
                ),
            )?),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IterativeSearchContext {
    files: Vec<File>,
    user_query: String,
    scratch_pad: String,
}

impl IterativeSearchContext {
    pub fn new(files: Vec<File>, user_query: String, scratch_pad: String) -> Self {
        Self {
            files,
            user_query,
            scratch_pad,
        }
    }

    pub fn files(&self) -> &[File] {
        &self.files
    }

    pub fn add_files(&mut self, files: Vec<File>) {
        self.files.extend(files)
    }

    pub fn file_paths_as_strings(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|f| f.path().to_string_lossy().into_owned())
            .collect()
    }

    pub fn update_scratch_pad(&mut self, scratch_pad: &str) {
        self.scratch_pad = scratch_pad.to_string()
    }

    pub fn extend_scratch_pad(&mut self, info: &str) {
        self.scratch_pad.push('\n');
        self.scratch_pad.push_str(info);
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn scratch_pad(&self) -> &str {
        &self.scratch_pad
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    path: PathBuf,
    thinking: String,
    snippet: String,
    // content: String,
    // preview: String,
}

impl File {
    pub fn new(path: &PathBuf, thinking: &str, snippet: &str) -> Self {
        Self {
            path: path.to_owned(),
            thinking: thinking.to_owned(),
            snippet: snippet.to_owned(),
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn serialise_files(files: &[File], separator: &str) -> String {
        let serialised_files: Vec<String> = files
            .iter()
            .filter_map(|f| match to_string(f) {
                Ok(s) => Some(GoogleStudioLLM::strip_xml_declaration(&s).to_string()),
                Err(e) => {
                    eprintln!("Error serializing Files: {:?}", e);
                    None
                }
            })
            .collect();

        serialised_files.join(separator)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum SearchToolType {
    #[default] // arbitrarily default to File
    File,
    Keyword,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SearchQuery {
    #[serde(default)]
    pub thinking: String,
    #[serde(default)]
    pub tool: SearchToolType,
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename = "search_requests", default)]
pub struct SearchRequests {
    #[serde(rename = "request", default)]
    pub requests: Vec<SearchQuery>,
}

#[derive(Debug, Error)]
pub enum IterativeSearchError {
    #[error("LLM Client erorr: {0}")]
    LLMClientError(#[from] LLMClientError),

    #[error("Serde error: {0}")]
    SerdeError(#[from] SerdeError),

    #[error("Quick xml error: {0}")]
    QuickXMLError(#[from] quick_xml::DeError),

    #[error("User context error: {0}")]
    UserContextError(#[from] UserContextError),

    #[error("Exhausted retries")]
    ExhaustedRetries,

    #[error("Empty response")]
    EmptyResponse,

    #[error("Wrong LLM for input: {0}")]
    WrongLLM(LLMType),

    #[error("Wrong format: {0}")]
    WrongFormat(String),

    #[error("No seed provided")]
    NoSeed(),

    #[error("Tree printing failed for: {0}")]
    PrintTreeError(String),

    #[error("Human communication error: {0}")]
    HumanCommunicationError(#[from] CommunicationError),

    #[error("Missing question choice, choice_id: {0}")]
    MissingQuestionChoiceError(String),

    #[error("No tags found for file: {0}")]
    NoTagsForFile(PathBuf),

    #[error("Send error: {0}")]
    SendError(#[from] SendError<UIEventWithID>),
}

impl SearchQuery {
    pub fn new(tool: SearchToolType, query: String, thinking: String) -> Self {
        Self {
            tool,
            query,
            thinking,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    path: PathBuf,
    thinking: String,
    snippet: SearchResultSnippet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchResultSnippet {
    FileContent(Vec<u8>),
    Tag(String),
}

impl SearchResult {
    pub fn new(path: PathBuf, thinking: &str, snippet: SearchResultSnippet) -> Self {
        Self {
            path,
            thinking: thinking.to_string(),
            snippet,
        }
    }
}

#[async_trait]
pub trait LLMOperations {
    async fn generate_search_query(
        &self,
        context: &IterativeSearchContext,
    ) -> Result<Vec<SearchQuery>, IterativeSearchError>;
    async fn identify_relevant_results(
        &self,
        context: &IterativeSearchContext,
        search_results: &[SearchResult],
    ) -> Result<IdentifyResponse, IterativeSearchError>;
    async fn decide_continue(
        &self,
        context: &mut IterativeSearchContext,
    ) -> Result<DecideResponse, IterativeSearchError>;
    async fn query_relevant_files(
        &self,
        user_query: &str,
        seed: IterativeSearchSeed,
    ) -> Result<QueryRelevantFilesResponse, IterativeSearchError>;
    async fn generate_human_question(
        &self,
        context: &IterativeSearchContext,
    ) -> Result<GenerateHumanQuestionResponse, IterativeSearchError>;
}

pub struct IterativeSearchSystem<T: LLMOperations> {
    context: IterativeSearchContext,
    repository: Repository,
    llm_ops: T,
    complete: bool,
    seed: Option<IterativeSearchSeed>,
    event_handlers: Vec<Box<dyn IterativeSearchEventHandler>>,
}

impl<T: LLMOperations> IterativeSearchSystem<T> {
    pub fn new(
        context: IterativeSearchContext,
        repository: Repository,
        llm_ops: T,
        event_handlers: Vec<Box<dyn IterativeSearchEventHandler>>,
    ) -> Self {
        Self {
            context,
            repository,
            llm_ops,
            complete: false,
            seed: None,
            event_handlers,
        }
    }

    pub fn add_event_handler(&mut self, handler: Box<dyn IterativeSearchEventHandler>) {
        self.event_handlers.push(handler);
    }

    fn emit_event(&self, event: IterativeSearchEvent) {
        for handler in &self.event_handlers {
            if let Err(e) = handler.handle_event(event.clone()) {
                eprintln!(
                    "IterativeSearchSystem::emit_event::error: Error handling event: {}",
                    e
                );
            }
        }
    }

    pub fn with_seed(mut self, seed: IterativeSearchSeed) -> Self {
        self.seed = Some(seed);
        self
    }

    fn context(&self) -> &IterativeSearchContext {
        &self.context
    }

    pub async fn apply_seed(&mut self) -> Result<(), IterativeSearchError> {
        if let Some(seed) = self.seed.take() {
            let scratch_pad_thinking = self
                .llm_ops
                .query_relevant_files(&self.context.user_query(), seed)
                .await?
                .scratch_pad;

            self.context.update_scratch_pad(&scratch_pad_thinking);
            println!("Seed applied successfully");
        } else {
            println!("No seed provided, skipping seed application");
        }
        Ok(())
    }

    pub async fn run(&mut self) -> Result<CodeSymbolImportantResponse, IterativeSearchError> {
        let start_time = Instant::now();
        self.emit_event(IterativeSearchEvent::SearchStarted);

        let seed_start = Instant::now();
        self.apply_seed().await?;
        self.emit_event(IterativeSearchEvent::SeedApplied(seed_start.elapsed()));
        println!("Seed applied in {:?}", seed_start.elapsed());

        let mut count = 0;
        while !self.complete && count < 3 {
            let loop_start = Instant::now();
            println!("===========");
            println!("search loop #{}", count);
            println!("===========");

            let search_query_start = Instant::now();
            let search_queries = self.generate_search_query().await?;
            self.emit_event(IterativeSearchEvent::SearchQueriesGenerated(
                search_queries.clone(),
                search_query_start.elapsed(),
            ));
            println!(
                "Search queries generated in {:?}",
                search_query_start.elapsed()
            );

            let search_start = Instant::now();
            let search_results: Vec<SearchResult> = search_queries
                .iter()
                .flat_map(|q| self.repository.execute_search(q))
                .collect();
            self.emit_event(IterativeSearchEvent::SearchExecuted(
                search_results.clone(),
                search_start.elapsed(),
            ));
            println!("Search executed in {:?}", search_start.elapsed());

            let identify_start = Instant::now();
            let identify_results = self.identify(&search_results).await?;
            self.emit_event(IterativeSearchEvent::IdentificationCompleted(
                identify_results.clone(),
                identify_start.elapsed(),
            ));
            println!("Identification completed in {:?}", identify_start.elapsed());

            self.context
                .update_scratch_pad(&identify_results.scratch_pad);

            let generate_file_outline = Instant::now();
            self.context.add_files(
                identify_results
                    .items
                    .iter()
                    .map(|f| {
                        let path = f.path();
                        let tags = self.repository.get_file_tags(path);

                        let snippet = if let Some(tags) = tags {
                            RepoMap::to_tree(&tags)
                        } else {
                            "".to_string()
                        };

                        File::new(f.path(), f.thinking(), &snippet)
                    })
                    .collect(),
            );
            self.emit_event(IterativeSearchEvent::FileOutlineGenerated(
                generate_file_outline.elapsed(),
            ));
            println!(
                "File outline generation completed in {:?}",
                generate_file_outline.elapsed()
            );

            let decision_start = Instant::now();
            let decision = self.decide().await?;
            self.emit_event(IterativeSearchEvent::DecisionMade(
                decision.clone(),
                decision_start.elapsed(),
            ));
            println!("Decision made in {:?}", decision_start.elapsed());

            self.context.update_scratch_pad(decision.suggestions());
            self.complete = decision.complete();

            println!("===========");
            println!("Decision: {}", decision.complete());
            println!("Suggestions: {}", decision.suggestions());
            println!("===========");

            count += 1;
            self.emit_event(IterativeSearchEvent::LoopCompleted(
                count,
                loop_start.elapsed(),
            ));
            println!("===========");
            println!("Loop {} completed in {:?}", count, loop_start.elapsed());
            println!("Scratch_pad: {}", self.context.scratch_pad());
            println!("===========");
        }

        let symbols = self
            .context()
            .file_paths_as_strings()
            .iter()
            .map(|path| CodeSymbolWithThinking::from_path(path))
            .collect();

        let ordered_symbols = self
            .context()
            .file_paths_as_strings()
            .iter()
            .map(|path| CodeSymbolWithSteps::from_path(path))
            .collect();

        let response = CodeSymbolImportantResponse::new(symbols, ordered_symbols);

        self.emit_event(IterativeSearchEvent::SearchCompleted(start_time.elapsed()));
        println!("Total execution time: {:?}", start_time.elapsed());

        Ok(response)
    }

    // this generates search queries
    async fn generate_search_query(&self) -> Result<Vec<SearchQuery>, IterativeSearchError> {
        self.llm_ops.generate_search_query(self.context()).await
    }

    // identifies keywords to keep
    async fn identify(
        &mut self,
        search_results: &[SearchResult],
    ) -> Result<IdentifyResponse, IterativeSearchError> {
        self.llm_ops
            .identify_relevant_results(self.context(), search_results)
            .await
    }

    async fn decide(&mut self) -> Result<DecideResponse, IterativeSearchError> {
        self.llm_ops.decide_continue(&mut self.context).await
    }
}
