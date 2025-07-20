import { Context, StepDefinition, StepOptions } from '../workflow/types';
import { RetryExecutor, RetryOptions } from '../retry';
import { CircuitBreakerManager } from '../circuit-breaker';
import { PerformanceOptimizer } from '../performance';

export interface StepExecutionResult {
  success: boolean;
  output?: any;
  error?: Error;
  attempts: number;
  totalDuration: number;
  retryDelays: number[];
}

export class StepExecutor {
  private static circuitBreakerManager = new CircuitBreakerManager();
  private static performanceOptimizer = new PerformanceOptimizer();

  static async executeStep(
    step: StepDefinition,
    context: Context,
    options?: StepOptions
  ): Promise<StepExecutionResult> {
    const startTime = Date.now();

    if (!options?.retry && !options?.circuitBreaker) {
      try {
        const output = await step.handler(context);
        const serializedOutput =
          this.performanceOptimizer.optimizeSerialization(output);

        return {
          success: true,
          output: JSON.parse(serializedOutput), // Parse back to object
          attempts: 1,
          totalDuration: Date.now() - startTime,
          retryDelays: [],
        };
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error : new Error(String(error)),
          attempts: 1,
          totalDuration: Date.now() - startTime,
          retryDelays: [],
        };
      }
    }

    const executeWithCircuitBreaker = async () => {
      if (options?.circuitBreaker) {
        const circuitBreakerName =
          options.circuitBreaker.name || `step-${step.name}`;
        return await this.circuitBreakerManager.executeWithCircuitBreaker(
          circuitBreakerName,
          () => step.handler(context),
          options.circuitBreaker
        );
      }
      return await step.handler(context);
    };

    if (!options?.retry) {
      try {
        const output = await executeWithCircuitBreaker();
        const serializedOutput =
          this.performanceOptimizer.optimizeSerialization(output);

        return {
          success: true,
          output: JSON.parse(serializedOutput), // Parse back to object
          attempts: 1,
          totalDuration: Date.now() - startTime,
          retryDelays: [],
        };
      } catch (error) {
        return {
          success: false,
          error: error instanceof Error ? error : new Error(String(error)),
          attempts: 1,
          totalDuration: Date.now() - startTime,
          retryDelays: [],
        };
      }
    }

    const retryOptions = RetryExecutor.createRetryOptions(options.retry, {
      onRetry: (attempt, error, delay) => {
        console.log(
          `🔄 Retry attempt ${attempt} for step '${step.name}': ${error.message}`
        );
        console.log(`⏳ Waiting ${delay}ms before next attempt...`);
      },
    });

    const retryResult = await RetryExecutor.execute(
      executeWithCircuitBreaker,
      retryOptions,
      context
    );

    let optimizedOutput = retryResult.result;
    if (retryResult.success && retryResult.result) {
      const serialized = this.performanceOptimizer.optimizeSerialization(
        retryResult.result
      );
      optimizedOutput = JSON.parse(serialized);
    }

    return {
      success: retryResult.success,
      output: optimizedOutput,
      error: retryResult.error,
      attempts: retryResult.attempts,
      totalDuration: retryResult.totalDuration,
      retryDelays: retryResult.retryDelays,
    };
  }

  static async executeStepWithTimeout(
    step: StepDefinition,
    context: Context,
    options?: StepOptions
  ): Promise<StepExecutionResult> {
    const timeout = options?.timeout;

    if (!timeout) {
      return this.executeStep(step, context, options);
    }

    const timeoutMs = this.parseDuration(timeout);

    return new Promise(resolve => {
      const timeoutId = setTimeout(() => {
        resolve({
          success: false,
          error: new Error(`Step '${step.name}' timed out after ${timeout}ms`),
          attempts: 1,
          totalDuration: timeoutMs,
          retryDelays: [],
        });
      }, timeoutMs);

      this.executeStep(step, context, options)
        .then(result => {
          clearTimeout(timeoutId);
          resolve(result);
        })
        .catch(error => {
          clearTimeout(timeoutId);
          resolve({
            success: false,
            error: error instanceof Error ? error : new Error(String(error)),
            attempts: 1,
            totalDuration: Date.now() - Date.now(),
            retryDelays: [],
          });
        });
    });
  }

  static async executeStepWithErrorHandling(
    step: StepDefinition,
    context: Context,
    options?: StepOptions
  ): Promise<StepExecutionResult> {
    const retryResult = await this.executeStep(step, context, options);

    if (retryResult.success) {
      return retryResult;
    }

    if (options?.onError && retryResult.error) {
      try {
        const fallbackResult = await options.onError(context);
        return {
          success: true,
          output: fallbackResult,
          attempts: retryResult.attempts,
          totalDuration: retryResult.totalDuration,
          retryDelays: retryResult.retryDelays,
        };
      } catch (fallbackError) {
        return {
          success: false,
          error:
            fallbackError instanceof Error
              ? fallbackError
              : new Error(String(fallbackError)),
          attempts: retryResult.attempts,
          totalDuration: retryResult.totalDuration,
          retryDelays: retryResult.retryDelays,
        };
      }
    }

    return retryResult;
  }

  static async executeOptimizedDatabaseQuery<T>(
    queryFn: () => Promise<T>,
    cacheKey?: string
  ): Promise<T> {
    return await this.performanceOptimizer.optimizeDatabaseQuery(
      queryFn,
      cacheKey
    );
  }

  static async getDatabaseConnection(): Promise<string> {
    return await this.performanceOptimizer.getConnection();
  }

  static releaseDatabaseConnection(connectionId: string): void {
    this.performanceOptimizer.releaseConnection(connectionId);
  }

  private static parseDuration(duration: string | number): number {
    if (typeof duration === 'number') {
      return duration;
    }

    const match = duration.match(/^(\d+)(ms|s|m|h|d)$/);
    if (!match) {
      throw new Error(`Invalid duration format: ${duration}`);
    }

    const value = parseInt(match[1], 10);
    const unit = match[2];

    switch (unit) {
      case 'ms':
        return value;
      case 's':
        return value * 1000;
      case 'm':
        return value * 60 * 1000;
      case 'h':
        return value * 60 * 60 * 1000;
      case 'd':
        return value * 24 * 60 * 60 * 1000;
      default:
        throw new Error(`Unknown duration unit: ${unit}`);
    }
  }

  static validateResult(result: StepExecutionResult): void {
    if (!result.success && !result.error) {
      throw new Error('Step execution failed but no error was provided');
    }
  }

  static createExecutionContext(baseContext: Context, error?: Error): Context {
    return {
      ...baseContext,
      error,
    };
  }

  static getCircuitBreakerManager(): CircuitBreakerManager {
    return this.circuitBreakerManager;
  }

  static getPerformanceOptimizer(): PerformanceOptimizer {
    return this.performanceOptimizer;
  }
}
