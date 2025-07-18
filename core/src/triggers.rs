//! Trigger system for the Node-Cronflow Core Engine
//! 
//! This module handles different types of triggers that can start workflow execution:
//! - Webhook triggers (HTTP endpoints)
//! - Schedule triggers (cron-based)
//! - Manual triggers (programmatic)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::error::{CoreError, CoreResult};
use log;
use chrono::{DateTime, Utc, Datelike, Timelike};
use cron::Schedule;
use std::str::FromStr;

/// Webhook trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTrigger {
    pub path: String,
    pub method: String,
    pub headers: Option<HashMap<String, String>>,
    pub validation: Option<WebhookValidation>,
}

impl WebhookTrigger {
    /// Create a new webhook trigger
    pub fn new(path: String, method: String) -> Self {
        Self {
            path,
            method: method.to_uppercase(),
            headers: None,
            validation: None,
        }
    }

    /// Add headers to the webhook trigger
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Add validation to the webhook trigger
    pub fn with_validation(mut self, validation: WebhookValidation) -> Self {
        self.validation = Some(validation);
        self
    }

    /// Validate the webhook trigger configuration
    pub fn validate(&self) -> CoreResult<()> {
        if self.path.is_empty() {
            return Err(CoreError::InvalidTrigger("Webhook path cannot be empty".to_string()));
        }

        if !self.path.starts_with('/') {
            return Err(CoreError::InvalidTrigger("Webhook path must start with /".to_string()));
        }

        let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];
        if !valid_methods.contains(&self.method.as_str()) {
            return Err(CoreError::InvalidTrigger(format!("Invalid HTTP method: {}", self.method)));
        }

        Ok(())
    }
}

/// Webhook validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookValidation {
    pub secret: Option<String>,
    pub signature_header: Option<String>,
    pub signature_algorithm: Option<String>,
    pub required_fields: Option<Vec<String>>,
}

impl WebhookValidation {
    /// Create a new webhook validation configuration
    pub fn new() -> Self {
        Self {
            secret: None,
            signature_header: None,
            signature_algorithm: None,
            required_fields: None,
        }
    }

    /// Add secret for signature validation
    pub fn with_secret(mut self, secret: String) -> Self {
        self.secret = Some(secret);
        self
    }

    /// Add signature header configuration
    pub fn with_signature_header(mut self, header: String, algorithm: String) -> Self {
        self.signature_header = Some(header);
        self.signature_algorithm = Some(algorithm);
        self
    }

    /// Add required fields validation
    pub fn with_required_fields(mut self, fields: Vec<String>) -> Self {
        self.required_fields = Some(fields);
        self
    }
}

/// Webhook request payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub query_params: HashMap<String, String>,
}

impl WebhookRequest {
    /// Create a new webhook request
    pub fn new(method: String, path: String) -> Self {
        Self {
            method: method.to_uppercase(),
            path,
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        }
    }

    /// Add headers to the request
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Add body to the request
    pub fn with_body(mut self, body: String) -> Self {
        self.body = Some(body);
        self
    }

    /// Add query parameters to the request
    pub fn with_query_params(mut self, params: HashMap<String, String>) -> Self {
        self.query_params = params;
        self
    }

    /// Validate the webhook request
    pub fn validate(&self) -> CoreResult<()> {
        if self.path.is_empty() {
            return Err(CoreError::InvalidTrigger("Request path cannot be empty".to_string()));
        }

        let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];
        if !valid_methods.contains(&self.method.as_str()) {
            return Err(CoreError::InvalidTrigger(format!("Invalid HTTP method: {}", self.method)));
        }

        Ok(())
    }
}

/// Webhook response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

impl WebhookResponse {
    /// Create a new webhook response
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code,
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Add headers to the response
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Add body to the response
    pub fn with_body(mut self, body: String) -> Self {
        self.body = Some(body);
        self
    }

    /// Create a success response
    pub fn success() -> Self {
        Self::new(200).with_body("{\"status\":\"success\"}".to_string())
    }

    /// Create an error response
    pub fn error(status_code: u16, message: String) -> Self {
        Self::new(status_code).with_body(format!("{{\"error\":\"{}\"}}", message))
    }
}

/// Schedule trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    pub cron_expression: String,
    pub timezone: Option<String>,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub validation: Option<ScheduleValidation>,
}

impl ScheduleTrigger {
    /// Create a new schedule trigger
    pub fn new(cron_expression: String) -> Self {
        Self {
            cron_expression,
            timezone: None,
            enabled: true,
            last_run: None,
            next_run: None,
            validation: None,
        }
    }

    /// Set timezone for the schedule
    pub fn with_timezone(mut self, timezone: String) -> Self {
        self.timezone = Some(timezone);
        self
    }

    /// Set enabled status
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add validation to the schedule trigger
    pub fn with_validation(mut self, validation: ScheduleValidation) -> Self {
        self.validation = Some(validation);
        self
    }

    /// Validate the schedule trigger configuration
    pub fn validate(&self) -> CoreResult<()> {
        if self.cron_expression.is_empty() {
            return Err(CoreError::InvalidTrigger("Cron expression cannot be empty".to_string()));
        }

        // Validate cron expression
        Schedule::from_str(&self.cron_expression)
            .map_err(|e| CoreError::InvalidTrigger(format!("Invalid cron expression: {}", e)))?;

        // Validate timezone if provided
        if let Some(ref tz) = self.timezone {
            chrono_tz::Tz::from_str(tz)
                .map_err(|e| CoreError::InvalidTrigger(format!("Invalid timezone: {}", e)))?;
        }

        Ok(())
    }

    /// Get the next run time for this schedule
    pub fn get_next_run(&self) -> CoreResult<DateTime<Utc>> {
        let schedule = Schedule::from_str(&self.cron_expression)
            .map_err(|e| CoreError::InvalidTrigger(format!("Invalid cron expression: {}", e)))?;

        let now = Utc::now();
        let next = schedule.after(&now).next()
            .ok_or_else(|| CoreError::InvalidTrigger("Could not calculate next run time".to_string()))?;

        Ok(next)
    }

    /// Check if the schedule should run now
    pub fn should_run(&self) -> CoreResult<bool> {
        if !self.enabled {
            return Ok(false);
        }

        let now = Utc::now();
        let next_run = self.get_next_run()?;
        
        // Check if we're within 1 minute of the scheduled time
        let diff = (now - next_run).num_seconds().abs();
        Ok(diff <= 60)
    }

    /// Update the last run time
    pub fn update_last_run(&mut self) {
        self.last_run = Some(Utc::now());
        // Recalculate next run time
        if let Ok(next) = self.get_next_run() {
            self.next_run = Some(next);
        }
    }
}

/// Schedule validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleValidation {
    pub max_runs_per_hour: Option<u32>,
    pub max_runs_per_day: Option<u32>,
    pub allowed_hours: Option<Vec<u8>>,
    pub allowed_days: Option<Vec<u8>>,
}

impl ScheduleValidation {
    /// Create a new schedule validation configuration
    pub fn new() -> Self {
        Self {
            max_runs_per_hour: None,
            max_runs_per_day: None,
            allowed_hours: None,
            allowed_days: None,
        }
    }

    /// Set maximum runs per hour
    pub fn with_max_runs_per_hour(mut self, max: u32) -> Self {
        self.max_runs_per_hour = Some(max);
        self
    }

    /// Set maximum runs per day
    pub fn with_max_runs_per_day(mut self, max: u32) -> Self {
        self.max_runs_per_day = Some(max);
        self
    }

    /// Set allowed hours (0-23)
    pub fn with_allowed_hours(mut self, hours: Vec<u8>) -> Self {
        self.allowed_hours = Some(hours);
        self
    }

    /// Set allowed days (0-6, where 0 is Sunday)
    pub fn with_allowed_days(mut self, days: Vec<u8>) -> Self {
        self.allowed_days = Some(days);
        self
    }

    /// Validate schedule execution
    pub fn validate_execution(&self, current_time: DateTime<Utc>) -> CoreResult<()> {
        // Check allowed hours
        if let Some(ref allowed_hours) = self.allowed_hours {
            let current_hour = current_time.hour() as u8;
            if !allowed_hours.contains(&current_hour) {
                return Err(CoreError::InvalidTrigger(
                    format!("Schedule execution not allowed at hour: {}", current_hour)
                ));
            }
        }

        // Check allowed days
        if let Some(ref allowed_days) = self.allowed_days {
            let current_day = current_time.weekday().num_days_from_sunday() as u8;
            if !allowed_days.contains(&current_day) {
                return Err(CoreError::InvalidTrigger(
                    format!("Schedule execution not allowed on day: {}", current_day)
                ));
            }
        }

        Ok(())
    }
}

/// Trigger manager for handling different trigger types
pub struct TriggerManager {
    pub webhook_triggers: HashMap<String, (WebhookTrigger, String)>, // path -> (trigger, workflow_id)
    pub schedule_triggers: HashMap<String, (ScheduleTrigger, String)>, // trigger_id -> (trigger, workflow_id)
}

impl TriggerManager {
    /// Create a new trigger manager
    pub fn new() -> Self {
        Self {
            webhook_triggers: HashMap::new(),
            schedule_triggers: HashMap::new(),
        }
    }

    /// Register a webhook trigger for a workflow
    pub fn register_webhook_trigger(&mut self, workflow_id: &str, trigger: WebhookTrigger) -> CoreResult<()> {
        log::info!("Registering webhook trigger for workflow: {} at path: {}", workflow_id, trigger.path);
        
        // Validate the trigger
        trigger.validate()?;
        
        // Check if path is already registered
        if self.webhook_triggers.contains_key(&trigger.path) {
            return Err(CoreError::InvalidTrigger(format!("Webhook path {} is already registered", trigger.path)));
        }
        
        // Register the trigger
        let path = trigger.path.clone();
        self.webhook_triggers.insert(path.clone(), (trigger, workflow_id.to_string()));
        
        log::info!("Successfully registered webhook trigger for workflow: {} at path: {}", workflow_id, path);
        Ok(())
    }

    /// Handle a webhook request
    pub fn handle_webhook_request(&self, request: WebhookRequest) -> CoreResult<(String, serde_json::Value)> {
        log::info!("Handling webhook request: {} {}", request.method, request.path);
        
        // Validate the request
        request.validate()?;
        
        // Find the trigger for this path
        let (trigger, workflow_id) = self.webhook_triggers.get(&request.path)
            .ok_or_else(|| CoreError::TriggerNotFound(format!("No webhook trigger found for path: {}", request.path)))?;
        
        // Validate method matches
        if trigger.method != request.method {
            return Err(CoreError::InvalidTrigger(format!(
                "Method mismatch: expected {}, got {}", trigger.method, request.method
            )));
        }
        
        // Validate webhook if configured
        if let Some(validation) = &trigger.validation {
            self.validate_webhook(&request, validation)?;
        }
        
        // Prepare payload for workflow
        let payload = self.prepare_workflow_payload(&request)?;
        
        log::info!("Webhook request validated, triggering workflow: {}", workflow_id);
        Ok((workflow_id.clone(), payload))
    }

    /// Validate webhook request based on validation rules
    fn validate_webhook(&self, request: &WebhookRequest, validation: &WebhookValidation) -> CoreResult<()> {
        // Check required fields if specified
        if let Some(required_fields) = &validation.required_fields {
            // Parse body as JSON to check for required fields
            if let Some(body) = &request.body {
                let body_json: serde_json::Value = serde_json::from_str(body)
                    .map_err(|e| CoreError::InvalidTrigger(format!("Invalid JSON body: {}", e)))?;
                
                for field in required_fields {
                    if !body_json.get(field).is_some() {
                        return Err(CoreError::InvalidTrigger(format!("Missing required field: {}", field)));
                    }
                }
            } else {
                return Err(CoreError::InvalidTrigger("Request body is required for field validation".to_string()));
            }
        }
        
        // TODO: Implement signature validation when needed
        // For now, we'll skip signature validation as it requires crypto libraries
        
        Ok(())
    }

    /// Prepare payload for workflow execution
    fn prepare_workflow_payload(&self, request: &WebhookRequest) -> CoreResult<serde_json::Value> {
        let mut payload = serde_json::json!({
            "method": request.method,
            "path": request.path,
            "headers": request.headers,
            "query_params": request.query_params,
        });
        
        // Add body if present
        if let Some(body) = &request.body {
            // Try to parse as JSON, fallback to string
            if let Ok(body_json) = serde_json::from_str::<serde_json::Value>(body) {
                payload["body"] = body_json;
            } else {
                payload["body"] = serde_json::Value::String(body.clone());
            }
        }
        
        Ok(payload)
    }

    /// Get all registered webhook triggers
    pub fn get_webhook_triggers(&self) -> Vec<(String, WebhookTrigger, String)> {
        self.webhook_triggers
            .iter()
            .map(|(path, (trigger, workflow_id))| (path.clone(), trigger.clone(), workflow_id.clone()))
            .collect()
    }

    /// Check if a webhook path is registered
    pub fn has_webhook_trigger(&self, path: &str) -> bool {
        self.webhook_triggers.contains_key(path)
    }

    /// Get workflow ID for a webhook path
    pub fn get_workflow_id_for_webhook(&self, path: &str) -> Option<&String> {
        self.webhook_triggers.get(path).map(|(_, workflow_id)| workflow_id)
    }

    /// Register a schedule trigger for a workflow
    pub fn register_schedule_trigger(&mut self, workflow_id: &str, trigger: ScheduleTrigger) -> CoreResult<String> {
        log::info!("Registering schedule trigger for workflow: {} with cron: {}", workflow_id, trigger.cron_expression);
        
        // Validate the trigger
        trigger.validate()?;
        
        // Generate unique trigger ID
        let trigger_id = format!("schedule_{}_{}", workflow_id, uuid::Uuid::new_v4().to_string().replace("-", ""));
        
        // Register the trigger
        self.schedule_triggers.insert(trigger_id.clone(), (trigger, workflow_id.to_string()));
        
        log::info!("Successfully registered schedule trigger for workflow: {} with ID: {}", workflow_id, trigger_id);
        Ok(trigger_id)
    }

    /// Get all schedule triggers that should run now
    pub fn get_schedules_to_run(&mut self) -> CoreResult<Vec<(String, String)>> {
        let mut to_run = Vec::new();
        let now = Utc::now();

        for (trigger_id, (trigger, workflow_id)) in self.schedule_triggers.iter_mut() {
            if let Ok(should_run) = trigger.should_run() {
                if should_run {
                    // Validate execution if validation is configured
                    if let Some(ref validation) = trigger.validation {
                        if let Err(e) = validation.validate_execution(now) {
                            log::warn!("Schedule validation failed for trigger {}: {}", trigger_id, e);
                            continue;
                        }
                    }

                    // Update last run time
                    trigger.update_last_run();
                    to_run.push((trigger_id.clone(), workflow_id.clone()));
                }
            }
        }

        Ok(to_run)
    }

    /// Get all schedule triggers
    pub fn get_schedule_triggers(&self) -> Vec<(String, ScheduleTrigger, String)> {
        self.schedule_triggers
            .iter()
            .map(|(trigger_id, (trigger, workflow_id))| {
                (trigger_id.clone(), trigger.clone(), workflow_id.clone())
            })
            .collect()
    }

    /// Check if a schedule trigger exists
    pub fn has_schedule_trigger(&self, trigger_id: &str) -> bool {
        self.schedule_triggers.contains_key(trigger_id)
    }

    /// Get workflow ID for a schedule trigger
    pub fn get_workflow_id_for_schedule(&self, trigger_id: &str) -> Option<&String> {
        self.schedule_triggers.get(trigger_id).map(|(_, workflow_id)| workflow_id)
    }

    /// Enable or disable a schedule trigger
    pub fn set_schedule_enabled(&mut self, trigger_id: &str, enabled: bool) -> CoreResult<()> {
        if let Some((trigger, _)) = self.schedule_triggers.get_mut(trigger_id) {
            trigger.enabled = enabled;
            log::info!("Schedule trigger {} {}abled", trigger_id, if enabled { "en" } else { "dis" });
            Ok(())
        } else {
            Err(CoreError::TriggerNotFound(format!("Schedule trigger not found: {}", trigger_id)))
        }
    }

    /// Update schedule trigger
    pub fn update_schedule_trigger(&mut self, trigger_id: &str, new_trigger: ScheduleTrigger) -> CoreResult<()> {
        if let Some((trigger, _)) = self.schedule_triggers.get_mut(trigger_id) {
            // Validate the new trigger
            new_trigger.validate()?;
            
            // Update the trigger
            *trigger = new_trigger;
            log::info!("Updated schedule trigger: {}", trigger_id);
            Ok(())
        } else {
            Err(CoreError::TriggerNotFound(format!("Schedule trigger not found: {}", trigger_id)))
        }
    }

    /// Remove a schedule trigger
    pub fn remove_schedule_trigger(&mut self, trigger_id: &str) -> CoreResult<()> {
        if self.schedule_triggers.remove(trigger_id).is_some() {
            log::info!("Removed schedule trigger: {}", trigger_id);
            Ok(())
        } else {
            Err(CoreError::TriggerNotFound(format!("Schedule trigger not found: {}", trigger_id)))
        }
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
} 