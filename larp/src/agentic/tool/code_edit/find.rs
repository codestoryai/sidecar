use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::{
    agentic::tool::{
        code_edit::models::broker::{CodeSnippet, CodeSnippetForEditing},
        errors::ToolError,
        input::ToolInput,
        output::{CodeToEditToolOutput, ToolOutput},
        r#type::{Tool, ToolRewardScale},
    },
    chunking::languages::TSLanguageParsing,
    inline_completion::symbols_tracker::SymbolTrackerInline,
};

use super::models::broker::CodeEditBroker;

// So there are multiple cases which can happen here:
// 1. We are going to edit(add/delete) an already present section (we need to check for references either way)
// 2. We are going to add a new section to the code (no checks for references etc required)
// 3. The other option is going the diff way and generating code (we know this works
// cause sweep and aider are using this approach) [not doing this]

pub struct FindCodeSectionsToEdit {
    _symbol_tracking: Arc<SymbolTrackerInline>,
    ts_language_config: Arc<TSLanguageParsing>,
    code_broker: Arc<CodeEditBroker>,
    llm_client: Arc<LLMBroker>,
}

impl FindCodeSectionsToEdit {
    pub fn new(
        symbol_tracking: Arc<SymbolTrackerInline>,
        ts_language_config: Arc<TSLanguageParsing>,
        code_broker: Arc<CodeEditBroker>,
        llm_client: Arc<LLMBroker>,
    ) -> Self {
        Self {
            _symbol_tracking: symbol_tracking,
            ts_language_config,
            code_broker,
            llm_client,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FindCodeSelectionInput {
    fs_file_path: String,
    file_content: String,
    language: String,
    file_path: String,
    user_query: String,
    llm_type: LLMType,
    api_key: LLMProviderAPIKeys,
    provider: LLMProvider,
    root_request_id: String,
}

impl FindCodeSelectionInput {
    pub fn model(&self) -> &LLMType {
        &self.llm_type
    }
}

#[async_trait]
impl Tool for FindCodeSectionsToEdit {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let find_code_context = input.is_code_find()?;
        let snippets = self.ts_language_config.chunk_file(
            &find_code_context.file_path,
            &find_code_context.file_content,
            None,
            Some(&find_code_context.language),
        );
        let id_to_spans = snippets
            .iter()
            .enumerate()
            .map(|(idx, snippet)| (idx as i64, (snippet.start, snippet.end)))
            .collect::<HashMap<_, _>>();
        let code_snippets = snippets
            .into_iter()
            .filter_map(|snippet| {
                if let Some(data) = snippet.data {
                    Some(CodeSnippet::new(
                        data,
                        snippet.start as i64,
                        snippet.end as i64,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let llm_message =
            self.code_broker
                .find_code_section_to_edit(&CodeSnippetForEditing::new(
                    code_snippets,
                    find_code_context.model().clone(),
                    find_code_context.file_path.to_owned(),
                    find_code_context.user_query.to_owned(),
                ))?;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                find_code_context.api_key,
                llm_message,
                find_code_context.provider,
                vec![("request".to_owned(), "find_code_edit".to_owned())]
                    .into_iter()
                    .collect(),
                sender,
            )
            .await
            .map_err(|e| ToolError::LLMClientError(e))?;
        // Now that we have the response we want to parse it properly based on the xml schema
        // we had defined before which was:
        // <reply>
        // <sections>
        // <section>
        // <id>
        // {id_to_grab}
        // </id>
        // <thinking>
        // {thinking_to_grab}
        // </thinking>
        // </section>
        // ... more sections
        // </reply>
        let mut ids = vec![];
        let mut thinkings = vec![];
        let mut is_id = false;
        let mut is_thinking = false;
        // TODO(skcd): Parsing here can be improved
        response.answer_up_until_now().lines().for_each(|line| {
            if is_id {
                // we have found the id, the next one will be the thinking
                // parse this line to a i64
                let id = line.parse::<i64>().unwrap();
                ids.push(id);
                is_id = false;
            } else if is_thinking {
                thinkings.push(line.to_owned());
            } else if line == "<id>" {
                // we have found a line, the next one will be the id
                is_id = true;
            } else if line == "<thinking>" {
                // we have found the thinking, the next one will be the thinking
                is_thinking = true;
            }
        });
        // now we want to zip together both the thinking and the ids together
        let mut zipped = vec![];
        for (id, thinking) in ids.into_iter().zip(thinkings.into_iter()) {
            zipped.push((id, thinking));
        }
        // we want to reply here with the snippets which need to be changed (just the line numbers
        // and the thinking behind it)
        let mut code_to_edit = CodeToEditToolOutput::new();
        zipped.into_iter().for_each(|(idx, thinking)| {
            if let Some((start_line, end_line)) = id_to_spans.get(&idx) {
                code_to_edit.add_snippet(
                    start_line.clone() as i64,
                    end_line.clone() as i64,
                    thinking,
                );
            }
        });
        Ok(ToolOutput::code_snippets_to_edit(code_to_edit))
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
