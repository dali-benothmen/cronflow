import { describe, it, expect, beforeEach } from 'vitest';
import {
  RetryExecutor,
  type RetryOptions,
  type RetryResult,
} from '../sdk/src/retry';
import { StepExecutor, type StepExecutionResult } from '../sdk/src/execution';
import { cronflow } from '../sdk/index';
import { Context } from '../sdk/src/workflow/types';

describe('Retry Logic Implementation', () => {
  describe('RetryExecutor', () => {
    describe('Basic Retry Functionality', () => {
      it('should succeed on first attempt', async () => {
        const result = await RetryExecutor.execute(
          () => Promise.resolve('success'),
          {
            maxAttempts: 3,
            backoff: { strategy: 'exponential', delay: '1s' },
          }
        );

        expect(result.success).toBe(true);
        expect(result.result).toBe('success');
        expect(result.attempts).toBe(1);
        expect(result.retryDelays).toHaveLength(0);
      });

      it('should retry and eventually succeed', async () => {
        let attempts = 0;
        const result = await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 3) {
              throw new Error('Temporary network failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 3,
            backoff: { strategy: 'exponential', delay: '10ms' },
          }
        );

        expect(result.success).toBe(true);
        expect(result.result).toBe('success');
        expect(result.attempts).toBe(3);
        expect(result.retryDelays).toHaveLength(2);
      });

      it('should fail after max attempts', async () => {
        const result = await RetryExecutor.execute(
          () => Promise.reject(new Error('Persistent failure')),
          {
            maxAttempts: 3,
            backoff: { strategy: 'exponential', delay: '10ms' },
          }
        );

        expect(result.success).toBe(false);
        expect(result.error?.message).toBe('Persistent failure');
        expect(result.attempts).toBe(3);
        expect(result.retryDelays).toHaveLength(2);
      });
    });

    describe('Backoff Strategies', () => {
      it('should use exponential backoff', async () => {
        const delays: number[] = [];
        let attempts = 0;

        await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 4) {
              throw new Error('Failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 4,
            backoff: { strategy: 'exponential', delay: '100ms' },
            onRetry: (attempt, error, delay) => {
              delays.push(delay);
            },
          }
        );

        // Exponential backoff: 100ms, 200ms, 400ms
        expect(delays).toHaveLength(3);
        expect(delays[0]).toBeGreaterThanOrEqual(50); // With jitter
        expect(delays[1]).toBeGreaterThanOrEqual(100);
        expect(delays[2]).toBeGreaterThanOrEqual(200);
      });

      it('should use fixed backoff', async () => {
        const delays: number[] = [];
        let attempts = 0;

        await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 4) {
              throw new Error('Failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 4,
            backoff: { strategy: 'fixed', delay: '100ms' },
            onRetry: (attempt, error, delay) => {
              delays.push(delay);
            },
          }
        );

        // Fixed backoff: ~100ms each time (with jitter)
        expect(delays).toHaveLength(3);
        delays.forEach(delay => {
          expect(delay).toBeGreaterThanOrEqual(50);
          expect(delay).toBeLessThanOrEqual(150);
        });
      });
    });

    describe('Jitter', () => {
      it('should add jitter to delays', async () => {
        const delays: number[] = [];
        let attempts = 0;

        await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 3) {
              throw new Error('Failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 3,
            backoff: { strategy: 'fixed', delay: '100ms' },
            jitter: true,
            onRetry: (attempt, error, delay) => {
              delays.push(delay);
            },
          }
        );

        expect(delays).toHaveLength(2);
        // With jitter, delays should vary
        expect(delays[0]).not.toBe(delays[1]);
      });

      it('should work without jitter', async () => {
        const delays: number[] = [];
        let attempts = 0;

        await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 3) {
              throw new Error('Failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 3,
            backoff: { strategy: 'fixed', delay: '100ms' },
            jitter: false,
            onRetry: (attempt, error, delay) => {
              delays.push(delay);
            },
          }
        );

        expect(delays).toHaveLength(2);
        // Without jitter, delays should be consistent
        expect(delays[0]).toBe(100);
        expect(delays[1]).toBe(100);
      });
    });

    describe('Max Backoff', () => {
      it('should cap delays at max backoff', async () => {
        const delays: number[] = [];
        let attempts = 0;

        await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 5) {
              throw new Error('Failure');
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 5,
            backoff: { strategy: 'exponential', delay: '100ms' },
            maxBackoff: '200ms',
            onRetry: (attempt, error, delay) => {
              delays.push(delay);
            },
          }
        );

        expect(delays).toHaveLength(4);
        delays.forEach(delay => {
          // With jitter, delays can exceed maxBackoff by up to 50%
          expect(delay).toBeLessThanOrEqual(300); // 200ms * 1.5 (max jitter)
        });
      });
    });

    describe('Error Filtering', () => {
      it('should retry retryable errors', async () => {
        let attempts = 0;
        const result = await RetryExecutor.execute(
          () => {
            attempts++;
            if (attempts < 3) {
              const error = new Error('ECONNRESET');
              (error as any).code = 'ECONNRESET';
              throw error;
            }
            return Promise.resolve('success');
          },
          {
            maxAttempts: 3,
            backoff: { strategy: 'exponential', delay: '10ms' },
          }
        );

        expect(result.success).toBe(true);
        expect(result.attempts).toBe(3);
      });

      it('should not retry non-retryable errors', async () => {
        const result = await RetryExecutor.execute(
          () => {
            throw new Error('Validation error - should not retry');
          },
          {
            maxAttempts: 3,
            backoff: { strategy: 'exponential', delay: '10ms' },
            shouldRetry: error => error.message.includes('network'),
          }
        );

        expect(result.success).toBe(false);
        expect(result.attempts).toBe(1);
        expect(result.error?.message).toBe(
          'Validation error - should not retry'
        );
      });
    });

    describe('Duration Parsing', () => {
      it('should parse various duration formats', () => {
        const testCases = [
          { input: '100ms', expected: 100 },
          { input: '1s', expected: 1000 },
          { input: '2m', expected: 120000 },
          { input: '1h', expected: 3600000 },
          { input: '1d', expected: 86400000 },
          { input: 5000, expected: 5000 },
        ];

        testCases.forEach(({ input, expected }) => {
          const result = RetryExecutor.execute(
            () => Promise.resolve('success'),
            {
              maxAttempts: 1,
              backoff: { strategy: 'fixed', delay: input },
            }
          );
          expect(result).toBeDefined();
        });
      });

      it('should throw error for invalid duration format', async () => {
        await expect(
          RetryExecutor.execute(() => Promise.resolve('success'), {
            maxAttempts: 1,
            backoff: { strategy: 'fixed', delay: 'invalid' },
          })
        ).rejects.toThrow('Invalid duration format');
      });
    });
  });

  describe('StepExecutor', () => {
    describe('Step Execution with Retry', () => {
      it('should execute step without retry', async () => {
        const step = {
          id: 'test-step',
          name: 'test-step',
          handler: async (ctx: Context) => 'success',
          type: 'step' as const,
        };

        const context: Context = {
          payload: {},
          steps: {},
          services: {},
          run: { id: 'test', workflowId: 'test' },
          state: {
            get: () => null,
            set: async () => {},
            incr: async () => 0,
          },
          last: null,
          trigger: { headers: {} },
          cancel: () => {
            throw new Error('cancelled');
          },
        };

        const result = await StepExecutor.executeStep(step, context);

        expect(result.success).toBe(true);
        expect(result.output).toBe('success');
        expect(result.attempts).toBe(1);
      });

      it('should execute step with retry', async () => {
        let attempts = 0;
        const step = {
          id: 'test-step',
          name: 'test-step',
          handler: async (ctx: Context) => {
            attempts++;
            if (attempts < 3) {
              throw new Error('Temporary network failure');
            }
            return 'success';
          },
          type: 'step' as const,
          options: {
            retry: {
              attempts: 3,
              backoff: { strategy: 'exponential' as const, delay: '10ms' },
            },
          },
        };

        const context: Context = {
          payload: {},
          steps: {},
          services: {},
          run: { id: 'test', workflowId: 'test' },
          state: {
            get: () => null,
            set: async () => {},
            incr: async () => 0,
          },
          last: null,
          trigger: { headers: {} },
          cancel: () => {
            throw new Error('cancelled');
          },
        };

        const result = await StepExecutor.executeStep(
          step,
          context,
          step.options
        );

        expect(result.success).toBe(true);
        expect(result.output).toBe('success');
        expect(result.attempts).toBe(3);
        expect(result.retryDelays).toHaveLength(2);
      });
    });

    describe('Timeout Handling', () => {
      it('should timeout step execution', async () => {
        const step = {
          id: 'test-step',
          name: 'test-step',
          handler: async (ctx: Context) => {
            await new Promise(resolve => setTimeout(resolve, 1000));
            return 'success';
          },
          type: 'step' as const,
          options: {
            timeout: '50ms',
          },
        };

        const context: Context = {
          payload: {},
          steps: {},
          services: {},
          run: { id: 'test', workflowId: 'test' },
          state: {
            get: () => null,
            set: async () => {},
            incr: async () => 0,
          },
          last: null,
          trigger: { headers: {} },
          cancel: () => {
            throw new Error('cancelled');
          },
        };

        const result = await StepExecutor.executeStepWithTimeout(
          step,
          context,
          step.options
        );

        expect(result.success).toBe(false);
        expect(result.error?.message).toContain('timed out');
      });
    });

    describe('Error Handling', () => {
      it('should use custom error handler', async () => {
        const step = {
          id: 'test-step',
          name: 'test-step',
          handler: async (ctx: Context) => {
            throw new Error('Step failed');
          },
          type: 'step' as const,
          options: {
            onError: async (ctx: Context) => {
              return 'fallback result';
            },
          },
        };

        const context: Context = {
          payload: {},
          steps: {},
          services: {},
          run: { id: 'test', workflowId: 'test' },
          state: {
            get: () => null,
            set: async () => {},
            incr: async () => 0,
          },
          last: null,
          trigger: { headers: {} },
          cancel: () => {
            throw new Error('cancelled');
          },
        };

        const result = await StepExecutor.executeStepWithErrorHandling(
          step,
          context,
          step.options
        );

        expect(result.success).toBe(true);
        expect(result.output).toBe('fallback result');
      });
    });
  });

  describe('Integration with Workflow', () => {
    it('should retry failed steps in workflow', async () => {
      const workflow = cronflow.define({
        id: 'retry-test-workflow',
        name: 'Retry Test Workflow',
      });

      let attempts = 0;
      workflow
        .step('unreliable-step', async (ctx: Context) => {
          attempts++;
          if (attempts < 3) {
            throw new Error('Temporary network failure');
          }
          return { success: true, attempt: attempts };
        })
        .retry({
          attempts: 3,
          backoff: { strategy: 'exponential', delay: '10ms' },
        });

      const testRun = await workflow
        .test()
        .trigger({ test: 'data' })
        .expectStep('unreliable-step')
        .toSucceed()
        .run();

      expect(testRun.status).toBe('completed');
      expect(testRun.steps).toHaveLength(1);
      expect(testRun.steps[0].status).toBe('completed');
      expect(testRun.steps[0].attempts).toBe(3);
      expect(testRun.steps[0].retryDelays).toHaveLength(2);
    });

    it('should handle retry exhaustion', async () => {
      const workflow = cronflow.define({
        id: 'retry-exhaustion-workflow',
        name: 'Retry Exhaustion Workflow',
      });

      workflow
        .step('always-failing-step', async (ctx: Context) => {
          throw new Error('Validation error - should not retry');
        })
        .retry({
          attempts: 2,
          backoff: { strategy: 'fixed', delay: '10ms' },
        });

      const testRun = await workflow
        .test()
        .trigger({ test: 'data' })
        .expectStep('always-failing-step')
        .toFail()
        .run();

      expect(testRun.status).toBe('completed');
      expect(testRun.steps).toHaveLength(1);
      expect(testRun.steps[0].status).toBe('failed');
      expect(testRun.steps[0].error?.message).toContain('Validation error');
    });
  });
});
