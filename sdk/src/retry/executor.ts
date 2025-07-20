import { Context } from '../workflow/types';

export interface RetryResult<T = any> {
  success: boolean;
  result?: T;
  error?: Error;
  attempts: number;
  totalDuration: number;
  retryDelays: number[];
}

export interface RetryOptions {
  maxAttempts: number;
  backoff: {
    strategy: 'exponential' | 'fixed';
    delay: string | number;
  };
  jitter?: boolean;
  maxBackoff?: string | number;
  onRetry?: (attempt: number, error: Error, delay: number) => void;
  shouldRetry?: (error: Error) => boolean;
}

export class RetryExecutor {
  static async execute<T>(
    fn: () => Promise<T>,
    options: RetryOptions,
    ctx?: Context
  ): Promise<RetryResult<T>> {
    const startTime = Date.now();
    const retryDelays: number[] = [];
    let lastError: Error | undefined;

    const baseDelay = this.parseDuration(options.backoff.delay);
    const maxBackoff = options.maxBackoff
      ? this.parseDuration(options.maxBackoff)
      : baseDelay * 10;

    for (let attempt = 1; attempt <= options.maxAttempts; attempt++) {
      try {
        const result = await fn();
        return {
          success: true,
          result,
          attempts: attempt,
          totalDuration: Date.now() - startTime,
          retryDelays,
        };
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));

        if (options.shouldRetry && !options.shouldRetry(lastError)) {
          return {
            success: false,
            error: lastError,
            attempts: attempt,
            totalDuration: Date.now() - startTime,
            retryDelays,
          };
        }

        if (attempt === options.maxAttempts) {
          break;
        }

        const delay = this.calculateDelay(
          attempt,
          baseDelay,
          options.backoff.strategy,
          maxBackoff,
          options.jitter
        );

        retryDelays.push(delay);

        if (options.onRetry) {
          options.onRetry(attempt, lastError, delay);
        }

        console.log(
          `🔄 Retry attempt ${attempt}/${options.maxAttempts} for step, waiting ${delay}ms`
        );

        await this.sleep(delay);
      }
    }

    return {
      success: false,
      error: lastError,
      attempts: options.maxAttempts,
      totalDuration: Date.now() - startTime,
      retryDelays,
    };
  }

  private static calculateDelay(
    attempt: number,
    baseDelay: number,
    strategy: 'exponential' | 'fixed',
    maxBackoff: number,
    jitter: boolean = true
  ): number {
    let delay: number;

    if (strategy === 'exponential') {
      delay = baseDelay * Math.pow(2, attempt - 1);
    } else {
      delay = baseDelay;
    }

    delay = Math.min(delay, maxBackoff);

    if (jitter) {
      const jitterFactor = 0.5 + Math.random();
      delay = Math.floor(delay * jitterFactor);
    }

    return delay;
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

  private static sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  static shouldRetryByDefault(error: Error): boolean {
    const retryableErrors = [
      'ECONNRESET',
      'ECONNREFUSED',
      'ENOTFOUND',
      'ETIMEDOUT',
      'ENETUNREACH',
      'ECONNRESET',
      'EPIPE',
    ];

    const retryableMessages = [
      'timeout',
      'network',
      'connection',
      'server error',
      'service unavailable',
      'internal server error',
    ];

    if ((error as any).code && retryableErrors.includes((error as any).code)) {
      return true;
    }

    const message = error.message.toLowerCase();
    return retryableMessages.some(keyword => message.includes(keyword));
  }

  static createRetryOptions(
    retryConfig: any,
    defaultOptions: Partial<RetryOptions> = {}
  ): RetryOptions {
    return {
      maxAttempts: retryConfig.attempts || 3,
      backoff: {
        strategy: retryConfig.backoff?.strategy || 'exponential',
        delay: retryConfig.backoff?.delay || '1s',
      },
      jitter: true,
      maxBackoff: retryConfig.backoff?.maxDelay || '30s',
      shouldRetry: RetryExecutor.shouldRetryByDefault,
      ...defaultOptions,
    };
  }
}
