use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use async_trait::async_trait;
use llm_client::clients::types::LLMClientMessageImage;
use logging::new_client;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RequestScreenshotInputPartial {}

impl RequestScreenshotInputPartial {
    pub fn new() -> Self {
        Self {}
    }

    pub fn to_string(&self) -> String {
        "<request_screenshot>
</request_screenshot>"
            .to_owned()
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "request_screenshot",
            "description": "Request a screenshot of the web application, running in the browser. This tool captures the visual state of the application and returns it as an image.",
            "input_schema": {
                "type": "object",
                "properties": {},
                "required": [],
            },
        })
    }
}

pub struct RequestScreenshot {
    client: reqwest_middleware::ClientWithMiddleware,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RequestScreenshotInput {
    editor_url: String,
}
impl RequestScreenshotInput {
    pub fn new(editor_url: String) -> Self {
        Self { editor_url }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestScreenshotOutput {
    #[serde(rename = "type")]
    r#type: String,
    media_type: String,
    data: String,
}

impl RequestScreenshotOutput {
    pub fn new(r#type: String, media_type: String, data: String) -> Self {
        Self {
            r#type,
            media_type,
            data,
        }
    }

    pub fn to_llm_image(&self) -> LLMClientMessageImage {
        LLMClientMessageImage::new(
            self.r#type.to_owned(),
            self.media_type.to_owned(),
            self.data.to_owned(),
        )
    }
}

impl RequestScreenshot {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for RequestScreenshot {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.screenshot_request()?;
        let editor_endpoint = context.editor_url.to_owned() + "/devtools_screenshot";
        let response = self
            .client
            .get(editor_endpoint)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: RequestScreenshotOutput = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        Ok(ToolOutput::RequestScreenshot(response))
    }

    fn tool_description(&self) -> String {
        format!(
            r#"### request_screenshot
Request a screenshot of the web application, running in the browser.
Use this when you need to capture the current visual state of the application for analysis or verification.
The screenshot will be taken of the entire visible viewport and returned as an image.
This is particularly useful for:
- Verifying UI changes
- Documenting the application state
- Analyzing visual bugs or layout issues
- Capturing the result of UI interactions"#
        )
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
No parameters required - the tool will automatically capture the current browser viewport.

Usage:
<request_screenshot>
</request_screenshot>"#
        )
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
