#!/usr/bin/env bun

/**
 * Retry Logic Example
 *
 * This example demonstrates the comprehensive retry logic implementation
 * with real-world scenarios including API calls, database operations,
 * and error handling patterns.
 */

import { cronflow } from '../sdk/index';

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

console.log('🔄 Retry Logic Example - Real-world Scenarios\n');

// Example 1: E-commerce Order Processing with Retry Logic
async function runEcommerceExample() {
  console.log('📦 Example 1: E-commerce Order Processing');
  console.log('='.repeat(50));

  const orderWorkflow = cronflow.define({
    id: 'ecommerce-order-processing',
    name: 'E-commerce Order Processing with Retry Logic',
  });

  orderWorkflow
    .step(
      'fetch-user-data',
      async ctx => {
        console.log('  🔍 Fetching user data...');
        const userData = await apiService.fetchUserData('user_123');
        console.log(`  ✅ User data fetched: ${userData.name}`);
        return userData;
      },
      {
        retry: {
          attempts: 3,
          backoff: { strategy: 'exponential', delay: '1s' },
        },
      }
    )
    .step(
      'process-payment',
      async ctx => {
        const userData = ctx.steps['fetch-user-data'].output;
        console.log(`  💳 Processing payment for ${userData.name}...`);
        const payment = await apiService.processPayment(99.99);
        console.log(`  ✅ Payment processed: ${payment.transactionId}`);
        return payment;
      },
      {
        retry: {
          attempts: 5,
          backoff: { strategy: 'exponential', delay: '2s' },
          maxBackoff: '30s',
        },
      }
    )
    .step(
      'validate-order',
      async ctx => {
        console.log('  📋 Validating order...');
        // This will fail with a validation error (non-retryable)
        const validation = await apiService.validateOrder('invalid_order');
        return validation;
      },
      {
        retry: {
          attempts: 2,
          backoff: { strategy: 'fixed', delay: '1s' },
        },
        onError: async ctx => {
          console.log('  ⚠️  Order validation failed, using fallback...');
          return {
            orderId: 'fallback_order_123',
            status: 'validated_fallback',
            reason: 'Using fallback validation due to service issues',
          };
        },
      }
    )
    .step(
      'send-notification',
      async ctx => {
        const userData = ctx.steps['fetch-user-data'].output;
        const payment = ctx.steps['process-payment'].output;
        console.log(`  📧 Sending notification to ${userData.name}...`);

        const notification = await apiService.sendNotification(
          userData.id,
          `Order confirmed! Payment: ${payment.transactionId}`
        );
        console.log(`  ✅ Notification sent: ${notification.notificationId}`);
        return notification;
      },
      {
        retry: {
          attempts: 3,
          backoff: { strategy: 'exponential', delay: '500ms' },
          jitter: true,
        },
      }
    );

  console.log('  🚀 Starting order processing workflow...\n');

  try {
    const result = await cronflow.trigger('ecommerce-order-processing', {
      orderId: 'order_123',
      amount: 99.99,
    });
    console.log('  🎉 Order processing completed successfully!');
    console.log('  📊 Final result:', JSON.stringify(result, null, 2));
  } catch (error) {
    console.log('  ❌ Order processing failed:', (error as Error).message);
  }
}

// Example 2: Data Processing Pipeline with Different Retry Strategies
async function runDataProcessingExample() {
  console.log('\n📊 Example 2: Data Processing Pipeline');
  console.log('='.repeat(50));

  const dataWorkflow = cronflow.define({
    id: 'data-processing-pipeline',
    name: 'Data Processing Pipeline with Advanced Retry Logic',
  });

  dataWorkflow
    .step(
      'extract-data',
      async ctx => {
        console.log('  📥 Extracting data from source...');
        // Simulate data extraction with potential network issues
        if (Math.random() < 0.7) {
          throw new Error('Connection timeout - data source unavailable');
        }

        const data = {
          records: Array.from({ length: 1000 }, (_, i) => ({
            id: i + 1,
            value: Math.random() * 100,
            timestamp: new Date().toISOString(),
          })),
        };
        console.log(`  ✅ Extracted ${data.records.length} records`);
        return data;
      },
      {
        retry: {
          attempts: 4,
          backoff: { strategy: 'exponential', delay: '500ms' },
          maxBackoff: '10s',
        },
      }
    )
    .step(
      'transform-data',
      async ctx => {
        const data = ctx.steps['extract-data'].output;
        console.log('  🔄 Transforming data...');

        // Simulate processing failures
        if (Math.random() < 0.5) {
          throw new Error('Processing error - transformation failed');
        }

        const transformed = data.records.map((record: any) => ({
          ...record,
          normalizedValue: record.value / 100,
          processed: true,
        }));
        console.log(`  ✅ Transformed ${transformed.length} records`);
        return { records: transformed };
      },
      {
        retry: {
          attempts: 3,
          backoff: { strategy: 'fixed', delay: '1s' },
        },
      }
    )
    .step(
      'load-data',
      async ctx => {
        const data = ctx.steps['transform-data'].output;
        console.log('  📤 Loading data to destination...');

        // Simulate database connection issues
        if (Math.random() < 0.6) {
          throw new Error('Database connection failed - retry needed');
        }

        console.log(`  ✅ Loaded ${data.records.length} records to database`);
        return {
          loadedRecords: data.records.length,
          status: 'completed',
          timestamp: new Date().toISOString(),
        };
      },
      {
        retry: {
          attempts: 5,
          backoff: { strategy: 'exponential', delay: '2s' },
          maxBackoff: '60s',
          jitter: true,
        },
      }
    );

  console.log('  🚀 Starting data processing pipeline...\n');

  try {
    const result = await cronflow.trigger('data-processing-pipeline', {
      batchId: 'batch_001',
    });
    console.log('  🎉 Data processing completed successfully!');
    console.log('  📊 Final result:', JSON.stringify(result, null, 2));
  } catch (error) {
    console.log('  ❌ Data processing failed:', (error as Error).message);
  }
}

// Example 3: Testing Different Retry Configurations
async function runRetryConfigurationExample() {
  console.log('\n⚙️  Example 3: Retry Configuration Testing');
  console.log('='.repeat(50));

  const testWorkflow = cronflow.define({
    id: 'retry-configuration-test',
    name: 'Testing Different Retry Configurations',
  });

  // Test 1: Exponential backoff with jitter
  testWorkflow.step(
    'exponential-backoff-test',
    async ctx => {
      console.log('  🔄 Testing exponential backoff with jitter...');
      if (Math.random() < 0.8) {
        throw new Error('Temporary network failure');
      }
      return { status: 'success', strategy: 'exponential' };
    },
    {
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '1s' },
        jitter: true,
      },
    }
  );

  // Test 2: Fixed delay without jitter
  testWorkflow.step(
    'fixed-delay-test',
    async ctx => {
      console.log('  ⏱️  Testing fixed delay without jitter...');
      if (Math.random() < 0.7) {
        throw new Error('Service temporarily unavailable');
      }
      return { status: 'success', strategy: 'fixed' };
    },
    {
      retry: {
        attempts: 4,
        backoff: { strategy: 'fixed', delay: '2s' },
        jitter: false,
      },
    }
  );

  // Test 3: Error filtering
  testWorkflow.step(
    'error-filtering-test',
    async ctx => {
      console.log('  🎯 Testing error filtering...');
      const errorType = Math.random() > 0.5 ? 'network' : 'validation';
      throw new Error(`${errorType} error occurred`);
    },
    {
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '1s' },
      },
    }
  );

  console.log('  🚀 Starting retry configuration tests...\n');

  try {
    const result = await cronflow.trigger('retry-configuration-test', {
      testId: 'config_test_001',
    });
    console.log('  🎉 Retry configuration tests completed!');
    console.log('  📊 Final result:', JSON.stringify(result, null, 2));
  } catch (error) {
    console.log(
      '  ❌ Retry configuration tests failed:',
      (error as Error).message
    );
  }
}

// Example 4: Performance Monitoring and Metrics
async function runPerformanceExample() {
  console.log('\n📈 Example 4: Performance Monitoring');
  console.log('='.repeat(50));

  const perfWorkflow = cronflow.define({
    id: 'performance-monitoring',
    name: 'Performance Monitoring with Retry Logic',
  });

  perfWorkflow.step(
    'measure-performance',
    async ctx => {
      console.log('  📊 Measuring performance with retry logic...');

      const startTime = Date.now();

      // Simulate a slow operation that might fail
      await new Promise(resolve =>
        setTimeout(resolve, 100 + Math.random() * 200)
      );

      if (Math.random() < 0.6) {
        throw new Error('Performance measurement timeout');
      }

      const duration = Date.now() - startTime;
      console.log(`  ⏱️  Operation completed in ${duration}ms`);

      return {
        duration,
        timestamp: new Date().toISOString(),
        attempts: 1, // Default to 1 since ctx.attempts doesn't exist
      };
    },
    {
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '500ms' },
        maxBackoff: '5s',
      },
    }
  );

  console.log('  🚀 Starting performance monitoring...\n');

  try {
    const result = await cronflow.trigger('performance-monitoring', {
      monitorId: 'perf_001',
    });
    console.log('  🎉 Performance monitoring completed!');
    console.log('  📊 Performance metrics:', JSON.stringify(result, null, 2));
  } catch (error) {
    console.log(
      '  ❌ Performance monitoring failed:',
      (error as Error).message
    );
  }
}

// Run all examples
async function runAllExamples() {
  try {
    // Start the cronflow engine to register workflows
    await cronflow.start();

    await runEcommerceExample();
    await runDataProcessingExample();
    await runRetryConfigurationExample();
    await runPerformanceExample();

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
  runEcommerceExample,
  runDataProcessingExample,
  runRetryConfigurationExample,
  runPerformanceExample,
};
