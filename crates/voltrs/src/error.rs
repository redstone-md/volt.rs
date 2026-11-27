use std::collections::BTreeMap;
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
    field_errors: Option<FieldErrors>,
}

/// Validation messages keyed by input field, sent to clients as `fieldErrors`.
pub type FieldErrors = BTreeMap<String, Vec<String>>;

impl Error {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        let public = status.is_client_error();
        Self { status, kind: ErrorKind::Status, message: message.into(), public, field_errors: None }
    }

    /// 422 with per-field messages, surfaced as `ActionError.fieldErrors`. Repeated fields collect messages.
    pub fn validation<K: Into<String>, V: Into<String>>(errors: impl IntoIterator<Item = (K, V)>) -> Self {
        let mut fields = FieldErrors::new();
        for (field, message) in errors {
            fields.entry(field.into()).or_default().push(message.into());
        }
        Self { field_errors: Some(fields), ..Self::new(StatusCode::UNPROCESSABLE_ENTITY, "Validation failed") }
    }

    pub fn field_errors(&self) -> Option<&FieldErrors> {
        self.field_errors.as_ref()
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
        if let Some(fields) = &self.field_errors {
            value["fieldErrors"] = serde_json::json!(fields);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_field_errors() {
        let error = Error::validation([("name", "required"), ("name", "too short"), ("age", "too young")]);
        assert_eq!(error.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = error.to_json(false);
        assert_eq!(json["message"], "Validation failed");
        assert_eq!(json["fieldErrors"], serde_json::json!({ "age": ["too young"], "name": ["required", "too short"] }));
        assert!(Error::bad_request("x").to_json(false).get("fieldErrors").is_none());
    }
}
