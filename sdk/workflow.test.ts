import { describe, it, expect, beforeEach } from 'vitest';
import { cronflow, WorkflowInstance } from './index';

describe('Workflow Builder', () => {
  beforeEach(() => {
    // Reset cronflow instance for each test
    (cronflow as any).workflows.clear();
    (cronflow as any).engineState = 'STOPPED';
  });

  describe('cronflow.define()', () => {
    it('should create a workflow with basic definition', () => {
      const workflow = cronflow.define({
        id: 'test-workflow',
        name: 'Test Workflow',
      });

      expect(workflow).toBeInstanceOf(WorkflowInstance);
      expect(workflow.getId()).toBe('test-workflow');
      expect(workflow.getName()).toBe('Test Workflow');
    });

    it('should throw error for empty ID', () => {
      expect(() => {
        cronflow.define({
          id: '',
          name: 'Test Workflow',
        });
      }).toThrow('Workflow ID cannot be empty');
    });

    it('should throw error for duplicate workflow ID', () => {
      cronflow.define({
        id: 'test-workflow',
        name: 'Test Workflow',
      });

      expect(() => {
        cronflow.define({
          id: 'test-workflow',
          name: 'Another Workflow',
        });
      }).toThrow("Workflow with ID 'test-workflow' already exists");
    });

    it('should accept setup function', () => {
      const workflow = cronflow.define({
        id: 'test-workflow',
        name: 'Test Workflow',
      });

      // Use the fluent API after creation
      workflow
        .step('test-step', async ctx => {
          return { result: 'test' };
        })
        .onWebhook('/webhook/test');

      expect(workflow).toBeInstanceOf(WorkflowInstance);
      expect(workflow.getId()).toBe('test-workflow');
    });
  });

  describe('WorkflowInstance fluent API', () => {
    let workflow: WorkflowInstance;

    beforeEach(() => {
      workflow = cronflow.define({
        id: 'test-workflow',
        name: 'Test Workflow',
      });
    });

    it('should add steps with fluent API', () => {
      workflow
        .step('step1', async ctx => {
          return { result: 'step1' };
        })
        .step('step2', async ctx => {
          return { result: 'step2' };
        });

      const steps = workflow.getSteps();
      expect(steps).toHaveLength(2);
      expect(steps[0].id).toBe('step1');
      expect(steps[0].name).toBe('step1');
      expect(steps[0].type).toBe('step');
      expect(steps[1].id).toBe('step2');
      expect(steps[1].name).toBe('step2');
      expect(steps[1].type).toBe('step');
    });

    it('should add actions with fluent API', () => {
      workflow
        .action('action1', async ctx => {
          console.log('Action 1');
        })
        .action('action2', async ctx => {
          console.log('Action 2');
        });

      const steps = workflow.getSteps();
      expect(steps).toHaveLength(2);
      expect(steps[0].type).toBe('action');
      expect(steps[1].type).toBe('action');
    });

    it('should set timeout for current step', () => {
      workflow.step(
        'step1',
        async ctx => {
          return { result: 'step1' };
        },
        {
          timeout: '30s',
        }
      );

      const step = workflow.getStep('step1');
      expect(step?.options?.timeout).toBe('30s');
    });

    it('should set retry configuration for current step', () => {
      workflow.step(
        'step1',
        async ctx => {
          return { result: 'step1' };
        },
        {
          retry: {
            attempts: 3,
            backoff: { strategy: 'exponential', delay: '1s' },
          },
        }
      );

      const step = workflow.getStep('step1');
      expect(step?.options?.retry).toEqual({
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '1s' },
      });
    });

    it('should add webhook trigger', () => {
      workflow.onWebhook('/webhook/test', {
        method: 'POST',
        parseRawBody: true,
      });

      const triggers = workflow.getTriggers();
      expect(triggers).toHaveLength(1);
      expect(triggers[0]).toEqual({
        type: 'webhook',
        path: '/webhook/test',
        options: {
          method: 'POST',
          parseRawBody: true,
        },
      });
    });

    it('should add schedule trigger', () => {
      workflow.onSchedule('0 0 * * *');

      const triggers = workflow.getTriggers();
      expect(triggers).toHaveLength(1);
      expect(triggers[0]).toEqual({
        type: 'schedule',
        cron_expression: '0 0 * * *',
      });
    });

    it('should add interval trigger', () => {
      workflow.onInterval('5m');

      const triggers = workflow.getTriggers();
      expect(triggers).toHaveLength(1);
      expect(triggers[0]).toEqual({
        type: 'schedule',
        cron_expression: '*/5 * * * *',
      });
    });

    it('should add manual trigger', () => {
      workflow.manual();

      const triggers = workflow.getTriggers();
      expect(triggers).toHaveLength(1);
      expect(triggers[0]).toEqual({
        type: 'manual',
      });
    });

    it('should validate workflow successfully', () => {
      workflow
        .step('step1', async ctx => {
          return { result: 'step1' };
        })
        .onWebhook('/webhook/test');

      expect(() => {
        workflow.validate();
      }).not.toThrow();
    });

    it('should throw error for invalid workflow (no steps)', () => {
      workflow.onWebhook('/webhook/test');

      expect(() => {
        workflow.validate();
      }).toThrow('Workflow must have at least one step');
    });

    it('should serialize workflow to JSON', () => {
      workflow
        .step(
          'step1',
          async ctx => {
            return { result: 'step1' };
          },
          {
            timeout: '30s',
            retry: {
              attempts: 3,
              backoff: { strategy: 'exponential', delay: '1s' },
            },
          }
        )
        .onWebhook('/webhook/test', {
          method: 'POST',
        });

      const json = workflow.toJSON();
      const parsed = JSON.parse(json);

      expect(parsed.id).toBe('test-workflow');
      expect(parsed.name).toBe('Test Workflow');
      expect(parsed.steps).toHaveLength(1);
      expect(parsed.triggers).toHaveLength(1);
    });

    it('should check trigger types correctly', () => {
      expect(workflow.hasWebhookTriggers()).toBe(false);
      expect(workflow.hasScheduleTriggers()).toBe(false);
      expect(workflow.hasManualTriggers()).toBe(false);

      workflow.onWebhook('/webhook/test');
      expect(workflow.hasWebhookTriggers()).toBe(true);

      workflow.onSchedule('0 0 * * *');
      expect(workflow.hasScheduleTriggers()).toBe(true);

      workflow.manual();
      expect(workflow.hasManualTriggers()).toBe(true);
    });

    it('should get step by ID', () => {
      workflow
        .step('step1', async ctx => {
          return { result: 'step1' };
        })
        .step('step2', async ctx => {
          return { result: 'step2' };
        });

      const step1 = workflow.getStep('step1');
      const step2 = workflow.getStep('step2');
      const step3 = workflow.getStep('step3');

      expect(step1?.id).toBe('step1');
      expect(step2?.id).toBe('step2');
      expect(step3).toBeUndefined();
    });

    it('should add logging step', () => {
      workflow.log('Test log message');

      const steps = workflow.getSteps();
      expect(steps).toHaveLength(1);
      expect(steps[0].type).toBe('action');
      expect(steps[0].name).toBe('log');
    });

    it('should add sleep step', () => {
      workflow.sleep('5s');

      const steps = workflow.getSteps();
      expect(steps).toHaveLength(1);
      expect(steps[0].type).toBe('action');
      expect(steps[0].name).toBe('sleep');
    });
  });

  describe('Complex workflow example', () => {
    it('should create a complex workflow with all features', () => {
      const workflow = cronflow.define({
        id: 'email-workflow',
        name: 'Email Processing Workflow',
        description: 'Process incoming emails and send notifications',
        tags: ['email', 'processing'],
        concurrency: 10,
        timeout: '5m',
        hooks: {
          onSuccess: ctx => {
            console.log('Workflow succeeded');
          },
          onFailure: ctx => {
            console.error('Workflow failed');
          },
        },
      });

      // Use the fluent API after creation
      workflow
        .onWebhook('/webhook/email', {
          method: 'POST',
          parseRawBody: true,
        })
        .onSchedule('0 */5 * * *')
        .step(
          'fetch-emails',
          async ctx => {
            return { emails: [] };
          },
          {
            timeout: '2m',
            retry: {
              attempts: 3,
              backoff: { strategy: 'exponential', delay: '5s' },
            },
          }
        )
        .step(
          'process-emails',
          async ctx => {
            const { emails } = ctx.steps['fetch-emails'].output;
            return { processed: emails.length };
          },
          {
            timeout: '3m',
          }
        )
        .step(
          'send-notifications',
          async ctx => {
            const { processed } = ctx.steps['process-emails'].output;
            return { sent: processed };
          },
          {
            timeout: '1m',
            retry: {
              attempts: 2,
              backoff: { strategy: 'fixed', delay: '3s' },
            },
          }
        );

      // Verify workflow structure
      expect(workflow.getId()).toBe('email-workflow');
      expect(workflow.getName()).toBe('Email Processing Workflow');
      expect(workflow.getDefinition().description).toBe(
        'Process incoming emails and send notifications'
      );

      // Verify steps
      const steps = workflow.getSteps();
      expect(steps).toHaveLength(3);

      const fetchStep = workflow.getStep('fetch-emails');
      expect(fetchStep?.options?.timeout).toBe('2m');
      expect(fetchStep?.options?.retry).toEqual({
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '5s' },
      });

      const processStep = workflow.getStep('process-emails');
      expect(processStep?.options?.timeout).toBe('3m');

      const notifyStep = workflow.getStep('send-notifications');
      expect(notifyStep?.options?.timeout).toBe('1m');
      expect(notifyStep?.options?.retry).toEqual({
        attempts: 2,
        backoff: { strategy: 'fixed', delay: '3s' },
      });

      // Verify triggers
      const triggers = workflow.getTriggers();
      expect(triggers).toHaveLength(2);
      expect(workflow.hasWebhookTriggers()).toBe(true);
      expect(workflow.hasScheduleTriggers()).toBe(true);

      // Verify validation
      expect(() => workflow.validate()).not.toThrow();

      // Verify JSON serialization
      const json = workflow.toJSON();
      const parsed = JSON.parse(json);
      expect(parsed.id).toBe('email-workflow');
      expect(parsed.steps).toHaveLength(3);
      expect(parsed.triggers).toHaveLength(2);
    });
  });
});
