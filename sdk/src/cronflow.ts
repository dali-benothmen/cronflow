import {
  WorkflowInstance,
  WorkflowDefinition,
  Context,
  WorkflowDefinitionSchema,
} from './workflow';

export const VERSION = '0.1.0';

export class Cronflow {
  private workflows: Map<string, WorkflowDefinition> = new Map();
  private engineState: 'STOPPED' | 'STARTING' | 'STARTED' = 'STOPPED';

  constructor() {
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

    // TODO: Initialize Rust core engine
    // TODO: Register workflows with the engine

    this.engineState = 'STARTED';
    // eslint-disable-next-line no-console
    console.log('Node-Cronflow engine started successfully');
  }

  async stop(): Promise<void> {
    this.engineState = 'STOPPED';
    // eslint-disable-next-line no-console
    console.log('Node-Cronflow engine stopped');
  }

  async trigger(workflowId: string, payload: any): Promise<void> {
    // TODO: Implement workflow triggering
    console.log(`Triggering workflow: ${workflowId} with payload:`, payload);
  }

  async inspect(runId: string): Promise<any> {
    // TODO: Implement run inspection
    console.log(`Inspecting run: ${runId}`);
    return {};
  }

  async cancelRun(runId: string): Promise<void> {
    // TODO: Implement run cancellation
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
    // TODO: Register with Rust engine
    console.log(`Registering workflow: ${workflow.id}`);

    // For now, just validate and store
    WorkflowDefinitionSchema.parse(workflow);
    this.workflows.set(workflow.id, workflow);
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
}
