//! Contains the semantic search module which is very midwit but should give us
//! a good sign of life
//! We send all the files in the repository (so we can't enable it for large codebases
//! out of the box) but the goal being deepseek with its unlimited RPM can be used
//! to filter and decide if a file is relevant to the query we are interested in

use crate::agentic::{
    symbol::{events::message_event::SymbolEventMessageProperties, identifier::LLMProperties},
    tool::{
        errors::ToolError,
        input::ToolInput,
        lsp::list_files::list_files,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
};
use quick_xml::de::from_str;
use std::{path::Path, sync::Arc};

/// Blacklisted extensions
pub const EXT_BLACKLIST: &[&str] = &[
    // graphics
    "png", "jpg", "jpeg", "ico", "bmp", "bpg", "eps", "pcx", "ppm", "tga", "tiff", "wmf", "xpm",
    "svg", "riv", "gif", // fonts
    "ttf", "woff2", "fnt", "fon", "otf", // documents
    "pdf", "ps", "doc", "dot", "docx", "dotx", "xls", "xlsx", "xlt", "odt", "ott", "ods", "ots",
    "dvi", "pcl", // media
    "mp3", "ogg", "ac3", "aac", "mod", "mp4", "mkv", "avi", "m4v", "mov", "flv",
    // compiled
    "jar", "pyc", "war", "ear", // compression
    "tar", "gz", "bz2", "xz", "7z", "bin", "apk", "deb", "rpm", "rar", "zip", // binary
    "pkg", "pyd", "pyz", "lib", "pack", "idx", "dylib", "so", // executable
    "com", "exe", "out", "coff", "obj", "dll", "app", "class", // misc.
    "log", "wad", "bsp", "bak", "sav", "dat", "lock", // lock files related
    "json",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticSearchParametersPartial {
    search_query: String,
}

impl SemanticSearchParametersPartial {
    pub fn new(search_query: String) -> Self {
        Self { search_query }
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }
}

impl SemanticSearchParametersPartial {
    pub fn to_string(&self) -> String {
        format!(
            r#"<semantic_search>
<search_query>
{}
</search_query>
</semantic_search>"#,
            &self.search_query
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "semantic_search",
            "description": r#""#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "search_query": {
                        "type": "string",
                        "description": "",
                    },
                },
                "required": ["search_query"],
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct SemanticSearchRequest {
    working_directory: String,
    llm_properties: LLMProperties,
    user_query: String,
    message_properties: SymbolEventMessageProperties,
}

impl SemanticSearchRequest {
    pub fn new(
        working_directory: String,
        llm_properties: LLMProperties,
        user_query: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            working_directory,
            llm_properties,
            user_query,
            message_properties,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "response")]
pub struct SemanticSearchLLMResponse {
    reason: String,
    score: f32,
    is_relevant: bool,
}

impl SemanticSearchLLMResponse {
    /// send a negative response to the caller this is a safe guard against any
    /// kind of errors
    fn negative_response() -> Self {
        Self {
            reason: "".to_owned(),
            score: 0.0,
            is_relevant: false,
        }
    }

    pub fn is_relevant(&self) -> bool {
        self.is_relevant
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn score(&self) -> f32 {
        self.score
    }
}

#[derive(Debug, Clone)]
pub struct SemanticSearchResponse {
    // contains a tuple with the file path and the semantic search llm response
    file_decisions: Vec<(String, SemanticSearchLLMResponse)>,
}

impl SemanticSearchResponse {
    pub fn file_decisions(self) -> Vec<(String, SemanticSearchLLMResponse)> {
        self.file_decisions
    }
}

#[derive(Clone)]
pub struct SemanticSearch {
    llm_client: Arc<LLMBroker>,
}

impl SemanticSearch {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn system_message(&self) -> String {
        format!(
            r#"You are an expert software engineer tasked with identifying if the file listed below is relevant to the user query.
- You have to understand the user query which will be provided to you in the <user_query> xml section.
- The file which you want to check for relevance will be provided to you in <file_to_check> xml section.

Before answering if the file is relevant you have to do the following:
- Think deeply if the file is relevant or if it is not relevant to the user query.
- Give the file a relevance score from 0 to 10 where 0 is the lowest score and 10 is the highest score which shows how relevant the file is
- You should provide a reason for your score too.
- And finally a true/false indicating if you believe the file is relevant

Your output should be in the following format:
<response>
<reason>
{{your reasoning about the file relevance over here}}
</reason>
<score>
{{your score for the file relevance ranging from 0.0 to 10.0}}
</score>
<is_relevant>
{{true/false if the file is relevant or not}}
</is_relevant>
</response>"#
        )
    }

    fn user_message(
        &self,
        user_query: String,
        fs_file_path: String,
        file_content: String,
    ) -> String {
        format!(
            r#"<user_query>
{user_query}
</user_query>
<fs_file_path>
{fs_file_path}
</fs_file_path>
<fs_file_content>
```
{file_content}
```
</fs_file_content>

Reminder about the output format:
<response>
<reason>
{{your reasoning about the file relevance over here}}
</reason>
<score>
{{your score for the file relevance ranging from 0.0 to 10.0}}
</score>
<is_relevant>
{{true/false if the file is relevant or not}}
</is_relevant>
</response>"#
        )
    }
}

#[async_trait]
impl Tool for SemanticSearch {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_semantic_search()?;

        // grab relevant context for the deepseek invocation
        let llm_properties = context.llm_properties.clone();
        let user_query = context.user_query.to_owned();
        let message_properties = context.message_properties.clone();
        let root_request_id = message_properties.root_request_id().to_owned();

        // grab all the files over here respecting the .gitignore as well
        let (list_files, _) = list_files(&Path::new(&context.working_directory), true, 1000);
        let llm_client = self.llm_client.clone();

        let llm_reasoning_call = stream::iter(
            list_files
                .into_iter()
                .filter(|file_path| file_path.is_file())
                .filter(|file_path| {
                    if let Some(extension) = file_path.extension() {
                        let is_package_json = match file_path.file_name() {
                            Some(os_str) => os_str == "package.json",
                            None => false,
                        };
                        // if its package.json then allow it
                        if is_package_json {
                            return true;
                        }
                        // make sure that the extension is not block listed
                        !EXT_BLACKLIST.into_iter().any(|ext| *ext == extension)
                    } else {
                        false
                    }
                })
                .map(|file_path| {
                    (
                        file_path,
                        llm_properties.clone(),
                        user_query.to_owned(),
                        root_request_id.to_owned(),
                        llm_client.clone(),
                    )
                }),
        )
        .map(
            |(fs_file_path, llm_properties, user_query, root_request_id, llm_client)| async move {
                let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
                let file_content = tokio::fs::read(fs_file_path.to_owned()).await;
                match file_content {
                    Ok(file_content) => {
                        let llm_request = LLMClientCompletionRequest::new(
                            LLMType::DeepSeekCoderV3,
                            vec![
                                LLMClientMessage::system(self.system_message()),
                                LLMClientMessage::user(self.user_message(
                                    user_query,
                                    fs_file_path.to_string_lossy().to_string(),
                                    String::from_utf8_lossy(&file_content).to_string(),
                                )),
                            ],
                            0.2,
                            None,
                        );
                        let completion_response = llm_client
                            .stream_completion(
                                llm_properties.api_key().clone(),
                                llm_request,
                                llm_properties.provider().clone(),
                                vec![
                                    ("root_id".to_owned(), root_request_id),
                                    (
                                        "event_type".to_owned(),
                                        "deepseek_semantic_search".to_owned(),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                                sender,
                            )
                            .await;

                        // now we parse out the completion response to make sure that it passes the checks we are interested in
                        match completion_response {
                            Ok(completion_response) => (
                                fs_file_path.to_string_lossy().to_string(),
                                from_str::<SemanticSearchLLMResponse>(
                                    completion_response.answer_up_until_now(),
                                )
                                .unwrap_or(SemanticSearchLLMResponse::negative_response()),
                            ),
                            Err(_) => (
                                fs_file_path.to_string_lossy().to_string(),
                                SemanticSearchLLMResponse::negative_response(),
                            ),
                        }
                    }
                    Err(_e) => (
                        fs_file_path.to_string_lossy().to_string(),
                        SemanticSearchLLMResponse::negative_response(),
                    ),
                }
            },
        )
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;

        Ok(ToolOutput::SemanticSearch(SemanticSearchResponse {
            file_decisions: llm_reasoning_call,
        }))
    }

    fn tool_description(&self) -> String {
        format!(
            r#"### semantic_search
Allows you to query a codebase to determine the relevance of each file with respect to the your question. It will search all available files, perform a semantic analysis, and return a yes/no-style result indicating whether each file is considered relevant.
The best practice is to phrase your queries as questions that can be answered with a yes or no. Examples:
- "Does the code contain references to function XYZ?"
- "Is there a struct or class that handles authentication?"
- "Is there a method that uses an external API to retrieve data?"
- "Does the codebase implement any logic for computing user statistics?""#
        )
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
- question: (required) The search question which you want to run against the codebase.

Usage:
<semantic_search>
<question>
Your search question over here
</question>
</semantic_search>"#
        )
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
