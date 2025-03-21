use async_trait::async_trait;
use serde_xml_rs::from_str;
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

#[derive(Clone, Debug, serde::Serialize)]
pub struct FindFileForSymbolRequest {
    fs_file_path: String,
    symbol_name: String,
    imports: String,
    import_file_locations: Vec<String>,
    user_query: String,
    code_content: String,
    root_request_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FindFileForSymbolResponse {
    thinking: String,
    fs_file_path: String,
}

impl FindFileForSymbolResponse {
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        from_str::<Self>(response).map_err(|_e| ToolError::SerdeConversionFailed)
    }
}

impl FindFileForSymbolRequest {
    pub fn new(
        fs_file_path: String,
        symbol_name: String,
        imports: String,
        import_file_locations: Vec<String>,
        user_query: String,
        code_content: String,
        root_request_id: String,
    ) -> Self {
        Self {
            fs_file_path,
            symbol_name,
            imports,
            import_file_locations,
            user_query,
            code_content,
            root_request_id,
        }
    }
}

/// Finds the file location for a new symbol which has to be written
/// this might involve creating a new file (which is not easy always)
pub struct FindFileForNewSymbol {
    llm_client: Arc<LLMBroker>,
    gemini_llm_properties: LLMProperties,
}

impl FindFileForNewSymbol {
    pub fn new(llm_client: Arc<LLMBroker>, gemini_llm_properties: LLMProperties) -> Self {
        Self {
            llm_client,
            gemini_llm_properties,
        }
    }

    fn system_message(&self) -> String {
        r#"You are an expert at figuring out where to place new code for the user query.
We could not find the code at any location, so we are going to create some new code but we want to make sure that its placed logically in the right location.

- We have gathered the code locations of all the imports in the file we are currently in, since often times the new code will be placed in one of the imports or somewhere close by in the directory.
- The imports for the current file will be also shown to you in <imports> section
- We will tell you about the user query in <user_query> tag, you should use this information to understand the context in which the new code is being added.
- We will also tell you about the file we are currently in and the section of the code we are looking at, this will help you better understand the context of the change, this information will be presented to you in <code_in_selection> section.
- The various files which are imported in the file we are currently in will be shown to you in <imported_files> section of the code. It might be necessary to create a new file if none of the current file can hold the new code which we want to write
- The user query might be talking about various symbols, but we are going to focus on the symbol which is present in <symbol_to_focus> section.
- First lets think step by step, and then reply with the file path where this change needs to be made.

Your reply should be in the following format:
<reply>
<thinking>
{your thinking here for selecting the file path}
</thinking>
<file_path>
{the file path where the change needs to be made}
</file_path>
</reply>
"#.to_owned()
    }

    fn user_message(&self, request: FindFileForSymbolRequest) -> String {
        let imports = request.imports;
        let import_files = request.import_file_locations.join("\n");
        let file_path = request.fs_file_path;
        let user_query = request.user_query;
        let symbol_to_focus = request.symbol_name;
        let code_content = request.code_content;
        format!(
            r#"<imports>
{imports}
</imports>

<code_in_selection>
<file_path>
{file_path}
</file_path>
<content>
{code_content}
</content>
</code_in_selection>

<imported_files>
{import_files}
</imported_files>

<user_query>
{user_query}
</user_query>

<symbol_to_focus>
{symbol_to_focus}
</symbol_to_focus>

Please follow the format given below strictly:
<reply>
<thinking>
{{your thinking here for selecting the file path}}
</thinking>
<file_path>
{{the file path where the change needs to be made}}
</file_path>
</reply>"#
        )
    }
}

#[async_trait]
impl Tool for FindFileForNewSymbol {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.find_file_for_new_symbol()?;
        let root_request_id = context.root_request_id.to_owned();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let message_request = LLMClientCompletionRequest::new(
            self.gemini_llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                self.gemini_llm_properties.api_key().clone(),
                message_request,
                self.gemini_llm_properties.provider().clone(),
                vec![
                    (
                        "event_type".to_owned(),
                        "find_file_for_new_symbol".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| ToolError::LLMClientError(e))?;
        let parsed_response =
            FindFileForSymbolResponse::parse_response(response.answer_up_until_now())?;
        Ok(ToolOutput::find_file_for_new_symbol(parsed_response))
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
