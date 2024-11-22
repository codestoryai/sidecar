use clap::{Args as ClapArgs, Parser};
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    config::LLMBrokerConfiguration,
    provider::{AnthropicAPIKey, GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys},
};
use serde::{Deserialize, Serialize};
use sidecar::{
    agentic::{
        symbol::{
            events::{input::SymbolEventRequestId, message_event::SymbolEventMessageProperties},
            identifier::LLMProperties,
            manager::SymbolManager,
            tool_box::ToolBox,
        },
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
            session::service::SessionService,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    inline_completion::symbols_tracker::SymbolTrackerInline,
    repo::types::RepoRef,
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
    #[arg(long)]
    anthropic_api_key: String,

    /// The run id for the current run
    #[arg(long)]
    run_id: String,

    #[arg(long)]
    repo_name: String,
}

/// Define the SWEbenchInstance arguments
#[derive(ClapArgs, Debug)]
struct SWEbenchInstanceArgs {
    /// Repository URL
    #[arg(long)]
    repo: String,

    /// Instance ID
    #[arg(long)]
    instance_id: String,

    /// Base commit hash
    #[arg(long)]
    base_commit: String,

    /// Patch content
    #[arg(long)]
    patch: String,

    /// Test patch content
    #[arg(long)]
    test_patch: String,

    /// Problem statement
    #[arg(long)]
    problem_statement: String,

    /// Hints text
    #[arg(long)]
    hints_text: String,

    /// Creation timestamp
    #[arg(long)]
    created_at: String,

    /// Version
    #[arg(long)]
    version: String,

    /// Fail-to-pass code
    #[arg(long)]
    fail_to_pass: String,

    /// Pass-to-pass code
    #[arg(long)]
    pass_to_pass: String,

    /// Environment setup commit hash
    #[arg(long)]
    environment_setup_commit: String,
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
    let llm_broker = Arc::new(
        LLMBroker::new(LLMBrokerConfiguration::new(default_index_dir()))
            .await
            .expect("to initialize properly"),
    );
    let tool_broker = Arc::new(ToolBroker::new(
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
    ));

    let symbol_tracker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));

    let symbol_manager = Arc::new(SymbolManager::new(
        tool_broker.clone(),
        symbol_tracker.clone(),
        editor_parsing.clone(),
        LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::Anthropic,
            LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned())),
        ),
    ));

    let tool_box = Arc::new(ToolBox::new(tool_broker, symbol_broker, editor_parsing));

    let editor_url = args.editor_url.to_owned();
    let _timeout = args.timeout;
    let input_path = args.input;
    let run_id = args.run_id.to_owned();
    let repo_name = args.repo_name.to_owned();
    let anthropic_api_key = args.anthropic_api_key.to_owned();
    let input_content = tokio::fs::read(input_path).await.expect("path content");
    let input_parts: InputParts =
        serde_json::from_slice(&input_content).expect("Parse the serde json");

    let model_configuration = LLMProperties::new(
        LLMType::ClaudeSonnet,
        LLMProvider::Anthropic,
        LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new(anthropic_api_key)),
    );

    let session_id = format!(
        "{}-{}",
        input_parts.instance.instance_id,
        run_id.to_string()
    );

    println!("session_id:{}", &session_id);

    // Creates the unique path for the session
    let session_path = default_index_dir().join("session");
    // check if the plan_storage_path_exists
    if tokio::fs::metadata(&session_path).await.is_err() {
        tokio::fs::create_dir(&session_path)
            .await
            .expect("directory creation to not fail");
    }
    let session_path = session_path.join(session_id.to_owned());
    let storage_path = session_path
        .to_str()
        .expect("path conversion to work on all platforms")
        .to_owned();

    let initial_exchange_id = 0;

    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(
            initial_exchange_id.to_string().to_owned(),
            session_id.to_string(),
        ),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        model_configuration,
    );

    let session_service = SessionService::new(tool_box.clone(), symbol_manager);
    println!("session_service::tool_use_agentic_swe_bench");
    let mut trajectory_tries = 0;
    // change this as required honestly
    let test_files = vec!["tests/queries/test_qs_combinators.py".to_owned()];

    loop {
        // generate tests to test out the code gen output
        trajectory_tries = trajectory_tries + 1;
        if trajectory_tries > 5 {
            break;
        }
        let _ = session_service
            .tool_use_agentic_swe_bench(
                session_id.to_owned(),
                storage_path.to_owned(),
                repo_name.to_owned(),
                input_parts.instance.problem_statement.to_owned(),
                initial_exchange_id.to_string(),
                vec![],
                vec![],
                "bash".to_owned(),
                vec![],
                RepoRef::local(&input_parts.git_drname).expect("to work"),
                input_parts.git_drname.to_owned(),
                tool_box.clone(),
                llm_broker.clone(),
                message_properties.clone(),
            )
            .await;

        // we can critique and provide feedback to the approach the agent took over here
        // this will allow us to stop the agent from making the same mistakes again and again
        // - first get the git-diff which we have generated over here after making our edits
        // - run the test which we know for sure will lead to insights over here
        // - get the trajectory the agent followed
        // - critique and catch the failure hypothesis over we can give that feedback
        // to the agent so it does not repeat the same mistakes again
        let git_diff_changes = tool_box
            .use_terminal_command("git diff", message_properties.clone())
            .await
            .expect("harness to not fail for git-diff");
        let test_output = tool_box
            .run_tests(test_files.to_vec(), message_properties.clone())
            .await
            .expect("harness to run the test");
        // exit code == 0 implies success, no test failures
        if test_output.exit_code() == 0 {
            break;
        }
        let trajectory = session_service
            .get_agent_trajectory(storage_path.to_owned(), tool_box.clone())
            .await
            .expect("trajectory to be always present");

        // now given these set of inputs we run our feedback agent whose job is to look
        // at the test output and the trajectory we took and figure out the limitations
        // of the current approach
    }
    Ok(())
}
