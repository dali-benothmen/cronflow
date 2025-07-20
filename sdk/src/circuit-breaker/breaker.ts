export interface CircuitBreakerOptions {
  failureThreshold: number;
  recoveryTimeout: string | number;
  expectedErrors?: string[];
  onStateChange?: (state: CircuitState, previousState: CircuitState) => void;
  onFailure?: (error: Error, failureCount: number) => void;
  onSuccess?: (successCount: number) => void;
}

export interface CircuitBreakerStats {
  totalRequests: number;
  successfulRequests: number;
  failedRequests: number;
  currentFailureCount: number;
  currentSuccessCount: number;
  lastFailureTime?: Date;
  lastSuccessTime?: Date;
  state: CircuitState;
  stateChangeTime: Date;
}

export type CircuitState = 'CLOSED' | 'OPEN' | 'HALF_OPEN';

export class CircuitBreaker {
  private state: CircuitState = 'CLOSED';
  private failureCount = 0;
  private successCount = 0;
  private lastFailureTime?: Date;
  private lastSuccessTime?: Date;
  private stateChangeTime = new Date();
  private recoveryTimeoutId?: NodeJS.Timeout;

  constructor(
    private readonly name: string,
    private readonly options: CircuitBreakerOptions
  ) {}

  async execute<T>(fn: () => Promise<T>): Promise<T> {
    if (this.state === 'OPEN') {
      if (this.shouldAttemptReset()) {
        this.transitionToHalfOpen();
      } else {
        throw new Error(`Circuit breaker '${this.name}' is OPEN`);
      }
    }

    try {
      const result = await fn();
      this.onSuccess();
      return result;
    } catch (error) {
      this.onFailure(error instanceof Error ? error : new Error(String(error)));
      throw error;
    }
  }

  private onSuccess(): void {
    this.successCount++;
    this.lastSuccessTime = new Date();

    if (this.state === 'HALF_OPEN') {
      this.transitionToClosed();
    }

    this.options.onSuccess?.(this.successCount);
  }

  private onFailure(error: Error): void {
    this.failureCount++;
    this.lastFailureTime = new Date();

    if (this.shouldOpenCircuit(error)) {
      this.transitionToOpen();
    }

    this.options.onFailure?.(error, this.failureCount);
  }

  private shouldOpenCircuit(error: Error): boolean {
    if (this.state === 'HALF_OPEN') {
      return true;
    }

    if (this.failureCount >= this.options.failureThreshold) {
      if (
        this.options.expectedErrors &&
        this.options.expectedErrors.length > 0
      ) {
        return this.options.expectedErrors.some(expectedError =>
          error.message.includes(expectedError)
        );
      }
      return true;
    }

    return false;
  }

  private shouldAttemptReset(): boolean {
    if (!this.lastFailureTime) return false;

    const recoveryTimeoutMs = this.parseDuration(this.options.recoveryTimeout);
    const timeSinceLastFailure = Date.now() - this.lastFailureTime.getTime();

    return timeSinceLastFailure >= recoveryTimeoutMs;
  }

  private transitionToOpen(): void {
    const previousState = this.state;
    this.state = 'OPEN';
    this.stateChangeTime = new Date();

    this.options.onStateChange?.(this.state, previousState);

    this.recoveryTimeoutId = setTimeout(() => {
      this.transitionToHalfOpen();
    }, this.parseDuration(this.options.recoveryTimeout));
  }

  private transitionToHalfOpen(): void {
    const previousState = this.state;
    this.state = 'HALF_OPEN';
    this.stateChangeTime = new Date();

    this.options.onStateChange?.(this.state, previousState);
  }

  private transitionToClosed(): void {
    const previousState = this.state;
    this.state = 'CLOSED';
    this.stateChangeTime = new Date();
    this.failureCount = 0;
    this.successCount = 0;

    this.options.onStateChange?.(this.state, previousState);
  }

  private parseDuration(duration: string | number): number {
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

  getStats(): CircuitBreakerStats {
    return {
      totalRequests: this.failureCount + this.successCount,
      successfulRequests: this.successCount,
      failedRequests: this.failureCount,
      currentFailureCount: this.failureCount,
      currentSuccessCount: this.successCount,
      lastFailureTime: this.lastFailureTime,
      lastSuccessTime: this.lastSuccessTime,
      state: this.state,
      stateChangeTime: this.stateChangeTime,
    };
  }

  getState(): CircuitState {
    return this.state;
  }

  isOpen(): boolean {
    return this.state === 'OPEN';
  }

  isHalfOpen(): boolean {
    return this.state === 'HALF_OPEN';
  }

  isClosed(): boolean {
    return this.state === 'CLOSED';
  }

  reset(): void {
    this.state = 'CLOSED';
    this.failureCount = 0;
    this.successCount = 0;
    this.lastFailureTime = undefined;
    this.lastSuccessTime = undefined;
    this.stateChangeTime = new Date();

    if (this.recoveryTimeoutId) {
      clearTimeout(this.recoveryTimeoutId);
      this.recoveryTimeoutId = undefined;
    }
  }

  destroy(): void {
    if (this.recoveryTimeoutId) {
      clearTimeout(this.recoveryTimeoutId);
    }
  }
}
