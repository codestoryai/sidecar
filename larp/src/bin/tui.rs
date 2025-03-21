use std::{
    io::{self, Stdout},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use sidecar::{
    agentic::{
        symbol::{
            events::{input::SymbolEventRequestId, message_event::SymbolEventMessageProperties},
            identifier::LLMProperties,
            ui_event::UIEventWithID,
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
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

/// Define the command-line arguments
#[derive(Parser, Debug)]
#[command(
    author = "CodeStory AI",
    version = "1.0",
    about = "Sidecar TUI - Terminal User Interface for the AI agent"
)]
struct CliArgs {
    /// Working directory path
    #[arg(long, default_value = ".")]
    working_directory: PathBuf,

    /// Anthropic API key
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    anthropic_api_key: Option<String>,

    /// OpenAI API key
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,

    /// Model to use
    #[arg(long, default_value = "claude-3-sonnet-20240229")]
    model: String,

    /// Repository name
    #[arg(long, default_value = "current")]
    repo_name: String,
}

enum InputMode {
    Normal,
    Editing,
}

struct App {
    input: String,
    input_mode: InputMode,
    messages: Vec<ChatMessage>,
    session_id: String,
    exchange_id: usize,
    sender: UnboundedSender<UIEventWithID>,
    working_directory: String,
    repo_name: String,
    is_processing: bool,
}

struct ChatMessage {
    role: String,
    content: String,
    timestamp: Instant,
}

impl App {
    fn new(
        sender: UnboundedSender<UIEventWithID>,
        working_directory: String,
        repo_name: String,
    ) -> Self {
        Self {
            input: String::new(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
            session_id: uuid::Uuid::new_v4().to_string(),
            exchange_id: 0,
            sender,
            working_directory,
            repo_name,
            is_processing: false,
        }
    }

    fn submit_message(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        // Add user message to chat history
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: self.input.clone(),
            timestamp: Instant::now(),
        });

        // Send the message to the agent
        let exchange_id = self.exchange_id.to_string();
        self.exchange_id += 1;

        let ui_event = UIEventWithID::user_message(
            self.session_id.clone(),
            exchange_id,
            self.input.clone(),
        );
        self.sender.send(ui_event).unwrap();

        // Clear the input field
        self.input.clear();
        self.is_processing = true;
    }

    fn add_agent_response(&mut self, content: String) {
        self.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content,
            timestamp: Instant::now(),
        });
        self.is_processing = false;
    }
}

async fn check_session_storage_path(config: Arc<Configuration>, session_id: String) -> String {
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

async fn setup_agent(
    args: &CliArgs,
    app: &App,
    sender: UnboundedSender<UIEventWithID>,
) -> (
    Arc<Application>,
    SymbolEventMessageProperties,
    String,
    tokio_util::sync::CancellationToken,
) {
    let mut configuration = Configuration::default();
    configuration.apply_directly = true;

    // Setup the application
    Application::install_logging(&configuration);
    Application::setup_scratch_pad(&configuration).await;

    let application = Application::initialize(configuration)
        .await
        .expect("application setup should work");

    // Determine which API key and model to use
    let llm_provider = if let Some(anthropic_key) = &args.anthropic_api_key {
        LLMProperties::new(
            LLMType::Custom(args.model.clone()),
            LLMProvider::Anthropic,
            LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new(anthropic_key.clone())),
        )
    } else if let Some(openai_key) = &args.openai_api_key {
        LLMProperties::new(
            LLMType::Gpt4O,
            LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAI(llm_client::provider::OpenAIProvider::new(
                openai_key.clone(),
            )),
        )
    } else {
        panic!("Either ANTHROPIC_API_KEY or OPENAI_API_KEY must be provided");
    };

    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let editor_url = "http://localhost:42427".to_owned();

    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new("0".to_owned(), app.session_id.clone()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(application.config.clone(), app.session_id.clone()).await;

    (
        application,
        message_properties,
        session_storage_path,
        cancellation_token,
    )
}

async fn process_agent_response(
    mut receiver: UnboundedReceiver<UIEventWithID>,
    app_sender: UnboundedSender<String>,
) {
    let mut response_buffer = String::new();

    while let Some(event) = receiver.recv().await {
        match event {
            UIEventWithID::AgentResponse {
                session_id: _,
                exchange_id: _,
                response,
                is_final: _,
            } => {
                response_buffer.push_str(&response);
                // For simplicity, we're sending each chunk immediately
                // In a more sophisticated implementation, you might want to buffer and send complete messages
                app_sender.send(response.clone()).unwrap();
            }
            UIEventWithID::AgentThinking {
                session_id: _,
                exchange_id: _,
                thinking,
            } => {
                // For debugging, you might want to show the agent's thinking process
                println!("Agent thinking: {}", thinking);
            }
            UIEventWithID::AgentToolUse {
                session_id: _,
                exchange_id: _,
                tool_name,
                tool_input,
                tool_output,
            } => {
                let tool_use_message = format!(
                    "\n\nTool: {}\nInput: {}\nOutput: {}\n\n",
                    tool_name, tool_input, tool_output
                );
                response_buffer.push_str(&tool_use_message);
                app_sender.send(tool_use_message).unwrap();
            }
            UIEventWithID::AgentComplete {
                session_id: _,
                exchange_id: _,
            } => {
                // Signal that the agent has completed its response
                app_sender.send("__COMPLETE__".to_string()).unwrap();
                response_buffer.clear();
            }
            _ => {
                // Handle other event types if needed
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(f.size());

    // Chat history
    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| {
            let role_style = if m.role == "user" {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            };

            let role_span = Span::styled(format!("{}: ", m.role), role_style);
            let content = Text::from(m.content.clone());

            ListItem::new(vec![
                Line::from(role_span),
                Line::from(""),
                Line::from(content),
                Line::from(""),
            ])
        })
        .collect();

    let messages = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_widget(messages, chunks[0]);

    // Input box
    let input = Paragraph::new(app.input.as_ref())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if app.is_processing {
                    "Input (Processing...)"
                } else {
                    "Input (Press Enter to edit, Esc to exit edit mode)"
                }),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(input, chunks[1]);

    // Show cursor when in editing mode
    if let InputMode::Editing = app.input_mode {
        f.set_cursor(
            chunks[1].x + app.input.len() as u16 + 1,
            chunks[1].y + 1,
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create channels for communication
    let (ui_sender, ui_receiver) = mpsc::unbounded_channel();
    let (app_sender, mut app_receiver) = mpsc::unbounded_channel();

    // Create app state
    let working_directory = args
        .working_directory
        .to_str()
        .unwrap_or(".")
        .to_string();
    let mut app = App::new(ui_sender.clone(), working_directory.clone(), args.repo_name.clone());

    // Setup agent components
    let (application, message_properties, session_storage_path, _cancellation_token) =
        setup_agent(&args, &app, ui_sender.clone()).await;

    // Spawn a task to process agent responses
    tokio::spawn(process_agent_response(ui_receiver, app_sender.clone()));

    // Main event loop
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Check for agent responses
        while let Ok(response) = app_receiver.try_recv() {
            if response == "__COMPLETE__" {
                app.is_processing = false;
            } else {
                // If this is the first response for this exchange, create a new message
                if !app.is_processing || app.messages.is_empty() || app.messages.last().unwrap().role != "assistant" {
                    app.add_agent_response(response);
                } else {
                    // Otherwise, append to the existing message
                    if let Some(last_message) = app.messages.last_mut() {
                        if last_message.role == "assistant" {
                            last_message.content.push_str(&response);
                        }
                    }
                }
            }
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => {
                                break;
                            }
                            KeyCode::Enter => {
                                app.input_mode = InputMode::Editing;
                            }
                            _ => {}
                        },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter if !app.is_processing => {
                                app.submit_message();

                                // Process the message with the agent
                                let session_service = application.session_service.clone();
                                let tool_box = application.tool_box.clone();
                                let llm_broker = application.llm_broker.clone();
                                let session_id = app.session_id.clone();
                                let exchange_id = (app.exchange_id - 1).to_string();
                                let user_message = app.messages.last().unwrap().content.clone();
                                let working_dir = working_directory.clone();
                                let repo_name = app.repo_name.clone();
                                let message_props = message_properties.clone();
                                let session_storage = session_storage_path.clone();

                                tokio::spawn(async move {
                                    // Define the tools the agent can use
                                    let tools = vec![
                                        ToolType::ListFiles,
                                        ToolType::SearchFileContentWithRegex,
                                        ToolType::OpenFile,
                                        ToolType::CodeEditing,
                                        ToolType::AttemptCompletion,
                                        ToolType::TerminalCommand,
                                        ToolType::FindFiles,
                                    ];

                                    // Define agent properties
                                    let aide_rules = Some(format!(
                                        r#"You are helping the user in the repository present in {}
FOLLOW these steps to resolve the issue:
1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Edit the sourcecode of the repo to resolve the issue
3. Think about edgecases and make sure your fix handles them as well

Your thinking should be thorough and so it's fine if it's very long."#,
                                        repo_name,
                                    ));

                                    let tool_use_agent_properties = ToolUseAgentProperties::new(
                                        false,
                                        "bash".to_owned(),
                                        AgentThinkingMode::MiniCOTBeforeTool,
                                        false,
                                        false,
                                        repo_name,
                                        aide_rules,
                                    );

                                    // Run the agent
                                    let _ = session_service
                                        .tool_use_agentic(
                                            session_id,
                                            session_storage,
                                            user_message,
                                            exchange_id,
                                            vec![],
                                            vec![],
                                            "bash".to_owned(),
                                            vec![],
                                            RepoRef::local(&working_dir).expect("repo_ref to work"),
                                            working_dir,
                                            tools,
                                            tool_box,
                                            llm_broker,
                                            UserContext::default(),
                                            false,
                                            false,
                                            None,
                                            tool_use_agent_properties,
                                            message_props,
                                            None,
                                        )
                                        .await;
                                });
                            }
                            KeyCode::Escape => {
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            _ => {}
                        },
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}