//! Contains the module which helps us follow code symbols on initial request

use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSymbolFollowInitialRequest {
    code_symbol_content: Vec<String>,
    user_query: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    root_request_id: String,
}

impl CodeSymbolFollowInitialRequest {
    pub fn new(
        code_symbol_content: Vec<String>,
        user_query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        root_request_id: String,
    ) -> Self {
        Self {
            code_symbol_content,
            user_query,
            llm,
            provider,
            api_keys,
            root_request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSymbolToFollow {
    symbol: String,
    line_content: String,
    file_path: String,
    reason_for_selection: String,
}

impl CodeSymbolToFollow {
    pub fn new(
        symbol: String,
        line_content: String,
        file_path: String,
        reason_for_selection: String,
    ) -> Self {
        Self {
            symbol,
            line_content,
            file_path,
            reason_for_selection,
        }
    }

    pub fn reason_for_selection(&self) -> &str {
        &self.reason_for_selection
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn line_content(&self) -> &str {
        &self.line_content
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct CodeSymbolFollowInitialResponse {
    code_symbols_to_follow: Vec<CodeSymbolToFollow>,
}

impl CodeSymbolFollowInitialResponse {
    pub fn code_symbols_to_follow(&self) -> &[CodeSymbolToFollow] {
        self.code_symbols_to_follow.as_slice()
    }

    pub fn parse_response(response: &str) -> Result<Self, ToolError> {
        // we want to gather only the code symbols to follow, the rest is just
        // jank on top
        let code_symbols_response = response
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .into_iter()
            .skip_while(|line| !line.contains("<code_symbols_to_follow>"))
            .skip(1)
            .take_while(|line| !line.contains("</code_symbols_to_follow>"))
            .collect::<Vec<_>>()
            .join("\n");
        // Now we will have sections of the reply in the following format:
        // <code_symbol>
        // <symbol_name>{}</symbol_name>
        // <line_content>{}</line_content>
        // <reason_to_follow>{}</reason_to_follow>
        // </code_symbol>
        // .. repeated
        let mut code_symbols: Vec<CodeSymbolToFollow> = vec![];
        let mut symbol_name = None;
        let mut line_content = None;
        let mut file_path = None;
        let mut index = 0;
        let code_symbol_response_lines = code_symbols_response
            .lines()
            .into_iter()
            .collect::<Vec<_>>();
        while index < code_symbol_response_lines.len() {
            if code_symbol_response_lines[index].contains("<code_symbol>") {
                index = index + 1;
                while index < code_symbol_response_lines.len() {
                    let current_line = code_symbol_response_lines[index];
                    if current_line.contains("<symbol>") && current_line.contains("</symbol>") {
                        // we have the symbol name over here so lets capture it
                        symbol_name = Some(
                            current_line
                                .strip_prefix("<symbol>")
                                .expect("to work")
                                .strip_suffix("</symbol>")
                                .expect("to work")
                                .to_owned(),
                        );
                        index = index + 1;
                        continue;
                    } else if current_line.contains("<line_content>")
                        && current_line.contains("</line_content>")
                    {
                        line_content = Some(
                            current_line
                                .strip_prefix("<line_content>")
                                .expect("to work")
                                .strip_suffix("</line_content>")
                                .expect("to work")
                                .to_owned(),
                        );
                        index = index + 1;
                        continue;
                    } else if current_line.contains("<file_path>")
                        && current_line.contains("</file_path>")
                    {
                        file_path = Some(
                            current_line
                                .strip_prefix("<file_path>")
                                .expect("to work")
                                .strip_suffix("</file_path>")
                                .expect("to work")
                                .to_owned(),
                        );
                        index = index + 1;
                        continue;
                    } else if current_line.contains("<reason_for_selection>") {
                        // now we have the start line which has the reason for selection which we want to parse
                        index = index + 1;
                        let mut reason_for_selection = vec![];
                        while index < code_symbol_response_lines.len()
                            && code_symbol_response_lines[index] != "</reason_for_selection>"
                        {
                            reason_for_selection.push(code_symbol_response_lines[index]);
                            index = index + 1;
                        }

                        if index < code_symbol_response_lines.len()
                            && code_symbol_response_lines[index] == "</reason_for_selection>"
                        {
                            if let (Some(symbol_name), Some(line_content), Some(file_path)) =
                                (symbol_name, line_content, file_path)
                            {
                                code_symbols.push(CodeSymbolToFollow::new(
                                    symbol_name,
                                    line_content,
                                    file_path,
                                    reason_for_selection.join("\n"),
                                ));
                            } else {
                                // aggressive return on failures
                                return Err(ToolError::SerdeConversionFailed);
                            }
                            // we need to reset the state of the symbol name and the
                            // line content over here
                            symbol_name = None;
                            line_content = None;
                            file_path = None;
                            index = index + 1;
                        } else {
                            index = index + 1;
                        }
                    } else {
                        index = index + 1;
                    }
                }
                // we are entering a code symbol loop over here
            } else {
                index = index + 1;
            }
        }
        Ok(Self {
            code_symbols_to_follow: code_symbols,
        })
    }
}

pub struct CodeSymbolFollowInitialRequestBroker {
    llm_client: Arc<LLMBroker>,
}

impl CodeSymbolFollowInitialRequestBroker {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn system_message(&self) -> String {
        format!("You are an expert software engineer how has been tasked with finding the relevant symbols to follow and gather more information before editing the code to solve the user query.

Your task is to locate the relevant code snippets to follow which would solve the user query.

Follow these instructions to the letter:
- Carefully review the user query and understand what code symbols might need following to solve the problem.
- You must be absolutely sure about the code symbols you want to follow to gather the information and have justification for doing so.
- These code symbols might be for understanding the problem a bit better or see code which is not present in the code snippet which has been provided to you in <symbol_content>
- We will visit the code symbols you selected and then ask them, deeper questions and take a look at what they contain. This will be extremely useful for you to build understanding of the codebase before going about solving the user query.
- Think step by step and write a brief summary of how you plan on selecting the code symbols and why and then list out the code symbols.
- Never include code snippets which are present in comments, these can not be followed since the editor will not allow us to click on it.

Your format of reply is shown below
<reply>
<thinking>
{{your reasoning over here}}
</thinking>
<code_symbols_to_follow>
<code_symbol>
<symbol>
{{symbol over here}}
</symbol>
<line_content>
{{line containing the symbol}}
</line_content>
<file_path>
{{file path containing the line content over here}}
</file_path>
<reason_for_selection>
{{your reason for selecting this symbol}}
</reason_for_selection>
</code_symbol>
... more code symbol in the same format as above
</code_symbols_to_follow>
</reply>

In this format, the line content is super important and should contain the line from the code snippet which contains the code symbol which you want to follow")
    }

    fn user_message(&self, request: CodeSymbolFollowInitialRequest) -> String {
        let user_query = request.user_query;
        let code_symbol_content = request.code_symbol_content.join("\n");
        format!(
            r#"<user_query>
{user_query}
</user_query>

<symbol_content>
{code_symbol_content}
</symbol_content>"#
        )
    }
}

#[async_trait]
impl Tool for CodeSymbolFollowInitialRequestBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_code_symbol_follow_initial_request()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm = context.llm.clone();
        let provider = context.provider.clone();
        let api_keys = context.api_keys.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let request =
            LLMClientCompletionRequest::new(llm, vec![system_message, user_message], 0.2, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                request,
                provider,
                vec![
                    (
                        "event_type".to_owned(),
                        "code_symbol_follow_initial_request_broker".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| ToolError::LLMClientError(e))?;
        CodeSymbolFollowInitialResponse::parse_response(response.answer_up_until_now())
            .map(|output| ToolOutput::code_symbol_follow_for_initial_request(output))
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::CodeSymbolFollowInitialResponse;

    #[test]
    fn test_parsing_from_string_works_happy_path() {
        let response = r#"
<reply>
<thinking>
The user query indicates that the `ccode` function is not able to handle the `sinc` function properly. To understand why this is happening and potentially find a solution, we need to look at the implementation of the `ccode` function and how it handles different mathematical functions.

The `ccode` function is part of the `CCodePrinter` class, which is responsible for converting Python expressions to C code strings. The key methods to look at are:

1. `_print_Function`: This method handles the printing of mathematical functions. It likely checks if the function is known and has a special printing method, or falls back to a generic printing method.

2. `known_functions`: This is a dictionary that maps function names to their C code representations. If `sinc` is not present in this dictionary, the `CCodePrinter` may not know how to handle it properly.

3. `_print_Piecewise`: This method handles the printing of piecewise functions, which is relevant since the user provided an example of a piecewise expression that works with `ccode`.

To gather more information and potentially find a solution, we should examine these methods and the `known_functions` dictionary.
</thinking>

<code_symbols_to_follow>
<code_symbol>
<symbol>_print_Function</symbol>
<line_content>def _print_Function(self, expr):</line_content>
<reason_for_selection>
This method is likely responsible for printing mathematical functions, including `sinc`. We need to understand how it handles unknown functions to see why `sinc` is not working properly.
</reason_for_selection>
</code_symbol>

<code_symbol>
<symbol>known_functions</symbol>
<line_content>self.known_functions = dict(known_functions)</line_content>
<reason_for_selection>
This dictionary contains the known functions and their C code representations. If `sinc` is not present in this dictionary, it may explain why `ccode` cannot handle it properly.
</reason_for_selection>
</code_symbol>

<code_symbol>
<symbol>_print_Piecewise</symbol>
<line_content>def _print_Piecewise(self, expr):</line_content>
<reason_for_selection>
The user provided an example of a piecewise expression that works with `ccode`. We should examine this method to understand how it handles piecewise expressions and see if there are any insights into why `sinc` is not working.
</reason_for_selection>
</code_symbol>
</code_symbols_to_follow>
</reply>
        "#;

        let parsed_response = CodeSymbolFollowInitialResponse::parse_response(response);
        assert!(parsed_response.is_ok());
    }
}
