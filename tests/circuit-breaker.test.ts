import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  CircuitBreaker,
  CircuitBreakerManager,
} from '../sdk/src/circuit-breaker';
import type { Context } from '../sdk/src/workflow/types';

describe('Circuit Breaker Implementation', () => {
  describe('CircuitBreaker', () => {
    let circuitBreaker: CircuitBreaker;

    beforeEach(() => {
      circuitBreaker = new CircuitBreaker('test-breaker', {
        failureThreshold: 3,
        recoveryTimeout: '1s',
      });
    });

    afterEach(() => {
      circuitBreaker.destroy();
    });

    describe('Basic Circuit Breaker Functionality', () => {
      it('should execute successfully when circuit is closed', async () => {
        const result = await circuitBreaker.execute(async () => 'success');
        expect(result).toBe('success');
        expect(circuitBreaker.getState()).toBe('CLOSED');
      });

      it('should open circuit after failure threshold is reached', async () => {
        let failureCount = 0;
        const failingFunction = async () => {
          failureCount++;
          throw new Error('Service unavailable');
        };

        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(failingFunction);
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');
        expect(failureCount).toBe(3);
      });

      it('should reject requests when circuit is open', async () => {
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');

        try {
          await circuitBreaker.execute(async () => 'should fail');
          expect(true).toBe(false);
        } catch (error) {
          expect((error as Error).message).toContain(
            "Circuit breaker 'test-breaker' is OPEN"
          );
        }
      });
    });

    describe('Circuit State Transitions', () => {
      it('should transition from CLOSED to OPEN after failures', async () => {
        expect(circuitBreaker.getState()).toBe('CLOSED');

        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');
      });

      it('should transition from OPEN to HALF_OPEN after recovery timeout', async () => {
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');

        await new Promise(resolve => setTimeout(resolve, 1100));

        expect(circuitBreaker.getState()).toBe('HALF_OPEN');
      });

      it('should transition from HALF_OPEN to CLOSED on success', async () => {
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        await new Promise(resolve => setTimeout(resolve, 1100));

        const result = await circuitBreaker.execute(async () => 'success');
        expect(result).toBe('success');
        expect(circuitBreaker.getState()).toBe('CLOSED');
      });

      it('should transition from HALF_OPEN to OPEN on failure', async () => {
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        await new Promise(resolve => setTimeout(resolve, 1100));

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('Service still unavailable');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        expect(circuitBreaker.getState()).toBe('OPEN');
      });
    });

    describe('Error Filtering', () => {
      it('should only open circuit for expected errors', async () => {
        const circuitBreaker = new CircuitBreaker('filtered-breaker', {
          failureThreshold: 2,
          recoveryTimeout: '1s',
          expectedErrors: ['network', 'timeout'],
        });

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('network error');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('timeout error');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        expect(circuitBreaker.getState()).toBe('OPEN');
      });

      it('should not open circuit for unexpected errors', async () => {
        const circuitBreaker = new CircuitBreaker('filtered-breaker', {
          failureThreshold: 2,
          recoveryTimeout: '1s',
          expectedErrors: ['network', 'timeout'],
        });

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('validation error');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('authorization error');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        expect(circuitBreaker.getState()).toBe('CLOSED');
      });
    });

    describe('Statistics and Monitoring', () => {
      it('should track statistics correctly', async () => {
        const stats = circuitBreaker.getStats();
        expect(stats.totalRequests).toBe(0);
        expect(stats.successfulRequests).toBe(0);
        expect(stats.failedRequests).toBe(0);
        expect(stats.state).toBe('CLOSED');

        await circuitBreaker.execute(async () => 'success');

        const statsAfterSuccess = circuitBreaker.getStats();
        expect(statsAfterSuccess.totalRequests).toBe(1);
        expect(statsAfterSuccess.successfulRequests).toBe(1);
        expect(statsAfterSuccess.failedRequests).toBe(0);

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('Service unavailable');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        const statsAfterFailure = circuitBreaker.getStats();
        expect(statsAfterFailure.totalRequests).toBe(2);
        expect(statsAfterFailure.successfulRequests).toBe(1);
        expect(statsAfterFailure.failedRequests).toBe(1);
      });

      it('should call callbacks on state changes', async () => {
        let stateChanges: string[] = [];
        let failures: Error[] = [];
        let successes: number[] = [];

        const circuitBreaker = new CircuitBreaker('callback-breaker', {
          failureThreshold: 2,
          recoveryTimeout: '1s',
          onStateChange: (state, previousState) => {
            stateChanges.push(`${previousState} → ${state}`);
          },
          onFailure: (error, count) => {
            failures.push(error);
          },
          onSuccess: count => {
            successes.push(count);
          },
        });

        await circuitBreaker.execute(async () => 'success');
        expect(successes).toContain(1);

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('Service unavailable');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        try {
          await circuitBreaker.execute(async () => {
            throw new Error('Service unavailable');
          });
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
        }

        expect(failures).toHaveLength(2);
        expect(stateChanges).toContain('CLOSED → OPEN');
      });
    });

    describe('Utility Methods', () => {
      it('should provide state checking methods', () => {
        expect(circuitBreaker.isClosed()).toBe(true);
        expect(circuitBreaker.isOpen()).toBe(false);
        expect(circuitBreaker.isHalfOpen()).toBe(false);
      });

      it('should reset circuit breaker state', async () => {
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');

        circuitBreaker.reset();
        expect(circuitBreaker.getState()).toBe('CLOSED');

        const stats = circuitBreaker.getStats();
        expect(stats.failedRequests).toBe(0);
        expect(stats.successfulRequests).toBe(0);
      });
    });
  });

  describe('CircuitBreakerManager', () => {
    let manager: CircuitBreakerManager;

    beforeEach(() => {
      manager = new CircuitBreakerManager({
        defaultFailureThreshold: 3,
        defaultRecoveryTimeout: '1s',
        enableLogging: false,
      });
    });

    afterEach(() => {
      manager.destroyAllCircuitBreakers();
    });

    describe('Circuit Breaker Management', () => {
      it('should create circuit breakers with default options', () => {
        const circuitBreaker = manager.createCircuitBreaker('test-breaker');
        expect(circuitBreaker).toBeInstanceOf(CircuitBreaker);
        expect(circuitBreaker.getState()).toBe('CLOSED');
      });

      it('should create circuit breakers with custom options', () => {
        const circuitBreaker = manager.createCircuitBreaker('custom-breaker', {
          failureThreshold: 5,
          recoveryTimeout: '2s',
          expectedErrors: ['network'],
        });

        expect(circuitBreaker).toBeInstanceOf(CircuitBreaker);
        expect(circuitBreaker.getState()).toBe('CLOSED');
      });

      it('should execute functions with circuit breaker', async () => {
        const result = await manager.executeWithCircuitBreaker(
          'test-breaker',
          async () => 'success'
        );

        expect(result).toBe('success');
      });

      it('should create circuit breaker automatically if it does not exist', async () => {
        const result = await manager.executeWithCircuitBreaker(
          'auto-created',
          async () => 'success'
        );

        expect(result).toBe('success');
        expect(manager.getCircuitBreaker('auto-created')).toBeDefined();
      });
    });

    describe('Statistics and Monitoring', () => {
      it('should provide global statistics', async () => {
        const circuitBreaker1 = manager.createCircuitBreaker('breaker1');
        const circuitBreaker2 = manager.createCircuitBreaker('breaker2');

        await circuitBreaker1.execute(async () => 'success');
        await circuitBreaker2.execute(async () => 'success');

        const globalStats = manager.getGlobalStats();
        expect(globalStats.totalCircuitBreakers).toBe(2);
        expect(globalStats.closedCircuitBreakers).toBe(2);
        expect(globalStats.totalRequests).toBe(2);
        expect(globalStats.totalSuccesses).toBe(2);
      });

      it('should track circuit breakers in different states', async () => {
        const circuitBreaker = manager.createCircuitBreaker('test-breaker');

        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        const globalStats = manager.getGlobalStats();
        expect(globalStats.openCircuitBreakers).toBe(1);
        expect(globalStats.totalFailures).toBe(3);
      });

      it('should provide individual circuit breaker statistics', () => {
        const circuitBreaker = manager.createCircuitBreaker('test-breaker');
        const stats = manager.getCircuitBreakerStats('test-breaker');

        expect(stats).toBeDefined();
        expect(stats?.state).toBe('CLOSED');
        expect(stats?.totalRequests).toBe(0);
      });

      it('should provide circuit breaker state', () => {
        const circuitBreaker = manager.createCircuitBreaker('test-breaker');
        const state = manager.getCircuitBreakerState('test-breaker');

        expect(state).toBe('CLOSED');
      });
    });

    describe('Circuit Breaker Lifecycle', () => {
      it('should reset individual circuit breakers', async () => {
        const circuitBreaker = manager.createCircuitBreaker('test-breaker');

        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker.getState()).toBe('OPEN');

        const reset = manager.resetCircuitBreaker('test-breaker');
        expect(reset).toBe(true);
        expect(circuitBreaker.getState()).toBe('CLOSED');
      });

      it('should reset all circuit breakers', async () => {
        const circuitBreaker1 = manager.createCircuitBreaker('breaker1');
        const circuitBreaker2 = manager.createCircuitBreaker('breaker2');

        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker1.execute(async () => {
              throw new Error('Service unavailable');
            });
          } catch (error) {
            expect(error).toBeInstanceOf(Error);
          }
        }

        expect(circuitBreaker1.getState()).toBe('OPEN');
        expect(circuitBreaker2.getState()).toBe('CLOSED');

        manager.resetAllCircuitBreakers();

        expect(circuitBreaker1.getState()).toBe('CLOSED');
        expect(circuitBreaker2.getState()).toBe('CLOSED');
      });

      it('should destroy individual circuit breakers', () => {
        manager.createCircuitBreaker('test-breaker');
        expect(manager.getCircuitBreaker('test-breaker')).toBeDefined();

        const destroyed = manager.destroyCircuitBreaker('test-breaker');
        expect(destroyed).toBe(true);
        expect(manager.getCircuitBreaker('test-breaker')).toBeUndefined();
      });

      it('should destroy all circuit breakers', () => {
        manager.createCircuitBreaker('breaker1');
        manager.createCircuitBreaker('breaker2');

        expect(manager.getCircuitBreaker('breaker1')).toBeDefined();
        expect(manager.getCircuitBreaker('breaker2')).toBeDefined();

        manager.destroyAllCircuitBreakers();

        expect(manager.getCircuitBreaker('breaker1')).toBeUndefined();
        expect(manager.getCircuitBreaker('breaker2')).toBeUndefined();
      });
    });

    describe('Circuit Breaker Information', () => {
      it('should provide information about all circuit breakers', () => {
        manager.createCircuitBreaker('breaker1', { failureThreshold: 5 });
        manager.createCircuitBreaker('breaker2', { failureThreshold: 3 });

        const circuitBreakers = manager.getAllCircuitBreakers();

        expect(circuitBreakers).toHaveLength(2);
        expect(circuitBreakers[0].name).toBe('breaker1');
        expect(circuitBreakers[0].state).toBe('CLOSED');
        expect(circuitBreakers[0].options.failureThreshold).toBe(5);
        expect(circuitBreakers[1].name).toBe('breaker2');
        expect(circuitBreakers[1].options.failureThreshold).toBe(3);
      });
    });
  });

  describe('Integration with Step Execution', () => {
    it('should integrate circuit breaker with step execution', async () => {
      const { StepExecutor } = await import(
        '../sdk/src/execution/step-executor'
      );

      const step = {
        id: 'test-step',
        name: 'test-step',
        handler: async (ctx: Context) => {
          throw new Error('Service unavailable');
        },
        type: 'step' as const,
        options: {
          circuitBreaker: {
            name: 'test-service',
            failureThreshold: 2,
            recoveryTimeout: '1s',
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

      try {
        await StepExecutor.executeStep(step, context, step.options);
        expect(true).toBe(false);
      } catch (error) {
        expect(error).toBeInstanceOf(Error);
      }

      const circuitBreakerManager = StepExecutor.getCircuitBreakerManager();
      const circuitBreaker =
        circuitBreakerManager.getCircuitBreaker('test-service');

      expect(circuitBreaker).toBeDefined();
      expect(circuitBreaker?.getState()).toBe('CLOSED');

      try {
        await StepExecutor.executeStep(step, context, step.options);
        expect(true).toBe(false);
      } catch (error) {
        expect(error).toBeInstanceOf(Error);
      }

      expect(circuitBreaker?.getState()).toBe('OPEN');
    });
  });
});
