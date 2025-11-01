import {
  WorkflowDefinition,
  StepDefinition,
  TriggerDefinition,
  StepOptions,
  Context,
  RetryConfig,
  CacheConfig,
  WebhookOptions,
  StepConfig,
} from './types';
import { validateWorkflow } from './validation';
import { parseDuration, generateId } from '../utils';
import type { RetryBackoffConfig } from './types';
import { createTestHarness } from '../testing/harness';
import {
  createAdvancedTestHarness,
  AdvancedTestHarness,
} from '../testing/advanced';

function parseDelayToMs(delay: string | number): number {
  if (typeof delay === 'number') {
    return delay;
  }

  const match = delay.match(/^(\d+(?:\.\d+)?)(ms|s|m|h)?$/);
  if (!match) {
    throw new Error(
      `Invalid delay format: ${delay}. Use formats like "5s", "1m", "500ms"`
    );
  }

  const value = parseFloat(match[1]);
  const unit = match[2] || 'ms';

  switch (unit) {
    case 'ms':
      return value;
    case 's':
      return value * 1000;
    case 'm':
      return value * 60 * 1000;
    case 'h':
      return value * 60 * 60 * 1000;
    default:
      throw new Error(`Unsupported time unit: ${unit}`);
  }
}

function calculateBackoffDelay(
  backoff: RetryBackoffConfig,
  attempt: number
): number {
  const baseDelay = parseDelayToMs(backoff.delay);
  const maxDelay = backoff.maxDelay
    ? parseDelayToMs(backoff.maxDelay)
    : Infinity;
  const multiplier =
    backoff.multiplier || (backoff.strategy === 'exponential' ? 2 : 1);

  let delay: number;

  switch (backoff.strategy || 'fixed') {
    case 'fixed':
      delay = baseDelay;
      break;
    case 'linear':
      delay = baseDelay * (1 + (attempt - 1) * multiplier);
      break;
    case 'exponential':
      delay = baseDelay * Math.pow(multiplier, attempt - 1);
      break;
    default:
      delay = baseDelay;
  }

  return Math.min(delay, maxDelay);
}

function shouldRetry(
  error: Error,
  attempt: number,
  retryConfig: RetryConfig
): boolean {
  if (attempt >= retryConfig.attempts) {
    return false;
  }

  const { retryOn } = retryConfig;
  if (!retryOn) {
    return true;
  }

  if (retryOn.conditions) {
    return retryOn.conditions(error, attempt);
  }

  if (retryOn.errors && retryOn.errors.length > 0) {
    const errorMessage = error.message.toLowerCase();
    return retryOn.errors.some(pattern =>
      errorMessage.includes(pattern.toLowerCase())
    );
  }

  if (retryOn.statusCodes && retryOn.statusCodes.length > 0) {
    const statusCode = (error as any).statusCode || (error as any).status;
    if (statusCode) {
      return retryOn.statusCodes.includes(statusCode);
    }
  }

  return true;
}

async function executeWithRetry<T>(
  operation: () => Promise<T>,
  retryConfig: RetryConfig,
  operationName: string = 'operation'
): Promise<T> {
  let lastError: Error;

  for (let attempt = 1; attempt <= retryConfig.attempts; attempt++) {
    try {
      return await operation();
    } catch (error: any) {
      lastError = error;

      if (!shouldRetry(error, attempt, retryConfig)) {
        console.log(
          `❌ ${operationName} failed after ${attempt} attempts, not retrying`
        );
        throw error;
      }

      if (attempt < retryConfig.attempts) {
        const delay = retryConfig.backoff
          ? calculateBackoffDelay(retryConfig.backoff, attempt)
          : 1000;

        console.log(
          `🔄 ${operationName} failed (attempt ${attempt}/${retryConfig.attempts}), retrying in ${delay}ms: ${error.message}`
        );

        if (retryConfig.onRetry) {
          try {
            await retryConfig.onRetry(error, attempt, delay);
          } catch (callbackError) {
            console.error('❌ Error in retry callback:', callbackError);
          }
        }

        await new Promise(resolve => setTimeout(resolve, delay));
      }
    }
  }

  console.log(
    `❌ ${operationName} failed after all ${retryConfig.attempts} attempts`
  );
  throw lastError!;
}

import {
  getFrameworkHandler,
  isFrameworkSupported,
} from './framework-registry';
import { registerEventListener, storePausedWorkflow } from '../cronflow';
import { concurrencyManager } from '../execution/concurrency-manager';
import { scheduler } from '../scheduler';

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
    private readonly _cronflowInstance: any
  ) {
    this._workflow = workflow;
  }

  test() {
    return createTestHarness(this._workflow);
  }

  advancedTest(): AdvancedTestHarness {
    return createAdvancedTestHarness(this._workflow);
  }

  step(
    nameOrConfig: string | StepConfig,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): WorkflowInstance {
    let stepName: string;
    let stepTitle: string | undefined;
    let stepDescription: string | undefined;

    if (typeof nameOrConfig === 'string') {
      stepName = nameOrConfig;
    } else {
      stepName = nameOrConfig.id;
      stepTitle = nameOrConfig.title;
      stepDescription = nameOrConfig.description;
    }

    const step: StepDefinition = {
      id: stepName,
      name: stepName,
      title: stepTitle,
      description: stepDescription,
      handler: handlerFn,
      type: 'step',
      options,
    };

    this._workflow.steps.push(step);
    this._currentStep = step;
    this._stepStack.push(step);

    if (this._cronflowInstance?.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        stepName,
        handlerFn,
        'step'
      );
    }

    return this;
  }

  action(
    nameOrConfig: string | StepConfig,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): WorkflowInstance {
    let stepName: string;
    let stepTitle: string | undefined;
    let stepDescription: string | undefined;

    if (typeof nameOrConfig === 'string') {
      stepName = nameOrConfig;
    } else {
      stepName = nameOrConfig.id;
      stepTitle = nameOrConfig.title;
      stepDescription = nameOrConfig.description;
    }

    const step: StepDefinition = {
      id: stepName,
      name: stepName,
      title: stepTitle,
      description: stepDescription,
      handler: handlerFn,
      type: 'action',
      options: {
        ...options,
        background: true,
      },
    };

    this._workflow.steps.push(step);
    this._currentStep = step;
    this._stepStack.push(step);

    if (this._cronflowInstance?.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        stepName,
        handlerFn,
        'action'
      );
    }

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

  onWebhook(path: string, options?: WebhookOptions): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'webhook',
      path,
      options,
    };

    this._workflow.triggers.push(trigger);

    if (options?.app && options?.appInstance) {
      this._registerWebhookRouteWithFramework(path, options);
    } else if (options?.registerRoute) {
      this._registerWebhookRoute(path, options);
    }

    return this;
  }

  private _registerWebhookRouteWithFramework(
    path: string,
    options: WebhookOptions
  ): void {
    const { app: frameworkName, appInstance } = options;

    if (!frameworkName || !appInstance) return;

    if (!isFrameworkSupported(frameworkName)) {
      throw new Error(
        `Unsupported framework: ${frameworkName}. Supported frameworks: express, fastify, koa, hapi, nestjs, bun, nextjs`
      );
    }

    const frameworkHandler = getFrameworkHandler(frameworkName);

    const webhookHandler = async (req: any, res: any) => {
      try {
        if (options.middleware && options.middleware.length > 0) {
          for (const middleware of options.middleware) {
            let nextCalled = false;
            let middlewareError: any = null;

            const next = () => {
              nextCalled = true;
            };

            try {
              const result = middleware(req, res, next);
              if (result instanceof Promise) {
                await result;
              }
            } catch (error) {
              middlewareError = error;
            }

            if (middlewareError) {
              console.error('❌ Middleware error:', middlewareError);
              return res.status(500).json({
                success: false,
                error: 'Middleware error',
                details: middlewareError.message,
              });
            }

            if (!nextCalled && res.headersSent) {
              return;
            }

            if (!nextCalled && !res.headersSent) {
              console.error(
                '❌ Middleware did not call next() or send a response'
              );
              return res.status(500).json({
                success: false,
                error: 'Middleware did not call next() or send a response',
              });
            }
          }
        }

        if (options.schema) {
          try {
            options.schema.parse(req.body);
          } catch (schemaError: any) {
            console.error('❌ Schema validation failed:', schemaError);
            return res.status(400).json({
              success: false,
              error: 'Payload validation failed',
              details: schemaError.message,
            });
          }
        }

        if (options.validate) {
          try {
            const validationResult = options.validate(req.body);
            const validationPassed =
              validationResult instanceof Promise
                ? await validationResult
                : validationResult;

            if (validationPassed !== true) {
              const errorMessage =
                typeof validationPassed === 'string'
                  ? validationPassed
                  : 'Custom validation failed';

              console.error('❌ Custom validation failed:', errorMessage);
              return res.status(400).json({
                success: false,
                error: 'Custom validation failed',
                details: errorMessage,
                timestamp: new Date().toISOString(),
              });
            }
          } catch (validationError: any) {
            console.error('❌ Custom validation error:', validationError);
            return res.status(400).json({
              success: false,
              error: 'Custom validation error',
              details: validationError.message,
              timestamp: new Date().toISOString(),
            });
          }
        }

        if (options.headers?.required) {
          const requiredHeaders = options.headers.required;
          for (const [key, value] of Object.entries(requiredHeaders)) {
            if (req.headers[key.toLowerCase()] !== value) {
              return res.status(400).json({
                success: false,
                error: `Missing or invalid required header: ${key}`,
              });
            }
          }
        }

        if (options.condition) {
          try {
            const conditionResult = options.condition(req);
            const conditionPassed =
              conditionResult instanceof Promise
                ? await conditionResult
                : conditionResult;

            if (!conditionPassed) {
              return res.status(200).json({
                success: true,
                message: 'Webhook received but condition not met',
                conditionMet: false,
                timestamp: new Date().toISOString(),
              });
            }
          } catch (conditionError: any) {
            console.error('❌ Condition evaluation failed:', conditionError);
            return res.status(400).json({
              success: false,
              error: 'Condition evaluation failed',
              details: conditionError.message,
              timestamp: new Date().toISOString(),
            });
          }
        }

        if (options.trigger) {
          const stepToTrigger = this._workflow.steps.find(
            step => step.id === options.trigger || step.name === options.trigger
          );

          if (!stepToTrigger) {
            console.error(`❌ Step not found: ${options.trigger}`);
            return res.status(400).json({
              success: false,
              error: `Step not found: ${options.trigger}`,
              availableSteps: this._workflow.steps.map(s => ({
                id: s.id,
                name: s.name,
              })),
            });
          }

          const stepContext = {
            run_id: `step_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
            workflow_id: this._workflow.id,
            step_name: stepToTrigger.name,
            payload: req.body,
            steps: {},
            run: {
              id: `step_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
              workflowId: this._workflow.id,
            },
            last: undefined,
            state: {
              get: () => null,
              set: async () => {},
              incr: async () => 0,
            },
            trigger: { headers: req.headers },
            cancel: (reason?: string) => {
              throw new Error(
                `Step cancelled: ${reason || 'No reason provided'}`
              );
            },
          };

          const contextJson = JSON.stringify(stepContext);

          const stepResult = options.retry
            ? await executeWithRetry(
                () =>
                  this._cronflowInstance.executeStepFunction(
                    stepToTrigger.name,
                    contextJson,
                    this._workflow.id,
                    stepContext.run_id
                  ),
                options.retry,
                `Step execution: ${stepToTrigger.name}`
              )
            : await this._cronflowInstance.executeStepFunction(
                stepToTrigger.name,
                contextJson,
                this._workflow.id,
                stepContext.run_id
              );

          if (stepResult.success && stepResult.result) {
            if (options.onSuccess) {
              try {
                const callbackResult = options.onSuccess(
                  stepContext,
                  stepResult.result
                );
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onSuccess callback error:', callbackError);
              }
            }

            res.json({
              success: true,
              stepId: options.trigger,
              stepName: stepToTrigger.name,
              output: stepResult.result.output,
              message: 'Step executed successfully',
              timestamp: new Date().toISOString(),
            });
          } else {
            const stepError = new Error(
              stepResult.message || 'Step execution failed'
            );

            if (options.onError) {
              try {
                const callbackResult = options.onError(stepContext, stepError);
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onError callback error:', callbackError);
              }
            }

            console.error(
              `❌ Step ${options.trigger} failed:`,
              stepResult.message
            );
            res.status(500).json({
              success: false,
              stepId: options.trigger,
              error: stepResult.message,
              timestamp: new Date().toISOString(),
            });
          }
        } else {
          const webhookContext = {
            payload: req.body,
            steps: {},
            run: {
              id: `webhook_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
              workflowId: this._workflow.id,
            },
            last: undefined,
            state: {
              get: () => null,
              set: async () => {},
              incr: async () => 0,
            },
            trigger: { headers: req.headers },
            cancel: (reason?: string) => {
              throw new Error(
                `Webhook cancelled: ${reason || 'No reason provided'}`
              );
            },
          };

          try {
            const runId = options.retry
              ? await executeWithRetry(
                  () =>
                    this._cronflowInstance.trigger(this._workflow.id, req.body),
                  options.retry,
                  `Workflow execution: ${this._workflow.name || this._workflow.id}`
                )
              : await this._cronflowInstance.trigger(
                  this._workflow.id,
                  req.body
                );

            if (options.onSuccess) {
              try {
                const callbackResult = options.onSuccess(webhookContext, {
                  runId,
                });
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onSuccess callback error:', callbackError);
              }
            }

            res.json({
              success: true,
              runId,
              message: 'Webhook processed successfully',
              timestamp: new Date().toISOString(),
            });
          } catch (workflowError: any) {
            if (options.onError) {
              try {
                const callbackResult = options.onError(
                  webhookContext,
                  workflowError
                );
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onError callback error:', callbackError);
              }
            }

            throw workflowError;
          }
        }
      } catch (error: any) {
        console.error('❌ Webhook processing failed:', error);
        res.status(500).json({
          success: false,
          error: error.message,
          timestamp: new Date().toISOString(),
        });
      }
    };

    const method = options.method || 'POST';
    frameworkHandler(appInstance, method, path, webhookHandler);
  }

  private _registerWebhookRoute(path: string, options: WebhookOptions): void {
    const { registerRoute } = options;

    if (!registerRoute) return;

    const webhookHandler = async (req: any, res: any) => {
      try {
        if (options.schema) {
          try {
            options.schema.parse(req.body);
          } catch (schemaError: any) {
            console.error('❌ Schema validation failed:', schemaError);
            return res.status(400).json({
              success: false,
              error: 'Payload validation failed',
              details: schemaError.message,
            });
          }
        }

        if (options.validate) {
          try {
            const validationResult = options.validate(req.body);
            const validationPassed =
              validationResult instanceof Promise
                ? await validationResult
                : validationResult;

            if (validationPassed !== true) {
              const errorMessage =
                typeof validationPassed === 'string'
                  ? validationPassed
                  : 'Custom validation failed';

              console.error('❌ Custom validation failed:', errorMessage);
              return res.status(400).json({
                success: false,
                error: 'Custom validation failed',
                details: errorMessage,
                timestamp: new Date().toISOString(),
              });
            }
          } catch (validationError: any) {
            console.error('❌ Custom validation error:', validationError);
            return res.status(400).json({
              success: false,
              error: 'Custom validation error',
              details: validationError.message,
              timestamp: new Date().toISOString(),
            });
          }
        }

        if (options.headers?.required) {
          const requiredHeaders = options.headers.required;
          for (const [key, value] of Object.entries(requiredHeaders)) {
            if (req.headers[key.toLowerCase()] !== value) {
              return res.status(400).json({
                success: false,
                error: `Missing or invalid required header: ${key}`,
              });
            }
          }
        }

        if (options.condition) {
          try {
            const conditionResult = options.condition(req);
            const conditionPassed =
              conditionResult instanceof Promise
                ? await conditionResult
                : conditionResult;

            if (!conditionPassed) {
              return res.status(200).json({
                success: true,
                message: 'Webhook received but condition not met',
                conditionMet: false,
                timestamp: new Date().toISOString(),
              });
            }
          } catch (conditionError: any) {
            console.error('❌ Condition evaluation failed:', conditionError);
            return res.status(400).json({
              success: false,
              error: 'Condition evaluation failed',
              details: conditionError.message,
              timestamp: new Date().toISOString(),
            });
          }
        }

        if (options.trigger) {
          const stepToTrigger = this._workflow.steps.find(
            step => step.id === options.trigger || step.name === options.trigger
          );

          if (!stepToTrigger) {
            console.error(`❌ Step not found: ${options.trigger}`);
            return res.status(400).json({
              success: false,
              error: `Step not found: ${options.trigger}`,
              availableSteps: this._workflow.steps.map(s => ({
                id: s.id,
                name: s.name,
              })),
            });
          }

          const stepContext = {
            run_id: `step_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
            workflow_id: this._workflow.id,
            step_name: stepToTrigger.name,
            payload: req.body,
            steps: {},
            run: {
              id: `step_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
              workflowId: this._workflow.id,
            },
            last: undefined,
            state: {
              get: () => null,
              set: async () => {},
              incr: async () => 0,
            },
            trigger: { headers: req.headers },
            cancel: (reason?: string) => {
              throw new Error(
                `Step cancelled: ${reason || 'No reason provided'}`
              );
            },
          };

          const contextJson = JSON.stringify(stepContext);

          const stepResult = options.retry
            ? await executeWithRetry(
                () =>
                  this._cronflowInstance.executeStepFunction(
                    stepToTrigger.name,
                    contextJson,
                    this._workflow.id,
                    stepContext.run_id
                  ),
                options.retry,
                `Step execution: ${stepToTrigger.name}`
              )
            : await this._cronflowInstance.executeStepFunction(
                stepToTrigger.name,
                contextJson,
                this._workflow.id,
                stepContext.run_id
              );

          if (stepResult.success && stepResult.result) {
            if (options.onSuccess) {
              try {
                const callbackResult = options.onSuccess(
                  stepContext,
                  stepResult.result
                );
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onSuccess callback error:', callbackError);
              }
            }

            res.json({
              success: true,
              stepId: options.trigger,
              stepName: stepToTrigger.name,
              output: stepResult.result.output,
              message: 'Step executed successfully',
              timestamp: new Date().toISOString(),
            });
          } else {
            const stepError = new Error(
              stepResult.message || 'Step execution failed'
            );

            if (options.onError) {
              try {
                const callbackResult = options.onError(stepContext, stepError);
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onError callback error:', callbackError);
              }
            }

            console.error(
              `❌ Step ${options.trigger} failed:`,
              stepResult.message
            );
            res.status(500).json({
              success: false,
              stepId: options.trigger,
              error: stepResult.message,
              timestamp: new Date().toISOString(),
            });
          }
        } else {
          const webhookContext = {
            payload: req.body,
            steps: {},
            run: {
              id: `webhook_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
              workflowId: this._workflow.id,
            },
            last: undefined,
            state: {
              get: () => null,
              set: async () => {},
              incr: async () => 0,
            },
            trigger: { headers: req.headers },
            cancel: (reason?: string) => {
              throw new Error(
                `Webhook cancelled: ${reason || 'No reason provided'}`
              );
            },
          };

          try {
            const runId = options.retry
              ? await executeWithRetry(
                  () =>
                    this._cronflowInstance.trigger(this._workflow.id, req.body),
                  options.retry,
                  `Workflow execution: ${this._workflow.name || this._workflow.id}`
                )
              : await this._cronflowInstance.trigger(
                  this._workflow.id,
                  req.body
                );

            if (options.onSuccess) {
              try {
                const callbackResult = options.onSuccess(webhookContext, {
                  runId,
                });
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onSuccess callback error:', callbackError);
              }
            }

            res.json({
              success: true,
              runId,
              message: 'Webhook processed successfully',
              timestamp: new Date().toISOString(),
            });
          } catch (workflowError: any) {
            if (options.onError) {
              try {
                const callbackResult = options.onError(
                  webhookContext,
                  workflowError
                );
                if (callbackResult instanceof Promise) {
                  await callbackResult;
                }
              } catch (callbackError) {
                console.error('❌ onError callback error:', callbackError);
              }
            }

            throw workflowError;
          }
        }
      } catch (error: any) {
        console.error('❌ Webhook processing failed:', error);
        res.status(500).json({
          success: false,
          error: error.message,
          timestamp: new Date().toISOString(),
        });
      }
    };

    const method = options.method || 'POST';
    registerRoute(method, path, webhookHandler);
  }

  onSchedule(cronExpression: string): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'schedule',
      cron_expression: cronExpression,
    };

    this._workflow.triggers.push(trigger);

    scheduler.scheduleWorkflow(this._workflow.id, cronExpression);

    return this;
  }

  onInterval(interval: string): WorkflowInstance {
    const cronExpression = this._intervalToCron(interval);
    return this.onSchedule(cronExpression);
  }

  onEvent(eventName: string): WorkflowInstance {
    const trigger: TriggerDefinition = {
      type: 'event',
      eventName,
    };

    this._workflow.triggers.push(trigger);

    if (this._cronflowInstance && this._cronflowInstance.trigger) {
      registerEventListener(
        eventName,
        this._workflow.id,
        this._cronflowInstance.trigger
      );
      console.log(
        `✅ Registered workflow ${this._workflow.id} to listen for event: ${eventName}`
      );
    }

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

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        `if_${name}`,
        condition,
        'step'
      );
    }

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
    const groupId = `parallel_group_${generateId('parallel')}`;

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
        parallelGroupId: groupId,
      },
      parallel: true,
      parallel_group_id: groupId,
      parallel_step_count: steps.length,
      race: false,
      for_each: false,
    };

    this._workflow.steps.push(parallelStep);
    this._currentStep = parallelStep;

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        parallelStep.name,
        parallelStep.handler,
        'step'
      );
    }

    return this;
  }

  race(steps: Array<(ctx: Context) => any | Promise<any>>): WorkflowInstance {
    const groupId = `race_group_${generateId('race')}`;

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
        parallelGroupId: groupId,
      },
      parallel: false,
      parallel_group_id: groupId,
      parallel_step_count: steps.length,
      race: true,
      for_each: false,
    };

    this._workflow.steps.push(raceStep);
    this._currentStep = raceStep;

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        raceStep.name,
        raceStep.handler,
        'step'
      );
    }

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
        const maxIterations = 1000;

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

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        `elseif_${name}`,
        condition,
        'step'
      );
    }

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

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        `else_${lastControlFlow.name}`,
        (ctx: Context) => true,
        'step'
      );
    }

    this._controlFlowStack.push({
      type: 'else',
      name: lastControlFlow.name,
      startIndex: this._workflow.steps.length - 1,
    });

    return this;
  }

  cancel(reason?: string): WorkflowInstance {
    const cancelStep: StepDefinition = {
      id: `cancel_${generateId('cancel')}`,
      name: 'cancel_workflow',
      handler: (ctx: Context) => {
        throw new Error(
          `Workflow cancelled: ${reason || 'No reason provided'}`
        );
      },
      type: 'step',
      options: {
        cancel: true,
        reason,
      },
    };

    this._workflow.steps.push(cancelStep);
    this._currentStep = cancelStep;

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
    const subflowStep: StepDefinition = {
      id: `subflow_${name}`,
      name: `subflow_${name}`,
      handler: async (ctx: Context) => {
        // TODO: Implement actual subflow execution
        // For now, simulate subflow execution

        // Simulate subflow result
        return {
          subflowId: workflowId,
          input,
          result: { status: 'completed', subflowName: name },
        };
      },
      type: 'step',
      options: {
        subflow: true,
        workflowId,
        input,
      },
    };

    this._workflow.steps.push(subflowStep);
    this._currentStep = subflowStep;

    return this;
  }

  forEach(
    name: string,
    items: (ctx: Context) => any[] | Promise<any[]>,
    iterationFn: (item: any, flow: WorkflowInstance) => void
  ): WorkflowInstance {
    const forEachStep: StepDefinition = {
      id: `forEach_${name}`,
      name: `forEach_${name}`,
      handler: async (ctx: Context) => {
        const itemsArray = await items(ctx);

        if (!Array.isArray(itemsArray)) {
          throw new Error('forEach items function must return an array');
        }

        const results = await Promise.all(
          itemsArray.map(async (item, index) => {
            const tempFlow = new WorkflowInstance(
              {
                id: `${this._workflow.id}_${name}_${index}`,
                name: `${name}_iteration_${index}`,
                steps: [],
                triggers: [],
                created_at: new Date(),
                updated_at: new Date(),
              },
              this._cronflowInstance
            );

            iterationFn(item, tempFlow);

            const iterationSteps = tempFlow.getSteps();

            let result = null;
            for (const step of iterationSteps) {
              if (step.handler) {
                result = await step.handler(ctx);
              }
            }

            return {
              item,
              result,
              index,
            };
          })
        );

        return {
          type: 'forEach',
          name,
          results,
          totalItems: itemsArray.length,
        };
      },
      type: 'step',
      options: {
        forEach: true,
        parallel: true,
      },
    };

    this._workflow.steps.push(forEachStep);
    this._currentStep = forEachStep;

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        forEachStep.name,
        forEachStep.handler,
        'step'
      );
    }

    return this;
  }

  batch(
    name: string,
    options: { items: (ctx: Context) => any[] | Promise<any[]>; size: number },
    batchFn: (batch: any[], flow: WorkflowInstance) => void
  ): WorkflowInstance {
    const batchStep: StepDefinition = {
      id: `batch_${name}`,
      name: `batch_${name}`,
      handler: async (ctx: Context) => {
        const itemsArray = await options.items(ctx);

        if (!Array.isArray(itemsArray)) {
          throw new Error('batch items function must return an array');
        }

        const { size } = options;
        const batches = [];

        for (let i = 0; i < itemsArray.length; i += size) {
          batches.push(itemsArray.slice(i, i + size));
        }

        const results = [];

        for (let batchIndex = 0; batchIndex < batches.length; batchIndex++) {
          const batch = batches[batchIndex];

          const tempFlow = new WorkflowInstance(
            {
              id: `${this._workflow.id}_${name}_batch_${batchIndex}`,
              name: `${name}_batch_${batchIndex}`,
              steps: [],
              triggers: [],
              created_at: new Date(),
              updated_at: new Date(),
            },
            this._cronflowInstance
          );

          batchFn(batch, tempFlow);

          const batchSteps = tempFlow.getSteps();

          let batchResult = null;
          for (const step of batchSteps) {
            if (step.handler) {
              batchResult = await step.handler(ctx);
            }
          }

          results.push({
            batchIndex,
            items: batch,
            result: batchResult,
            size: batch.length,
          });
        }

        return {
          type: 'batch',
          name,
          results,
          totalBatches: batches.length,
          totalItems: itemsArray.length,
        };
      },
      type: 'step',
      options: {
        batch: true,
        batchSize: options.size,
      },
    };

    this._workflow.steps.push(batchStep);
    this._currentStep = batchStep;

    return this;
  }

  humanInTheLoop(options: {
    timeout?: string;
    onPause: (ctx: Context, token: string) => void;
    description: string;
    approvalUrl?: string;
    metadata?: Record<string, any>;
  }): WorkflowInstance {
    const token = generateId('human_approval');
    const timeoutMs = options.timeout
      ? parseDuration(options.timeout)
      : undefined;
    const createdAt = Date.now();
    const expiresAt = timeoutMs ? createdAt + timeoutMs : undefined;

    const humanStep: StepDefinition = {
      id: `human_${token}`,
      name: 'human_in_the_loop',
      handler: async (ctx: Context) => {
        const enhancedContext = {
          ...ctx,
          token,
        };

        options.onPause(enhancedContext, token);

        const pauseInfo = {
          token,
          workflowId: ctx.run.workflowId,
          runId: ctx.run.id,
          stepId: `human_${token}`,
          description: options.description,
          metadata: options.metadata || {},
          createdAt,
          expiresAt,
          status: 'waiting' as const,
          payload: ctx.payload,
          lastStepOutput: ctx.last,
        };

        storePausedWorkflow(token, pauseInfo);

        if (!timeoutMs) {
          return new Promise(resolve => {
            storePausedWorkflow(token, {
              ...pauseInfo,
              resumeCallback: (resumePayload: any) => {
                resolve({
                  type: 'human_approval',
                  token,
                  description: options.description,
                  metadata: options.metadata,
                  approved: resumePayload.approved || false,
                  reason: resumePayload.reason || 'Manual approval',
                  approvedBy: resumePayload.approvedBy || 'human',
                  approvedAt: Date.now(),
                  status: 'resumed',
                  resumePayload,
                });
              },
            });
          });
        }

        return new Promise(resolve => {
          let resolved = false;

          const timeoutId = setTimeout(() => {
            if (!resolved) {
              resolved = true;
              resolve({
                type: 'human_approval',
                token,
                description: options.description,
                metadata: options.metadata,
                approved: false,
                reason: 'Timeout - no approval received within specified time',
                approvedBy: 'system',
                approvedAt: Date.now(),
                expiresAt,
                status: 'timeout',
                timedOut: true,
              });
            }
          }, timeoutMs);

          storePausedWorkflow(token, {
            ...pauseInfo,
            resumeCallback: (resumePayload: any) => {
              if (!resolved) {
                resolved = true;
                clearTimeout(timeoutId);
                resolve({
                  type: 'human_approval',
                  token,
                  description: options.description,
                  metadata: options.metadata,
                  approved: resumePayload.approved || false,
                  reason: resumePayload.reason || 'Manual approval',
                  approvedBy: resumePayload.approvedBy || 'human',
                  approvedAt: Date.now(),
                  status: 'resumed',
                  resumePayload,
                });
              }
            },
          });
        });
      },
      type: 'step',
      options: {
        humanInTheLoop: true,
        token,
        timeout: timeoutMs,
        description: options.description,
        approvalUrl: options.approvalUrl,
        metadata: options.metadata,
        createdAt,
        expiresAt,
      },
    };

    this._workflow.steps.push(humanStep);
    this._currentStep = humanStep;

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        humanStep.name,
        humanStep.handler,
        'step'
      );
    }

    return this;
  }

  waitForEvent(eventName: string, timeout?: string): WorkflowInstance {
    const timeoutMs = timeout ? parseDuration(timeout) : undefined;

    const waitStep: StepDefinition = {
      id: `wait_${eventName}`,
      name: `wait_for_event_${eventName}`,
      handler: async (ctx: Context) => {
        // TODO: Implement actual event waiting mechanism

        return {
          type: 'event_wait',
          eventName,
          received: true,
          data: { event: eventName, timestamp: Date.now() },
        };
      },
      type: 'step',
      options: {
        waitForEvent: true,
        eventName,
        timeout: timeoutMs,
      },
    };

    this._workflow.steps.push(waitStep);
    this._currentStep = waitStep;

    return this;
  }

  onError(handlerFn: (ctx: Context) => any): WorkflowInstance {
    if (!this._currentStep) {
      throw new Error('No current step. Call .step() or .action() first.');
    }

    if (!this._currentStep.options) {
      this._currentStep.options = {};
    }

    this._currentStep.options.onError = handlerFn;
    return this;
  }

  log(
    message: string | ((ctx: Context) => string),
    level?: 'info' | 'warn' | 'error'
  ): WorkflowInstance {
    const step: StepDefinition = {
      id: `log_${Date.now()}`,
      name: 'log',
      handler: async (ctx: Context) => {
        const messageStr =
          typeof message === 'function' ? message(ctx) : message;
        return { message: messageStr, level: level || 'info' };
      },
      type: 'action',
      options: { background: true },
    };

    this._workflow.steps.push(step);
    return this;
  }

  pause(
    pauseCallback: (ctx: Context) => void | Promise<void>
  ): WorkflowInstance {
    const step: StepDefinition = {
      id: `pause_${Date.now()}`,
      name: 'pause',
      handler: async (ctx: Context) => {
        await pauseCallback(ctx);

        return {
          paused: true,
          timestamp: new Date().toISOString(),
          stepId: ctx.step_name,
          message: 'Workflow paused for manual intervention',
        };
      },
      type: 'step',
      options: {
        pause: true,
        background: false,
      },
    };

    this._workflow.steps.push(step);
    this._currentStep = step;
    this._stepStack.push(step);

    if (this._cronflowInstance && this._cronflowInstance.registerStepHandler) {
      this._cronflowInstance.registerStepHandler(
        this._workflow.id,
        'pause',
        step.handler,
        'step'
      );
    }

    return this;
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

    // Register concurrency limits if specified
    if (this._workflow.concurrency && this._workflow.concurrency > 0) {
      concurrencyManager.registerWorkflow(
        this._workflow.id,
        this._workflow.concurrency
      );
    }

    if (
      this._cronflowInstance &&
      typeof this._cronflowInstance.registerWorkflow === 'function'
    ) {
      await this._cronflowInstance.registerWorkflow(this._workflow);
    }

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
      case 'm':
        return `*/${num} * * * *`;
      case 'h':
        return `0 */${num} * * *`;
      case 'd':
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
