//! We allow for another round of COT to happen here before we start editing
//! This is to show the agent the code symbols it has gathered and come up with
//! an even better plan after the initial fetch

use async_trait::async_trait;
use quick_xml::de::from_str;
use std::{collections::HashMap, sync::Arc};

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

fn escape_xml(s: String) -> String {
    s.replace("\"", "&quot;")
        .replace("'", "&apos;")
        .replace(">", "&gt;")
        .replace("<", "&lt;")
        .replace("&", "&amp;")
}

fn dirty_unescape_fix(s: String) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn unescape_xml(s: String) -> String {
    quick_xml::escape::unescape(&s)
        .map(|output| output.to_string())
        .unwrap_or(dirty_unescape_fix(s))
        .to_string()
}

#[derive(Debug, Clone)]
pub struct PlanningBeforeCodeEditRequest {
    user_query: String,
    files_with_content: HashMap<String, String>,
    original_plan: String,
    llm_properties: LLMProperties,
    root_request_id: String,
    _exchange_id: String,
    _cancellation_token: tokio_util::sync::CancellationToken,
}

impl PlanningBeforeCodeEditRequest {
    pub fn new(
        user_query: String,
        files_with_content: HashMap<String, String>,
        original_plan: String,
        llm_properties: LLMProperties,
        root_request_id: String,
        exchange_id: String,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self {
            user_query,
            files_with_content,
            original_plan,
            llm_properties,
            root_request_id,
            _exchange_id: exchange_id,
            _cancellation_token: cancellation_token,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol")]
pub struct CodeEditingSymbolPlan {
    #[serde(rename = "name")]
    symbol_name: String,
    file_path: String,
    plan: String,
}

impl CodeEditingSymbolPlan {
    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn plan(&self) -> &str {
        &self.plan
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "symbol_list")]
pub struct PlanningBeforeCodeEditResponse {
    #[serde(rename = "$value")]
    final_plan_list: Vec<CodeEditingSymbolPlan>,
}

impl PlanningBeforeCodeEditResponse {
    pub fn final_plan_list(self) -> Vec<CodeEditingSymbolPlan> {
        self.final_plan_list
    }

    fn unescape_plan_string(self) -> Self {
        let final_plan_list = self
            .final_plan_list
            .into_iter()
            .map(|plan_item| {
                let symbol_name = plan_item.symbol_name;
                let file_path = plan_item.file_path;
                let plan = plan_item
                    .plan
                    .lines()
                    .map(|line| unescape_xml(line.to_owned()))
                    .collect::<Vec<_>>()
                    .join("\n");
                CodeEditingSymbolPlan {
                    symbol_name,
                    file_path,
                    plan,
                }
            })
            .collect::<Vec<_>>();
        Self { final_plan_list }
    }

    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let tags_to_check = vec![
            "<reply>",
            "</reply>",
            "<thinking>",
            "</thinking>",
            "<symbol_list>",
            "</symbol_list>",
        ];
        if tags_to_check.into_iter().any(|tag| !response.contains(tag)) {
            return Err(ToolError::MissingXMLTags);
        }
        // otherwise its correct and we need to grab the content between the <code_symbol> tags
        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<symbol_list>"))
            .skip(1)
            .take_while(|line| !line.contains("</symbol_list>"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = format!(
            r#"<symbol_list>
{lines}
</symbol_list>"#
        );

        let mut final_lines = vec![];
        let mut is_inside = false;
        for line in lines.lines() {
            if line == "<plan>" {
                is_inside = true;
                final_lines.push(line.to_owned());
                continue;
            } else if line == "</plan>" {
                is_inside = false;
                final_lines.push(line.to_owned());
                continue;
            }
            if is_inside {
                final_lines.push(escape_xml(line.to_owned()));
            } else {
                final_lines.push(line.to_owned());
            }
        }

        let parsed_response = from_str::<PlanningBeforeCodeEditResponse>(&final_lines.join("\n"));
        match parsed_response {
            Err(_e) => Err(ToolError::SerdeConversionFailed),
            Ok(parsed_list) => Ok(parsed_list.unescape_plan_string()),
        }
    }
}

pub struct PlanningBeforeCodeEdit {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl PlanningBeforeCodeEdit {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    fn system_message(&self) -> String {
        r#"You are an expert software engineer who has to come up with a plan to help with the user query. A junior engineer has already taken a pass at identifying the important code symbols in the codebase and a plan to tackle the problem. Your job is to take that plan, and analyse the code and correct any mistakes in the plan and make it more informative. You never make mistakes when coming up with the plan.
- The user query will be provided in <user_query> section of the message.
- We are working at the level of code symbols, which implies that when coming up with a plan to help, you should only select the symbols which are present in the code. Code Symbols can be functions, classes, enums, types etc.
as an example:
```rust
struct Something {{
    // rest of the code..
}}
```
is a code symbol since it represents a struct in rust, similarly
```py
def something():
    pass
```
is a code symbol since it represents a function in python.
- The original plan will be provided to you in the <original_plan> section of the message.
- We will show you the full file content where the selected code symbols are present, this is present in the <files_in_selection> section. You should use this to analyse if the plan has covered all the code symbols which need editing or changes.
- Deeply analyse the provided files in <files_in_selection> along with the <original_plan> and the <user_query> and come up with a detailed plan of what changes needs to made and the order in which the changes need to happen.
- First let's think step-by-step on how to reply to the user query and then reply to the user query.
- The output should be strictly in the following format:
<reply>
<thinking>
{{your thoughts here on how to go about solving the problem and analysing the original plan which was created}}
</thinking>
<symbol_list>
<symbol>
<name>
{{name of the symbol you want to change}}
</name>
<file_path>
{{file path of the symbol where its present, this should be the absolute path as given to you in the original query}}
</file_path>
<plan>
{{your modified plan for this symbol}}
</plan>
</symbol>
{{more symbols here following the same format as above}}
</symbol_list>
</reply>

Your reply should always be in <reply> tags in xml, if you start your reply without the <reply> tag it will fail to parse.
A correct example of reply coming from you always starts with the <reply> tag as an example again:
<reply>
<thinking>
{{your thinking here}}
</thinking>
<symbol_list>
{{the rest of the symbol list items here}}
</symbol_list>
</reply>"#.to_owned()
    }

    fn user_query(&self, request: PlanningBeforeCodeEditRequest) -> String {
        let user_query = request.user_query;
        let original_plan = request.original_plan;
        let files_with_content = request
            .files_with_content
            .into_iter()
            .map(|(file_path, content)| {
                format!(
                    r#"<file_content>
<file_path>
{file_path}
</file_path>
<content>
{content}
</content>
</file_content>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"<user_query>
{user_query}
</user_query>

<files_in_selection>
{files_with_content}
</files_in_selection>

<original_plan>
{original_plan}
</original_plan>

Start your reply with the <reply> tag"#
        )
    }
}

#[async_trait]
impl Tool for PlanningBeforeCodeEdit {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.plan_before_code_editing()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_query(context));
        let message_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );
        let mut retries = 0;
        loop {
            if retries > 4 {
                return Err(ToolError::MissingXMLTags);
            }
            let (llm, api_key, provider) = if retries % 2 == 1 {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            } else {
                (
                    llm_properties.llm().clone(),
                    llm_properties.api_key().clone(),
                    llm_properties.provider().clone(),
                )
            };
            let cloned_message = message_request.clone().set_llm(llm);
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key,
                    cloned_message.clone(),
                    provider,
                    vec![
                        ("event_type".to_owned(), "plan_before_code_edit".to_owned()),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;
            if let Ok(response) = response {
                match PlanningBeforeCodeEditResponse::parse_response(response.answer_up_until_now())
                {
                    Ok(parsed_response) => {
                        println!("tool::planning_before_code_edit::parsed::success");
                        // Now parse the response over here in the format we want it to be in
                        // we need to take care of the xml tags like ususal over here.. sigh
                        return Ok(ToolOutput::planning_before_code_editing(parsed_response));
                    }
                    Err(e) => {
                        println!("tool::planning_before_code_edit::parsed::error");
                        println!("{:?}", e);
                        retries = retries + 1;
                    }
                }
            } else {
                println!("tool::planning_before_code_edit::response::error");
                retries = retries + 1;
            }
        }
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
    use super::PlanningBeforeCodeEditResponse;

    #[test]
    fn test_parsing_output_works() {
        let response = r#"
        <reply>
        <thinking>
        After analyzing the code and the original plan, I believe some modifications and additions are necessary to properly address the issue:
        
        1. The root cause of the problem is in the `sympify` function, which attempts to evaluate unknown expressions. We need to modify this function to prevent unsafe evaluation.
        
        2. The `eval_expr` function is not the main issue here, as it's just evaluating the code that has already been processed by `stringify_expr`. The real problem occurs before this point.
        
        3. The `parse_expr` function doesn't need to be significantly changed, as it's not directly responsible for the unsafe evaluation.
        
        4. The `__eq__` method in `Expr` class is where the sympify is called, which leads to the unsafe evaluation. We need to modify this method to handle unknown objects safely.
        
        5. We should add a new function to safely convert objects to SymPy expressions without using `eval`.
        
        Based on these observations, I'll modify the plan to better address the issue.
        </thinking>
        
        <symbol_list>
        <symbol>
        <name>sympify</name>
        <file_path>sympy/core/sympify.py</file_path>
        <plan>
        Modify the sympify function to:
        1. Check if the input is already a SymPy type or a basic Python type that can be safely converted.
        2. If not, instead of using eval, use a new safe_convert function to attempt conversion.
        3. If safe conversion fails, raise a SympifyError instead of evaluating unknown expressions.
        </plan>
        </symbol>
        
        <symbol>
        <name>safe_convert</name>
        <file_path>sympy/core/sympify.py</file_path>
        <plan>
        Add a new function safe_convert that:
        1. Attempts to convert known types to SymPy expressions without using eval.
        2. Returns None if the conversion is not possible.
        This function will be used by sympify to safely convert objects.
        </plan>
        </symbol>
        
        <symbol>
        <name>__eq__</name>
        <file_path>sympy/core/expr.py</file_path>
        <plan>
        Modify the __eq__ method in the Expr class to:
        1. Use the modified sympify function to convert the other object.
        2. If sympify raises a SympifyError, return False instead of raising an exception.
        This will prevent unsafe evaluation of unknown objects during equality comparison.
        </plan>
        </symbol>
        
        <symbol>
        <name>parse_expr</name>
        <file_path>sympy/parsing/sympy_parser.py</file_path>
        <plan>
        Modify parse_expr to:
        1. Use the safe_convert function first to attempt conversion without parsing.
        2. Only proceed with parsing if safe_convert returns None.
        3. Ensure that the resulting expression is a valid SymPy type before returning.
        </plan>
        </symbol>
        
        <symbol>
        <name>eval_expr</name>
        <file_path>sympy/parsing/sympy_parser.py</file_path>
        <plan>
        Modify eval_expr to:
        1. Only accept pre-validated SymPy expressions or Python code generated by stringify_expr.
        2. Raise an exception if an invalid input is provided.
        This will add an extra layer of security, although the main fix will be in the sympify function.
        </plan>
        </symbol>
        </symbol_list>
        </reply>
        "#;

        let parsed_response = PlanningBeforeCodeEditResponse::parse_response(&response);
        assert!(parsed_response.is_ok());
    }

    #[test]
    fn test_with_extra_data() {
        let output = "<reply>\n<thinking>\nAfter analyzing the code and the original plan, I believe some modifications and additions are necessary to properly address the issue:\n\n1. The root cause of the problem is in the `sympify` function, which attempts to evaluate unknown expressions. We need to modify this function to prevent unsafe evaluation.\n\n2. The `eval_expr` function is not the main issue here, as it's just evaluating the code that has already been processed by `stringify_expr`. The real problem occurs before this point.\n\n3. The `parse_expr` function doesn't need to be significantly changed, as it's not directly responsible for the unsafe evaluation.\n\n4. The `__eq__` method in `Expr` class is where the sympify is called, which leads to the unsafe evaluation. We need to modify this method to handle unknown objects safely.\n\n5. We should add a new function to safely convert objects to SymPy expressions without using `eval`.\n\nBased on these observations, I'll modify the plan to better address the issue.\n</thinking>\n\n<symbol_list>\n<symbol>\n<name>sympify</name>\n<file_path>sympy/core/sympify.py</file_path>\n<plan>\nModify the sympify function to:\n1. Check if the input is already a SymPy type or a basic Python type that can be safely converted.\n2. If not, instead of using eval, use a new safe_convert function to attempt conversion.\n3. If safe conversion fails, raise a SympifyError instead of evaluating unknown expressions.\n</plan>\n</symbol>\n\n<symbol>\n<name>safe_convert</name>\n<file_path>sympy/core/sympify.py</file_path>\n<plan>\nAdd a new function safe_convert that:\n1. Attempts to convert known types to SymPy expressions without using eval.\n2. Returns None if the conversion is not possible.\nThis function will be used by sympify to safely convert objects.\n</plan>\n</symbol>\n\n<symbol>\n<name>__eq__</name>\n<file_path>sympy/core/expr.py</file_path>\n<plan>\nModify the __eq__ method in the Expr class to:\n1. Use the modified sympify function to convert the other object.\n2. If sympify raises a SympifyError, return False instead of raising an exception.\nThis will prevent unsafe evaluation of unknown objects during equality comparison.\n</plan>\n</symbol>\n\n<symbol>\n<name>parse_expr</name>\n<file_path>sympy/parsing/sympy_parser.py</file_path>\n<plan>\nModify parse_expr to:\n1. Use the safe_convert function first to attempt conversion without parsing.\n2. Only proceed with parsing if safe_convert returns None.\n3. Ensure that the resulting expression is a valid SymPy type before returning.\n</plan>\n</symbol>\n\n<symbol>\n<name>eval_expr</name>\n<file_path>sympy/parsing/sympy_parser.py</file_path>\n<plan>\nModify eval_expr to:\n1. Only accept pre-validated SymPy expressions or Python code generated by stringify_expr.\n2. Raise an exception if an invalid input is provided.\nThis will add an extra layer of security, although the main fix will be in the sympify function.\n</plan>\n</symbol>\n</symbol_list>\n</reply>";
        let parsed_response = PlanningBeforeCodeEditResponse::parse_response(&output);
        println!("{:?}", &parsed_response);
        assert!(parsed_response.is_ok());
    }
}
