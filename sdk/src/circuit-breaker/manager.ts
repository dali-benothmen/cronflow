import {
  CircuitBreaker,
  CircuitBreakerOptions,
  CircuitBreakerStats,
  CircuitState,
} from './breaker';

export interface CircuitBreakerManagerOptions {
  defaultFailureThreshold?: number;
  defaultRecoveryTimeout?: string | number;
  enableLogging?: boolean;
}

export interface CircuitBreakerInfo {
  name: string;
  state: CircuitState;
  stats: CircuitBreakerStats;
  options: CircuitBreakerOptions;
}

export class CircuitBreakerManager {
  private circuitBreakers = new Map<string, CircuitBreaker>();
  private defaultOptions: Required<CircuitBreakerManagerOptions>;

  constructor(options: CircuitBreakerManagerOptions = {}) {
    this.defaultOptions = {
      defaultFailureThreshold: options.defaultFailureThreshold || 5,
      defaultRecoveryTimeout: options.defaultRecoveryTimeout || '30s',
      enableLogging: options.enableLogging ?? true,
    };
  }

  createCircuitBreaker(
    name: string,
    options: Partial<CircuitBreakerOptions> = {}
  ): CircuitBreaker {
    const circuitBreakerOptions: CircuitBreakerOptions = {
      failureThreshold:
        options.failureThreshold || this.defaultOptions.defaultFailureThreshold,
      recoveryTimeout:
        options.recoveryTimeout || this.defaultOptions.defaultRecoveryTimeout,
      expectedErrors: options.expectedErrors,
      onStateChange: (state, previousState) => {
        if (this.defaultOptions.enableLogging) {
          console.log(
            `🔄 Circuit breaker '${name}' state changed: ${previousState} → ${state}`
          );
        }
        options.onStateChange?.(state, previousState);
      },
      onFailure: (error, failureCount) => {
        if (this.defaultOptions.enableLogging) {
          console.log(
            `❌ Circuit breaker '${name}' failure (${failureCount}): ${error.message}`
          );
        }
        options.onFailure?.(error, failureCount);
      },
      onSuccess: successCount => {
        if (this.defaultOptions.enableLogging) {
          console.log(`✅ Circuit breaker '${name}' success (${successCount})`);
        }
        options.onSuccess?.(successCount);
      },
    };

    const circuitBreaker = new CircuitBreaker(name, circuitBreakerOptions);
    this.circuitBreakers.set(name, circuitBreaker);

    if (this.defaultOptions.enableLogging) {
      console.log(
        `🔧 Created circuit breaker '${name}' with threshold ${circuitBreakerOptions.failureThreshold}`
      );
    }

    return circuitBreaker;
  }

  getCircuitBreaker(name: string): CircuitBreaker | undefined {
    return this.circuitBreakers.get(name);
  }

  async executeWithCircuitBreaker<T>(
    name: string,
    fn: () => Promise<T>,
    options: Partial<CircuitBreakerOptions> = {}
  ): Promise<T> {
    let circuitBreaker = this.getCircuitBreaker(name);

    if (!circuitBreaker) {
      circuitBreaker = this.createCircuitBreaker(name, options);
    }

    return circuitBreaker.execute(fn);
  }

  getAllCircuitBreakers(): CircuitBreakerInfo[] {
    return Array.from(this.circuitBreakers.entries()).map(
      ([name, circuitBreaker]) => ({
        name,
        state: circuitBreaker.getState(),
        stats: circuitBreaker.getStats(),
        options: {
          failureThreshold: circuitBreaker['options'].failureThreshold,
          recoveryTimeout: circuitBreaker['options'].recoveryTimeout,
          expectedErrors: circuitBreaker['options'].expectedErrors,
        },
      })
    );
  }

  getCircuitBreakerStats(name: string): CircuitBreakerStats | undefined {
    const circuitBreaker = this.getCircuitBreaker(name);
    return circuitBreaker?.getStats();
  }

  getCircuitBreakerState(name: string): CircuitState | undefined {
    const circuitBreaker = this.getCircuitBreaker(name);
    return circuitBreaker?.getState();
  }

  resetCircuitBreaker(name: string): boolean {
    const circuitBreaker = this.getCircuitBreaker(name);
    if (circuitBreaker) {
      circuitBreaker.reset();
      if (this.defaultOptions.enableLogging) {
        console.log(`🔄 Reset circuit breaker '${name}'`);
      }
      return true;
    }
    return false;
  }

  resetAllCircuitBreakers(): void {
    for (const [name, circuitBreaker] of this.circuitBreakers) {
      circuitBreaker.reset();
      if (this.defaultOptions.enableLogging) {
        console.log(`🔄 Reset circuit breaker '${name}'`);
      }
    }
  }

  destroyCircuitBreaker(name: string): boolean {
    const circuitBreaker = this.getCircuitBreaker(name);
    if (circuitBreaker) {
      circuitBreaker.destroy();
      this.circuitBreakers.delete(name);
      if (this.defaultOptions.enableLogging) {
        console.log(`🗑️  Destroyed circuit breaker '${name}'`);
      }
      return true;
    }
    return false;
  }

  destroyAllCircuitBreakers(): void {
    for (const [name, circuitBreaker] of this.circuitBreakers) {
      circuitBreaker.destroy();
      if (this.defaultOptions.enableLogging) {
        console.log(`🗑️  Destroyed circuit breaker '${name}'`);
      }
    }
    this.circuitBreakers.clear();
  }

  getGlobalStats(): {
    totalCircuitBreakers: number;
    openCircuitBreakers: number;
    halfOpenCircuitBreakers: number;
    closedCircuitBreakers: number;
    totalRequests: number;
    totalFailures: number;
    totalSuccesses: number;
  } {
    let openCount = 0;
    let halfOpenCount = 0;
    let closedCount = 0;
    let totalRequests = 0;
    let totalFailures = 0;
    let totalSuccesses = 0;

    for (const circuitBreaker of this.circuitBreakers.values()) {
      const state = circuitBreaker.getState();
      const stats = circuitBreaker.getStats();

      switch (state) {
        case 'OPEN':
          openCount++;
          break;
        case 'HALF_OPEN':
          halfOpenCount++;
          break;
        case 'CLOSED':
          closedCount++;
          break;
      }

      totalRequests += stats.totalRequests;
      totalFailures += stats.failedRequests;
      totalSuccesses += stats.successfulRequests;
    }

    return {
      totalCircuitBreakers: this.circuitBreakers.size,
      openCircuitBreakers: openCount,
      halfOpenCircuitBreakers: halfOpenCount,
      closedCircuitBreakers: closedCount,
      totalRequests,
      totalFailures,
      totalSuccesses,
    };
  }
}
