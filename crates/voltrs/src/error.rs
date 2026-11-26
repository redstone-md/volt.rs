use std::fmt;

use http::StatusCode;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Status,
    NotFound,
    Redirect(String),
}

/// Error returned by actions, loaders and handlers. Any `std::error::Error` converts into a 500.
#[derive(Debug, Clone)]
pub struct Error {
    status: StatusCode,
    kind: ErrorKind,
    message: String,
    public: bool,
}

impl Error {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        let public = status.is_client_error();
        Self { status, kind: ErrorKind::Status, message: message.into(), public }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn not_found() -> Self {
        Self { kind: ErrorKind::NotFound, ..Self::new(StatusCode::NOT_FOUND, "Not Found") }
    }

    pub fn redirect(location: impl Into<String>) -> Self {
        Self::redirect_with(StatusCode::SEE_OTHER, location)
    }

    pub fn permanent_redirect(location: impl Into<String>) -> Self {
        Self::redirect_with(StatusCode::PERMANENT_REDIRECT, location)
    }

    fn redirect_with(status: StatusCode, location: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Redirect(location.into()), public: true, ..Self::new(status, "Redirect") }
    }

    /// Marks the message as safe to show to clients in production.
    pub fn public(mut self) -> Self {
        self.public = true;
        self
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn client_message(&self, dev: bool) -> &str {
        if self.public || dev { &self.message } else { "Internal Server Error" }
    }

    pub fn to_json(&self, dev: bool) -> serde_json::Value {
        let mut value = serde_json::json!({ "status": self.status.as_u16(), "message": self.client_message(dev) });
        match &self.kind {
            ErrorKind::Redirect(location) => value["redirect"] = location.clone().into(),
            ErrorKind::NotFound => value["notFound"] = true.into(),
            ErrorKind::Status => {}
        }
        value
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.message)
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self::internal(error.to_string())
    }
}

pub fn redirect(location: impl Into<String>) -> Error {
    Error::redirect(location)
}

pub fn not_found() -> Error {
    Error::not_found()
}
