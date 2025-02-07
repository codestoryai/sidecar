use std::{fs, path::PathBuf, sync::Arc};
use clap::Parser;
use serde_json;
use sidecar::mcts::{
    action_node::{SearchTree, SearchTreeMinimal, ActionNode},
    selector::selector::Selector,
};
use sidecar::{
    agentic::{
        symbol::{
            events::message_event::SymbolEventMessageProperties,
            events::input::SymbolEventRequestId,
            identifier::LLMProperties,
            ui_event::UIEventWithID,
            tool_box::ToolBox,
        },
        tool::{
            session::tool_use_agent::{ToolUseAgent, ToolUseAgentReasoningInput, ToolUseAgentProperties},
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
        },
    },
    chunking::{
        editor_parsing::EditorParsing,
        languages::TSLanguageParsing,
    },
    inline_completion::symbols_tracker::SymbolTrackerInline,
};
use llm_client::{
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys, OpenAIProvider},
    broker::LLMBroker,
};
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

#[derive(Parser, Debug)]
#[clap(name = "mcts_query")]
#[clap(about = "Query an MCTS JSON file with questions")]
struct Args {
    /// Path to the MCTS JSON file
    #[clap(short, long)]
    mcts_path: PathBuf,

    /// Question to ask about the MCTS trajectory
    #[clap(short, long)]
    question: String,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let args = Args::parse();

    // Read the JSON file
    let json_content = fs::read_to_string(args.mcts_path)?;
    
    // Deserialize into SearchTreeMinimal
    let search_tree_minimal: SearchTreeMinimal = serde_json::from_str(&json_content)?;

    // Create required dependencies in the correct order
    let llm_broker = Arc::new(LLMBroker::new().await?);
    let editor_parsing = Arc::new(EditorParsing::default());
    let language_parsing = Arc::new(TSLanguageParsing::init());
    let symbol_tracker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    
    // Create dummy/default components for ToolBroker
    let code_edit_broker = Arc::new(CodeEditBroker::new());
    let tool_broker_config = ToolBrokerConfiguration::new(None, false);
    // Use empty string for now since we don't actually need the API key for this use case
    let fail_over_llm = LLMProperties::new(
        LLMType::O1,
        LLMProvider::OpenAI,
        LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(String::new())),
    );

    let tool_broker = Arc::new(ToolBroker::new(
        llm_broker.clone(),
        code_edit_broker,
        symbol_tracker.clone(),
        language_parsing,
        tool_broker_config,
        fail_over_llm,
    ));
    
    let tool_box = Arc::new(ToolBox::new(
        tool_broker,
        symbol_tracker,
        editor_parsing,
    ));

    // Create selector with default values similar to swe_bench_submission
    let selector = Selector::new(
        1.0,    // exploitation_weight
        false,  // use_average_reward
        1.0,    // exploration_weight
        0.8,    // depth_weight
        0.0,    // depth_bonus_factor
        50.0,   // high_value_threshold
        0.0,    // low_value_threshold
        75.0,   // very_high_value_threshold
        50.0,   // high_value_leaf_bonus_constant
        20.0,   // high_value_bad_children_bonus_constant
        5.0,    // high_value_child_penalty_constant
        50.0,   // finished_trajectory_penalty
        50.0,   // expect_correction_bonus
        vec![], // check_for_bad_child_actions
        100.0,  // diversity_weight
        25.0,   // duplicate_child_penalty_constant
        50.0,   // duplicate_action_penalty_constant
    );

    // Convert to SearchTree with updated parameters
    let search_tree = SearchTree::from_minimal_tree(
        search_tree_minimal,
        selector,
        llm_broker.clone(),
        tool_box.clone(),
        vec![], // Empty tools vector as default
    );

    // Create tool use agent with updated parameters
    let tool_use_agent = ToolUseAgent::new(
        llm_broker.clone(),
        std::env::current_dir()?.to_string_lossy().to_string(),
        "linux".to_string(),
        "bash".to_string(),
        ToolUseAgentProperties::new(
            true,
            Some("mcts_query".to_string()),
            None,
        ),
    );

    // Create necessary components for SymbolEventMessageProperties
    let (ui_sender, _ui_receiver) = unbounded_channel::<UIEventWithID>();
    let request_id = SymbolEventRequestId::new(
        "mcts_query".to_string(),
        "mcts_query_root".to_string(),
    );
    let cancellation_token = CancellationToken::new();
    let api_key = LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(
        std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "sk-dummy-test-key".to_string())
    ));
    let llm_properties = LLMProperties::new(
        LLMType::O1,
        LLMProvider::OpenAI,
        api_key,
    );

    // Create message properties
    let message_properties = SymbolEventMessageProperties::new(
        request_id,
        ui_sender,
        "mcts_query".to_string(),
        cancellation_token,
        llm_properties,
    );

    // Convert SearchTree nodes to Vec<ActionNode>
    let action_nodes: Vec<ActionNode> = search_tree.index_to_node.values().cloned().collect();

    // Test print to verify thinking is preserved
    if let Some(first_node) = action_nodes.first() {
        if let Some(observation) = first_node.observation() {
            if let Some(thinking) = observation.thinking() {
                println!("Thinking from first node: {}", thinking);
            }
        }
    }

    // Create reasoning input
    let reasoning_input = ToolUseAgentReasoningInput::new(
        args.question.clone(),
        action_nodes,
        None,
        message_properties,
    );

    // Process the reasoning request
    let result = tool_use_agent.reasoning_output(reasoning_input).await?;
    
    // Print the result
    println!("Instruction:\n{}\n", result.instruction());
    println!("Notes:\n{}\n", result.notes());
    println!("Result summary:\nInstructions and notes from MCTS trajectory analysis");

    Ok(())
}