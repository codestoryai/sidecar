#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FileImportantResponse {
    file_paths: Vec<String>,
}

impl FileImportantResponse {
    pub fn new(file_paths: Vec<String>) -> Self {
        Self { file_paths }
    }

    pub fn file_paths(&self) -> &[String] {
        self.file_paths.as_slice()
    }
}
