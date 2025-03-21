use async_trait::async_trait;
use futures::StreamExt;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};
use serde_xml_rs::from_str;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use crate::agentic::{
    symbol::{identifier::LLMProperties, types::run_with_cancellation, ui_event::UIEventWithID},
    tool::{
        code_edit::xml_processor::XmlProcessor,
        code_symbol::{
            correctness::{CodeCorrectness, CodeCorrectnessAction, CodeCorrectnessRequest},
            error_fix::{CodeEditingErrorRequest, CodeSymbolErrorFix},
            followup::{
                ClassSymbolFollowup, ClassSymbolFollowupRequest, ClassSymbolFollowupResponse,
                ClassSymbolMember,
            },
            important::{
                CodeSymbolFollowAlongForProbing, CodeSymbolImportant, CodeSymbolImportantRequest,
                CodeSymbolImportantResponse, CodeSymbolImportantWideSearch,
                CodeSymbolProbingSummarize, CodeSymbolToAskQuestionsRequest,
                CodeSymbolUtilityRequest, CodeSymbolWithSteps,
            },
            repo_map_search::{RepoMapSearch, RepoMapSearchQuery},
            types::{CodeSymbolError, SerdeError},
        },
        jitter::jitter_sleep,
    },
};

pub struct AnthropicCodeSymbolImportant {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl AnthropicCodeSymbolImportant {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    fn parse_code_edit_reply(response: &str) -> Result<String, CodeSymbolError> {
        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<reply>"))
            .skip(1)
            .take_while(|line| !line.contains("</reply>"))
            .collect::<Vec<_>>()
            .into_iter()
            .skip_while(|line| !line.contains("```"))
            .skip(1)
            .take_while(|line| !line.contains("```"))
            .collect::<Vec<_>>()
            .join("\n");
        if lines == "" {
            Err(CodeSymbolError::WrongFormat(response.to_owned()))
        } else {
            Ok(lines)
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "step_list")]
pub struct StepListItem {
    name: String,
    step: Vec<String>,
    #[serde(default)]
    new: bool,
    file_path: String,
}

impl StepListItem {
    fn parse_from_str(response: &str) -> Option<StepListItem> {
        // we have a string with
        // <step_list>
        // <name>
        // {name}
        // </name>
        // <step>
        // {step over here}
        // </step>
        // ...
        let response_lines = response
            .lines()
            .into_iter()
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        let mut final_lines = vec![];
        let mut inside_step = false;
        let mut accumulate = vec![];
        for line in response_lines.into_iter() {
            if line == "<step>" {
                inside_step = true;
                final_lines.push("<step>".to_owned());
                continue;
            } else if line == "</step>" {
                inside_step = false;
                // some accumulated lines
                // we want to escape this part right?
                let accumulated_lines = accumulate.to_vec().join("\n");
                accumulate = vec![];
                let accumulated_lines = quick_xml::escape::escape(&accumulated_lines);
                final_lines.push(accumulated_lines.to_string());
                final_lines.push("</step>".to_owned());
                continue;
            } else {
                if inside_step {
                    accumulate.push(line);
                } else {
                    final_lines.push(line);
                }
            }
        }
        let final_response = final_lines.join("\n");
        from_str::<StepListItem>(&final_response)
            .map(|mut parsed_response| {
                parsed_response.step = parsed_response
                    .step
                    .into_iter()
                    .filter_map(|step| {
                        quick_xml::escape::unescape(&step)
                            .ok()
                            .map(|output| output.to_string())
                    })
                    .collect::<Vec<_>>();
                parsed_response
            })
            .ok()
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename = "step_by_step")]
pub struct StepList {
    #[serde(rename = "$value")]
    steps: Vec<StepListItem>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct Reply {
    // #[serde(rename = "step_by_step")]
    #[serde(default)]
    step_by_step: StepList,
}

// <reply>
// <steps_to_answer>
// - For answering the user query we have to understand how we are getting the prettier config from the language
// - `prettier_settings` is being used today to get the prettier parser
// - the language configuration seems to be contained in `prettier is not allowed for language {{buffer_language:?}}"`
// - we are getting the language configuration by calling `buffer_language` function
// - we should check what `get_language` returns to understand what properies `buffer_language` has
// </steps_to_answer>
// <symbol_list>
// <symbol>
// <name>
// Language
// </name>
// <line_content>
//     pub fn language(&self) -> Option<&Arc<Language>> {{
// </line_content>
// <file_path>
// crates/language/src/buffer.rs
// </file_path>
// <thinking>
// Does the language type expose any prettier settings, because we want to get it so we can use that as the fallback
// </thinking>
// </symbol>
// <symbol>
// </symbol_list>
// </reply>

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol")]
pub struct AskQuestionSymbolHint {
    name: String,
    line_content: String,
    file_path: String,
    thinking: String,
}

impl AskQuestionSymbolHint {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn line_content(&self) -> &str {
        &self.line_content
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol_list")]
pub struct CodeSymbolToAskQuestionsSymboList {
    #[serde(rename = "$value")]
    symbol_list: Vec<AskQuestionSymbolHint>,
}

impl CodeSymbolToAskQuestionsSymboList {
    pub fn new(symbol_list: Vec<AskQuestionSymbolHint>) -> Self {
        Self { symbol_list }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct CodeSymbolToAskQuestionsResponse {
    steps_to_answer: String,
    symbol_list: CodeSymbolToAskQuestionsSymboList,
}

impl CodeSymbolToAskQuestionsResponse {
    pub fn symbol_list(self) -> Vec<AskQuestionSymbolHint> {
        self.symbol_list.symbol_list
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct CodeSymbolShouldAskQuestionsResponse {
    thinking: String,
    context_enough: bool,
}

impl CodeSymbolShouldAskQuestionsResponse {
    pub fn thinking(&self) -> &str {
        &self.thinking
    }

    pub fn should_follow(&self) -> bool {
        // if we have enough context then we do not need to follow the symbols
        // any longer, if thats not the case then we need to follow it more
        !self.context_enough
    }

    pub fn parse_response(
        response: String,
    ) -> Result<CodeSymbolShouldAskQuestionsResponse, CodeSymbolError> {
        // parse out the string properly over here
        let mut fixed_lines = vec![];
        let mut is_inside = false;
        // we should remove lines where the current value is <thinking> and the next one is </thinking>
        // this implies that they both are empty and hallucinations, but a better question would be why
        // this is the case
        let mut is_current_thinking = false;
        response.lines().into_iter().for_each(|line| {
            if line.starts_with("<thinking>") {
                is_inside = true;
                is_current_thinking = true;
                fixed_lines.push(line.to_owned());
                return;
            } else if line.starts_with("</thinking>") {
                // if the previous one was also </thinking> then we should not add this
                if fixed_lines.last().map(|s| s.to_owned()) == Some("</thinking>".to_owned()) {
                    is_current_thinking = false;
                    return;
                }
                is_inside = false;
                if is_current_thinking {
                    is_current_thinking = false;
                    // remove the last <thinking> which we added
                    fixed_lines.pop();
                } else {
                    is_current_thinking = false;
                    fixed_lines.push(line.to_owned());
                }
                return;
            }
            is_current_thinking = false;
            if is_inside {
                fixed_lines.push(AnthropicCodeSymbolImportant::unescape_xml(line.to_owned()));
            } else {
                fixed_lines.push(line.to_owned());
            }
        });
        let fixed_response = fixed_lines.join("\n");
        println!("{}", &fixed_response);
        let parsed_response = from_str::<CodeSymbolShouldAskQuestionsResponse>(&fixed_response)
            .map_err(|e| {
                CodeSymbolError::SerdeError(SerdeError::new(e, fixed_response.to_owned()))
            });
        match parsed_response {
            Ok(response) => Ok(CodeSymbolShouldAskQuestionsResponse {
                thinking: AnthropicCodeSymbolImportant::escape_xml(response.thinking),
                context_enough: response.context_enough,
            }),
            Err(e) => Err(e),
        }
    }
}

impl CodeSymbolToAskQuestionsResponse {
    fn escape_string(self) -> Self {
        let steps_to_answer = AnthropicCodeSymbolImportant::escape_xml(self.steps_to_answer);
        let symbol_list = self.symbol_list;
        Self {
            steps_to_answer,
            symbol_list: CodeSymbolToAskQuestionsSymboList::new(
                symbol_list
                    .symbol_list
                    .into_iter()
                    .map(|symbol_list| AskQuestionSymbolHint {
                        name: symbol_list.name,
                        line_content: AnthropicCodeSymbolImportant::escape_xml(
                            symbol_list.line_content,
                        ),
                        file_path: symbol_list.file_path,
                        thinking: AnthropicCodeSymbolImportant::escape_xml(symbol_list.thinking),
                    })
                    .collect(),
            ),
        }
    }

    fn parse_response(response: String) -> Result<Self, CodeSymbolError> {
        // we need to parse the sections of the reply properly
        // we can start with opening and closing brackets and unescape the lines properly
        let lines = response
            .lines()
            .into_iter()
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        let tags = vec!["steps_to_answer", "line_content", "thinking"]
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<Vec<_>>();
        let same_line_tags = vec!["name", "line_content", "file_path"]
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<Vec<_>>();
        let mut final_lines = vec![];
        let mut inside = false;
        for line in lines.into_iter() {
            // if both the opening and closing tags are contained in the same line
            // we can just add the line and short-circuit the flow
            if same_line_tags.iter().any(|tag| {
                line.starts_with(&format!("<{tag}>")) && line.contains(&format!("</{tag}>"))
            }) {
                final_lines.push(line);
                continue;
            }
            if tags.iter().any(|tag| line.starts_with(&format!("<{tag}>"))) {
                inside = true;
                final_lines.push(line);
                continue;
            }
            if tags
                .iter()
                .any(|tag| line.starts_with(&format!("</{tag}>")))
            {
                inside = false;
                final_lines.push(line);
                continue;
            }
            if inside {
                final_lines.push(AnthropicCodeSymbolImportant::unescape_xml(line));
            } else {
                final_lines.push(line);
            }
        }

        let final_line = final_lines.join("\n");
        let parsed_reply = quick_xml::de::from_str::<CodeSymbolToAskQuestionsResponse>(&final_line)
            .map_err(|e| CodeSymbolError::QuickXMLError(e))?
            .escape_string();

        // Now we need to escape the strings again for xml
        // TODO(skcd): Make this work over here
        Ok(parsed_reply)
    }
}

impl Reply {
    pub fn fix_escaped_string(self) -> Self {
        let step_by_step = self
            .step_by_step
            .steps
            .into_iter()
            .map(|step| {
                let steps = step
                    .step
                    .into_iter()
                    .map(|step| AnthropicCodeSymbolImportant::escape_xml(step))
                    .collect();
                StepListItem {
                    name: step.name,
                    step: steps,
                    new: step.new,
                    file_path: step.file_path,
                }
            })
            .collect::<Vec<_>>();
        Self {
            step_by_step: StepList {
                steps: step_by_step,
            },
        }
    }
}

impl Reply {
    fn to_code_symbol_important_response(self) -> CodeSymbolImportantResponse {
        let code_symbols_with_steps = self
            .step_by_step
            .steps
            .into_iter()
            .map(|step| CodeSymbolWithSteps::new(step.name, step.step, step.new, step.file_path))
            .collect();
        CodeSymbolImportantResponse::new(vec![], code_symbols_with_steps)
    }

    fn unescape_xml(s: String) -> String {
        quick_xml::escape::escape(&s).to_string()
    }

    // Welcome to the jungle and an important lesson on why xml sucks sometimes
    // &, and < are invalid characters in xml, so we can't simply parse it using
    // serde cause the xml parser will legit look at these symbols and fail
    // hard, instead we have to escape these strings properly
    // and not at everypace (see it gets weird)
    // we only have to do this inside the <step>{content}</step> tags
    // so lets get to it
    // one good thing we know is that because we ask claude to follow this format
    // it will always give a new line so we can just split into lines and then
    // do the replacement
    pub fn cleanup_string(response: &str) -> String {
        let mut is_inside_step = false;
        let mut new_lines = vec![];
        for line in response.lines() {
            if line == "<step>" {
                is_inside_step = true;
                new_lines.push("<step>".to_owned());
                continue;
            } else if line == "</step>" {
                is_inside_step = false;
                new_lines.push("</step>".to_owned());
                continue;
            }
            if is_inside_step {
                new_lines.push(Self::unescape_xml(line.to_owned()))
            } else {
                new_lines.push(line.to_owned());
            }
        }
        new_lines.join("\n")
    }

    pub fn parse_response(response: &str) -> Result<Self, CodeSymbolError> {
        if response.is_empty() {
            return Err(CodeSymbolError::EmptyResponse);
        }
        let parsed_response = Self::cleanup_string(response);
        // we want to grab the section between <reply> and </reply> tags
        // and then we want to parse the response which is in the following format
        let lines = parsed_response
            .lines()
            .skip_while(|line| !line.contains("<reply>"))
            .skip(1)
            .take_while(|line| !line.contains("</reply>"))
            .collect::<Vec<&str>>()
            .join("\n");
        let reply = format!(
            r#"<reply>
{lines}
</reply>"#
        );
        Ok(from_str::<Reply>(&reply)
            .map(|reply| reply.fix_escaped_string())
            .map_err(|e| CodeSymbolError::SerdeError(SerdeError::new(e, response.to_owned())))?)
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct ProbeShouldFollowInfo {
    name: String,
    #[serde(rename = "file_path")]
    file_path: String,
    reason: String,
}

impl ProbeShouldFollowInfo {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename = "tool")]
pub enum ProbeNextSymbol {
    #[serde(rename = "answer_user_query")]
    AnswerUserQuery(String),
    #[serde(rename = "should_follow")]
    ShouldFollow(ProbeShouldFollowInfo),
    #[serde(rename = "wrong_path")]
    WrongPath(String),
    #[serde(rename = "")]
    Empty,
}

impl ProbeNextSymbol {
    fn parse_response(response: &str) -> Result<Self, CodeSymbolError> {
        // here we want to escape any of the strings which are inside the answer_user_query or should_follow or wrong_path tags
        let response_lines = response.lines();
        let mut final_lines = vec![];
        let mut is_inside = false;
        for line in response_lines {
            if line.starts_with("<answer_user_query>")
                || line.starts_with("<reason>")
                || line.starts_with("<wrong_path>")
            {
                is_inside = true;
                final_lines.push(line.to_owned());
                continue;
            } else if line.starts_with("</answer_user_query>")
                || line.starts_with("</reason>")
                || line.starts_with("</wrong_path>")
            {
                is_inside = false;
                final_lines.push(line.to_owned());
                continue;
            }
            if is_inside {
                final_lines.push(AnthropicCodeSymbolImportant::unescape_xml(line.to_owned()));
            } else {
                final_lines.push(line.to_owned());
            }
        }
        let fixed_response = final_lines.join("\n");
        let parsed_response = from_str::<ProbeNextSymbol>(&fixed_response)
            .map_err(|e| CodeSymbolError::SerdeError(SerdeError::new(e, response.to_owned())))?;
        // Now we fix the lines in parsed_response again
        Ok(match parsed_response {
            Self::AnswerUserQuery(response) => {
                Self::AnswerUserQuery(AnthropicCodeSymbolImportant::escape_xml(response))
            }
            Self::ShouldFollow(response) => Self::ShouldFollow(ProbeShouldFollowInfo {
                name: response.name,
                file_path: response.file_path,
                reason: AnthropicCodeSymbolImportant::escape_xml(response.reason),
            }),
            Self::WrongPath(response) => {
                Self::WrongPath(AnthropicCodeSymbolImportant::escape_xml(response))
            }
            Self::Empty => Self::Empty,
        })
    }
}

impl AnthropicCodeSymbolImportant {
    fn fix_class_symbol_response(
        &self,
        response: String,
    ) -> Result<ClassSymbolFollowupResponse, CodeSymbolError> {
        let lines = response
            .lines()
            .into_iter()
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        let mut fixed_lines = vec![];
        let mut inside_thinking = false;
        for line in lines.into_iter() {
            if line == "<thinking>" || line == "<line>" {
                inside_thinking = true;
                fixed_lines.push(line);
                continue;
            } else if line == "</thinking>" || line == "</line>" {
                inside_thinking = false;
                fixed_lines.push(line);
                continue;
            }
            if inside_thinking {
                fixed_lines.push(Self::unescape_xml(line));
            } else {
                fixed_lines.push(line);
            }
        }
        let fixed_response = fixed_lines.join("\n");
        from_str::<ClassSymbolFollowupResponse>(&fixed_response)
            .map(|response| {
                let members = response.members();
                let members = members
                    .into_iter()
                    .map(|member| {
                        let line = member.line();
                        let name = member.name();
                        let thinking = member.thinking();
                        ClassSymbolMember::new(
                            Self::escape_xml(line.to_owned()),
                            name.to_owned(),
                            Self::escape_xml(thinking.to_owned()),
                        )
                    })
                    .collect::<Vec<_>>();
                ClassSymbolFollowupResponse::new(members)
            })
            .map_err(|e| CodeSymbolError::SerdeError(SerdeError::new(e, fixed_response)))
    }

    fn user_message_for_ask_question_symbols(
        &self,
        request: CodeSymbolToAskQuestionsRequest,
    ) -> String {
        let user_query = request.user_query();
        let extra_data = request.extra_data();
        let fs_file_path = request.fs_file_path();
        let code_above = request.code_above().unwrap_or("".to_owned());
        let code_below = request.code_below().unwrap_or("".to_owned());
        let code_in_selection = request.code_in_selection();
        format!(
            r#"<user_query>
{user_query}
</user_query>
<file>
<file_path>
{fs_file_path}
</file_path>
<code_above>
{code_above}
</code_above>
<code_below>
{code_below}
</code_below>
<code_in_selection>
{code_in_selection}
</code_in_selection>
</file>
<extra_data>
{extra_data}
</extra_data>

As a reminder the code in selection is:
<code_in_selection>
{code_in_selection}
</code_in_selection>

Your reply for the symbol list you are interested in should be strictly in the following format:
<symbol_list>
<steps_to_answer>
{{The steps you are gonig to take to answer the user query}}
</steps_to_answer>
<symbol>
<name>
{{name of the symbol you are interested in}}
</name>
<line_content>
{{the line containing the symbol you are interested in, this should ALWAYS be a single line}}
</line_content>
<file_path>
{{the file path where this symbol and the line containing it are located}}
</file_path>
<thinking>
{{your reason behind choosing this symbol}}
</thinking>
</symbol>
... {{more symbols here if required}}
</symbol_list>"#
        )
    }

    fn _system_message_for_symbols_edit_operations(
        &self,
        symbol_name: &str,
        fs_file_path: &str,
    ) -> String {
        format!(
            r#"You are an expert software engineer who is going to ask for edits on some more symbols to make sure that the user query is satisfied.
- You are responsible for implementing the change suggested in <user_query>
- You are also given the implementations and the definitions of the code symbols which we have gathered since these are important to answer the user query.
- You are responsible for any changes which we will make to <symbol_name>{symbol_name}</symbol_name> present in file {fs_file_path} after ALL the other symbols and your dependencies have changed.
- The user query is given to you in <user_query>
- The <user_query> is one of the following use case:
<use_case_list>
<use_case>
Reference change: A code symbol struct, implementation, trait, subclass, enum, function, constant or type has changed, you have to figure out what needs to be changed and we want to make some changes to our own implementation
</use_case>
<use_case>
We are going to add more functionality which the user wants to create using the <user_query>
</use_case>
<use_case>
The <user_query> is about asking for some important information
</use_case>
<use_case>
<user_query> wants to gather more information about a particular workflow which the user has asked for in their query
</use_case>
</use_case_list>
- You have to select at most 5 symbols and you can do the following:
<operation>
You can initiate a write operation on one of the symbols to make changes to it, so you can start making changes as required by the user query. This allows you to make sure that the rest of the codebase has changed to support the user query and if required you can make changes to yourself.
</operation>
- Now first think step by step on how you are going to approach this problem and write down your steps in <steps_to_answer> section
- Here are some approaches and questions you can ask for the symbol:
<approach_list>
<approach>
You want to add a parameter to one of the structures or classes which are present and expose that information to the <symbol_name>{symbol_name}</symbol_name>
</approach>
<approach>
You want to change a function from sync to async or vice verse because the user query asks for that
</approach>
<approach>
The implementation of the dependency of the function needs to change to satisfy the user query.
</approach>
</approach_list>
These are just examples of approaches you can take, keep them in mind and be critical and helpful when implementing the user query.
- Your reply should be in the <reply> tag and strictly follow this format:
<reply>
<steps_to_answer>
{{The steps you are gonig to take to answer the user query}}
</steps_to_answer>
<symbol_list>
<symbol>
<name>
{{name of the symbol you are interested in}}
</name>
<line_content>
{{the line containing the symbol you are interested in}}
</line_content>
<file_path>
{{the file path where this symbol and the line containing it are located}}
</file_path>
<thinking>
{{your reason behind choosing this symbol}}
</thinking>
</symbol>
... {{more symbols here if required}}
</symbol_list>
</reply>

We are now going to show you an example:
<user_query>
I want to try and grab the prettier settings from the language config as a fall back instead of erroring out
</user_query>
<file>
<file_path>
crates/prettier/src/prettier.rs
</file_path>
<code_above>
```rust
/// An in-memory representation of a source code file, including its text,
/// syntax trees, git status, and diagnostics.
pub struct Buffer {{
    text: TextBuffer,
    diff_base: Option<Rope>,
    git_diff: git::diff::BufferDiff,
    file: Option<Arc<dyn File>>,
    /// The mtime of the file when this buffer was last loaded from
    /// or saved to disk.
    saved_mtime: Option<SystemTime>,
    /// The version vector when this buffer was last loaded from
    /// or saved to disk.
    saved_version: clock::Global,
    transaction_depth: usize,
    was_dirty_before_starting_transaction: Option<bool>,
    reload_task: Option<Task<Result<()>>>,
    language: Option<Arc<Language>>,
    autoindent_requests: Vec<Arc<AutoindentRequest>>,
    pending_autoindent: Option<Task<()>>,
    sync_parse_timeout: Duration,
    syntax_map: Mutex<SyntaxMap>,
    parsing_in_background: bool,
    parse_count: usize,
    diagnostics: SmallVec<[(LanguageServerId, DiagnosticSet); 2]>,
    remote_selections: TreeMap<ReplicaId, SelectionSet>,
    selections_update_count: usize,
    diagnostics_update_count: usize,
    diagnostics_timestamp: clock::Lamport,
    file_update_count: usize,
    git_diff_update_count: usize,
    completion_triggers: Vec<String>,
    completion_triggers_timestamp: clock::Lamport,
    deferred_ops: OperationQueue<Operation>,
    capability: Capability,
    has_conflict: bool,
    diff_base_version: usize,
}}

impl Buffer {{
    /// Create a new buffer with the given base text.
    pub fn local<T: Into<String>>(base_text: T, cx: &mut ModelContext<Self>) -> Self {{
        Self::build(
            TextBuffer::new(0, cx.entity_id().as_non_zero_u64().into(), base_text.into()),
            None,
            None,
            Capability::ReadWrite,
        )
    }}

    /// Assign a language to the buffer, returning the buffer.
    pub fn with_language(mut self, language: Arc<Language>, cx: &mut ModelContext<Self>) -> Self {{
        self.set_language(Some(language), cx);
        self
    }}

    /// Returns the [Capability] of this buffer.
    pub fn capability(&self) -> Capability {{
        self.capability
    }}

    /// Whether this buffer can only be read.
    pub fn read_only(&self) -> bool {{
        self.capability == Capability::ReadOnly
    }}

    /// Builds a [Buffer] with the given underlying [TextBuffer], diff base, [File] and [Capability].
    pub fn build(
        buffer: TextBuffer,
        diff_base: Option<String>,
        file: Option<Arc<dyn File>>,
        capability: Capability,
    ) -> Self {{
        let saved_mtime = file.as_ref().and_then(|file| file.mtime());

        Self {{
            saved_mtime,
            saved_version: buffer.version(),
            reload_task: None,
            transaction_depth: 0,
            was_dirty_before_starting_transaction: None,
            text: buffer,
            diff_base: diff_base
                .map(|mut raw_diff_base| {{
                    LineEnding::normalize(&mut raw_diff_base);
                    raw_diff_base
                }})
                .map(Rope::from),
            diff_base_version: 0,
            git_diff: git::diff::BufferDiff::new(),
            file,
            capability,
            syntax_map: Mutex::new(SyntaxMap::new()),
            parsing_in_background: false,
            parse_count: 0,
            sync_parse_timeout: Duration::from_millis(1),
            autoindent_requests: Default::default(),
            pending_autoindent: Default::default(),
            language: None,
            remote_selections: Default::default(),
            selections_update_count: 0,
            diagnostics: Default::default(),
            diagnostics_update_count: 0,
            diagnostics_timestamp: Default::default(),
            file_update_count: 0,
            git_diff_update_count: 0,
            completion_triggers: Default::default(),
            completion_triggers_timestamp: Default::default(),
            deferred_ops: OperationQueue::new(),
            has_conflict: false,
        }}
    }}

    /// Retrieve a snapshot of the buffer's current state. This is computationally
    /// cheap, and allows reading from the buffer on a background thread.
    pub fn snapshot(&self) -> BufferSnapshot {{
        let text = self.text.snapshot();
        let mut syntax_map = self.syntax_map.lock();
        syntax_map.interpolate(&text);
        let syntax = syntax_map.snapshot();

        BufferSnapshot {{
            text,
            syntax,
            git_diff: self.git_diff.clone(),
            file: self.file.clone(),
            remote_selections: self.remote_selections.clone(),
            diagnostics: self.diagnostics.clone(),
            diagnostics_update_count: self.diagnostics_update_count,
            file_update_count: self.file_update_count,
            git_diff_update_count: self.git_diff_update_count,
            language: self.language.clone(),
            parse_count: self.parse_count,
            selections_update_count: self.selections_update_count,
        }}
    }}

    #[cfg(test)]
    pub(crate) fn as_text_snapshot(&self) -> &text::BufferSnapshot {{
        &self.text
    }}

    /// Retrieve a snapshot of the buffer's raw text, without any
    /// language-related state like the syntax tree or diagnostics.
    pub fn text_snapshot(&self) -> text::BufferSnapshot {{
        self.text.snapshot()
    }}

    /// The file associated with the buffer, if any.
    pub fn file(&self) -> Option<&Arc<dyn File>> {{
        self.file.as_ref()
    }}

    /// The version of the buffer that was last saved or reloaded from disk.
    pub fn saved_version(&self) -> &clock::Global {{
        &self.saved_version
    }}

    /// The mtime of the buffer's file when the buffer was last saved or reloaded from disk.
    pub fn saved_mtime(&self) -> Option<SystemTime> {{
        self.saved_mtime
    }}

    /// Assign a language to the buffer.
    pub fn set_language(&mut self, language: Option<Arc<Language>>, cx: &mut ModelContext<Self>) {{
        self.parse_count += 1;
        self.syntax_map.lock().clear();
        self.language = language;
        self.reparse(cx);
        cx.emit(Event::LanguageChanged);
    }}

    /// Assign a language registry to the buffer. This allows the buffer to retrieve
    /// other languages if parts of the buffer are written in different languages.
    pub fn set_language_registry(&mut self, language_registry: Arc<LanguageRegistry>) {{
        self.syntax_map
            .lock()
            .set_language_registry(language_registry);
    }}

    /// Assign the buffer a new [Capability].
    pub fn set_capability(&mut self, capability: Capability, cx: &mut ModelContext<Self>) {{
        self.capability = capability;
        cx.emit(Event::CapabilityChanged)
    }}

    /// Waits for the buffer to receive operations up to the given version.
    pub fn wait_for_version(&mut self, version: clock::Global) -> impl Future<Output = Result<()>> {{
        self.text.wait_for_version(version)
    }}

    /// Forces all futures returned by [`Buffer::wait_for_version`], [`Buffer::wait_for_edits`], or
    /// [`Buffer::wait_for_version`] to resolve with an error.
    pub fn give_up_waiting(&mut self) {{
        self.text.give_up_waiting();
    }}

    fn did_edit(
        &mut self,
        old_version: &clock::Global,
        was_dirty: bool,
        cx: &mut ModelContext<Self>,
    ) {{
        if self.edits_since::<usize>(old_version).next().is_none() {{
            return;
        }}

        self.reparse(cx);

        cx.emit(Event::Edited);
        if was_dirty != self.is_dirty() {{
            cx.emit(Event::DirtyChanged);
        }}
        cx.notify();
    }}

    /// Applies the given remote operations to the buffer.
    pub fn apply_ops<I: IntoIterator<Item = Operation>>(
        &mut self,
        ops: I,
        cx: &mut ModelContext<Self>,
    ) -> Result<()> {{
        self.pending_autoindent.take();
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        let mut deferred_ops = Vec::new();
        let buffer_ops = ops
            .into_iter()
            .filter_map(|op| match op {{
                Operation::Buffer(op) => Some(op),
                _ => {{
                    if self.can_apply_op(&op) {{
                        self.apply_op(op, cx);
                    }} else {{
                        deferred_ops.push(op);
                    }}
                    None
                }}
            }})
            .collect::<Vec<_>>();
        self.text.apply_ops(buffer_ops)?;
        self.deferred_ops.insert(deferred_ops);
        self.flush_deferred_ops(cx);
        self.did_edit(&old_version, was_dirty, cx);
        // Notify independently of whether the buffer was edited as the operations could include a
        // selection update.
        cx.notify();
        Ok(())
    }}

    fn flush_deferred_ops(&mut self, cx: &mut ModelContext<Self>) {{
        let mut deferred_ops = Vec::new();
        for op in self.deferred_ops.drain().iter().cloned() {{
            if self.can_apply_op(&op) {{
                self.apply_op(op, cx);
            }} else {{
                deferred_ops.push(op);
            }}
        }}
        self.deferred_ops.insert(deferred_ops);
    }}

    fn can_apply_op(&self, operation: &Operation) -> bool {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                diagnostics: diagnostic_set,
                ..
            }} => diagnostic_set.iter().all(|diagnostic| {{
                self.text.can_resolve(&diagnostic.range.start)
                    && self.text.can_resolve(&diagnostic.range.end)
            }}),
            Operation::UpdateSelections {{ selections, .. }} => selections
                .iter()
                .all(|s| self.can_resolve(&s.start) && self.can_resolve(&s.end)),
            Operation::UpdateCompletionTriggers {{ .. }} => true,
        }}
    }}

    fn apply_op(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                server_id,
                diagnostics: diagnostic_set,
                lamport_timestamp,
            }} => {{
                let snapshot = self.snapshot();
                self.apply_diagnostic_update(
                    server_id,
                    DiagnosticSet::from_sorted_entries(diagnostic_set.iter().cloned(), &snapshot),
                    lamport_timestamp,
                    cx,
                );
            }}
            Operation::UpdateSelections {{
                selections,
                lamport_timestamp,
                line_mode,
                cursor_shape,
            }} => {{
                if let Some(set) = self.remote_selections.get(&lamport_timestamp.replica_id) {{
                    if set.lamport_timestamp > lamport_timestamp {{
                        return;
                    }}
                }}

                self.remote_selections.insert(
                    lamport_timestamp.replica_id,
                    SelectionSet {{
                        selections,
                        lamport_timestamp,
                        line_mode,
                        cursor_shape,
                    }},
                );
                self.text.lamport_clock.observe(lamport_timestamp);
                self.selections_update_count += 1;
            }}
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp,
            }} => {{
                self.completion_triggers = triggers;
                self.text.lamport_clock.observe(lamport_timestamp);
            }}
        }}
    }}

    fn apply_diagnostic_update(
        &mut self,
        server_id: LanguageServerId,
        diagnostics: DiagnosticSet,
        lamport_timestamp: clock::Lamport,
        cx: &mut ModelContext<Self>,
    ) {{
        if lamport_timestamp > self.diagnostics_timestamp {{
            let ix = self.diagnostics.binary_search_by_key(&server_id, |e| e.0);
            if diagnostics.len() == 0 {{
                if let Ok(ix) = ix {{
                    self.diagnostics.remove(ix);
                }}
            }} else {{
                match ix {{
                    Err(ix) => self.diagnostics.insert(ix, (server_id, diagnostics)),
                    Ok(ix) => self.diagnostics[ix].1 = diagnostics,
                }};
            }}
            self.diagnostics_timestamp = lamport_timestamp;
            self.diagnostics_update_count += 1;
            self.text.lamport_clock.observe(lamport_timestamp);
            cx.notify();
            cx.emit(Event::DiagnosticsUpdated);
        }}
    }}

    fn send_operation(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        cx.emit(Event::Operation(operation));
    }}

    /// Removes the selections for a given peer.
    pub fn remove_peer(&mut self, replica_id: ReplicaId, cx: &mut ModelContext<Self>) {{
        self.remote_selections.remove(&replica_id);
        cx.notify();
    }}

    /// Undoes the most recent transaction.
    pub fn undo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.undo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Manually undoes a specific transaction in the buffer's undo history.
    pub fn undo_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        if let Some(operation) = self.text.undo_transaction(transaction_id) {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            true
        }} else {{
            false
        }}
    }}

    /// Manually undoes all changes after a given transaction in the buffer's undo history.
    pub fn undo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.undo_to_transaction(transaction_id);
        let undone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if undone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        undone
    }}

    /// Manually redoes a specific transaction in the buffer's redo history.
    pub fn redo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.redo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Returns the primary [Language] assigned to this [Buffer].
    pub fn language(&self) -> Option<&Arc<Language>> {{
        self.language.as_ref()
    }}

    /// Manually undoes all changes until a given transaction in the buffer's redo history.
    pub fn redo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.redo_to_transaction(transaction_id);
        let redone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if redone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        redone
    }}

    /// Override current completion triggers with the user-provided completion triggers.
    pub fn set_completion_triggers(&mut self, triggers: Vec<String>, cx: &mut ModelContext<Self>) {{
        self.completion_triggers.clone_from(&triggers);
        self.completion_triggers_timestamp = self.text.lamport_clock.tick();
        self.send_operation(
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp: self.completion_triggers_timestamp,
            }},
            cx,
        );
        cx.notify();
    }}

    /// Returns a list of strings which trigger a completion menu for this language.
    /// Usually this is driven by LSP server which returns a list of trigger characters for completions.
    pub fn completion_triggers(&self) -> &[String] {{
        &self.completion_triggers
    }}
}}
```
</code_above>
<code_below>
```rust
```
</code_below>
<code_in_selection>
```rust

```
</code_in_selection>
</file>
<extra_data>
<symbol_name>
Language
</symbol_name>
<fs_file_path>
crates/langauges/src/language.rs
</fs_file_path>
<content>
```rust
pub struct Language {{
    pub(crate) id: LanguageId,
    pub(crate) config: LanguageConfig,
    pub(crate) grammar: Option<Arc<Grammar>>,
    pub(crate) context_provider: Option<Arc<dyn ContextProvider>>,
}}

impl Language {{
    pub fn new(config: LanguageConfig, ts_language: Option<tree_sitter::Language>) -> Self {{
        Self::new_with_id(LanguageId::new(), config, ts_language)
    }}

    fn new_with_id(
        id: LanguageId,
        config: LanguageConfig,
        ts_language: Option<tree_sitter::Language>,
    ) -> Self {{
        Self {{
            id,
            config,
            grammar: ts_language.map(|ts_language| {{
                Arc::new(Grammar {{
                    id: GrammarId::new(),
                    highlights_query: None,
                    brackets_config: None,
                    outline_config: None,
                    embedding_config: None,
                    indents_config: None,
                    injection_config: None,
                    override_config: None,
                    redactions_config: None,
                    runnable_config: None,
                    error_query: Query::new(&ts_language, "(ERROR) @error").unwrap(),
                    ts_language,
                    highlight_map: Default::default(),
                }})
            }}),
            context_provider: None,
        }}
    }}

    pub fn with_context_provider(mut self, provider: Option<Arc<dyn ContextProvider>>) -> Self {{
        self.context_provider = provider;
        self
    }}

    pub fn with_queries(mut self, queries: LanguageQueries) -> Result<Self> {{
        if let Some(query) = queries.highlights {{
            self = self
                .with_highlights_query(query.as_ref())
                .context("Error loading highlights query")?;
        }}
        if let Some(query) = queries.brackets {{
            self = self
                .with_brackets_query(query.as_ref())
                .context("Error loading brackets query")?;
        }}
        if let Some(query) = queries.indents {{
            self = self
                .with_indents_query(query.as_ref())
                .context("Error loading indents query")?;
        }}
        if let Some(query) = queries.outline {{
            self = self
                .with_outline_query(query.as_ref())
                .context("Error loading outline query")?;
        }}
        if let Some(query) = queries.embedding {{
            self = self
                .with_embedding_query(query.as_ref())
                .context("Error loading embedding query")?;
        }}
        if let Some(query) = queries.injections {{
            self = self
                .with_injection_query(query.as_ref())
                .context("Error loading injection query")?;
        }}
        if let Some(query) = queries.overrides {{
            self = self
                .with_override_query(query.as_ref())
                .context("Error loading override query")?;
        }}
        if let Some(query) = queries.redactions {{
            self = self
                .with_redaction_query(query.as_ref())
                .context("Error loading redaction query")?;
        }}
        if let Some(query) = queries.runnables {{
            self = self
                .with_runnable_query(query.as_ref())
                .context("Error loading tests query")?;
        }}
        Ok(self)
    }}

    pub fn with_highlights_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        grammar.highlights_query = Some(Query::new(&grammar.ts_language, source)?);
        Ok(self)
    }}

    pub fn with_runnable_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;

        let query = Query::new(&grammar.ts_language, source)?;
        let mut extra_captures = Vec::with_capacity(query.capture_names().len());

        for name in query.capture_names().iter() {{
            let kind = if *name == "run" {{
                RunnableCapture::Run
            }} else {{
                RunnableCapture::Named(name.to_string().into())
            }};
            extra_captures.push(kind);
        }}

        grammar.runnable_config = Some(RunnableConfig {{
            extra_captures,
            query,
        }});

        Ok(self)
    }}

    pub fn with_outline_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        let query = Query::new(&grammar.ts_language, source)?;
        let mut item_capture_ix = None;
        let mut name_capture_ix = None;
        let mut context_capture_ix = None;
        let mut extra_context_capture_ix = None;
        get_capture_indices(
            &query,
            &mut [
                ("item", &mut item_capture_ix),
                ("name", &mut name_capture_ix),
                ("context", &mut context_capture_ix),
                ("context.extra", &mut extra_context_capture_ix),
            ],
        );
        if let Some((item_capture_ix, name_capture_ix)) = item_capture_ix.zip(name_capture_ix) {{
            grammar.outline_config = Some(OutlineConfig {{
                query,
                item_capture_ix,
                name_capture_ix,
                context_capture_ix,
                extra_context_capture_ix,
            }});
        }}
        Ok(self)
    }}

    pub fn with_embedding_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        let query = Query::new(&grammar.ts_language, source)?;
        let mut item_capture_ix = None;
        let mut name_capture_ix = None;
        let mut context_capture_ix = None;
        let mut collapse_capture_ix = None;
        let mut keep_capture_ix = None;
        get_capture_indices(
            &query,
            &mut [
                ("item", &mut item_capture_ix),
                ("name", &mut name_capture_ix),
                ("context", &mut context_capture_ix),
                ("keep", &mut keep_capture_ix),
                ("collapse", &mut collapse_capture_ix),
            ],
        );
        if let Some(item_capture_ix) = item_capture_ix {{
            grammar.embedding_config = Some(EmbeddingConfig {{
                query,
                item_capture_ix,
                name_capture_ix,
                context_capture_ix,
                collapse_capture_ix,
                keep_capture_ix,
            }});
        }}
        Ok(self)
    }}

    pub fn with_brackets_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        let query = Query::new(&grammar.ts_language, source)?;
        let mut open_capture_ix = None;
        let mut close_capture_ix = None;
        get_capture_indices(
            &query,
            &mut [
                ("open", &mut open_capture_ix),
                ("close", &mut close_capture_ix),
            ],
        );
        if let Some((open_capture_ix, close_capture_ix)) = open_capture_ix.zip(close_capture_ix) {{
            grammar.brackets_config = Some(BracketConfig {{
                query,
                open_capture_ix,
                close_capture_ix,
            }});
        }}
        Ok(self)
    }}

    pub fn with_indents_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        let query = Query::new(&grammar.ts_language, source)?;
        let mut indent_capture_ix = None;
        let mut start_capture_ix = None;
        let mut end_capture_ix = None;
        let mut outdent_capture_ix = None;
        get_capture_indices(
            &query,
            &mut [
                ("indent", &mut indent_capture_ix),
                ("start", &mut start_capture_ix),
                ("end", &mut end_capture_ix),
                ("outdent", &mut outdent_capture_ix),
            ],
        );
        if let Some(indent_capture_ix) = indent_capture_ix {{
            grammar.indents_config = Some(IndentConfig {{
                query,
                indent_capture_ix,
                start_capture_ix,
                end_capture_ix,
                outdent_capture_ix,
            }});
        }}
        Ok(self)
    }}

    pub fn with_injection_query(mut self, source: &str) -> Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        let query = Query::new(&grammar.ts_language, source)?;
        let mut language_capture_ix = None;
        let mut content_capture_ix = None;
        get_capture_indices(
            &query,
            &mut [
                ("language", &mut language_capture_ix),
                ("content", &mut content_capture_ix),
            ],
        );
        let patterns = (0..query.pattern_count())
            .map(|ix| {{
                let mut config = InjectionPatternConfig::default();
                for setting in query.property_settings(ix) {{
                    match setting.key.as_ref() {{
                        "language" => {{
                            config.language.clone_from(&setting.value);
                        }}
                        "combined" => {{
                            config.combined = true;
                        }}
                        _ => {{}}
                    }}
                }}
                config
            }})
            .collect();
        if let Some(content_capture_ix) = content_capture_ix {{
            grammar.injection_config = Some(InjectionConfig {{
                query,
                language_capture_ix,
                content_capture_ix,
                patterns,
            }});
        }}
        Ok(self)
    }}

    pub fn with_override_query(mut self, source: &str) -> anyhow::Result<Self> {{
        let query = {{
            let grammar = self
                .grammar
                .as_ref()
                .ok_or_else(|| anyhow!("no grammar for language"))?;
            Query::new(&grammar.ts_language, source)?
        }};

        let mut override_configs_by_id = HashMap::default();
        for (ix, name) in query.capture_names().iter().enumerate() {{
            if !name.starts_with('_') {{
                let value = self.config.overrides.remove(*name).unwrap_or_default();
                for server_name in &value.opt_into_language_servers {{
                    if !self
                        .config
                        .scope_opt_in_language_servers
                        .contains(server_name)
                    {{
                        util::debug_panic!("Server {{server_name:?}} has been opted-in by scope {{name:?}} but has not been marked as an opt-in server");
                    }}
                }}

                override_configs_by_id.insert(ix as u32, (name.to_string(), value));
            }}
        }}

        if !self.config.overrides.is_empty() {{
            let keys = self.config.overrides.keys().collect::<Vec<_>>();
            Err(anyhow!(
                "language {{:?}} has overrides in config not in query: {{keys:?}}",
                self.config.name
            ))?;
        }}

        for disabled_scope_name in self
            .config
            .brackets
            .disabled_scopes_by_bracket_ix
            .iter()
            .flatten()
        {{
            if !override_configs_by_id
                .values()
                .any(|(scope_name, _)| scope_name == disabled_scope_name)
            {{
                Err(anyhow!(
                    "language {{:?}} has overrides in config not in query: {{disabled_scope_name:?}}",
                    self.config.name
                ))?;
            }}
        }}

        for (name, override_config) in override_configs_by_id.values_mut() {{
            override_config.disabled_bracket_ixs = self
                .config
                .brackets
                .disabled_scopes_by_bracket_ix
                .iter()
                .enumerate()
                .filter_map(|(ix, disabled_scope_names)| {{
                    if disabled_scope_names.contains(name) {{
                        Some(ix as u16)
                    }} else {{
                        None
                    }}
                }})
                .collect();
        }}

        self.config.brackets.disabled_scopes_by_bracket_ix.clear();

        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;
        grammar.override_config = Some(OverrideConfig {{
            query,
            values: override_configs_by_id,
        }});
        Ok(self)
    }}

    pub fn with_redaction_query(mut self, source: &str) -> anyhow::Result<Self> {{
        let grammar = self
            .grammar_mut()
            .ok_or_else(|| anyhow!("cannot mutate grammar"))?;

        let query = Query::new(&grammar.ts_language, source)?;
        let mut redaction_capture_ix = None;
        get_capture_indices(&query, &mut [("redact", &mut redaction_capture_ix)]);

        if let Some(redaction_capture_ix) = redaction_capture_ix {{
            grammar.redactions_config = Some(RedactionConfig {{
                query,
                redaction_capture_ix,
            }});
        }}

        Ok(self)
    }}

    fn grammar_mut(&mut self) -> Option<&mut Grammar> {{
        Arc::get_mut(self.grammar.as_mut()?)
    }}

    pub fn name(&self) -> Arc<str> {{
        self.config.name.clone()
    }}

    pub fn code_fence_block_name(&self) -> Arc<str> {{
        self.config
            .code_fence_block_name
            .clone()
            .unwrap_or_else(|| self.config.name.to_lowercase().into())
    }}

    pub fn context_provider(&self) -> Option<Arc<dyn ContextProvider>> {{
        self.context_provider.clone()
    }}

    pub fn highlight_text<'a>(
        self: &'a Arc<Self>,
        text: &'a Rope,
        range: Range<usize>,
    ) -> Vec<(Range<usize>, HighlightId)> {{
        let mut result = Vec::new();
        if let Some(grammar) = &self.grammar {{
            let tree = grammar.parse_text(text, None);
            let captures =
                SyntaxSnapshot::single_tree_captures(range.clone(), text, &tree, self, |grammar| {{
                    grammar.highlights_query.as_ref()
                }});
            let highlight_maps = vec![grammar.highlight_map()];
            let mut offset = 0;
            for chunk in BufferChunks::new(text, range, Some((captures, highlight_maps)), vec![]) {{
                let end_offset = offset + chunk.text.len();
                if let Some(highlight_id) = chunk.syntax_highlight_id {{
                    if !highlight_id.is_default() {{
                        result.push((offset..end_offset, highlight_id));
                    }}
                }}
                offset = end_offset;
            }}
        }}
        result
    }}

    pub fn path_suffixes(&self) -> &[String] {{
        &self.config.matcher.path_suffixes
    }}

    pub fn should_autoclose_before(&self, c: char) -> bool {{
        c.is_whitespace() || self.config.autoclose_before.contains(c)
    }}

    pub fn set_theme(&self, theme: &SyntaxTheme) {{
        if let Some(grammar) = self.grammar.as_ref() {{
            if let Some(highlights_query) = &grammar.highlights_query {{
                *grammar.highlight_map.lock() =
                    HighlightMap::new(highlights_query.capture_names(), theme);
            }}
        }}
    }}

    pub fn grammar(&self) -> Option<&Arc<Grammar>> {{
        self.grammar.as_ref()
    }}

    pub fn default_scope(self: &Arc<Self>) -> LanguageScope {{
        LanguageScope {{
            language: self.clone(),
            override_id: None,
        }}
    }}

    pub fn lsp_id(&self) -> String {{
        match self.config.name.as_ref() {{
            "Plain Text" => "plaintext".to_string(),
            language_name => language_name.to_lowercase(),
        }}
    }}

    pub fn prettier_parser_name(&self) -> Option<&str> {{
        self.config.prettier_parser_name.as_deref()
    }}
}}
```
</content>
<symbol_name>
Buffer
</symbol_name>
<fs_file_path>
crates/languages/src/buffer.rs
</fs_file_path>
<content>
```rust
/// An in-memory representation of a source code file, including its text,
/// syntax trees, git status, and diagnostics.
pub struct Buffer {{
    text: TextBuffer,
    diff_base: Option<Rope>,
    git_diff: git::diff::BufferDiff,
    file: Option<Arc<dyn File>>,
    /// The mtime of the file when this buffer was last loaded from
    /// or saved to disk.
    saved_mtime: Option<SystemTime>,
    /// The version vector when this buffer was last loaded from
    /// or saved to disk.
    saved_version: clock::Global,
    transaction_depth: usize,
    was_dirty_before_starting_transaction: Option<bool>,
    reload_task: Option<Task<Result<()>>>,
    language: Option<Arc<Language>>,
    autoindent_requests: Vec<Arc<AutoindentRequest>>,
    pending_autoindent: Option<Task<()>>,
    sync_parse_timeout: Duration,
    syntax_map: Mutex<SyntaxMap>,
    parsing_in_background: bool,
    parse_count: usize,
    diagnostics: SmallVec<[(LanguageServerId, DiagnosticSet); 2]>,
    remote_selections: TreeMap<ReplicaId, SelectionSet>,
    selections_update_count: usize,
    diagnostics_update_count: usize,
    diagnostics_timestamp: clock::Lamport,
    file_update_count: usize,
    git_diff_update_count: usize,
    completion_triggers: Vec<String>,
    completion_triggers_timestamp: clock::Lamport,
    deferred_ops: OperationQueue<Operation>,
    capability: Capability,
    has_conflict: bool,
    diff_base_version: usize,
}}

impl Buffer {{
    /// Create a new buffer with the given base text.
    pub fn local<T: Into<String>>(base_text: T, cx: &mut ModelContext<Self>) -> Self {{
        Self::build(
            TextBuffer::new(0, cx.entity_id().as_non_zero_u64().into(), base_text.into()),
            None,
            None,
            Capability::ReadWrite,
        )
    }}

    /// Assign a language to the buffer, returning the buffer.
    pub fn with_language(mut self, language: Arc<Language>, cx: &mut ModelContext<Self>) -> Self {{
        self.set_language(Some(language), cx);
        self
    }}

    /// Returns the [Capability] of this buffer.
    pub fn capability(&self) -> Capability {{
        self.capability
    }}

    /// Whether this buffer can only be read.
    pub fn read_only(&self) -> bool {{
        self.capability == Capability::ReadOnly
    }}

    /// Builds a [Buffer] with the given underlying [TextBuffer], diff base, [File] and [Capability].
    pub fn build(
        buffer: TextBuffer,
        diff_base: Option<String>,
        file: Option<Arc<dyn File>>,
        capability: Capability,
    ) -> Self {{
        let saved_mtime = file.as_ref().and_then(|file| file.mtime());

        Self {{
            saved_mtime,
            saved_version: buffer.version(),
            reload_task: None,
            transaction_depth: 0,
            was_dirty_before_starting_transaction: None,
            text: buffer,
            diff_base: diff_base
                .map(|mut raw_diff_base| {{
                    LineEnding::normalize(&mut raw_diff_base);
                    raw_diff_base
                }})
                .map(Rope::from),
            diff_base_version: 0,
            git_diff: git::diff::BufferDiff::new(),
            file,
            capability,
            syntax_map: Mutex::new(SyntaxMap::new()),
            parsing_in_background: false,
            parse_count: 0,
            sync_parse_timeout: Duration::from_millis(1),
            autoindent_requests: Default::default(),
            pending_autoindent: Default::default(),
            language: None,
            remote_selections: Default::default(),
            selections_update_count: 0,
            diagnostics: Default::default(),
            diagnostics_update_count: 0,
            diagnostics_timestamp: Default::default(),
            file_update_count: 0,
            git_diff_update_count: 0,
            completion_triggers: Default::default(),
            completion_triggers_timestamp: Default::default(),
            deferred_ops: OperationQueue::new(),
            has_conflict: false,
        }}
    }}

    /// Retrieve a snapshot of the buffer's current state. This is computationally
    /// cheap, and allows reading from the buffer on a background thread.
    pub fn snapshot(&self) -> BufferSnapshot {{
        let text = self.text.snapshot();
        let mut syntax_map = self.syntax_map.lock();
        syntax_map.interpolate(&text);
        let syntax = syntax_map.snapshot();

        BufferSnapshot {{
            text,
            syntax,
            git_diff: self.git_diff.clone(),
            file: self.file.clone(),
            remote_selections: self.remote_selections.clone(),
            diagnostics: self.diagnostics.clone(),
            diagnostics_update_count: self.diagnostics_update_count,
            file_update_count: self.file_update_count,
            git_diff_update_count: self.git_diff_update_count,
            language: self.language.clone(),
            parse_count: self.parse_count,
            selections_update_count: self.selections_update_count,
        }}
    }}

    #[cfg(test)]
    pub(crate) fn as_text_snapshot(&self) -> &text::BufferSnapshot {{
        &self.text
    }}

    /// Retrieve a snapshot of the buffer's raw text, without any
    /// language-related state like the syntax tree or diagnostics.
    pub fn text_snapshot(&self) -> text::BufferSnapshot {{
        self.text.snapshot()
    }}

    /// The file associated with the buffer, if any.
    pub fn file(&self) -> Option<&Arc<dyn File>> {{
        self.file.as_ref()
    }}

    /// The version of the buffer that was last saved or reloaded from disk.
    pub fn saved_version(&self) -> &clock::Global {{
        &self.saved_version
    }}

    /// The mtime of the buffer's file when the buffer was last saved or reloaded from disk.
    pub fn saved_mtime(&self) -> Option<SystemTime> {{
        self.saved_mtime
    }}

    /// Assign a language to the buffer.
    pub fn set_language(&mut self, language: Option<Arc<Language>>, cx: &mut ModelContext<Self>) {{
        self.parse_count += 1;
        self.syntax_map.lock().clear();
        self.language = language;
        self.reparse(cx);
        cx.emit(Event::LanguageChanged);
    }}

    /// Assign a language registry to the buffer. This allows the buffer to retrieve
    /// other languages if parts of the buffer are written in different languages.
    pub fn set_language_registry(&mut self, language_registry: Arc<LanguageRegistry>) {{
        self.syntax_map
            .lock()
            .set_language_registry(language_registry);
    }}

    /// Assign the buffer a new [Capability].
    pub fn set_capability(&mut self, capability: Capability, cx: &mut ModelContext<Self>) {{
        self.capability = capability;
        cx.emit(Event::CapabilityChanged)
    }}

    /// Waits for the buffer to receive operations up to the given version.
    pub fn wait_for_version(&mut self, version: clock::Global) -> impl Future<Output = Result<()>> {{
        self.text.wait_for_version(version)
    }}

    /// Forces all futures returned by [`Buffer::wait_for_version`], [`Buffer::wait_for_edits`], or
    /// [`Buffer::wait_for_version`] to resolve with an error.
    pub fn give_up_waiting(&mut self) {{
        self.text.give_up_waiting();
    }}

    fn did_edit(
        &mut self,
        old_version: &clock::Global,
        was_dirty: bool,
        cx: &mut ModelContext<Self>,
    ) {{
        if self.edits_since::<usize>(old_version).next().is_none() {{
            return;
        }}

        self.reparse(cx);

        cx.emit(Event::Edited);
        if was_dirty != self.is_dirty() {{
            cx.emit(Event::DirtyChanged);
        }}
        cx.notify();
    }}

    /// Applies the given remote operations to the buffer.
    pub fn apply_ops<I: IntoIterator<Item = Operation>>(
        &mut self,
        ops: I,
        cx: &mut ModelContext<Self>,
    ) -> Result<()> {{
        self.pending_autoindent.take();
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        let mut deferred_ops = Vec::new();
        let buffer_ops = ops
            .into_iter()
            .filter_map(|op| match op {{
                Operation::Buffer(op) => Some(op),
                _ => {{
                    if self.can_apply_op(&op) {{
                        self.apply_op(op, cx);
                    }} else {{
                        deferred_ops.push(op);
                    }}
                    None
                }}
            }})
            .collect::<Vec<_>>();
        self.text.apply_ops(buffer_ops)?;
        self.deferred_ops.insert(deferred_ops);
        self.flush_deferred_ops(cx);
        self.did_edit(&old_version, was_dirty, cx);
        // Notify independently of whether the buffer was edited as the operations could include a
        // selection update.
        cx.notify();
        Ok(())
    }}

    fn flush_deferred_ops(&mut self, cx: &mut ModelContext<Self>) {{
        let mut deferred_ops = Vec::new();
        for op in self.deferred_ops.drain().iter().cloned() {{
            if self.can_apply_op(&op) {{
                self.apply_op(op, cx);
            }} else {{
                deferred_ops.push(op);
            }}
        }}
        self.deferred_ops.insert(deferred_ops);
    }}

    fn can_apply_op(&self, operation: &Operation) -> bool {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                diagnostics: diagnostic_set,
                ..
            }} => diagnostic_set.iter().all(|diagnostic| {{
                self.text.can_resolve(&diagnostic.range.start)
                    && self.text.can_resolve(&diagnostic.range.end)
            }}),
            Operation::UpdateSelections {{ selections, .. }} => selections
                .iter()
                .all(|s| self.can_resolve(&s.start) && self.can_resolve(&s.end)),
            Operation::UpdateCompletionTriggers {{ .. }} => true,
        }}
    }}

    fn apply_op(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                server_id,
                diagnostics: diagnostic_set,
                lamport_timestamp,
            }} => {{
                let snapshot = self.snapshot();
                self.apply_diagnostic_update(
                    server_id,
                    DiagnosticSet::from_sorted_entries(diagnostic_set.iter().cloned(), &snapshot),
                    lamport_timestamp,
                    cx,
                );
            }}
            Operation::UpdateSelections {{
                selections,
                lamport_timestamp,
                line_mode,
                cursor_shape,
            }} => {{
                if let Some(set) = self.remote_selections.get(&lamport_timestamp.replica_id) {{
                    if set.lamport_timestamp > lamport_timestamp {{
                        return;
                    }}
                }}

                self.remote_selections.insert(
                    lamport_timestamp.replica_id,
                    SelectionSet {{
                        selections,
                        lamport_timestamp,
                        line_mode,
                        cursor_shape,
                    }},
                );
                self.text.lamport_clock.observe(lamport_timestamp);
                self.selections_update_count += 1;
            }}
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp,
            }} => {{
                self.completion_triggers = triggers;
                self.text.lamport_clock.observe(lamport_timestamp);
            }}
        }}
    }}

    fn apply_diagnostic_update(
        &mut self,
        server_id: LanguageServerId,
        diagnostics: DiagnosticSet,
        lamport_timestamp: clock::Lamport,
        cx: &mut ModelContext<Self>,
    ) {{
        if lamport_timestamp > self.diagnostics_timestamp {{
            let ix = self.diagnostics.binary_search_by_key(&server_id, |e| e.0);
            if diagnostics.len() == 0 {{
                if let Ok(ix) = ix {{
                    self.diagnostics.remove(ix);
                }}
            }} else {{
                match ix {{
                    Err(ix) => self.diagnostics.insert(ix, (server_id, diagnostics)),
                    Ok(ix) => self.diagnostics[ix].1 = diagnostics,
                }};
            }}
            self.diagnostics_timestamp = lamport_timestamp;
            self.diagnostics_update_count += 1;
            self.text.lamport_clock.observe(lamport_timestamp);
            cx.notify();
            cx.emit(Event::DiagnosticsUpdated);
        }}
    }}

    fn send_operation(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        cx.emit(Event::Operation(operation));
    }}

    /// Removes the selections for a given peer.
    pub fn remove_peer(&mut self, replica_id: ReplicaId, cx: &mut ModelContext<Self>) {{
        self.remote_selections.remove(&replica_id);
        cx.notify();
    }}

    /// Undoes the most recent transaction.
    pub fn undo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.undo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Manually undoes a specific transaction in the buffer's undo history.
    pub fn undo_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        if let Some(operation) = self.text.undo_transaction(transaction_id) {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            true
        }} else {{
            false
        }}
    }}

    /// Manually undoes all changes after a given transaction in the buffer's undo history.
    pub fn undo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.undo_to_transaction(transaction_id);
        let undone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if undone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        undone
    }}

    /// Manually redoes a specific transaction in the buffer's redo history.
    pub fn redo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.redo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Returns the primary [Language] assigned to this [Buffer].
    pub fn language(&self) -> Option<&Arc<Language>> {{
        self.language.as_ref()
    }}

    /// Manually undoes all changes until a given transaction in the buffer's redo history.
    pub fn redo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.redo_to_transaction(transaction_id);
        let redone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if redone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        redone
    }}

    /// Override current completion triggers with the user-provided completion triggers.
    pub fn set_completion_triggers(&mut self, triggers: Vec<String>, cx: &mut ModelContext<Self>) {{
        self.completion_triggers.clone_from(&triggers);
        self.completion_triggers_timestamp = self.text.lamport_clock.tick();
        self.send_operation(
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp: self.completion_triggers_timestamp,
            }},
            cx,
        );
        cx.notify();
    }}

    /// Returns a list of strings which trigger a completion menu for this language.
    /// Usually this is driven by LSP server which returns a list of trigger characters for completions.
    pub fn completion_triggers(&self) -> &[String] {{
        &self.completion_triggers
    }}
}}
```
</content>
</extra_data>
<extra_data>


Your reply will start now and you should reply strictly in the following format:
<reply>
<steps_to_answer>
- For answering the user query we have to understand how we are getting the prettier config from the language
- `prettier_settings` is being used today to get the prettier parser
- the language configuration seems to be contained in `prettier is not allowed for language {{buffer_language:?}}"`
- we are getting the language configuration by calling `buffer_language` function
- we should check what `language` returns to understand what properies `buffer_language` has
</steps_to_answer>
<symbol_list>
<symbol>
<name>
Language
</name>
<line_content>
    pub fn language(&self) -> Option<&Arc<Language>> {{
</line_content>
<file_path>
crates/language/src/buffer.rs
</file_path>
<thinking>
Does the language type expose any prettier settings, because we want to get it so we can use that as the fallback
</thinking>
</symbol>
<symbol>
</symbol_list>
</reply>"#
        )
    }

    fn system_message_for_summarizing_probe_result(&self) -> String {
        r#"You are an expert software engineer who is an expert at summarising the work done by other engineers who are trying to more deeply answer user queries.
- The user query is present in <user_query> section.
- You are working on the <current_symbol> and the other engineers have looked at various sections of this symbol and generted answers which would help you better answer the <user_query>
- The other engineers have all looked at various portions of the code and tried to answer the user query you can see what they found in <sub_symbol_probing> section.
- You have to answer the <user_query> after understanding the questions and the responses we got from following the other symbols which the other engineers have already done.
- When summarising the information make sure that if you think some symbol is important you keep track of the file name and the name of the symbol as well, this is extremely important.

Below we are showing you an example of how the input will look like:
<user_query>
Do we store the author name along with the movies?
</user_query>

<history>
<item>
<symbol>
City
</symbol>
<file_path>
city/mod.rs
</file_path>
<content>
```rust
impl City {
    fn get_theaters(&self) -> &[Theater] {
        self.theaters.as_slice
    }
}
```
</content>
<question>
Do we store the actors for each movie?
</question>
</item>
</histroy>

<current_symbol>
<symbol_name>
Theater
</symbol_name>
<content>
use artwork::movies::Movie;
use artwork::location::Location;

#[derive(Debug)]
struct Theater {
    movies: Vec<Movie>,
    location: Location,
    max_capacity: usize,
}

impl Theater {
    fn movies(&self) -> &[Movie] {
        self.movies.as_slice()
    }

    fn location(&self) -> &Location {
        &self.location
    }

    fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}
</content>
</current_symbol>

<sub_symbol_probing>
<symbol>
<name>
movies
</name>
<file_path>
src/movies.rs
</file_path>
<content>
```rust
    fn movies(&self) -> &[Movie] {
        self.movies.as_slice()
    }
```
</content>
<probing_results>
Movie defined in src/movies.rs does not contain actors or any reference to it as the structure of Movie has no reference to actors.
```rust
struct Movie {
    name: String,
    lenght_in_seconds: usize,
}
```
</probing_results>
</symbol>
</sub_symbol_probing>

Your reply:
<reply>
After following `movies` function in `Theater` we can see that `Movie` defined in src/movies.rs does not contain actors as a field and it is missing it from the definition.
</reply>

Your reply should always be contained in <reply> tags, and the summarized result in between the tags.
"#.to_owned()
    }

    fn user_message_for_summarizing_probe_result(
        &self,
        request: CodeSymbolProbingSummarize,
    ) -> String {
        let user_query = request.user_query();
        let history = request.history();
        let symbol_name = request.symbol_identifier();
        let symbol_content = request.symbol_outline();
        let symbol_probing_results = request.symbol_probing_results();
        format!(
            r#"<user_query>
{user_query}
</user_query>

<history>
{history}
</history>

<current_symbol>
<symbol_name>
{symbol_name}
</symbol_name>
<content>
{symbol_content}
</content>
</current_symbol>

<sub_symbol_probing>
{symbol_probing_results}
</sub_symbol_probing>"#
        )
    }

    fn system_message_for_probe_next_symbol(&self) -> String {
        r#"You are an expert software engineer who is an expert at deciding if we have enough information to answer the user query or we need to look deeper in the codebase while working in an editor.
- You are given this history of the various clicks we have done up until now in the editor to get to the current location in the codebase in <history> tag.
- You are also given some extra symbols which we have accumulated up until now in the <extra_data> section.
- Our current position is present in <file> section under the <code_in_selection> tag. This is the code snippet we are focussed on and where we initiated the jump to the next symbol either by clicking go-to-definition, or go-to-implementation.
- We have determined the next symbols we can jump to, and ask depeer question in <next_symbols> section. We will show you the list of next symbols in <next_symbol_names> too. This is because we followed a previous function or class or variable and decided to go to the definition.
- The reason why one of the next symbols is possible is also show to you in <reason_for_next_symbol> which contains the position where we clicked to go to the next symbol in <jump_to_next_symbol> tag. This gives you an idea for why we are jumping and the link between the current position we are in and the next symbol we want to jump to.
- Since asking for a new question to another symbol takes time, we advice you to think hard and decide if you really want to go deeper or you have enough information to answer the user query.
You are given 3 tools which you can use as your response:
1. <answer_user_query>
If you choose answer user query, then you have enough information to answer the user query and your reply should contain the answer to the user query.
The format for this tool use is:
<answer_user_query>
{your answer here}
</answer_user_query>
2. <should_follow>
If you choose to follow the next symbol, then your reply should contain the question you want to ask the next symbol.
The format for this tool use is:
<should_follow>
<name>
{name of the symbol to follow should be one of the names in <next_symbol_names>}
</name>
<file_path>
{file path of the symbol to follow without the line-numbers}
</file_path>
<reason>
{your question for the next symbol considering every other context which has been provided to you}
</reason>
</should_follow>
3. <wrong_path>
If you believe going depeer in this path will not return an answer, you can stop here and reply with the reason why you think following the next symbol will not yield the answer
The format for this tool use is
<wrong_path>
{The reason why this is the wrong path to follow}
</wrong_path>
You must choose one of the 3 tools always!

Below we show you an example of what the input will look like:

<user_query>
Theaters might have information about the movies, we want to check if movies have authors.
</user_query>

<file>
<file_path>
artwork/theather.rs
</file_path>
<code_above>
```rust
use artwork::movies::Movie;
use artwork::location::Location;

#[derive(Debug)]
struct Theater {
    movies: Vec<Movie>,
    location: Location,
    max_capacity: usize,
}
```
```
</code_above>
<code_below>
```rust
    fn location(&self) -> &Location {
        &self.location
    }

    fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}
```
</code_below>
<code_in_selection>
```rust
impl Theater {
    fn movies(&self) -> &[Movie] {
        self.movies.as_slice()
    }

```
</code_in_selection>
</file>

<history>
<item>
<symbol>
City
</symbol>
<file_path>
city/mod.rs
</file_path>
<content>
```rust
impl City {
    fn get_theaters(&self) -> &[Theater] {
        self.theaters.as_slice
    }
}
```
</content>
<question>
Do we store the actors for each movie?
</question>
</item>
</histroy>

<extra_data>
</extra_data>

<next_symbol_names>
Movie
</next_symbol_names>

<next_symbols>
<file_path>
artwork/movies.rs
</file_path>
<content>
```rust
struct Movie {
    name: String,
    lenght_in_seconds: usize,
}

impl Movie {
    fn name(&self) -> &str {
        &self.name
    }

    fn length_in_seconds(&self) -> usize {
        self.length_in_seconds
    }
}
```
</content>
</next_symbols>

<jump_to_next_symbol>
We followed `Movie` as it was the return type for `movies` function in `Theater`. We can now learn if movie has the information about the actors.
</jump_to_next_symbol>

Your reply:
<tool>
<answer_user_query>
We can already see that the movie type does not have the actors so we can answer the user query at this point. The code here illustrates this:
```rust
struct Movie {
    name: String,
    lenght_in_seconds: usize,
}
```
</answer_user_query>
</tool>

You can understand from the response here how we did not have to follow the next symbol since we can already see that the movie struct does not contain the actors."#.to_owned()
    }

    fn user_message_for_probe_next_symbol(
        &self,
        request: CodeSymbolFollowAlongForProbing,
    ) -> String {
        let user_query = request.user_query();
        let file_path = request.file_path();
        let language = request.language();
        let code_above = request.code_above().unwrap_or("".to_owned());
        let code_below = request.code_below().unwrap_or("".to_owned());
        let code_in_selection = request.code_in_selection();
        let history = request.history();
        let next_symbol_outline = request.next_symbol_outline().join("\n");
        let next_symbol_names = request.next_symbol_names().join("\n");
        let reference_link = request.next_symbol_link();
        format!(
            r#"<user_query>
{user_query}
</user_query>

<file>
<file_path>
{file_path}
</file_path>
<code_above>
```{language}
{code_above}
```
</code_above>
<code_below>
```{language}
{code_below}
```
</code_below>
<code_in_selection>
```{language}
{code_in_selection}
```
</code_in_selection>
</file>

<history>
{history}
</history>

<next_symbol_names>
{next_symbol_names}
</next_symbol_names>

<next_symbols>
{next_symbol_outline}
</next_symbols>

<jump_to_next_symbol>
{reference_link}
</jump_to_next_symbol>"#
        )
    }

    fn user_message_for_should_ask_questions(
        &self,
        request: CodeSymbolToAskQuestionsRequest,
    ) -> String {
        let user_query = request.user_query();
        let file_path = request.fs_file_path();
        let code_above = request.code_above().unwrap_or("".to_owned());
        let code_below = request.code_below().unwrap_or("".to_owned());
        let code_in_selection = request.code_in_selection();
        let history = request.history();
        let extra_data = request.extra_data();
        format!(
            r#"<user_query>
{user_query}
</user_query>

<file>
<file_path>
{file_path}
</file_path>
<code_above>
{code_above}
</code_above>
<code_below>
{code_below}
</code_below>
<code_in_selection>
{code_in_selection}
</code_in_selection>
</file>

<history>
{history}
</history>

<extra_data>
{extra_data}
</extra_data>"#
        )
    }
    fn system_message_for_should_ask_questions(&self) -> String {
        r#"You are an expert software engineer who is going to decide if the context you have up until now is enough to answer the user request and we are working in the context of a code editor or IDE which implies we have operations like go-to-definition , go-to-reference, go-to-implementation.
- You are focussed on the code which is present in <code_selection> section, additionally you are also show the code above and below the selection in <code_above> and <code_below> sections.
- You are given this history of the various clicks we have done up until now in the editor to get to the current location in the codebase in <history> tag.
- The context you have accumulated up until now is shown to you in <history> and <extra_data> section.
- You are responsible for telling us if you have enough context from <history> section and the <extra_data> section along with the code you can see right now to answer the user query.
- You have to decide if you can answer the user query or you need to see more code sections before you are able to answer the user query.
- From the code in <code_in_selection> you can go deeper into
- Your reply should be in the following format:
<reply>
<thinking>
{your reason for deciding if we need to dive deeper into the code to completely answer the user query.}
</thinking>
<context_enough>
{reply with either true or false} if in your <thinking> you realise that we should follow some more symbols to completely answer the question, then reply with flase otherwise reply with true
</context_enough>
</reply>

Below we show you an example of what the input will look like:

<user_query>
What are the different kind of providers we support using `LLMProvider`?
</user_query>

<file>
<file_path>
llm_client/src/provider.rs
</file_path>
<code_above>
```rust
//! Contains types for setting the provider for the LLM, we are going to support
//! 3 things for now:
//! - CodeStory
//! - OpenAI
//! - Ollama
//! - Azure
//! - together.ai

use crate::clients::types::LLMType;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct AzureOpenAIDeploymentId {
    pub deployment_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct CodeStoryLLMTypes {
    // shoehorning the llm type here so we can provide the correct api keys
    pub llm_type: Option<LLMType>,
}

impl CodeStoryLLMTypes {
    pub fn new() -> Self {
        Self { llm_type: None }
    }
}
```
</code_above>
<code_below>
```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum LLMProviderAPIKeys {
    OpenAI(OpenAIProvider),
    TogetherAI(TogetherAIProvider),
    Ollama(OllamaProvider),
    OpenAIAzureConfig(AzureConfig),
    LMStudio(LMStudioConfig),
    OpenAICompatible(OpenAICompatibleConfig),
    CodeStory,
    Anthropic(AnthropicAPIKey),
    FireworksAI(FireworksAPIKey),
    GeminiPro(GeminiProAPIKey),
}

impl LLMProviderAPIKeys {
    pub fn is_openai(&self) -> bool {
        matches!(self, LLMProviderAPIKeys::OpenAI(_))
    }

    pub fn provider_type(&self) -> LLMProvider {
        match self {
            LLMProviderAPIKeys::OpenAI(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::TogetherAI(_) => LLMProvider::TogetherAI,
            LLMProviderAPIKeys::Ollama(_) => LLMProvider::Ollama,
            LLMProviderAPIKeys::OpenAIAzureConfig(_) => {
                LLMProvider::Azure(AzureOpenAIDeploymentId {
                    deployment_id: "".to_owned(),
                })
            }
            LLMProviderAPIKeys::LMStudio(_) => LLMProvider::LMStudio,
            LLMProviderAPIKeys::CodeStory => {
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
            }
            LLMProviderAPIKeys::OpenAICompatible(_) => LLMProvider::OpenAICompatible,
            LLMProviderAPIKeys::Anthropic(_) => LLMProvider::Anthropic,
            LLMProviderAPIKeys::FireworksAI(_) => LLMProvider::FireworksAI,
            LLMProviderAPIKeys::GeminiPro(_) => LLMProvider::GeminiPro,
        }
    }

    // Gets the relevant key from the llm provider
    pub fn key(&self, llm_provider: &LLMProvider) -> Option<Self> {
        match llm_provider {
            LLMProvider::OpenAI => {
                if let LLMProviderAPIKeys::OpenAI(key) = self {
                    Some(LLMProviderAPIKeys::OpenAI(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::TogetherAI => {
                if let LLMProviderAPIKeys::TogetherAI(key) = self {
                    Some(LLMProviderAPIKeys::TogetherAI(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::Ollama => {
                if let LLMProviderAPIKeys::Ollama(key) = self {
                    Some(LLMProviderAPIKeys::Ollama(key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::LMStudio => {
                if let LLMProviderAPIKeys::LMStudio(key) = self {
                    Some(LLMProviderAPIKeys::LMStudio(key.clone()))
                } else {
                    None
                }
            }
            // Azure is weird, so we are have to copy the config which we get
            // from the provider keys and then set the deployment id of it
            // properly for the azure provider, if its set to "" that means
            // we do not have a deployment key and we should be returning quickly
            // here.
            // NOTE: We should change this to using the codestory configuration
            // and make calls appropriately, for now this is fine
            LLMProvider::Azure(deployment_id) => {
                if deployment_id.deployment_id == "" {
                    return None;
                }
                if let LLMProviderAPIKeys::OpenAIAzureConfig(key) = self {
                    let mut azure_config = key.clone();
                    azure_config.deployment_id = deployment_id.deployment_id.to_owned();
                    Some(LLMProviderAPIKeys::OpenAIAzureConfig(azure_config))
                } else {
                    None
                }
            }
            LLMProvider::CodeStory(_) => Some(LLMProviderAPIKeys::CodeStory),
            LLMProvider::OpenAICompatible => {
                if let LLMProviderAPIKeys::OpenAICompatible(openai_compatible) = self {
                    Some(LLMProviderAPIKeys::OpenAICompatible(
                        openai_compatible.clone(),
                    ))
                } else {
                    None
                }
            }
            LLMProvider::Anthropic => {
                if let LLMProviderAPIKeys::Anthropic(api_key) = self {
                    Some(LLMProviderAPIKeys::Anthropic(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::FireworksAI => {
                if let LLMProviderAPIKeys::FireworksAI(api_key) = self {
                    Some(LLMProviderAPIKeys::FireworksAI(api_key.clone()))
                } else {
                    None
                }
            }
            LLMProvider::GeminiPro => {
                if let LLMProviderAPIKeys::GeminiPro(api_key) = self {
                    Some(LLMProviderAPIKeys::GeminiPro(api_key.clone()))
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OpenAIProvider {
    pub api_key: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TogetherAIProvider {
    pub api_key: String,
}

impl TogetherAIProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GeminiProAPIKey {
    pub api_key: String,
    pub api_base: String,
}
```
</code_below>
<code_in_selection>
```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub enum LLMProvider {
    OpenAI,
    TogetherAI,
    Ollama,
    LMStudio,
    CodeStory(CodeStoryLLMTypes),
    Azure(AzureOpenAIDeploymentId),
    OpenAICompatible,
    Anthropic,
    FireworksAI,
    GeminiPro,
}

impl LLMProvider {
    pub fn is_codestory(&self) -> bool {
        matches!(self, LLMProvider::CodeStory(_))
    }

    pub fn is_anthropic_api_key(&self) -> bool {
        matches!(self, LLMProvider::Anthropic)
    }
}
```
</code_in_selection>
</file>

<history>
<item>
<file_path>
sidecar/src/agentic/tool/filtering/broker.rs:136-143
</file_path>
<content>
```rust
#[derive(Debug, Clone)]
pub struct CodeToEditFilterRequest {
    snippets: Vec<Snippet>,
    query: String,
    llm: LLMType,
    provider: LLMProvider,
    api_key: LLMProviderAPIKeys,
}
```
</content>
<question>
What are the various providers we support?
</question>
</item>
</history>

<extra_data>
</extra_data>


Your reply is:
<reply>
<thinking>
We can see the different names of the providers which is contained in the enum `LLMProvider`
</thinking>
<context_enough>
false
</context_enough>
</reply>


Here's another example where we need to ask deeper questions:

<user_query>
Do movies store the actors along with them?
</user_query>

<file>
<file_path>
src/theater.rs
</file_path>
<code_above>
</code_above>
<code_below>
</code_below>
<code_in_selection>
```rust
use artwork::movies::Movie;
use artwork::location::Location;

#[derive(Debug)]
struct Theater {
    movies: Vec<Movie>,
    location: Location,
    max_capacity: usize,
}

impl Theater {
    fn movies(&self) -> &[Movie] {
        self.movies.as_slice()
    }

    fn location(&self) -> &Location {
        &self.location
    }

    fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}
```
</code_in_selection>
</file>

<history>
</history>

<extra_data>
</extra_data>

Your reply should be:
<reply>
<thinking>
We should look at `Movie` defined in the movie function to understand if actors are stored with the moveis.
</thinking>
<context_enough>
true
</context_enough>
</reply>

Some more examples of when you should say <context_enough>flase</context_enough> for certain <thinking>

Example 1:
<reply>
<thinking>
we need to look at methods of XYZ class? (where XYZ is a placeholder for anything)
<thinking>
<context_enough>
false
</context_enough>
</reply>

Example 2:
<reply>
<thinking>
I now understand how function_xyz is implemented, I am able to answer the user query (where XYZ is a placeholder)
</thinking>
<context_enough>
false
</context_enough>
</reply>

Example 3:
<reply>
<thinking>
The `Agent::prepare_for_search` function sets up an `Agent` instance to handle a search query and generate a final answer using the LLM client. The `Agent::answer` function is where the LLM client is invoked to generate the final answer based on the search results and code snippets. To understand how the LLM client is invoked, we should follow the implementation of `Agent::answer`.
</thinking>
<context_enough>
false
</context_enough>
</reply>

Example 4:
<reply>
<thinking>
The `ConversationMessage` contains the user id and we are storing `user_id` it uniquely with every message
</thinking>
<context_enough>
true
</context_enough>
</reply>"#.to_owned()
    }

    fn system_message_for_ask_question_symbols(
        &self,
        symbol_name: &str,
        fs_file_path: &str,
    ) -> String {
        format!(
            r#"You are an expert software engineer who is going to look at some more symbols to deeply answer the user query.
- You are reponsible for answering the <user_query>
- You are also given the implementations of some of the code symbols we have gathered since these are important to answer the user query.
- You are reponsible for any changes which need to be made to <symbol_name>{symbol_name}</symbol_name> present in file {fs_file_path}
- The user query is given to you in <user_query>
- The <user_query> is one of the following use case:
<use_case_list>
<use_case>
Reference change: A code symbol struct, implementation, trait, subclass, enum, function, constant or type has changed, you have to figure out what needs to be changed and we want to make some changes to our own implementation
</use_case>
<use_case>
We are going to add more functionality which the user wants to create using the <user_query>
</use_case>
<use_case>
The <user_query> is about asking for some important information
</use_case>
<use_case>
<user_query> wants to gather more information about a particular workflow which the user has asked for in their query
</use_case>
</use_case_list>
- You have to select at the most 5 symbols and you can do the following:
<operation>
You can initiate a read operation on one of the symbols and gather more information before answering the user query. This allows you to understand the complete picture of how to answer the user query. You can do this in form of another question to the symbol, so they can answer it for you.
</operation>
- Now first think step by step on how you are going to approach this problem and write down your steps in <steps_to_answer> section
- Here are some approaches and questions you can ask for the symbol:
<approach_list>
<approach>
You can ask to follow a given symbol more deeply. This allows you to focus on a symbol more deeply and ask deeper questions for the symbol
For example you might want to follow the function call more deeply if say the function returns back the symbol
```rust
fn some_function(parameter1: Parameter1, parameter2: Parameter2) -> ReturnType {{
    // rest of the code
}}
```
Here you might want to follow the `ReturnType` to understand more about how the symbol is constructed or what kind of functionality is available on it.
</approach>
<approach>
You suspect that the some code symbol in the code selection can more deeply answer the user query, so its worthwhile following that symbol and asking it a deeper question.
</approach>
</approach_list>
These are just examples of approaches you can take, keep them in mind and be more imaginative when answering the user query.
- You will also be given the <history> of sequence of questions which have been asked, this allows you to keep track of every interaction that has happend up until now and use that for figuring out your next set of symbols to select.
- Your reply should be in the <reply> tag

We are now going to show you an example:
<user_query>
I want to try and grab the prettier settings from the language config as a fall back instead of erroring out
</user_query>
<file>
<file_path>
crates/prettier/src/prettier.rs
</file_path>
<code_above>
```rust
impl Parser {{
    #[cfg(not(any(test, feature = "test-support")))]
    pub async fn start(
        server_id: LanguageServerId,
        prettier_dir: PathBuf,
        node: Arc<dyn NodeRuntime>,
        cx: AsyncAppContext,
    ) -> anyhow::Result<Self> {{
        use lsp::LanguageServerBinary;
        let executor = cx.background_executor().clone();
        anyhow::ensure!(
            prettier_dir.is_dir(),
            "Prettier dir {{prettier_dir:?}} is not a directory"
        );
        let prettier_server = DEFAULT_PRETTIER_DIR.join(PRETTIER_SERVER_FILE);
        anyhow::ensure!(
            prettier_server.is_file(),
            "no prettier server package found at {{prettier_server:?}}"
        );
        let node_path = executor
            .spawn(async move {{ node.binary_path().await }})
            .await?;
        let server = LanguageServer::new(
            Arc::new(parking_lot::Mutex::new(None)),
            server_id,
            LanguageServerBinary {{
                path: node_path,
                arguments: vec![prettier_server.into(), prettier_dir.as_path().into()],
                env: None,
            }},
            &prettier_dir,
            None,
            cx.clone(),
        )
        .context("prettier server creation")?;
        let server = cx
            .update(|cx| executor.spawn(server.initialize(None, cx)))?
            .await
            .context("prettier server initialization")?;
        Ok(Self::Real(RealPrettier {{
            server,
            default: prettier_dir == DEFAULT_PRETTIER_DIR.as_path(),
            prettier_dir,
        }}))
    }}
```
</code_above>
<code_below>
```rust
    pub async fn clear_cache(&self) -> anyhow::Result<()> {{
        match self {{
            Self::Real(local) => local
                .server
                .request::<ClearCache>(())
                .await
                .context("prettier clear cache"),
            #[cfg(any(test, feature = "test-support"))]
            Self::Test(_) => Ok(()),
        }}
    }}
}}
```
</code_below>
<code_in_selection>
```rust
    pub async fn format(
        &self,
        buffer: &Model<Buffer>,
        buffer_path: Option<PathBuf>,
        cx: &mut AsyncAppContext,
    ) -> anyhow::Result<Diff> {{
        match self {{
            Self::Real(local) => {{
                let params = buffer
                    .update(cx, |buffer, cx| {{
                        let buffer_language = buffer.language();
                        let language_settings = language_settings(buffer_language, buffer.file(), cx);
                        let prettier_settings = &language_settings.prettier;
                        anyhow::ensure!(
                            prettier_settings.allowed,
                            "Cannot format: prettier is not allowed for language {{buffer_language:?}}"
                        );
                        let prettier_node_modules = self.prettier_dir().join("node_modules");
                        anyhow::ensure!(
                            prettier_node_modules.is_dir(),
                            "Prettier node_modules dir does not exist: {{prettier_node_modules:?}}"
                        );
                        let plugin_name_into_path = |plugin_name: &str| {{
                            let prettier_plugin_dir = prettier_node_modules.join(plugin_name);
                            [
                                prettier_plugin_dir.join("dist").join("index.mjs"),
                                prettier_plugin_dir.join("dist").join("index.js"),
                                prettier_plugin_dir.join("dist").join("plugin.js"),
                                prettier_plugin_dir.join("index.mjs"),
                                prettier_plugin_dir.join("index.js"),
                                prettier_plugin_dir.join("plugin.js"),
                                // this one is for @prettier/plugin-php
                                prettier_plugin_dir.join("standalone.js"),
                                prettier_plugin_dir,
                            ]
                            .into_iter()
                            .find(|possible_plugin_path| possible_plugin_path.is_file())
                        }};
                        // Tailwind plugin requires being added last
                        // https://github.com/tailwindlabs/prettier-plugin-tailwindcss#compatibility-with-other-prettier-plugins
                        let mut add_tailwind_back = false;
                        let mut located_plugins = prettier_settings.plugins.iter()
                            .filter(|plugin_name| {{
                                if plugin_name.as_str() == TAILWIND_PRETTIER_PLUGIN_PACKAGE_NAME {{
                                    add_tailwind_back = true;
                                    false
                                }} else {{
                                    true
                                }}
                            }})
                            .map(|plugin_name| {{
                                let plugin_path = plugin_name_into_path(plugin_name);
                                (plugin_name.clone(), plugin_path)
                            }})
                            .collect::<Vec<_>>();
                        if add_tailwind_back {{
                            located_plugins.push((
                                TAILWIND_PRETTIER_PLUGIN_PACKAGE_NAME.to_owned(),
                                plugin_name_into_path(TAILWIND_PRETTIER_PLUGIN_PACKAGE_NAME),
                            ));
                        }}
                        let prettier_options = if self.is_default() {{
                            let mut options = prettier_settings.options.clone();
                            if !options.contains_key("tabWidth") {{
                                options.insert(
                                    "tabWidth".to_string(),
                                    serde_json::Value::Number(serde_json::Number::from(
                                        language_settings.tab_size.get(),
                                    )),
                                );
                            }}
                            if !options.contains_key("printWidth") {{
                                options.insert(
                                    "printWidth".to_string(),
                                    serde_json::Value::Number(serde_json::Number::from(
                                        language_settings.preferred_line_length,
                                    )),
                                );
                            }}
                            if !options.contains_key("useTabs") {{
                                options.insert(
                                    "useTabs".to_string(),
                                    serde_json::Value::Bool(language_settings.hard_tabs),
                                );
                            }}
                            Some(options)
                        }} else {{
                            None
                        }};
                        let plugins = located_plugins
                            .into_iter()
                            .filter_map(|(plugin_name, located_plugin_path)| {{
                                match located_plugin_path {{
                                    Some(path) => Some(path),
                                    None => {{
                                        log::error!("Have not found plugin path for {{plugin_name:?}} inside {{prettier_node_modules:?}}");
                                        None
                                    }}
                                }}
                            }})
                            .collect();

                            if prettier_settings.parser.is_none() && buffer_path.is_none() {{
                                log::error!("Formatting unsaved file with prettier failed. No prettier parser configured for language");
                                return Err(anyhow!("Cannot determine prettier parser for unsaved file"));
                        }}

                        log::debug!(
                            "Formatting file {{:?}} with prettier, plugins :{{:?}}, options: {{:?}}",
                            buffer.file().map(|f| f.full_path(cx)),
                            plugins,
                            prettier_options,
                        );
                        anyhow::Ok(FormatParams {{
                            text: buffer.text(),
                            options: FormatOptions {{
                                parser: prettier_settings.parser.clone(),
                                parser: prettier_parser.map(ToOwned::to_owned),
                                plugins,
                                path: buffer_path,
                                prettier_options,
                            }},
                        }})
                    }})?
                    .context("prettier params calculation")?;
                let response = local
                    .server
                    .request::<Format>(params)
                    .await
                    .context("prettier format request")?;
                let diff_task = buffer.update(cx, |buffer, cx| buffer.diff(response.text, cx))?;
                Ok(diff_task.await)
            }}
            #[cfg(any(test, feature = "test-support"))]
            Self::Test(_) => Ok(buffer
                .update(cx, |buffer, cx| {{
                    match buffer
                        .language()
                        .map(|language| language.lsp_id())
                        .as_deref()
                    {{
                        Some("rust") => anyhow::bail!("prettier does not support Rust"),
                        Some(_other) => {{
                            let formatted_text = buffer.text() + FORMAT_SUFFIX;
                            Ok(buffer.diff(formatted_text, cx))
                        }}
                        None => panic!("Should not format buffer without a language with prettier"),
                    }}
                }})??
                .await),
        }}
    }}
```
</code_in_selection>
</file>
<extra_data>
<symbol_name>
Buffer
</symbol_name>
<fs_file_path>
crates/languages/src/buffer.rs
</fs_file_path>
<content>
```rust
/// An in-memory representation of a source code file, including its text,
/// syntax trees, git status, and diagnostics.
pub struct Buffer {{
    text: TextBuffer,
    diff_base: Option<Rope>,
    git_diff: git::diff::BufferDiff,
    file: Option<Arc<dyn File>>,
    /// The mtime of the file when this buffer was last loaded from
    /// or saved to disk.
    saved_mtime: Option<SystemTime>,
    /// The version vector when this buffer was last loaded from
    /// or saved to disk.
    saved_version: clock::Global,
    transaction_depth: usize,
    was_dirty_before_starting_transaction: Option<bool>,
    reload_task: Option<Task<Result<()>>>,
    language: Option<Arc<Language>>,
    autoindent_requests: Vec<Arc<AutoindentRequest>>,
    pending_autoindent: Option<Task<()>>,
    sync_parse_timeout: Duration,
    syntax_map: Mutex<SyntaxMap>,
    parsing_in_background: bool,
    parse_count: usize,
    diagnostics: SmallVec<[(LanguageServerId, DiagnosticSet); 2]>,
    remote_selections: TreeMap<ReplicaId, SelectionSet>,
    selections_update_count: usize,
    diagnostics_update_count: usize,
    diagnostics_timestamp: clock::Lamport,
    file_update_count: usize,
    git_diff_update_count: usize,
    completion_triggers: Vec<String>,
    completion_triggers_timestamp: clock::Lamport,
    deferred_ops: OperationQueue<Operation>,
    capability: Capability,
    has_conflict: bool,
    diff_base_version: usize,
}}

impl Buffer {{
    /// Create a new buffer with the given base text.
    pub fn local<T: Into<String>>(base_text: T, cx: &mut ModelContext<Self>) -> Self {{
        Self::build(
            TextBuffer::new(0, cx.entity_id().as_non_zero_u64().into(), base_text.into()),
            None,
            None,
            Capability::ReadWrite,
        )
    }}

    /// Assign a language to the buffer, returning the buffer.
    pub fn with_language(mut self, language: Arc<Language>, cx: &mut ModelContext<Self>) -> Self {{
        self.set_language(Some(language), cx);
        self
    }}

    /// Returns the [Capability] of this buffer.
    pub fn capability(&self) -> Capability {{
        self.capability
    }}

    /// Whether this buffer can only be read.
    pub fn read_only(&self) -> bool {{
        self.capability == Capability::ReadOnly
    }}

    /// Builds a [Buffer] with the given underlying [TextBuffer], diff base, [File] and [Capability].
    pub fn build(
        buffer: TextBuffer,
        diff_base: Option<String>,
        file: Option<Arc<dyn File>>,
        capability: Capability,
    ) -> Self {{
        let saved_mtime = file.as_ref().and_then(|file| file.mtime());

        Self {{
            saved_mtime,
            saved_version: buffer.version(),
            reload_task: None,
            transaction_depth: 0,
            was_dirty_before_starting_transaction: None,
            text: buffer,
            diff_base: diff_base
                .map(|mut raw_diff_base| {{
                    LineEnding::normalize(&mut raw_diff_base);
                    raw_diff_base
                }})
                .map(Rope::from),
            diff_base_version: 0,
            git_diff: git::diff::BufferDiff::new(),
            file,
            capability,
            syntax_map: Mutex::new(SyntaxMap::new()),
            parsing_in_background: false,
            parse_count: 0,
            sync_parse_timeout: Duration::from_millis(1),
            autoindent_requests: Default::default(),
            pending_autoindent: Default::default(),
            language: None,
            remote_selections: Default::default(),
            selections_update_count: 0,
            diagnostics: Default::default(),
            diagnostics_update_count: 0,
            diagnostics_timestamp: Default::default(),
            file_update_count: 0,
            git_diff_update_count: 0,
            completion_triggers: Default::default(),
            completion_triggers_timestamp: Default::default(),
            deferred_ops: OperationQueue::new(),
            has_conflict: false,
        }}
    }}

    /// Retrieve a snapshot of the buffer's current state. This is computationally
    /// cheap, and allows reading from the buffer on a background thread.
    pub fn snapshot(&self) -> BufferSnapshot {{
        let text = self.text.snapshot();
        let mut syntax_map = self.syntax_map.lock();
        syntax_map.interpolate(&text);
        let syntax = syntax_map.snapshot();

        BufferSnapshot {{
            text,
            syntax,
            git_diff: self.git_diff.clone(),
            file: self.file.clone(),
            remote_selections: self.remote_selections.clone(),
            diagnostics: self.diagnostics.clone(),
            diagnostics_update_count: self.diagnostics_update_count,
            file_update_count: self.file_update_count,
            git_diff_update_count: self.git_diff_update_count,
            language: self.language.clone(),
            parse_count: self.parse_count,
            selections_update_count: self.selections_update_count,
        }}
    }}

    #[cfg(test)]
    pub(crate) fn as_text_snapshot(&self) -> &text::BufferSnapshot {{
        &self.text
    }}

    /// Retrieve a snapshot of the buffer's raw text, without any
    /// language-related state like the syntax tree or diagnostics.
    pub fn text_snapshot(&self) -> text::BufferSnapshot {{
        self.text.snapshot()
    }}

    /// The file associated with the buffer, if any.
    pub fn file(&self) -> Option<&Arc<dyn File>> {{
        self.file.as_ref()
    }}

    /// The version of the buffer that was last saved or reloaded from disk.
    pub fn saved_version(&self) -> &clock::Global {{
        &self.saved_version
    }}

    /// The mtime of the buffer's file when the buffer was last saved or reloaded from disk.
    pub fn saved_mtime(&self) -> Option<SystemTime> {{
        self.saved_mtime
    }}

    /// Assign a language to the buffer.
    pub fn set_language(&mut self, language: Option<Arc<Language>>, cx: &mut ModelContext<Self>) {{
        self.parse_count += 1;
        self.syntax_map.lock().clear();
        self.language = language;
        self.reparse(cx);
        cx.emit(Event::LanguageChanged);
    }}

    /// Assign a language registry to the buffer. This allows the buffer to retrieve
    /// other languages if parts of the buffer are written in different languages.
    pub fn set_language_registry(&mut self, language_registry: Arc<LanguageRegistry>) {{
        self.syntax_map
            .lock()
            .set_language_registry(language_registry);
    }}

    /// Assign the buffer a new [Capability].
    pub fn set_capability(&mut self, capability: Capability, cx: &mut ModelContext<Self>) {{
        self.capability = capability;
        cx.emit(Event::CapabilityChanged)
    }}

    /// Waits for the buffer to receive operations up to the given version.
    pub fn wait_for_version(&mut self, version: clock::Global) -> impl Future<Output = Result<()>> {{
        self.text.wait_for_version(version)
    }}

    /// Forces all futures returned by [`Buffer::wait_for_version`], [`Buffer::wait_for_edits`], or
    /// [`Buffer::wait_for_version`] to resolve with an error.
    pub fn give_up_waiting(&mut self) {{
        self.text.give_up_waiting();
    }}

    fn did_edit(
        &mut self,
        old_version: &clock::Global,
        was_dirty: bool,
        cx: &mut ModelContext<Self>,
    ) {{
        if self.edits_since::<usize>(old_version).next().is_none() {{
            return;
        }}

        self.reparse(cx);

        cx.emit(Event::Edited);
        if was_dirty != self.is_dirty() {{
            cx.emit(Event::DirtyChanged);
        }}
        cx.notify();
    }}

    /// Applies the given remote operations to the buffer.
    pub fn apply_ops<I: IntoIterator<Item = Operation>>(
        &mut self,
        ops: I,
        cx: &mut ModelContext<Self>,
    ) -> Result<()> {{
        self.pending_autoindent.take();
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        let mut deferred_ops = Vec::new();
        let buffer_ops = ops
            .into_iter()
            .filter_map(|op| match op {{
                Operation::Buffer(op) => Some(op),
                _ => {{
                    if self.can_apply_op(&op) {{
                        self.apply_op(op, cx);
                    }} else {{
                        deferred_ops.push(op);
                    }}
                    None
                }}
            }})
            .collect::<Vec<_>>();
        self.text.apply_ops(buffer_ops)?;
        self.deferred_ops.insert(deferred_ops);
        self.flush_deferred_ops(cx);
        self.did_edit(&old_version, was_dirty, cx);
        // Notify independently of whether the buffer was edited as the operations could include a
        // selection update.
        cx.notify();
        Ok(())
    }}

    fn flush_deferred_ops(&mut self, cx: &mut ModelContext<Self>) {{
        let mut deferred_ops = Vec::new();
        for op in self.deferred_ops.drain().iter().cloned() {{
            if self.can_apply_op(&op) {{
                self.apply_op(op, cx);
            }} else {{
                deferred_ops.push(op);
            }}
        }}
        self.deferred_ops.insert(deferred_ops);
    }}

    fn can_apply_op(&self, operation: &Operation) -> bool {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                diagnostics: diagnostic_set,
                ..
            }} => diagnostic_set.iter().all(|diagnostic| {{
                self.text.can_resolve(&diagnostic.range.start)
                    && self.text.can_resolve(&diagnostic.range.end)
            }}),
            Operation::UpdateSelections {{ selections, .. }} => selections
                .iter()
                .all(|s| self.can_resolve(&s.start) && self.can_resolve(&s.end)),
            Operation::UpdateCompletionTriggers {{ .. }} => true,
        }}
    }}

    fn apply_op(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        match operation {{
            Operation::Buffer(_) => {{
                unreachable!("buffer operations should never be applied at this layer")
            }}
            Operation::UpdateDiagnostics {{
                server_id,
                diagnostics: diagnostic_set,
                lamport_timestamp,
            }} => {{
                let snapshot = self.snapshot();
                self.apply_diagnostic_update(
                    server_id,
                    DiagnosticSet::from_sorted_entries(diagnostic_set.iter().cloned(), &snapshot),
                    lamport_timestamp,
                    cx,
                );
            }}
            Operation::UpdateSelections {{
                selections,
                lamport_timestamp,
                line_mode,
                cursor_shape,
            }} => {{
                if let Some(set) = self.remote_selections.get(&lamport_timestamp.replica_id) {{
                    if set.lamport_timestamp > lamport_timestamp {{
                        return;
                    }}
                }}

                self.remote_selections.insert(
                    lamport_timestamp.replica_id,
                    SelectionSet {{
                        selections,
                        lamport_timestamp,
                        line_mode,
                        cursor_shape,
                    }},
                );
                self.text.lamport_clock.observe(lamport_timestamp);
                self.selections_update_count += 1;
            }}
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp,
            }} => {{
                self.completion_triggers = triggers;
                self.text.lamport_clock.observe(lamport_timestamp);
            }}
        }}
    }}

    fn apply_diagnostic_update(
        &mut self,
        server_id: LanguageServerId,
        diagnostics: DiagnosticSet,
        lamport_timestamp: clock::Lamport,
        cx: &mut ModelContext<Self>,
    ) {{
        if lamport_timestamp > self.diagnostics_timestamp {{
            let ix = self.diagnostics.binary_search_by_key(&server_id, |e| e.0);
            if diagnostics.len() == 0 {{
                if let Ok(ix) = ix {{
                    self.diagnostics.remove(ix);
                }}
            }} else {{
                match ix {{
                    Err(ix) => self.diagnostics.insert(ix, (server_id, diagnostics)),
                    Ok(ix) => self.diagnostics[ix].1 = diagnostics,
                }};
            }}
            self.diagnostics_timestamp = lamport_timestamp;
            self.diagnostics_update_count += 1;
            self.text.lamport_clock.observe(lamport_timestamp);
            cx.notify();
            cx.emit(Event::DiagnosticsUpdated);
        }}
    }}

    fn send_operation(&mut self, operation: Operation, cx: &mut ModelContext<Self>) {{
        cx.emit(Event::Operation(operation));
    }}

    /// Removes the selections for a given peer.
    pub fn remove_peer(&mut self, replica_id: ReplicaId, cx: &mut ModelContext<Self>) {{
        self.remote_selections.remove(&replica_id);
        cx.notify();
    }}

    /// Undoes the most recent transaction.
    pub fn undo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.undo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Manually undoes a specific transaction in the buffer's undo history.
    pub fn undo_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();
        if let Some(operation) = self.text.undo_transaction(transaction_id) {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            true
        }} else {{
            false
        }}
    }}

    /// Manually undoes all changes after a given transaction in the buffer's undo history.
    pub fn undo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.undo_to_transaction(transaction_id);
        let undone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if undone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        undone
    }}

    /// Manually redoes a specific transaction in the buffer's redo history.
    pub fn redo(&mut self, cx: &mut ModelContext<Self>) -> Option<TransactionId> {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        if let Some((transaction_id, operation)) = self.text.redo() {{
            self.send_operation(Operation::Buffer(operation), cx);
            self.did_edit(&old_version, was_dirty, cx);
            Some(transaction_id)
        }} else {{
            None
        }}
    }}

    /// Returns the primary [Language] assigned to this [Buffer].
    pub fn language(&self) -> Option<&Arc<Language>> {{
        self.language.as_ref()
    }}

    /// Manually undoes all changes until a given transaction in the buffer's redo history.
    pub fn redo_to_transaction(
        &mut self,
        transaction_id: TransactionId,
        cx: &mut ModelContext<Self>,
    ) -> bool {{
        let was_dirty = self.is_dirty();
        let old_version = self.version.clone();

        let operations = self.text.redo_to_transaction(transaction_id);
        let redone = !operations.is_empty();
        for operation in operations {{
            self.send_operation(Operation::Buffer(operation), cx);
        }}
        if redone {{
            self.did_edit(&old_version, was_dirty, cx)
        }}
        redone
    }}

    /// Override current completion triggers with the user-provided completion triggers.
    pub fn set_completion_triggers(&mut self, triggers: Vec<String>, cx: &mut ModelContext<Self>) {{
        self.completion_triggers.clone_from(&triggers);
        self.completion_triggers_timestamp = self.text.lamport_clock.tick();
        self.send_operation(
            Operation::UpdateCompletionTriggers {{
                triggers,
                lamport_timestamp: self.completion_triggers_timestamp,
            }},
            cx,
        );
        cx.notify();
    }}

    /// Returns a list of strings which trigger a completion menu for this language.
    /// Usually this is driven by LSP server which returns a list of trigger characters for completions.
    pub fn completion_triggers(&self) -> &[String] {{
        &self.completion_triggers
    }}
}}
```
</content>
</extra_data>
<extra_data>


Your reply will start now and you should reply strictly in the following format:
<reply>
<steps_to_answer>
- For answering the user query we have to understand how we are getting the prettier config from the language
- `prettier_settings` is being used today to get the prettier parser
- the language configuration seems to be contained in `prettier is not allowed for language {{buffer_language:?}}"`
- we are getting the language configuration by calling `buffer_language` function
- we should check what `get_language` returns to understand what properies `buffer_language` has
</steps_to_answer>
<symbol_list>
<symbol>
<name>
Language
</name>
<line_content>
    pub fn language(&self) -> Option<&Arc<Language>> {{
</line_content>
<file_path>
crates/language/src/buffer.rs
</file_path>
<thinking>
Does the language type expose any prettier settings, because we want to get it so we can use that as the fallback
</thinking>
</symbol>
<symbol>
</symbol_list>
</reply>

We will also show you some more examples to help you understand how to do symbol selection just the name and the line content for the symbol_list below,

Example 1:
<code_in_selection>
```rust
let message =
        Message::new("message", "sender", "receiver);
message.send().await;
```rust
</code_in_selection>
and you want to select the `new` function in Message::new you would do the following:
<symbol>
<name>
new
</new>
<line_content>
        Message::new("message", "sender", "receiver);
</line_content>
</symbol>

Example 2:
<code_above>
```rust
use city::Movie;
```
</code_above>
<code_in_selection>
```rust
fn has_started(
    movie: Movie,
    time_now: usize,
) -> bool {{
    movie.start_time() <= time_now
}}
</code_in_selection>
and you want to select the `Movie` you will do the following:
<symbol>
<name>
Movie
</name>
<line_content>
    movie: Movie,
</line_content>
</symbol>
As you can see we ALWAYS select symbol from the code selection and never make mistakes when selecting the line, even if the import was a valid selection in this case but it was outside the <code_in_selection> block.

Notice how each xml tag ends with a new line and the content begins after the tag line, follow this format strictly.
In <line_content> only include a single line where the symbol you are interested in is contained.
You must only select symbols and lines to reply from the <code_in_selection> section and from no where else! This is very important."#
        )
    }

    fn system_message_for_class_symbol(&self) -> String {
        r#"You are an expert software engineer tasked with coming up with the next set of steps which need to be done because some class or enum definition has changed. More specifically some part of the class or enum defintiions have changed and we have to understand which identifier symbols to follow.
- You will be provided with the original code for the symbol we are looking at and the content of the symbol after we have made the edits. You have to only select the identifiers which we should follow (usually by doing go-to-references) since they have changed in a big way.
- The original code for the symbol will be provided in <original_code> section and the edited code in <edited_code> section.
- The instructions for why the change was made is also provided in <instructions> section
- You have to select the class memebers for which we need to check where it is being used and make necessary changes.

An example is given to you below:

<file_path>
testing/component.rs
</file_path>

<instructions>
We need to keep track of the symbol name along with the outline
</instructions>

<original_content>
```rust
struct SymbolTracker {
    symbol: String,
    range: Range,
}
```
</original_content>

<edited_code>
```rust
struct SymbolTracker {
    // we are going to track the name and the outline together here as (symbol_name, outline)
    symbol: (
        String,
        String,
    ),
    range: Range,
    is_edited: bool,
}
```
</edited_code>

Your reply should be stricly in the following format with it contained in the xml section called <members_to_follow>:
<members_to_follow>
<member>
<line>
    symbol: (
</line>
<name>
symbol
</name>
<thinking>
We need to check where symbol is being used because the types have changed and we are tracking the content as well
</thinking>
</member>
<member>
<line>
    is_edited: bool,
</line>
<name>
is_edited
</name>
<thinking>
is_edited has been also added possibly to keep track if the symbol was recently edited. It would be good to check if its being used anywhere and
keep track of that
</thinking>
</member>
</members_to_follow>

As you can observe, we do not just include the symbol but also the line containing the symbol and the partial line (see how for symbol the type is spread across lines but we are including just the first line), we also include the name of the symbol since that's really important for identifying it and the thinking process behind why the symbol should be followed."#.to_owned()
    }

    fn user_message_for_class_symbol(&self, request: ClassSymbolFollowupRequest) -> String {
        let file_path = request.fs_file_path();
        let original_code = request.original_code();
        let edited_code = request.edited_code();
        let instructions = request.instructions();
        let language = request.language();
        format!(
            r#"<file_path>
{file_path}
</file_path>

<instructions>
{instructions}
</instructions>

<original_code>
```{language}
{original_code}
```
</original_code>

<edited_code>
```{language}
{edited_code}
```
</edited_code>"#
        )
    }

    fn user_message_for_code_error_fix(
        &self,
        code_error_fix_request: &CodeEditingErrorRequest,
    ) -> String {
        let user_instruction = code_error_fix_request.instructions();
        let user_instruction = format!(
            r#"<user_instruction>
{user_instruction}
</user_instruction>"#
        );

        let file_path = code_error_fix_request.fs_file_path();
        let code_above = code_error_fix_request.code_above().unwrap_or("".to_owned());
        let code_below = code_error_fix_request.code_below().unwrap_or("".to_owned());
        let code_in_selection = code_error_fix_request.code_in_selection();
        let original_code = code_error_fix_request.original_code();
        let error_instructions = code_error_fix_request.error_instructions();

        let extra_data = code_error_fix_request.extra_context();

        let extra_context = format!(
            r#"<extra_context>
{extra_data}
</extra_context>"#
        );

        let file = format!(
            r#"<file>
<file_path>
{file_path}
</file_path>
<code_above>
{code_above}
</code_above>
<code_below>
{code_below}
</code_below>
<code_in_selection>
{code_in_selection}
</code_in_selection>
</file>"#
        );

        let original_code = format!(
            r#"<original_code>
{original_code}
</original_code>"#
        );

        let error_instructions = format!(
            r#"<error_instructions>
{error_instructions}
</error_instructions>"#
        );

        // The prompt is formatted over here
        format!(
            r#"<query>
{user_instruction}

{extra_context}

{file}

{original_code}

{error_instructions}
</query>"#
        )
    }

    fn system_message_for_code_error_fix(&self) -> String {
        format!(
            r#"You are an expert software engineer who is tasked with fixing broken written written by a junior engineer.
- All the definitions of code symbols which you might require are also provided to you in <extra_data> section, these are important as they show which functions or parameters you can use on different classes.
- The junior engineer has taken the instructions which were provided in <user_instructions> and made edits to the code which is now present in <code_in_selection> section.
- The original code before any changes were made is present in <original_code> , this should help you understand how the junior engineer went about making changes.
- You are also shown the whole file content in the <file> section, this will be useful for you to understand the overall context in which the change was made.
- The user has also noticed some errors with the modified code which is present in <code_in_selection> and given their reasoning in <error_instructions> section.
- You have to rewrite the code which is present only in <code_in_selection> making sure that the error instructions present in <error_instructions> are handled.

An example is shown below to you:

<user_instruction>
We want to be able to subtract 4 numbers instead of 2
</user_instruction>

<file>
<file_path>
testing/maths.py
</file_path>
<code_above>
```python
def add(a, b):
    return a + b
```
</code_above>
<code_below>
```python
def multiply(a, b):
    return a * b
```
</code_below>
<code_in_selection>
```python
def subtract(a, b, c):
    return a - b - c
</code_in_selection>
</file>

<original_code>
```python
def subtract(a, b):
    return a - b
```
</original_code>

<error_instructions>
You are subtracting 3 numbers not 4
</error_instructions>

Your reply is:
<reply>
```python
def subtract(a, b, c, d):
    return a - b - c - d
```
</reply>
"#
        )
    }
    fn system_message_for_correctness_check(&self) -> String {
        format!(
            r#"You are an expert software engineer who is tasked with taking actions for fixing errors in the code which is being written in the editor.
- You will be given a list of quick fixes suggested by your code editor.
- These are simple, deterministic actions that the editor can make on your behalf to fix simple errors.
- The code has been edited so that the user instruction present in <user_instruction> section is satisfied.
- This code is provided in <code_in_selection>
- The various errors which are present in the edited code are shown to you as <diagnostic_list>
- The actions you can take to fix the errors present in <diagnostic_list> is shown in <action_list>
- You have to only select a single action, even if multiple actions will be required for making the fix.
- You must have high confidence in your answer. Do not select changes that you cannot finish. Prefer simple and effective. Do not try to be too clever.
- You also have an option to solicit help if you are unsure:
- "ask for help" allows you to solicit the help of a more knowledgeable and intelligent colleague.
- You do not want to cause extra burden to others by attempting changes that will require a heavy refactor. Instead, ask for help.

An example is shown below to you:
<query>
<file>
<code_in_selection>
pub struct Tag {{
    pub rel_fname: PathBuf,
    pub fname: PathBuf,
    pub line: usize,
    pub name: String,
    pub kind: TagKind,
    pub user_id: Symbol,
}}
</code_in_selection>
</file>
<diagnostic_list>
<diagnostic>
<content>
pub user_id: Symbol,
</content>
<message>
Cannot find type Symbol in this scope
</message>
<diagnostic>
</diagnostic_list>
<action_list>
<action>
<index>
0
</index>
<intent>
Import 'webserver::agentic::symbol'
</intent>
</action>
<action>
<index>
1
</index>
<intent>
Ask for help
</intent>
</action>
</action_list>
<user_instruction>
add user_id with type Symbol
</user_instruction>
</query>

Your reply should be:
<code_action>
<thinking>
We should import the relevant type
</thinking>
<index>
0
</index>
</code_action>

You can notice how we chose to import the type as our action, and included a thinking field.
You have to do that always and only select a single action at a time."#
        )
    }

    fn format_lsp_diagnostic_for_prompt(&self, snippet: &str, diagnostic_message: &str) -> String {
        format!(
            r#"<diagnostic>
<content>
{}
</content>
<message>
{}
</message>
<diagnostic>"#,
            snippet, diagnostic_message
        )
        .to_owned()
    }

    fn format_code_correctness_request(
        &self,
        code_correctness_request: CodeCorrectnessRequest,
    ) -> String {
        // this should no longer be necessary.
        // we need a diagnostics snippet?
        let diagnostic_with_snippet = code_correctness_request.diagnostic_with_snippet();

        let formatted_diagnostic = self.format_lsp_diagnostic_for_prompt(
            diagnostic_with_snippet.snippet(),
            diagnostic_with_snippet.message(),
        );

        // now we show the quick actions which are avaiable as tools along with
        // the code edit which is always an option as well
        let mut quick_actions = code_correctness_request
            .quick_fix_actions()
            .into_iter()
            .map(|quick_action| {
                let index = quick_action.index();
                let label = quick_action.label();
                format!(
                    r#"<action>
<index>
{index}
</index>
<intent>
{label}
</intent>
</action>"#
                )
            })
            .collect::<Vec<_>>();

        // todo(zi: limit) - report to scratch pad?
        let action_index_for_help = quick_actions.len();
        quick_actions.push(format!(
            r#"<action>
<index>
{action_index_for_help}
</index>
<intent>
ask for help
</intent>
</action>"#
        ));

        let formatted_actions = quick_actions.join("\n");

        let code_in_selection = code_correctness_request.code_in_selection();

        let instruction = code_correctness_request.instruction();

        let file_content = format!(
            r#"<file>
<code_in_selection>
{code_in_selection}
</code_in_selection>
</file>"#
        );

        format!(
            r#"<query>
{file_content}
<diagnostic_list>
{formatted_diagnostic}
</diagnostic_list>
<action_list>
{formatted_actions}
</action_list>
<user_instruction>
{instruction}
</user_instruction>
</query>"#
        )
    }

    async fn user_message_for_utility_symbols(
        &self,
        user_request: CodeSymbolUtilityRequest,
    ) -> Result<String, CodeSymbolError> {
        // definitions which are already present
        let definitions = user_request.definitions().join("\n");
        let user_query = user_request.user_query().to_owned();
        // We need to grab the code context above, below and in the selection
        let file_path = user_request.fs_file_path().to_owned();
        let language = user_request.language().to_owned();
        let lines = user_request
            .file_content()
            .lines()
            .enumerate()
            .collect::<Vec<(usize, _)>>();
        let selection_range = user_request.selection_range();
        let line_above = (selection_range.start_line() as i64) - 1;
        let line_below = (selection_range.end_line() as i64) + 1;
        let code_above = lines
            .iter()
            .filter(|(line_number, _)| *line_number as i64 <= line_above)
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let code_below = lines
            .iter()
            .filter(|(line_number, _)| *line_number as i64 >= line_below)
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let code_selection = lines
            .iter()
            .filter(|(line_number, _)| {
                *line_number as i64 >= selection_range.start_line() as i64
                    && *line_number as i64 <= selection_range.end_line() as i64
            })
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let user_context = user_request.user_context();
        let context_string = user_context
            .to_xml(Default::default())
            .await
            .map_err(|e| CodeSymbolError::UserContextError(e))?;
        Ok(format!(
            r#"Here is all the required context:
<user_query>
{user_query}
</user_query>

<context>
{context_string}
</context>

Now the code which needs to be edited (we also show the code above, below and in the selection):
<file_path>
{file_path}
</file_path>
<code_above>
```{language}
{code_above}
```
</code_above>
<code_below>
```{language}
{code_below}
```
</code_below>
<code_in_selection>
```{language}
{code_selection}
```
</code_in_selection>

code symbols already selected:
<already_selected>
{definitions}
</alredy_selected>

As a reminder again here's the user query and the code we are focussing on. You have to grab more code symbols to make sure that the user query can be satisfied
<user_query>
{user_query}
</user_query>

<code_in_selection>
<file_path>
{file_path}
</file_path>
<content>
```{language}
{code_selection}
```
</content>
</code_in_selection>"#
        ))
    }

    fn system_message_for_utility_function(&self) -> String {
        format!(
            r#"You are a search engine which makes no mistakes while retriving important classes, functions or other values which would be important for the given user-query.
The user has already taken a pass and retrived some important code symbols to use. You have to make sure you select ANYTHING else which would be necessary for satisfying the user-query.
- The user has selected some context manually in the form of <selection> where we have to select the extra context.
- You will be given files which contains a lot of code, you have to select the "code symbols" which are important.
- "code symbols" here referes to the different classes, functions or constants which will be necessary to help with the user query.
- Now you will write a step by step process for gathering this extra context.
- In your step by step list make sure that the symbols are listed in the order in which they are relevant.
- Strictly follow the reply format which is mentioned to you below, your reply should always start with the <reply> tag and end with the </reply> tag

Let's focus on getting the "code symbols" which are absolutely necessary to satisfy the user query for the given <code_selection>

As a reminder, we only want to grab extra code symbols only for the code which we want to edit in <code_selection> section, nothing else

As an example, given the following code selection and the extra context already selected by the user.
<code_selection>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
```rust
pub struct FillInMiddleBroker {{
    providers: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self {{
        let broker = Self {{
            providers: HashMap::new(),
        }};
        broker
            .add_llm(
                LLMType::CodeLlama13BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::CodeLlama7BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder1_3BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder6BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder33BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeHaiku,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeOpus,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeSonnet,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
    }}
```
</code_selection>

The user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Already selected snippets:
<already_selected>
<code_symbol>
<file_path>
sidecar/llm_prompts/src/fim/types.rs
</file_path>
<name>
FillInMiddleFormatter
</name>
<content>
```rust
pub trait FillInMiddleFormatter {{
    fn fill_in_middle(
        &self,
        request: FillInMiddleRequest,
    ) -> Either<LLMClientCompletionRequest, LLMClientCompletionStringRequest>;
}}
```
</content>
</code_symbol>
<code_symbol>
<file_path>
sidecar/llm_prompts/src/fim/types.rs
</file_path>
<name>
FillInMiddleRequest
</name>
<content>
```rust
pub struct FillInMiddleRequest {{
    prefix: String,
    suffix: String,
    llm_type: LLMType,
    stop_words: Vec<String>,
    completion_tokens: Option<i64>,
    current_line_content: String,
    is_current_line_whitespace: bool,
    current_line_indentation: String,
}}
```
</content>
</code_symbol>
<code_symbol>
<file_path>
sidecar/llm_client/src/clients/types.rs
</file_path>
<name>
LLMClientCompletionRequest
</name>
<content>
```rust
#[derive(Clone, Debug)]
pub struct LLMClientCompletionRequest {{
    model: LLMType,
    messages: Vec<LLMClientMessage>,
    temperature: f32,
    frequency_penalty: Option<f32>,
    stop_words: Option<Vec<String>>,
    max_tokens: Option<usize>,
}}
```
</content>
</code_symbol>
</already_selected>

<selection>
<selection_item>
<file_path>
sidecar/llm_prompts/src/fim/deepseek.rs
</file_path>
<content>
```rust
pub struct DeepSeekFillInMiddleFormatter;

impl DeepSeekFillInMiddleFormatter {{
    pub fn new() -> Self {{
        Self
    }}
}}

impl FillInMiddleFormatter for DeepSeekFillInMiddleFormatter {{
    fn fill_in_middle(
        &self,
        request: FillInMiddleRequest,
    ) -> Either<LLMClientCompletionRequest, LLMClientCompletionStringRequest> {{
        // format is
        // <｜fim▁begin｜>{{prefix}}<｜fim▁hole｜>{{suffix}}<｜fim▁end｜>
        // https://ollama.ai/library/deepseek
        let prefix = request.prefix();
        let suffix = request.suffix();
        let response = format!("<｜fim▁begin｜>{{prefix}}<｜fim▁hole｜>{{suffix}}<｜fim▁end｜>");
        let string_request =
            LLMClientCompletionStringRequest::new(request.llm().clone(), response, 0.0, None)
                .set_stop_words(request.stop_words())
                .set_max_tokens(512);
        Either::Right(string_request)
    }}
}}
```
</content>
</selection_item>
<selection_item>
<file_path>
sidecar/llm_prompts/src/fim/grok.rs
</file_path>
<content>
```rust
fn grok_fill_in_middle_formatter(
    &self,
    request: FillInMiddleRequest,
) -> Either<LLMClientCompletionRequest, LLMClientCompletionStringRequest> {{
    todo!("this still needs to be implemented by following the website")
}}
```
</content>
</selection_item>
</selection>

Your reply should be:
<reply>
<symbol_list>
<symbol>
<name>
grok_fill_in_middle_formatter
</name>
<file_path>
sidecar/llm_prompts/src/fim/grok.rs
</file_path>
<thinking>
We require the grok_fill_in_middle_formatter since this function is the one which seems to be implementing the function to conver FillInMiddleRequest to the appropriate LLM request.
</thinking>
</symbol>
</symbol_list>
</reply>

Notice here that we made sure to include the `grok_fill_in_middle_formatter` and did not care about the DeepSeekFillInMiddleFormatter since its not necessary for the user query which asks us to implement the grok llm support
"#
        )
    }
    fn system_message_context_wide(&self) -> String {
        format!(
            r#"You are a search engine which makes no mistakes while retriving important context for a user-query.
You will be given context which the user has selected in <user_context> and you have to retrive the "code symbols" which are important for answering to the user query.
- The user might have selected some context manually in the form of <selection> these might be more important
- You will be given files which contains a lot of code, you have to select the "code symbols" which are important
- "code symbols" here referes to the different classes, functions, or constants which might be necessary to answer the user query.
- The user also shows you the recent changes made to the codebase in a git-diff style output in <recent_edits> use this to understand the continuity of the work being done.
- The user also shows you the various diagnostic errors which are present in the coding editor for the files they are interested in editing, this is present in <lsp_diagnostics> section.
- Now you will write a step by step process for making the code edit, this ensures that you lay down the plan before making the change, put this in an xml section called <step_by_step> where each step is in <step_list> section where each section has the name of the symbol on which the operation will happen, if no such symbol exists and you need to create a new one put a <new>true</new> inside the step section and after the symbols
- In your step by step list make sure that the symbols are listed in the order in which we have to go about making the changes
- If we are using absolute paths, make sure to use absolute paths in your reply.
- We will give you an outline of the symbols which are present in the file so you can use that as reference for selecting the right symbol, this outline is present to you in <outline> section
- Before giving us the list of symbols, you can think for a bit in the <thinking> section in no more than 2 lines.
- Strictly follow the reply format which is mentioned to you below, your reply should always start with <reply> tag and end with </reply> tag

Let's focus on getting the "code symbols" which are necessary to satisfy the user query.

As an example, given the following code selection:
<code_selection>
<file_path>
/broker/fill_in_middle.rs
</file_path>
```rust
pub struct FillInMiddleBroker {{
    providers: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self {{
        let broker = Self {{
            providers: HashMap::new(),
        }};
        broker
            .add_llm(
                LLMType::CodeLlama13BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::CodeLlama7BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder1_3BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder6BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder33BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeHaiku,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeOpus,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeSonnet,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
    }}
```
</code_selection>

<outline>
FILEPATH: /broker/fill_in_middle.rs
pub struct FillInMiddleBroker {{
    provider: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self
}}
</outline>

and the user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Your reply should be, you should strictly follow this format:
<reply>
<thinking>
Check if the LLMType supports grok and the various definitions which are present in `FillInMiddleBroker`
</thinking>
<step_by_step>
<step_list>
<name>
LLMType
</name>
<file_path>
/broker/fill_in_middle.rs
</file_path>
<step>
We will need to first check the LLMType if it has support for grok or we need to edit it first
</step>
</step_list>
<step_list>
<name>
GrokFillInMiddleFormatter
</name>
<file_path>
/broker/fill_in_middle.rs
</file_path>
<new>
true
</new>
<step>
Implement the GrokFillInMiddleFormatter following the similar pattern in `CodeLlamaFillInMiddleFormatter`
</step>
</step_list>
</step_by_step>
</reply>"#
        )
    }

    fn system_message(&self, code_symbol_important_request: &CodeSymbolImportantRequest) -> String {
        if code_symbol_important_request.symbol_identifier().is_some() {
            todo!("we need to figure it out")
        } else {
            format!(
                r#"You are responsible context to plan for a change requested in <user_query>. Your job is to select the most important symbols that you must explore in order to gather necessary context to execute the change. Do not suggest the change itself.

- You are working in an editor so you can go-to-definition on certain symbols, but you can only do that for code which is present in <code_selection> section.
- The user has selected some code which is present in <code_selection> section, before you start making changes you select the most important symbols which you need to either change or follow along for the context.
- You can get more context about the different symbols such as classes, functions, enums, types (and more) for only the code which is present ONLY in <code_selection> section, this ensures that you are able to gather everything necessary before making the code edit and the code you write will not use any wrong code out of this selection. Do not select code symbols outside of this section.
- The code which is already present on the file will be also visible to you when making changes, so do not worry about the symbols which you can already see.
- Make sure to select code symbols for which you will need to look deeper since you might end up using a function on some attribute from that symbol.
- Strictly follow the reply format which is mentioned to you below, your reply should always start with <reply> tag and end with </reply> tag

Let's focus on the step which is, gathering all the required symbol definitions and types.

As an example, given the following code selection:
<code_selection>
```rust
// FILEPATH: src/fill_in_middle_broker.rs
pub struct FillInMiddleBroker {{
    providers: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self {{
        let broker = Self {{
            providers: HashMap::new(),
        }};
        broker
            .add_llm(
                LLMType::CodeLlama13BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::CodeLlama7BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder1_3BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder6BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder33BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeHaiku,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeOpus,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeSonnet,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
    }}
```
</code_selection>

and the user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Your reply should be, you should strictly follow this format:
and the user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Your reply should be, you should strictly follow this format:
<reply>
<symbol_list>
<symbol>
<name>
LLMType
</name>
<file_path>
src/fill_in_middle_broker.rs
</file_path>
<thinking>
We need to first check if grok is part of the LLMType enum, this will make sure that the code we produce is never wrong
</thinking>
</symbol>
<symbol>
<name>
FillInMiddleFormatter
</name>
<file_path>
src/fill_in_middle_broker.rs
</file_path>
<thinking>
Other LLM's are implementing FillInMiddleFormatter trait, grok will also require support for this, so we need to check how to implement FillInMiddleFormatter trait
</thinking>
</symbol>
</symbol_list>
</reply>"#
            )
        }
    }

    fn user_message(&self, code_symbols: &CodeSymbolImportantRequest) -> String {
        let query = code_symbols.query();
        let file_path = code_symbols.file_path();
        let language = code_symbols.language();
        let lines = code_symbols
            .content()
            .lines()
            .enumerate()
            .collect::<Vec<(usize, _)>>();
        let selection_range = code_symbols.range();
        let line_above = (selection_range.start_line() as i64) - 1;
        let line_below = (selection_range.end_line() as i64) + 1;
        let code_above = lines
            .iter()
            .filter(|(line_number, _)| *line_number as i64 <= line_above)
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let code_below = lines
            .iter()
            .filter(|(line_number, _)| *line_number as i64 >= line_below)
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let code_selection = lines
            .iter()
            .filter(|(line_number, _)| {
                *line_number as i64 >= selection_range.start_line() as i64
                    && *line_number as i64 <= selection_range.end_line() as i64
            })
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        if code_symbols.symbol_identifier().is_none() {
            format!(
                r#"<file_path>
{file_path}
</file_path>
<code_above>
```{language}
{code_above}
```
</code_above>
<code_below>
```{language}
{code_below}
```
</code_below>
<code_selection>
```{language}
{code_selection}
```
</code_selection>
<user_query>
{query}
</user_query>"#
            )
        } else {
            format!("")
        }
    }

    fn unescape_xml(s: String) -> String {
        quick_xml::escape::escape(&s).to_string()
    }

    fn dirty_escape_fix(s: String) -> String {
        s.replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&gt;", ">")
            .replace("&lt;", "<")
            .replace("&amp;", "&")
    }

    fn escape_xml(s: String) -> String {
        quick_xml::escape::unescape(&s)
            .map(|output| output.to_string())
            .unwrap_or(Self::dirty_escape_fix(s))
            .to_string()
    }

    async fn user_message_for_codebase_wide_search(
        &self,
        code_symbol_search_context_wide: CodeSymbolImportantWideSearch,
    ) -> Result<String, CodeSymbolError> {
        let user_query = code_symbol_search_context_wide.user_query().to_owned();
        let file_extension_filter = code_symbol_search_context_wide.file_extension_filters();
        let recent_edits = code_symbol_search_context_wide.recent_edits().to_owned();
        let lsp_diagnostics = code_symbol_search_context_wide.lsp_diagnostics().to_owned();
        let user_context = code_symbol_search_context_wide.remove_user_context();
        let context_string = user_context
            .to_xml(file_extension_filter)
            .await
            .map_err(|e| CodeSymbolError::UserContextError(e))?;
        let mut user_message = format!(
            r#"{context_string}
<recent_edits>
{recent_edits}
</recent_edits>
<lsp_diagnostics>
{lsp_diagnostics}
</lsp_diagnostics>
<user_query>
{user_query}
</user_query>"#
        );

        // if this is a big message, the easiest proxy is to look at the number of lines
        // and make sure that we send a reminder to it
        if user_message.lines().collect::<Vec<_>>().len() > 2000 {
            user_message = user_message
                + "\n"
                + r#"As a reminder, your output should strictly follow this format:
<reply>
<thinking>
{your thoughts over here}
</thinking>
<step_by_step>
<step_list>
<name>
{symbol name over here}
</name>
<file_path>
{full_file_path for the symbol}
</file_path>
<step>
{the edit instruction which you want to give to this symbol}
</step>
</step_list>
{.. more <step_list> items}
</step_by_step>
</reply>"#;
        }
        Ok(user_message)
    }

    fn system_message_for_repo_map_search(
        &self,
        repo_map_search_request: &RepoMapSearchQuery,
    ) -> String {
        let root_directory = repo_map_search_request
            .root_directory()
            .clone()
            .map(|root_directory| format!("{}/", root_directory))
            .unwrap_or("".to_owned());
        format!(r#"You are a search engine which makes no mistakes while retriving important context for a user-query.
You will be given context which the user has selected in <user_context> and you have to retrive the "code symbols" which are important for answering to the user query.
- The user might have selected some context manually in the form of <selection> these might be more important
- You will be given files which contains a lot of code, you have to select the "code symbols" which are important
- "code symbols" here referes to the different classes, functions, enums, methods or constants which might be necessary to answer the user query.
- Now you will write a step by step process for making the code edit, this ensures that you lay down the plan before making the change, put this in an xml section called <step_by_step> where each step is in <step_list> section where each section has the name of the symbol on which the operation will happen, if no such symbol exists and you need to create a new one put a <new>true</new> inside the step section and after the symbols
- In your step by step list make sure that the symbols are listed in the order in which we have to go about making the changes
- Strictly follow the reply format which is mentioned to you below, your reply should always start with <reply> tag and end with </reply> tag

Let's focus on getting the "code symbols" which are necessary to satisfy the user query.

As an example, given the following code selection:
<code_selection>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
```rust
pub struct FillInMiddleBroker {{
    providers: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self {{
        let broker = Self {{
            providers: HashMap::new(),
        }};
        broker
            .add_llm(
                LLMType::CodeLlama13BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::CodeLlama7BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder1_3BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder6BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder33BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeHaiku,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeOpus,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeSonnet,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
    }}
```
</code_selection>

and the user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Your reply should be, you should strictly follow this format:
<reply>
<symbol_list>
<symbol>
<name>
LLMType
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
We need to first check if grok is part of the LLMType enum, this will make sure that the code we produce is never wrong
</thinking>
</symbol>
<symbol>
<name>
FillInMiddleFormatter
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
Other LLM's are implementing FillInMiddleFormatter trait, grok will also require support for this, so we need to check how to implement FillInMiddleFormatter trait
</thinking>
</symbol>
<symbol>
<name>
new
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
We have to change the new function and add the grok llm after implementing the formatter for grok llm.
</thinking>
</symbol>
</symbol_list>
<step_by_step>
<step_list>
<name>
LLMType
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<step>
We will need to first check the LLMType if it has support for grok or we need to edit it first
</step>
</step_list>
<step_list>
<name>
FillInMiddleFormatter
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<step>
Check the definition of `FillInMiddleFormatter` to see how to implement it
</step>
</step_list>
<step_list
<name>
CodeLlamaFillInMiddleFormatter
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<step>
We can follow the implementation of CodeLlamaFillInMiddleFormatter since we will also have to follow a similar pattern of making changes and adding it to the right places if there are more.
</step>
</step_list>
<step_list>
<name>
GrokFillInMiddleFormatter
</name>
<file_path>
{root_directory}sidecar/broker/fill_in_middle.rs
</file_path>
<new>
true
</new>
<step>
Implement the GrokFillInMiddleFormatter following the similar pattern in `CodeLlamaFillInMiddleFormatter`
</step>
</step_list>
</step_by_step>
</reply>

Another example:
<file_path>
{root_directory}sidecar/bin/webserver.rs
</file_path>
<code_selection>
```rust
fn tree_sitter_router() -> Router {{
    use axum::routing::*;
    Router::new()
        .route(
            "/documentation_parsing",
            post(sidecar::webserver::tree_sitter::extract_documentation_strings),
        )
        .route(
            "/diagnostic_parsing",
            post(sidecar::webserver::tree_sitter::extract_diagnostics_range),
        )
        .route(
            "/tree_sitter_valid",
            post(sidecar::webserver::tree_sitter::tree_sitter_node_check),
        )
}}

fn file_operations_router() -> Router {{
    use axum::routing::*;
    Router::new().route("/edit_file", post(sidecar::webserver::file_edit::file_edit))
}}

fn inline_completion() -> Router {{
    use axum::routing::*;
    Router::new()
        .route(
            "/inline_completion",
            post(sidecar::webserver::inline_completion::inline_completion),
        )
        .route(
            "/cancel_inline_completion",
            post(sidecar::webserver::inline_completion::cancel_inline_completion),
        )
        .route(
            "/document_open",
            post(sidecar::webserver::inline_completion::inline_document_open),
        )
        .route(
            "/document_content_changed",
            post(sidecar::webserver::inline_completion::inline_completion_file_content_change),
        )
        .route(
            "/get_document_content",
            post(sidecar::webserver::inline_completion::inline_completion_file_content),
        )
        .route(
            "/get_identifier_nodes",
            post(sidecar::webserver::inline_completion::get_identifier_nodes),
        )
        .route(
            "/get_symbol_history",
            post(sidecar::webserver::inline_completion::symbol_history),
        )
}}

// TODO(skcd): Figure out why we are passing the context in the suffix and not the prefix

```
</code_selection>

and the user query is:
<user_query>
I want to get the list of most important symbols in inline completions
</user_query>

Your reply should be:
<reply>
<symbol_list>
<symbol>
<name>
inline_completion
</name>
<file_path>
{root_directory}sidecar/bin/webserver.rs
</file_path>
<thinking>
inline_completion holds all the endpoints for symbols because it also has the `get_symbol_history` endpoint. We have to start adding the endpoint there
</thinking>
</symbol>
<symbol>
<name>
symbol_history
</name>
<file_path>
{root_directory}sidecar/bin/webserver.rs
</file_path>
<thinking>
I can find more information on how to write the code for the endpoint by following the symbol `symbol_history` in the line: `             post(sidecar::webserver::inline_completion::symbol_history),`
<thinking>
</symbol>
</symbol_list>
<step_by_step>
<step_list>
<name>
symbol_history
</name>
<file_path>
{root_directory}sidecar/bin/webserver.rs
</file_path>
<thinking>
We need to follow the symbol_history to check the pattern on how we are going to implement the very similar functionality
</thinking>
</step_list>
<step_list>
<name>
inline_completion
</name>
<file_path>
{root_directory}sidecar/bin/webserver.rs
</file_path>
<thinking>
We have to add the newly created endpoint in inline_completion to add support for the new endpoint which we want to create
</thinking>
</step_list>
</step_by_step>
</reply>"#).to_owned()
    }

    fn user_message_for_repo_map_search(
        &self,
        repo_map_search_request: RepoMapSearchQuery,
    ) -> String {
        let repo_map = repo_map_search_request.repo_map();
        let user_query = repo_map_search_request.user_query();
        format!(
            r#"<code_selection>
{repo_map}
</code_selection>
<user_query>
{user_query}
</user_query>

- Remember your reply should always be contained in <reply> tags and follow the format which we have shown you before in the system message.
- Do not forget to include the <file_path> in your reply
- Return no more than 20 symbols"#
        )
    }
}

#[async_trait]
impl CodeSymbolImportant for AnthropicCodeSymbolImportant {
    async fn get_important_symbols(
        &self,
        code_symbols: CodeSymbolImportantRequest,
    ) -> Result<CodeSymbolImportantResponse, CodeSymbolError> {
        let system_message = LLMClientMessage::system(self.system_message(&code_symbols));
        let user_message = LLMClientMessage::user(self.user_message(&code_symbols));
        let messages = LLMClientCompletionRequest::new(
            code_symbols.model().clone(),
            vec![system_message, user_message],
            0.0,
            None,
        );

        // we should add retries over here
        let mut retries = 0;
        loop {
            if retries >= 4 {
                return Err(CodeSymbolError::ExhaustedRetries);
            }
            let (llm, api_key, provider) = if retries % 2 == 1 {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            } else {
                (
                    code_symbols.model().clone(),
                    code_symbols.api_key().clone(),
                    code_symbols.provider().clone(),
                )
            };
            let cloned_message = messages.clone().set_llm(llm);
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key,
                    cloned_message,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "grab_definitions_for_symbol_editing".to_owned(),
                        ),
                        (
                            "root_id".to_owned(),
                            code_symbols.root_request_id().to_owned(),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| CodeSymbolError::LLMClientError(e));
            match response {
                Ok(response) => {
                    if let Ok(parsed_response) =
                        Reply::parse_response(response.answer_up_until_now())
                            .map(|reply| reply.to_code_symbol_important_response())
                    {
                        return Ok(parsed_response);
                    } else {
                        retries = retries + 1;
                    }
                }
                _ => {
                    retries = retries + 1;
                }
            }
        }
    }

    async fn context_wide_search(
        &self,
        code_symbols: CodeSymbolImportantWideSearch,
    ) -> Result<CodeSymbolImportantResponse, CodeSymbolError> {
        let api_key = code_symbols.api_key();
        let provider = code_symbols.llm_provider();
        let model = code_symbols.model().clone();
        let root_request_id = code_symbols.root_request_id().to_owned();
        let exchange_id = code_symbols.exchange_id();
        let ui_sender = code_symbols.message_properties().ui_sender();
        let cancellation_token = code_symbols.message_properties().cancellation_token();
        let system_message = LLMClientMessage::system(self.system_message_context_wide());
        let user_message = LLMClientMessage::user(
            self.user_message_for_codebase_wide_search(code_symbols)
                .await?,
        );
        let messages = LLMClientCompletionRequest::new(
            model.clone(),
            vec![system_message, user_message],
            0.0,
            None,
        );

        let mut retries = 0;
        loop {
            if cancellation_token.is_cancelled() {
                return Err(CodeSymbolError::Cancelled);
            }
            if retries >= 4 {
                return Err(CodeSymbolError::ExhaustedRetries);
            }
            let (llm, api_key, provider) = if retries % 2 == 1 {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            } else {
                (model.clone(), api_key.clone(), provider.clone())
            };
            let cloned_message = messages.clone().set_llm(llm);
            let cloned_llm_client = self.llm_client.clone();
            let cloned_root_request_id = root_request_id.clone();
            let cloned_root_request_id_2 = root_request_id.clone();
            let cloned_exchange_id = exchange_id.clone();
            let cloned_ui_sender = ui_sender.clone();
            let cloned_cancellation_token = cancellation_token.clone();
            let cloned_cancellation_token_2 = cancellation_token.clone();

            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

            let stream_handle = tokio::spawn(run_with_cancellation(
                cloned_cancellation_token_2,
                async move {
                    cloned_llm_client
                        .stream_completion(
                            api_key.clone(),
                            cloned_message.clone(),
                            provider.clone(),
                            vec![
                                ("event_type".to_owned(), "context_wide_search".to_owned()), // but stream only for context_wide_search
                                ("root_id".to_owned(), cloned_root_request_id.clone()),
                            ]
                            .into_iter()
                            .collect(),
                            sender,
                        )
                        .await
                },
            ));

            let stream_thinking_and_step_list_handle = tokio::spawn(run_with_cancellation(
                cloned_cancellation_token,
                async move {
                    let mut delta_stream =
                        tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
                    let mut xml_processor = XmlProcessor::new();
                    let mut thinking_extracted = false;

                    while let Some(stream_msg) = delta_stream.next().await {
                        if let Some(delta) = stream_msg.delta() {
                            xml_processor.append(&delta);

                            if !thinking_extracted {
                                if let Some(content) = xml_processor.extract_tag_content("thinking")
                                {
                                    println!(
                                        r#"context_wide_search::stream_thinking_and_step_list_handle::extract_tag_content("thinking"): {}"#,
                                        content
                                    );
                                    thinking_extracted = true;

                                    let ui_event = UIEventWithID::agentic_top_level_thinking(
                                        cloned_root_request_id_2.to_owned(),
                                        cloned_exchange_id.to_owned(),
                                        &content,
                                    );
                                    let _ = cloned_ui_sender.send(ui_event);
                                }
                            }

                            // implicitly we track last processed position, so this will not duplicate.
                            // it handles the case where a single chunk contains multiple step_list items.
                            let step_lists = xml_processor.extract_all_tag_contents("step_list");
                            for step_list in step_lists {
                                let wrapped_step = XmlProcessor::wrap_xml("step_list", &step_list);
                                match StepListItem::parse_from_str(&wrapped_step) {
                                    Some(step_list_item) => {
                                        let ui_event = UIEventWithID::agentic_symbol_level_thinking(
                                            cloned_root_request_id_2.to_owned(),
                                            cloned_exchange_id.to_owned(),
                                            step_list_item,
                                        );
                                        let _ = cloned_ui_sender.send(ui_event);
                                    }
                                    None => {
                                        eprintln!("context_wide_search::stream_thinking_and_step_list_handle::from_str::error");
                                    }
                                }
                            }
                        }
                    }
                },
            ));

            // ensure streaming completes
            let _ = stream_thinking_and_step_list_handle.await;

            match stream_handle.await {
                // Happy path
                Ok(Some(Ok(result))) => {
                    let s = Reply::parse_response(result.answer_up_until_now())
                        .map(|reply| reply.to_code_symbol_important_response());
                    match s {
                        Ok(parsed_response) => return Ok(parsed_response),
                        Err(_) => retries += 1,
                    }
                }
                _ => {
                    eprintln!("context_wide_search::stream_handle::error::retrying");
                    retries += 1;
                }
            }
        }
    }

    async fn gather_utility_symbols(
        &self,
        utility_symbol_request: CodeSymbolUtilityRequest,
    ) -> Result<CodeSymbolImportantResponse, CodeSymbolError> {
        let api_key = utility_symbol_request.api_key();
        let provider = utility_symbol_request.provider();
        let model = utility_symbol_request.model();
        let root_request_id = utility_symbol_request.root_request_id().to_owned();
        let system_message = LLMClientMessage::system(self.system_message_for_utility_function());
        let user_message = LLMClientMessage::user(
            self.user_message_for_utility_symbols(utility_symbol_request)
                .await?,
        );
        let messages =
            LLMClientCompletionRequest::new(model, vec![system_message, user_message], 0.0, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_key,
                messages,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "utility_function_search".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        Reply::parse_response(response.answer_up_until_now())
            .map(|reply| reply.to_code_symbol_important_response())
    }

    // This replies back with more data about what questions to ask further
    // we can just initiate a stop operation and call it a day
    async fn symbols_to_probe_questions(
        &self,
        request: CodeSymbolToAskQuestionsRequest,
    ) -> Result<CodeSymbolToAskQuestionsResponse, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.model().clone();
        let request_api_key = request.api_key().clone();
        let request_provider = request.provider().clone();
        let system_message =
            LLMClientMessage::system(self.system_message_for_ask_question_symbols(
                request.symbol_identifier(),
                request.fs_file_path(),
            ));
        let user_message =
            LLMClientMessage::user(self.user_message_for_ask_question_symbols(request));
        let messages = LLMClientCompletionRequest::new(
            model.clone(),
            vec![system_message, user_message],
            0.0,
            None,
        );
        let mut retries = 0;
        let root_request_id_ref = &root_request_id;
        loop {
            if retries > 4 {
                return Err(CodeSymbolError::ExhaustedRetries);
            }
            let mut cloned_messages = messages.clone();
            // if its odd use fail over llm otherwise stick to the provided on
            let (api_key, provider) = if retries % 2 == 1 {
                cloned_messages = cloned_messages.set_llm(self.fail_over_llm.llm().clone());
                (self.fail_over_llm.api_key(), self.fail_over_llm.provider())
            } else {
                cloned_messages = cloned_messages.set_llm(model.clone());
                (&request_api_key, &request_provider)
            };
            retries = retries + 1;
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key.clone(),
                    cloned_messages,
                    provider.clone(),
                    vec![
                        (
                            "event_type".to_owned(),
                            "symbols_to_probe_questions".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id_ref.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await?;
            // now we want to parse the reply here properly
            let parsed_response = CodeSymbolToAskQuestionsResponse::parse_response(
                response.answer_up_until_now().to_owned(),
            );
            match parsed_response {
                Ok(_) => return parsed_response,
                Err(_) => {
                    continue;
                }
            }
        }
    }

    async fn should_probe_question_request(
        &self,
        request: CodeSymbolToAskQuestionsRequest,
    ) -> Result<CodeSymbolShouldAskQuestionsResponse, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.model().clone();
        let api_key = request.api_key().clone();
        let provider = request.provider().clone();
        let system_message =
            LLMClientMessage::system(self.system_message_for_should_ask_questions());
        let user_message =
            LLMClientMessage::user(self.user_message_for_should_ask_questions(request));
        let system_message_content = system_message.content();
        let user_message_content = user_message.content();
        info!(
            event_name = "should_probe_question_request[request]",
            system_message = system_message_content,
            user_message = user_message_content
        );
        let messages =
            LLMClientCompletionRequest::new(model, vec![system_message, user_message], 0.0, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_key,
                messages,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "should_probe_question_request".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        // parse out the response over here
        info!(
            event_name = "should_probe_question_request[response]",
            response = response.answer_up_until_now(),
        );
        println!("Should probe question request: {:?}", &response);
        CodeSymbolShouldAskQuestionsResponse::parse_response(
            response.answer_up_until_now().to_owned(),
        )
    }

    async fn should_probe_follow_along_symbol_request(
        &self,
        request: CodeSymbolFollowAlongForProbing,
    ) -> Result<ProbeNextSymbol, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let llm = request.llm().clone();
        let provider = request.llm_provider().clone();
        let api_keys = request.api_keys().clone();
        let system_message = LLMClientMessage::system(self.system_message_for_probe_next_symbol());
        let user_messagee =
            LLMClientMessage::user(self.user_message_for_probe_next_symbol(request));
        let messages =
            LLMClientCompletionRequest::new(llm, vec![system_message, user_messagee], 0.0, None);
        let mut retries = 0;
        let root_request_id_ref = &root_request_id;
        loop {
            if retries > 3 {
                return Err(CodeSymbolError::ExhaustedRetries);
            }
            if retries != 0 {
                jitter_sleep(10.0, retries as f64).await;
            }
            retries = retries + 1;

            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_keys.clone(),
                    messages.clone(),
                    provider.clone(),
                    vec![
                        ("event_type".to_owned(), "probe_next_symbol".to_owned()),
                        ("root_id".to_owned(), root_request_id_ref.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await?;
            // Now we want to parse this response properly
            let parsed_answer = ProbeNextSymbol::parse_response(response.answer_up_until_now());
            match parsed_answer {
                Ok(ProbeNextSymbol::Empty) | Err(_) => {
                    continue;
                }
                _ => return parsed_answer,
            }
        }
    }

    async fn probe_summarize_answer(
        &self,
        request: CodeSymbolProbingSummarize,
    ) -> Result<String, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let llm = request.llm().clone();
        let provider = request.provider().clone();
        let api_keys = request.api_keys().clone();
        let system_message =
            LLMClientMessage::system(self.system_message_for_summarizing_probe_result());
        let user_message =
            LLMClientMessage::user(self.user_message_for_summarizing_probe_result(request));
        let messages = LLMClientCompletionRequest::new(
            llm.clone(),
            vec![system_message, user_message],
            0.0,
            None,
        );
        let mut retries = 0;
        let root_request_id_ref = &root_request_id;
        loop {
            if retries > 4 {
                return Err(CodeSymbolError::ExhaustedRetries);
            }

            let (llm, api_key, provider) = if retries % 2 == 0 {
                (llm.clone(), api_keys.clone(), provider.clone())
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };

            let cloned_message = messages.clone().set_llm(llm);

            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key,
                    cloned_message,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "probe_summarize_results".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id_ref.to_string()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;

            match response {
                Ok(response) => {
                    if response.answer_up_until_now().is_empty() {
                        retries = retries + 1;
                        continue;
                    } else {
                        // The response we get back here is just going to be inside <reply>{reply}</reply> tags, so we can parse it very easily
                        let response_lines = response.answer_up_until_now().lines();
                        let mut final_answer = vec![];
                        let mut is_inside = false;
                        for line in response_lines {
                            if line.starts_with("<reply>") {
                                is_inside = true;
                                continue;
                            } else if line.starts_with("</reply>") {
                                is_inside = false;
                                continue;
                            }
                            if is_inside {
                                final_answer.push(line.to_owned());
                            }
                        }
                        return Ok(final_answer.join("\n"));
                    }
                }
                Err(e) => {
                    println!("tool::probe_summarize_answer::error({:?})", e);
                    retries = retries + 1;
                }
            }
        }
    }
}

#[async_trait]
impl CodeCorrectness for AnthropicCodeSymbolImportant {
    async fn decide_tool_use(
        &self,
        code_correctness_request: CodeCorrectnessRequest,
    ) -> Result<CodeCorrectnessAction, CodeSymbolError> {
        let root_request_id = code_correctness_request.root_request_id().to_owned();
        let request_llm = code_correctness_request.llm().clone();
        let request_provider = code_correctness_request.llm_provider().clone();
        let request_api_keys = code_correctness_request.llm_api_keys().clone();
        let system_message = LLMClientMessage::system(self.system_message_for_correctness_check());
        let user_message =
            LLMClientMessage::user(self.format_code_correctness_request(code_correctness_request));
        let messages = LLMClientCompletionRequest::new(
            request_llm.clone(),
            vec![system_message, user_message],
            0.0,
            None,
        );
        let (llm, api_keys, provider) = (
            request_llm.clone(),
            request_api_keys.clone(),
            request_provider.clone(),
        );
        let cloned_request = messages.clone().set_llm(llm);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                cloned_request,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "code_correctness_tool_use".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        // now that we have the response we have to make sure to parse the thinking
        // process properly or else it will blow up in our faces pretty quickly
        let mut inside_thinking = false;
        let fixed_response = response
            .answer_up_until_now()
            .lines()
            .into_iter()
            .map(|response| {
                if response.starts_with("<thinking>") {
                    inside_thinking = true;
                    return response.to_owned();
                } else if response.starts_with("</thinking>") {
                    inside_thinking = false;
                    return response.to_owned();
                }
                if inside_thinking {
                    // espcae the string here
                    Self::unescape_xml(response.to_owned())
                } else {
                    response.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let parsed_response = from_str::<CodeCorrectnessAction>(&fixed_response).map_err(|e| {
            CodeSymbolError::SerdeError(SerdeError::new(e, fixed_response.to_owned()))
        });

        parsed_response
    }
}

#[async_trait]
impl CodeSymbolErrorFix for AnthropicCodeSymbolImportant {
    async fn fix_code_symbol(
        &self,
        code_fix: CodeEditingErrorRequest,
    ) -> Result<String, CodeSymbolError> {
        let root_request_id = code_fix.root_request_id().to_owned();
        let model = code_fix.llm().clone();
        let provider = code_fix.llm_provider().clone();
        let api_keys = code_fix.llm_api_keys().clone();
        let system_message = LLMClientMessage::system(self.system_message_for_code_error_fix());
        let user_message = LLMClientMessage::user(self.user_message_for_code_error_fix(&code_fix));
        let messages =
            LLMClientCompletionRequest::new(model, vec![system_message, user_message], 0.2, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                messages,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "fix_code_symbol_code_editing".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        // We want to parse the response here since it should be within the
        // <reply> tags and then have the ``` backticks as well
        Self::parse_code_edit_reply(response.answer_up_until_now())
    }
}

#[async_trait]
impl ClassSymbolFollowup for AnthropicCodeSymbolImportant {
    async fn get_class_symbol(
        &self,
        request: ClassSymbolFollowupRequest,
    ) -> Result<ClassSymbolFollowupResponse, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.llm().clone();
        let provider = request.provider().clone();
        let api_keys = request.api_keys().clone();
        let system_message = LLMClientMessage::system(self.system_message_for_class_symbol());
        let user_message = LLMClientMessage::user(self.user_message_for_class_symbol(request));
        let messages =
            LLMClientCompletionRequest::new(model, vec![system_message, user_message], 0.2, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                messages,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "class_symbols_to_follow".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        self.fix_class_symbol_response(response.answer_up_until_now().to_owned())
    }
}

#[async_trait]
impl RepoMapSearch for AnthropicCodeSymbolImportant {
    async fn get_repo_symbols(
        &self,
        request: RepoMapSearchQuery,
    ) -> Result<CodeSymbolImportantResponse, CodeSymbolError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.llm().clone();
        let provider = request.provider().clone();
        let api_keys = request.api_keys().clone();
        let system_message =
            LLMClientMessage::system(self.system_message_for_repo_map_search(&request));
        let user_message = LLMClientMessage::user(self.user_message_for_repo_map_search(request));
        let messages = LLMClientCompletionRequest::new(
            model,
            vec![system_message.clone(), user_message.clone()],
            0.2,
            None,
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();

        let start = Instant::now();

        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                messages,
                provider,
                vec![
                    ("event_type".to_owned(), "repo_map_search".to_owned()),
                    ("root_id".to_owned(), root_request_id.clone()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;
        let parsed_response = Reply::parse_response(response.answer_up_until_now())
            .map(|reply| reply.to_code_symbol_important_response());

        let duration = start.elapsed();
        println!("get_repo_symbols::LLM_response_time: {:?}", duration);

        // writes a trace-safe version of the response
        if let Ok(ref parsed_response) = parsed_response {
            let ordered_symbols = parsed_response.ordered_symbols();

            // disabled writing the trace for now
            let _ordered_symbols_string = ordered_symbols
                .iter()
                .map(|symbol| {
                    // file_path, steps: Vec<String>, code_symbol
                    format!("{}: {:?}", symbol.file_path(), symbol.steps())
                })
                .collect::<Vec<_>>()
                .join("\n");

            // self.write_trace(
            //     &root_request_id,
            //     &system_message.content(),
            //     &user_message.content(),
            //     &ordered_symbols_string,
            // );
        }

        parsed_response
    }
}

// for swe-bench traces

// impl AnthropicCodeSymbolImportant {
//     fn write_trace(
//         &self,
//         root_request_id: &str,
//         system_message: &str,
//         user_message: &str,
//         response: &str,
//     ) -> std::io::Result<()> {
//         let traces_dir_path = PathBuf::from("/Users/zi/codestory/sidecar/traces");

//         let extension = "md";
//         let safe_id = root_request_id.replace(|c: char| !c.is_alphanumeric(), "_");

//         let file_name = format!("{}.{}", safe_id, extension);

//         let file_path = traces_dir_path.join(file_name);

//         let mut file = File::create(&file_path)
//             .unwrap_or_else(|_| panic!("Failed to create trace file: {:?}", file_path));

//         writeln!(file, "# [System]")?;
//         writeln!(file, "{}", system_message)?;
//         writeln!(file)?;

//         writeln!(file, "# [User]")?;
//         writeln!(file, "{}", user_message,)?;
//         writeln!(file)?;

//         writeln!(file, "# [Response]")?;
//         writeln!(file, "{}", response)?;

//         Ok(())
//     }
// }

#[cfg(test)]
mod tests {

    use crate::agentic::tool::code_symbol::models::anthropic::{Reply, StepListItem};

    use super::{CodeSymbolShouldAskQuestionsResponse, CodeSymbolToAskQuestionsResponse};

    #[test]
    fn test_parsing_works_for_important_symbol() {
        let reply = r#"<reply>
<symbol_list>
<symbol>
<name>
LLMProvider
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/provider.rs
</file_path>
<thinking>
We need to first add a new variant to the LLMProvider enum to represent the GROQ provider.
</thinking>
</symbol>
<symbol>
<name>
LLMProviderAPIKeys
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/provider.rs
</file_path>
<thinking>
We also need to add a new variant to the LLMProviderAPIKeys enum to hold the API key for the GROQ provider.
</thinking>
</symbol>
<symbol>
<name>
LLMBroker
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/broker.rs
</file_path>
<thinking>
We need to update the LLMBroker to add support for the new GROQ provider. This includes adding a new case in the get_provider function and adding a new provider to the providers HashMap.
</thinking>
</symbol>
<symbol>
<new>
true
</new>
<name>
GroqClient
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/clients/groq.rs
</file_path>
<thinking>
We need to create a new GroqClient struct that implements the LLMClient trait. This client will handle communication with the GROQ provider.
</thinking>
</symbol>
</symbol_list>
<step_by_step>
<step_list>
<name>
LLMProvider
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/provider.rs
</file_path>
<step>
Add a new variant to the LLMProvider enum to represent the GROQ provider:

```rust
pub enum LLMProvider {
    // ...
    Groq,
    // ...
}
```
</step>
</step_list>
<step_list>
<name>
LLMProviderAPIKeys
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/provider.rs
</file_path>
<step>
Add a new variant to the LLMProviderAPIKeys enum to hold the API key for the GROQ provider:

```rust
pub enum LLMProviderAPIKeys {
    // ...
    Groq(GroqAPIKey),
    // ...
}
```

Create a new struct to hold the API key for the GROQ provider:

```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GroqAPIKey {
    pub api_key: String,
    // Add any other necessary fields
}
```
</step>
</step_list>
<step_list>
<name>
LLMBroker
</name>
<file_path>
/Users/skcd/scratch/sidecar/llm_client/src/broker.rs
</file_path>
<step>
Update the get_provider function in the LLMBroker to handle the new GROQ provider:

```rust
fn get_provider(&self, api_key: &LLMProviderAPIKeys) -> LLMProvider {
    match api_key {
        // ...
        LLMProviderAPIKeys::Groq(_) => LLMProvider::Groq,
        // ...
    }
}
```

Add a new case in the stream_completion and stream_string_completion functions to handle the GROQ provider:

```rust
pub async fn stream_completion(
    &self,
    api_key: LLMProviderAPIKeys,
    request: LLMClientCompletionRequest,
    provider: LLMProvider,
    metadata: HashMap<String, String>,
    sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
) -> LLMBrokerResponse {
    // ...
    let provider_type = match &api_key {
        // ...
        LLMProviderAPIKeys::Groq(_) => LLMProvider::Groq,
        // ...
    };
    // ...
}

pub async fn stream_string_completion(
    &self,
    api_key: LLMProviderAPIKeys,
    request: LLMClientCompletionStringRequest,
    metadata: HashMap<String, String>,
    sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
) -> LLMBrokerResponse {
    // ...
    let provider_type = match &api_key {
        // ...
        LLMProviderAPIKeys::Groq(_) => LLMProvider::Groq,
        // ...
    };
    // ...
}
```

In the LLMBroker::new function, add the new GROQ provider to the providers HashMap:

```rust
pub async fn new(config: LLMBrokerConfiguration) -> Result<Self, LLMClientError> {
    // ...
    Ok(broker
        // ...
        .add_provider(LLMProvider::Groq, Box::new(GroqClient::new())))
}
```
</step>
</step_list>
</step_by_step>
</reply>"#;

        let parsed_response = Reply::parse_response(reply);
        assert!(parsed_response.is_ok());
    }

    #[test]
    fn test_parsing_code_symbol_to_follow_questions() {
        let response = r#"
<reply>
<steps_to_answer>
- To understand how the `hybrid_search` function works, we need to inspect the implementation of the `Agent::code_search_hybrid` method, which is responsible for performing the hybrid search.
- We also need to understand how the `Agent` struct is initialized for the semantic search, as this is done in the `Agent::prepare_for_semantic_search` method.
- Additionally, we should look at how the search results are combined and scored, as mentioned in the comments.
</steps_to_answer>
<symbol_list>
<symbol>
<name>
Agent::code_search_hybrid
</name>
<line_content>
    let hybrid_search_results = agent.code_search_hybrid(&query).await.unwrap_or(vec![]);
</line_content>
<file_path>
/Users/skcd/scratch/sidecar/sidecar/src/webserver/agent.rs
</file_path>
<thinking>
This line calls the `code_search_hybrid` method on the `Agent` struct, which is likely the core implementation of the hybrid search functionality. We need to inspect this method to understand how the hybrid search is performed.
</thinking>
</symbol>
<symbol>
<name>
Agent::prepare_for_semantic_search
</name>
<line_content>
    let mut agent = Agent::prepare_for_semantic_search(
</line_content>
<file_path>
/Users/skcd/scratch/sidecar/sidecar/src/webserver/agent.rs
</file_path>
<thinking>
This line initializes the `Agent` struct for the semantic search. We should inspect the `prepare_for_semantic_search` method to understand how the `Agent` is set up for the hybrid search, which includes the semantic search component.
</thinking>
</symbol>
<symbol>
<name>
HybridSearchResponse
</name>
<line_content>
    Ok(json(HybridSearchResponse {
</line_content>
<file_path>
/Users/skcd/scratch/sidecar/sidecar/src/webserver/agent.rs
</file_path>
<thinking>
This struct represents the response of the hybrid search. We should inspect its fields to understand how the search results are structured and returned.
</thinking>
</symbol>
</symbol_list>
</reply>"#.to_owned();
        let output = CodeSymbolToAskQuestionsResponse::parse_response(response);
        assert!(output.is_ok());
    }

    #[test]
    fn test_parsing_code_symbol_should_ask_question_response() {
        let response = "<reply>\n<thinking>\nThe `Agent` struct contains a field called `chat_broker` of type `Arc&lt;LLMChatModelBroker&gt;`. This field likely holds the broker or client used to send requests to the LLM (Large Language Model) for chat-based interactions. By probing into the implementation of `LLMChatModelBroker`, we may find where the request is sent to the LLM client.\n</thinking>\n</thinking>\n<context_enough>\nfalse\n</context_enough>\n</reply>";
        let output = CodeSymbolShouldAskQuestionsResponse::parse_response(response.to_owned());
        assert!(output.is_ok());
    }

    /// TODO:(Zi) welp this sucks
    #[test]
    fn test_parsing_windows_like_path() {
        let response = r#"
<reply>
<symbol_list>
<symbol>
<name>
server.listen
</name>
<file_path>
c:\Users\Nicolas\Documents\2.0\gpt-pilot\workspace\RedesingMarket\server\index.js
</file_path>
<thinking>
The server is listening on a specific port defined by the PORT environment variable or 3002 if not provided.
</thinking>
</symbol>
<symbol>
<name>
app.use
</name>
<file_path>
c:\Users\Nicolas\Documents\2.0\gpt-pilot\workspace\RedesingMarket\server\index.js
</file_path>
<thinking>
Defines middleware functions and routes for the Express application.
</thinking>
</symbol>
<symbol>
<name>
connectDB
</name>
<file_path>
c:\Users\Nicolas\Documents\2.0\gpt-pilot\workspace\RedesingMarket\server\index.js
</file_path>
<thinking>
Establishes a connection to the MongoDB database.
</thinking>
</symbol>
</symbol_list>
<step_by_step>
</step_by_step>
</reply>
        "#;
        let parsed_reply = Reply::parse_response(response);
        println!("{:?}", &parsed_reply);
        assert!(parsed_reply.is_ok());
    }

    #[test]
    fn test_parsing_code_symbol_important() {
        let response = r#"<reply>
<thinking>
We need to add a new variant to SymbolEventSubStep enum and create corresponding structs and implementations.
</thinking>
<step_by_step>
<step_list>
<name>SymbolEventSubStep</name>
<file_path>/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/ui_event.rs</file_path>
<step>Add a new Document variant to the SymbolEventSubStep enum</step>
</step_list>
<step_list>
<name>SymbolEventDocumentRequest</name>
<file_path>/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/ui_event.rs</file_path>
<new>true</new>
<step>Create a new struct SymbolEventDocumentRequest similar to SymbolEventEditRequest</step>
</step_list>
<step_list>
<name>DocumentationForSymbolRequest</name>
<file_path>/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/ui_event.rs</file_path>
<new>true</new>
<step>Create a new struct DocumentationForSymbolRequest to hold documentation details</step>
</step_list>
<step_list>
<name>SymbolEventSubStepRequest</name>
<file_path>/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/ui_event.rs</file_path>
<step>Add a new method to implement documentation functionality in SymbolEventSubStepRequest</step>
</step_list>
</step_by_step>
</reply>"#;
        let parsed_response = Reply::parse_response(response);
        println!("{:?}", parsed_response);
        assert!(parsed_response.is_ok());
    }

    #[test]
    fn test_parsing_step_list_items() {
        let response = r#"<step_list>
<name>
get_outline_node_from_snippet
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/agentic/symbol/tool_box.rs
</file_path>
<step>
Update this method to use `self.get_outline_nodes_from_editor()`:

```rust
let symbols_outline = self.get_outline_nodes_from_editor(&fs_file_path, message_properties).await
    .ok_or(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))?
    .into_iter()
    .filter(|outline_node| outline_node.name() == snippet.symbol_name())
    .collect::<Vec<_>>();
```
</step>
</step_list>
"#;
        let parsed_response = StepListItem::parse_from_str(response);
        assert!(parsed_response.is_some());
    }
}
