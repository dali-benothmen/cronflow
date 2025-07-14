//! Node-Cronflow SDK
//!
//! This is the Node.js SDK that provides the developer-friendly API
//! for defining and managing workflows.

import { z } from 'zod';

// SDK version
export const VERSION = '0.1.0';

// Core types and interfaces
export interface WorkflowDefinition {
  id: string;
  name: string;
  description?: string;
  steps: StepDefinition[];
  triggers: TriggerDefinition[];
  created_at: Date;
  updated_at: Date;
}

export interface StepDefinition {
  id: string;
  name: string;
  action: string;
  timeout?: number;
  retry?: RetryConfig;
  depends_on: string[];
}

export interface RetryConfig {
  max_attempts: number;
  backoff_ms: number;
}

export type TriggerDefinition =
  | { type: 'webhook'; path: string; method: string }
  | { type: 'schedule'; cron_expression: string }
  | { type: 'manual' };

// Zod schemas for validation
export const WorkflowDefinitionSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().optional(),
  steps: z.array(
    z.object({
      id: z.string(),
      name: z.string(),
      action: z.string(),
      timeout: z.number().optional(),
      retry: z
        .object({
          max_attempts: z.number(),
          backoff_ms: z.number(),
        })
        .optional(),
      depends_on: z.array(z.string()),
    })
  ),
  triggers: z.array(
    z.union([
      z.object({
        type: z.literal('webhook'),
        path: z.string(),
        method: z.string(),
      }),
      z.object({ type: z.literal('schedule'), cron_expression: z.string() }),
      z.object({ type: z.literal('manual') }),
    ])
  ),
  created_at: z.date(),
  updated_at: z.date(),
});

// Main cronflow singleton
class Cronflow {
  private workflows: Map<string, WorkflowDefinition> = new Map();
  private engineState: 'STOPPED' | 'STARTING' | 'STARTED' = 'STOPPED';

  constructor() {
    console.log(`Node-Cronflow SDK v${VERSION} initialized`);
  }

  /**
   * Define a new workflow
   */
  define(
    id: string,
    definition: Omit<WorkflowDefinition, 'id' | 'created_at' | 'updated_at'>
  ): WorkflowInstance {
    const now = new Date();
    const workflow: WorkflowDefinition = {
      ...definition,
      id,
      created_at: now,
      updated_at: now,
    };

    // Validate the workflow definition
    WorkflowDefinitionSchema.parse(workflow);

    this.workflows.set(id, workflow);
    console.log(`Workflow '${id}' defined successfully`);

    return new WorkflowInstance(workflow, this);
  }

  /**
   * Start the cronflow engine
   */
  async start(): Promise<void> {
    if (this.engineState === 'STARTED' || this.engineState === 'STARTING') {
      return Promise.resolve();
    }

    this.engineState = 'STARTING';
    console.log('Starting Node-Cronflow engine...');

    // TODO: Initialize Rust core engine
    // TODO: Register workflows with the engine

    this.engineState = 'STARTED';
    console.log('Node-Cronflow engine started successfully');
  }

  /**
   * Get all defined workflows
   */
  getWorkflows(): WorkflowDefinition[] {
    return Array.from(this.workflows.values());
  }

  /**
   * Get a specific workflow by ID
   */
  getWorkflow(id: string): WorkflowDefinition | undefined {
    return this.workflows.get(id);
  }
}

// Workflow instance for fluent API
class WorkflowInstance {
  constructor(
    private workflow: WorkflowDefinition,
    private cronflow: Cronflow
  ) {}

  /**
   * Start this specific workflow
   */
  async start(): Promise<void> {
    return this.cronflow.start();
  }

  /**
   * Get the workflow definition
   */
  getDefinition(): WorkflowDefinition {
    return this.workflow;
  }
}

// Export the singleton instance
export const cronflow = new Cronflow();

// Export types
export type { Cronflow, WorkflowInstance };
