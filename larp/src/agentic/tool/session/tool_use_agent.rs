//! Takes as input whatever is required to generate the next tool which should be used

use std::sync::Arc;

use futures::StreamExt;
use llm_client::{
    broker::LLMBroker,
    clients::{
        anthropic::AnthropicClient,
        open_router::OpenRouterClient,
        types::{
            LLMClientCompletionRequest, LLMClientCompletionResponse, LLMClientMessage,
            LLMClientUsageStatistics, LLMType,
        },
    },
    provider::OpenAIProvider,
};

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError, events::message_event::SymbolEventMessageProperties,
            identifier::LLMProperties, ui_event::UIEventWithID,
        },
        tool::{
            code_edit::{code_editor::CodeEditorParameters, types::CodeEditingPartialRequest},
            devtools::screenshot::RequestScreenshotInputPartial,
            errors::ToolError,
            file::semantic_search::SemanticSearchParametersPartial,
            helpers::cancellation_future::run_with_cancellation,
            input::ToolInputPartial,
            lsp::{
                file_diagnostics::WorkspaceDiagnosticsPartial, find_files::FindFileInputPartial,
                list_files::ListFilesInputPartial, open_file::OpenFileRequestPartial,
                search_file::SearchFileContentInputPartial,
            },
            mcp::input::McpToolPartial,
            r#type::ToolType,
            repo_map::generator::RepoMapGeneratorRequestPartial,
            session::chat::SessionChatRole,
            terminal::terminal::TerminalInputPartial,
            test_runner::runner::TestRunnerRequestPartial,
            thinking::thinking::ThinkingPartialInput,
        },
    },
    mcts::action_node::ActionNode,
};

use super::{
    ask_followup_question::AskFollowupQuestionsRequest,
    attempt_completion::AttemptCompletionClientRequest, chat::SessionChatMessage,
};

#[derive(Clone)]
pub struct ToolUseAgentInputOnlyTools {
    session_messages: Vec<SessionChatMessage>,
    tools: Vec<serde_json::Value>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentInputOnlyTools {
    pub fn new(
        session_messages: Vec<SessionChatMessage>,
        tools: Vec<serde_json::Value>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            session_messages,
            tools,
            symbol_event_message_properties,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToolUseAgentReasoningParams {
    plan: String,
    instruction: String,
    notes: String,
}

impl ToolUseAgentReasoningParams {
    pub fn new(plan: String, instruction: String, notes: String) -> Self {
        Self {
            plan,
            instruction,
            notes,
        }
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<reasoning_input>
<plan>
{}
</plan>
<instruction>
{}
</instruction>
<notes>
{}
</notes>
</reasoning_input>"#,
            &self.plan, &self.instruction, &self.notes
        )
    }

    pub fn add_previous_notes(&mut self, notes: &str) {
        if self.notes.is_empty() {
            self.notes = notes.to_owned();
        } else {
            self.notes = format!(
                r#"{notes}
{}"#,
                self.notes
            );
        }
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    pub fn instruction(&self) -> &str {
        &self.instruction
    }

    pub fn from_response(response: &str) -> Self {
        // grab everything in the <plan> and </plan> section
        let plan = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<plan>"))
            .skip(1)
            .take_while(|line| !line.contains("</plan>"))
            .collect::<Vec<&str>>()
            .join("\n");
        // grab everything from <current_task> and </current_task> section
        let instruction = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<current_task>"))
            .skip(1)
            .take_while(|line| !line.contains("</current_task>"))
            .collect::<Vec<&str>>()
            .join("\n");
        // grab everything from <notes> and </notes> section
        let notes = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<notes>"))
            .skip(1)
            .take_while(|line| !line.contains("</notes>"))
            .collect::<Vec<&str>>()
            .join("\n");
        Self {
            plan,
            instruction,
            notes,
        }
    }
}

/// When we crunch the context we need to generate the new instruction for the
/// llm to follow
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextCrunchingInputPartial {
    summary: String,
    instruction: String,
}

impl ContextCrunchingInputPartial {
    /// TODO(skcd): We do not really need this
    pub fn to_string(&self) -> String {
        "".to_owned()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn instruction(&self) -> &str {
        &self.instruction
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToolUseAgentReasoningParamsPartial {
    user_instruction: String,
    action_nodes: Vec<ActionNode>,
    params: Option<ToolUseAgentReasoningParams>,
}

impl ToolUseAgentReasoningParamsPartial {
    // TODO(skcd): We do not really require this
    pub fn to_string(&self) -> String {
        "".to_owned()
    }

    pub fn from_params(
        params: Option<ToolUseAgentReasoningParams>,
        user_instruction: String,
    ) -> Self {
        Self {
            user_instruction,
            action_nodes: vec![],
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolUseAgentContextCrunchingInput {
    user_instruction: String,
    action_nodes: Vec<ActionNode>,
    reasoning_action_nodes: Vec<ActionNode>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentContextCrunchingInput {
    pub fn new(
        user_instruction: String,
        action_nodes: Vec<ActionNode>,
        reasoning_action_nodes: Vec<ActionNode>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            user_instruction,
            action_nodes,
            reasoning_action_nodes,
            symbol_event_message_properties,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolUseAgentReasoningInput {
    user_instruction: String,
    action_nodes: Vec<ActionNode>,
    params: Option<ToolUseAgentReasoningParams>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentReasoningInput {
    pub fn new(
        user_instruction: String,
        action_nodes: Vec<ActionNode>,
        params: Option<ToolUseAgentReasoningParams>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            user_instruction,
            action_nodes,
            params,
            symbol_event_message_properties,
        }
    }
}

#[derive(Clone)]
pub struct ToolUseAgentInput {
    // pass in the messages
    session_messages: Vec<SessionChatMessage>,
    tool_descriptions: Vec<String>,
    tool_format_reminder: Vec<String>,
    pending_spawned_process_output: Option<String>,
    symbol_event_message_properties: SymbolEventMessageProperties,
}

impl ToolUseAgentInput {
    pub fn new(
        session_messages: Vec<SessionChatMessage>,
        tool_descriptions: Vec<String>,
        tool_format_reminder: Vec<String>,
        pending_spawned_process_output: Option<String>,
        symbol_event_message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            session_messages,
            tool_descriptions,
            tool_format_reminder,
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
    // If we have a reasoning output here to guide the agent
    Reasoning(String),
}

#[derive(Debug)]
pub struct ToolUseAgentOutput {
    r#type: ToolUseAgentOutputType,
    usage_statistics: LLMClientUsageStatistics,
}

impl ToolUseAgentOutput {
    fn new(r#type: ToolUseAgentOutputType, usage_statistics: LLMClientUsageStatistics) -> Self {
        Self {
            r#type,
            usage_statistics,
        }
    }

    pub fn usage_statistics(&self) -> LLMClientUsageStatistics {
        self.usage_statistics.clone()
    }

    pub fn output_type(self) -> ToolUseAgentOutputType {
        self.r#type
    }
}

#[derive(Debug)]
pub struct ToolUseAgentOuputSuccess {
    tool_parameters: ToolInputPartial,
    thinking: String,
    tool_use_id: String,
}

impl ToolUseAgentOuputSuccess {
    pub fn new(tool_parameters: ToolInputPartial, thinking: String, tool_use_id: String) -> Self {
        Self {
            tool_parameters,
            thinking,
            tool_use_id,
        }
    }

    pub fn tool_parameters(&self) -> &ToolInputPartial {
        &self.tool_parameters
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }

    pub fn tool_use_id(&self) -> &str {
        &self.tool_use_id
    }
}

#[derive(Debug)]
pub enum ToolUseAgentOutputType {
    Success(ToolUseAgentOuputSuccess),
    Failure(String),
}

/// if the agent should use an explicit tool to think or if we should
/// a mini-cot before using a tool in the agent
#[derive(Clone)]
pub enum AgentThinkingMode {
    ToolBased,
    MiniCOTBeforeTool,
}

/// The various properties which the tool use agent can use
/// We can configure if we are in-editor and additional metadata
/// which might be present
#[derive(Clone)]
pub struct ToolUseAgentProperties {
    in_editor: bool,
    shell: String,
    // keeping this disabled for now while  we write out the prompts and run a few
    // evals on this to measure how the performance is
    thinking: AgentThinkingMode,
    // if the current agent is running under a eval harness, this helps tune the system
    // prompt for the agent appropriately
    is_eval_run: bool,
    // is test running allowed
    is_test_running_allowed: bool,
    repo_name: String,
    aide_rules: Option<String>,
}

impl ToolUseAgentProperties {
    pub fn new(
        in_editor: bool,
        shell: String,
        thinking: AgentThinkingMode,
        is_eval_run: bool,
        is_test_running_allowed: bool,
        repo_name: String,
        aide_rules: Option<String>,
    ) -> Self {
        Self {
            in_editor,
            shell,
            is_eval_run,
            is_test_running_allowed,
            thinking,
            repo_name,
            aide_rules,
        }
    }
}

#[derive(Clone)]
pub struct ToolUseAgent {
    llm_client: Arc<LLMBroker>,
    working_directory: String,
    operating_system: String,
    properties: ToolUseAgentProperties,
    temperature: f32,
    context_crunching_llm: Option<LLMProperties>,
}

impl ToolUseAgent {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        working_directory: String,
        operating_system: String,
        properties: ToolUseAgentProperties,
    ) -> Self {
        Self {
            llm_client,
            working_directory,
            operating_system,
            properties,
            // we always default to 0.2 temp to start with
            temperature: 0.2,
            context_crunching_llm: None,
        }
    }

    // should use json mode for tool calling
    pub fn is_json_mode_and_eval(&self) -> bool {
        // right now gate it behind an eval run and only when we are doing
        // tool based thinking: we provide think as a tool to the agent
        self.properties.is_eval_run
            && matches!(&self.properties.thinking, AgentThinkingMode::ToolBased)
    }

    /// Update the temperature for the tool use agent
    pub fn set_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Set the LLM properties to use for context crunching
    pub fn set_context_crunching_llm(mut self, llm_properties: Option<LLMProperties>) -> Self {
        self.context_crunching_llm = llm_properties;
        self
    }

    /// o1 message for reasoning
    fn system_message_for_o1(&self, repo_name: &str) -> String {
        let working_directory = self.working_directory.to_owned();
        format!(
            r#"Provide instructions to a junior eningeer who will be working as per your instructions to solve a user instruction.
Study the user instruction and the current code and the repository.
You will keep a high level plan and give out tasks to the junior engineer.
After the junior engineer has completed a task, they will report back to you, use that to further inform and improve your plan.
Keep refining the plan and giving out tasks to the junior engineer until the user instructions are finished.

### Repository Information

Repository Name: {repo_name}
Working Directory: {working_directory}

## Types of user instruction:
- Asking to implement a feature and has a correctness tool to verify and accomplish when the feature is finished.
- Asking to implement a feature and has no correctness tool to use.
- Bug fixing (this can be with or without a correctness tool)
- Understanding the codebase

## Rules to follow:
- You can not create a new branch on the repository or change the commit of the repository.
- You cannot access any file outside the repository directory.
- Once you have solved the user instruction, finish by not returning any instruction to the junior engineer.
- If a correctness tool is given, use it after the junior engineer has worked to sanity check their work.

## How to solve and help with the user instruction:
1. As a first step, it might be a good idea to explore the repository to familiarize yourself with its structure.
2. Edit the sourcecode of the repository to resolve the issue using the junior engineer.
3. Think about edgecases and make that the junior engineer handles them as well.

## How to leverage the junior engineer

### Junior Engineer Visibility
- You will provide instructions to a junior engineer who will do the actual work.
- The junior engineer does not see the original user instructions. They only work on the task you give them.
- The junior engineer is supposed to write the code and run terminal commands as required.


### Junior engineer Instruction Content Rules
- They are good at searching for code and understandings specific parts, using keyword search and grep.
- Be explicit in what files to edit or create, what changes to make, and commands the junior engineer should run.
- Include sample code snippets or test code (if required) for clarity and to avoid ambiguity.
- Provide context and justification for each task so the junior engineer understands why they are doing it.
- Consider any edge cases or complexities in your instructions.
- Do not reference any information from the user instructions in your instruction to the junior engineer.

## Plan generation

### Plan specifics
- You maintain a high-level plan consisting of sequential instructions.
- For each instruction, you will provide a clear task to the junior engineer.
- You can refine the plan as the engineer reports back with progress or any discoveries.
- You will see the previous plan you were working on in the <previous_plan> section, use this to inform your new plan.
- **Always keep track in the `<plan>` section of the tasks that have already been completed. Mark any finished steps as done or indicate the outcome so there is no confusion about what remains to be done.**

## Workflow

- **Identify the Problem**: Describe the user instruction in your own words (since the junior engineer won't see it).
- **Understand the codebase**: Understand the codebase you are working on and where the problem is present.
- **Break Down the Task**: Outline the tasks needed to address the problem.
- **Assign Tasks**: Provide instructions with enough detail that the junior engineer can carry them out without additional context. The junior engineer does it one by one.
- **Track Progress**: After the engineer executes a task, use the generated artifacts (opened files, code changes, terminal output) to update or refine your plan.
- **Iterate**: Continue until the user instructions is resolved.

## Notes and Reminders
- Keep any additional insights or references in <notes> sections so they're easy to refer back to later.
- The <notes> also help you keep track of the progress the junior engineer has done. This will help you plan out what has already been accomplished and insights you have learnt from the junior engineer.
- You can use the <notes> along with the steps the junior engineer has taken for your instruction to plan out the next instruction for the junior engineer.

## Current task:
- You can assign tasks to the junior engineer which are small sized and complete.
- These tasks should be short and have no ambiguity in them.
- Do not dump your whole plan as the current task to the junior engineer, they need you to guide them properly through the changes.

## Output Format Requirements

When you produce an output in response to the junior engineer's progress, include the following sections in this order:

### Plan Section

<plan>
<instruction>
{{High-level step-by-step plan}}
</instruction>
</plan>
- This is the updated plan, reflecting the overall strategy and steps to address the user problem.
- Include a brief acknowledgment of completed tasks from previous instructions so they are not repeated.

### Notes Section (if needed)

<notes>
{{Any helpful references, code snippets and insights for future steps}}
</notes>
This can contain extra details, insights and code for future use.

### Current Task Section (if needed)

<current_task>
<instruction>
{{The specific instruction the engineer should execute next}}
</instruction>
</current_task>

Direct, specific standalone task instructions for the junior engineer to execute immediately.

### Junior Engineer's Tools
They have access to:

- Bash commands (Terminal)
- A local editor to modify or create files
- Python installed on the terminal to run standalone scripts

### Repository Information

Repository Name: {repo_name}
Working Directory: {working_directory}

The junior engineer will communicate their progress after completing the instruction in the following format:

<current_instruction>
{{the instruction junior engineer is working on}}
</current_instruction>
And the steps they took to work on the instruction:
<steps>
<step>
<tool_input>
{{commands or code they ran}}
</tool_input>
<tool_output>
{{results, errors, or logs}}
</tool_output>
</step>
</steps>

This ensures you can refine your plan in <plan> and keep track of exactly which tasks have been completed and what insights have been discovered."#
        )
    }

    /// Generates the user message for o1
    fn user_message_for_o1(&self, input: ToolUseAgentReasoningInput) -> String {
        let problem_statement = input.user_instruction.to_owned();
        if let Some(params) = input.params.clone() {
            let steps = input
                .action_nodes
                .iter()
                .filter_map(|action_node| {
                    let action_input = action_node.action();
                    let observation = action_node.observation();
                    match (action_input, observation) {
                        (Some(input), Some(output)) => Some(format!(
                            r#"<step>
<tool_input>
{}
</tool_input>
<tool_output>
{}
</tool_output>
</step>"#,
                            input.to_string(),
                            output.message()
                        )),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let previous_instruction = params.instruction.clone();
            let previous_plan = params.plan.clone();
            let previous_notes = params.notes.clone();
            format!(
                r#"<user_instruction>
{}
</user_instruction>

<previous_plan>
{}
</previous_plan>

<notes>
{}
</notes>

<current_instruction>
{}
</current_instruction>

<steps>
{}
</steps>"#,
                problem_statement, previous_plan, previous_notes, previous_instruction, steps
            )
        } else {
            format!(
                r#"<user_instruction>
{}
</user_instruction>"#,
                problem_statement
            )
        }
    }

    /// The system message for midwit tool use agent, which takes xml formatted
    /// tools as input and has similar objective as to the any swe agent:
    /// - create a repo script
    /// - find the bug
    /// - fix it
    /// - rerun repo script to prove things are okay
    fn system_message_midwit_tool_mode(
        &self,
        repo_name: &str,
        context: &ToolUseAgentInput,
    ) -> String {
        let tool_descriptions = context.tool_descriptions.to_vec().join("\n\n");
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
<start_line>
1
</start_line>
<end_line>
250
</end_line>
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

# Tools provided

{tool_descriptions}

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
- You can use grep_string to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.

====

RULES

- Your current working directory is: {working_directory}
- When using the grep_string tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
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


You are NOT ALLOWED to create or edit any of the test-files. The test-files are NOT RUNNABLE.
You are NOT ALLOWED to install any new packages. The dev environment has already been setup for you before you run any command or the reproduce_error.py script.

1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script to reproduce the error and execute it with `python reproduce_error.py` using the execute_command (which uses bash internally), to confirm the error
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. Think about edgecases and make sure your fix handles them as well.
6. You can ONLY USE 1 TOOL in each step and not multiple tools, using multiple tools is not allowed.
7. ONLY ATTEMPT COMPLETION if you have finished with your round of edits.
8. Run test files at the very end so you can catch any regressions in your solution. Some test output might be wrong or conflict the Github Issue so carefully understand the test file and the outcome before commiting to making more changes based on the test output.
9. NEVER forget to include the <thinking></thinking> section before using a tool. We will not be able to invoke the tool properly if you forget it."#
        )
    }

    fn system_message_for_swe_bench_json_mode_testing_allowed(&self, repo_name: &str) -> String {
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        format!(
            r#"You are an expert software engineer tasked with solving Github issues which the user will provide given in <pr_description>. You are an expert at {repo_name} and you will be given a list of tools which you can use one after the other to debug and fix the issue.
I have already setup the development environment for you in {working_directory}. This means you DON'T have to install any packages or do any work for running tests, you can use the `test_runner` tool directly!
Your task is to make the minimal changes to non-tests files in the {working_directory} directory to ensure the Github Issue is satisfied.

<uploaded_files>
{working_directory}
</uploaded_files>

====

TOOL USE

You have access to a set of tools. You can use one tool per message (and only one), and you will receive the result of the tool use from the user. You should use the tools step-by-step to accomplish the user task.
You use the previous information which you get from using the tools to inform your next tool usage.

# Tool Use Guidelines

1. Choose the most appropriate tool based on the task and the tool descriptions provided. Assess if you need additional information to proceed, and which of the available tools would be most effective for gathering this information. For example using the list_files tool is more effective than running a command like \`ls\` in the terminal. It's critical that you think about each available tool and use the one that best fits the current step in the task.
2. If multiple actions are needed, use one tool at a time per message to accomplish the task iteratively, with each tool use being informed by the result of the previous tool use. Do not assume the outcome of any tool use. Each step must be informed by the previous step's result.

# Test Runner Guidelines

Below is a good and a bad example for the test runner tool
<good_example>
test_runner src/test_file.py
</good_example>

<bad_example>
test_runner src/test_file.py::test_function
</bad_example>
The test_runner tool will ONLY work with complete file paths and will fail if you try to specify individual test functions

It is crucial to proceed step-by-step, waiting for the tool output after each tool use before moving forward with the task.

By waiting for and carefully considering the tool output after each tool use, you can react accordingly and make informed decisions about how to proceed with the task. This iterative process helps ensure the overall success and accuracy of your work.

====

CAPABILITIES

- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- The code_edit_input tool also allows you to implicilty create a new file and write content to it. You can use it to edit the code or create a new file and write content to it.
- You can use grep_string to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.
- Use the find_file tool to locate file using their names, this is more effecient than writing a grep command by hand to find some files.
- Use the test_runner tool to run tests which are present in the file. Make sure that the file exists before using this tool to run the tests.

====

RULES

- Your current working directory is: {working_directory}
- When using the grep_string tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- Use the tools provided to accomplish the Github Issue efficiently and effectively. When you've completed solving the issue, you must use the attempt_completion tool to present the result to the user.
- Your goal is to solve the Github Issue be laser focussed on that.
- You must run the `python reproduce_error.py` script and the relevant tests using the `test_runner` tool to ensure that you have solved the Github Issue and there are no regressions in the code you have edited.
- Use the test_runner tool to run tests which are present in the file. IMPORTANT: The test_runner tool ONLY accepts full file paths (e.g., test_file.py) and CANNOT run individual tests (e.g., test_file.py::test_function). Always run all tests in a file.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
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
Your first goal should be to reproduce the issue which you can then run using `python reproduce_error.py` using the execute_command to confirm the error, you can put prints to deeply understand the issue. Make sure the script exits with exit code 0 on success and 1 on failure.
Also ensure that you run the relevant tests after you have made the fixes using the `test_runner` tool. This is important to ensure that there are no regressions in the code changes you have made for the fix.
You are an expert in {repo_name} and know in detail everything about this repository and all the different code structures which are present in it source code for it.

1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script to reproduce the error and execute it with `python reproduce_error.py` using the execute_command (which uses bash internally), to confirm the error
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. RUN relevant tests present in the repository to make sure there are no regressions. There might be unrelated test failures which you can ignore. 
5. Think about edgecases and make sure your fix handles them as well.
6. You can ONLY USE 1 TOOL in each step and not multiple tools, using multiple tools is not allowed.
7. ONLY ATTEMPT COMPLETION if you have finished with your round of edits and made sure that the issue has been fixed. To ensure that the issue has been fixed run the `python reproduce_error.py` script and the relevant tests."#
        )
    }

    fn system_message_for_swe_bench_json_mode(&self, repo_name: &str) -> String {
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        format!(
            r#"You are an expert software engineer tasked with solving Github issues which the user will provide given in <pr_description>. You are an expert at {repo_name} and you will be given a list of tools which you can use one after the other to debug and fix the issue.
I have already taken care of all changes to any test files described in {working_directory}. This means you DON'T have to modify the testing logic or any of the tests in any way!
Your task is to make the minimal changes to non-tests files in the {working_directory} directory to ensure the Github Issue is satisfied.

<uploaded_files>
{working_directory}
</uploaded_files>

====

TOOL USE

You have access to a set of tools. You can use one tool per message (and only one), and you will receive the result of the tool use from the user. You should use the tools step-by-step to accomplish the user task.
You use the previous information which you get from using the tools to inform your next tool usage.

# Tool Use Guidelines

1. Choose the most appropriate tool based on the task and the tool descriptions provided. Assess if you need additional information to proceed, and which of the available tools would be most effective for gathering this information. For example using the list_files tool is more effective than running a command like \`ls\` in the terminal. It's critical that you think about each available tool and use the one that best fits the current step in the task.
2. If multiple actions are needed, use one tool at a time per message to accomplish the task iteratively, with each tool use being informed by the result of the previous tool use. Do not assume the outcome of any tool use. Each step must be informed by the previous step's result.

It is crucial to proceed step-by-step, waiting for the tool output after each tool use before moving forward with the task.

By waiting for and carefully considering the tool output after each tool use, you can react accordingly and make informed decisions about how to proceed with the task. This iterative process helps ensure the overall success and accuracy of your work.

====

CAPABILITIES

- You have access to tools that let you execute CLI commands on the local checkout, list files, view source code definitions, regex search, read and write files. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, and much more.
- The code_edit_input tool also allows you to implicilty create a new file and write content to it. You can use it to edit the code or create a new file and write content to it.
- You can use grep_string to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.

====

RULES

- Your current working directory is: {working_directory}
- When using the grep_string tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the Github Issue you may use it to find code patterns, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- Use the tools provided to accomplish the Github Issue efficiently and effectively. When you've completed solving the issue, you must use the attempt_completion tool to present the result to the user.
- Your goal is to solve the Github Issue be laser focussed on that.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
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
Your first goal should be to reproduce the issue which you can then run using `python reproduce_error.py` using the execute_command to confirm the error, you can put prints to deeply understand the issue. Make sure the script exits with exit code 0 on success and 1 on failure.
You are an expert in {repo_name} and know in detail everything about this repository and all the different code structures which are present in it source code for it.


1. As a first step, it might be a good idea to explore the repo to familiarize yourself with its structure.
2. Create a script to reproduce the error and execute it with `python reproduce_error.py` using the execute_command (which uses bash internally), to confirm the error
3. Edit the sourcecode of the repo to resolve the issue
4. Rerun your reproduce script and confirm that the error is fixed!
5. Think about edgecases and make sure your fix handles them as well.
6. You can ONLY USE 1 TOOL in each step and not multiple tools, using multiple tools is not allowed.
7. ONLY ATTEMPT COMPLETION if you have finished with your round of edits."#
        )
    }

    fn system_message_for_context_crunching(&self) -> String {
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        let default_shell = self.properties.shell.to_owned();
        let repo_name = self.properties.repo_name.to_owned();
        format!(
            r#"**Role:**
You are a senior engineer tasked with reviewing and summarizing the work an AI agent has completed so far. Your summary ensures that you remain on track with the task.

**Task Objectives:**
- **Summarize the Work:** Combine your understanding of the task with insights and steps the AI agent has taken. Ensure the summary is detailed and accurate.
- **Provide Feedback:** Offer strategic guidance to help the agent move forward effectively.
- **Avoid Duplicates:** Strongly discourage repeating any work that might be considered a duplicate and help the agent avoid getting stuck in repetitive loops.

**Context Information:**
- The AI agent’s progress is provided in individual steps.
- Each step’s output is enclosed in `<step></step>` tags.
= You write summary everytime the AI agent shows you their work, the history of the summary is present in <previous_summary></previous_summary>
- Each item inside <previous_summary> is indexed by the order in which you wrote them.
- The current AI agent's steps are based on your LAST summary.
- The repository name is {repo_name}.
- The operating system is {operating_system}.
- The working directory is {working_directory}.
- The shell used is {default_shell}.

**Instructions:**
1. **Think First:** Take a moment to reflect on all the provided details regarding the agent’s progress.
2. **Generate a Detailed Summary:** Create a comprehensive summary that captures:
    - The work the agent has completed so far.
    - Key steps and insights from the agent’s outputs.
    - Any potential areas where the agent might be duplicating work or getting stuck in loops.
    - The current state of the task.
3. **Enrich the Instructions:** Based on your summary, modify or enrich the original user instruction if necessary. The revised instructions must:
    - Provide clear guidance for the agent to continue effectively.
    - Avoid any duplication or repetitive cycles.
    - Remain faithful to the original user task.
    - If the original user task asks to iterate against a `test`, `linter error` or a `binary` make sure that the binary exits with the right condition when the agent works on your instructions

**Output Format:**
Your final output must strictly adhere to the following format:

<thinking>
{{Your thoughts on the agent’s progress}}
</thinking>
<summarize>
<summary>
{{A comprehensive summary of the steps taken by the agent}}
</summary>
<instruction>
{{Revised or enriched instructions for the AI agent, ensuring the task’s original intent remains unchanged}}
</instruction>
</summarize>"#
        )
    }

    fn user_message_for_context_crunching(
        &self,
        context: &ToolUseAgentContextCrunchingInput,
    ) -> String {
        let user_instruction = context.user_instruction.to_owned();
        let running_summary = context
            .reasoning_action_nodes
            .iter()
            // make everything type safe over here
            .filter_map(|action_node| {
                if action_node
                    .action()
                    .map(|action| action.to_tool_type())
                    .flatten()
                    == Some(ToolType::ContextCrunching)
                {
                    action_node
                        .action()
                        .map(|action| action.context_crunching_summary())
                        .flatten()
                } else {
                    None
                }
            })
            .enumerate()
            .map(|(idx, action_node)| {
                format!(
                    r#"<summary idx={}>
{}
</summary>"#,
                    idx, action_node
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let steps = context
            .action_nodes
            .iter()
            .filter_map(|action_node| {
                let action_input = action_node.action();
                let observation = action_node.observation();
                match (action_input, observation) {
                    (Some(input), Some(output)) => Some(format!(
                        r#"<step>
<tool_input>
{}
</tool_input>
<tool_output>
{}
</tool_output>
</step>"#,
                        input.to_string(),
                        output.message()
                    )),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "<user_instruction>
{user_instruction}
</user_instruction>

<previous_summary>
{running_summary}
</previous_summary>

<steps>
{steps}
</steps>"
        )
    }

    fn system_message(&self, context: &ToolUseAgentInput) -> String {
        let tool_descriptions = context.tool_descriptions.join("\n\n");
        let working_directory = self.working_directory.to_owned();
        let operating_system = self.operating_system.to_owned();
        let in_editor_message = if self.properties.in_editor {
            "\n- You are working in an editor, DO NOT CYCLE through various suggestions or improvements, pick the first one and then finish working".to_owned()
        } else {
            "".to_owned()
        };
        let aide_rules = match self.properties.aide_rules.clone() {
            Some(aide_rules) => {
                format!(
                    "

====

Additional guildelines and rules the user has provided which must be followed:
{aide_rules}{in_editor_message}

===="
                )
            }
            None => "".to_owned(),
        };
        let default_shell = self.properties.shell.to_owned();
        format!(
            r#"You are SOTA-agent, a highly skilled AI software engineer with extensive knowledge in all programming languages, frameworks, design patterns, and best practices. Your primary goal is to accomplish tasks related to software development, file manipulation, and system operations within the specified project directory.

====

SYSTEM INFORMATION

Operating System: {operating_system}
Default Shell: {default_shell}
Current Working Directory: {working_directory}

====
{aide_rules}

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
<start_line>
1
</start_line>
<end_line>
250
</end_line>
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

- You have access to tools that let you execute CLI or shell commands on the user's computer, list files, view source code definitions, regex search, read and write files, and ask follow-up questions. These tools help you effectively accomplish a wide range of tasks, such as writing code, making edits or improvements to existing files, understanding the current state of a project, performing system operations, and much more.
- To further explore directories such as outside the current working directory, you can use the list_files tool. If you pass 'true' for the recursive parameter, it will list files recursively. Otherwise, it will list files at the top level, which is better suited for generic directories where you don't necessarily need the nested structure, like the Desktop.
- You can use search_files to perform regex searches across files in a specified directory, outputting context-rich results that include surrounding lines. This is particularly useful for understanding code patterns, finding specific implementations, or identifying areas that need refactoring.
- You can use the execute_command tool to run commands on the user's computer whenever you feel it can help accomplish the user's task. When you need to execute a CLI command, you must provide a clear explanation of what the command does. Prefer to execute complex CLI commands over creating executable scripts, since they are more flexible and easier to run. Interactive and long-running commands are allowed, since the commands are run in the user's VSCode terminal. The user may keep commands running in the background and you will be kept updated on their status along the way. Each command you execute is run in a new terminal instance.

====

RULES

- Your current working directory is: {working_directory}
- You cannot \`cd\` into a different directory to complete a task. You are stuck operating from '{working_directory}', so be sure to pass in the correct 'path' parameter when using tools that require a path.
- Do not use the ~ character or $HOME to refer to the home directory.
- If you have executed some terminal commands before which are long running, the user will show you that output in <executed_terminal_output></executed_terminal_output> section. This way you can stay on top of long running commands or in case you missed the output from before.
- Before using the execute_command tool, you must first think about the SYSTEM INFORMATION context provided to understand the user's environment and tailor your commands to ensure they are compatible with their system. You must also consider if the command you need to run should be executed in a specific directory outside of the current working directory {working_directory}, and if so prepend with \`cd\`'ing into that directory && then executing the command (as one command since you are stuck operating from {working_directory}. You can only run commands in the {working_directory} you are not allowed to run commands outside of this directory.
- When using the search_files tool, craft your regex patterns carefully to balance specificity and flexibility. Based on the user's task you may use it to find code patterns, TODO comments, function definitions, or any text-based information across the project. The results include context, so analyze the surrounding code to better understand the matches. Leverage the search_files tool in combination with other tools for more comprehensive analysis. For example, use it to find specific code patterns, then use read_file to examine the full context of interesting matches before using code_edit_input to make informed changes.
- When creating a new project (such as an app, website, or any software project), organize all new files within a dedicated project directory unless the user specifies otherwise. Use ABSOLUTE FILE PATHS when writing files, as the code_edit_input tool will automatically create any necessary directories. Structure the project logically, adhering to best practices for the specific type of project being created. Unless otherwise specified, new projects should be easily run without additional setup, for example most projects can be built in HTML, CSS, and JavaScript - which you can open in a browser.
- Be sure to consider the type of project (e.g. Python, JavaScript, web application) when determining the appropriate structure and files to include. Also consider what files may be most relevant to accomplishing the task, for example looking at a project's manifest file would help you understand the project's dependencies, which you could incorporate into any code you write.
- When making changes to code, always consider the context in which the code is being used. Ensure that your changes are compatible with the existing codebase and that they follow the project's coding standards and best practices.
- When you want to modify a file, use the code_edit_input tool directly with the desired content. You do not need to display the content before using the tool.
- Do not ask for more information than necessary. Use the tools provided to accomplish the user's request efficiently and effectively. When you've completed your task, you must use the attempt_completion tool to present the result to the user. The user may provide feedback, which you can use to make improvements and try again.
- You are only allowed to ask the user questions using the ask_followup_question tool. Use this tool only when you need additional details to complete a task, and be sure to use a clear and concise question that will help you move forward with the task. However if you can use the available tools to avoid having to ask the user questions, you should do so. For example, if the user mentions a file that may be in an outside directory like the Desktop, you should use the list_files tool to list the files in the Desktop and check if the file they are talking about is there, rather than asking the user to provide the file path themselves.
- When executing commands, if you don't see the expected output, assume the terminal executed the command successfully and proceed with the task. The user's terminal may be unable to stream the output back properly. If you absolutely need to see the actual terminal output, use the ask_followup_question tool to request the user to copy and paste it back to you.
- The user may provide a file's contents directly in their message, in which case you shouldn't use the read_file tool to get the file contents again since you already have it.
- Your goal is to try to accomplish the user's task, NOT engage in a back and forth conversation.
- NEVER end attempt_completion result with a question or request to engage in further conversation! Formulate the end of your result in a way that is final and does not require further input from the user.
- It is critical you wait for the user's response after each tool use, in order to confirm the success of the tool use. For example, if asked to make a todo app, you would create a file, wait for the user's response it was created successfully, then create another file if needed, wait for the user's response it was created successfully
- ALWAYS start your tool use with the <thinking></thinking> section.
- ONLY USE A SINGLE tool at a time, never use multiple tools in the same response.
- Each xml tag should be on a new line. This is important because we are parsing the input line by line.
- You are not allowed to run long running commands, commands like `npm start` or `python server.py` or `npm run dev` are prohibited etc.

OBJECTIVE

You accomplish a given task iteratively, breaking it down into clear steps and working through them methodically.

1. Analyze the user's task and set clear, achievable goals to accomplish it. Prioritize these goals in a logical order.
2. Work through these goals sequentially, utilizing available tools one at a time as necessary. Each goal should correspond to a distinct step in your problem-solving process. You will be informed on the work completed and what's remaining as you go.
3. Remember, you have extensive capabilities with access to a wide range of tools that can be used in powerful and clever ways as necessary to accomplish each goal. Before calling a tool, do some analysis within <thinking></thinking> tags. First, analyze the file structure provided in environment_details to gain context and insights for proceeding effectively. Then, think about which of the provided tools is the most relevant tool to accomplish the user's task. Next, go through each of the required parameters of the relevant tool and determine if the user has directly provided or given enough information to infer a value. When deciding if the parameter can be inferred, carefully consider all the context to see if it supports a specific value. If all of the required parameters are present or can be reasonably inferred, close the thinking tag and proceed with the tool use. BUT, if one of the values for a required parameter is missing, DO NOT invoke the tool (not even with fillers for the missing params) and instead, ask the user to provide the missing parameters using the ask_followup_question tool. DO NOT ask for more information on optional parameters if it is not provided.
4. Once you've completed the user's task, you must use the `attempt_completion` tool to present the result of the task to the user. You may also provide a CLI command to showcase the result of your task; this can be particularly useful for web development tasks, where you can run e.g. \`open index.html\` to show the website you've built.
5. The user may provide feedback, which you can use to make improvements and try again. But DO NOT continue in pointless back and forth conversations, i.e. don't end your responses with questions or offers for further assistance."#
        )
    }

    /// Passes the previous plan if any and the steps the agent has taken to generate
    /// the next instruction for the agent to work on
    pub async fn reasoning_output(
        &self,
        input: ToolUseAgentReasoningInput,
    ) -> Result<ToolUseAgentReasoningParams, SymbolError> {
        let repo_name = self.properties.repo_name.to_owned();
        let message_properties = input.symbol_event_message_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message_for_o1(&repo_name));
        let user_message = LLMClientMessage::user(self.user_message_for_o1(input));
        let llm_properties = message_properties
            .llm_properties()
            .clone()
            .set_llm(LLMType::O3MiniHigh);
        let request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        // Parse out the output from here
        let response = self
            .llm_client
            .stream_completion(
                llm_client::provider::LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(
                    std::env::var("OPENAI_API_KEY").expect("env var to be present"),
                )),
                request,
                llm_client::provider::LLMProvider::OpenAI,
                vec![
                    ("event_type".to_owned(), "o1_orchestrator".to_owned()),
                    (
                        "root_id".to_owned(),
                        message_properties.root_request_id().to_owned(),
                    ),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| SymbolError::LLMClientError(e))?;
        // parse out the response from here somehow
        Ok(ToolUseAgentReasoningParams::from_response(
            response.answer_up_until_now(),
        ))
    }

    /// This is a special call we are using only for anthropic and nothing
    /// else right now
    pub async fn invoke_json_tool_swe_bench(
        &self,
        input: ToolUseAgentInputOnlyTools,
    ) -> Result<ToolUseAgentOutput, SymbolError> {
        let repo_name = self.properties.repo_name.to_owned();
        let system_message = if self.properties.is_test_running_allowed {
            LLMClientMessage::system(
                self.system_message_for_swe_bench_json_mode_testing_allowed(&repo_name),
            )
            .insert_tools(input.tools)
        } else {
            LLMClientMessage::system(self.system_message_for_swe_bench_json_mode(&repo_name))
                .insert_tools(input.tools)
        };

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

        let agent_temperature = self.temperature;

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
                                agent_temperature,
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
                                agent_temperature,
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
                        serde_json::from_str::<ListFilesInputPartial>(&tool_input).map_err(
                            |_e| SymbolError::ToolError(ToolError::SerdeConversionFailed),
                        )?,
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
                    "code_edit_input" => ToolInputPartial::CodeEditing(
                        serde_json::from_str::<CodeEditingPartialRequest>(&tool_input).map_err(
                            |e| {
                                println!("code_edit_input::error::{:?}", e);
                                SymbolError::ToolError(ToolError::SerdeConversionFailed)
                            },
                        )?,
                    ),
                    "Think" => ToolInputPartial::Thinking(
                        serde_json::from_str::<ThinkingPartialInput>(&tool_input).map_err(|e| {
                            println!("think::error::{:?}", e);
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

            // we are going to be careful over here, we want to make sure that
            // we give back the correct values over here and that the agent is using
            // a single tool instead of multiple tools
            if tool_inputs_parsed.is_empty() {
                Ok(ToolUseAgentOutput::new(
                    ToolUseAgentOutputType::Failure("Empty tools selected".to_owned()),
                    Default::default(),
                ))
            } else {
                let first_tool_selected = tool_inputs_parsed.remove(0);
                Ok(ToolUseAgentOutput::new(
                    ToolUseAgentOutputType::Success(ToolUseAgentOuputSuccess {
                        tool_parameters: first_tool_selected.1,
                        thinking: thinking.trim().to_owned(),
                        tool_use_id: first_tool_selected.0,
                    }),
                    Default::default(),
                ))
            }
        } else {
            Ok(ToolUseAgentOutput::new(
                ToolUseAgentOutputType::Failure("Failed to query llm".to_owned()),
                Default::default(),
            ))
        }
    }

    pub async fn context_crunching(
        &self,
        input: ToolUseAgentContextCrunchingInput,
    ) -> Result<ToolUseAgentOutput, SymbolError> {
        let system_message =
            LLMClientMessage::system(self.system_message_for_context_crunching()).cache_point();
        let user_message = LLMClientMessage::user(self.user_message_for_context_crunching(&input));

        let llm_properties = match &self.context_crunching_llm {
            Some(props) => props.clone(),
            None => LLMProperties::new(
                LLMType::O3MiniHigh,
                llm_client::provider::LLMProvider::OpenAI,
                llm_client::provider::LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(
                    std::env::var("OPENAI_API_KEY").expect("env var to be present"),
                )),
            ),
        };

        let message_properties = input.symbol_event_message_properties.clone();
        if let Some(result) = self
            .try_with_llm(
                llm_properties,
                message_properties.cancellation_token().clone(),
                message_properties.root_request_id().to_owned(),
                message_properties.ui_sender().clone(),
                message_properties.request_id_str(),
                vec![system_message, user_message],
                Some("context_crunching".to_owned()),
            )
            .await?
        {
            if matches!(
                result,
                ToolUseAgentOutput {
                    r#type: ToolUseAgentOutputType::Success(_),
                    usage_statistics: _,
                }
            ) {
                return Ok(result);
            }
        }
        Err(SymbolError::FailedToGetTool)
    }

    pub async fn invoke(
        &self,
        input: ToolUseAgentInput,
    ) -> Result<ToolUseAgentOutput, SymbolError> {
        let system_message = if self.properties.is_eval_run {
            if self.properties.is_test_running_allowed {
                LLMClientMessage::system(
                    self.system_message_midwit_tool_mode(&self.properties.repo_name, &input),
                )
                .cache_point()
            } else {
                LLMClientMessage::system(
                    self.system_message_midwit_tool_mode(&self.properties.repo_name, &input),
                )
                .cache_point()
            }
        } else {
            LLMClientMessage::system(self.system_message(&input)).cache_point()
        };
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

        previous_messages.push(LLMClientMessage::user(format!(
            r#"
---

## Reminder about the tool format:
{}"#,
            input.tool_format_reminder.join("\n\n")
        )));

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
            if let Some(pending_spawned_process_output) =
                input.pending_spawned_process_output.clone()
            {
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
        let final_messages: Vec<_> = vec![system_message.clone()]
            .into_iter()
            .chain(previous_messages.clone())
            .collect();

        let cancellation_token = input.symbol_event_message_properties.cancellation_token();

        // First try with original LLM (Sonnet)
        if let Ok(Some(result)) = self
            .try_with_llm(
                llm_properties.clone(),
                cancellation_token.clone(),
                root_request_id.clone(),
                ui_sender.clone(),
                exchange_id,
                final_messages.to_vec(),
                None,
            )
            .await
        {
            // only return over here if we have success with the tool output
            if matches!(
                result,
                ToolUseAgentOutput {
                    r#type: ToolUseAgentOutputType::Success(_),
                    usage_statistics: _,
                }
            ) {
                return Ok(result);
            }
        }

        // If original LLM (Sonnet3.7) failed, try with sonnet3.5 first
        if llm_properties.llm() == &LLMType::ClaudeSonnet3_7 {
            println!("sonnet37_failed::failing_back_to_sonnet35");
            let sonnet35_properties = llm_properties.clone().set_llm(LLMType::ClaudeSonnet);
            if let Ok(Some(result)) = self
                .try_with_llm(
                    sonnet35_properties,
                    cancellation_token.clone(),
                    root_request_id.clone(),
                    ui_sender.clone(),
                    exchange_id,
                    final_messages.to_vec(),
                    None,
                )
                .await
            {
                if matches!(
                    result,
                    ToolUseAgentOutput {
                        r#type: ToolUseAgentOutputType::Success(_),
                        usage_statistics: _,
                    }
                ) {
                    return Ok(result);
                }
            }
        }

        // If sonnet3.5 failed or if using a different model, try with gemini-flash-2.0
        if llm_properties.llm() == &LLMType::ClaudeSonnet3_7
            || llm_properties.llm() == &LLMType::ClaudeSonnet
        {
            println!("sonnet_failed::failing_back_to_gemini-2.0-flash");
            let gemini_pro_properties = llm_properties.clone().set_llm(LLMType::Gemini2_0Flash);
            if let Ok(Some(result)) = self
                .try_with_llm(
                    gemini_pro_properties,
                    cancellation_token.clone(),
                    root_request_id.clone(),
                    ui_sender.clone(),
                    exchange_id,
                    final_messages.to_vec(),
                    None,
                )
                .await
            {
                // only return over here if we have success with the tool output
                if matches!(
                    result,
                    ToolUseAgentOutput {
                        r#type: ToolUseAgentOutputType::Success(_),
                        usage_statistics: _,
                    }
                ) {
                    return Ok(result);
                }
            }
        }

        // If gemini-pro-1.5 failed, try with gemini-pro-1.5
        if llm_properties.llm() == &LLMType::ClaudeSonnet
            || llm_properties.llm() == &LLMType::ClaudeSonnet3_7
        {
            println!("sonnet_failed::failing_back_to_gemini-pro");
            let gemini_pro_properties = llm_properties.clone().set_llm(LLMType::GeminiPro);
            if let Ok(Some(result)) = self
                .try_with_llm(
                    gemini_pro_properties,
                    cancellation_token.clone(),
                    root_request_id.clone(),
                    ui_sender.clone(),
                    exchange_id,
                    final_messages.to_vec(),
                    None,
                )
                .await
            {
                // only return over here if we have success with the tool output
                if matches!(
                    result,
                    ToolUseAgentOutput {
                        r#type: ToolUseAgentOutputType::Success(_),
                        usage_statistics: _,
                    }
                ) {
                    return Ok(result);
                }
            }
        }

        if llm_properties.llm() == &LLMType::ClaudeSonnet
            || llm_properties.llm() == &LLMType::ClaudeSonnet3_7
        {
            println!("sonnet_failed::failing_back_to_gpt4o");
            let gpt4o_properties = llm_properties.clone().set_llm(LLMType::Gpt4O);
            if let Ok(Some(result)) = self
                .try_with_llm(
                    gpt4o_properties,
                    cancellation_token,
                    root_request_id,
                    ui_sender,
                    exchange_id,
                    final_messages,
                    None,
                )
                .await
            {
                {
                    // only return over here if we have success with the tool output
                    if matches!(
                        result,
                        ToolUseAgentOutput {
                            r#type: ToolUseAgentOutputType::Success(_),
                            usage_statistics: _,
                        }
                    ) {
                        return Ok(result);
                    }
                }
            }
        }

        Err(SymbolError::FailedToGetTool)
    }

    async fn try_with_llm(
        &self,
        llm_properties: LLMProperties,
        cancellation_token: tokio_util::sync::CancellationToken,
        root_request_id: String,
        ui_sender: tokio::sync::mpsc::UnboundedSender<UIEventWithID>,
        exchange_id: &str,
        final_messages: Vec<LLMClientMessage>,
        event_type: Option<String>,
    ) -> Result<Option<ToolUseAgentOutput>, SymbolError> {
        let agent_temperature = self.temperature;
        let (sender, receiver) =
            tokio::sync::mpsc::unbounded_channel::<LLMClientCompletionResponse>();
        let cloned_llm_client = self.llm_client.clone();
        let cloned_root_request_id = root_request_id.clone();
        let cloned_cancellation_token = cancellation_token.clone();

        let llm_stream_handle = run_with_cancellation(
            cancellation_token.clone(),
            tokio::spawn(async move {
                cloned_llm_client
                    .stream_completion(
                        llm_properties.api_key().clone(),
                        LLMClientCompletionRequest::new(
                            llm_properties.llm().clone(),
                            final_messages,
                            agent_temperature,
                            None,
                        ),
                        llm_properties.provider().clone(),
                        vec![
                            (
                                "event_type".to_owned(),
                                event_type.unwrap_or_else(|| "tool_use".to_owned()),
                            ),
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

        let tool_found_token = tokio_util::sync::CancellationToken::new();
        let cloned_tool_found_token = tool_found_token.clone();
        let delta_updater_task = tokio::spawn(async move {
            let mut llm_statistics: LLMClientUsageStatistics = Default::default();
            let llm_statistics_ref = &mut llm_statistics;

            while let Some(Some(stream_msg)) =
                run_with_cancellation(cloned_cancellation_token.clone(), delta_receiver.next())
                    .await
            {
                // If cancelled during stream processing, return early
                if cloned_cancellation_token.is_cancelled() {
                    return Err(SymbolError::CancelledResponseStream);
                }
                llm_statistics_ref.set_usage_statistics(stream_msg.usage_statistics());

                if cloned_tool_found_token.is_cancelled() {
                    break;
                }
                let delta = stream_msg.delta();
                if let Some(delta) = delta {
                    tool_use_generator.add_delta(delta);
                }
            }
            tool_use_generator.flush_answer();
            let thinking_for_tool = tool_use_generator.thinking;
            let tool_input_partial = tool_use_generator.tool_input_partial;
            let complete_response = tool_use_generator.answer_up_until_now;
            println!("tool_use_agent::try_with_llm::delta_task_updater::finished");

            Ok((
                thinking_for_tool,
                tool_input_partial,
                llm_statistics,
                complete_response,
            ))
        });

        let mut tool_update_receiver =
            tokio_stream::wrappers::UnboundedReceiverStream::new(tool_update_receiver);
        while let Some(Some(tool_update)) =
            run_with_cancellation(cancellation_token.clone(), tool_update_receiver.next()).await
        {
            match tool_update {
                ToolBlockEvent::ThinkingFull(thinking_up_until_now) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_thinking(
                        root_request_id.clone(),
                        exchange_id.to_owned(),
                        thinking_up_until_now,
                    ));
                }
                ToolBlockEvent::NoToolFound(full_output) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_not_found(
                        root_request_id.clone(),
                        exchange_id.to_owned(),
                        full_output,
                    ));
                }
                ToolBlockEvent::ToolFound(tool_found) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_found(
                        root_request_id.clone(),
                        exchange_id.to_owned(),
                        tool_found,
                    ));
                }
                ToolBlockEvent::ToolWithParametersFound => {
                    tool_found_token.cancel();
                    break;
                }
                ToolBlockEvent::ToolParameters(tool_parameters_update) => {
                    let _ = ui_sender.clone().send(UIEventWithID::tool_parameter_found(
                        root_request_id.clone(),
                        exchange_id.to_owned(),
                        tool_parameters_update,
                    ));
                }
            }
        }

        // If the loop was broken due to cancellation, return early with CancelledResponseStream
        if cancellation_token.is_cancelled() {
            println!("tool_use_agent::try_with_llm::cancellation_token::is_cancelled");
            return Err(SymbolError::CancelledResponseStream);
        }

        let result = match delta_updater_task.await {
            Ok(Ok((thinking_for_tool, tool_input_partial, llm_statistics, complete_response))) => {
                let final_output = match tool_input_partial {
                    Some(tool_input_partial) => Ok(ToolUseAgentOutputType::Success(
                        ToolUseAgentOuputSuccess::new(
                            tool_input_partial,
                            thinking_for_tool,
                            exchange_id.to_owned(),
                        ),
                    )),
                    None => Ok(ToolUseAgentOutputType::Failure(complete_response)),
                };
                Ok(Some(ToolUseAgentOutput::new(final_output?, llm_statistics)))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SymbolError::CancelledResponseStream),
        };

        match llm_stream_handle.await {
            // The task completed successfully.
            Some(Ok(Err(e))) => Err(SymbolError::LLMClientError(e)),
            _ => result,
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
    // MCP tools work a bit differently.
    // They take a blob of JSON as input rather than trying to map pseudo-XML to JSON
    // for input to the MCP server
    McpToolFound,
    // these are all the different attributes of the tool input
    FilePathFound,
    InstructionFound,
    SummaryFound,
    DirectoryPathFound,
    RecursiveFound,
    RegexPatternFound,
    FilePatternFound,
    CommandFound,
    QuestionFound,
    ResultFound,
    FilePathsFound,
    WaitForExitFound,
    StartLineFound,
    EndLineFound,
    GlobPatternFound,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolParameters {
    pub(crate) field_name: String,
    pub(crate) field_content_up_until_now: String,
    pub(crate) field_content_delta: String,
}

impl ToolParameters {
    pub fn new(
        field_name: String,
        field_content_up_until_now: String,
        field_content_delta: String,
    ) -> Self {
        Self {
            field_name,
            field_content_delta,
            field_content_up_until_now,
        }
    }
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
    pattern: Option<String>,
    fs_file_paths: Option<Vec<String>>,
    instruction: Option<String>,
    directory_path: Option<String>,
    recursive: Option<bool>,
    regex_pattern_found: Option<String>,
    file_pattern: Option<String>,
    command: Option<String>,
    question: Option<String>,
    result: Option<String>,
    wait_for_exit: Option<bool>,
    summary: Option<String>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    tool_input_partial: Option<ToolInputPartial>,
    sender: tokio::sync::mpsc::UnboundedSender<ToolBlockEvent>,
    mcp_json_input: Vec<String>,
}

impl ToolUseGenerator {
    fn new(sender: tokio::sync::mpsc::UnboundedSender<ToolBlockEvent>) -> Self {
        Self {
            answer_up_until_now: "".to_owned(),
            previous_answer_line_number: None,
            tool_block_status: ToolBlockStatus::NoBlock,
            thinking: "".to_owned(),
            tool_type_possible: None,
            pattern: None,
            fs_file_path: None,
            fs_file_paths: None,
            instruction: None,
            directory_path: None,
            recursive: None,
            regex_pattern_found: None,
            file_pattern: None,
            command: None,
            summary: None,
            question: None,
            result: None,
            wait_for_exit: None,
            start_line: None,
            end_line: None,
            tool_input_partial: None,
            sender,
            mcp_json_input: vec![],
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
                    if answer_line_at_index == "<summarize>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::ContextCrunching);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::ContextCrunching));
                    } else if answer_line_at_index == "<semantic_search>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::SemanticSearch);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::SemanticSearch));
                    } else if answer_line_at_index == "<find_file>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::FindFiles);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::FindFiles));
                    } else if answer_line_at_index == "<grep_string>" {
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
                    } else if answer_line_at_index == "<request_screenshot>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                        self.tool_type_possible = Some(ToolType::RequestScreenshot);
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::RequestScreenshot));
                    } else if answer_line_at_index.starts_with("<mcp::") {
                        self.tool_block_status = ToolBlockStatus::McpToolFound;
                        let tool_name = answer_line_at_index
                            .strip_prefix("<")
                            .unwrap_or_default()
                            .strip_suffix(">")
                            .unwrap_or_default();
                        self.tool_type_possible = Some(ToolType::McpTool(tool_name.to_string()));
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolFound(ToolType::McpTool(
                                tool_name.to_string(),
                            )));
                    }
                }
                ToolBlockStatus::ToolFound => {
                    // there are cases where the llm does not put the \n properly
                    // we still want to parse it out properly
                    if answer_line_at_index.starts_with("<pattern>")
                        && answer_line_at_index.ends_with("</pattern>")
                    {
                        // record that we found a file path over here
                        if let Some(prefix_removed) = answer_line_at_index.strip_prefix("<pattern>")
                        {
                            if let Some(suffix_removed) = prefix_removed.strip_suffix("</pattern>")
                            {
                                self.pattern = Some(suffix_removed.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "pattern".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<fs_file_path>")
                        && answer_line_at_index.ends_with("</fs_file_path>")
                    {
                        // record that we found a file path over here
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<fs_file_path>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</fs_file_path>")
                            {
                                self.fs_file_path = Some(suffix_removed.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "fs_file_path".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<directory_path>")
                        && answer_line_at_index.ends_with("</directory_path>")
                    {
                        // record that we found a directory_path over here
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<directory_path>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</directory_path>")
                            {
                                self.directory_path = Some(suffix_removed.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "directory_path".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<recursive>")
                        && answer_line_at_index.ends_with("</recursive>")
                    {
                        // record that we found a recursive path over here
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<recursive>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</recursive>")
                            {
                                self.recursive =
                                    Some(suffix_removed.parse::<bool>().unwrap_or(false));
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "recursive".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<regex_pattern>")
                        && answer_line_at_index.ends_with("</regex_pattern>")
                    {
                        // record that we found a regex pattern over here
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<regex_pattern>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</regex_pattern>")
                            {
                                match self.regex_pattern_found.clone() {
                                    Some(existing_pattern) => {
                                        let new_pattern =
                                            existing_pattern.clone() + "\n" + suffix_removed;
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "regex_pattern".to_owned(),
                                                field_content_up_until_now: new_pattern.clone(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                        self.regex_pattern_found = Some(new_pattern);
                                    }
                                    None => {
                                        self.regex_pattern_found = Some(suffix_removed.to_owned());
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "regex_pattern".to_owned(),
                                                field_content_up_until_now: suffix_removed
                                                    .to_owned(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    } else if answer_line_at_index.starts_with("<command>")
                        && answer_line_at_index.ends_with("</command>")
                    {
                        // parse out the command properly
                        if let Some(prefix_removed) = answer_line_at_index.strip_prefix("<command>")
                        {
                            if let Some(suffix_removed) = prefix_removed.strip_suffix("</command>")
                            {
                                match self.command.clone() {
                                    Some(command) => {
                                        let new_command = command.clone() + "\n" + suffix_removed;
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "command".to_owned(),
                                                field_content_up_until_now: new_command.clone(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                        self.command = Some(new_command);
                                    }
                                    None => {
                                        self.command = Some(suffix_removed.to_owned());
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "command".to_owned(),
                                                field_content_up_until_now: suffix_removed
                                                    .to_owned(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    } else if answer_line_at_index.starts_with("<file_pattern>")
                        && answer_line_at_index.ends_with("</file_pattern>")
                    {
                        // record that we found a recursive path over here
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<file_pattern>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</file_pattern>")
                            {
                                self.file_pattern = Some(suffix_removed.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "file_pattern".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<start_line>")
                        && answer_line_at_index.ends_with("</start_line>")
                    {
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<start_line>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</start_line>")
                            {
                                let parsed = suffix_removed.parse::<usize>().ok();
                                self.start_line = parsed;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters::new(
                                        "start_line".to_owned(),
                                        suffix_removed.to_owned(),
                                        suffix_removed.to_owned(),
                                    ),
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<end_line>")
                        && answer_line_at_index.ends_with("</end_line>")
                    {
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<end_line>")
                        {
                            if let Some(suffix_removed) = prefix_removed.strip_suffix("</end_line>")
                            {
                                let parsed = suffix_removed.parse::<usize>().ok();
                                self.end_line = parsed;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters::new(
                                        "end_line".to_owned(),
                                        suffix_removed.to_owned(),
                                        suffix_removed.to_owned(),
                                    ),
                                ));
                            }
                        }
                    } else if answer_line_at_index.starts_with("<summary>")
                        && answer_line_at_index.ends_with("</summary>")
                    {
                        if let Some(prefix_removed) = answer_line_at_index.strip_prefix("<summary>")
                        {
                            if let Some(suffix_removed) = prefix_removed.strip_suffix("</summary>")
                            {
                                match self.summary.clone() {
                                    Some(summary) => {
                                        let new_summary = summary.clone() + "\n" + suffix_removed;
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "summary".to_owned(),
                                                field_content_up_until_now: new_summary.clone(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                        self.summary = Some(new_summary);
                                    }
                                    None => {
                                        self.summary = Some(suffix_removed.to_owned());
                                        let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                            ToolParameters {
                                                field_name: "summary".to_owned(),
                                                field_content_up_until_now: suffix_removed
                                                    .to_owned(),
                                                field_content_delta: suffix_removed.to_owned(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    } else if answer_line_at_index == "<summary>" {
                        self.tool_block_status = ToolBlockStatus::SummaryFound;
                    } else if answer_line_at_index == "<pattern>" {
                        self.tool_block_status = ToolBlockStatus::GlobPatternFound;
                    } else if answer_line_at_index == "<fs_file_path>" {
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
                    } else if answer_line_at_index == "</summarize>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match (self.instruction.clone(), self.summary.clone()) {
                            (Some(instruction), Some(summary)) => {
                                self.tool_input_partial = Some(ToolInputPartial::ContextCrunching(
                                    ContextCrunchingInputPartial {
                                        summary,
                                        instruction,
                                    },
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</grep_string>" {
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
                    } else if answer_line_at_index == "</semantic_search>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.question.clone() {
                            Some(question) => {
                                self.tool_input_partial = Some(ToolInputPartial::SemanticSearch(
                                    SemanticSearchParametersPartial::new(question),
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
                                    ListFilesInputPartial::new(directory_path, recursive),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "</read_file>" {
                        // The big one: read_file
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        if let Some(fs_file_path) = self.fs_file_path.clone() {
                            let start_line = self.start_line;
                            let end_line = self.end_line;
                            let request =
                                OpenFileRequestPartial::new(fs_file_path, start_line, end_line);
                            self.tool_input_partial = Some(ToolInputPartial::OpenFile(request));
                            let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                        } else {
                            println!("Warning: fs_file_path was None, skipping OpenFile request creation");
                        }

                        // Reset any temporary state so it doesn't leak across tool calls
                        self.tool_type_possible = None;
                        self.fs_file_path = None;
                        self.start_line = None;
                        self.end_line = None;
                    } else if answer_line_at_index == "</get_diagnostics>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        self.tool_input_partial = Some(ToolInputPartial::LSPDiagnostics(
                            WorkspaceDiagnosticsPartial::new(),
                        ));
                        let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                        self.tool_type_possible = None;
                    } else if answer_line_at_index == "<wait_for_exit>" {
                        self.tool_block_status = ToolBlockStatus::WaitForExitFound;
                    } else if answer_line_at_index.starts_with("<wait_for_exit>")
                        && answer_line_at_index.ends_with("</wait_for_exit>")
                    {
                        // Parse inline wait_for_exit value
                        if let Some(prefix_removed) =
                            answer_line_at_index.strip_prefix("<wait_for_exit>")
                        {
                            if let Some(suffix_removed) =
                                prefix_removed.strip_suffix("</wait_for_exit>")
                            {
                                self.wait_for_exit =
                                    Some(suffix_removed.parse::<bool>().unwrap_or(true));
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "wait_for_exit".to_owned(),
                                        field_content_up_until_now: suffix_removed.to_owned(),
                                        field_content_delta: suffix_removed.to_owned(),
                                    },
                                ));
                            }
                        }
                    } else if answer_line_at_index == "</execute_command>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        match self.command.clone() {
                            Some(command) => {
                                self.tool_input_partial = Some(ToolInputPartial::TerminalCommand(
                                    TerminalInputPartial::new(
                                        command.to_owned(),
                                        self.wait_for_exit.unwrap_or(true),
                                    ),
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
                    } else if answer_line_at_index == "</request_screenshot>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        self.tool_type_possible = None;
                        self.tool_input_partial = Some(ToolInputPartial::RequestScreenshot(
                            RequestScreenshotInputPartial::new(),
                        ));
                        let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                    } else if answer_line_at_index == "</find_file>" {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        self.tool_type_possible = None;
                        match self.pattern.clone() {
                            Some(pattern) => {
                                self.tool_input_partial = Some(ToolInputPartial::FindFile(
                                    FindFileInputPartial::new(pattern),
                                ));
                                let _ = self.sender.send(ToolBlockEvent::ToolWithParametersFound);
                            }
                            _ => {}
                        }
                    } else if answer_line_at_index == "<start_line>" {
                        self.tool_block_status = ToolBlockStatus::StartLineFound;
                    } else if answer_line_at_index == "<end_line>" {
                        self.tool_block_status = ToolBlockStatus::EndLineFound;
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
                ToolBlockStatus::WaitForExitFound => {
                    if answer_line_at_index == "</wait_for_exit>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        self.wait_for_exit =
                            Some(answer_line_at_index.parse::<bool>().unwrap_or(true));
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "wait_for_exit".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::StartLineFound => {
                    if answer_line_at_index == "</start_line>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        let parsed = answer_line_at_index.parse::<usize>().ok();
                        self.start_line = parsed;
                        let _ =
                            self.sender
                                .send(ToolBlockEvent::ToolParameters(ToolParameters::new(
                                    "start_line".to_owned(),
                                    answer_line_at_index.to_owned(),
                                    answer_line_at_index.to_owned(),
                                )));
                    }
                }
                ToolBlockStatus::EndLineFound => {
                    if answer_line_at_index == "</end_line>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        let parsed = answer_line_at_index.parse::<usize>().ok();
                        self.end_line = parsed;
                        let _ =
                            self.sender
                                .send(ToolBlockEvent::ToolParameters(ToolParameters::new(
                                    "end_line".to_owned(),
                                    answer_line_at_index.to_owned(),
                                    answer_line_at_index.to_owned(),
                                )));
                    }
                }
                ToolBlockStatus::GlobPatternFound => {
                    if answer_line_at_index == "</pattern>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        self.pattern = Some(answer_line_at_index.to_owned());
                        let _ = self
                            .sender
                            .send(ToolBlockEvent::ToolParameters(ToolParameters {
                                field_name: "pattern".to_owned(),
                                field_content_up_until_now: answer_line_at_index.to_owned(),
                                field_content_delta: answer_line_at_index.to_owned(),
                            }));
                    }
                }
                ToolBlockStatus::SummaryFound => {
                    if answer_line_at_index == "</summary>" {
                        self.tool_block_status = ToolBlockStatus::ToolFound;
                    } else {
                        match self.summary.clone() {
                            Some(summary) => {
                                let new_summary = summary.clone() + "\n" + answer_line_at_index;
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "summary".to_owned(),
                                        field_content_up_until_now: new_summary.clone(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                                self.summary = Some(new_summary);
                            }
                            None => {
                                self.summary = Some(answer_line_at_index.to_owned());
                                let _ = self.sender.send(ToolBlockEvent::ToolParameters(
                                    ToolParameters {
                                        field_name: "summary".to_owned(),
                                        field_content_up_until_now: answer_line_at_index.to_owned(),
                                        field_content_delta: answer_line_at_index.to_owned(),
                                    },
                                ));
                            }
                        }
                    }
                }
                ToolBlockStatus::McpToolFound => {
                    if answer_line_at_index.starts_with("</mcp::") {
                        self.tool_block_status = ToolBlockStatus::NoBlock;
                        let tool_name = answer_line_at_index
                            .strip_prefix("</")
                            .unwrap_or_default()
                            .strip_suffix(">")
                            .unwrap_or_default();
                        if let Ok(partial @ McpToolPartial { .. }) =
                            McpToolPartial::parse(tool_name, &self.mcp_json_input.join("\n"))
                        {
                            self.tool_input_partial = Some(ToolInputPartial::McpTool(partial));
                            self.tool_type_possible = None;
                        }
                    } else {
                        self.mcp_json_input.push(answer_line_at_index.to_string());
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
    use super::ToolUseAgentReasoningParams;
    use super::ToolUseGenerator;

    #[test]
    fn test_agent_reasoning_params_parsing() {
        let response = r#"<plan>
<instruction>
1. Create a standalone Python script that demonstrates the unexpected behavior of separability_matrix with nested compound models.
2. Run the script to confirm that the final matrix for "m.Pix2Sky_TAN() & cm" is not the block diagonal, as suspected.
3. Analyze the output and proceed to inspect "astropy/modeling/separable.py" to understand how the separability_matrix is computed.
4. Propose and implement a fix in the code.
5. Rerun the reproduction script to verify the fix.
6. Confirm that the unexpected behavior is resolved.
</instruction>
</plan>

<current_task>
<instruction>
1) In the root directory of the "astropy" repository, create a file named "reproduce_separability_issue.py".
2) In that file, reproduce the user's example code demonstrating the nested compound models and how separability_matrix is returning unexpected results:
--------------------------------------------------------------------------------
from astropy.modeling import models as m
from astropy.modeling.separable import separability_matrix

def main():
    cm = m.Linear1D(10) & m.Linear1D(5)
    print("separability_matrix(cm):")
    print(separability_matrix(cm))

    tan_and_cm = m.Pix2Sky_TAN() & cm
    print("\nseparability_matrix(m.Pix2Sky_TAN() & cm):")
    print(separability_matrix(tan_and_cm))

if __name__ == "__main__":
    main()
--------------------------------------------------------------------------------
3) Save your changes.
4) Run the script (e.g., "python reproduce_separability_issue.py") in the same directory and capture the output for our reference.
</instruction>
</current_task>"#;
        let parsed_response = ToolUseAgentReasoningParams::from_response(&response);
        assert!(!parsed_response.instruction.is_empty());
        assert!(!parsed_response.plan.is_empty());
    }

    #[test]
    fn test_make_tool_parsing_work() {
        let input = r#"<thinking>
I need to first locate and read the Tool trait definition. Based on the context, it's likely in one of the Rust source files. Let me search for it.
</thinking>

<grep_string>
<directory_path>
/Users/skcd/test_repo/sidecar
</directory_path>
<regex_pattern>
trait\s+Tool\s*\{
</regex_pattern>
<file_pattern>
*.rs
</file_pattern>
</grep_string>"#;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut tool_use_generator = ToolUseGenerator::new(sender);
        tool_use_generator.add_delta(&input);
        tool_use_generator.flush_answer();

        let tool_use_possible = tool_use_generator.tool_input_partial;
        assert!(tool_use_possible.is_some());
    }

    #[test]
    fn test_parsing_same_line_input_works() {
        let input = r#"<thinking>
I need to first locate and read the Tool trait definition. Based on the context, it's likely in one of the Rust source files. Let me search for it.
</thinking>

<grep_string>
<directory_path>/Users/skcd/test_repo/sidecar</directory_path>
<regex_pattern>trait\s+Tool\s*\{</regex_pattern>
<file_pattern>*.rs</file_pattern>
</grep_string>"#;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut tool_use_generator = ToolUseGenerator::new(sender);
        tool_use_generator.add_delta(&input);
        tool_use_generator.flush_answer();

        let tool_use_possible = tool_use_generator.tool_input_partial;
        assert!(tool_use_possible.is_some());
    }

    #[test]
    fn test_parsing_mcp_json_works() {
        let input = r#"<thinking>
To get the current time in Timbuktu, Mali, I'll need to use the mcp::time::get_current_time tool with the appropriate timezone. Timbuktu is in Mali which uses the Africa/Bamako timezone.
</thinking>

<mcp::time::get_current_time>
{
  "timezone": "Africa/Bamako"
}
</mcp::time::get_current_time>"#;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut tool_use_generator = ToolUseGenerator::new(sender);
        tool_use_generator.add_delta(&input);
        tool_use_generator.flush_answer();

        let tool_use_possible = tool_use_generator.tool_input_partial;
        assert!(tool_use_possible.is_some());
    }
}
