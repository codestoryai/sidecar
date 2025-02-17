// This is where we will define the core application and all the related things
// on how to startup the application

use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys, OpenAIProvider},
    tokenizer::tokenizer::LLMTokenizer,
};
use llm_prompts::{
    answer_model::LLMAnswerModelBroker, chat::broker::LLMChatModelBroker,
    fim::types::FillInMiddleBroker, in_line_edit::broker::InLineEditPromptBroker,
    reranking::broker::ReRankBroker,
};
use once_cell::sync::OnceCell;
use tracing::{debug, warn};

use crate::repo::state::RepositoryPool;
use crate::{
    agentic::{
        symbol::{identifier::LLMProperties, manager::SymbolManager, tool_box::ToolBox},
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
            session::service::SessionService,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    reporting::posthog::client::{posthog_client, PosthogClient},
    webserver::agentic::{AnchoredEditingTracker, ProbeRequestTracker},
};

use super::{config::configuration::Configuration, logging::tracing::tracing_subscribe};

static LOGGER_INSTALLED: OnceCell<bool> = OnceCell::new();

#[derive(Clone)]
pub struct Application {
    // Arc here because its shared by many things and is the consistent state
    // for the application
    pub config: Arc<Configuration>,
    pub repo_pool: RepositoryPool,
    /// We also want to keep the language parsing functionality here
    pub language_parsing: Arc<TSLanguageParsing>,
    pub posthog_client: Arc<PosthogClient>,
    pub user_id: String,
    pub llm_broker: Arc<LLMBroker>,
    pub llm_tokenizer: Arc<LLMTokenizer>,
    pub chat_broker: Arc<LLMChatModelBroker>,
    pub reranker: Arc<ReRankBroker>,
    pub answer_models: Arc<LLMAnswerModelBroker>,
    pub editor_parsing: Arc<EditorParsing>,
    pub probe_request_tracker: Arc<ProbeRequestTracker>,
    pub symbol_manager: Arc<SymbolManager>,
    pub tool_box: Arc<ToolBox>,
    pub anchored_request_tracker: Arc<AnchoredEditingTracker>,
    pub session_service: Arc<SessionService>,
}

impl Application {
    pub async fn initialize(mut config: Configuration) -> anyhow::Result<Self> {
        config.max_threads = config.max_threads.max(minimum_parallelism());
        // Setting the directory for the state and where we will be storing
        // things
        config.state_source.set_default_dir(&config.index_dir);
        debug!(?config, "configuration after loading");
        let repo_pool = config.state_source.initialize_pool()?;
        let config = Arc::new(config);
        let language_parsing = Arc::new(TSLanguageParsing::init());
        let posthog_client = posthog_client(&config.user_id);
        let llm_broker = Arc::new(LLMBroker::new().await?);
        let llm_tokenizer = Arc::new(LLMTokenizer::new()?);
        let chat_broker = Arc::new(LLMChatModelBroker::init());
        let reranker = Arc::new(ReRankBroker::new());
        let answer_models = Arc::new(LLMAnswerModelBroker::new());
        let editor_parsing = Arc::new(EditorParsing::default());

        let tool_broker = Arc::new(
            ToolBroker::new(
                llm_broker.clone(),
                Arc::new(CodeEditBroker::new()),
                language_parsing.clone(),
                ToolBrokerConfiguration::new(None, config.apply_directly),
                LLMProperties::new(
                    LLMType::Gpt4O,
                    LLMProvider::OpenAI,
                    LLMProviderAPIKeys::OpenAI(OpenAIProvider::new("".to_owned())),
                ),
            )
            .await,
        );
        let tool_box = Arc::new(ToolBox::new(
            tool_broker.clone(),
            editor_parsing.clone(),
        ));
        let symbol_manager = Arc::new(SymbolManager::new(
            tool_broker,
            editor_parsing.clone(),
            LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::Anthropic,
                LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned())),
            ),
        ));
        let session_service = Arc::new(SessionService::new(
            tool_box.clone(),
            symbol_manager.clone(),
        ));

        let anchored_request_tracker = Arc::new(AnchoredEditingTracker::new());
        Ok(Self {
            config: config.clone(),
            repo_pool,
            language_parsing,
            posthog_client: Arc::new(posthog_client),
            user_id: config.user_id.clone(),
            llm_broker,
            llm_tokenizer,
            chat_broker,
            reranker,
            answer_models,
            editor_parsing,
            probe_request_tracker: Arc::new(ProbeRequestTracker::new()),
            symbol_manager,
            tool_box,
            anchored_request_tracker,
            session_service,
        })
    }

    pub fn install_logging(config: &Configuration) {
        if let Some(true) = LOGGER_INSTALLED.get() {
            return;
        }

        if !tracing_subscribe(config) {
            warn!("Failed to install tracing_subscriber. There's probably one already...");
        };

        if color_eyre::install().is_err() {
            warn!("Failed to install color-eyre. Oh well...");
        };

        LOGGER_INSTALLED.set(true).unwrap();
    }

    /// This can blow up, so be careful
    pub async fn setup_scratch_pad(config: &Configuration) {
        let scratch_pad_path = config.scratch_pad();
        if !tokio::fs::try_exists(&scratch_pad_path)
            .await
            .expect("checking for scratch_pad directory creation should work")
        {
            tokio::fs::create_dir_all(&scratch_pad_path)
                .await
                .expect("scratch_pad directory creation failed");
        }
    }
}

// We need at the very least 1 thread to do background work
fn minimum_parallelism() -> usize {
    1
}