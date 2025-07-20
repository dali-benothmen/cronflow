#!/usr/bin/env bun

/**
 * Circuit Breaker Example
 *
 * This example demonstrates the circuit breaker pattern implementation
 * with real-world scenarios including service failures, recovery, and monitoring.
 */

import {
  CircuitBreaker,
  CircuitBreakerManager,
} from '../sdk/src/circuit-breaker';
import type { Context } from '../sdk/src/workflow/types';

// Simulate external services with different failure patterns
class ExternalService {
  private failureCount = 0;
  private serviceName: string;

  constructor(serviceName: string) {
    this.serviceName = serviceName;
  }

  async callService(): Promise<any> {
    this.failureCount++;

    // Simulate different failure patterns
    if (this.serviceName === 'payment-service') {
      if (this.failureCount <= 3) {
        throw new Error('Payment service temporarily unavailable');
      }
    } else if (this.serviceName === 'email-service') {
      if (this.failureCount % 2 === 0) {
        throw new Error('Email service connection timeout');
      }
    } else if (this.serviceName === 'database-service') {
      if (this.failureCount <= 5) {
        throw new Error('Database connection failed');
      }
    } else if (this.serviceName === 'validation-service') {
      throw new Error('Validation error - invalid input');
    }

    return {
      service: this.serviceName,
      status: 'success',
      timestamp: new Date().toISOString(),
      requestId: `req_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
    };
  }

  reset(): void {
    this.failureCount = 0;
  }
}

console.log('🔄 Circuit Breaker Example - Real-world Scenarios\n');

// Example 1: Basic Circuit Breaker Usage
async function demonstrateBasicCircuitBreaker() {
  console.log('📦 Example 1: Basic Circuit Breaker Usage');
  console.log('='.repeat(50));

  const paymentService = new ExternalService('payment-service');

  const circuitBreaker = new CircuitBreaker('payment-service', {
    failureThreshold: 3,
    recoveryTimeout: '2s',
    onStateChange: (state, previousState) => {
      console.log(`  🔄 Payment service circuit: ${previousState} → ${state}`);
    },
    onFailure: (error, count) => {
      console.log(`  ❌ Payment service failure (${count}): ${error.message}`);
    },
    onSuccess: count => {
      console.log(`  ✅ Payment service success (${count})`);
    },
  });

  console.log('  💳 Testing payment service with circuit breaker...\n');

  for (let i = 1; i <= 5; i++) {
    try {
      const result = await circuitBreaker.execute(() =>
        paymentService.callService()
      );
      console.log(`  ✅ Request ${i}: ${result.status}`);
    } catch (error) {
      console.log(`  ❌ Request ${i}: ${(error as Error).message}`);
    }

    await new Promise(resolve => setTimeout(resolve, 500));
  }

  console.log(`  📊 Final circuit state: ${circuitBreaker.getState()}`);
  console.log(
    `  📊 Statistics: ${JSON.stringify(circuitBreaker.getStats(), null, 2)}`
  );
}

// Example 2: Circuit Breaker Manager
async function demonstrateCircuitBreakerManager() {
  console.log('\n🔧 Example 2: Circuit Breaker Manager');
  console.log('='.repeat(50));

  const manager = new CircuitBreakerManager({
    defaultFailureThreshold: 2,
    defaultRecoveryTimeout: '1s',
    enableLogging: true,
  });

  const emailService = new ExternalService('email-service');
  const databaseService = new ExternalService('database-service');

  console.log('  📧 Testing email service...');
  for (let i = 1; i <= 4; i++) {
    try {
      const result = await manager.executeWithCircuitBreaker(
        'email-service',
        () => emailService.callService()
      );
      console.log(`  ✅ Email request ${i}: ${result.status}`);
    } catch (error) {
      console.log(`  ❌ Email request ${i}: ${(error as Error).message}`);
    }
  }

  console.log('\n  🗄️  Testing database service...');
  for (let i = 1; i <= 6; i++) {
    try {
      const result = await manager.executeWithCircuitBreaker(
        'database-service',
        () => databaseService.callService()
      );
      console.log(`  ✅ Database request ${i}: ${result.status}`);
    } catch (error) {
      console.log(`  ❌ Database request ${i}: ${(error as Error).message}`);
    }
  }

  console.log('\n  📊 Global Statistics:');
  const globalStats = manager.getGlobalStats();
  console.log(
    `  - Total circuit breakers: ${globalStats.totalCircuitBreakers}`
  );
  console.log(`  - Open circuit breakers: ${globalStats.openCircuitBreakers}`);
  console.log(
    `  - Half-open circuit breakers: ${globalStats.halfOpenCircuitBreakers}`
  );
  console.log(
    `  - Closed circuit breakers: ${globalStats.closedCircuitBreakers}`
  );
  console.log(`  - Total requests: ${globalStats.totalRequests}`);
  console.log(`  - Total failures: ${globalStats.totalFailures}`);
  console.log(`  - Total successes: ${globalStats.totalSuccesses}`);

  console.log('\n  📋 Individual Circuit Breakers:');
  const circuitBreakers = manager.getAllCircuitBreakers();
  for (const cb of circuitBreakers) {
    console.log(
      `  - ${cb.name}: ${cb.state} (${cb.stats.totalRequests} requests)`
    );
  }
}

// Example 3: Error Filtering
async function demonstrateErrorFiltering() {
  console.log('\n🎯 Example 3: Error Filtering');
  console.log('='.repeat(50));

  const validationService = new ExternalService('validation-service');

  const circuitBreaker = new CircuitBreaker('validation-service', {
    failureThreshold: 2,
    recoveryTimeout: '1s',
    expectedErrors: ['network', 'timeout', 'connection'],
    onStateChange: (state, previousState) => {
      console.log(
        `  🔄 Validation service circuit: ${previousState} → ${state}`
      );
    },
  });

  console.log('  📋 Testing validation service with error filtering...\n');

  for (let i = 1; i <= 3; i++) {
    try {
      const result = await circuitBreaker.execute(() =>
        validationService.callService()
      );
      console.log(`  ✅ Validation request ${i}: ${result.status}`);
    } catch (error) {
      console.log(`  ❌ Validation request ${i}: ${(error as Error).message}`);
    }
  }

  console.log(`  📊 Final circuit state: ${circuitBreaker.getState()}`);
  console.log(
    '  💡 Note: Validation errors are not retryable, so circuit stays CLOSED'
  );
}

// Example 4: Recovery and Half-Open State
async function demonstrateRecovery() {
  console.log('\n🔄 Example 4: Recovery and Half-Open State');
  console.log('='.repeat(50));

  const paymentService = new ExternalService('payment-service');

  const circuitBreaker = new CircuitBreaker('recovery-test', {
    failureThreshold: 2,
    recoveryTimeout: '1s',
    onStateChange: (state, previousState) => {
      console.log(`  🔄 Recovery test circuit: ${previousState} → ${state}`);
    },
  });

  console.log('  💳 Testing circuit breaker recovery...\n');

  // Fail enough times to open the circuit
  for (let i = 1; i <= 2; i++) {
    try {
      await circuitBreaker.execute(() => paymentService.callService());
    } catch (error) {
      console.log(`  ❌ Request ${i}: ${(error as Error).message}`);
    }
  }

  console.log(
    `  📊 Circuit state after failures: ${circuitBreaker.getState()}`
  );

  // Wait for recovery timeout
  console.log('  ⏳ Waiting for recovery timeout...');
  await new Promise(resolve => setTimeout(resolve, 1100));

  console.log(`  📊 Circuit state after timeout: ${circuitBreaker.getState()}`);

  // Test half-open state
  try {
    const result = await circuitBreaker.execute(() =>
      paymentService.callService()
    );
    console.log(`  ✅ Recovery test successful: ${result.status}`);
    console.log(`  📊 Final circuit state: ${circuitBreaker.getState()}`);
  } catch (error) {
    console.log(`  ❌ Recovery test failed: ${(error as Error).message}`);
    console.log(`  📊 Final circuit state: ${circuitBreaker.getState()}`);
  }
}

// Example 5: Integration with Workflow Steps
async function demonstrateWorkflowIntegration() {
  console.log('\n⚙️  Example 5: Integration with Workflow Steps');
  console.log('='.repeat(50));

  const { StepExecutor } = await import('../sdk/src/execution/step-executor');

  const paymentService = new ExternalService('payment-service');
  const emailService = new ExternalService('email-service');

  const step = {
    id: 'payment-step',
    name: 'process-payment',
    handler: async (ctx: Context) => {
      return await paymentService.callService();
    },
    type: 'step' as const,
    options: {
      circuitBreaker: {
        name: 'payment-service',
        failureThreshold: 2,
        recoveryTimeout: '1s',
      },
    },
  };

  const context: Context = {
    payload: { amount: 99.99 },
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

  console.log('  💳 Testing payment step with circuit breaker...\n');

  for (let i = 1; i <= 4; i++) {
    try {
      const result = await StepExecutor.executeStep(
        step,
        context,
        step.options
      );
      if (result.success) {
        console.log(`  ✅ Payment step ${i}: ${result.output.status}`);
      } else {
        console.log(`  ❌ Payment step ${i}: ${result.error?.message}`);
      }
    } catch (error) {
      console.log(`  ❌ Payment step ${i}: ${(error as Error).message}`);
    }

    await new Promise(resolve => setTimeout(resolve, 500));
  }

  const circuitBreakerManager = StepExecutor.getCircuitBreakerManager();
  const circuitBreaker =
    circuitBreakerManager.getCircuitBreaker('payment-service');

  if (circuitBreaker) {
    console.log(`  📊 Circuit breaker state: ${circuitBreaker.getState()}`);
    console.log(
      `  📊 Circuit breaker stats: ${JSON.stringify(circuitBreaker.getStats(), null, 2)}`
    );
  }
}

// Run all examples
async function runAllExamples() {
  try {
    await demonstrateBasicCircuitBreaker();
    await demonstrateCircuitBreakerManager();
    await demonstrateErrorFiltering();
    await demonstrateRecovery();
    await demonstrateWorkflowIntegration();

    console.log('\n🎉 All circuit breaker examples completed successfully!');
    console.log('\n📚 Key Takeaways:');
    console.log('  • Circuit breakers prevent cascading failures');
    console.log('  • Three states: CLOSED, OPEN, HALF_OPEN');
    console.log('  • Automatic recovery after timeout');
    console.log('  • Error filtering for selective circuit opening');
    console.log('  • Comprehensive monitoring and statistics');
    console.log('  • Seamless integration with workflow steps');
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
  demonstrateBasicCircuitBreaker,
  demonstrateCircuitBreakerManager,
  demonstrateErrorFiltering,
  demonstrateRecovery,
  demonstrateWorkflowIntegration,
};
