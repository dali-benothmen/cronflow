use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::models::{WorkflowRun, StepResult};
use crate::error::CoreError;

/// Context object passed to Bun.js for job execution
/// Contains all necessary information for step execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    /// Unique identifier for this workflow run
    pub run_id: String,
    /// Unique identifier for the workflow
    pub workflow_id: String,
    /// Current step being executed
    pub step_name: String,
    /// Input payload for the workflow
    pub payload: serde_json::Value,
    /// Results from completed steps
    pub steps: HashMap<String, StepResult>,
    /// Available services for this execution
    pub services: HashMap<String, serde_json::Value>,
    /// Current workflow run state
    pub run: WorkflowRun,
    /// Metadata about the execution
    pub metadata: ContextMetadata,
}

/// Metadata about the context execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetadata {
    /// Timestamp when context was created
    pub created_at: String,
    /// Current step index
    pub step_index: usize,
    /// Total number of steps
    pub total_steps: usize,
    /// Execution timeout in seconds
    pub timeout: Option<u64>,
    /// Retry count for current step
    pub retry_count: u32,
    /// Maximum retries allowed
    pub max_retries: u32,
}

impl Context {
    /// Create a new context from job data
    pub fn new(
        run_id: String,
        workflow_id: String,
        step_name: String,
        payload: serde_json::Value,
        run: WorkflowRun,
        completed_steps: Vec<StepResult>,
    ) -> Result<Self, CoreError> {
        // Validate required fields
        if run_id.is_empty() {
            return Err(CoreError::Validation("run_id cannot be empty".to_string()));
        }
        if workflow_id.is_empty() {
            return Err(CoreError::Validation("workflow_id cannot be empty".to_string()));
        }
        if step_name.is_empty() {
            return Err(CoreError::Validation("step_name cannot be empty".to_string()));
        }

        let mut steps = HashMap::new();
        for step_result in completed_steps {
            steps.insert(step_result.step_id.clone(), step_result);
        }

        let metadata = ContextMetadata {
            created_at: chrono::Utc::now().to_rfc3339(),
            step_index: 0, // Will be updated by caller
            total_steps: 0, // Will be updated by caller
            timeout: None,
            retry_count: 0,
            max_retries: 3,
        };

        Ok(Context {
            run_id,
            workflow_id,
            step_name,
            payload,
            steps,
            services: HashMap::new(),
            run,
            metadata,
        })
    }

    /// Add a service to the context
    pub fn add_service(&mut self, name: String, config: serde_json::Value) {
        self.services.insert(name, config);
    }

    /// Get a completed step result
    pub fn get_step_result(&self, step_name: &str) -> Option<&StepResult> {
        self.steps.get(step_name)
    }

    /// Get all completed step results
    pub fn get_completed_steps(&self) -> Vec<&StepResult> {
        self.steps.values().collect()
    }

    /// Update step metadata
    pub fn update_step_metadata(&mut self, step_index: usize, total_steps: usize) {
        self.metadata.step_index = step_index;
        self.metadata.total_steps = total_steps;
    }

    /// Set timeout for current execution
    pub fn set_timeout(&mut self, timeout_seconds: u64) {
        self.metadata.timeout = Some(timeout_seconds);
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) -> bool {
        self.metadata.retry_count += 1;
        self.metadata.retry_count <= self.metadata.max_retries
    }

    /// Reset retry count
    pub fn reset_retry_count(&mut self) {
        self.metadata.retry_count = 0;
    }

    /// Validate the context
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.run_id.is_empty() {
            return Err(CoreError::Validation("run_id cannot be empty".to_string()));
        }
        if self.workflow_id.is_empty() {
            return Err(CoreError::Validation("workflow_id cannot be empty".to_string()));
        }
        if self.step_name.is_empty() {
            return Err(CoreError::Validation("step_name cannot be empty".to_string()));
        }
        Ok(())
    }

    /// Serialize context to JSON string
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::Serialization(e))
    }

    /// Create context from JSON string
    pub fn from_json(json: &str) -> Result<Self, CoreError> {
        serde_json::from_str(json)
            .map_err(|e| CoreError::Serialization(e))
    }

    /// Get context as a JSON value
    pub fn to_json_value(&self) -> Result<serde_json::Value, CoreError> {
        serde_json::to_value(self)
            .map_err(|e| CoreError::Serialization(e))
    }
}

impl Default for ContextMetadata {
    fn default() -> Self {
        Self {
            created_at: chrono::Utc::now().to_rfc3339(),
            step_index: 0,
            total_steps: 0,
            timeout: None,
            retry_count: 0,
            max_retries: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{WorkflowRun, RunStatus, StepResult, StepStatus};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_context_creation() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({"test": "data"}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        let context = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "test-step".to_string(),
            serde_json::json!({"input": "value"}),
            run,
            vec![],
        ).unwrap();

        assert_eq!(context.run_id, "run-123");
        assert_eq!(context.workflow_id, "workflow-123");
        assert_eq!(context.step_name, "test-step");
        assert!(context.validate().is_ok());
    }

    #[test]
    fn test_context_with_completed_steps() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        let completed_step = StepResult {
            step_id: "previous-step".to_string(),
            status: StepStatus::Completed,
            output: Some(serde_json::json!({"output": "success"})),
            error: None,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(1000),
        };

        let context = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "current-step".to_string(),
            serde_json::json!({}),
            run,
            vec![completed_step],
        ).unwrap();

        assert_eq!(context.get_completed_steps().len(), 1);
        assert!(context.get_step_result("previous-step").is_some());
        assert!(context.get_step_result("non-existent").is_none());
    }

    #[test]
    fn test_context_validation() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        // Test empty run_id
        let result = Context::new(
            "".to_string(),
            "workflow-123".to_string(),
            "test-step".to_string(),
            serde_json::json!({}),
            run.clone(),
            vec![],
        );
        assert!(result.is_err());

        // Test empty workflow_id
        let result = Context::new(
            "run-123".to_string(),
            "".to_string(),
            "test-step".to_string(),
            serde_json::json!({}),
            run.clone(),
            vec![],
        );
        assert!(result.is_err());

        // Test empty step_name
        let result = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "".to_string(),
            serde_json::json!({}),
            run,
            vec![],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_context_serialization() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        let context = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "test-step".to_string(),
            serde_json::json!({"input": "value"}),
            run,
            vec![],
        ).unwrap();

        // Test JSON serialization
        let json = context.to_json().unwrap();
        let deserialized = Context::from_json(&json).unwrap();
        
        assert_eq!(context.run_id, deserialized.run_id);
        assert_eq!(context.workflow_id, deserialized.workflow_id);
        assert_eq!(context.step_name, deserialized.step_name);
    }

    #[test]
    fn test_context_metadata() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        let mut context = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "test-step".to_string(),
            serde_json::json!({}),
            run,
            vec![],
        ).unwrap();

        // Test metadata updates
        context.update_step_metadata(2, 5);
        assert_eq!(context.metadata.step_index, 2);
        assert_eq!(context.metadata.total_steps, 5);

        // Test timeout
        context.set_timeout(30);
        assert_eq!(context.metadata.timeout, Some(30));

        // Test retry logic
        assert_eq!(context.metadata.retry_count, 0);
        assert!(context.increment_retry());
        assert_eq!(context.metadata.retry_count, 1);
        assert!(context.increment_retry());
        assert!(context.increment_retry());
        assert!(!context.increment_retry()); // Should fail on 4th retry

        context.reset_retry_count();
        assert_eq!(context.metadata.retry_count, 0);
    }

    #[test]
    fn test_context_services() {
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "workflow-123".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        let mut context = Context::new(
            "run-123".to_string(),
            "workflow-123".to_string(),
            "test-step".to_string(),
            serde_json::json!({}),
            run,
            vec![],
        ).unwrap();

        // Test adding services
        context.add_service("email".to_string(), serde_json::json!({
            "smtp": "smtp.gmail.com",
            "port": 587
        }));

        context.add_service("database".to_string(), serde_json::json!({
            "url": "postgresql://localhost:5432/mydb"
        }));

        assert_eq!(context.services.len(), 2);
        assert!(context.services.contains_key("email"));
        assert!(context.services.contains_key("database"));
    }
} 