use async_trait::async_trait;
use quick_xml::de::from_str;
use std::sync::Arc;

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewSubSymbolRequiredRequest {
    user_query: String,
    plan: String,
    symbol_name: String,
    symbol_content: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl NewSubSymbolRequiredRequest {
    pub fn new(
        user_query: String,
        plan: String,
        symbol_name: String,
        symbol_content: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            user_query,
            plan,
            symbol_name,
            symbol_content,
            llm_properties,
            root_request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "method")]
pub struct NewSymbol {
    #[serde(rename = "method_name")]
    symbol_name: String,
    reason_to_create: String,
}

impl NewSymbol {
    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub fn reason_to_create(&self) -> &str {
        &self.reason_to_create
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "new_methods")]
pub struct NewSubSymbolRequiredResponse {
    #[serde(default, rename = "$value")]
    symbols: Vec<NewSymbol>,
}

impl NewSubSymbolRequiredResponse {
    pub fn symbols(self) -> Vec<NewSymbol> {
        self.symbols
    }

    fn unescape_thinking_string(self) -> Self {
        let fixed_symbols = self
            .symbols
            .into_iter()
            .map(|symbol| {
                let symbol_name = symbol.symbol_name;
                let reason_to_create = symbol
                    .reason_to_create
                    .lines()
                    .map(|line| unescape_xml(line.to_owned()))
                    .collect::<Vec<_>>()
                    .join("\n");
                NewSymbol {
                    symbol_name,
                    reason_to_create,
                }
            })
            .collect();
        Self {
            symbols: fixed_symbols,
        }
    }
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let tags_to_exist = vec!["<reply>", "</reply>", "<new_methods>", "</new_methods>"];
        if tags_to_exist.into_iter().any(|tag| !response.contains(tag)) {
            return Err(ToolError::MissingXMLTags);
        }
        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<new_methods>"))
            .skip(1)
            .take_while(|line| !line.contains("</new_methods>"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = format!(
            r#"<new_methods>
{lines}
</new_methods>"#
        );

        let mut final_lines = vec![];
        let mut is_inside = false;
        for line in lines.lines() {
            if line == "<reason_to_create>" {
                is_inside = true;
                final_lines.push(line.to_owned());
                continue;
            } else if line == "</reason_to_create>" {
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

        let parsed_response = from_str::<NewSubSymbolRequiredResponse>(&final_lines.join("\n"));
        match parsed_response {
            Err(_e) => Err(ToolError::SerdeConversionFailed),
            Ok(parsed_list) => Ok(parsed_list.unescape_thinking_string()),
        }
    }
}

pub struct NewSubSymbolRequired {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl NewSubSymbolRequired {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    pub fn system_message(&self, context: &NewSubSymbolRequiredRequest) -> String {
        let symbol_name = context.symbol_name.to_owned();
        format!(r#"You are an expert software engineer who is an expert at figuring out if we need to create new methods inside a class or the implementation block of the class or if existing methods can be edited to satisfy the user query.
- You will be given the original user query in <user_query>
- You will be provided the class in <symbol_content> section.
- The plan of edits which we want to do on this class is also given in <plan> section.
- You have to decide if we can make changes to the existing methods inside this class or if we need to create new methods which will belong to this class or the implementation block
- Creating a new methods inside the implementation block is hard, so only do it if its absolutely required and is said so in the plan.
- Before replying, think step-by-step on what approach we want to take and put your thinking in <thinking> section.
Your reply should be in the following format:
<reply>
<thinking>
{{your thinking process before replying}}
</thinking>
<new_methods>
<method>
<method_name>
{{name of the method}}
</method_name>
<reason_to_create>
{{your reason for creating this new method inside the class}}
</reason_to_create>
</method>
{{... more methods which should belong in the list}}
</new_methods>
</reply>

- Please make sure to keep your reply in the <reply> tag and the new methods which you need to generate properly in the format under <new_symbols> section.
- You can only create methods or functions for `{symbol_name}` and no other struct, enum or type.
- If you do not need to create a new method or function for `{symbol_name}` just give back an empty list in <new_methods> section
- Remember you cannot create new classes or enums or types, just methods or functions at this point, even if you think we need to create new classes or enums or types."#).to_owned()
    }

    pub fn user_message(&self, request: NewSubSymbolRequiredRequest) -> String {
        let user_query = request.user_query;
        let plan = request.plan;
        let symbol_content = request.symbol_content;
        format!(
            r#"<user_query>
{user_query}
</user_query>

<plan>
{plan}
</plan>

<symbol_content>
{symbol_content}
</symbol_content>"#
        )
    }
}

#[async_trait]
impl Tool for NewSubSymbolRequired {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.get_new_sub_symbol_for_code_editing()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message(&context));
        let user_message = LLMClientMessage::user(self.user_message(context));
        let llm_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );
        let mut retries = 0;
        loop {
            if retries >= 4 {
                return Err(ToolError::RetriesExhausted);
            }
            let (llm, api_key, provider) = if retries % 2 == 1 {
                (
                    llm_properties.llm().clone(),
                    llm_properties.api_key().clone(),
                    llm_properties.provider().clone(),
                )
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };
            let cloned_message = llm_request.clone().set_llm(llm);
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
                            "new_sub_sybmol_required".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;
            match response {
                Ok(response) => {
                    if let Ok(parsed_response) =
                        NewSubSymbolRequiredResponse::parse_response(response.answer_up_until_now())
                    {
                        return Ok(ToolOutput::new_sub_symbol_creation(parsed_response));
                    } else {
                        retries = retries + 1;
                        continue;
                    }
                }
                Err(_e) => {
                    retries = retries + 1;
                    continue;
                }
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
    use super::NewSubSymbolRequiredResponse;

    #[test]
    fn test_parsing_works() {
        let response = r#"
        <reply>
        <thinking>
        Let's analyze the problem and the proposed solution:
        
        1. The issue is that the Identity matrix is being misinterpreted as the complex number 1j when using lambdify.
        
        2. The plan suggests modifying the NumPyPrinter class to handle the Identity matrix correctly.
        
        3. We need to add a specific method to print the Identity matrix using numpy.eye().
        
        4. The existing NumPyPrinter class already has methods for printing various matrix operations and functions.
        
        5. Adding a new method to handle the Identity matrix seems to be the most appropriate solution, as it follows the existing pattern in the class.
        
        6. We don't need to create a new function or symbol, but rather add a new method to the existing NumPyPrinter class.
        
        Based on this analysis, we can conclude that we don't need to create any new symbols (functions) in this class. Instead, we should add a new method to handle the Identity matrix printing.
        </thinking>
        
        <new_methods>
        </new_methods>
        </reply>
        "#;

        let output = NewSubSymbolRequiredResponse::parse_response(&response);
        assert!(output.is_ok());
    }

    #[test]
    fn test_output_parsing() {
        let response = "<reply>\n<thinking>\nLet's analyze the problem and the proposed solution:\n\n1. The issue is that the Identity matrix is being misinterpreted as the complex number 1j when using lambdify.\n\n2. The plan suggests modifying the NumPyPrinter class to handle the Identity matrix correctly.\n\n3. We need to add a specific method to print the Identity matrix using numpy.eye().\n\n4. The existing NumPyPrinter class already has methods for printing various matrix operations and functions.\n\n5. Adding a new method for printing the Identity matrix aligns with the existing structure of the class.\n\n6. We don't need to create entirely new methods, but rather add one specific method to handle the Identity matrix.\n\nBased on this analysis, we should add a new method to the NumPyPrinter class to handle the Identity matrix specifically.\n</thinking>\n\n<new_methods>\n<method>\n<method_name>_print_Identity</method_name>\n<reason_to_create>\nWe need to add a specific method to handle the printing of Identity matrices in NumPy format. This method will use numpy.eye() to create the correct representation of an identity matrix. This aligns with the existing structure of the NumPyPrinter class, which has specific methods for different matrix operations and functions.\n</reason_to_create>\n</method>\n</new_methods>\n</reply>";
        let output = NewSubSymbolRequiredResponse::parse_response(&response);
        assert!(output.is_ok());
    }
}
