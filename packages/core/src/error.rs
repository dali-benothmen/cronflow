//! Error types for the Node-Cronflow Core Engine

use std::{fmt, option, convert, write};
use thiserror::Error;

/// Core engine error types
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid workflow definition: {0}")]
    InvalidWorkflow(String),

    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("Step execution failed: {0}")]
    StepExecution(String),

    #[error("State management error: {0}")]
    State(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for core operations
pub type CoreResult<T> = Result<T, CoreError>; 