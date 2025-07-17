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
import { parseDuration, generateId } from '../utils';

export class WorkflowInstance {
  private _workflow: WorkflowDefinition;
  private _currentStep: StepDefinition | null = null;
  private _stepStack: StepDefinition[] = [];

  private _controlFlowStack: Array<{
    type: 'if' | 'elseIf' | 'else';
    name: string;
    condition?: (ctx: Context) => boolean | Promise<boolean>;
    startIndex: number;
  }> = [];

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
    const conditionalStep: StepDefinition = {
      id: `conditional_${name}`,
      name: `if_${name}`,
      handler: condition,
      type: 'step',
      options: {
        ...options,
        conditional: true,
        conditionType: 'if',
      },
    };

    this._workflow.steps.push(conditionalStep);
    this._currentStep = conditionalStep;

    this._controlFlowStack.push({
      type: 'if',
      name,
      condition,
      startIndex: this._workflow.steps.length - 1,
    });

    return this;
  }

  parallel(
    steps: Array<(ctx: Context) => any | Promise<any>>
  ): WorkflowInstance {
    const parallelStep: StepDefinition = {
      id: `parallel_${generateId('parallel')}`,
      name: 'parallel_execution',
      handler: async (ctx: Context) => {
        const results = await Promise.all(steps.map(stepFn => stepFn(ctx)));
        return results;
      },
      type: 'step',
      options: {
        parallel: true,
        stepCount: steps.length,
      },
    };

    this._workflow.steps.push(parallelStep);
    this._currentStep = parallelStep;

    return this;
  }

  race(steps: Array<(ctx: Context) => any | Promise<any>>): WorkflowInstance {
    const raceStep: StepDefinition = {
      id: `race_${generateId('race')}`,
      name: 'race_execution',
      handler: async (ctx: Context) => {
        const result = await Promise.race(steps.map(stepFn => stepFn(ctx)));
        return result;
      },
      type: 'step',
      options: {
        race: true,
        stepCount: steps.length,
      },
    };

    this._workflow.steps.push(raceStep);
    this._currentStep = raceStep;

    return this;
  }

  while(
    name: string,
    condition: (ctx: Context) => boolean | Promise<boolean>,
    iterationFn: (ctx: Context) => void
  ): WorkflowInstance {
    const whileStep: StepDefinition = {
      id: `while_${name}`,
      name: `while_${name}`,
      handler: async (ctx: Context) => {
        let iterations = 0;
        const maxIterations = 1000; // Prevent infinite loops

        while ((await condition(ctx)) && iterations < maxIterations) {
          iterationFn(ctx);
          iterations++;
        }

        if (iterations >= maxIterations) {
          throw new Error(
            `While loop exceeded maximum iterations (${maxIterations})`
          );
        }

        return { iterations, completed: true };
      },
      type: 'step',
      options: {
        loop: true,
        maxIterations: 1000,
      },
    };

    this._workflow.steps.push(whileStep);
    this._currentStep = whileStep;

    return this;
  }

  endIf(): WorkflowInstance {
    // Validate that we're in an if block
    if (this._controlFlowStack.length === 0) {
      throw new Error('endIf() called without matching if()');
    }

    const lastControlFlow =
      this._controlFlowStack[this._controlFlowStack.length - 1];
    if (
      lastControlFlow.type !== 'if' &&
      lastControlFlow.type !== 'elseIf' &&
      lastControlFlow.type !== 'else'
    ) {
      throw new Error('endIf() called without matching if()');
    }

    const endIfStep: StepDefinition = {
      id: `endif_${lastControlFlow.name}`,
      name: `endif_${lastControlFlow.name}`,
      handler: (ctx: Context) => {
        // This step just marks the end of the conditional block
        return { type: 'endIf', blockName: lastControlFlow.name };
      },
      type: 'step',
      options: {
        controlFlow: true,
        endIf: true,
      },
    };

    this._workflow.steps.push(endIfStep);
    this._currentStep = endIfStep;

    this._controlFlowStack.pop();

    return this;
  }

  elseIf(
    name: string,
    condition: (ctx: Context) => boolean | Promise<boolean>
  ): WorkflowInstance {
    if (this._controlFlowStack.length === 0) {
      throw new Error('elseIf() called without matching if()');
    }

    const lastControlFlow =
      this._controlFlowStack[this._controlFlowStack.length - 1];
    if (lastControlFlow.type !== 'if' && lastControlFlow.type !== 'elseIf') {
      throw new Error('elseIf() called without matching if()');
    }

    const elseIfStep: StepDefinition = {
      id: `elseif_${name}`,
      name: `elseif_${name}`,
      handler: condition,
      type: 'step',
      options: {
        conditional: true,
        conditionType: 'elseIf',
      },
    };

    this._workflow.steps.push(elseIfStep);
    this._currentStep = elseIfStep;

    this._controlFlowStack.push({
      type: 'elseIf',
      name,
      condition,
      startIndex: this._workflow.steps.length - 1,
    });

    return this;
  }

  else(): WorkflowInstance {
    if (this._controlFlowStack.length === 0) {
      throw new Error('else() called without matching if()');
    }

    const lastControlFlow =
      this._controlFlowStack[this._controlFlowStack.length - 1];
    if (lastControlFlow.type !== 'if' && lastControlFlow.type !== 'elseIf') {
      throw new Error('else() called without matching if()');
    }

    const elseStep: StepDefinition = {
      id: `else_${lastControlFlow.name}`,
      name: `else_${lastControlFlow.name}`,
      handler: (ctx: Context) => {
        // Else condition is always true (fallback)
        return true;
      },
      type: 'step',
      options: {
        conditional: true,
        conditionType: 'else',
      },
    };

    this._workflow.steps.push(elseStep);
    this._currentStep = elseStep;

    this._controlFlowStack.push({
      type: 'else',
      name: lastControlFlow.name,
      startIndex: this._workflow.steps.length - 1,
    });

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
