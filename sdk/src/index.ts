// ============================================================================
// CRONFLOW SDK - MAIN ENTRY POINT
// ============================================================================

import { WorkflowInstance } from './workflow/instance';
import { WorkflowDefinition } from './workflow/types';
import * as cronflowFunctions from './cronflow';

// ============================================================================
// FUNCTIONAL CRONFLOW API
// ============================================================================

/**
 * Main cronflow object with functional API
 */
export const cronflow = {
  /**
   * Define a new workflow
   */
  define: cronflowFunctions.define,

  /**
   * Start the cronflow engine
   */
  start: cronflowFunctions.start,

  /**
   * Stop the cronflow engine
   */
  stop: cronflowFunctions.stop,

  /**
   * Trigger a workflow manually
   */
  trigger: cronflowFunctions.trigger,

  /**
   * Inspect a workflow run
   */
  inspect: cronflowFunctions.inspect,

  /**
   * Execute manual trigger
   */
  executeManualTrigger: cronflowFunctions.executeManualTrigger,

  /**
   * Execute webhook trigger
   */
  executeWebhookTrigger: cronflowFunctions.executeWebhookTrigger,

  /**
   * Execute schedule trigger
   */
  executeScheduleTrigger: cronflowFunctions.executeScheduleTrigger,

  /**
   * Get trigger statistics
   */
  getTriggerStats: cronflowFunctions.getTriggerStats,

  /**
   * Get workflow triggers
   */
  getWorkflowTriggers: cronflowFunctions.getWorkflowTriggers,

  /**
   * Unregister workflow triggers
   */
  unregisterWorkflowTriggers: cronflowFunctions.unregisterWorkflowTriggers,

  /**
   * Get schedule triggers
   */
  getScheduleTriggers: cronflowFunctions.getScheduleTriggers,

  /**
   * Get all workflows
   */
  getWorkflows: cronflowFunctions.getWorkflows,

  /**
   * Get a specific workflow
   */
  getWorkflow: cronflowFunctions.getWorkflow,

  /**
   * Get engine state
   */
  getEngineState: cronflowFunctions.getEngineState,

  /**
   * Check if Rust core is available
   */
  isRustCoreAvailable: cronflowFunctions.isRustCoreAvailable,

  /**
   * Replay a workflow run
   */
  replay: cronflowFunctions.replay,

  /**
   * Resume a workflow
   */
  resume: cronflowFunctions.resume,

  /**
   * Cancel a workflow run
   */
  cancelRun: cronflowFunctions.cancelRun,

  /**
   * Publish an event
   */
  publishEvent: cronflowFunctions.publishEvent,
};

// ============================================================================
// TYPE EXPORTS
// ============================================================================

export type { WorkflowDefinition } from './workflow/types';
export type { Context } from './workflow/types';
export type { StepDefinition, StepOptions } from './workflow/types';
export type { TriggerDefinition } from './workflow/types';

// ============================================================================
// UTILITY EXPORTS
// ============================================================================

export * from './workflow';
