use crate::{
    agentic::tool::{errors::ToolError, input::ToolInput, output::ToolOutput, r#type::Tool},
    chunking::text_document::{Position, Range},
};
use async_trait::async_trait;
use gix::bstr::ByteSlice;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenFileRequest {
    fs_file_path: String,
    editor_url: String,
}

impl OpenFileRequest {
    pub fn new(fs_file_path: String, editor_url: String) -> Self {
        Self {
            fs_file_path,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenFileResponse {
    fs_file_path: String,
    file_contents: String,
    exists: bool,
    // TODO(skcd): This might break
    language: String,
}

impl OpenFileResponse {
    pub fn new(
        fs_file_path: String,
        file_contents: String,
        exists: bool,
        language: String,
    ) -> Self {
        Self {
            fs_file_path,
            file_contents,
            exists,
            language,
        }
    }

    pub fn contents(self) -> String {
        self.file_contents
    }

    pub fn contents_ref(&self) -> &str {
        &self.file_contents
    }

    pub fn fs_file_path(&self) -> &str {
        self.fs_file_path.as_str()
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn content_in_range(&self, range: &Range) -> Option<String> {
        if !self.exists {
            None
        } else {
            // we are calling it content in range and thats what it is exactly
            // if we do not have anything at the start of it, we leave it
            // but I am not sure if this is the best thing to do and do not know
            // of cases where we will be looking at exact text and not start_line..end_line
            // TODO(skcd): Make this more accurate when lines match up
            self.file_contents
                .lines()
                .enumerate()
                .filter(|(i, _line)| i >= &range.start_line())
                .take_while(|(i, _line)| i <= &range.end_line())
                .map(|(_i, line)| line.to_string())
                .collect::<Vec<String>>()
                .join("\n")
                .into()
        }
    }

    /// Grabs the content in a range using the byte offset
    pub fn content_in_ranges_exact(&self, range: &Range) -> Option<String> {
        if !self.exists {
            None
        } else {
            let bytes_len = self.file_contents.as_bytes().len();
            if range.start_byte() < bytes_len && range.end_byte() < bytes_len {
                self.file_contents.as_bytes()[range.start_byte()..range.end_byte()]
                    .to_str()
                    .map(|s| s.to_owned())
                    .ok()
            } else {
                None
            }
        }
    }

    /// Length of the file contents
    pub fn file_content_len(&self) -> usize {
        self.file_contents
            .lines()
            .into_iter()
            .collect::<Vec<_>>()
            .len()
    }

    pub fn full_range(&self) -> Range {
        let mut file_content_len = self.file_content_len();
        if file_content_len != 0 {
            file_content_len = file_content_len - 1;
        }
        Range::new(
            Position::new(0, 0, 0),
            Position::new(file_content_len, 0, 0),
        )
    }
}

pub struct LSPOpenFile {
    client: reqwest::Client,
}

impl LSPOpenFile {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for LSPOpenFile {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        // we want to create a new file open request over here
        let context = input.is_file_open()?;
        // now we send it over to the editor
        let editor_endpoint = context.editor_url.to_owned() + "/file_open";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: OpenFileResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        Ok(ToolOutput::FileOpen(response))
    }
}
