use chrono::NaiveDateTime;
use reqwest::header::CONTENT_TYPE;
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::time::Duration;

extern crate serde_json;

const API_ENDPOINT: &str = "https://app.posthog.com/capture/";
const TIMEOUT: &Duration = &Duration::from_millis(800); // This should be specified by the user

pub fn client<C: Into<ClientOptions>>(options: C, user_id: String) -> PosthogClient {
    let client = HttpClient::builder()
        .timeout(TIMEOUT.clone())
        .build()
        .unwrap(); // Unwrap here is as safe as `HttpClient::new`
    PosthogClient {
        options: options.into(),
        client,
        user_id,
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connection(msg) => write!(f, "Connection Error: {}", msg),
            Error::Serialization(msg) => write!(f, "Serialization Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum Error {
    Connection(String),
    Serialization(String),
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    api_endpoint: String,
    api_key: String,
}

impl From<&str> for ClientOptions {
    fn from(api_key: &str) -> Self {
        ClientOptions {
            api_endpoint: API_ENDPOINT.to_string(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PosthogClient {
    options: ClientOptions,
    client: HttpClient,
    user_id: String,
}

impl PosthogClient {
    pub async fn capture(&self, event: PosthogEvent) -> Result<(), Error> {
        let inner_event =
            InnerEvent::new(event, self.options.api_key.clone(), self.user_id.to_owned());
        let _res = self
            .client
            .post(self.options.api_endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&inner_event).expect("unwrap here is safe"))
            .send()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(())
    }

    pub async fn capture_batch(&self, events: Vec<PosthogEvent>) -> Result<(), Error> {
        for event in events {
            self.capture(event).await?;
        }
        Ok(())
    }
}

// This exists so that the client doesn't have to specify the API key over and over
#[derive(Serialize)]
struct InnerEvent {
    api_key: String,
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

impl InnerEvent {
    fn new(mut event: PosthogEvent, api_key: String, user_id: String) -> Self {
        event.properties.distinct_id = user_id;
        Self {
            api_key,
            event: event.event,
            properties: event.properties,
            timestamp: event.timestamp,
        }
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct PosthogEvent {
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Properties {
    distinct_id: String,
    props: HashMap<String, serde_json::Value>,
}

impl Properties {
    fn new<S: Into<String>>(distinct_id: S) -> Self {
        Self {
            distinct_id: distinct_id.into(),
            props: Default::default(),
        }
    }
}

impl PosthogEvent {
    pub fn new<S: Into<String>>(event: S) -> Self {
        Self {
            event: event.into(),
            properties: Properties::new("codestory"),
            timestamp: None,
        }
    }

    /// Errors if `prop` fails to serialize
    pub fn insert_prop<K: Into<String>, P: Serialize>(
        &mut self,
        key: K,
        prop: P,
    ) -> Result<(), Error> {
        let as_json =
            serde_json::to_value(prop).map_err(|e| Error::Serialization(e.to_string()))?;
        let _ = self.properties.props.insert(key.into(), as_json);
        Ok(())
    }
}

pub fn posthog_client(user_id: &str) -> PosthogClient {
    client(
        "phc_dKVAmUNwlfHYSIAH1kgnvq3iEw7ovE5YYvGhTyeRlaB",
        user_id.to_owned(),
    )
}
