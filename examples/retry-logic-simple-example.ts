#!/usr/bin/env bun

/**
 * Simple Retry Logic Example
 *
 * This example demonstrates the retry logic implementation
 * directly without using the full workflow engine.
 */

import { RetryExecutor, type RetryOptions } from '../sdk/src/retry';
import { StepExecutor, type StepExecutionResult } from '../sdk/src/execution';
import { Context } from '../sdk/src/workflow/types';

// Simulate external API calls with different failure patterns
class ExternalAPIService {
  private callCount = 0;

  async fetchUserData(userId: string): Promise<any> {
    this.callCount++;

    // Simulate network failures for first 2 attempts
    if (this.callCount <= 2) {
      throw new Error('Network timeout - service temporarily unavailable');
    }

    return {
      id: userId,
      name: 'John Doe',
      email: 'john@example.com',
      status: 'active',
    };
  }

  async processPayment(amount: number): Promise<any> {
    this.callCount++;

    // Simulate server errors for first 3 attempts
    if (this.callCount <= 3) {
      throw new Error('Internal server error - payment processing failed');
    }

    return {
      transactionId: `txn_${Date.now()}`,
      amount,
      status: 'completed',
      timestamp: new Date().toISOString(),
    };
  }

  async validateOrder(orderId: string): Promise<any> {
    // This will always fail with a validation error (non-retryable)
    throw new Error('Validation error - invalid order ID format');
  }

  async sendNotification(userId: string, message: string): Promise<any> {
    this.callCount++;

    // Simulate intermittent failures
    if (this.callCount % 3 === 0) {
      throw new Error('Service unavailable - notification service down');
    }

    return {
      notificationId: `notif_${Date.now()}`,
      userId,
      message,
      status: 'sent',
      timestamp: new Date().toISOString(),
    };
  }
}

// Initialize the API service
const apiService = new ExternalAPIService();

console.log('🔄 Simple Retry Logic Example\n');

// Example 1: Basic Retry Logic
async function demonstrateBasicRetry() {
  console.log('📦 Example 1: Basic Retry Logic');
  console.log('='.repeat(50));

  const retryOptions: RetryOptions = {
    maxAttempts: 3,
    backoff: { strategy: 'exponential', delay: '1s' },
    jitter: true,
  };

  console.log('  🔍 Fetching user data with retry logic...');

  try {
    const result = await RetryExecutor.execute(
      () => apiService.fetchUserData('user_123'),
      retryOptions
    );

    console.log('  ✅ User data fetched successfully!');
    console.log(`  📊 Result: ${JSON.stringify(result, null, 2)}`);
  } catch (error) {
    console.log('  ❌ Failed to fetch user data:', (error as Error).message);
  }
}

// Example 2: Payment Processing with Retry
async function demonstratePaymentRetry() {
  console.log('\n💳 Example 2: Payment Processing with Retry');
  console.log('='.repeat(50));

  const retryOptions: RetryOptions = {
    maxAttempts: 5,
    backoff: { strategy: 'exponential', delay: '2s' },
    maxBackoff: '30s',
    jitter: true,
    onRetry: (attempt, error, delay) => {
      console.log(
        `  🔄 Retry attempt ${attempt}: ${error.message} (waiting ${delay}ms)`
      );
    },
  };

  console.log('  💳 Processing payment with retry logic...');

  try {
    const result = await RetryExecutor.execute(
      () => apiService.processPayment(99.99),
      retryOptions
    );

    console.log('  ✅ Payment processed successfully!');
    console.log(`  📊 Result: ${JSON.stringify(result, null, 2)}`);
  } catch (error) {
    console.log('  ❌ Payment processing failed:', (error as Error).message);
  }
}

// Example 3: Error Filtering
async function demonstrateErrorFiltering() {
  console.log('\n🎯 Example 3: Error Filtering');
  console.log('='.repeat(50));

  const retryOptions: RetryOptions = {
    maxAttempts: 3,
    backoff: { strategy: 'exponential', delay: '1s' },
    shouldRetry: error => error.message.includes('network'),
  };

  console.log('  📋 Validating order (should not retry validation errors)...');

  try {
    const result = await RetryExecutor.execute(
      () => apiService.validateOrder('invalid_order'),
      retryOptions
    );

    console.log('  ✅ Order validation successful!');
    console.log(`  📊 Result: ${JSON.stringify(result, null, 2)}`);
  } catch (error) {
    console.log(
      '  ❌ Order validation failed (expected):',
      (error as Error).message
    );
  }
}

// Example 4: Step Execution with Retry
async function demonstrateStepExecution() {
  console.log('\n⚙️  Example 4: Step Execution with Retry');
  console.log('='.repeat(50));

  const step = {
    id: 'notification-step',
    name: 'send-notification',
    handler: async (ctx: Context) => {
      return await apiService.sendNotification(
        'user_123',
        'Hello from retry logic!'
      );
    },
    type: 'step' as const,
    options: {
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '500ms' },
        jitter: true,
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

  console.log('  📧 Sending notification with step execution retry...');

  try {
    const result = await StepExecutor.executeStep(step, context, step.options);

    if (result.success) {
      console.log('  ✅ Notification sent successfully!');
      console.log(`  📊 Result: ${JSON.stringify(result, null, 2)}`);
    } else {
      console.log('  ❌ Notification failed:', result.error?.message);
    }
  } catch (error) {
    console.log('  ❌ Step execution failed:', (error as Error).message);
  }
}

// Example 5: Different Retry Strategies
async function demonstrateRetryStrategies() {
  console.log('\n🔄 Example 5: Different Retry Strategies');
  console.log('='.repeat(50));

  // Strategy 1: Exponential backoff with jitter
  console.log('  📊 Strategy 1: Exponential backoff with jitter');
  const exponentialOptions: RetryOptions = {
    maxAttempts: 3,
    backoff: { strategy: 'exponential', delay: '1s' },
    jitter: true,
  };

  try {
    const result1 = await RetryExecutor.execute(
      () => apiService.fetchUserData('user_456'),
      exponentialOptions
    );
    console.log('  ✅ Exponential backoff successful!');
  } catch (error) {
    console.log('  ❌ Exponential backoff failed:', (error as Error).message);
  }

  // Strategy 2: Fixed delay without jitter
  console.log('  📊 Strategy 2: Fixed delay without jitter');
  const fixedOptions: RetryOptions = {
    maxAttempts: 4,
    backoff: { strategy: 'fixed', delay: '2s' },
    jitter: false,
  };

  try {
    const result2 = await RetryExecutor.execute(
      () => apiService.processPayment(50.0),
      fixedOptions
    );
    console.log('  ✅ Fixed delay successful!');
  } catch (error) {
    console.log('  ❌ Fixed delay failed:', (error as Error).message);
  }
}

// Run all examples
async function runAllExamples() {
  try {
    await demonstrateBasicRetry();
    await demonstratePaymentRetry();
    await demonstrateErrorFiltering();
    await demonstrateStepExecution();
    await demonstrateRetryStrategies();

    console.log('\n🎉 All retry logic examples completed successfully!');
    console.log('\n📚 Key Takeaways:');
    console.log('  • Exponential backoff prevents overwhelming services');
    console.log('  • Jitter prevents thundering herd problems');
    console.log(
      '  • Error filtering ensures only appropriate errors are retried'
    );
    console.log('  • Custom error handlers provide graceful fallbacks');
    console.log('  • Performance monitoring helps optimize retry strategies');
  } catch (error) {
    console.error('❌ Example execution failed:', error);
  }
}

// Run the examples if this file is executed directly
if (typeof require !== 'undefined' && require.main === module) {
  runAllExamples();
}

export {
  runAllExamples,
  demonstrateBasicRetry,
  demonstratePaymentRetry,
  demonstrateErrorFiltering,
  demonstrateStepExecution,
  demonstrateRetryStrategies,
};
