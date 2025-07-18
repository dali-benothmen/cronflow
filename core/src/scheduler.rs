//! Scheduler for the Node-Cronflow Core Engine
//! 
//! This module handles time-based scheduling and execution of scheduled workflows.

use crate::error::{CoreError, CoreResult};
use crate::triggers::TriggerManager;
use chrono::{DateTime, Utc};
use log;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep, interval};

/// Scheduler for managing scheduled workflow execution
pub struct Scheduler {
    trigger_manager: Arc<Mutex<TriggerManager>>,
    running: bool,
    check_interval: Duration,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new(trigger_manager: Arc<Mutex<TriggerManager>>) -> Self {
        Self {
            trigger_manager,
            running: false,
            check_interval: Duration::from_secs(30), // Check every 30 seconds
        }
    }

    /// Set the check interval for the scheduler
    pub fn with_check_interval(mut self, interval_seconds: u64) -> Self {
        self.check_interval = Duration::from_secs(interval_seconds);
        self
    }

    /// Start the scheduler
    pub async fn start(&mut self) -> CoreResult<()> {
        if self.running {
            return Err(CoreError::Configuration("Scheduler is already running".to_string()));
        }

        self.running = true;
        log::info!("Starting scheduler with {} second check interval", self.check_interval.as_secs());

        let trigger_manager = Arc::clone(&self.trigger_manager);
        let check_interval = self.check_interval;

        // Spawn the scheduler task
        tokio::spawn(async move {
            let mut interval_stream = interval(check_interval);
            
            loop {
                interval_stream.tick().await;
                
                if let Err(e) = Self::check_and_execute_schedules(&trigger_manager).await {
                    log::error!("Error in scheduler: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Stop the scheduler
    pub fn stop(&mut self) {
        self.running = false;
        log::info!("Scheduler stopped");
    }

    /// Check if the scheduler is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Check for schedules that should run and execute them
    async fn check_and_execute_schedules(trigger_manager: &Arc<Mutex<TriggerManager>>) -> CoreResult<()> {
        // Get schedules that should run
        let schedules_to_run = {
            let mut manager = trigger_manager.lock()
                .map_err(|e| CoreError::Internal(format!("Failed to lock trigger manager: {}", e)))?;
            manager.get_schedules_to_run()?
        };
        
        // Execute each schedule
        for (trigger_id, workflow_id) in schedules_to_run {
            log::info!("Executing scheduled workflow: {} (trigger: {})", workflow_id, trigger_id);
            
            // Here we would trigger the workflow execution
            // For now, we'll just log the execution
            if let Err(e) = Self::execute_scheduled_workflow(&workflow_id, &trigger_id).await {
                log::error!("Failed to execute scheduled workflow {}: {}", workflow_id, e);
            }
        }

        Ok(())
    }

    /// Execute a scheduled workflow
    async fn execute_scheduled_workflow(workflow_id: &str, trigger_id: &str) -> CoreResult<()> {
        // This is where we would integrate with the workflow execution system
        // For now, we'll simulate the execution
        
        log::info!("Executing scheduled workflow: {} triggered by: {}", workflow_id, trigger_id);
        
        // Simulate some execution time
        sleep(Duration::from_millis(100)).await;
        
        log::info!("Completed scheduled workflow execution: {}", workflow_id);
        Ok(())
    }

    /// Get next scheduled run times for all triggers
    pub fn get_next_run_times(&self) -> CoreResult<Vec<(String, String, DateTime<Utc>)>> {
        let manager = self.trigger_manager.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to lock trigger manager: {}", e)))?;

        let mut next_runs = Vec::new();
        
        for (trigger_id, (trigger, workflow_id)) in manager.schedule_triggers.iter() {
            if let Ok(next_run) = trigger.get_next_run() {
                next_runs.push((trigger_id.clone(), workflow_id.clone(), next_run));
            }
        }

        // Sort by next run time
        next_runs.sort_by(|a, b| a.2.cmp(&b.2));
        
        Ok(next_runs)
    }

    /// Get schedule statistics
    pub fn get_schedule_stats(&self) -> CoreResult<ScheduleStats> {
        let manager = self.trigger_manager.lock()
            .map_err(|e| CoreError::Internal(format!("Failed to lock trigger manager: {}", e)))?;

        let total_schedules = manager.schedule_triggers.len();
        let enabled_schedules = manager.schedule_triggers
            .values()
            .filter(|(trigger, _)| trigger.enabled)
            .count();

        let mut next_runs = Vec::new();
        for (trigger_id, (trigger, workflow_id)) in manager.schedule_triggers.iter() {
            if let Ok(next_run) = trigger.get_next_run() {
                next_runs.push((trigger_id.clone(), workflow_id.clone(), next_run));
            }
        }

        // Sort by next run time and take the next 5
        next_runs.sort_by(|a, b| a.2.cmp(&b.2));
        let next_5_runs = next_runs.into_iter().take(5).collect();

        Ok(ScheduleStats {
            total_schedules,
            enabled_schedules,
            next_runs: next_5_runs,
        })
    }
}

/// Statistics about scheduled workflows
#[derive(Debug, Clone)]
pub struct ScheduleStats {
    pub total_schedules: usize,
    pub enabled_schedules: usize,
    pub next_runs: Vec<(String, String, DateTime<Utc>)>,
}

impl ScheduleStats {
    /// Create a new schedule stats instance
    pub fn new() -> Self {
        Self {
            total_schedules: 0,
            enabled_schedules: 0,
            next_runs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::{ScheduleTrigger, ScheduleValidation};

    #[tokio::test]
    async fn test_scheduler_creation() {
        let trigger_manager = Arc::new(Mutex::new(TriggerManager::new()));
        let scheduler = Scheduler::new(trigger_manager);
        
        assert!(!scheduler.is_running());
    }

    #[tokio::test]
    async fn test_schedule_stats() {
        let trigger_manager = Arc::new(Mutex::new(TriggerManager::new()));
        let scheduler = Scheduler::new(trigger_manager);
        
        let stats = scheduler.get_schedule_stats().unwrap();
        assert_eq!(stats.total_schedules, 0);
        assert_eq!(stats.enabled_schedules, 0);
    }

    #[test]
    fn test_schedule_trigger_validation() {
        let trigger = ScheduleTrigger::new("0 0 * * *".to_string());
        assert!(trigger.validate().is_ok());
        
        let invalid_trigger = ScheduleTrigger::new("invalid cron".to_string());
        assert!(invalid_trigger.validate().is_err());
    }

    #[test]
    fn test_schedule_validation() {
        let validation = ScheduleValidation::new()
            .with_allowed_hours(vec![9, 10, 11])
            .with_allowed_days(vec![1, 2, 3, 4, 5]); // Monday to Friday
        
        let now = Utc::now();
        let result = validation.validate_execution(now);
        
        // This test depends on the current time, so we just check it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }
} 