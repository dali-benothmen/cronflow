import {
  WorkflowDefinition,
  StepDefinition,
  TriggerDefinition,
  StepOptions,
  Context,
  RetryConfig,
  CacheConfig,
} from './types';
import { validateWorkflow } from './validation';
import { parseDuration } from '../utils';

export class WorkflowInstance {
  private _workflow: WorkflowDefinition;
  private _currentStep: StepDefinition | null = null;
  private _stepStack: StepDefinition[] = [];

  constructor(
    workflow: WorkflowDefinition,
    private readonly _cronflowInstance: any // Will be typed properly later
  ) {
    this._workflow = workflow;
  }

  step(
    name: string,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): WorkflowInstance {
    const step: StepDefinition = {
      id: name,
      name,
      handler: handlerFn,
      type: 'step',
      options,
    };

    this._workflow.steps.push(step);
    this._currentStep = step;
    this._stepStack.push(step);

    return this;
  }

  action(
    name: string,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): WorkflowInstance {
    const step: StepDefinition = {
      id: name,
      name,
      handler: handlerFn,
      type: 'action',
      options,
    };

    this._workflow.steps.push(step);
    this._currentStep = step;
    this._stepStack.push(step);

    return this;
  }

  retry(options: RetryConfig): WorkflowInstance {
    if (!this._currentStep) {
      throw new Error('No current step. Call .step() or .action() first.');
    }

    if (!this._currentStep.options) {
      this._currentStep.options = {};
    }

    this._currentStep.options.retry = options;
    return this;
  }

  timeout(duration: string | number): WorkflowInstance {
    if (!this._currentStep) {
      throw new Error('No current step. Call .step() or .action() first.');
    }

    if (!this._currentStep.options) {
      this._currentStep.options = {};
    }

    this._currentStep.options.timeout = duration;
    return this;
  }

  cache(config: CacheConfig): WorkflowInstance {
    if (!this._currentStep) {
      throw new Error('No current step. Call .step() or .action() first.');
    }

    if (!this._currentStep.options) {
      this._currentStep.options = {};
    }

    this._currentStep.options.cache = config;
    return this;
  }

  delay(duration: string | number): WorkflowInstance {
    if (!this._currentStep) {
      throw new Error('No current step. Call .step() or .action() first.');
    }

    if (!this._currentStep.options) {
      this._currentStep.options = {};
    }

    this._currentStep.options.delay = duration;
    return this;
  }

  onWebhook(path: string, options?: any): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'webhook',
      path,
      options,
    };

    this._workflow.triggers.push(trigger);
    return this;
  }

  onSchedule(cronExpression: string): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'schedule',
      cron_expression: cronExpression,
    };

    this._workflow.triggers.push(trigger);
    return this;
  }

  onInterval(interval: string): WorkflowInstance {
    const cronExpression = this._intervalToCron(interval);
    return this.onSchedule(cronExpression);
  }

  onEvent(eventName: string): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'manual', // For now, treat events as manual triggers
    };

    this._workflow.triggers.push(trigger);
    return this;
  }

  manual(): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'manual',
    };

    this._workflow.triggers.push(trigger);
    return this;
  }

  if(
    name: string,
    condition: (ctx: Context) => boolean | Promise<boolean>,
    options?: any
  ): WorkflowInstance {
    // TODO: Implement conditional logic
    return this;
  }

  parallel(
    steps: Array<(ctx: Context) => any | Promise<any>>
  ): WorkflowInstance {
    // TODO: Implement parallel execution
    return this;
  }

  race(steps: Array<(ctx: Context) => any | Promise<any>>): WorkflowInstance {
    // TODO: Implement race execution
    return this;
  }

  while(
    name: string,
    condition: (ctx: Context) => boolean | Promise<boolean>,
    iterationFn: (ctx: Context) => void
  ): WorkflowInstance {
    // TODO: Implement while loop
    return this;
  }

  cancel(reason?: string): WorkflowInstance {
    // TODO: Implement cancel
    return this;
  }

  sleep(duration: string | number): WorkflowInstance {
    return this.action('sleep', async ctx => {
      const ms =
        typeof duration === 'string' ? parseDuration(duration) : duration;
      await new Promise(resolve => setTimeout(resolve, ms));
    });
  }

  subflow(name: string, workflowId: string, input?: any): WorkflowInstance {
    // TODO: Implement subflow
    return this;
  }

  forEach(
    name: string,
    items: (ctx: Context) => any[] | Promise<any[]>,
    iterationFn: (item: any, flow: WorkflowInstance) => void
  ): WorkflowInstance {
    // TODO: Implement forEach
    return this;
  }

  batch(
    name: string,
    options: { items: (ctx: Context) => any[] | Promise<any[]>; size: number },
    batchFn: (batch: any[], flow: WorkflowInstance) => void
  ): WorkflowInstance {
    // TODO: Implement batch
    return this;
  }

  humanInTheLoop(options: {
    timeout: string;
    onPause: (token: string) => void;
    description: string;
  }): WorkflowInstance {
    // TODO: Implement human in the loop
    return this;
  }

  waitForEvent(eventName: string, timeout?: string): WorkflowInstance {
    // TODO: Implement wait for event
    return this;
  }

  endIf(): WorkflowInstance {
    // TODO: Implement endIf
    return this;
  }

  elseIf(
    name: string,
    condition: (ctx: Context) => boolean | Promise<boolean>
  ): WorkflowInstance {
    // TODO: Implement elseIf
    return this;
  }

  else(): WorkflowInstance {
    // TODO: Implement else
    return this;
  }

  onError(handlerFn: (ctx: Context) => any): WorkflowInstance {
    // TODO: Implement onError
    return this;
  }

  log(
    message: string | ((ctx: Context) => string),
    level?: 'info' | 'warn' | 'error'
  ): WorkflowInstance {
    return this.action('log', ctx => {
      const msg = typeof message === 'function' ? message(ctx) : message;
      console.log(`[${level || 'info'}] ${msg}`);
    });
  }

  validate(): void {
    validateWorkflow(this._workflow);
  }

  toJSON(): string {
    this.validate();
    return JSON.stringify(this._workflow, null, 2);
  }

  getDefinition(): WorkflowDefinition {
    return { ...this._workflow };
  }

  getId(): string {
    return this._workflow.id;
  }

  getName(): string {
    return this._workflow.name || this._workflow.id;
  }

  getSteps(): StepDefinition[] {
    return [...this._workflow.steps];
  }

  getTriggers(): TriggerDefinition[] {
    return [...this._workflow.triggers];
  }

  hasWebhookTriggers(): boolean {
    return this._workflow.triggers.some(t => t.type === 'webhook');
  }

  hasScheduleTriggers(): boolean {
    return this._workflow.triggers.some(t => t.type === 'schedule');
  }

  hasManualTriggers(): boolean {
    return this._workflow.triggers.some(t => t.type === 'manual');
  }

  getStep(id: string): StepDefinition | undefined {
    return this._workflow.steps.find(step => step.id === id);
  }

  async register(): Promise<void> {
    this.validate();

    // TODO: Register with Rust engine
    console.log(`Registering workflow: ${this._workflow.id}`);

    return Promise.resolve();
  }

  private _intervalToCron(interval: string): string {
    const match = interval.match(/^(\d+)([mhd])$/);
    if (!match) {
      throw new Error(
        `Invalid interval format: ${interval}. Expected format like "5m", "1h", "1d"`
      );
    }

    const [, amount, unit] = match;
    const num = parseInt(amount);

    switch (unit) {
      case 'm': // minutes
        return `*/${num} * * * *`;
      case 'h': // hours
        return `0 */${num} * * *`;
      case 'd': // days
        return `0 0 */${num} * *`;
      default:
        throw new Error(`Unsupported interval unit: ${unit}`);
    }
  }
}

export function createWorkflow(
  options: Omit<
    WorkflowDefinition,
    'steps' | 'triggers' | 'created_at' | 'updated_at'
  >,
  cronflowInstance: any
): WorkflowInstance {
  const workflow: WorkflowDefinition = {
    ...options,
    steps: [],
    triggers: [],
    created_at: new Date(),
    updated_at: new Date(),
  };

  return new WorkflowInstance(workflow, cronflowInstance);
}
