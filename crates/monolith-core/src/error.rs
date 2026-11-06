use monolith_rules::RuleError;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldError {
    pub field: String,
    pub code: &'static str,
    pub message: String,
}

impl FieldError {
    pub fn new(field: &str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            field: field.to_owned(),
            code,
            message: message.into(),
        }
    }
}

/// Maps 1:1 onto the problem `type` codes in `api/jsonschema/error.json`.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    BadRequest(String),
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(String),
    #[error("server busy, retry later")]
    Busy,
    #[error("internal error: {0}")]
    Internal(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

impl From<monolith_store::Error> for CoreError {
    fn from(e: monolith_store::Error) -> Self {
        use monolith_store::Error as E;
        match e {
            E::Busy => CoreError::Busy,
            E::NotFound => CoreError::NotFound("record"),
            E::Constraint(m) => CoreError::Conflict(m),
            E::Invalid(m) => CoreError::BadRequest(m.into()),
            E::Closed | E::Engine(_) => CoreError::Internal(e.to_string()),
        }
    }
}

impl From<RuleError> for CoreError {
    fn from(e: RuleError) -> Self {
        CoreError::BadRequest(e.to_string())
    }
}
