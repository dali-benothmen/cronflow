//! Node-Cronflow SDK
//!
//! This is the Node.js SDK that provides the developer-friendly API
//! for defining and managing workflows.

// Export the main cronflow singleton
export { Cronflow, VERSION } from './src/cronflow';

// Export types
export type {
  WorkflowDefinition,
  StepDefinition,
  TriggerDefinition,
  RetryConfig,
  Context,
} from './src/workflow';

// Export the WorkflowInstance class for direct usage
export { WorkflowInstance } from './src/workflow';

// Create and export the singleton instance
import { Cronflow } from './src/cronflow';
export const cronflow = new Cronflow();
