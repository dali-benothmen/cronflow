use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::CoreError;
use crate::models::{WorkflowDefinition, WorkflowRun, StepResult};
use crate::job::Job;

/// Context object passed to Bun.js during job execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    /// The original payload that triggered the workflow
    pub payload: serde_json::Value,
    
    /// Outputs from all previously completed steps
    pub steps: HashMap<String, StepResult>,
    
    /// Configured service instances for this workflow
    pub services: HashMap<String, serde_json::Value>,
    
    /// Run metadata
    pub run: RunMetadata,
    
    /// Persistent state shared across workflow runs
    pub state: HashMap<String, serde_json::Value>,
    
    /// Additional context data
    pub context: HashMap<String, serde_json::Value>,
    
    /// Current step information
    pub current_step: StepMetadata,
    
    /// Workflow definition
    pub workflow: WorkflowDefinition,
    
    /// Timestamp when context was created
    pub created_at: DateTime<Utc>,
}

/// Run metadata for the current workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    /// Unique run ID
    pub run_id: String,
    
    /// Workflow ID
    pub workflow_id: String,
    
    /// Run status
    pub status: String,
    
    /// When the run started
    pub started_at: DateTime<Utc>,
    
    /// When the run completed (if completed)
    pub completed_at: Option<DateTime<Utc>>,
    
    /// Run error (if failed)
    pub error: Option<String>,
    
    /// Run tags for categorization
    pub tags: HashMap<String, String>,
}

/// Current step metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepMetadata {
    /// Step ID
    pub step_id: String,
    
    /// Step name
    pub name: String,
    
    /// Step action/function name
    pub action: String,
    
    /// Step timeout in milliseconds
    pub timeout_ms: Option<u64>,
    
    /// Retry configuration
    pub retry: Option<RetryConfig>,
    
    /// Step dependencies
    pub depends_on: Vec<String>,
    
    /// When the step started
    pub started_at: DateTime<Utc>,
    
    /// Step attempt number
    pub attempt: u32,
}

/// Retry configuration for steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    
    /// Backoff delay in milliseconds
    pub backoff_ms: u64,
    
    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
    
    /// Whether to add jitter to backoff
    pub jitter: bool,
}

impl Context {
    /// Create a new context from job data
    pub fn from_job(
        job: &Job,
        workflow: &WorkflowDefinition,
        run: &WorkflowRun,
        completed_steps: &[StepResult],
        services: HashMap<String, serde_json::Value>,
    ) -> Result<Self, CoreError> {
        // Find the current step in the workflow
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == job.step_name)
            .ok_or_else(|| {
                CoreError::InvalidWorkflow(format!("Step '{}' not found in workflow", job.step_name))
            })?;

        // Build steps map from completed steps
        let mut steps = HashMap::new();
        for step_result in completed_steps {
            steps.insert(step_result.step_id.clone(), step_result.clone());
        }

        // Create run metadata
        let run_metadata = RunMetadata {
            run_id: job.run_id.clone(),
            workflow_id: job.workflow_id.clone(),
            status: run.status.as_str().to_string(),
            started_at: run.started_at,
            completed_at: run.completed_at,
            error: run.error.clone(),
            tags: HashMap::new(), // TODO: Add tags from run
        };

        // Create step metadata
        let step_metadata = StepMetadata {
            step_id: job.step_name.clone(),
            name: step.name.clone(),
            action: step.action.clone(),
            timeout_ms: step.timeout,
            retry: step.retry.as_ref().map(|r| RetryConfig {
                max_attempts: r.max_attempts,
                backoff_ms: r.backoff_ms,
                max_backoff_ms: r.backoff_ms * 10, // Use 10x backoff as max
                jitter: true,
            }),
            depends_on: step.depends_on.clone(),
            started_at: Utc::now(),
            attempt: job.metadata.attempt_count,
        };

        Ok(Self {
            payload: job.payload.clone(),
            steps,
            services,
            run: run_metadata,
            state: HashMap::new(), // TODO: Load from persistent state
            context: job.context.clone(),
            current_step: step_metadata,
            workflow: workflow.clone(),
            created_at: Utc::now(),
        })
    }

    /// Get a step result by step ID
    pub fn get_step_result(&self, step_id: &str) -> Option<&StepResult> {
        self.steps.get(step_id)
    }

    /// Get a step output value
    pub fn get_step_output(&self, step_id: &str) -> Option<&serde_json::Value> {
        self.steps.get(step_id).and_then(|result| result.output.as_ref())
    }

    /// Get a service instance
    pub fn get_service(&self, service_name: &str) -> Option<&serde_json::Value> {
        self.services.get(service_name)
    }

    /// Get a state value
    pub fn get_state(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.get(key)
    }

    /// Set a state value
    pub fn set_state(&mut self, key: String, value: serde_json::Value) {
        self.state.insert(key, value);
    }

    /// Get a context value
    pub fn get_context(&self, key: &str) -> Option<&serde_json::Value> {
        self.context.get(key)
    }

    /// Set a context value
    pub fn set_context(&mut self, key: String, value: serde_json::Value) {
        self.context.insert(key, value);
    }

    /// Get a tag from the run
    pub fn get_run_tag(&self, key: &str) -> Option<&String> {
        self.run.tags.get(key)
    }

    /// Set a tag on the run
    pub fn set_run_tag(&mut self, key: String, value: String) {
        self.run.tags.insert(key, value);
    }

    /// Check if a step has completed
    pub fn is_step_completed(&self, step_id: &str) -> bool {
        self.steps.contains_key(step_id)
    }

    /// Get all completed step IDs
    pub fn get_completed_step_ids(&self) -> Vec<String> {
        self.steps.keys().cloned().collect()
    }

    /// Validate the context
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.run.run_id.is_empty() {
            return Err(CoreError::Validation("Run ID cannot be empty".to_string()));
        }

        if self.run.workflow_id.is_empty() {
            return Err(CoreError::Validation("Workflow ID cannot be empty".to_string()));
        }

        if self.current_step.step_id.is_empty() {
            return Err(CoreError::Validation("Step ID cannot be empty".to_string()));
        }

        Ok(())
    }

    /// Serialize context to JSON
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string(self)
            .map_err(|e| CoreError::Serialization(e))
    }

    /// Deserialize context from JSON
    pub fn from_json(json: &str) -> Result<Self, CoreError> {
        serde_json::from_str(json)
            .map_err(|e| CoreError::Serialization(e))
    }

    /// Create a context for testing
    pub fn test_context() -> Self {
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: Some("A test workflow".to_string()),
            steps: vec![
                crate::models::StepDefinition {
                    id: "step1".to_string(),
                    name: "Test Step".to_string(),
                    action: "test_action".to_string(),
                    timeout: Some(5000),
                    retry: Some(crate::models::RetryConfig {
                        max_attempts: 3,
                        backoff_ms: 1000,
                    }),
                    depends_on: vec![],
                }
            ],
            triggers: vec![
                crate::models::TriggerDefinition::Webhook {
                    path: "/webhook/test".to_string(),
                    method: "POST".to_string(),
                }
            ],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let run_metadata = RunMetadata {
            run_id: Uuid::new_v4().to_string(),
            workflow_id: "test-workflow".to_string(),
            status: "running".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
            tags: HashMap::new(),
        };

        let step_metadata = StepMetadata {
            step_id: "step1".to_string(),
            name: "Test Step".to_string(),
            action: "test_action".to_string(),
            timeout_ms: Some(5000),
            retry: Some(RetryConfig {
                max_attempts: 3,
                backoff_ms: 1000,
                max_backoff_ms: 10000,
                jitter: true,
            }),
            depends_on: vec![],
            started_at: Utc::now(),
            attempt: 1,
        };

        Self {
            payload: serde_json::json!({"test": "data"}),
            steps: HashMap::new(),
            services: HashMap::new(),
            run: run_metadata,
            state: HashMap::new(),
            context: HashMap::new(),
            current_step: step_metadata,
            workflow,
            created_at: Utc::now(),
        }
    }
}

impl RunMetadata {
    /// Create a new run metadata
    pub fn new(run_id: String, workflow_id: String) -> Self {
        Self {
            run_id,
            workflow_id,
            status: "pending".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
            tags: HashMap::new(),
        }
    }

    /// Mark the run as started
    pub fn start(&mut self) {
        self.status = "running".to_string();
        self.started_at = Utc::now();
    }

    /// Mark the run as completed
    pub fn complete(&mut self) {
        self.status = "completed".to_string();
        self.completed_at = Some(Utc::now());
    }

    /// Mark the run as failed
    pub fn fail(&mut self, error: String) {
        self.status = "failed".to_string();
        self.error = Some(error);
        self.completed_at = Some(Utc::now());
    }

    /// Validate the run metadata
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.run_id.is_empty() {
            return Err(CoreError::Validation("Run ID cannot be empty".to_string()));
        }

        if self.workflow_id.is_empty() {
            return Err(CoreError::Validation("Workflow ID cannot be empty".to_string()));
        }

        Ok(())
    }
}

impl StepMetadata {
    /// Create a new step metadata
    pub fn new(step_id: String, name: String, action: String) -> Self {
        Self {
            step_id,
            name,
            action,
            timeout_ms: None,
            retry: None,
            depends_on: vec![],
            started_at: Utc::now(),
            attempt: 1,
        }
    }

    /// Validate the step metadata
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.step_id.is_empty() {
            return Err(CoreError::Validation("Step ID cannot be empty".to_string()));
        }

        if self.name.is_empty() {
            return Err(CoreError::Validation("Step name cannot be empty".to_string()));
        }

        if self.action.is_empty() {
            return Err(CoreError::Validation("Step action cannot be empty".to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{WorkflowDefinition, WorkflowRun, RunStatus, StepResult, StepStatus};
    use crate::job::{Job, JobPriority};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_context_creation() {
        let context = Context::test_context();
        assert!(context.validate().is_ok());
        assert_eq!(context.current_step.step_id, "step1");
        assert_eq!(context.run.workflow_id, "test-workflow");
    }

    #[test]
    fn test_context_from_job() {
        // Create a test workflow
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: Some("A test workflow".to_string()),
            steps: vec![
                crate::models::StepDefinition {
                    id: "step1".to_string(),
                    name: "Test Step".to_string(),
                    action: "test_action".to_string(),
                    timeout: Some(5000),
                    retry: Some(crate::models::RetryConfig {
                        max_attempts: 3,
                        backoff_ms: 1000,
                    }),
                    depends_on: vec![],
                }
            ],
            triggers: vec![
                crate::models::TriggerDefinition::Webhook {
                    path: "/webhook/test".to_string(),
                    method: "POST".to_string(),
                }
            ],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Create a test run
        let run = WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: "test-workflow".to_string(),
            status: RunStatus::Running,
            payload: serde_json::json!({"test": "data"}),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        };

        // Create a test job
        let job = Job::new(
            "test-workflow".to_string(),
            run.id.to_string(),
            "step1".to_string(),
            serde_json::json!({"test": "payload"}),
            JobPriority::Normal,
        );

        // Create context from job
        let context_result = Context::from_job(&job, &workflow, &run, &[], HashMap::new());
        assert!(context_result.is_ok());

        let context = context_result.unwrap();
        assert_eq!(context.current_step.step_id, "step1");
        assert_eq!(context.run.workflow_id, "test-workflow");
        assert_eq!(context.payload, serde_json::json!({"test": "payload"}));
    }

    #[test]
    fn test_context_serialization() {
        let context = Context::test_context();
        
        // Test serialization
        let json_result = context.to_json();
        assert!(json_result.is_ok());
        
        let json = json_result.unwrap();
        assert!(json.contains("test-workflow"));
        assert!(json.contains("step1"));
        
        // Test deserialization
        let deserialized_result = Context::from_json(&json);
        assert!(deserialized_result.is_ok());
        
        let deserialized = deserialized_result.unwrap();
        assert_eq!(deserialized.current_step.step_id, context.current_step.step_id);
        assert_eq!(deserialized.run.workflow_id, context.run.workflow_id);
    }

    #[test]
    fn test_context_methods() {
        let mut context = Context::test_context();
        
        // Test state operations
        context.set_state("key1".to_string(), serde_json::json!("value1"));
        assert_eq!(context.get_state("key1"), Some(&serde_json::json!("value1")));
        
        // Test context operations
        context.set_context("ctx1".to_string(), serde_json::json!("ctx_value1"));
        assert_eq!(context.get_context("ctx1"), Some(&serde_json::json!("ctx_value1")));
        
        // Test run tag operations
        context.set_run_tag("tag1".to_string(), "tag_value1".to_string());
        assert_eq!(context.get_run_tag("tag1"), Some(&"tag_value1".to_string()));
        
        // Test step completion check
        assert!(!context.is_step_completed("step1"));
        assert_eq!(context.get_completed_step_ids().len(), 0);
    }

    #[test]
    fn test_run_metadata() {
        let mut run = RunMetadata::new("run-123".to_string(), "workflow-123".to_string());
        
        assert!(run.validate().is_ok());
        assert_eq!(run.status, "pending");
        
        run.start();
        assert_eq!(run.status, "running");
        
        run.complete();
        assert_eq!(run.status, "completed");
        assert!(run.completed_at.is_some());
        
        let mut run2 = RunMetadata::new("run-456".to_string(), "workflow-456".to_string());
        run2.fail("Test error".to_string());
        assert_eq!(run2.status, "failed");
        assert_eq!(run2.error, Some("Test error".to_string()));
    }

    #[test]
    fn test_step_metadata() {
        let step = StepMetadata::new("step1".to_string(), "Test Step".to_string(), "test_action".to_string());
        
        assert!(step.validate().is_ok());
        assert_eq!(step.step_id, "step1");
        assert_eq!(step.name, "Test Step");
        assert_eq!(step.action, "test_action");
        assert_eq!(step.attempt, 1);
    }

    #[test]
    fn test_validation_errors() {
        // Test invalid run metadata
        let invalid_run = RunMetadata {
            run_id: "".to_string(),
            workflow_id: "workflow-123".to_string(),
            status: "pending".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
            tags: HashMap::new(),
        };
        
        assert!(invalid_run.validate().is_err());
        
        // Test invalid step metadata
        let invalid_step = StepMetadata {
            step_id: "".to_string(),
            name: "Test Step".to_string(),
            action: "test_action".to_string(),
            timeout_ms: None,
            retry: None,
            depends_on: vec![],
            started_at: Utc::now(),
            attempt: 1,
        };
        
        assert!(invalid_step.validate().is_err());
    }
} 