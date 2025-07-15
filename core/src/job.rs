use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::error::CoreError;
use crate::models::{WorkflowDefinition, WorkflowRun, StepResult};

/// Job states for tracking execution progress
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Retrying,
}

/// Job priority levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, Ord, Eq)]
pub enum JobPriority {
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,
}

/// Retry configuration for jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 1000,
            max_backoff_ms: 30000,
            jitter: true,
        }
    }
}

/// Job metadata for tracking and debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub attempt_count: u32,
    pub last_error: Option<String>,
    pub tags: HashMap<String, String>,
}

impl Default for JobMetadata {
    fn default() -> Self {
        Self {
            created_at: Utc::now(),
            updated_at: Utc::now(),
            started_at: None,
            completed_at: None,
            attempt_count: 0,
            last_error: None,
            tags: HashMap::new(),
        }
    }
}

/// Main Job structure for workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub step_name: String,
    pub state: JobState,
    pub priority: JobPriority,
    pub payload: serde_json::Value,
    pub result: Option<StepResult>,
    pub retry_config: RetryConfig,
    pub metadata: JobMetadata,
    pub dependencies: Vec<String>, // IDs of jobs this job depends on
    pub timeout_ms: Option<u64>,
    pub context: HashMap<String, serde_json::Value>, // Additional context data
}

impl Job {
    /// Create a new job
    pub fn new(
        workflow_id: String,
        run_id: String,
        step_name: String,
        payload: serde_json::Value,
        priority: JobPriority,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            workflow_id,
            run_id,
            step_name,
            state: JobState::Pending,
            priority,
            payload,
            result: None,
            retry_config: RetryConfig::default(),
            metadata: JobMetadata::default(),
            dependencies: Vec::new(),
            timeout_ms: None,
            context: HashMap::new(),
        }
    }

    /// Create a job from workflow definition and run
    pub fn from_workflow_step(
        workflow: &WorkflowDefinition,
        run: &WorkflowRun,
        step_name: &str,
        payload: serde_json::Value,
    ) -> Result<Self, CoreError> {
        // Find the step in the workflow
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == step_name)
            .ok_or_else(|| {
                CoreError::InvalidWorkflow(format!("Step '{}' not found in workflow", step_name))
            })?;

        // Create job with step configuration
        let mut job = Self::new(
            workflow.id.clone(),
            run.id.to_string(), // Convert Uuid to String
            step_name.to_string(),
            payload,
            JobPriority::Normal,
        );

        // Apply step-specific configuration
        if let Some(timeout) = step.timeout {
            job.timeout_ms = Some(timeout);
        }

        if let Some(retry) = &step.retry {
            job.retry_config = RetryConfig {
                max_attempts: retry.max_attempts,
                backoff_ms: retry.backoff_ms,
                max_backoff_ms: retry.backoff_ms * 10, // Use 10x backoff as max
                jitter: true,
            };
        }

        // Add dependencies based on step configuration
        job.dependencies = step.depends_on.clone();

        Ok(job)
    }

    /// Start the job execution
    pub fn start(&mut self) -> Result<(), CoreError> {
        if self.state != JobState::Pending && self.state != JobState::Retrying {
            return Err(CoreError::State(
                format!("Cannot start job in state: {:?}", self.state)
            ));
        }

        self.state = JobState::Running;
        self.metadata.started_at = Some(Utc::now());
        self.metadata.updated_at = Utc::now();
        self.metadata.attempt_count += 1;

        Ok(())
    }

    /// Complete the job successfully
    pub fn complete(&mut self, result: StepResult) -> Result<(), CoreError> {
        if self.state != JobState::Running {
            return Err(CoreError::State(
                format!("Cannot complete job in state: {:?}", self.state)
            ));
        }

        self.state = JobState::Completed;
        self.result = Some(result);
        self.metadata.completed_at = Some(Utc::now());
        self.metadata.updated_at = Utc::now();

        Ok(())
    }

    /// Fail the job
    pub fn fail(&mut self, error: String) -> Result<(), CoreError> {
        if self.state != JobState::Running {
            return Err(CoreError::State(
                format!("Cannot fail job in state: {:?}", self.state)
            ));
        }

        self.state = JobState::Failed;
        self.metadata.last_error = Some(error);
        self.metadata.completed_at = Some(Utc::now());
        self.metadata.updated_at = Utc::now();

        Ok(())
    }

    /// Retry the job
    pub fn retry(&mut self) -> Result<(), CoreError> {
        if self.state != JobState::Failed {
            return Err(CoreError::State(
                format!("Cannot retry job in state: {:?}", self.state)
            ));
        }

        if self.metadata.attempt_count >= self.retry_config.max_attempts {
            return Err(CoreError::Configuration(
                "Maximum retry attempts exceeded".to_string()
            ));
        }

        self.state = JobState::Retrying;
        self.metadata.updated_at = Utc::now();

        Ok(())
    }

    /// Cancel the job
    pub fn cancel(&mut self) -> Result<(), CoreError> {
        if self.state == JobState::Completed || self.state == JobState::Failed {
            return Err(CoreError::State(
                format!("Cannot cancel job in state: {:?}", self.state)
            ));
        }

        self.state = JobState::Cancelled;
        self.metadata.completed_at = Some(Utc::now());
        self.metadata.updated_at = Utc::now();

        Ok(())
    }

    /// Check if job is ready to execute (dependencies satisfied)
    pub fn is_ready(&self, completed_jobs: &[String]) -> bool {
        if self.state != JobState::Pending && self.state != JobState::Retrying {
            return false;
        }

        // Check if all dependencies are completed
        self.dependencies.iter().all(|dep_id| completed_jobs.contains(dep_id))
    }

    /// Check if job has timed out
    pub fn is_timed_out(&self) -> bool {
        if let Some(timeout_ms) = self.timeout_ms {
            if let Some(started_at) = self.metadata.started_at {
                let elapsed = Utc::now().signed_duration_since(started_at);
                return elapsed.num_milliseconds() as u64 > timeout_ms;
            }
        }
        false
    }

    /// Check if job can be retried
    pub fn can_retry(&self) -> bool {
        self.state == JobState::Failed 
            && self.metadata.attempt_count < self.retry_config.max_attempts
    }

    /// Calculate next retry delay with exponential backoff
    pub fn next_retry_delay(&self) -> u64 {
        let base_delay = self.retry_config.backoff_ms;
        let attempt = self.metadata.attempt_count.saturating_sub(1);
        let delay = base_delay * 2_u64.pow(attempt);
        
        // Cap at max backoff
        delay.min(self.retry_config.max_backoff_ms)
    }

    /// Validate job configuration
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.id.is_empty() {
            return Err(CoreError::Configuration("Job ID cannot be empty".to_string()));
        }

        if self.workflow_id.is_empty() {
            return Err(CoreError::Configuration("Workflow ID cannot be empty".to_string()));
        }

        if self.run_id.is_empty() {
            return Err(CoreError::Configuration("Run ID cannot be empty".to_string()));
        }

        if self.step_name.is_empty() {
            return Err(CoreError::Configuration("Step name cannot be empty".to_string()));
        }

        if self.retry_config.max_attempts == 0 {
            return Err(CoreError::Configuration("Max attempts must be greater than 0".to_string()));
        }

        Ok(())
    }

    /// Add a tag to the job
    pub fn add_tag(&mut self, key: String, value: String) {
        self.metadata.tags.insert(key, value);
        self.metadata.updated_at = Utc::now();
    }

    /// Get a tag value
    pub fn get_tag(&self, key: &str) -> Option<&String> {
        self.metadata.tags.get(key)
    }

    /// Add context data
    pub fn add_context(&mut self, key: String, value: serde_json::Value) {
        self.context.insert(key, value);
        self.metadata.updated_at = Utc::now();
    }

    /// Get context data
    pub fn get_context(&self, key: &str) -> Option<&serde_json::Value> {
        self.context.get(key)
    }
}

/// Job queue for managing job execution order
#[derive(Debug, Clone)]
pub struct JobQueue {
    pub jobs: Vec<Job>,
}

impl JobQueue {
    /// Create a new job queue
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
        }
    }

    /// Add a job to the queue
    pub fn enqueue(&mut self, job: Job) -> Result<(), CoreError> {
        job.validate()?;
        self.jobs.push(job);
        Ok(())
    }

    /// Get the next job to execute (highest priority, oldest first)
    pub fn dequeue(&mut self, completed_jobs: &[String]) -> Option<Job> {
        // Find jobs that are ready to execute
        let ready_jobs: Vec<_> = self.jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| job.is_ready(completed_jobs))
            .collect();

        if ready_jobs.is_empty() {
            return None;
        }

        // Sort by priority (highest first), then by creation time (oldest first)
        let next_job_index = ready_jobs
            .iter()
            .max_by(|(_, a), (_, b)| {
                a.priority.cmp(&b.priority)
                    .then(a.metadata.created_at.cmp(&b.metadata.created_at))
            })
            .map(|(index, _)| *index)?;

        Some(self.jobs.remove(next_job_index))
    }

    /// Get all jobs in the queue
    pub fn get_jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// Get job by ID
    pub fn get_job(&self, job_id: &str) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == job_id)
    }

    /// Get job by ID (mutable)
    pub fn get_job_mut(&mut self, job_id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == job_id)
    }

    /// Remove job by ID
    pub fn remove_job(&mut self, job_id: &str) -> Option<Job> {
        let index = self.jobs.iter().position(|job| job.id == job_id)?;
        Some(self.jobs.remove(index))
    }

    /// Get queue statistics
    pub fn stats(&self) -> JobQueueStats {
        let mut stats = JobQueueStats::default();
        
        for job in &self.jobs {
            match job.state {
                JobState::Pending => stats.pending += 1,
                JobState::Running => stats.running += 1,
                JobState::Completed => stats.completed += 1,
                JobState::Failed => stats.failed += 1,
                JobState::Cancelled => stats.cancelled += 1,
                JobState::Retrying => stats.retrying += 1,
            }
        }

        stats
    }

    /// Clear completed and failed jobs
    pub fn cleanup(&mut self) {
        self.jobs.retain(|job| {
            job.state != JobState::Completed && job.state != JobState::Failed
        });
    }
}

/// Statistics for job queue
#[derive(Debug, Clone, Default)]
pub struct JobQueueStats {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub retrying: usize,
}

impl JobQueueStats {
    /// Get total jobs in queue
    pub fn total(&self) -> usize {
        self.pending + self.running + self.completed + self.failed + self.cancelled + self.retrying
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_job_creation() {
        let job = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-1".to_string(),
            json!({"test": "data"}),
            JobPriority::Normal,
        );

        assert_eq!(job.workflow_id, "workflow-1");
        assert_eq!(job.run_id, "run-1");
        assert_eq!(job.step_name, "step-1");
        assert_eq!(job.state, JobState::Pending);
        assert_eq!(job.priority, JobPriority::Normal);
    }

    #[test]
    fn test_job_lifecycle() {
        let mut job = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-1".to_string(),
            json!({"test": "data"}),
            JobPriority::Normal,
        );

        // Start job
        assert!(job.start().is_ok());
        assert_eq!(job.state, JobState::Running);
        assert_eq!(job.metadata.attempt_count, 1);

        // Complete job
        let result = StepResult {
            step_id: "step1".to_string(),
            status: crate::models::StepStatus::Completed,
            output: Some(json!({"result": "success"})),
            error: None,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(100),
        };
        assert!(job.complete(result).is_ok());
        assert_eq!(job.state, JobState::Completed);
    }

    #[test]
    fn test_job_validation() {
        let mut job = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-1".to_string(),
            json!({"test": "data"}),
            JobPriority::Normal,
        );

        assert!(job.validate().is_ok());

        // Test invalid job
        job.id = "".to_string();
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_queue() {
        let mut queue = JobQueue::new();
        
        let job1 = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-1".to_string(),
            json!({"test": "data"}),
            JobPriority::Normal,
        );

        let job2 = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-2".to_string(),
            json!({"test": "data"}),
            JobPriority::High,
        );

        assert!(queue.enqueue(job1).is_ok());
        assert!(queue.enqueue(job2).is_ok());
        assert_eq!(queue.get_jobs().len(), 2);

        // Dequeue should return highest priority job
        let next_job = queue.dequeue(&[]);
        assert!(next_job.is_some());
        assert_eq!(next_job.unwrap().priority, JobPriority::High);
    }

    #[test]
    fn test_job_retry() {
        let mut job = Job::new(
            "workflow-1".to_string(),
            "run-1".to_string(),
            "step-1".to_string(),
            json!({"test": "data"}),
            JobPriority::Normal,
        );

        // Start and fail job
        job.start().unwrap();
        job.fail("Test error".to_string()).unwrap();

        // Retry job
        assert!(job.retry().is_ok());
        assert_eq!(job.state, JobState::Retrying);

        // Start again
        assert!(job.start().is_ok());
        assert_eq!(job.state, JobState::Running);
    }
} 