import {
  WorkflowInstance,
  WorkflowDefinition,
  Context,
  WorkflowDefinitionSchema,
} from './workflow';

// Import the Rust addon
let core: any;
try {
  core = require('../../core/core.node');
} catch (error) {
  console.warn('⚠️  Rust core not available, running in simulation mode');
  core = null;
}

export const VERSION = '0.1.0';

export class Cronflow {
  private workflows: Map<string, WorkflowDefinition> = new Map();
  private engineState: 'STOPPED' | 'STARTING' | 'STARTED' = 'STOPPED';
  private dbPath: string;

  constructor(dbPath?: string) {
    this.dbPath = dbPath || './cronflow.db';
    // eslint-disable-next-line no-console
    console.log(`Node-Cronflow SDK v${VERSION} initialized`);
  }

  define(
    options: Omit<
      WorkflowDefinition,
      'steps' | 'triggers' | 'created_at' | 'updated_at'
    >
  ): WorkflowInstance {
    if (!options.id || options.id.trim() === '') {
      throw new Error('Workflow ID cannot be empty');
    }

    if (this.workflows.has(options.id)) {
      throw new Error(`Workflow with ID '${options.id}' already exists`);
    }

    const workflow: WorkflowDefinition = {
      ...options,
      steps: [],
      triggers: [],
      created_at: new Date(),
      updated_at: new Date(),
    };

    this.workflows.set(options.id, workflow);
    // eslint-disable-next-line no-console
    console.log(`Workflow '${options.id}' defined successfully`);

    return new WorkflowInstance(workflow, this);
  }

  async start(): Promise<void> {
    if (this.engineState === 'STARTED' || this.engineState === 'STARTING') {
      return Promise.resolve();
    }

    this.engineState = 'STARTING';
    // eslint-disable-next-line no-console
    console.log('Starting Node-Cronflow engine...');

    if (core) {
      try {
        for (const workflow of this.workflows.values()) {
          await this.registerWorkflowWithRust(workflow);
        }
        console.log('✅ All workflows registered with Rust engine');
      } catch (error) {
        console.error(
          '❌ Failed to register workflows with Rust engine:',
          error
        );
        throw error;
      }
    } else {
      console.log('⚠️  Running in simulation mode (Rust core not available)');
    }

    this.engineState = 'STARTED';
    // eslint-disable-next-line no-console
    console.log('Node-Cronflow engine started successfully');
  }

  async stop(): Promise<void> {
    this.engineState = 'STOPPED';
    // eslint-disable-next-line no-console
    console.log('Node-Cronflow engine stopped');
  }

  async trigger(workflowId: string, payload: any): Promise<string> {
    if (!core) {
      console.log(
        `⚠️  Simulation: Triggering workflow: ${workflowId} with payload:`,
        payload
      );
      return 'simulation-run-id';
    }

    try {
      const payloadJson = JSON.stringify(payload);
      const result = core.createRun(workflowId, payloadJson, this.dbPath);

      if (result.success && result.runId) {
        console.log(
          `✅ Created run ${result.runId} for workflow ${workflowId}`
        );
        return result.runId;
      } else if (result.success && !result.runId) {
        console.warn(
          `⚠️  Rust engine returned success but no runId for workflow ${workflowId}`
        );
        console.warn(`Message: ${result.message}`);
        throw new Error(`Rust engine error: ${result.message}`);
      } else {
        throw new Error(`Failed to create run: ${result.message}`);
      }
    } catch (error) {
      console.error(`❌ Failed to trigger workflow ${workflowId}:`, error);
      throw error;
    }
  }

  async inspect(runId: string): Promise<any> {
    if (!core) {
      console.log(`⚠️  Simulation: Inspecting run: ${runId}`);
      return { status: 'simulation', runId };
    }

    try {
      const result = core.getRunStatus(runId, this.dbPath);

      if (result.success) {
        return JSON.parse(result.status || '{}');
      } else {
        throw new Error(`Failed to get run status: ${result.message}`);
      }
    } catch (error) {
      console.error(`❌ Failed to inspect run ${runId}:`, error);
      throw error;
    }
  }

  async cancelRun(runId: string): Promise<void> {
    // TODO: Implement run cancellation in Rust engine
    console.log(`Cancelling run: ${runId}`);
  }

  async publishEvent(name: string, payload: any): Promise<void> {
    // TODO: Implement event publishing
    console.log(`Publishing event: ${name} with payload:`, payload);
  }

  getWorkflows(): WorkflowDefinition[] {
    return Array.from(this.workflows.values());
  }

  getWorkflow(id: string): WorkflowDefinition | undefined {
    return this.workflows.get(id);
  }

  async registerWorkflow(workflow: WorkflowDefinition): Promise<void> {
    WorkflowDefinitionSchema.parse(workflow);
    this.workflows.set(workflow.id, workflow);

    if (core && this.engineState === 'STARTED') {
      await this.registerWorkflowWithRust(workflow);
    }
  }

  private async registerWorkflowWithRust(
    workflow: WorkflowDefinition
  ): Promise<void> {
    if (!core) {
      throw new Error('Rust core not available');
    }

    try {
      const rustWorkflow = this.convertToRustFormat(workflow);

      const workflowJson = JSON.stringify(rustWorkflow);

      const result = core.registerWorkflow(workflowJson, this.dbPath);

      if (result.success) {
        console.log(`✅ Workflow '${workflow.id}' registered with Rust engine`);
      } else {
        throw new Error(`Failed to register workflow: ${result.message}`);
      }
    } catch (error) {
      console.error(
        `❌ Failed to register workflow '${workflow.id}' with Rust engine:`,
        error
      );
      throw error;
    }
  }

  private convertToRustFormat(workflow: WorkflowDefinition): any {
    const rustSteps = workflow.steps.map(step => ({
      id: step.id,
      name: step.name,
      action: step.name, // Use step name as action for now
      timeout: step.options?.timeout
        ? this.parseDuration(step.options.timeout)
        : undefined,
      retry: step.options?.retry
        ? {
            max_attempts: step.options.retry.attempts,
            backoff_ms: this.parseDuration(step.options.retry.backoff.delay),
          }
        : undefined,
      depends_on: [], // TODO: Implement dependency tracking
    }));

    const rustTriggers = workflow.triggers.map(trigger => {
      switch (trigger.type) {
        case 'webhook':
          return {
            Webhook: {
              path: trigger.path,
              method: trigger.options?.method || 'POST',
            },
          };
        case 'schedule':
          return {
            Schedule: {
              cron_expression: trigger.cron_expression,
            },
          };
        case 'manual':
          return 'Manual';
        default:
          throw new Error(`Unknown trigger type: ${(trigger as any).type}`);
      }
    });

    return {
      id: workflow.id,
      name: workflow.name || workflow.id,
      description: workflow.description,
      steps: rustSteps,
      triggers: rustTriggers,
      created_at: workflow.created_at.toISOString(),
      updated_at: workflow.updated_at.toISOString(),
    };
  }

  private parseDuration(duration: string | number): number {
    if (typeof duration === 'number') {
      return duration;
    }

    const match = duration.match(/^(\d+)([smhd])$/);
    if (!match) {
      throw new Error(`Invalid duration format: ${duration}`);
    }

    const [, amount, unit] = match;
    const num = parseInt(amount);

    switch (unit) {
      case 's':
        return num * 1000; // seconds to milliseconds
      case 'm':
        return num * 60 * 1000; // minutes to milliseconds
      case 'h':
        return num * 60 * 60 * 1000; // hours to milliseconds
      case 'd':
        return num * 24 * 60 * 60 * 1000; // days to milliseconds
      default:
        throw new Error(`Unknown duration unit: ${unit}`);
    }
  }

  getState(): 'STOPPED' | 'STARTING' | 'STARTED' {
    return this.engineState;
  }

  async replay(
    runId: string,
    options?: {
      overridePayload?: any;
      mockStep?: (stepName: string, mockFn: (ctx: Context) => any) => void;
    }
  ): Promise<void> {
    // TODO: Implement replay functionality
    console.log(`Replaying run: ${runId} with options:`, options);
  }

  async resume(token: string, payload: any): Promise<void> {
    // TODO: Implement resume functionality
    console.log(`Resuming workflow with token: ${token} and payload:`, payload);
  }

  isRustCoreAvailable(): boolean {
    return core !== null;
  }
}
