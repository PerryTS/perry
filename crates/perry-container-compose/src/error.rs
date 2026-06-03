//! Error types for perry-container-compose.
//!
//! Defines the canonical `ComposeError` enum and FFI error mapping.

use crate::backend::BackendProbeResult;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level crate error
#[derive(Debug, Error, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComposeError {
    #[error("Dependency cycle detected in services: {services:?}")]
    DependencyCycle { services: Vec<String> },

    #[error("Service '{service}' failed to start: {message}")]
    ServiceStartupFailed { service: String, message: String },

    #[error("Backend error (exit {code}): {message}")]
    BackendError { code: i32, message: String },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Parse error: {0}")]
    #[serde(serialize_with = "serialize_error", skip_deserializing)]
    ParseError(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    #[serde(serialize_with = "serialize_error", skip_deserializing)]
    JsonError(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    #[serde(serialize_with = "serialize_error", skip_deserializing)]
    IoError(#[from] std::io::Error),

    #[error("Validation error: {message}")]
    ValidationError { message: String },

    #[error("Image verification failed for '{image}': {reason}")]
    VerificationFailed { image: String, reason: String },

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("No container backend found. Probed: {probed:?}")]
    NoBackendFound { probed: Vec<BackendProbeResult> },

    #[error("Specified backend '{name}' is not available: {reason}")]
    BackendNotAvailable { name: String, reason: String },

    /// Strict-mode `up()` aborted because at least one service's spec
    /// references features the chosen backend cannot honor. The user
    /// opted into Strict via `ComposeEngine::with_enforcement(...)`;
    /// fail loud rather than silently downgrade the spec. The `details`
    /// field carries a `; `-joined summary `<service>: <field> (<reason>)`
    /// so log scrapers can extract the offending axes without parsing
    /// the trace stream.
    #[error("Backend '{backend}' cannot honor the spec for service '{service}': {details}")]
    EnforcementViolation {
        backend: String,
        service: String,
        details: String,
    },
}

fn serialize_error<S, E>(e: &E, s: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    E: std::fmt::Display,
{
    s.serialize_str(&e.to_string())
}

impl ComposeError {
    pub fn validation(msg: impl Into<String>) -> Self {
        ComposeError::ValidationError {
            message: msg.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, ComposeError>;

/// Convert a `ComposeError` to a JSON string `{ "message": "...", "code": N }`
/// suitable for passing across the FFI boundary.
pub fn compose_error_to_js(e: &ComposeError) -> String {
    let code = match e {
        ComposeError::NotFound(_) => 404,
        ComposeError::FileNotFound { .. } => 404,
        ComposeError::BackendError { code, .. } => *code,
        ComposeError::DependencyCycle { .. } => 422,
        ComposeError::ValidationError { .. } => 400,
        ComposeError::ParseError(_) => 400,
        ComposeError::JsonError(_) => 400,
        ComposeError::VerificationFailed { .. } => 403,
        ComposeError::NoBackendFound { .. } => 503,
        ComposeError::BackendNotAvailable { .. } => 503,
        _ => 500,
    };
    serde_json::json!({
        "message": e.to_string(),
        "code": code
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = ComposeError::NotFound("foo".into());
        assert_eq!(compose_error_to_js(&err).contains("\"code\":404"), true);

        let err = ComposeError::DependencyCycle {
            services: vec!["a".into()],
        };
        assert_eq!(compose_error_to_js(&err).contains("\"code\":422"), true);

        let err = ComposeError::ValidationError {
            message: "bad".into(),
        };
        assert_eq!(compose_error_to_js(&err).contains("\"code\":400"), true);

        let err = ComposeError::VerificationFailed {
            image: "img".into(),
            reason: "fail".into(),
        };
        assert_eq!(compose_error_to_js(&err).contains("\"code\":403"), true);

        let err = ComposeError::ParseError(
            serde_yaml::from_str::<serde_yaml::Value>("bad: [1,2").unwrap_err(),
        );
        assert_eq!(compose_error_to_js(&err).contains("\"code\":400"), true);
    }
}
