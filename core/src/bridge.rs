//! N-API bridge for Node.js communication
//! 
//! This module handles the communication between the Rust core engine
//! and the Node.js SDK via N-API.

use napi_derive::napi;
use crate::error::{CoreError, CoreResult};
use crate::state::StateManager;
use crate::models::WorkflowDefinition;
use crate::context::Context;
use crate::job::Job;
use crate::triggers::{TriggerManager, WebhookTrigger};
use std::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;

/// N-API bridge for Node.js communication
pub struct Bridge {
    state_manager: Mutex<StateManager>,
    trigger_manager: Mutex<TriggerManager>,
}

impl Bridge {
    /// Create a new N-API bridge
    pub fn new(db_path: &str) -> CoreResult<Self> {
        let state_manager = StateManager::new(db_path)?;
        let trigger_manager = TriggerManager::new();
        Ok(Bridge { 
            state_manager: Mutex::new(state_manager),
            trigger_manager: Mutex::new(trigger_manager),
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

    /// Register a webhook trigger for a workflow
    pub fn register_webhook_trigger(&self, workflow_id: &str, trigger_json: &str) -> CoreResult<()> {
        log::info!("Registering webhook trigger for workflow: {} with config: {}", workflow_id, trigger_json);
        
        // Parse trigger JSON
        let trigger: WebhookTrigger = serde_json::from_str(trigger_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Validate the trigger
        trigger.validate()?;
        
        // Register with trigger manager
        let mut trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        trigger_manager.register_webhook_trigger(workflow_id, trigger)?;
        
        log::info!("Successfully registered webhook trigger for workflow: {}", workflow_id);
        Ok(())
    }

    /// Get all registered webhook triggers
    pub fn get_webhook_triggers(&self) -> CoreResult<String> {
        let trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        let triggers = trigger_manager.get_webhook_triggers();
        
        let triggers_json = serde_json::to_string(&triggers)
            .map_err(|e| CoreError::Serialization(e))?;
        
        Ok(triggers_json)
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

    /// Execute a job with context for Bun.js
    pub fn execute_job(&self, job: &Job, services: HashMap<String, serde_json::Value>) -> CoreResult<String> {
        log::info!("Executing job: {}", job.id);
        
        // Get state manager
        let state_manager = self.state_manager.lock().unwrap();
        
        // Get workflow definition
        let _workflow = state_manager.get_workflow(&job.workflow_id)?;
        
        // Get run information
        let run_uuid = Uuid::parse_str(&job.run_id)
            .map_err(|e| CoreError::UuidParse(e))?;
        let run = state_manager.get_run(&run_uuid)?
            .ok_or_else(|| CoreError::WorkflowNotFound(format!("Run not found: {}", job.run_id)))?;
        
        // Get completed steps for this run
        let completed_steps = state_manager.get_completed_steps(&run_uuid)?;
        
        // Create context object using the new method
        let context = Context::new(
            job.run_id.clone(),
            job.workflow_id.clone(),
            job.step_name.clone(),
            job.payload.clone(),
            run,
            completed_steps,
        )?;
        
        // Add services to context
        let mut context_with_services = context;
        for (name, config) in services {
            context_with_services.add_service(name, config);
        }
        
        // Serialize context for Bun.js
        let context_json = context_with_services.to_json()?;
        
        // For now, return the context JSON
        // TODO: In the real implementation, this would be sent to Bun.js for execution
        let result = serde_json::json!({
            "job_id": job.id,
            "run_id": job.run_id,
            "step_id": job.step_name,
            "context": context_json,
            "status": "ready_for_execution",
            "message": "Job context created successfully, ready for Bun.js execution"
        });
        
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Job execution context created for {}: {}", job.id, result_json);
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

#[napi(object)]
pub struct JobExecutionResult {
    pub success: bool,
    pub job_id: Option<String>,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub context: Option<String>,
    pub result: Option<String>,
    pub message: String,
}

#[napi(object)]
pub struct WebhookTriggerRegistrationResult {
    pub success: bool,
    pub message: String,
}

#[napi(object)]
pub struct WebhookTriggersResult {
    pub success: bool,
    pub triggers: Option<String>,
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

/// Register a webhook trigger via N-API
#[napi]
pub fn register_webhook_trigger(workflow_id: String, trigger_json: String, db_path: String) -> WebhookTriggerRegistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WebhookTriggerRegistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.register_webhook_trigger(&workflow_id, &trigger_json) {
        Ok(_) => {
            WebhookTriggerRegistrationResult {
                success: true,
                message: "Webhook trigger registered successfully".to_string(),
            }
        }
        Err(e) => {
            WebhookTriggerRegistrationResult {
                success: false,
                message: format!("Failed to register webhook trigger: {}", e),
            }
        }
    }
}

/// Get all webhook triggers via N-API
#[napi]
pub fn get_webhook_triggers(db_path: String) -> WebhookTriggersResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WebhookTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_webhook_triggers() {
        Ok(triggers_json) => {
            WebhookTriggersResult {
                success: true,
                triggers: Some(triggers_json),
                message: "Webhook triggers retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            WebhookTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to get webhook triggers: {}", e),
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

/// Execute a job with context via N-API
#[napi]
pub fn execute_job(job_json: String, services_json: String, db_path: String) -> JobExecutionResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return JobExecutionResult {
                success: false,
                job_id: None,
                run_id: None,
                step_id: None,
                context: None,
                result: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    // Parse job from JSON
    let job: Job = match serde_json::from_str(&job_json) {
        Ok(job) => job,
        Err(e) => {
            return JobExecutionResult {
                success: false,
                job_id: None,
                run_id: None,
                step_id: None,
                context: None,
                result: None,
                message: format!("Failed to parse job JSON: {}", e),
            };
        }
    };
    
    // Parse services from JSON
    let services: HashMap<String, serde_json::Value> = match serde_json::from_str(&services_json) {
        Ok(services) => services,
        Err(e) => {
            return JobExecutionResult {
                success: false,
                job_id: None,
                run_id: None,
                step_id: None,
                context: None,
                result: None,
                message: format!("Failed to parse services JSON: {}", e),
            };
        }
    };
    
    match bridge.execute_job(&job, services) {
        Ok(result_json) => {
            // Parse the result to extract individual fields
            let result: serde_json::Value = match serde_json::from_str(&result_json) {
                Ok(result) => result,
                Err(_) => {
                    return JobExecutionResult {
                        success: false,
                        job_id: None,
                        run_id: None,
                        step_id: None,
                        context: None,
                        result: None,
                        message: "Failed to parse execution result".to_string(),
                    };
                }
            };
            
            JobExecutionResult {
                success: true,
                job_id: result["job_id"].as_str().map(|s| s.to_string()),
                run_id: result["run_id"].as_str().map(|s| s.to_string()),
                step_id: result["step_id"].as_str().map(|s| s.to_string()),
                context: result["context"].as_str().map(|s| s.to_string()),
                result: Some(result_json),
                message: "Job executed successfully".to_string(),
            }
        }
        Err(e) => {
            JobExecutionResult {
                success: false,
                job_id: None,
                run_id: None,
                step_id: None,
                context: None,
                result: None,
                message: format!("Failed to execute job: {}", e),
            }
        }
    }
} 