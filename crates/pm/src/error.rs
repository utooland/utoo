//! Stable CLI error categories and rendering metadata.

use std::error::Error;
use std::fmt;

use schemars::JsonSchema;
use serde::Serialize;

use crate::model::cli_output::{ErrorDetails, PartialResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Transient,
    Usage,
    Auth,
    NotFound,
    RateLimited,
    Precondition,
    Local,
}

impl ErrorKind {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Transient => 1,
            Self::Usage => 2,
            Self::Auth => 3,
            Self::NotFound => 4,
            Self::RateLimited => 5,
            Self::Precondition => 7,
            Self::Local => 11,
        }
    }

    pub const fn from_http_status(status: u16) -> Self {
        match status {
            401 | 403 => Self::Auth,
            404 => Self::NotFound,
            409 => Self::Precondition,
            429 => Self::RateLimited,
            500..=599 => Self::Transient,
            _ => Self::Local,
        }
    }
}

#[derive(Debug)]
pub struct CliError {
    kind: ErrorKind,
    code: String,
    message: String,
    suggestion: Option<String>,
    partial_result: Option<PartialResult>,
    details: Option<ErrorDetails>,
}

impl CliError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: "operation_failed".to_string(),
            message: message.into(),
            suggestion: None,
            partial_result: None,
            details: None,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message).with_code("invalid_arguments")
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message).with_code("not_found")
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_partial_result(mut self, partial_result: PartialResult) -> Self {
        self.partial_result = Some(partial_result);
        self
    }

    pub fn with_details(mut self, details: ErrorDetails) -> Self {
        self.details = Some(details);
        self
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }

    pub const fn partial_result(&self) -> Option<&PartialResult> {
        self.partial_result.as_ref()
    }

    pub const fn details(&self) -> Option<&ErrorDetails> {
        self.details.as_ref()
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, "\n  suggestion: {suggestion}")?;
        }
        Ok(())
    }
}

impl Error for CliError {}

pub fn classify(error: &anyhow::Error) -> ErrorKind {
    if let Some(error) = error.downcast_ref::<CliError>() {
        return error.kind();
    }

    for source in error.chain() {
        if let Some(error) = source.downcast_ref::<reqwest::Error>() {
            if error.is_timeout() || error.is_connect() {
                return ErrorKind::Transient;
            }
            if let Some(status) = error.status() {
                return ErrorKind::from_http_status(status.as_u16());
            }
        }
    }

    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("not logged in")
        || message.contains("authentication failed")
        || message.contains("unauthorized")
        || message.contains("http 401")
        || message.contains("http 403")
    {
        ErrorKind::Auth
    } else if message.contains("rate limit") || message.contains("http 429") {
        ErrorKind::RateLimited
    } else if message.contains("not found")
        || message.contains("does not exist")
        || message.contains("http 404")
    {
        ErrorKind::NotFound
    } else if message.contains("already exists") || message.contains("precondition") {
        ErrorKind::Precondition
    } else if message.contains("timed out")
        || message.contains("connection refused")
        || message.contains("failed to send")
        || message.contains("http 5")
    {
        ErrorKind::Transient
    } else {
        ErrorKind::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_registry_statuses() {
        assert_eq!(
            classify(&anyhow::anyhow!("HTTP 404: package")),
            ErrorKind::NotFound
        );
        assert_eq!(
            classify(&anyhow::anyhow!("HTTP 401: registry")),
            ErrorKind::Auth
        );
        assert_eq!(
            classify(&anyhow::anyhow!("HTTP 429: registry")),
            ErrorKind::RateLimited
        );
        assert_eq!(
            classify(&anyhow::anyhow!("HTTP 503: registry")),
            ErrorKind::Transient
        );
    }
}
