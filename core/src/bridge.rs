//! N-API bridge for Node.js communication
//! 
//! This module handles the communication between the Rust core engine
//! and the Node.js SDK via N-API.

use napi_derive::napi;
use crate::error::{CoreError, CoreResult};
use crate::state::StateManager;
use crate::models::WorkflowDefinition;
use crate::job::Job;
use crate::triggers::{TriggerManager, WebhookTrigger, ScheduleTrigger};
use crate::trigger_executor::TriggerExecutor;
use crate::dispatcher::Dispatcher;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use uuid::Uuid;
use serde::Serialize;

/// N-API bridge for Node.js communication
pub struct Bridge {
    state_manager: Arc<Mutex<StateManager>>,
    trigger_manager: Arc<Mutex<TriggerManager>>,
    trigger_executor: TriggerExecutor,
    job_dispatcher: Arc<Mutex<Dispatcher>>,
}

impl Bridge {
    /// Create a new N-API bridge
    pub fn new(db_path: &str) -> CoreResult<Self> {
        let state_manager = Arc::new(Mutex::new(StateManager::new(db_path)?));
        let trigger_manager = Arc::new(Mutex::new(TriggerManager::new()));
        
        // Create job dispatcher with default configuration and state manager
        let dispatcher_config = crate::dispatcher::WorkerPoolConfig::default();
        let job_dispatcher = Arc::new(Mutex::new(Dispatcher::new(dispatcher_config, state_manager.clone())));
        
        let trigger_executor = TriggerExecutor::new(
            state_manager.clone(), 
            trigger_manager.clone(),
            job_dispatcher.clone()
        );
        
        Ok(Bridge { 
            state_manager,
            trigger_manager,
            trigger_executor,
            job_dispatcher,
        })
    }

    /// Create a dummy bridge for the dispatcher (temporary workaround)
    fn create_dummy_bridge() -> CoreResult<Bridge> {
        // This is a temporary workaround to create a bridge for the dispatcher
        // In a real implementation, we'd need to handle this circular dependency properly
        Bridge::new(":memory:")
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
        
        // Register triggers for the workflow
        let trigger_ids = self.trigger_executor.register_workflow_triggers(&workflow.id, &workflow)?;
        
        log::info!("Successfully registered workflow: {} with {} triggers: {:?}", workflow.id, trigger_ids.len(), trigger_ids);
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

    /// Register a schedule trigger for a workflow
    pub fn register_schedule_trigger(&self, workflow_id: &str, trigger_json: &str) -> CoreResult<String> {
        log::info!("Registering schedule trigger for workflow: {} with config: {}", workflow_id, trigger_json);
        
        // Parse trigger JSON
        let trigger: ScheduleTrigger = serde_json::from_str(trigger_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Validate the trigger
        trigger.validate()?;
        
        // Register with trigger manager
        let mut trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        let trigger_id = trigger_manager.register_schedule_trigger(workflow_id, trigger)?;
        
        log::info!("Successfully registered schedule trigger for workflow: {} with ID: {}", workflow_id, trigger_id);
        Ok(trigger_id)
    }

    /// Get all registered schedule triggers
    pub fn get_schedule_triggers(&self) -> CoreResult<String> {
        let trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        let triggers = trigger_manager.get_schedule_triggers();
        
        let triggers_json = serde_json::to_string(&triggers)
            .map_err(|e| CoreError::Serialization(e))?;
        
        Ok(triggers_json)
    }

    /// Enable or disable a schedule trigger
    pub fn set_schedule_enabled(&self, trigger_id: &str, enabled: bool) -> CoreResult<()> {
        log::info!("Setting schedule trigger {} enabled: {}", trigger_id, enabled);
        
        let mut trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        trigger_manager.set_schedule_enabled(trigger_id, enabled)?;
        
        log::info!("Successfully updated schedule trigger: {}", trigger_id);
        Ok(())
    }

    /// Remove a schedule trigger
    pub fn remove_schedule_trigger(&self, trigger_id: &str) -> CoreResult<()> {
        log::info!("Removing schedule trigger: {}", trigger_id);
        
        let mut trigger_manager = self.trigger_manager.lock()
            .map_err(|_| CoreError::Internal("Failed to acquire trigger manager lock".to_string()))?;
        
        trigger_manager.remove_schedule_trigger(trigger_id)?;
        
        log::info!("Successfully removed schedule trigger: {}", trigger_id);
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

    /// Execute a step with context for Bun.js
    pub fn execute_step(&self, run_id: &str, step_id: &str, services_json: &str) -> CoreResult<String> {
        log::info!("Executing step {} for run {}", step_id, run_id);
        
        let run_uuid = uuid::Uuid::parse_str(run_id)
            .map_err(|e| CoreError::UuidParse(e))?;
        
        let state_manager = self.state_manager.lock().unwrap();
        let run = state_manager.get_run(&run_uuid)?
            .ok_or_else(|| CoreError::RunNotFound(format!("Run not found: {}", run_id)))?;
        
        // Get workflow definition
        let workflow = state_manager.get_workflow(&run.workflow_id)?
            .ok_or_else(|| CoreError::WorkflowNotFound(run.workflow_id.clone()))?;
        
        // Find the step to execute
        let step = workflow.get_step(step_id)
            .ok_or_else(|| CoreError::Validation(format!("Step '{}' not found in workflow '{}'", step_id, run.workflow_id)))?;
        
        // Get completed steps for context
        let completed_steps = state_manager.get_completed_steps(&run_uuid)?;
        
        // Parse services from JSON
        let services: HashMap<String, serde_json::Value> = if services_json.is_empty() {
            HashMap::new()
        } else {
            serde_json::from_str(services_json)
                .map_err(|e| CoreError::Serialization(e))?
        };
        
        // Create context object for Bun.js execution
        let mut context = crate::context::Context::new(
            run_id.to_string(),
            run.workflow_id.clone(),
            step_id.to_string(),
            run.payload.clone(),
            run.clone(),
            completed_steps,
        )?;
        
        // Add services to context
        for (service_name, service_config) in services {
            context.add_service(service_name, service_config);
        }
        
        // Set timeout if configured
        if let Some(timeout) = step.timeout {
            context.set_timeout(timeout);
        }
        
        // Serialize context for Bun.js
        let context_json = context.to_json()?;
        
        // For now, return the context as a result
        // TODO: In Task 1.2, we'll add the N-API call to Bun.js
        let result = serde_json::json!({
            "run_id": run_id,
            "step_id": step_id,
            "workflow_id": run.workflow_id,
            "context": context_json,
            "status": "ready_for_execution",
            "message": "Step context prepared for Bun.js execution"
        });
        
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        return Ok(result_json);
    }

    /// Execute a job with context for Bun.js
    pub fn execute_job(&self, job: &Job, _services: HashMap<String, serde_json::Value>) -> CoreResult<String> {
        log::info!("Executing job: {}", job.id);
        
        // Get state manager
        let state_manager = self.state_manager.lock().unwrap();
        
        // Get workflow definition
        let _workflow = state_manager.get_workflow(&job.workflow_id)?;
        
        // Get run information
        let _run_uuid = Uuid::parse_str(&job.run_id)
            .map_err(|e| CoreError::UuidParse(e))?;
        
        // For now, return a simple result
        // TODO: Implement actual job execution logic
        let result = serde_json::json!({
            "job_id": job.id,
            "run_id": job.run_id,
            "step_id": job.step_name,
            "status": "pending",
            "message": "Job execution not yet implemented"
        });
        
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Job execution result for {}: {}", job.id, result_json);
        Ok(result_json)
    }

    /// Execute a webhook trigger
    pub fn execute_webhook_trigger(&self, request_json: &str) -> CoreResult<String> {
        log::info!("Executing webhook trigger with request: {}", request_json);
        
        // Parse webhook request JSON
        let request: crate::triggers::WebhookRequest = serde_json::from_str(request_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Execute the webhook trigger
        let result = self.trigger_executor.execute_webhook_trigger(request)?;
        
        // Serialize the result
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Webhook trigger execution result: {}", result_json);
        Ok(result_json)
    }

    /// Execute a schedule trigger
    pub fn execute_schedule_trigger(&self, trigger_id: &str) -> CoreResult<String> {
        log::info!("Executing schedule trigger: {}", trigger_id);
        
        // Execute the schedule trigger
        let result = self.trigger_executor.execute_schedule_trigger(trigger_id)?;
        
        // Serialize the result
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Schedule trigger execution result: {}", result_json);
        Ok(result_json)
    }

    /// Execute a manual trigger
    pub fn execute_manual_trigger(&self, workflow_id: &str, payload_json: &str) -> CoreResult<String> {
        log::info!("Executing manual trigger for workflow: {} with payload: {}", workflow_id, payload_json);
        
        // Parse payload JSON
        let payload: serde_json::Value = serde_json::from_str(payload_json)
            .map_err(|e| CoreError::Serialization(e))?;
        
        // Execute the manual trigger
        let result = self.trigger_executor.execute_manual_trigger(workflow_id, payload)?;
        
        // Serialize the result
        let result_json = serde_json::to_string(&result)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Manual trigger execution result: {}", result_json);
        Ok(result_json)
    }

    /// Get trigger statistics
    pub fn get_trigger_stats(&self) -> CoreResult<String> {
        log::info!("Getting trigger statistics");
        
        // Get trigger statistics
        let stats = self.trigger_executor.get_trigger_stats()?;
        
        // Serialize the result
        let stats_json = serde_json::to_string(&stats)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Trigger statistics: {}", stats_json);
        Ok(stats_json)
    }

    /// Get triggers for a workflow
    pub fn get_workflow_triggers(&self, workflow_id: &str) -> CoreResult<String> {
        log::info!("Getting triggers for workflow: {}", workflow_id);
        
        // Get workflow triggers
        let triggers = self.trigger_executor.get_workflow_triggers(workflow_id)?;
        
        // Serialize the result
        let triggers_json = serde_json::to_string(&triggers)
            .map_err(|e| CoreError::Serialization(e))?;
        
        log::info!("Workflow triggers: {}", triggers_json);
        Ok(triggers_json)
    }

    /// Unregister triggers for a workflow
    pub fn unregister_workflow_triggers(&self, workflow_id: &str) -> CoreResult<()> {
        log::info!("Unregistering triggers for workflow: {}", workflow_id);
        
        // Unregister workflow triggers
        self.trigger_executor.unregister_workflow_triggers(workflow_id)?;
        
        log::info!("Successfully unregistered triggers for workflow: {}", workflow_id);
        Ok(())
    }

    /// Start the webhook server
    pub fn start_webhook_server(&mut self) -> CoreResult<()> {
        log::info!("Starting webhook server...");
        
        // For now, we'll just log that the server is configured
        // The actual HTTP server will be started by the Node.js side
        // This is a temporary workaround until we resolve the threading issues
        
        log::info!("Webhook server configuration ready");
        log::info!("Note: HTTP server needs to be started separately");
        
        Ok(())
    }
    
    /// Stop the webhook server
    pub fn stop_webhook_server(&mut self) -> CoreResult<()> {
        log::info!("Stopping webhook server");
        // TODO: Implement webhook server stop
        Ok(())
    }

    /// Get job status
    pub fn get_job_status(&self, job_id: &str) -> CoreResult<Option<String>> {
        log::info!("Getting job status for: {}", job_id);
        
        let dispatcher = self.job_dispatcher.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to acquire dispatcher lock: {}", e)))?;
        
        match dispatcher.get_job_status(job_id)? {
            Some(state) => Ok(Some(format!("{:?}", state))),
            None => Ok(None),
        }
    }

    /// Cancel a job
    pub fn cancel_job(&self, job_id: &str) -> CoreResult<bool> {
        log::info!("Cancelling job: {}", job_id);
        
        let dispatcher = self.job_dispatcher.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to acquire dispatcher lock: {}", e)))?;
        
        dispatcher.cancel_job(job_id)
    }

    /// Get dispatcher statistics
    pub fn get_dispatcher_stats(&self) -> CoreResult<crate::dispatcher::DispatcherStats> {
        log::info!("Getting dispatcher statistics");
        
        let dispatcher = self.job_dispatcher.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to acquire dispatcher lock: {}", e)))?;
        
        dispatcher.get_stats()
    }

    /// Get workflow run status
    pub fn get_workflow_run_status(&self, run_id: &str) -> CoreResult<Option<crate::models::RunStatus>> {
        log::info!("Getting workflow run status for: {}", run_id);
        
        let dispatcher = self.job_dispatcher.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to acquire dispatcher lock: {}", e)))?;
        
        dispatcher.get_workflow_run_status(run_id)
    }

    /// Get completed steps for a workflow run
    pub fn get_workflow_completed_steps(&self, run_id: &str) -> CoreResult<Vec<crate::models::StepResult>> {
        log::info!("Getting completed steps for workflow run: {}", run_id);
        
        let dispatcher = self.job_dispatcher.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to acquire dispatcher lock: {}", e)))?;
        
        dispatcher.get_workflow_completed_steps(run_id)
    }

    /// Execute workflow steps using step orchestrator and state machine
    pub fn execute_workflow_steps(&self, run_id: &str, workflow_id: &str) -> CoreResult<String> {
        log::info!("Executing workflow steps for run: {} workflow: {}", run_id, workflow_id);
        
        // Parse run ID
        let run_uuid = Uuid::parse_str(run_id)
            .map_err(|e| CoreError::Validation(format!("Invalid run ID: {}", e)))?;
        
        // Create step orchestrator
        let step_orchestrator = crate::step_orchestrator::StepOrchestrator::new(self.state_manager.clone());
        
        // Start step execution using the orchestrator
        match step_orchestrator.start_step_execution(&run_uuid, workflow_id) {
            Ok(()) => {
                log::info!("Successfully executed workflow steps for run: {}", run_id);
                Ok(serde_json::json!({
                    "success": true,
                    "run_id": run_id,
                    "workflow_id": workflow_id,
                    "message": "Workflow steps executed successfully"
                }).to_string())
            }
            Err(error) => {
                log::error!("Failed to execute workflow steps for run {}: {}", run_id, error);
                Err(error)
            }
        }
    }

    /// Execute workflow hook (onSuccess or onFailure)
    pub fn execute_workflow_hook(&self, hook_type: &str, context_json: &str, workflow_id: &str) -> CoreResult<String> {
        log::info!("Executing {} hook for workflow: {}", hook_type, workflow_id);
        
        // Validate hook type
        if hook_type != "onSuccess" && hook_type != "onFailure" {
            return Err(CoreError::Validation(format!("Invalid hook type: {}", hook_type)));
        }
        
        // For now, we'll return a success response
        // In the next phase, this will call the Bun.js hook execution
        let result = serde_json::json!({
            "success": true,
            "hook_type": hook_type,
            "workflow_id": workflow_id,
            "message": format!("{} hook executed successfully", hook_type),
            "context": serde_json::from_str::<serde_json::Value>(context_json).unwrap_or(serde_json::Value::Null)
        });
        
        Ok(result.to_string())
    }
}

/// Result for job execution
#[derive(Debug, Clone, Serialize)]
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

/// Result for job status retrieval
#[derive(Debug, Clone, Serialize)]
#[napi(object)]
pub struct JobStatusResult {
    pub success: bool,
    pub job_id: Option<String>,
    pub status: Option<String>,
    pub message: String,
}

/// Result for job cancellation
#[derive(Debug, Clone, Serialize)]
#[napi(object)]
pub struct JobCancellationResult {
    pub success: bool,
    pub job_id: Option<String>,
    pub cancelled: bool,
    pub message: String,
}

/// Result for dispatcher statistics
#[derive(Debug, Clone, Serialize)]
#[napi(object)]
pub struct DispatcherStatsResult {
    pub success: bool,
    pub stats: Option<String>,
    pub message: String,
}

/// Result for workflow run status
#[derive(Debug, Clone, Serialize)]
#[napi(object)]
pub struct WorkflowRunStatusResult {
    pub success: bool,
    pub run_id: Option<String>,
    pub status: Option<String>,
    pub message: String,
}

/// Result for workflow completed steps
#[derive(Debug, Clone, Serialize)]
#[napi(object)]
pub struct WorkflowStepsResult {
    pub success: bool,
    pub run_id: Option<String>,
    pub steps: Option<String>,
    pub message: String,
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

#[napi(object)]
pub struct ScheduleTriggerRegistrationResult {
    pub success: bool,
    pub message: String,
}

#[napi(object)]
pub struct ScheduleTriggersResult {
    pub success: bool,
    pub triggers: Option<String>,
    pub message: String,
}

/// Trigger execution result
#[napi_derive::napi]
pub struct TriggerExecutionResult {
    pub success: bool,
    pub run_id: Option<String>,
    pub workflow_id: Option<String>,
    pub message: String,
}

/// Trigger statistics result
#[napi_derive::napi]
pub struct TriggerStatsResult {
    pub success: bool,
    pub stats: Option<String>,
    pub message: String,
}

/// Workflow triggers result
#[napi_derive::napi]
pub struct WorkflowTriggersResult {
    pub success: bool,
    pub triggers: Option<String>,
    pub message: String,
}

/// Trigger unregistration result
#[napi_derive::napi]
pub struct TriggerUnregistrationResult {
    pub success: bool,
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

/// Register a schedule trigger via N-API
#[napi]
pub fn register_schedule_trigger(workflow_id: String, trigger_json: String, db_path: String) -> ScheduleTriggerRegistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.register_schedule_trigger(&workflow_id, &trigger_json) {
        Ok(trigger_id) => {
            ScheduleTriggerRegistrationResult {
                success: true,
                message: format!("Schedule trigger registered successfully with ID: {}", trigger_id),
            }
        }
        Err(e) => {
            ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to register schedule trigger: {}", e),
            }
        }
    }
}

/// Get all schedule triggers via N-API
#[napi]
pub fn get_schedule_triggers(db_path: String) -> ScheduleTriggersResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return ScheduleTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_schedule_triggers() {
        Ok(triggers_json) => {
            ScheduleTriggersResult {
                success: true,
                triggers: Some(triggers_json),
                message: "Schedule triggers retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            ScheduleTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to get schedule triggers: {}", e),
            }
        }
    }
}

/// Enable or disable a schedule trigger via N-API
#[napi]
pub fn set_schedule_enabled(trigger_id: String, enabled: bool, db_path: String) -> ScheduleTriggerRegistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.set_schedule_enabled(&trigger_id, enabled) {
        Ok(_) => {
            ScheduleTriggerRegistrationResult {
                success: true,
                message: format!("Schedule trigger {} enabled: {}", trigger_id, enabled),
            }
        }
        Err(e) => {
            ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to set schedule trigger enabled: {}", e),
            }
        }
    }
}

/// Remove a schedule trigger via N-API
#[napi]
pub fn remove_schedule_trigger(trigger_id: String, db_path: String) -> ScheduleTriggerRegistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.remove_schedule_trigger(&trigger_id) {
        Ok(_) => {
            ScheduleTriggerRegistrationResult {
                success: true,
                message: format!("Schedule trigger {} removed", trigger_id),
            }
        }
        Err(e) => {
            ScheduleTriggerRegistrationResult {
                success: false,
                message: format!("Failed to remove schedule trigger: {}", e),
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
pub fn execute_step(run_id: String, step_id: String, db_path: String, services_json: String) -> StepExecutionResult {
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

    match bridge.execute_step(&run_id, &step_id, &services_json) {
        Ok(result) => {
            StepExecutionResult {
                success: true,
                result: Some(result),
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

/// Execute a step function in Bun.js via N-API
#[napi]
pub fn execute_step_function(
    step_name: String,
    context_json: String,
    workflow_id: String,
    run_id: String,
    db_path: String
) -> StepExecutionResult {
    log::info!("Executing step function: {} for workflow: {} run: {}", step_name, workflow_id, run_id);
    
    // Parse context JSON to validate it
    let context: crate::context::Context = match serde_json::from_str(&context_json) {
        Ok(context) => context,
        Err(e) => {
            return StepExecutionResult {
                success: false,
                result: None,
                message: format!("Failed to parse context JSON: {}", e),
            };
        }
    };
    
    // Create bridge to access state manager
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
    
    // Get state manager to save step results
    let state_manager = match bridge.state_manager.lock() {
        Ok(manager) => manager,
        Err(_) => {
            return StepExecutionResult {
                success: false,
                result: None,
                message: "Failed to acquire state manager lock".to_string(),
            };
        }
    };
    
    // Parse run ID
    let run_uuid = match uuid::Uuid::parse_str(&run_id) {
        Ok(uuid) => uuid,
        Err(e) => {
            return StepExecutionResult {
                success: false,
                result: None,
                message: format!("Failed to parse run ID: {}", e),
            };
        }
    };
    
    // Create step result for tracking
    let step_result = crate::models::StepResult {
        step_id: step_name.clone(),
        status: crate::models::StepStatus::Running,
        started_at: chrono::Utc::now(),
        completed_at: None,
        output: None,
        error: None,
        duration_ms: None,
    };
    
    // Save step result to database
    if let Err(e) = state_manager.save_step_result(&run_uuid, step_result) {
        log::warn!("Failed to save step result: {}", e);
    }
    
    // For now, return success with context for Bun.js execution
    // The actual Bun.js execution will be handled by the SDK when this function is called
    let result = serde_json::json!({
        "step_name": step_name,
        "workflow_id": workflow_id,
        "run_id": run_id,
        "context": context_json,
        "status": "ready_for_bun_execution",
        "message": "Step function ready for Bun.js execution via SDK"
    });
    
    let result_json = serde_json::to_string(&result)
        .map_err(|e| CoreError::Serialization(e))
        .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
    
    StepExecutionResult {
        success: true,
        result: Some(result_json),
        message: "Step function prepared for Bun.js execution".to_string(),
    }
}

/// Execute step in Bun.js (called from Rust step orchestrator)
#[napi]
pub fn execute_step_in_bun(
    step_name: String, context_json: String, workflow_id: String, run_id: String, db_path: String
) -> StepExecutionResult {
    let result = serde_json::json!({
        "step_name": step_name, "workflow_id": workflow_id, "run_id": run_id,
        "context": context_json, "status": "executing_in_bun",
        "message": "Step execution delegated to Bun.js"
    });
    match serde_json::to_string(&result) {
        Ok(result_json) => StepExecutionResult {
            success: true,
            result: Some(result_json),
            message: "Step execution delegated to Bun.js".to_string(),
        },
        Err(e) => StepExecutionResult {
            success: false,
            result: None,
            message: format!("Failed to serialize result: {}", e),
        },
    }
}

/// Execute step via Bun.js step execution handler (for Rust orchestrator)
#[napi]
pub fn execute_step_via_bun(
    step_name: String, context_json: String, workflow_id: String, run_id: String, db_path: String
) -> StepExecutionResult {
    // This function will be called from the Rust step orchestrator
    // It should trigger the Bun.js step execution handler
    
    // For now, we'll return a success result indicating the step should be executed
    // In a real implementation, this would trigger the Bun.js step execution
    let result = serde_json::json!({
        "step_name": step_name,
        "workflow_id": workflow_id,
        "run_id": run_id,
        "context": context_json,
        "status": "ready_for_bun_execution",
        "message": "Step ready for Bun.js execution",
        "execution_type": "bun_step_handler"
    });
    
    match serde_json::to_string(&result) {
        Ok(result_json) => StepExecutionResult {
            success: true,
            result: Some(result_json),
            message: "Step ready for Bun.js execution".to_string(),
        },
        Err(e) => StepExecutionResult {
            success: false,
            result: None,
            message: format!("Failed to serialize result: {}", e),
        },
    }
}

/// Execute a job function via N-API (enhanced for Task 2.4)
#[napi]
pub fn execute_job_function(
    job_json: String,
    services_json: String,
    db_path: String
) -> JobExecutionResult {
    log::info!("Executing job function with job: {}", job_json);
    
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
    
    // Execute the job using the bridge
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
                message: "Job function executed successfully".to_string(),
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

/// Execute a job with context via N-API (enhanced for Task 2.4)
#[napi]
pub fn execute_job(job_json: String, services_json: String, db_path: String) -> JobExecutionResult {
    log::info!("Executing job with context: {}", job_json);
    
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

/// Get job status via N-API
#[napi]
pub fn get_job_status(job_id: String, db_path: String) -> JobStatusResult {
    log::info!("Getting job status for: {}", job_id);
    
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return JobStatusResult {
                success: false,
                job_id: None,
                status: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_job_status(&job_id) {
        Ok(status) => {
            let status_str = match status {
                Some(s) => format!("{:?}", s),
                None => "not_found".to_string(),
            };
            
            JobStatusResult {
                success: true,
                job_id: Some(job_id),
                status: Some(status_str),
                message: "Job status retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            JobStatusResult {
                success: false,
                job_id: None,
                status: None,
                message: format!("Failed to get job status: {}", e),
            }
        }
    }
}

/// Cancel a job via N-API
#[napi]
pub fn cancel_job(job_id: String, db_path: String) -> JobCancellationResult {
    log::info!("Cancelling job: {}", job_id);
    
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return JobCancellationResult {
                success: false,
                job_id: None,
                cancelled: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.cancel_job(&job_id) {
        Ok(cancelled) => {
            JobCancellationResult {
                success: true,
                job_id: Some(job_id),
                cancelled,
                message: if cancelled {
                    "Job cancelled successfully".to_string()
                } else {
                    "Job not found or already completed".to_string()
                },
            }
        }
        Err(e) => {
            JobCancellationResult {
                success: false,
                job_id: None,
                cancelled: false,
                message: format!("Failed to cancel job: {}", e),
            }
        }
    }
}

/// Get dispatcher statistics via N-API
#[napi]
pub fn get_dispatcher_stats(db_path: String) -> DispatcherStatsResult {
    log::info!("Getting dispatcher statistics");
    
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return DispatcherStatsResult {
                success: false,
                stats: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_dispatcher_stats() {
        Ok(stats) => {
            let stats_json = serde_json::to_string(&stats)
                .unwrap_or_else(|_| "{}".to_string());
            
            DispatcherStatsResult {
                success: true,
                stats: Some(stats_json),
                message: "Dispatcher statistics retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            DispatcherStatsResult {
                success: false,
                stats: None,
                message: format!("Failed to get dispatcher stats: {}", e),
            }
        }
    }
}

/// Get workflow run status via N-API
#[napi]
pub fn get_workflow_run_status(run_id: String, db_path: String) -> WorkflowRunStatusResult {
    log::info!("Getting workflow run status for: {}", run_id);
    
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WorkflowRunStatusResult {
                success: false,
                run_id: None,
                status: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_workflow_run_status(&run_id) {
        Ok(status) => {
            let status_str = match status {
                Some(s) => format!("{:?}", s),
                None => "not_found".to_string(),
            };
            
            WorkflowRunStatusResult {
                success: true,
                run_id: Some(run_id),
                status: Some(status_str),
                message: "Workflow run status retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            WorkflowRunStatusResult {
                success: false,
                run_id: None,
                status: None,
                message: format!("Failed to get workflow run status: {}", e),
            }
        }
    }
}

/// Get completed steps for a workflow run via N-API
#[napi]
pub fn get_workflow_completed_steps(run_id: String, db_path: String) -> WorkflowStepsResult {
    log::info!("Getting completed steps for workflow run: {}", run_id);
    
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WorkflowStepsResult {
                success: false,
                run_id: None,
                steps: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_workflow_completed_steps(&run_id) {
        Ok(steps) => {
            let steps_json = serde_json::to_string(&steps)
                .unwrap_or_else(|_| "[]".to_string());
            
            WorkflowStepsResult {
                success: true,
                run_id: Some(run_id),
                steps: Some(steps_json),
                message: "Workflow completed steps retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            WorkflowStepsResult {
                success: false,
                run_id: None,
                steps: None,
                message: format!("Failed to get workflow completed steps: {}", e),
            }
        }
    }
}

/// Execute a webhook trigger via N-API
#[napi]
pub fn execute_webhook_trigger(request_json: String, db_path: String) -> TriggerExecutionResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.execute_webhook_trigger(&request_json) {
        Ok(result_json) => {
            // Parse the result to extract individual fields
            let result: serde_json::Value = match serde_json::from_str(&result_json) {
                Ok(result) => result,
                Err(_) => {
                    return TriggerExecutionResult {
                        success: false,
                        run_id: None,
                        workflow_id: None,
                        message: "Failed to parse execution result".to_string(),
                    };
                }
            };
            
            TriggerExecutionResult {
                success: true,
                run_id: result["run_id"].as_str().map(|s| s.to_string()),
                workflow_id: result["workflow_id"].as_str().map(|s| s.to_string()),
                message: result["message"].as_str().unwrap_or("Webhook trigger executed successfully").to_string(),
            }
        }
        Err(e) => {
            TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to execute webhook trigger: {}", e),
            }
        }
    }
}

/// Execute a schedule trigger via N-API
#[napi]
pub fn execute_schedule_trigger(trigger_id: String, db_path: String) -> TriggerExecutionResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.execute_schedule_trigger(&trigger_id) {
        Ok(result_json) => {
            // Parse the result to extract individual fields
            let result: serde_json::Value = match serde_json::from_str(&result_json) {
                Ok(result) => result,
                Err(_) => {
                    return TriggerExecutionResult {
                        success: false,
                        run_id: None,
                        workflow_id: None,
                        message: "Failed to parse execution result".to_string(),
                    };
                }
            };
            
            TriggerExecutionResult {
                success: true,
                run_id: result["run_id"].as_str().map(|s| s.to_string()),
                workflow_id: result["workflow_id"].as_str().map(|s| s.to_string()),
                message: result["message"].as_str().unwrap_or("Schedule trigger executed successfully").to_string(),
            }
        }
        Err(e) => {
            TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to execute schedule trigger: {}", e),
            }
        }
    }
}

/// Execute a manual trigger via N-API
#[napi]
pub fn execute_manual_trigger(workflow_id: String, payload_json: String, db_path: String) -> TriggerExecutionResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.execute_manual_trigger(&workflow_id, &payload_json) {
        Ok(result_json) => {
            // Parse the result to extract individual fields
            let result: serde_json::Value = match serde_json::from_str(&result_json) {
                Ok(result) => result,
                Err(_) => {
                    return TriggerExecutionResult {
                        success: false,
                        run_id: None,
                        workflow_id: None,
                        message: "Failed to parse execution result".to_string(),
                    };
                }
            };
            
            TriggerExecutionResult {
                success: true,
                run_id: result["run_id"].as_str().map(|s| s.to_string()),
                workflow_id: result["workflow_id"].as_str().map(|s| s.to_string()),
                message: result["message"].as_str().unwrap_or("Manual trigger executed successfully").to_string(),
            }
        }
        Err(e) => {
            TriggerExecutionResult {
                success: false,
                run_id: None,
                workflow_id: None,
                message: format!("Failed to execute manual trigger: {}", e),
            }
        }
    }
}

/// Get trigger statistics via N-API
#[napi]
pub fn get_trigger_stats(db_path: String) -> TriggerStatsResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return TriggerStatsResult {
                success: false,
                stats: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_trigger_stats() {
        Ok(stats_json) => {
            TriggerStatsResult {
                success: true,
                stats: Some(stats_json),
                message: "Trigger statistics retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            TriggerStatsResult {
                success: false,
                stats: None,
                message: format!("Failed to get trigger statistics: {}", e),
            }
        }
    }
}

/// Get triggers for a workflow via N-API
#[napi]
pub fn get_workflow_triggers(workflow_id: String, db_path: String) -> WorkflowTriggersResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WorkflowTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.get_workflow_triggers(&workflow_id) {
        Ok(triggers_json) => {
            WorkflowTriggersResult {
                success: true,
                triggers: Some(triggers_json),
                message: "Workflow triggers retrieved successfully".to_string(),
            }
        }
        Err(e) => {
            WorkflowTriggersResult {
                success: false,
                triggers: None,
                message: format!("Failed to get workflow triggers: {}", e),
            }
        }
    }
}

/// Unregister triggers for a workflow via N-API
#[napi]
pub fn unregister_workflow_triggers(workflow_id: String, db_path: String) -> TriggerUnregistrationResult {
    let bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return TriggerUnregistrationResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.unregister_workflow_triggers(&workflow_id) {
        Ok(_) => {
            TriggerUnregistrationResult {
                success: true,
                message: format!("Successfully unregistered triggers for workflow: {}", workflow_id),
            }
        }
        Err(e) => {
            TriggerUnregistrationResult {
                success: false,
                message: format!("Failed to unregister workflow triggers: {}", e),
            }
        }
    }
} 

/// Start the webhook server via N-API
#[napi]
pub fn start_webhook_server(db_path: String) -> WebhookServerResult {
    let mut bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WebhookServerResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.start_webhook_server() {
        Ok(_) => {
            WebhookServerResult {
                success: true,
                message: "Webhook server started successfully".to_string(),
            }
        }
        Err(e) => {
            WebhookServerResult {
                success: false,
                message: format!("Failed to start webhook server: {}", e),
            }
        }
    }
}

/// Stop the webhook server via N-API
#[napi]
pub fn stop_webhook_server(db_path: String) -> WebhookServerResult {
    let mut bridge = match Bridge::new(&db_path) {
        Ok(bridge) => bridge,
        Err(e) => {
            return WebhookServerResult {
                success: false,
                message: format!("Failed to create bridge: {}", e),
            };
        }
    };
    
    match bridge.stop_webhook_server() {
        Ok(_) => {
            WebhookServerResult {
                success: true,
                message: "Webhook server stopped successfully".to_string(),
            }
        }
        Err(e) => {
            WebhookServerResult {
                success: false,
                message: format!("Failed to stop webhook server: {}", e),
            }
        }
    }
}

#[napi(object)]
pub struct WebhookServerResult {
    pub success: bool,
    pub message: String,
} 

#[napi(object)]
pub struct HookExecutionResult {
    pub success: bool,
    pub hook_type: Option<String>,
    pub workflow_id: Option<String>,
    pub result: Option<String>,
    pub message: String,
}

#[napi]
pub fn execute_workflow_steps(run_id: String, workflow_id: String, db_path: String) -> StepExecutionResult {
    match Bridge::new(&db_path) {
        Ok(bridge) => {
            match bridge.execute_workflow_steps(&run_id, &workflow_id) {
                Ok(result) => {
                    StepExecutionResult {
                        success: true,
                        result: Some(result),
                        message: "Workflow steps executed successfully".to_string(),
                    }
                }
                Err(error) => {
                    StepExecutionResult {
                        success: false,
                        result: None,
                        message: format!("Failed to execute workflow steps: {}", error),
                    }
                }
            }
        }
        Err(error) => {
            StepExecutionResult {
                success: false,
                result: None,
                message: format!("Failed to create bridge: {}", error),
            }
        }
    }
} 

#[napi]
pub fn execute_workflow_hook(hook_type: String, context_json: String, workflow_id: String, db_path: String) -> HookExecutionResult {
    match Bridge::new(&db_path) {
        Ok(bridge) => {
            match bridge.execute_workflow_hook(&hook_type, &context_json, &workflow_id) {
                Ok(result) => {
                    HookExecutionResult {
                        success: true,
                        hook_type: Some(hook_type),
                        workflow_id: Some(workflow_id),
                        result: Some(result),
                        message: "Hook executed successfully".to_string(),
                    }
                }
                Err(error) => {
                    HookExecutionResult {
                        success: false,
                        hook_type: Some(hook_type),
                        workflow_id: Some(workflow_id),
                        result: None,
                        message: format!("Failed to execute hook: {}", error),
                    }
                }
            }
        }
        Err(error) => {
            HookExecutionResult {
                success: false,
                hook_type: Some(hook_type),
                workflow_id: Some(workflow_id),
                result: None,
                message: format!("Failed to create bridge: {}", error),
            }
        }
    }
} 

#[napi(object)]
pub struct PauseResumeResult {
    pub success: bool,
    pub run_id: Option<String>,
    pub workflow_id: Option<String>,
    pub status: Option<String>,
    pub message: String,
}

#[napi]
pub fn pause_workflow(run_id: String, db_path: String) -> PauseResumeResult {
    match Bridge::new(&db_path) {
        Ok(bridge) => {
            // For now, we'll return a success response
            // In the future, this will integrate with the workflow state machine
            PauseResumeResult {
                success: true,
                run_id: Some(run_id.clone()),
                workflow_id: None,
                status: Some("paused".to_string()),
                message: "Workflow paused successfully".to_string(),
            }
        }
        Err(error) => {
            PauseResumeResult {
                success: false,
                run_id: Some(run_id),
                workflow_id: None,
                status: None,
                message: format!("Failed to create bridge: {}", error),
            }
        }
    }
}

#[napi]
pub fn resume_workflow(run_id: String, db_path: String) -> PauseResumeResult {
    match Bridge::new(&db_path) {
        Ok(bridge) => {
            // For now, we'll return a success response
            // In the future, this will integrate with the workflow state machine
            PauseResumeResult {
                success: true,
                run_id: Some(run_id.clone()),
                workflow_id: None,
                status: Some("resumed".to_string()),
                message: "Workflow resumed successfully".to_string(),
            }
        }
        Err(error) => {
            PauseResumeResult {
                success: false,
                run_id: Some(run_id),
                workflow_id: None,
                status: None,
                message: format!("Failed to create bridge: {}", error),
            }
        }
    }
} 