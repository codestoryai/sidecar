use std::{path::PathBuf, sync::Arc};

/// This contains the binary responsible for running the agents as a farm
/// Dead simple where the inputs are the input to the git repository containing the input
/// and the problem statement, keeping it super simple and limited
use clap::Parser;
use llm_client::{
    clients::types::LLMType,
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use sidecar::{
    agentic::{
        symbol::{
            events::{input::SymbolEventRequestId, message_event::SymbolEventMessageProperties},
            identifier::LLMProperties,
        },
        tool::{
            r#type::ToolType,
            session::tool_use_agent::{AgentThinkingMode, ToolUseAgentProperties},
        },
    },
    application::{application::Application, config::configuration::Configuration},
    repo::types::RepoRef,
    user_context::types::UserContext,
};

pub async fn check_session_storage_path(config: Arc<Configuration>, session_id: String) -> String {
    let mut session_path = config.index_dir.clone();
    session_path = session_path.join("session");
    // check if the plan_storage_path_exists
    if tokio::fs::metadata(&session_path).await.is_err() {
        tokio::fs::create_dir(&session_path)
            .await
            .expect("directory creation to not fail");
    }
    session_path = session_path.join(session_id);
    session_path
        .to_str()
        .expect("path conversion to work on all platforms")
        .to_owned()
}

/// Define the command-line arguments
#[derive(Parser, Debug)]
#[command(
    author = "skcd",
    version = "1.0",
    about = "Agent binary sidecar runner"
)]
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
    anthropic_api_key: String,

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

    /// Model name override
    #[arg(long)]
    model_name: Option<String>,
}

/// Define the SWEbenchInstance struct for serialization
#[derive(Debug, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InputParts {
    git_drname: String,
    instance: SWEbenchInstance,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("agent::start");
    let args = CliArgs::parse();
    eprintln!("run_id::{}", &args.run_id);

    // Ensure OpenAI API key is present early
    let _openai_key =
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY environment variable must be set");

    let mut configuration = Configuration::default();
    // we apply the edits directly over here
    configuration.apply_directly = true;

    // setup the application
    Application::install_logging(&configuration);
    Application::setup_scratch_pad(&configuration).await;

    let application = Application::initialize(configuration)
        .await
        .expect("application setup should work");
    let exchange_id = "0".to_owned();

    let llm_model = if let Some(model_name) = args.model_name {
        LLMType::Custom(model_name)
    } else {
        LLMType::ClaudeSonnet
    };

    let llm_provider = LLMProperties::new(
        llm_model,
        LLMProvider::Anthropic,
        LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new(args.anthropic_api_key.to_owned())),
    );
    // Define context crunching LLM properties - using the same model as the main agent for now
    let _context_crunching_llm = Some(llm_provider.clone());
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new("0".to_owned(), args.run_id.to_owned()),
        sender.clone(),
        args.editor_url.clone(),
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(application.config.clone(), args.run_id.clone()).await;

    let session_service = application.session_service.clone();

    let input_path = args.input;
    let input_content = tokio::fs::read(input_path).await.expect("path content");
    let input_parts: InputParts =
        serde_json::from_slice(&input_content).expect("Parse the serde json");

    let cloned_session_id = args.run_id.to_string();
    let user_message = input_parts.instance.problem_statement.clone();
    let cloned_working_directory = input_parts.git_drname.to_owned();
    let tool_box = application.tool_box.clone();
    let llm_broker = application.llm_broker.clone();

    let tools = vec![
        ToolType::ListFiles,
        ToolType::SearchFileContentWithRegex,
        ToolType::OpenFile,
        ToolType::CodeEditing,
        ToolType::AttemptCompletion,
        ToolType::TerminalCommand,
        ToolType::FindFiles,
    ];

    let tool_use_agent_properties = ToolUseAgentProperties::new(
        false,
        "bash".to_owned(),
        AgentThinkingMode::MiniCOTBeforeTool,
        false, // is running under eval
        false, // test running not allowed
        args.repo_name.to_owned(),
        None,
    );

    // wait for the agent to finish over here while busy looping
    println!("agent::tool_use::start");
    let _ = session_service
        .tool_use_agentic(
            cloned_session_id,
            session_storage_path,
            user_message,
            exchange_id,
            vec![],
            vec![],
            "bash".to_owned(),
            vec![],
            RepoRef::local(&cloned_working_directory).expect("repo_ref to work"),
            cloned_working_directory,
            tools,
            tool_box,
            llm_broker,
            UserContext::default(),
            true, // turn on reasoning
            false,
            Some(args.log_directory.clone()),
            tool_use_agent_properties,
            message_properties,
            None, // No context crunching LLM for agent_bin_reasoning
        )
        .await;
    println!("agent::tool_use::end");
    Ok(())
}
