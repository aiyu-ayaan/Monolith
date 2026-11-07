use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Per-field validation error details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldProblem {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl FieldProblem {
    pub fn new(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// RFC 9457 Problem Details object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FieldProblem>>,
}

impl Problem {
    pub fn new(problem_type: impl Into<String>, title: impl Into<String>, status: u16) -> Self {
        Self {
            problem_type: problem_type.into(),
            title: title.into(),
            status,
            detail: None,
            request_id: None,
            errors: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn with_errors(mut self, errors: Vec<FieldProblem>) -> Self {
        self.errors = Some(errors);
        self
    }

    /// Finds a field error message by field name, if any.
    pub fn get_field_error(&self, field: &str) -> Option<&str> {
        self.errors
            .as_deref()?
            .iter()
            .find(|e| e.field == field)
            .map(|e| e.message.as_str())
    }
}

/// Errors returned by the Monolith client SDK.
#[derive(Debug, Error)]
pub enum Error {
    #[error("API error ({status}): {message}")]
    Api {
        status: u16,
        message: String,
        problem: Option<Box<Problem>>,
    },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl Error {
    pub fn api(status: u16, problem: Problem) -> Self {
        let message = problem
            .detail
            .clone()
            .unwrap_or_else(|| problem.title.clone());
        Error::Api {
            status,
            message,
            problem: Some(Box::new(problem)),
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Error::Config(msg.into())
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn problem(&self) -> Option<&Problem> {
        match self {
            Error::Api { problem, .. } => problem.as_deref(),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
