use clap::Parser;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{
        AnthropicAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys, OpenRouterAPIKey,
    },
};
use serde::{Deserialize, Serialize};
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
#[command(author = "skcd", version = "1.0", about = "SWE-Bench Sidecar Runner")]
struct CliArgs {
    /// Git directory name
    #[arg(long)]
    timeout: usize,

    /// Endpoint URL
    #[arg(long)]
    editor_url: String,

    /// Timeout in seconds
    #[arg(long)]
    input: PathBuf,

    /// Anthropic api key
    #[arg(long, default_value = None)]
    anthropic_api_key: Option<String>,

    /// OPen Router api key
    #[arg(long, default_value = None)]
    openrouter_api_key: Option<String>,

    /// The run id for the current run
    #[arg(long)]
    run_id: String,

    #[arg(long)]
    repo_name: String,

    /// Directory to dump all the logs into
    #[arg(long)]
    log_directory: String,

    /// Use json mode strictly
    #[arg(long, default_value = "true")]
    json_mode: bool,

    /// Use midwit mode (aka sonnet3.5 with tool)
    #[arg(long, default_value = "true")]
    midwit_mode: bool,

    /// Run in single trajectory but a lot of them
    #[arg(long, default_value = None)]
    single_traj_search: Option<usize>,

    /// Maximum depth for the search tree
    #[arg(long, default_value = "30")]
    max_depth: u32,
}

/// Define the SWEbenchInstance struct for serialization
#[derive(Debug, Serialize, Deserialize)]
struct SWEbenchInstance {
    repo: String,
    instance_id: String,
    base_commit: String,
    patch: String,
    test_patch: String,
    problem_statement: String,
    hints_text: String,
    created_at: String,
    version: String,
    #[serde(rename = "FAIL_TO_PASS")]
    fail_to_pass: String,
    #[serde(rename = "PASS_TO_PASS")]
    pass_to_pass: String,
    environment_setup_commit: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct InputParts {
    git_drname: String,
    instance: SWEbenchInstance,
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

    let editor_url = args.editor_url.to_owned();
    let _timeout = args.timeout;
    let input_path = args.input;
    let run_id = args.run_id.to_owned();
    let repo_name = args.repo_name.to_owned();
    let log_directory = args.log_directory.to_owned();
    let input_content = tokio::fs::read(input_path).await.expect("path content");
    let input_parts: InputParts =
        serde_json::from_slice(&input_content).expect("Parse the serde json");

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

    let session_id = format!(
        "{}-{}",
        input_parts.instance.instance_id,
        run_id.to_string()
    );

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

    let agent_settings = AgentSettings::new(args.json_mode, args.midwit_mode);

    // The bad actions which hurt the check_for_bad_children_actions
    let bad_actions = if agent_settings.is_json() {
        vec![ToolType::CodeEditorTool]
    } else {
        vec![ToolType::CodeEditing]
    };

    let mut tools = vec![
        ToolType::ListFiles,
        ToolType::SearchFileContentWithRegex,
        // if we are in json mode then select the code editor tool
        if args.json_mode {
            ToolType::CodeEditorTool
        } else {
            ToolType::CodeEditing
        },
        ToolType::AttemptCompletion,
        ToolType::TerminalCommand,
    ];

    if !args.midwit_mode {
        tools.push(ToolType::TestRunner);
    }

    // add the open file only if we are not in the json mode
    // if !args.json_mode {
    //     tools.push(ToolType::OpenFile);
    // }
    tools.push(ToolType::OpenFile);

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

    // how many children the node can have?
    let expansions = if args.single_traj_search.is_some() {
        // if we are doing single traj then only allow for a single node expansion
        1
    } else {
        2
    };

    // Instantiate the mcts tree over here and start the search
    let mut search_tree = SearchTree::new(
        expansions,                                  // max_expansions
        args.max_depth,                              // max_depth of the tree
        400,                                         // max_iterations
        Some(5),                                     // max_finished_nodes
        None,                                        // reward_threshold
        Some(2),                                     // min_finished_nodes
        args.single_traj_search,                     // max_search_try
        input_parts.git_drname.to_owned(),           // root_directory
        repo_name,                                   // repo_name
        input_parts.instance.base_commit.to_owned(), // base_commit
        input_parts.instance.problem_statement,      // problem_statment
        selector,                                    // selector
        tools,                                       // tools
        tool_box,                                    // tool_box
        llm_broker,                                  // llm_client
        log_directory,                               // log directory
        agent_settings,                              // agent_settings
    );

    // Run the search
    search_tree.run_search(message_properties).await;

    Ok(())
}
