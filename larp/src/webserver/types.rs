use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::borrow::Cow;

pub(crate) trait ApiResponse: erased_serde::Serialize {}
erased_serde::serialize_trait_object!(ApiResponse);

/// Every endpoint exposes a Response type
#[derive(serde::Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub(crate) enum Response<'a> {
    Ok(Box<dyn erased_serde::Serialize + Send + Sync + 'static>),
    Error(EndpointError<'a>),
}

impl<T: ApiResponse + Send + Sync + 'static> From<T> for Response<'static> {
    fn from(value: T) -> Self {
        Self::Ok(Box::new(value))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct Error {
    status: StatusCode,
    body: EndpointError<'static>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.body.message)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(Response::from(self.body))).into_response()
    }
}

impl Error {
    pub fn internal<S: std::fmt::Display>(message: S) -> Self {
        Error {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: EndpointError {
                kind: ErrorKind::Internal,
                message: message.to_string().into(),
            },
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::internal(value)
    }
}

/// The response upon encountering an error
#[derive(serde::Serialize, PartialEq, Eq, Debug)]
pub struct EndpointError<'a> {
    /// The kind of this error
    kind: ErrorKind,

    /// A context aware message describing the error
    message: Cow<'a, str>,
}

impl<'a> From<EndpointError<'a>> for Response<'a> {
    fn from(value: EndpointError<'a>) -> Self {
        Self::Error(value)
    }
}

/// The kind of an error
#[allow(unused)]
#[derive(serde::Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorKind {
    User,
    Unknown,
    NotFound,
    Configuration,
    UpstreamService,
    Internal,

    #[doc(hidden)]
    Custom,
}

pub(crate) fn json<'a, T>(val: T) -> Json<Response<'a>>
where
    Response<'a>: From<T>,
{
    Json(Response::from(val))
}
