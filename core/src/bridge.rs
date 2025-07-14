//! N-API bridge for Node.js communication
//! 
//! This module handles the communication between the Rust core engine
//! and the Node.js SDK via N-API.

use napi::{JsObject, JsString, Result as NapiResult, Env};
use napi_derive::napi;
use crate::error::{CoreError, CoreResult};
use crate::state::StateManager;
use crate::models::WorkflowDefinition;
use serde_json;
use std::sync::Mutex;

/// N-API bridge for Node.js communication
pub struct Bridge {
    state_manager: Mutex<StateManager>,
}

impl Bridge {
    /// Create a new N-API bridge
    pub fn new(db_path: &str) -> CoreResult<Self> {
        let state_manager = StateManager::new(db_path)?;
        Ok(Bridge { 
            state_manager: Mutex::new(state_manager) 
        })
    }

    /// Register a workflow from Node.js
    pub fn register_workflow(&self, workflow_json: &str) -> CoreResult<()> {
        log::info!("Registering workflow from JSON: {}", workflow_json);
        
        // Parse JSON to WorkflowDefinition
        let workflow: WorkflowDefinition = serde_json::from_str(workflow_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Validate the workflow
        workflow.validate()
            .map_err(|e| CoreError::InvalidWorkflow(e))?;
        
        // Register with state manager
        let state_manager = self.state_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire state manager lock".to_string()))?;
        
        state_manager.register_workflow(workflow.clone())?;
        
        log::info!("Successfully registered workflow: {}", workflow.id);
        Ok(())
    }

    /// Create a workflow run from Node.js
    pub fn create_run(&self, workflow_id: &str, payload_json: &str) -> CoreResult<String> {
        log::info!("Creating run for workflow: {} with payload: {}", workflow_id, payload_json);
        
        // Parse payload JSON
        let payload: serde_json::Value = serde_json::from_str(payload_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Create run with state manager
        let mut state_manager = self.state_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire state manager lock".to_string()))?;
        
        let run_id = state_manager.create_run(workflow_id, payload)?;
        
        log::info!("Successfully created run: {} for workflow: {}", run_id, workflow_id);
        Ok(run_id.to_string())
    }

    /// Get workflow run status
    pub fn get_run_status(&self, run_id: &str) -> CoreResult<String> {
        log::info!("Getting status for run: {}", run_id);
        
        // Parse run ID
        let run_uuid = uuid::Uuid::parse_str(run_id)
            .map_err(|e| CoreError::UuidParse(e))?;
        
        // Get run from state manager
        let state_manager = self.state_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire state manager lock".to_string()))?;
        
        let _run = state_manager.get_run(&run_uuid)?
            .ok_or_else(|| CoreError::WorkflowNotFound(format!("Run not found: {}", run_id)))?;
        
        // For now, return a simple status
        // TODO: Implement full run status serialization
        let status_json = serde_json::json!({
            "run_id": run_id,
            "status": "pending",
            "message": "Run status retrieved successfully"
        });
        
        let result = serde_json::to_string(&status_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Retrieved status for run: {}", run_id);
        Ok(result)
    }

    /// Execute a workflow step
    pub fn execute_step(&self, run_id: &str, step_id: &str) -> CoreResult<String> {
        log::info!("Executing step {} for run {}", step_id, run_id);
        
        // Parse run ID
        let run_uuid = uuid::Uuid::parse_str(run_id)
            .map_err(|e| CoreError::UuidParse(e))?;
        
        // Get run from state manager
        let state_manager = self.state_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire state manager lock".to_string()))?;
        
        let _run = state_manager.get_run(&run_uuid)?
            .ok_or_else(|| CoreError::WorkflowNotFound(format!("Run not found: {}", run_id)))?;
        
        // For now, return a simple result
        // TODO: Implement actual step execution logic
        let result = serde_json::json!({
            "run_id": run_id,
            "step_id": step_id,
            "status": "pending",
            "message": "Step execution not yet implemented"
        });
        
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Step execution result for {}: {}", step_id, result_json);
        Ok(result_json)
    }
}

// N-API module setup
#[napi(object)]
pub struct WorkflowRegistrationResult {
    pub success: bool,
    pub message: String,
}

#[napi(object)]
pub struct RunCreationResult {
    pub success: bool,
    pub run_id: Option<String>,
    pub message: String,
}

#[napi(object)]
pub struct RunStatusResult {
    pub success: bool,
    pub status: Option<String>,
    pub message: String,
}

#[napi(object)]
pub struct StepExecutionResult {
    pub success: bool,
    pub result: Option<String>,
    pub message: String,
}

/// Register a workflow via N-API
#[napi]
pub fn register_workflow(workflow_json: String, db_path: String) -> WorkflowRegistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WorkflowRegistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.register_workflow(&workflow_json) {
        Ok(_) => {
            WorkflowRegistrationResult {
                success: true,
                message: "Workflow registered successfully".to_string(),
            }
        }
        Err(e) => {
            WorkflowRegistrationResult {
                success: false,
                message: format!("Failed to register workflow: {}", e),
            }
        }
    }
}

/// Create a workflow run via N-API
#[napi]
pub fn create_run(workflow_id: String, payload_json: String, db_path: String) -> RunCreationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return RunCreationResult {
                success: false,
                run_id: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.create_run(&workflow_id, &payload_json) {
        Ok(run_id) => {
            RunCreationResult {
                success: true,
                run_id: Some(run_id),
                message: "Run created successfully".to_string(),
            }
        }
        Err(e) => {
            RunCreationResult {
                success: false,
                run_id: None,
                message: format!("Failed to create run: {}", e),
            }
        }
    }
}

/// Get run status via N-API
#[napi]
pub fn get_run_status(run_id: String, db_path: String) -> RunStatusResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return RunStatusResult {
                success: false,
                status: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_run_status(&run_id) {
        Ok(status_json) => {
            RunStatusResult {
                success: true,
                status: Some(status_json),
                message: "Status retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            RunStatusResult {
                success: false,
                status: None,
                message: format!("Failed to get run status: {}", e),
            }
        }
    }
}

/// Execute a step via N-API
#[napi]
pub fn execute_step(run_id: String, step_id: String, db_path: String) -> StepExecutionResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return StepExecutionResult {
                success: false,
                result: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.execute_step(&run_id, &step_id) {
        Ok(result_json) => {
            StepExecutionResult {
                success: true,
                result: Some(result_json),
                message: "Step executed successfully".to_string(),
            }
        }
        Err(e) => {
            StepExecutionResult {
                success: false,
                result: None,
                message: format!("Failed to execute step: {}", e),
            }
        }
    }
} 