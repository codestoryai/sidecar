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
};

#[tokio::main]
async fn main() {
    // we want to grab the implementations of the symbols over here which we are
    // interested in
    let editor_parsing = Arc::new(EditorParsing::default());
    let language_parsing = Arc::new(TSLanguageParsing::init());
    let llm_broker = Arc::new(LLMBroker::new().await.expect("to initialize properly"));
    let tool_broker = Arc::new(
        ToolBroker::new(
            llm_broker.clone(),
            Arc::new(CodeEditBroker::new()),
            editor_parsing.clone(),
            language_parsing.clone(),
            ToolBrokerConfiguration::new(None, true),
            LLMProperties::new(
                LLMType::GeminiPro,
                LLMProvider::GoogleAIStudio,
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
            ),
        )
        .await,
    );

    let tool_box = Arc::new(ToolBox::new(tool_broker, editor_parsing.clone(), editor_parsing));

    // Use this to get back the parent-symbol and the child symbols which have
    // been edited in a file
    // so iteration is literally making changes and having any kind of changes
    // on a file, we can hook this up with the implementations/references test

    // Your root directory
    let root_directory = "/Users/skcd/scratch/sidecar";
    // File where you have made changes
    let _fs_file_path = "/Users/skcd/scratch/sidecar/llm_client/src/clients/types.rs";
    let output = tool_box
        .get_git_diff(root_directory)
        .await
        .expect("to work");

    println!("{:?}", output);

    // // from here we have to go a level deeper into the sub-symbol of the symbol where
    // // the changed values are present and then invoke a followup at that point
    // // println!("{:?}", &output);
    // // a more readable output
    // output.changes().iter().for_each(|symbol_changes| {
    //     println!(
    //         "symbol_name::({})::children({})",
    //         symbol_changes.symbol_identifier().symbol_name(),
    //         symbol_changes
    //             .changes()
    //             .iter()
    //             .map(|(symbol_to_edit, _, _)| symbol_to_edit.symbol_name().to_owned())
    //             .collect::<Vec<_>>()
    //             .join(",")
    //     );
    // });
}