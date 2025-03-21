use clap::Parser;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{
        AnthropicAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys, OpenRouterAPIKey,
    },
};
use sidecar::{
    agentic::{
        symbol::{
            events::{input::SymbolEventRequestId, message_event::SymbolEventMessageProperties},
            identifier::LLMProperties,
            tool_box::ToolBox,
        },
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
            r#type::ToolType,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    inline_completion::symbols_tracker::SymbolTrackerInline,
    mcts::{
        action_node::SearchTree, agent_settings::settings::AgentSettings,
        selector::selector::Selector,
    },
};
use std::{path::PathBuf, sync::Arc};

/// Define the command-line arguments
#[derive(Parser, Debug)]
#[command(author = "skcd", version = "1.0", about = "Midwit tool use")]
struct CliArgs {
    /// Timeout in seconds
    #[arg(long)]
    timeout: usize,

    /// Repository location
    #[arg(long)]
    repo_location: PathBuf,

    /// Repository name (I am sorry for asking this)
    #[arg(long)]
    repo_name: String,

    /// Anthropic api key
    #[arg(long, default_value = None)]
    anthropic_api_key: Option<String>,

    /// OPen Router api key
    #[arg(long, default_value = None)]
    openrouter_api_key: Option<String>,

    /// The run id for the current run
    #[arg(long)]
    problem_statement: String,
    /// Restore an existing search graph.
    #[arg(long)]
    restore: Option<PathBuf>,
}

fn default_index_dir() -> PathBuf {
    match directories::ProjectDirs::from("ai", "codestory", "sidecar") {
        Some(dirs) => dirs.data_dir().to_owned(),
        None => "codestory_sidecar".into(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments
    let args = CliArgs::parse();

    let editor_parsing = Arc::new(EditorParsing::default());
    let symbol_broker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    let llm_broker = Arc::new(LLMBroker::new().await.expect("to initialize properly"));
    let tool_broker = Arc::new(
        ToolBroker::new(
            llm_broker.clone(),
            Arc::new(CodeEditBroker::new()),
            symbol_broker.clone(),
            Arc::new(TSLanguageParsing::init()),
            ToolBrokerConfiguration::new(None, true),
            LLMProperties::new(
                LLMType::GeminiPro,
                LLMProvider::GoogleAIStudio,
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
            ),
        )
        .await,
    );

    let tool_box = Arc::new(ToolBox::new(tool_broker, symbol_broker, editor_parsing));

    let editor_url = "".to_owned();
    let _timeout = args.timeout;
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clocks to not drift")
        .as_secs()
        .to_string();

    let log_directory;
    {
        let log_directory_path = default_index_dir().join("tool_use");
        if tokio::fs::metadata(&log_directory_path).await.is_err() {
            tokio::fs::create_dir(&log_directory_path)
                .await
                .expect("directory creation to not fail");
        }
        log_directory = default_index_dir().join("tool_use").join(run_id.to_owned());
    }

    let model_configuration: LLMProperties;
    if let Some(anthropic_key) = args.anthropic_api_key {
        model_configuration = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::Anthropic,
            LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new(anthropic_key)),
        );
    } else if let Some(open_router_key) = args.openrouter_api_key {
        model_configuration = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::OpenRouter,
            LLMProviderAPIKeys::OpenRouter(OpenRouterAPIKey::new(open_router_key)),
        );
    } else {
        println!("NO VALID KEY FOUND, TERMINATING");
        return Ok(());
    }

    let session_id = format!("{}", run_id.to_string());

    println!("session_id:{}", &session_id);

    let initial_exchange_id = 0;

    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(
            initial_exchange_id.to_string().to_owned(),
            run_id.to_string(),
        ),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        model_configuration,
    );

    let agent_settings = AgentSettings::new(true, true);

    let bad_actions = vec![ToolType::CodeEditorTool];

    let tools = vec![
        // if we are in json mode then select the code editor tool
        ToolType::CodeEditorTool,
        ToolType::AttemptCompletion,
        ToolType::TerminalCommand,
    ];

    let selector = Selector::new(
        1.0,         // exploitation_weight
        false,       // use_average_reward
        1.0,         // exploration_weight
        0.8,         // depth_weight
        0.0,         // depth_bonus_factor
        50.0,        // high_value_threshold
        0.0,         // low_value_threshold
        75.0,        // very_high_value_threshold
        50.0,        // high_value_leaf_bonus_constant
        20.0,        // high_value_bad_children_bonus_constant
        5.0,         // high_value_child_penalty_constant
        50.0,        // finished_trajectory_penalty
        50.0,        // expect_correction_bonus
        bad_actions, // check_for_bad_child_actions
        100.0,       // diversity_weight
        25.0,        // duplicate_child_penalty_constant
        50.0,        // duplicate_action_penalty_constant
    );

    let expansions = 1;
    let mut search_tree = if let Some(restore) = args.restore {
        SearchTree::from_minimal_tree(
            serde_json::from_slice(&tokio::fs::read(restore).await?)?,
            selector,
            llm_broker,
            tool_box,
            tools,
        )
    } else {
        // Instantiate the mcts tree over here and start the search
        SearchTree::new(
            expansions,                                       // max_expansions
            30,                                               // max_depth of the tree
            400,                                              // max_iterations
            Some(5),                                          // max_finished_nodes
            None,                                             // reward_threshold
            Some(2),                                          // min_finished_nodes
            Some(1),                                          // max_search_try
            args.repo_location.to_string_lossy().to_string(), // root_directory
            args.repo_name.to_owned(),                        // repo_name
            "".to_owned(),                                    // base_commit
            args.problem_statement,                           // problem_statment
            selector,                                         // selector
            tools,                                            // tools
            tool_box,                                         // tool_box
            llm_broker,                                       // llm_client
            log_directory.to_string_lossy().to_string(),      // log directory
            agent_settings,                                   // agent_settings
        )
    };
    // Run the search
    search_tree.run_search(message_properties).await;

    Ok(())
}
