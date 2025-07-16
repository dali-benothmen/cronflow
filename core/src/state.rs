//! State management for the Node-Cronflow Core Engine

use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;
use crate::error::{CoreError, CoreResult};
use crate::models::{WorkflowDefinition, WorkflowRun, StepResult, RunStatus};
use crate::database::Database;

/// State manager for workflow orchestration
pub struct StateManager {
    db: Database,
    active_runs: HashMap<Uuid, WorkflowRun>,
}

impl StateManager {
    /// Create a new state manager
    pub fn new(db_path: &str) -> CoreResult<Self> {
        let db = Database::new(db_path)?;
        Ok(StateManager {
            db,
            active_runs: HashMap::new(),
        })
    }

    /// Register a new workflow
    pub fn register_workflow(&self, workflow: WorkflowDefinition) -> CoreResult<()> {
        log::info!("Registering workflow: {}", workflow.id);
        self.db.save_workflow(&workflow)
    }

    /// Get a workflow by ID
    pub fn get_workflow(&self, id: &str) -> CoreResult<Option<WorkflowDefinition>> {
        self.db.get_workflow(id)
    }

    /// Create a new workflow run
    pub fn create_run(&mut self, workflow_id: &str, payload: serde_json::Value) -> CoreResult<Uuid> {
        let _workflow = self.get_workflow(workflow_id)?
            .ok_or_else(|| CoreError::WorkflowNotFound(workflow_id.to_string()))?;

        let run_id = Uuid::new_v4();
        let now = Utc::now();

        let run = WorkflowRun {
            id: run_id,
            workflow_id: workflow_id.to_string(),
            status: RunStatus::Pending,
            payload,
            started_at: now,
            completed_at: None,
            error: None,
        };

        self.db.save_run(&run)?;
        self.active_runs.insert(run_id, run);

        log::info!("Created workflow run: {} for workflow: {}", run_id, workflow_id);
        Ok(run_id)
    }

    /// Get a workflow run by ID
    pub fn get_run(&self, run_id: &Uuid) -> CoreResult<Option<WorkflowRun>> {
        // First check active runs
        if let Some(run) = self.active_runs.get(run_id) {
            return Ok(Some(run.clone()));
        }

        // Load from database
        self.db.get_run(&run_id.to_string())
    }

    /// Update run status
    pub fn update_run_status(&mut self, run_id: &Uuid, status: RunStatus) -> CoreResult<()> {
        if let Some(run) = self.active_runs.get_mut(run_id) {
            run.status = status.clone();
            
            if matches!(status, RunStatus::Completed | RunStatus::Failed) {
                run.completed_at = Some(Utc::now());
            }

            self.db.save_run(run)?;
            log::info!("Updated run {} status to {:?}", run_id, status);
        }

        Ok(())
    }

    /// Save step result
    pub fn save_step_result(&self, run_id: &Uuid, result: StepResult) -> CoreResult<()> {
        self.db.save_step_result(&result, &run_id.to_string())
    }

    /// Get all active runs
    pub fn get_active_runs(&self) -> Vec<WorkflowRun> {
        self.active_runs.values().cloned().collect()
    }

    /// Clean up completed runs
    pub fn cleanup_completed_runs(&mut self) -> CoreResult<()> {
        let completed_runs: Vec<Uuid> = self.active_runs
            .iter()
            .filter(|(_, run)| matches!(run.status, RunStatus::Completed | RunStatus::Failed))
            .map(|(id, _)| *id)
            .collect();

        let count = completed_runs.len();
        for run_id in completed_runs {
            self.active_runs.remove(&run_id);
        }

        log::info!("Cleaned up {} completed runs", count);
        Ok(())
    }

    /// Get completed steps for a run
    pub fn get_completed_steps(&self, run_id: &Uuid) -> CoreResult<Vec<StepResult>> {
        self.db.get_step_results(&run_id.to_string())
    }
} 