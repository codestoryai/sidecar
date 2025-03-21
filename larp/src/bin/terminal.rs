use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{
        AnthropicAPIKey, FireworksAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys,
        OpenAIProvider,
    },
};
use sidecar::agentic::symbol::ui_event::UIEventWithID;
use sidecar::{
    agentic::{
        symbol::{identifier::LLMProperties, manager::SymbolManager},
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    inline_completion::symbols_tracker::SymbolTrackerInline,
    user_context::types::UserContext,
};

#[tokio::main]
async fn main() {
    let request_id = uuid::Uuid::new_v4();
    let request_id_str = request_id.to_string();
    let parea_url = format!(
        r#"https://app.parea.ai/logs?colViz=%7B%220%22%3Afalse%2C%221%22%3Afalse%2C%222%22%3Afalse%2C%223%22%3Afalse%2C%22error%22%3Afalse%2C%22deployment_id%22%3Afalse%2C%22feedback_score%22%3Afalse%2C%22time_to_first_token%22%3Afalse%2C%22scores%22%3Afalse%2C%22start_timestamp%22%3Afalse%2C%22user%22%3Afalse%2C%22session_id%22%3Afalse%2C%22target%22%3Afalse%2C%22experiment_uuid%22%3Afalse%2C%22dataset_references%22%3Afalse%2C%22in_dataset%22%3Afalse%2C%22event_type%22%3Afalse%2C%22request_type%22%3Afalse%2C%22evaluation_metric_names%22%3Afalse%2C%22request%22%3Afalse%2C%22calling_node%22%3Afalse%2C%22edges%22%3Afalse%2C%22metadata_evaluation_metric_names%22%3Afalse%2C%22metadata_event_type%22%3Afalse%2C%22metadata_0%22%3Afalse%2C%22metadata_calling_node%22%3Afalse%2C%22metadata_edges%22%3Afalse%2C%22metadata_root_id%22%3Afalse%7D&filter=%7B%22filter_field%22%3A%22meta_data%22%2C%22filter_operator%22%3A%22equals%22%2C%22filter_key%22%3A%22root_id%22%2C%22filter_value%22%3A%22{request_id_str}%22%7D&page=1&page_size=50&time_filter=1m"#
    );
    println!("===========================================\nRequest ID: {}\nParea AI: {}\n===========================================", request_id.to_string(), parea_url);

    // check this
    let _editor_url = "http://localhost:42427".to_owned();
    let anthropic_api_keys = LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned()));
    let anthropic_llm_properties = LLMProperties::new(
        LLMType::ClaudeSonnet,
        LLMProvider::Anthropic,
        anthropic_api_keys.clone(),
    );
    let _llama_70b_properties = LLMProperties::new(
        LLMType::Llama3_1_70bInstruct,
        LLMProvider::FireworksAI,
        LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new("".to_owned())),
    );
    let _google_ai_studio_api_keys =
        LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned()));
    let editor_parsing = Arc::new(EditorParsing::default());
    let symbol_broker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    let llm_broker = LLMBroker::new().await.expect("to initialize properly");

    let tool_broker = Arc::new(
        ToolBroker::new(
            Arc::new(llm_broker),
            Arc::new(CodeEditBroker::new()),
            symbol_broker.clone(),
            Arc::new(TSLanguageParsing::init()),
            // for our testing workflow we want to apply the edits directly
            ToolBrokerConfiguration::new(None, true),
            LLMProperties::new(
                LLMType::Gpt4O,
                LLMProvider::OpenAI,
                LLMProviderAPIKeys::OpenAI(OpenAIProvider::new("".to_owned())),
            ), // LLMProperties::new(
               //     LLMType::GeminiPro,
               //     LLMProvider::GoogleAIStudio,
               //     LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
               //         "".to_owned(),
               //     )),
               // ),
        )
        .await,
    );

    let _user_context = UserContext::new(vec![], vec![], None, vec![]);

    let (sender, mut _receiver) = tokio::sync::mpsc::unbounded_channel();

    // fill this
    let _access_token = String::from("");

    // let message_properties = SymbolEventMessageProperties::new(
    //     SymbolEventRequestId::new("".to_owned(), "".to_owned()),
    //     sender.clone(),
    //     editor_url.to_owned(),
    //     tokio_util::sync::CancellationToken::new(),
    //     access_token,
    // );

    let test_command = "ls".to_owned();

    let session_id = "14b55c44-fab8-4a23-a598-94b3295dc574".to_owned();
    let exchange_id = "14b55c44-fab8-4a23-a598-94b3295dc574".to_owned();

    let ui_event_with_id = UIEventWithID::terminal_command(
        session_id.clone(),
        exchange_id.clone(),
        test_command.clone(),
    );
    // Send the event
    let res = sender.send(ui_event_with_id);

    dbg!(&res);

    let _ = _receiver.recv().await;

    // Process received events from the receiver
    // while let Some(event) = _receiver.recv().await {
    //     match event {
    //         UIEventWithID::TerminalCommand {
    //             session_id: _,
    //             exchange_id: _,
    //             command,
    //         } => {
    //             println!("Received terminal command: {}", command);
    //             // Handle the terminal command here
    //         }
    //         _ => {
    //             // Handle other event types if needed
    //         }
    //     }
    // }

    let _symbol_manager = SymbolManager::new(
        tool_broker.clone(),
        symbol_broker.clone(),
        editor_parsing,
        anthropic_llm_properties.clone(),
    );
}
