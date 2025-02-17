//! Agent Binary Module
//! 
//! This binary is responsible for running AI agents in a farm-like setup.
//! It processes input from git repositories and executes problem-solving
//! workflows using configured LLM providers. The implementation focuses
//! on simplicity and reliability.
//!
//! Key features:
//! - Command-line interface for configuration
//! - Integration with LLM providers (Anthropic, OpenRouter)
//! - Session-based execution tracking
//! - Configurable logging and monitoring
//! - Support for different execution modes (JSON, midwit)

use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use llm_client::{
    clients::types::LLMType,
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use sidecar::{
    agentic::symbol::{
        events::{input::SymbolEventRequestId, message_event::SymbolEventMessageProperties},
        identifier::LLMProperties,
    },
    application::{application::Application, config::configuration::Configuration},
    repo::types::RepoRef,
    user_context::types::UserContext,
};

/// Creates and returns the session storage path for a given session ID.
/// 
/// This function ensures that the session storage directory exists and creates it if necessary.
/// The path is constructed by joining the index directory with "session" and the provided session ID.
///
/// # Arguments
/// * `config` - Application configuration containing the index directory
/// * `session_id` - Unique identifier for the session
///
/// # Returns
/// A string containing the absolute path to the session storage directory
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

/// Command-line argument structure for configuring the agent binary.
/// Contains all necessary parameters for setting up and running the agent.
#[derive(Parser, Debug)]
#[command(
    author = "skcd",
    version = "1.0",
    about = "Agent binary sidecar runner"
)]
struct CliArgs {
    /// Maximum time in seconds for the agent to run
    #[arg(long)]
    timeout: usize,

    /// URL endpoint for the editor service
    #[arg(long)]
    editor_url: String,

    /// Path to the input file containing problem configuration
    #[arg(long)]
    input: PathBuf,

    /// API key for Anthropic services
    #[arg(long, default_value = None)]
    anthropic_api_key: String,

    /// API key for OpenRouter services (optional)
    #[arg(long, default_value = None)]
    openrouter_api_key: Option<String>,

    /// Unique identifier for the current execution run
    #[arg(long)]
    run_id: String,

    /// Name of the repository being processed
    #[arg(long)]
    repo_name: String,

    /// Directory path for storing execution logs
    #[arg(long)]
    log_directory: String,

    /// Enable strict JSON mode for communication
    #[arg(long, default_value = "true")]
    json_mode: bool,

    /// Enable midwit mode (sonnet3.5 with tool)
    #[arg(long, default_value = "true")]
    midwit_mode: bool,

    /// Number of trajectories for single trajectory search mode
    #[arg(long, default_value = None)]
    single_traj_search: Option<usize>,

    /// Maximum depth limit for the search tree
    #[arg(long, default_value = "30")]
    max_depth: u32,

    /// Override for the default LLM model name
    #[arg(long)]
    model_name: Option<String>,
}

/// Represents a benchmark instance for software engineering tasks.
/// Contains all necessary information about a repository, problem statement,
/// and test configurations.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SWEbenchInstance {
    /// Repository URL or identifier
    repo: String,
    /// Unique identifier for this benchmark instance
    instance_id: String,
    /// Base commit hash to start from
    base_commit: String,
    /// Patch to be applied
    patch: String,
    /// Test patch to be applied
    test_patch: String,
    /// Description of the problem to be solved
    problem_statement: String,
    /// Additional hints for solving the problem
    hints_text: String,
    /// Creation timestamp
    created_at: String,
    /// Version identifier
    version: String,
    /// Tests that should transition from failing to passing
    #[serde(rename = "FAIL_TO_PASS")]
    fail_to_pass: String,
    /// Tests that should remain passing
    #[serde(rename = "PASS_TO_PASS")]
    pass_to_pass: String,
    /// Commit hash for environment setup
    environment_setup_commit: String,
}

/// Input structure for the agent binary that combines git directory information
/// with a benchmark instance.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InputParts {
    /// The directory name containing the git repository
    git_drname: String,
    /// The benchmark instance to be processed
    instance: SWEbenchInstance,
}
/// Main entry point for the agent binary.
/// 
/// Orchestrates the complete agent execution workflow:
/// 1. Parses and validates command line arguments
/// 2. Configures application settings and logging infrastructure
/// 3. Sets up LLM provider connections and messaging channels
/// 4. Initializes session storage and services
/// 5. Processes input configuration and problem statement
/// 6. Executes the agent with specified parameters
/// 
/// # Returns
/// Returns a Result indicating success or containing an error if execution failed
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("agent::start");
    let args = CliArgs::parse();
    eprintln!("run_id::{}", &args.run_id);

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

    let aide_rules = Some(format!(
        r#"You are helping the user in the repository present in {}
FOLLOW these steps to resolve the issue:
1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Edit the sourcecode of the repo to resolve the issue
3. Think about edgecases and make sure your fix handles them as well

Your thinking should be thorough and so it's fine if it's very long."#,
        args.repo_name,
    ));

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
            tool_box,
            llm_broker,
            UserContext::default(),
            aide_rules,
            false,
            false,
            false,
            Some(args.log_directory.clone()),
            Some(args.repo_name.clone()),
            message_properties,
            false, // not in devtools context
            None, // No context crunching LLM for agent_bin
        )
        .await;
    println!("agent::tool_use::end");
    Ok(())
}