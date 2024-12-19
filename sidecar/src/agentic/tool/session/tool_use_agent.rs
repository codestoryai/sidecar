//! Takes as input whatever is required to generate the next tool which should be used

use std::sync::Arc;

use futures::StreamExt;
use llm_client::{
    broker::LLMBroker,
    clients::{
        anthropic::AnthropicClient,
        open_router::OpenRouterClient,
        types::{LLMClientCompletionRequest, LLMClientMessage},
    },
};

use crate::agentic::{
    symbol::{
        errors::SymbolError, events::message_event::SymbolEventMessageProperties,
        ui_event::UIEventWithID,
    },
    tool::{
        code_edit::{code_editor::CodeEditorParameters, types::CodeEditingPartialRequest},
        errors::ToolError,
        helpers::cancellation_future::run_with_cancellation,
        input::ToolInputPartial,
        lsp::{
            file_diagnostics::WorkspaceDiagnosticsPartial, list_files::ListFilesInput,
            open_file::OpenFileRequestPartial, search_file::SearchFileContentInputPartial,
        },
        r#type::ToolType,
        repo_map::generator::RepoMapGeneratorRequestPartial,
        session::chat::SessionChatRole,
        terminal::terminal::TerminalInputPartial,
        test_runner::runner::TestRunnerRequestPartial,
    },
};

use super::{
    ask_followup_question::AskFollowupQuestionsRequest,
    attempt_completion::AttemptCompletionClientRequest, chat::SessionChatMessage,
};

#[derive(Clone)]
pub struct ToolUseAgentInputOnlyTools {
    session_messages: Vec<SessionChatMessage>,
    tools: Vec<serde_json::Value>,
    problem_statement: String,
    is_midwit_mode: bool,
    pending_spawned_process_output: Option<String>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentInputOnlyTools {
    pub fn new(
        session_messages: Vec<SessionChatMessage>,
        tools: Vec<serde_json::Value>,
        problem_statement: String,
        is_midwit_mode: bool,
        pending_spawned_process_output: Option<String>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            session_messages,
            tools,
            problem_statement,
            is_midwit_mode,
            pending_spawned_process_output,
            symbol_event_message_properties,
        }
    }
}

#[derive(Clone)]
pub struct ToolUseAgentInput {
    // pass in the messages
    session_messages: Vec<SessionChatMessage>,
    tool_descriptions: Vec<String>,
    pending_spawned_process_output: Option<String>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentInput {
    pub fn new(
        session_messages: Vec<SessionChatMessage>,
        tool_descriptions: Vec<String>,
        pending_spawned_process_output: Option<String>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            session_messages,
            tool_descriptions,
            pending_spawned_process_output,
            symbol_event_message_properties,
        }
    }
}

#[derive(Debug)]
pub enum ToolUseAgentOutputWithTools {
    /// How to understand this data:
    /// Vec<(String, ToolInputPartial)> -> Vec<(tool_use_id, tool_input_params)>
    /// String -> thinking string
    Success((Vec<(String, ToolInputPartial)>, String)),
    /// Option<String> -> If we were able to get the thinking string for the tool use
    Failure(Option<String>),
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
    swe_bench_repo_name: Option<String>,
}

impl ToolUseAgent {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        working_directory: String,
        operating_system: String,
        shell: String,
        swe_bench_repo_name: Option<String>,
    ) -> Self {
        Self {
            llm_client,
            working_directory,
            operating_system,
            shell,
            swe_bench_repo_name,
        }
    }

    fn system_message_midwit_json_with_notes(&self) -> String {
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        let shell = self.shell.to_owned();
        format!(
            r#"You are an expert software engineer taked with helping the developer.
You know in detail everything about this repository and all the different code structures which are present in it source code for it.

<uploaded_files>
{working_directory}
</uploaded_files>
I've uploaded a python code repository in the directory {working_directory} (not in /tmp/inputs).

Can you help me implement the necessary changes to the repository so that the requirements specified by the user are met?
I've also setup the developer environment in {working_directory}.

Your task is to make the minimal changes to files in the {working_directory} directory to ensure the developer is satisfied.

Tool capabilities:
- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the developer needs you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis.
- Once a file has been created using `create` on `str_replace_editor` tool, you should not keep creating the same file again and again. Focus on editing the file after it has been created.
- You can run long running terminal commands which can run in the background, we will present you with the updated logs. This can be useful if the user wants you to start a debug server in the terminal and then look at the logs or other long running processes.

====

SYSTEM INFORMATION

Operating System: {operating_system}
Default Shell: {shell}
Current Working Directory: {working_directory}

====

FOLLOW these steps to resolve the issue:
1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Open the file called notes.txt where you have previously taken notes about the repository. This will be useful for you to understand what is going on in the repository. You should reuse and make sure the knowledge here is upto date with the repository. Keep making changes to this to keep the notes up to date with your work as well.
3. Edit the sourcecode of the repo to resolve the issue, your job is to make minimal changes.

Your thinking should be thorough and so it's fine if it's very long.
This is super important and before using any tool you have to output your thinking in <thinking> section like this:'
<thinking>
{{your thoughts about using the tool}}
</thinking>
NEVER forget to include the <thinking></thinking> section before using a tool. We will not be able to invoke the tool properly if you forget it"#
        )
    }

    fn system_message_midwit_json_mode(&self, repo_name: &str, problem_statement: &str) -> String {
        let working_directory = self.working_directory.to_owned();
        format!(
            r#"You are an expert software engineer taked with solving the <pr_description> the I am going to provide. You are an expert at {repo_name} and you will be given a list of tools which you can use one after the other to debug and fix the <pr_description>.
You are an expert in {repo_name} and know in detail everything about this repository and all the different code structures which are present in it source code for it.

<uploaded_files>
{working_directory}
</uploaded_files>
I've uploaded a python code repository in the directory {working_directory} (not in /tmp/inputs). Consider the following PR description:

<pr_description>
{problem_statement}
</pr_description>

Can you help me implement the necessary changes to the repository {repo_name} so that the requirements specified in the <pr_description> are met?
I've already taken care of all changes to any of the test files described in the <pr_description>. This means you DON'T have to modify the testing logic or any of the tests in any way!
I've also setup the developer environment in {working_directory} for {repo_name}. This means you DON'T have to install any new libraries in any way!

Your task is to make the minimal changes to non-tests files in the {working_directory} directory to ensure the <pr_description> is satisfied.

Tool capabilities:
- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis.
- Once a file has been created using `create` on `str_replace_editor` tool, you should not keep creating the same file again and again. Focus on editing the file after it has been created.

====

SYSTEM INFORMATION

Operating System: linux
Default Shell: bash
Current Working Directory: {working_directory}
Current Repo Name: {repo_name}

====

FOLLOW these steps to resolve the issue:
1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script called reproduce_error.py to reproduce the error and execute it with *`python reproduce_error.py`*, to confirm the error. It is very important that you create the reproduce_error.py script FIRST before executing it.
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. Think about edgecases and make sure your fix handles them as well

Your thinking should be thorough and so it's fine if it's very long.
This is super important and before using any tool you have to output your thinking in <thinking> section like this:'
<thinking>
{{your thoughts about using the tool}}
</thinking>
NEVER forget to include the <thinking></thinking> section before using a tool. We will not be able to invoke the tool properly if you forget it"#
        )
    }

    fn system_message_for_swe_bench_json_mode(&self, repo_name: &str) -> String {
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        format!(
            r#"You are an expert software engineer tasked with solving Github issues which the user will provide. You are an expert at {repo_name} and you will be given a list of tools which you can use one after the other to debug and fix the issue.
I have already taken care of all changes to any test files described in {working_directory}. This means you DON'T have to modify the testing logic or any of the tests in any way!
Your task is to make the minimal changes to non-tests files in the {working_directory} directory to ensure the Github Issue is satisfied.
====

TOOL USE

You have access to a set of tools. You can use one tool per message (and only one), and you will receive the result of the tool use from the user. You should use the tools step-by-step to accomplish the user task.
You use the previous information which you get from using the tools to inform your next tool usage.
You should always output the <thinking></thinking> section before using a tool

# Tool Use Guidelines

1. In <thinking> tags, assess what information you already have and what information you need to proceed with the task. Your thinking should be thorough and so it's fine if it's very long.
2. Choose the most appropriate tool based on the task and the tool descriptions provided. Assess if you need additional information to proceed, and which of the available tools would be most effective for gathering this information. For example using the list_files tool is more effective than running a command like \`ls\` in the terminal. It's critical that you think about each available tool and use the one that best fits the current step in the task.
3. If multiple actions are needed, use one tool at a time per message to accomplish the task iteratively, with each tool use being informed by the result of the previous tool use. Do not assume the outcome of any tool use. Each step must be informed by the previous step's result.

It is crucial to proceed step-by-step, waiting for the tool output after each tool use before moving forward with the task.

By waiting for and carefully considering the tool output after each tool use, you can react accordingly and make informed decisions about how to proceed with the task. This iterative process helps ensure the overall success and accuracy of your work.

====
 
CAPABILITIES

- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- The code_edit tool also allows you to implicilty create a new file and write content to it. You can use it to edit the code or create a new file and write content to it.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.

====

RULES

- Your current working directory is: {working_directory}
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- Use the tools provided to accomplish the Github Issue efficiently and effectively. When you've completed solving the issue, you must use the attempt_completion tool to present the result to the user.
- Your goal is to solve the Github Issue be laser focussed on that.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
- ALWAYS start your tool use with the <thinking></thinking> section.
- ONLY USE A SINGLE tool at a time, never use multiple tools in the same response.

====

SYSTEM INFORMATION

Operating System: {operating_system}
Default Shell: bash
Current Working Directory: {working_directory}
Current Repo Name: {repo_name}

====

OBJECTIVE

You are an expert software engineer taked with solving Github issues which the user will provide, breaking it down into clear steps and working through them methodically.
Your first goal should be to reproduce the issue which you can then run using `python reproduce_error.py` using the execute_command to confirm the error, you can put prints to deeply understand the issue.
You are an expert in {repo_name} and know in detail everything about this repository and all the different code structures which are present in it source code for it.


You are NOT ALLOWED to create or edit any of the test-files. You can only run them to check for regressions.
You are NOT ALLOWED to install any new packages. The dev environment has already been setup for you before you run any command or the reproduce_error.py script.

1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script to reproduce the error and execute it with `python reproduce_error.py` using the execute_command (which uses bash internally), to confirm the error
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. Think about edgecases and make sure your fix handles them as well.
6. You can ONLY USE 1 TOOL in each step and not multiple tools, using multiple tools is not allowed.
7. ONLY ATTEMPT COMPLETION if you have finished with your round of edits.
8. TEST files need to be run using the test_runner command, while the reproduce_error script can be run only using the execute_command tool.
9. Run test files at the very end so you can catch any regressions in your solution. Some test output might be wrong or conflict the Github Issue so carefully understand the test file and the outcome before commiting to making more changes based on the test output.
10. NEVER forget to include the <thinking></thinking> section before using a tool. We will not be able to invoke the tool properly if you forget it."#
        )
    }

    fn system_message_for_swe_bench(&self, context: &ToolUseAgentInput, repo_name: &str) -> String {
        let tool_descriptions = context.tool_descriptions.join("\n");
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        let default_shell = self.shell.to_owned();
        format!(
            r#"You are an expert software engineer tasked with solving Github issues which the user will provide. You are an expert at {repo_name} and you will be given a list of tools which you can use one after the other to debug and fix the issue.
I have already taken care of all changes to any test files described in {working_directory}. This means you DON'T have to modify the testing logic or any of the tests in any way!
Your task is to make the minimal changes to non-tests files in the {working_directory} directory to ensure the Github Issue is satisfied.
====

TOOL USE

You have access to a set of tools. You can use one tool per message (and only one), and you will receive the result of the tool use from the user. You should use the tools step-by-step to accomplish the user task.
You use the previous information which you get from using the tools to inform your next tool usage.
You should always output the <thinking></thinking> section before using a tool and we are showing you an example

# Tool Use Formatting

Tool use is formatted using XML-style tags. The tool name is enclosed in opening and closing tags, and each parameter is similarly enclosed within its own set of tags. Each tag is on a new line. Here's the structure:

<tool_name>
<parameter1_name>
value1
</parameter1_name>
<parameter2_name>
value2
</parameter2_name>
{{rest of the parameters}}
</tool_name>

As an example:
<thinking>
I want to read the content of bin/main.rs
</thinking>
<read_file>
<fs_file_path>
bin/main.rs
</fs_file_path>
</read_file>

Always adhere to this format for the tool use to ensure proper parsing and execution from the tool use.

# Tools - do not use tools which are not listed here

{tool_descriptions}

# Tool Use Guidelines

1. In <thinking> tags, assess what information you already have and what information you need to proceed with the task. Your thinking should be thorough and so it's fine if it's very long.
2. Choose the most appropriate tool based on the task and the tool descriptions provided. Assess if you need additional information to proceed, and which of the available tools would be most effective for gathering this information. For example using the list_files tool is more effective than running a command like \`ls\` in the terminal. It's critical that you think about each available tool and use the one that best fits the current step in the task.
3. If multiple actions are needed, use one tool at a time per message to accomplish the task iteratively, with each tool use being informed by the result of the previous tool use. Do not assume the outcome of any tool use. Each step must be informed by the previous step's result.
4. Formulate your tool use using the XML format specified for each tool.
5. After each tool use, the user will respond with the result of that tool use. This result will provide you with the necessary information to continue your task or make further decisions. This response may include:
  - Information about whether the tool succeeded or failed, along with any reasons for failure.
  - Any other relevant feedback or information related to the tool use.

It is crucial to proceed step-by-step, waiting for the user's message after each tool use before moving forward with the task.

By waiting for and carefully considering the user's response after each tool use, you can react accordingly and make informed decisions about how to proceed with the task. This iterative process helps ensure the overall success and accuracy of your work.

====
 
CAPABILITIES

- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- The code_edit tool also allows you to implicilty create a new file and write content to it. You can use it to edit the code or create a new file and write content to it.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.

====

RULES

- Your current working directory is: {working_directory}
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, TODO comments, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- Use the tools provided to accomplish the Github Issue efficiently and effectively. When you've completed solving the issue, you must use the attempt_completion tool to present the result to the user.
- Your goal is to solve the Github Issue be laser focussed on that.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
- ALWAYS start your tool use with the <thinking></thinking> section.
- ONLY USE A SINGLE tool at a time, never use multiple tools in the same response.
- VERY IMPORTANT: Each xml tag should be on a new line. This is important because we are parsing the input line by line.

====

SYSTEM INFORMATION

Operating System: {operating_system}
Default Shell: {default_shell}
Current Working Directory: {working_directory}
Current Repo Name: {repo_name}

====

OBJECTIVE

You are an expert software engineer taked with solving Github issues which the user will provide, breaking it down into clear steps and working through them methodically.
Your first goal should be to reproduce the issue which you can then run using `python reproduce_error.py` using the execute_command to confirm the error, you can put prints to deeply understand the issue.
You are an expert in {repo_name} and know in detail everything about this repository and all the different code structures which are present in it source code for it.


You are NOT ALLOWED to create or edit any of the test-files. You can only run them to check for regressions.
You are NOT ALLOWED to install any new packages. The dev environment has already been setup for you before you run any command or the reproduce_error.py script.

1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script to reproduce the error and execute it with `python reproduce_error.py` using the execute_command (which uses bash internally), to confirm the error
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. Think about edgecases and make sure your fix handles them as well.
6. You can ONLY USE 1 TOOL in each step and not multiple tools, using multiple tools is not allowed.
7. ONLY ATTEMPT COMPLETION if you have finished with your round of edits.
9. TEST files need to be run using the test_runner command, while the reproduce_error script can be run only using the execute_command tool.
8. Run test files at the very end so you can catch any regressions in your solution. Some test output might be wrong or conflict the Github Issue so carefully understand the test file and the outcome before commiting to making more changes based on the test output.
10. All the XML sections for the tool use format should be in a new line, this is important because we parese the tool output line by line.
11. NEVER forget to include the <thinking></thinking> section before using a tool. We will not be able to invoke the tool properly if you forget it.
"#
        )
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

Tool use is formatted using XML-style tags. The tool name is enclosed in opening and closing tags, and each parameter is similarly enclosed within its own set of tags. Each tag is on a new line. Here's the structure:

<tool_name>
<parameter1_name>
value1
</parameter1_name>
<parameter2_name>
value2
</parameter2_name>
{{rest of the parameters}}
</tool_name>

As an example:

<read_file>
<fs_file_path>
bin/main.rs
</fs_file_path>
</read_file>

Another example:
<list_files>
<path>
.
</path>
<recursive>
true
</recursive>
</list_files>

Always adhere to this format for the tool use to ensure proper parsing and execution from the tool use. And NOTICE HOW ALL XML TAGS ARE ON A NEW LINE. This is important to not break parsing.

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
- If you have executed some terminal commands before which are long running, the user will show you that output in <executed_terminal_output></executed_terminal_output> section. This way you can stay on top of long running commands or in case you missed the output from before.
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
- Each xml tag should be on a new line. This is important because we are parsing the input line by line.

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

    /// Use this when invoking the agent for the normal tool use flow
    pub async fn invoke_json_tool_use(
        &self,
        input: ToolUseAgentInputOnlyTools,
    ) -> Result<ToolUseAgentOutputWithTools, SymbolError> {
        let system_message = LLMClientMessage::system(self.system_message_midwit_json_with_notes())
            .insert_tools(input.tools);

        // grab the previous messages as well
        let llm_properties = input
            .symbol_event_message_properties
            .llm_properties()
            .clone();
        let mut previous_messages = input
            .session_messages
            .into_iter()
            .map(|session_message| {
                let role = session_message.role();
                let tool_use = session_message.tool_use();
                match role {
                    SessionChatRole::User => {
                        LLMClientMessage::user(session_message.message().to_owned())
                            .with_images(
                                session_message
                                    .images()
                                    .into_iter()
                                    .map(|session_image| session_image.to_llm_image())
                                    .collect(),
                            )
                            .insert_tool_return_values(
                                session_message
                                    .tool_return()
                                    .into_iter()
                                    .map(|tool_return| tool_return.to_llm_tool_return())
                                    .collect(),
                            )
                    }
                    SessionChatRole::Assistant => {
                        LLMClientMessage::assistant(session_message.message().to_owned())
                            .insert_tool_use_values(
                                tool_use
                                    .into_iter()
                                    .map(|tool_use| tool_use.to_llm_tool_use())
                                    .collect(),
                            )
                    }
                }
            })
            .collect::<Vec<_>>();

        let mut cache_points_set = 0;
        let cache_points_allowed = 3;
        previous_messages
            .iter_mut()
            .rev()
            .into_iter()
            .for_each(|message| {
                if cache_points_set >= cache_points_allowed {
                    return;
                }
                if message.is_human_message() {
                    message.set_cache_point();
                    cache_points_set = cache_points_set + 1;
                }
            });

        // TODO(skcd): This will not work since we have to grab the pending spawned process output here properly
        if previous_messages
            .last()
            .map(|last_message| last_message.is_human_message())
            .unwrap_or_default()
        {
            if let Some(pending_spawned_process_output) = input.pending_spawned_process_output {
                previous_messages.push(LLMClientMessage::user(format!(
                    r#"<executed_terminal_output>
{}
</executed_terminal_output>"#,
                    pending_spawned_process_output
                )));
            }
        }

        let root_request_id = input
            .symbol_event_message_properties
            .root_request_id()
            .to_owned();
        let final_messages: Vec<_> = vec![system_message]
            .into_iter()
            .chain(previous_messages)
            .collect::<Vec<_>>();

        let cancellation_token = input.symbol_event_message_properties.cancellation_token();

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_root_request_id = root_request_id.to_owned();
        let response = run_with_cancellation(
            cancellation_token.clone(),
            tokio::spawn(async move {
                if llm_properties.provider().is_anthropic_api_key() {
                    AnthropicClient::new()
                        .stream_completion_with_tool(
                            llm_properties.api_key().clone(),
                            LLMClientCompletionRequest::new(
                                llm_properties.llm().clone(),
                                final_messages,
                                0.2,
                                None,
                            ),
                            // llm_properties.provider().clone(),
                            vec![
                                ("event_type".to_owned(), "tool_use".to_owned()),
                                ("root_id".to_owned(), cloned_root_request_id),
                            ]
                            .into_iter()
                            .collect(),
                            sender,
                        )
                        .await
                } else {
                    OpenRouterClient::new()
                        .stream_completion_with_tool(
                            llm_properties.api_key().clone(),
                            LLMClientCompletionRequest::new(
                                llm_properties.llm().clone(),
                                final_messages,
                                0.2,
                                None,
                            ),
                            // llm_properties.provider().clone(),
                            vec![
                                ("event_type".to_owned(), "tool_use".to_owned()),
                                ("root_id".to_owned(), cloned_root_request_id),
                            ]
                            .into_iter()
                            .collect(),
                            sender,
                        )
                        .await
                }
            }),
        )
        .await;

        println!("tool_use_agent::invoke_json_tool");
        if let Some(Ok(Ok(response))) = response {
            println!("tool_use_agent::invoke_json_tool::reply({:?})", &response);
            // we will have a string here representing the thinking and another with the various tool inputs and their json representation
            let thinking = response.0;
            let tool_inputs = response.1;
            let mut tool_inputs_parsed = vec![];
            for (tool_type, tool_input) in tool_inputs.into_iter() {
                let tool_use_id = tool_input.0;
                let tool_input = tool_input.1;
                let tool_input = match tool_type.as_ref() {
                    "list_files" => ToolInputPartial::ListFiles(
                        serde_json::from_str::<ListFilesInput>(&tool_input).map_err(|_e| {
                            SymbolError::ToolError(ToolError::SerdeConversionFailed)
                        })?,
                    ),
                    "search_files" => ToolInputPartial::SearchFileContentWithRegex(
                        serde_json::from_str::<SearchFileContentInputPartial>(&tool_input)
                            .map_err(|_e| {
                                SymbolError::ToolError(ToolError::SerdeConversionFailed)
                            })?,
                    ),
                    "read_file" => ToolInputPartial::OpenFile(
                        serde_json::from_str::<OpenFileRequestPartial>(&tool_input).map_err(
                            |_e| SymbolError::ToolError(ToolError::SerdeConversionFailed),
                        )?,
                    ),
                    "execute_command" => ToolInputPartial::TerminalCommand({
                        serde_json::from_str::<TerminalInputPartial>(&tool_input)
                            .map_err(|_e| SymbolError::ToolError(ToolError::SerdeConversionFailed))?
                            // well gotta do the hard things sometimes right?
                            // or the dumb things
                            .sanitise_for_repro_script()
                    }),
                    "attempt_completion" => ToolInputPartial::AttemptCompletion(
                        serde_json::from_str::<AttemptCompletionClientRequest>(&tool_input)
                            .map_err(|_e| {
                                SymbolError::ToolError(ToolError::SerdeConversionFailed)
                            })?,
                    ),
                    "test_runner" => ToolInputPartial::TestRunner(
                        serde_json::from_str::<TestRunnerRequestPartial>(&tool_input).map_err(
                            |_e| SymbolError::ToolError(ToolError::SerdeConversionFailed),
                        )?,
                    ),
                    "str_replace_editor" => ToolInputPartial::CodeEditorParameters(
                        serde_json::from_str::<CodeEditorParameters>(&tool_input).map_err(|e| {
                            println!("str_replace_editor::error::{:?}", e);
                            SymbolError::ToolError(ToolError::SerdeConversionFailed)
                        })?,
                    ),
                    _ => {
                        println!("unknow tool found: {}", tool_type);
                        return Err(SymbolError::WrongToolOutput);
                    }
                };
                tool_inputs_parsed.push((tool_use_id, tool_input));
            }

            Ok(ToolUseAgentOutputWithTools::Success((
                tool_inputs_parsed,
                // trim the string properly so we remove all the \n
                thinking.trim().to_owned(),
            )))
        } else {
            Ok(ToolUseAgentOutputWithTools::Failure(None))
        }
    }

    /// TODO(skcd): This is a special call we are using only for anthropic and nothing
    /// else right now
    pub async fn invoke_json_tool_swe_bench(
        &self,
        input: ToolUseAgentInputOnlyTools,
    ) -> Result<ToolUseAgentOutputWithTools, SymbolError> {
        let repo_name = self.swe_bench_repo_name.clone().expect("to be present");
        let problem_statement = &input.problem_statement;
        let system_message = LLMClientMessage::system(if input.is_midwit_mode {
            self.system_message_midwit_json_mode(&repo_name, problem_statement)
        } else {
            self.system_message_for_swe_bench_json_mode(&repo_name)
        })
        .insert_tools(input.tools);

        // grab the previous messages as well
        let llm_properties = input
            .symbol_event_message_properties
            .llm_properties()
            .clone();
        let mut previous_messages = input
            .session_messages
            .into_iter()
            .map(|session_message| {
                let role = session_message.role();
                let tool_use = session_message.tool_use();
                match role {
                    SessionChatRole::User => {
                        LLMClientMessage::user(session_message.message().to_owned())
                            .with_images(
                                session_message
                                    .images()
                                    .into_iter()
                                    .map(|session_image| session_image.to_llm_image())
                                    .collect(),
                            )
                            .insert_tool_return_values(
                                session_message
                                    .tool_return()
                                    .into_iter()
                                    .map(|tool_return| tool_return.to_llm_tool_return())
                                    .collect(),
                            )
                    }
                    SessionChatRole::Assistant => {
                        LLMClientMessage::assistant(session_message.message().to_owned())
                            .insert_tool_use_values(
                                tool_use
                                    .into_iter()
                                    .map(|tool_use| tool_use.to_llm_tool_use())
                                    .collect(),
                            )
                    }
                }
            })
            .collect::<Vec<_>>();

        // we want to modify 2 things here, the last user message and the one before
        // should be cached as well
        previous_messages.last_mut().map(|previous_message| {
            if previous_message.is_human_message() {
                previous_message.is_cache_point();
            }
        });

        let root_request_id = input
            .symbol_event_message_properties
            .root_request_id()
            .to_owned();
        let final_messages: Vec<_> = vec![system_message]
            .into_iter()
            .chain(previous_messages)
            .collect::<Vec<_>>();

        let cancellation_token = input.symbol_event_message_properties.cancellation_token();

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_root_request_id = root_request_id.to_owned();
        let response = run_with_cancellation(
            cancellation_token.clone(),
            tokio::spawn(async move {
                if llm_properties.provider().is_anthropic_api_key() {
                    AnthropicClient::new()
                        .stream_completion_with_tool(
                            llm_properties.api_key().clone(),
                            LLMClientCompletionRequest::new(
                                llm_properties.llm().clone(),
                                final_messages,
                                0.2,
                                None,
                            ),
                            // llm_properties.provider().clone(),
                            vec![
                                ("event_type".to_owned(), "tool_use".to_owned()),
                                ("root_id".to_owned(), cloned_root_request_id),
                            ]
                            .into_iter()
                            .collect(),
                            sender,
                        )
                        .await
                } else {
                    OpenRouterClient::new()
                        .stream_completion_with_tool(
                            llm_properties.api_key().clone(),
                            LLMClientCompletionRequest::new(
                                llm_properties.llm().clone(),
                                final_messages,
                                0.2,
                                None,
                            ),
                            // llm_properties.provider().clone(),
                            vec![
                                ("event_type".to_owned(), "tool_use".to_owned()),
                                ("root_id".to_owned(), cloned_root_request_id),
                            ]
                            .into_iter()
                            .collect(),
                            sender,
                        )
                        .await
                }
            }),
        )
        .await;

        println!("tool_use_agent::invoke_json_tool");
        if let Some(Ok(Ok(response))) = response {
            println!("tool_use_agent::invoke_json_tool::reply({:?})", &response);
            // we will have a string here representing the thinking and another with the various tool inputs and their json representation
            let thinking = response.0;
            let tool_inputs = response.1;
            let mut tool_inputs_parsed = vec![];
            for (tool_type, tool_input) in tool_inputs.into_iter() {
                let tool_use_id = tool_input.0;
                let tool_input = tool_input.1;
                let tool_input = match tool_type.as_ref() {
                    "list_files" => ToolInputPartial::ListFiles(
                        serde_json::from_str::<ListFilesInput>(&tool_input).map_err(|_e| {
                            SymbolError::ToolError(ToolError::SerdeConversionFailed)
                        })?,
                    ),
                    "search_files" => ToolInputPartial::SearchFileContentWithRegex(
                        serde_json::from_str::<SearchFileContentInputPartial>(&tool_input)
                            .map_err(|_e| {
                                SymbolError::ToolError(ToolError::SerdeConversionFailed)
                            })?,
                    ),
                    "read_file" => ToolInputPartial::OpenFile(
                        serde_json::from_str::<OpenFileRequestPartial>(&tool_input).map_err(
                            |_e| SymbolError::ToolError(ToolError::SerdeConversionFailed),
                        )?,
                    ),
                    "execute_command" => ToolInputPartial::TerminalCommand({
                        serde_json::from_str::<TerminalInputPartial>(&tool_input)
                            .map_err(|_e| SymbolError::ToolError(ToolError::SerdeConversionFailed))?
                            // well gotta do the hard things sometimes right?
                            // or the dumb things
                            .sanitise_for_repro_script()
                    }),
                    "attempt_completion" => ToolInputPartial::AttemptCompletion(
                        serde_json::from_str::<AttemptCompletionClientRequest>(&tool_input)
                            .map_err(|_e| {
                                SymbolError::ToolError(ToolError::SerdeConversionFailed)
                            })?,
                    ),
                    "test_runner" => ToolInputPartial::TestRunner(
                        serde_json::from_str::<TestRunnerRequestPartial>(&tool_input).map_err(
                            |_e| SymbolError::ToolError(ToolError::SerdeConversionFailed),
                        )?,
                    ),
                    "str_replace_editor" => ToolInputPartial::CodeEditorParameters(
                        serde_json::from_str::<CodeEditorParameters>(&tool_input).map_err(|e| {
                            println!("str_replace_editor::error::{:?}", e);
                            SymbolError::ToolError(ToolError::SerdeConversionFailed)
                        })?,
                    ),
                    _ => {
                        println!("unknow tool found: {}", tool_type);
                        return Err(SymbolError::WrongToolOutput);
                    }
                };
                tool_inputs_parsed.push((tool_use_id, tool_input));
            }

            Ok(ToolUseAgentOutputWithTools::Success((
                tool_inputs_parsed,
                // trim the string properly so we remove all the \n
                thinking.trim().to_owned(),
            )))
        } else {
            Ok(ToolUseAgentOutputWithTools::Failure(None))
        }
    }

    pub async fn invoke(
        &self,
        input: ToolUseAgentInput,
    ) -> Result<ToolUseAgentOutput, SymbolError> {
        // Now over here we want to trigger the tool agent recursively and also parse out the output as required
        // this will involve some kind of magic because for each tool type we want to be sure about how we are parsing the output but it should not be too hard to make that happen
        let system_message =
            LLMClientMessage::system(if let Some(repo_name) = self.swe_bench_repo_name.as_ref() {
                self.system_message_for_swe_bench(&input, repo_name)
            } else {
                self.system_message(&input)
            })
            .cache_point();
        // grab the previous messages as well
        let llm_properties = input
            .symbol_event_message_properties
            .llm_properties()
            .clone();
        let mut previous_messages = input
            .session_messages
            .into_iter()
            .map(|session_message| {
                let role = session_message.role();
                match role {
                    SessionChatRole::User => {
                        LLMClientMessage::user(session_message.message().to_owned()).with_images(
                            session_message
                                .images()
                                .into_iter()
                                .map(|session_image| session_image.to_llm_image())
                                .collect(),
                        )
                    }
                    SessionChatRole::Assistant => {
                        LLMClientMessage::assistant(session_message.message().to_owned())
                    }
                }
            })
            .collect::<Vec<_>>();

        // anthropic allows setting up to 4 cache points, we are going to be more
        // lax here and set 3, since system_messgae takes 1 slot
        let mut cache_points_set = 0;
        let cache_points_allowed = 3;
        previous_messages
            .iter_mut()
            .rev()
            .into_iter()
            .for_each(|message| {
                if cache_points_set >= cache_points_allowed {
                    return;
                }
                if message.is_human_message() {
                    message.set_cache_point();
                    cache_points_set = cache_points_set + 1;
                }
            });
        if previous_messages
            .last()
            .map(|last_message| last_message.is_human_message())
            .unwrap_or_default()
        {
            if let Some(pending_spawned_process_output) = input.pending_spawned_process_output {
                previous_messages.push(LLMClientMessage::user(format!(
                    r#"<executed_terminal_output>
{}
</executed_terminal_output>"#,
                    pending_spawned_process_output
                )));
            }
        }
        let root_request_id = input
            .symbol_event_message_properties
            .root_request_id()
            .to_owned();
        let ui_sender = input.symbol_event_message_properties.ui_sender();
        let exchange_id = input.symbol_event_message_properties.request_id_str();
        let final_messages: Vec<_> = vec![system_message]
            .into_iter()
            .chain(previous_messages)
            .collect();

        let cancellation_token = input.symbol_event_message_properties.cancellation_token();

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_llm_client = self.llm_client.clone();
        let cloned_root_request_id = root_request_id.to_owned();
        let response = run_with_cancellation(
            cancellation_token.clone(),
            tokio::spawn(async move {
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
                            ("root_id".to_owned(), cloned_root_request_id),
                        ]
                        .into_iter()
                        .collect(),
                        sender,
                    )
                    .await
            }),
        );

        let mut delta_receiver = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
        let (tool_update_sender, tool_update_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut tool_use_generator = ToolUseGenerator::new(tool_update_sender);

        // run this in a background thread for now
        let tool_found_token = tokio_util::sync::CancellationToken::new();
        let cloned_tool_found_token = tool_found_token.clone();
        let cloned_cancellation_token = cancellation_token.clone();
        let delta_updater_task = tokio::spawn(async move {
            while let Some(Some(stream_msg)) =
                run_with_cancellation(cloned_cancellation_token.clone(), delta_receiver.next())
                    .await
            {
                // if we have found a tool then break and flush
                if cloned_tool_found_token.is_cancelled() {
                    break;
                }
                let delta = stream_msg.delta();
                if let Some(delta) = delta {
                    tool_use_generator.add_delta(delta);
                }
            }
            // for forcing a flush, we append a \n on our own to the answer up until now
            // so that there are no remaining lines
            tool_use_generator.flush_answer();
            let thinking_for_tool = tool_use_generator.thinking;
            let tool_input_partial = tool_use_generator.tool_input_partial;
            let complete_response = tool_use_generator.answer_up_until_now;
            (thinking_for_tool, tool_input_partial, complete_response)
        });

        // now take the tool_receiver and try sending them over as a ui_sender
        // event
        let mut tool_update_receiver =
            tokio_stream::wrappers::UnboundedReceiverStream::new(tool_update_receiver);
        while let Some(Some(tool_update)) =
            run_with_cancellation(cancellation_token.clone(), tool_update_receiver.next()).await
        {
            match tool_update {
                ToolBlockEvent::ThinkingFull(thinking_up_until_now) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_thinking(
                        root_request_id.to_owned(),
                        exchange_id.to_owned(),
                        thinking_up_until_now,
                    ));
                }
                ToolBlockEvent::NoToolFound(full_output) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_not_found(
                        root_request_id.to_owned(),
                        exchange_id.to_owned(),
                        full_output,
                    ));
                }
                ToolBlockEvent::ToolFound(tool_found) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_found(
                        root_request_id.to_owned(),
                        exchange_id.to_owned(),
                        tool_found,
                    ));
                }
                ToolBlockEvent::ToolWithParametersFound => {
                    // cancel the token once we have a tool
                    tool_found_token.cancel();
                    // If we have found a tool we should break hard over here
                    break;
                }
                ToolBlockEvent::ToolParameters(tool_parameters_update) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_parameter_found(
                        root_request_id.to_owned(),
                        exchange_id.to_owned(),
                        tool_parameters_update,
                    ));
                }
            }
        }

        if let Ok((thinking_for_tool, tool_input_partial, complete_response)) =
            delta_updater_task.await
        {
            let final_output = match tool_input_partial {
                Some(tool_input_partial) => Ok(ToolUseAgentOutput::Success((
                    tool_input_partial,
                    thinking_for_tool,
                ))),
                None => Ok(ToolUseAgentOutput::Failure(complete_response)),
            };
            match response.await {
                Some(_) => final_output,
                None => Err(SymbolError::CancelledResponseStream),
            }
        } else {
            Err(SymbolError::CancelledResponseStream)
        }
    }
}

#[derive(Debug, Clone)]
enum ToolBlockStatus {
    // this is when we haven't found anything
    NoBlock,
    // this is when we find the thinking block
    Thinking,
    // this is when we found a tool use tag
    ToolUseFind,
    // once we have the start of a tool input, we go over here
    ToolFound,
    // these are all the different attributes of the tool input
    FilePathFound,
    InstructionFound,
    DirectoryPathFound,
    RecursiveFound,
    RegexPatternFound,
    FilePatternFound,
    CommandFound,
    QuestionFound,
    ResultFound,
    FilePathsFound,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolParameters {
    pub(crate) field_name: String,
    pub(crate) field_content_up_until_now: String,
    pub(crate) field_content_delta: String,
}

#[derive(Debug, Clone)]
enum ToolBlockEvent {
    ThinkingFull(String),
    ToolFound(ToolType),
    ToolWithParametersFound,
    ToolParameters(ToolParameters),
    // contains the full string of the step output since we failed to find any event
    NoToolFound(String),
}

struct ToolUseGenerator {
    answer_up_until_now: String,
    previous_answer_line_number: Option<usize>,
    tool_block_status: ToolBlockStatus,
    thinking: String,
    tool_type_possible: Option<ToolType>,
    fs_file_path: Option<String>,
    fs_file_paths: Option<Vec<String>>,
    instruction: Option<String>,
    directory_path: Option<String>,
    recursive: Option<bool>,
    regex_pattern_found: Option<String>,
    file_pattern: Option<String>,
    command: Option<String>,
    question: Option<String>,
    result: Option<String>,
    tool_input_partial: Option<ToolInputPartial>,
    sender: tokio::sync::mpsc::UnboundedSender<ToolBlockEvent>,
}

impl ToolUseGenerator {
    fn new(sender: tokio::sync::mpsc::UnboundedSender<ToolBlockEvent>) -> Self {
        Self {
            answer_up_until_now: "".to_owned(),
            previous_answer_line_number: None,
            tool_block_status: ToolBlockStatus::NoBlock,
            thinking: "".to_owned(),
            tool_type_possible: None,
            fs_file_path: None,
            fs_file_paths: None,
            instruction: None,
            directory_path: None,
            recursive: None,
            regex_pattern_found: None,
            file_pattern: None,
            command: None,
            question: None,
            result: None,
            tool_input_partial: None,
            sender,
        }
    }

    fn flush_answer(&mut self) {
        self.answer_up_until_now.push_str("\n");
        self.process_answer();
        if self.tool_input_partial.is_none() {
            let _ = self.sender.clone().send(ToolBlockEvent::NoToolFound(
                self.answer_up_until_now.to_owned(),
            ));
        }
    }

    fn add_delta(&mut self, delta: &str) {
        self.answer_up_until_now.push_str(delta);
        self.process_answer();
    }

    fn process_answer(&mut self) {
        let line_number_to_process = get_last_newline_line_number(&self.answer_up_until_now);
        if line_number_to_process.is_none() {
            return;
        }

        let line_number_to_process_until =
            line_number_to_process.expect("is_none to hold above") - 1;

        let stream_lines = self.answer_up_until_now.to_owned();
        let stream_lines = stream_lines.lines().into_iter().collect::<Vec<_>>();

        let start_index = self
            .previous_answer_line_number
            .map_or(0, |line_number| line_number + 1);

        for line_number in start_index..=line_number_to_process_until {
            println!(
                "{:?}::{}",
                &self.tool_block_status, &stream_lines[line_number]
            );
            self.previous_answer_line_number = Some(line_number);
            let answer_line_at_index = stream_lines[line_number];
            match self.tool_block_status.clone() {
                ToolBlockStatus::NoBlock => {
                    if answer_line_at_index == "<thinking>" {
                        self.tool_block_status = ToolBlockStatus::Thinking;
                    }
                }
                ToolBlockStatus::Thinking => {
                    if answer_line_at_index == "</thinking>" {
                        self.tool_block_status = ToolBlockStatus::ToolUseFind;
                    } else {
                        if self.thinking.is_empty() {
                            self.thinking = answer_line_at_index.to_owned();
                        } else {
                            self.thinking.push_str("\n");
                            self.thinking.push_str(answer_line_at_index);
                        }
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ThinkingFull(self.thinking.to_owned()));
                    }
                }
                ToolBlockStatus::ToolUseFind => {
                    if answer_line_at_index == "<search_files>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::SearchFileContentWithRegex);
                        let _ = self.sender.send(ToolBlockEvent::ToolFound(
                            ToolType::SearchFileContentWithRegex,
                        ));
                    } else if answer_line_at_index == "<code_edit_input>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::CodeEditing);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::CodeEditing));
                    } else if answer_line_at_index == "<list_files>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::ListFiles);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::ListFiles));
                    } else if answer_line_at_index == "<read_file>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::OpenFile);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::OpenFile));
                    } else if answer_line_at_index == "<get_diagnostics>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::FileDiagnostics);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::FileDiagnostics));
                    } else if answer_line_at_index == "<execute_command>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::TerminalCommand);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::TerminalCommand));
                    } else if answer_line_at_index == "<attempt_completion>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::AttemptCompletion);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::AttemptCompletion));
                    } else if answer_line_at_index == "<ask_followup_question>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::AskFollowupQuestions);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::AskFollowupQuestions));
                    } else if answer_line_at_index == "<repo_map_generation>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::RepoMapGeneration);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::RepoMapGeneration));
                        // these are the ending condition over here
                        // we grab all the fields which are required and then return them back over here
                    } else if answer_line_at_index == "<test_runner>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::TestRunner);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::TestRunner));
                    }
                }
                ToolBlockStatus::ToolFound => {
                    if answer_line_at_index == "<fs_file_path>" {
                        self.tool_block_status = ToolBlockStatus::FilePathFound;
                    } else if answer_line_at_index == "<instruction>" {
                        self.tool_block_status = ToolBlockStatus::InstructionFound;
                    } else if answer_line_at_index == "<directory_path>" {
                        self.tool_block_status = ToolBlockStatus::DirectoryPathFound;
                    } else if answer_line_at_index == "<recursive>" {
                        self.tool_block_status = ToolBlockStatus::RecursiveFound;
                    } else if answer_line_at_index == "<regex_pattern>" {
                        self.tool_block_status = ToolBlockStatus::RegexPatternFound;
                    } else if answer_line_at_index == "<file_pattern>" {
                        self.tool_block_status = ToolBlockStatus::FilePatternFound;
                    } else if answer_line_at_index == "<command>" {
                        self.tool_block_status = ToolBlockStatus::CommandFound;
                    } else if answer_line_at_index == "<question>" {
                        self.tool_block_status = ToolBlockStatus::QuestionFound;
                    } else if answer_line_at_index == "<result>" {
                        self.tool_block_status = ToolBlockStatus::ResultFound;
                    } else if answer_line_at_index == "<fs_file_paths>" {
                        self.tool_block_status = ToolBlockStatus::FilePathsFound;
                    } else if answer_line_at_index == "</search_files>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match (
                            self.directory_path.clone(),
                            self.regex_pattern_found.clone(),
                        ) {
                            (Some(directory_path), Some(regex_pattern)) => {
                                self.tool_input_partial =
                                    Some(ToolInputPartial::SearchFileContentWithRegex(
                                        SearchFileContentInputPartial::new(
                                            directory_path,
                                            regex_pattern,
                                            self.file_pattern.clone(),
                                        ),
                                    ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</code_edit_input>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match (self.fs_file_path.clone(), self.instruction.clone()) {
                            (Some(fs_file_path), Some(instruction)) => {
                                self.tool_input_partial = Some(ToolInputPartial::CodeEditing(
                                    CodeEditingPartialRequest::new(fs_file_path, instruction),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</list_files>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match (self.directory_path.clone(), self.recursive.clone()) {
                            (Some(directory_path), Some(recursive)) => {
                                self.tool_input_partial = Some(ToolInputPartial::ListFiles(
                                    ListFilesInput::new(directory_path, recursive),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</read_file>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.fs_file_path.clone() {
                            Some(fs_file_path) => {
                                self.tool_input_partial = Some(ToolInputPartial::OpenFile(
                                    OpenFileRequestPartial::new(fs_file_path),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</get_diagnostics>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        self.tool_input_partial = Some(ToolInputPartial::LSPDiagnostics(
                            WorkspaceDiagnosticsPartial::new(),
                        ));
                        let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</execute_command>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.command.clone() {
                            Some(command) => {
                                self.tool_input_partial = Some(ToolInputPartial::TerminalCommand(
                                    TerminalInputPartial::new(command.to_owned()),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</attempt_completion>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.result.clone() {
                            Some(result) => {
                                self.tool_input_partial =
                                    Some(ToolInputPartial::AttemptCompletion(
                                        AttemptCompletionClientRequest::new(
                                            result,
                                            self.command.clone(),
                                        ),
                                    ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</ask_followup_question>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.question.clone() {
                            Some(question) => {
                                self.tool_input_partial =
                                    Some(ToolInputPartial::AskFollowupQuestions(
                                        AskFollowupQuestionsRequest::new(question),
                                    ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</repo_map_generation>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.directory_path.clone() {
                            Some(directory_path) => {
                                self.tool_input_partial =
                                    Some(ToolInputPartial::RepoMapGeneration(
                                        RepoMapGeneratorRequestPartial::new(directory_path),
                                    ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</test_runner>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        self.tool_type_possible = None;
                        match self.fs_file_paths.clone() {
                            Some(fs_file_paths) => {
                                self.tool_input_partial = Some(ToolInputPartial::TestRunner(
                                    TestRunnerRequestPartial::new(fs_file_paths),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                    }
                }
                ToolBlockStatus::FilePathFound => {
                    if answer_line_at_index == "</fs_file_path>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        self.fs_file_path = Some(answer_line_at_index.to_owned());
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "fs_file_path".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::FilePathsFound => {
                    if answer_line_at_index == "</fs_file_paths>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        let mut fs_file_paths = self.fs_file_paths.clone().unwrap_or(vec![]);
                        fs_file_paths.push(answer_line_at_index.to_owned());
                        self.fs_file_paths = Some(fs_file_paths);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "fs_file_paths".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::InstructionFound => {
                    if answer_line_at_index == "</instruction>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.instruction.clone() {
                            Some(instruction) => {
                                let new_instruction = instruction + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "instruction".to_owned(),
                                        field_content_up_until_now: new_instruction.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.instruction = Some(new_instruction);
                            }
                            None => self.instruction = Some(answer_line_at_index.to_owned()),
                        }
                    }
                }
                ToolBlockStatus::DirectoryPathFound => {
                    if answer_line_at_index == "</directory_path>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        self.directory_path = Some(answer_line_at_index.to_owned());
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "directory_path".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::RecursiveFound => {
                    if answer_line_at_index == "</recursive>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        let recursive_value = answer_line_at_index.parse::<bool>().unwrap_or(false);
                        self.recursive = Some(recursive_value);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "recursive".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::RegexPatternFound => {
                    if answer_line_at_index == "</regex_pattern>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.regex_pattern_found.clone() {
                            Some(existing_pattern) => {
                                let new_pattern =
                                    existing_pattern.clone() + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "regex_pattern".to_owned(),
                                        field_content_up_until_now: new_pattern.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.regex_pattern_found = Some(new_pattern);
                            }
                            None => {
                                self.regex_pattern_found = Some(answer_line_at_index.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "regex_pattern".to_owned(),
                                        field_content_up_until_now: answer_line_at_index.to_owned(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                            }
                        }
                    }
                }
                ToolBlockStatus::FilePatternFound => {
                    if answer_line_at_index == "</file_pattern>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        self.file_pattern = Some(answer_line_at_index.to_owned());
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "file_pattern".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::CommandFound => {
                    if answer_line_at_index == "</command>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.command.clone() {
                            Some(command) => {
                                let new_command = command.clone() + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "command".to_owned(),
                                        field_content_up_until_now: new_command.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.command = Some(new_command);
                            }
                            None => {
                                self.command = Some(answer_line_at_index.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "command".to_owned(),
                                        field_content_up_until_now: answer_line_at_index.to_owned(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                            }
                        }
                    }
                }
                ToolBlockStatus::QuestionFound => {
                    if answer_line_at_index == "</question>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.question.clone() {
                            Some(question) => {
                                let new_question = question.clone() + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "question".to_owned(),
                                        field_content_up_until_now: new_question.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.question = Some(new_question);
                            }
                            None => {
                                self.question = Some(answer_line_at_index.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "question".to_owned(),
                                        field_content_up_until_now: answer_line_at_index.to_owned(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                            }
                        }
                    }
                }
                ToolBlockStatus::ResultFound => {
                    if answer_line_at_index == "</result>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.result.clone() {
                            Some(result) => {
                                let new_result = result.clone() + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "result".to_owned(),
                                        field_content_up_until_now: new_result.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.result = Some(new_result);
                            }
                            None => {
                                self.result = Some(answer_line_at_index.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "result".to_owned(),
                                        field_content_up_until_now: answer_line_at_index.to_owned(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Helps to get the last line number which has a \n
fn get_last_newline_line_number(s: &str) -> Option<usize> {
    s.rfind('\n')
        .map(|last_index| s[..=last_index].chars().filter(|&c| c == '\n').count())
}

#[cfg(test)]
mod tests {
    use super::ToolUseGenerator;

    #[test]
    fn test_make_tool_parsing_work() {
        let input = r#"<thinking>
I need to first locate and read the Tool trait definition. Based on the context, it's likely in one of the Rust source files. Let me search for it.
</thinking>

<search_files>
<directory_path>
/Users/skcd/test_repo/sidecar
</directory_path>
<regex_pattern>
trait\s+Tool\s*\{
</regex_pattern>
<file_pattern>
*.rs
</file_pattern>
</search_files>"#;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut tool_use_generator = ToolUseGenerator::new(sender);
        tool_use_generator.add_delta(&input);
        tool_use_generator.flush_answer();

        let tool_use_possible = tool_use_generator.tool_input_partial;
        assert!(tool_use_possible.is_some());
    }
}
