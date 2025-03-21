use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys},
};
use sidecar::{
    agentic::{
        symbol::{identifier::LLMProperties, tool_box::ToolBox},
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    inline_completion::symbols_tracker::SymbolTrackerInline,
    mcts::{
        action_node::{SearchTree, SearchTreeMinimal},
        selector::selector::Selector,
    },
};

#[tokio::main]
async fn main() {
    let content = tokio::fs::read("/Users/skcd/scratch/SWE-bench/swebench_logs/swebench_logs/django__django-12273/1733625780/mcts-1733625780.json")
        .await
        .expect("reading file should work with correct args");

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

    let search_tree_minimal = serde_json::from_slice::<SearchTreeMinimal>(content.as_slice())
        .expect("search_tree_minimal_to_not_fail");

    let search_tree =
        SearchTree::from_minimal_tree(search_tree_minimal, selector, llm_broker, tool_box, vec![]);

    let mut tree_output = vec![];
    search_tree.print_tree(&mut tree_output);

    println!("===========================================");

    let mut steps = vec![];
    search_tree.print_midwit_tree(0, &mut steps);
    println!("{}", steps.join("\n\n"));
}
