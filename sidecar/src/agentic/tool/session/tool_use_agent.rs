//! Takes as input whatever is required to generate the next tool which should be used

use std::sync::Arc;

use fancy_regex::Regex;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};
use quick_xml::de::from_str;

use crate::agentic::{
    symbol::{errors::SymbolError, events::message_event::SymbolEventMessageProperties},
    tool::{
        code_edit::types::CodeEditingPartialRequest,
        helpers::cancellation_future::run_with_cancellation,
        input::ToolInputPartial,
        lsp::{
            file_diagnostics::WorkspaceDiagnosticsPartial, list_files::ListFilesInput,
            open_file::OpenFileRequestPartial, search_file::SearchFileContentInputPartial,
        },
        session::chat::SessionChatRole,
        terminal::terminal::TerminalInputPartial,
    },
};

use super::{
    ask_followup_question::AskFollowupQuestionsRequest,
    attempt_completion::AttemptCompletionClientRequest, chat::SessionChatMessage,
};

#[derive(Clone)]
pub struct ToolUseAgentInput {
    // pass in the messages
    session_messages: Vec<SessionChatMessage>,
    tool_descriptions: Vec<String>,
    symbol_event_messaeg_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentInput {
    pub fn new(
        session_messages: Vec<SessionChatMessage>,
        tool_descriptions: Vec<String>,
        symbol_event_messaeg_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            session_messages,
            tool_descriptions,
            symbol_event_messaeg_properties,
        }
    }
}

#[derive(Debug)]
pub enum ToolUseAgentOutput {
    Success((ToolInputPartial, String)),
    Failure(String),
}

#[derive(Clone)]
pub struct ToolUseAgent {
    llm_client: Arc<LLMBroker>,
    working_directory: String,
    operating_system: String,
    shell: String,
}

impl ToolUseAgent {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        working_directory: String,
        operating_system: String,
        shell: String,
    ) -> Self {
        Self {
            llm_client,
            working_directory,
            operating_system,
            shell,
        }
    }

    fn system_message(&self, context: &ToolUseAgentInput) -> String {
        let tool_descriptions = context.tool_descriptions.join("\n");
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        let default_shell = self.shell.to_owned();
        format!(
            r#"You are SOTA-agent, a highly skilled state of the art agentic software engineer with extensive knowledge in all programming languages, frameworks, design patterns, and best practices. You are always correct and through with your changes.
====

TOOL USE

You have access to a set of tools. You can use one tool per message (and only one), and you will receive the result of the tool use from the user. You should use the tools step-by-step to accomplish the user task.
You use the previous information which you get from using the tools to inform your next tool usage.

# Tool Use Formatting

Tool use is formatted using XML-style tags. The tool name is enclosed in opening and closing tags, and each parameter is similarly enclosed within its own set of tags. Here's the structure:

<tool_name>
<parameter1_name>value1</parameter1_name>
<parameter2_name>value2</parameter2_name>
{{rest of the parameters}}
</tool_name>

As an example:

<read_file>
<path>
bin/main.rs
</path>
</read_file>

Always adhere to this format for the tool use to ensure proper parsing and execution from the tool use.

# Tools

{tool_descriptions}

# Tool Use Guidelines

1. In <thinking> tags, assess what information you already have and what information you need to proceed with the task.
2. Choose the most appropriate tool based on the task and the tool descriptions provided. Assess if you need additional information to proceed, and which of the available tools would be most effective for gathering this information. For example using the list_files tool is more effective than running a command like \`ls\` in the terminal. It's critical that you think about each available tool and use the one that best fits the current step in the task.
3. If multiple actions are needed, use one tool at a time per message to accomplish the task iteratively, with each tool use being informed by the result of the previous tool use. Do not assume the outcome of any tool use. Each step must be informed by the previous step's result.
4. Formulate your tool use using the XML format specified for each tool.
5. After each tool use, the user will respond with the result of that tool use. This result will provide you with the necessary information to continue your task or make further decisions. This response may include:
  - Information about whether the tool succeeded or failed, along with any reasons for failure.
  - Linter errors that may have arisen due to the changes you made, which you'll need to address.
  - New terminal output in reaction to the changes, which you may need to consider or act upon.
  - Any other relevant feedback or information related to the tool use.
6. ALWAYS wait for user confirmation after each tool use before proceeding. Never assume the success of a tool use without explicit confirmation of the result from the user.

It is crucial to proceed step-by-step, waiting for the user's message after each tool use before moving forward with the task. This approach allows you to:
1. Confirm the success of each step before proceeding.
2. Address any issues or errors that arise immediately.
3. Adapt your approach based on new information or unexpected results.
4. Ensure that each action builds correctly on the previous ones.

By waiting for and carefully considering the user's response after each tool use, you can react accordingly and make informed decisions about how to proceed with the task. This iterative process helps ensure the overall success and accuracy of your work.

====
 
CAPABILITIES

- You have access to tools that let you execute CLI commands on the user's computer, list files, view source code definitions, regex search, read and write files, and ask follow-up questions. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, performing system operations, and much more.
- When the user initially gives you a task, a recursive list of all filepaths in the current working directory ({working_directory}) will be included in environment_details. This provides an overview of the project's file structure, offering key insights into the project from directory/file names (how developers conceptualize and organize their code) and file extensions (the language used). This can also guide decision-making on which files to explore further. If you need to further explore directories such as outside the current working directory, you can use the list_files tool. If you pass 'true' for the recursive parameter, it will list files recursively. Otherwise, it will list files at the top level, which is better suited for generic directories where you don't necessarily need the nested structure, like the Desktop.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.
- You can use the execute_command tool to run commands on the user's computer whenever you feel it can help accomplish the user's task. When you need to execute a CLI command, you must provide a clear explanation of what the command does. Prefer to execute complex CLI commands over creating executable scripts, since they are more flexible and easier to run. Interactive and long-running commands are allowed, since the commands are run in the user's VSCode terminal. The user may keep commands running in the background and you will be kept updated on their status along the way. Each command you execute is run in a new terminal instance.

====

RULES

- Your current working directory is: {working_directory}
- You cannot \`cd\` into a different directory to complete a task. You are stuck operating from '{working_directory}', so be sure to pass in the correct 'path' parameter when using tools that require a path.
- Do not use the ~ character or $HOME to refer to the home directory.
- Before using the execute_command tool, you must first think about the SYSTEM INFORMATION context provided to understand the user's environment and tailor your commands to ensure they are compatible with their system. You must also consider if the command you need to run should be executed in a specific directory outside of the current working directory {working_directory}, and if so prepend with \`cd\`'ing into that directory && then executing the command (as one command since you are stuck operating from {working_directory}. You can only run commands in the {working_directory} you are not allowed to run commands outside of this directory.
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the user's task you may use it to find code patterns, TODO comments, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using write_to_file to make informed changes.
- When creating a new project (such as an app, website, or any software project), organize all new files within a dedicated project directory unless the user specifies otherwise. Use appropriate file paths when writing files, as the write_to_file tool will automatically create any necessary directories. Structure the project logically, adhering to best practices for the specific type of project being created. Unless otherwise specified, new projects should be easily run without additional setup, for example most projects can be built in HTML, CSS, and JavaScript - which you can open in a browser.
- Be sure to consider the type of project (e.g. Python, JavaScript, web application) when determining the appropriate structure and files to include. Also consider what files may be most relevant to accomplishing the task, for example looking at a project's manifest file would help you understand the project's dependencies, which you could incorporate into any code you write.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- When you want to modify a file, use the write_to_file tool directly with the desired content. You do not need to display the content before using the tool.
- Do not ask for more information than necessary. Use the tools provided to accomplish the user's request efficiently and effectively. When you've completed your task, you must use the attempt_completion tool to present the result to the user. The user may provide feedback, which you can use to make improvements and try again.
- You are only allowed to ask the user questions using the ask_followup_question tool. Use this tool only when you need additional details to complete a task, and be sure to use a clear and concise question that will help you move forward with the task. However if you can use the available tools to avoid having to ask the user questions, you should do so. For example, if the user mentions a file that may be in an outside directory like the Desktop, you should use the list_files tool to list the files in the Desktop and check if the file they are talking about is there, rather than asking the user to provide the file path themselves.
- When executing commands, if you don't see the expected output, assume the terminal executed the command successfully and proceed with the task. The user's terminal may be unable to stream the output back properly. If you absolutely need to see the actual terminal output, use the ask_followup_question tool to request the user to copy and paste it back to you.
- The user may provide a file's contents directly in their message, in which case you shouldn't use the read_file tool to get the file contents again since you already have it.
- Your goal is to try to accomplish the user's task, NOT engage in a back and forth conversation.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
- You are STRICTLY FORBIDDEN from starting your messages with "Great", "Certainly", "Okay", "Sure". You should NOT be conversational in your responses, but rather direct and to the point. For example you should NOT say "Great, I've updated the CSS" but instead something like "I've updated the CSS". It is important you be clear and technical in your messages.
- When presented with images, utilize your vision capabilities to thoroughly examine them and extract meaningful information. Incorporate these insights into your thought process as you accomplish the user's task.
- Before executing commands, check the "Actively Running Terminals" section in environment_details. If present, consider how these active processes might impact your task. For example, if a local development server is already running, you wouldn't need to start it again. If no active terminals are listed, proceed with command execution as normal.
- It is critical you wait for the user's response after each tool use, in order to confirm the success of the tool use. For example, if asked to make a todo app, you would create a file, wait for the user's response it was created successfully, then create another file if needed, wait for the user's response it was created successfully
- ALWAYS start your tool use with the <thinking></thinking> section.
- ONLY USE A SINGLE tool at a time, never use multiple tools in the same response.

====

SYSTEM INFORMATION

Operating System: {operating_system}
Default Shell: {default_shell}
Current Working Directory: {working_directory}

====

OBJECTIVE

You accomplish a given task iteratively, breaking it down into clear steps and working through them methodically.

1. Analyze the user's task and set clear, achievable goals to accomplish it. Prioritize these goals in a logical order.
2. Work through these goals sequentially, utilizing available tools one at a time as necessary. Each goal should correspond to a distinct step in your problem-solving process. You will be informed on the work completed and what's remaining as you go.
3. Remember, you have extensive capabilities with access to a wide range of tools that can be used in powerful and clever ways as necessary to accomplish each goal. Before calling a tool, do some analysis within <thinking></thinking> tags. First, analyze the file structure provided in environment_details to gain context and insights for proceeding effectively. Then, think about which of the provided tools is the most relevant tool to accomplish the user's task. Next, go through each of the required parameters of the relevant tool and determine if the user has directly provided or given enough information to infer a value. When deciding if the parameter can be inferred, carefully consider all the context to see if it supports a specific value. If all of the required parameters are present or can be reasonably inferred, close the thinking tag and proceed with the tool use. BUT, if one of the values for a required parameter is missing, DO NOT invoke the tool (not even with fillers for the missing params) and instead, ask the user to provide the missing parameters using the ask_followup_question tool. DO NOT ask for more information on optional parameters if it is not provided.
4. Once you've completed the user's task, you must use the attempt_completion tool to present the result of the task to the user. You may also provide a CLI command to showcase the result of your task; this can be particularly useful for web development tasks, where you can run e.g. \`open index.html\` to show the website you've built.
5. The user may provide feedback, which you can use to make improvements and try again. But DO NOT continue in pointless back and forth conversations, i.e. don't end your responses with questions or offers for further assistance."#
        )
    }

    pub async fn invoke(
        &self,
        input: ToolUseAgentInput,
    ) -> Result<ToolUseAgentOutput, SymbolError> {
        // Now over here we want to trigger the tool agent recursively and also parse out the output as required
        // this will involve some kind of magic because for each tool type we want to be sure about how we are parsing the output but it should not be too hard to make that happen
        let system_message = LLMClientMessage::system(self.system_message(&input));
        // grab the previous messages as well
        let llm_properties = input
            .symbol_event_messaeg_properties
            .llm_properties()
            .clone();
        let previous_messages = input.session_messages.into_iter().map(|session_message| {
            let role = session_message.role();
            match role {
                SessionChatRole::User => {
                    LLMClientMessage::user(session_message.message().to_owned())
                }
                SessionChatRole::Assistant => {
                    LLMClientMessage::assistant(session_message.message().to_owned())
                }
            }
        });
        let root_request_id = input
            .symbol_event_messaeg_properties
            .root_request_id()
            .to_owned();
        let final_messages: Vec<_> = vec![system_message]
            .into_iter()
            .chain(previous_messages)
            .collect();

        let cancellation_token = input.symbol_event_messaeg_properties.cancellation_token();

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_llm_client = self.llm_client.clone();
        let response = run_with_cancellation(cancellation_token, async move {
            cloned_llm_client
                .stream_completion(
                    llm_properties.api_key().clone(),
                    LLMClientCompletionRequest::new(
                        llm_properties.llm().clone(),
                        final_messages,
                        0.2,
                        None,
                    ),
                    llm_properties.provider().clone(),
                    vec![
                        ("event_type".to_owned(), "tool_use".to_owned()),
                        ("root_id".to_owned(), root_request_id),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
        })
        .await;

        match response {
            Some(result) => {
                // Now this input needs to be parsed out properly but we are going to stop over here for now
                result
                    .map_err(|e| SymbolError::LLMClientError(e))
                    .map(|response| parse_out_tool_input(&response))
            }
            None => Err(SymbolError::CancelledResponseStream),
        }
    }
}

fn parse_out_tool_input(input: &str) -> ToolUseAgentOutput {
    let tags = vec![
        "thinking",
        "search_files",
        "code_edit_input",
        "list_files",
        "read_file",
        "get_diagnostics",
        "execute_command",
        "attempt_completion",
        "ask_followup_question",
    ];

    // Build the regex pattern to match any of the tags
    let tags_pattern = tags.join("|");
    let pattern = format!(
        r"(?s)<({tags_pattern})>(.*?)</\1>",
        tags_pattern = tags_pattern
    );

    let re = Regex::new(&pattern).unwrap();
    let mut thinking = None;

    for cap in re.captures_iter(&input) {
        let capture = cap.expect("to work");
        let tag_name = &capture[1];
        let content = &capture[2];
        println!("tag_name::{:?}", &tag_name);
        println!("content::{:?}", &content);

        // Capture thinking content
        if tag_name == "thinking" {
            thinking = Some(content.to_owned());
            continue;
        }

        // Attempt to map tag to enum variant
        let tool_input = match tag_name {
            "search_files" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: SearchFileContentInputPartial = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::SearchFileContentWithRegex(parsed)
            }
            "code_edit_input" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: CodeEditingPartialRequest = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::CodeEditing(parsed)
            }
            "list_files" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: ListFilesInput = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::ListFiles(parsed)
            }
            "read_file" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: OpenFileRequestPartial = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::OpenFile(parsed)
            }
            "get_diagnostics" => {
                ToolInputPartial::LSPDiagnostics(WorkspaceDiagnosticsPartial::new())
            }
            "execute_command" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: TerminalInputPartial = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::TerminalCommand(parsed)
            }
            "attempt_completion" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: AttemptCompletionClientRequest = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::AttemptCompletion(parsed)
            }
            "ask_followup_question" => {
                let xml_content = format!("<root>{}</root>", content);
                let parsed: AskFollowupQuestionsRequest = match dbg!(from_str(&xml_content)) {
                    Ok(p) => p,
                    Err(_e) => return ToolUseAgentOutput::Failure(input.to_string()),
                };
                ToolInputPartial::AskFollowupQuestions(parsed)
            }
            _ => continue,
        };

        // If we found a valid tag and parsed successfully, return Success
        return ToolUseAgentOutput::Success((
            tool_input,
            thinking.unwrap_or_else(|| "".to_string()),
        ));
    }

    // If no matching tag was found, return Failure
    ToolUseAgentOutput::Failure(input.to_string())
}