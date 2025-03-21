use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};
use serde::{Deserialize, Serialize};
use serde_xml_rs::from_str;

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::file::{
        file_finder::{ImportantFilesFinder, ImportantFilesFinderQuery},
        important::FileImportantResponse,
        types::{FileImportantError, SerdeError},
    },
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "files")]
pub struct FileImportantReply {
    #[serde(rename = "file", default)]
    files: Vec<FileThinking>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileThinking {
    path: String,
    thinking: String,
}

impl FileThinking {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}

impl FileImportantReply {
    pub fn parse_response(response: &str) -> Result<Self, FileImportantError> {
        if response.is_empty() {
            return Err(FileImportantError::EmptyResponse);
        }

        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<reply>"))
            .skip(1)
            .take_while(|line| !line.contains("</reply>"))
            .map(|line| line.to_owned())
            .collect::<Vec<String>>();

        let line_string = lines.join("\n");

        match from_str::<FileImportantReply>(&line_string) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                eprintln!("parsing error: {:?}", e);
                Err(FileImportantError::SerdeError(SerdeError::new(
                    e,
                    line_string.to_owned(),
                )))
            }
        }
    }

    pub fn get_paths(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|file| file.path().to_string())
            .collect()
    }

    pub fn files(&self) -> &Vec<FileThinking> {
        &self.files
    }

    fn convert_path_for_os<P: AsRef<Path>>(path: P) -> PathBuf {
        let path = path.as_ref();

        match std::env::consts::OS {
            "windows" => PathBuf::from(path.to_string_lossy().replace('/', "\\")),
            _ => path.to_path_buf(),
        }
    }

    pub fn prepend_root_dir(&self, root: &Path) -> Self {
        let new_files: Vec<FileThinking> = self
            .files
            .iter()
            .map(|file| {
                let file_path = FileImportantReply::convert_path_for_os(&file.path);
                let os_adapted_root = FileImportantReply::convert_path_for_os(root);

                let new_path = os_adapted_root.join(file_path);
                FileThinking {
                    path: new_path.to_string_lossy().into_owned(),
                    thinking: file.thinking.clone(),
                }
            })
            .collect();

        FileImportantReply { files: new_files }
    }

    pub fn to_file_important_response(self) -> FileImportantResponse {
        let paths = self.get_paths();
        FileImportantResponse::new(paths)
    }
}

pub struct AnthropicFileFinder {
    llm_client: Arc<LLMBroker>,
    _fail_over_llm: LLMProperties,
}

impl AnthropicFileFinder {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            _fail_over_llm: fail_over_llm,
        }
    }

    fn system_message_for_file_important(
        &self,
        _file_important_request: &ImportantFilesFinderQuery,
    ) -> String {
        format!(
            r#"Consider the provided user query.
        
Select files from the provided repository tree that may be relevant to solving the user query.

Do not hallucinate files that do not appear in the tree.

You must return at least 1 file, but no more than 10, in order of relevance.
            
Respond in the following XML format:

<reply>
<files>
<file>
<path>
path/to/file1
</path>
<thinking>
</thinking>
</file>
</files>
</reply>

Notice how each xml tag ends with a new line, follow this format strictly.

Response:

<files>
"#,
        )
    }

    fn user_message_for_file_important(
        &self,
        file_important_request: &ImportantFilesFinderQuery,
    ) -> String {
        format!(
            "User query: {}\n\nTree:\n{}",
            file_important_request.user_query(),
            file_important_request.tree()
        )
    }
}

#[async_trait]
impl ImportantFilesFinder for AnthropicFileFinder {
    async fn find_important_files(
        &self,
        request: ImportantFilesFinderQuery,
    ) -> Result<FileImportantResponse, FileImportantError> {
        let root_request_id = request.root_request_id().to_owned();
        let model = request.llm().clone();
        let provider = request.provider().clone();
        let api_keys = request.api_keys().clone();
        let system_message =
            LLMClientMessage::system(self.system_message_for_file_important(&request));
        let user_message = LLMClientMessage::user(self.user_message_for_file_important(&request));
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
                    ("event_type".to_owned(), "important_file_finder".to_owned()),
                    ("root_id".to_owned(), root_request_id),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;

        println!("file_important_broker::time_take({:?})", start.elapsed());

        let parsed_response = FileImportantReply::parse_response(response.answer_up_until_now());

        match parsed_response {
            Ok(parsed_response) => Ok(parsed_response
                .prepend_root_dir(Path::new(request.repo_name()))
                .to_file_important_response()),
            Err(e) => Err(e),
        }
    }
}
