//! Contains the test tool

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use async_trait::async_trait;
use logging::new_client;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SWEBenchTestRequest {
    swe_bench_test_endpoint: String,
    timeout: Option<Duration>,
    retry_count: Option<u32>,
}

impl SWEBenchTestRequest {
    pub fn new(swe_bench_test_endpoint: String) -> Self {
        Self {
            swe_bench_test_endpoint,
            timeout: Some(Duration::from_secs(30)),
            retry_count: Some(3),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_retry_count(mut self, retry_count: u32) -> Self {
        self.retry_count = Some(retry_count);
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SWEBenchTestResponse {
    test_output: Option<String>,
    passed: bool,
    execution_time: Option<Duration>,
    error_message: Option<String>,
    test_name: Option<String>,
}

impl SWEBenchTestResponse {
    pub fn passed(&self) -> bool {
        self.passed
    }

    pub fn test_output(&self) -> Option<String> {
        self.test_output.clone()
    }

    pub fn execution_time(&self) -> Option<Duration> {
        self.execution_time
    }

    pub fn error_message(&self) -> Option<String> {
        self.error_message.clone()
    }

    pub fn test_name(&self) -> Option<String> {
        self.test_name.clone()
    }
}

pub struct SWEBenchTestTool {
    client: reqwest_middleware::ClientWithMiddleware,
    default_timeout: Duration,
    default_retry_count: u32,
}

impl SWEBenchTestTool {
    pub fn new() -> Self {
        Self {
            client: new_client(),
            default_timeout: Duration::from_secs(30),
            default_retry_count: 3,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    pub fn with_retry_count(mut self, retry_count: u32) -> Self {
        self.default_retry_count = retry_count;
        self
    }

    async fn execute_test_with_retry(
        &self,
        endpoint: &str,
        request_body: &str,
        timeout: Duration,
        retry_count: u32,
    ) -> Result<SWEBenchTestResponse, ToolError> {
        let mut last_error = None;

        for attempt in 0..retry_count {
            info!("Executing test attempt {}/{}", attempt + 1, retry_count);

            match self.execute_test(endpoint, request_body, timeout).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!("Test execution attempt {} failed: {:?}", attempt + 1, e);
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }

        Err(last_error.unwrap_or(ToolError::SWEBenchTestEndpointError))
    }

    async fn execute_test(
        &self,
        endpoint: &str,
        request_body: &str,
        timeout: Duration,
    ) -> Result<SWEBenchTestResponse, ToolError> {
        let start_time = std::time::Instant::now();

        let response = self
            .client
            .post(endpoint)
            .timeout(timeout)
            .body(request_body.to_owned())
            .send()
            .await
            .map_err(|e| {
                error!("Failed to send test request: {:?}", e);
                ToolError::SWEBenchTestEndpointError
            })?;

        let execution_time = start_time.elapsed();

        let response: SWEBenchTestResponse = response.json().await.map_err(|e| {
            error!("Failed to parse test response: {:?}", e);
            ToolError::SerdeConversionFailed
        })?;

        Ok(SWEBenchTestResponse {
            execution_time: Some(execution_time),
            ..response
        })
    }
}

#[async_trait]
impl Tool for SWEBenchTestTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.swe_bench_test()?;
        let timeout = context.timeout.unwrap_or(self.default_timeout);
        let retry_count = context.retry_count.unwrap_or(self.default_retry_count);

        info!(
            "Executing test with timeout {:?} and {} retries",
            timeout, retry_count
        );

        let response = self
            .execute_test_with_retry(
                &context.swe_bench_test_endpoint,
                &serde_json::to_string(&context).map_err(|e| {
                    error!("Failed to serialize test request: {:?}", e);
                    ToolError::SerdeConversionFailed
                })?,
                timeout,
                retry_count,
            )
            .await?;

        info!(
            "Test execution completed. Passed: {}, Execution time: {:?}",
            response.passed, response.execution_time
        );

        Ok(ToolOutput::swe_bench_test_output(response))
    }

    fn tool_description(&self) -> String {
        "A tool for executing and analyzing SWE-bench tests with retry capabilities and detailed reporting".to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"{
            "swe_bench_test_endpoint": "string",
            "timeout": "optional duration in seconds",
            "retry_count": "optional number of retries"
        }"#
        .to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "Test execution success".to_owned(),
            "Execution time within limits".to_owned(),
            "Proper error handling".to_owned(),
            "Detailed test output".to_owned(),
        ]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new("test_success", 1.0),
            ToolRewardScale::new("execution_time", 0.5),
            ToolRewardScale::new("error_handling", 0.3),
        ]
    }
}
