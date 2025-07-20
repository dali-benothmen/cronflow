import { cronflow } from '../sdk/src/index';
import { TestDataGenerator } from '../sdk/src/testing/advanced';

console.log('🧪 Testing Advanced Testing Features...\n');

async function runAdvancedTests() {
  // Test 1: Enhanced service mocking
  console.log('✅ Test 1: Enhanced service mocking');
  try {
    const serviceTestWorkflow = cronflow.define({
      id: 'service-mock-test',
      name: 'Service Mock Test Workflow',
      services: [
        {
          id: 'payment-service',
          name: 'Payment Service',
          version: '1.0.0',
          config: {},
          auth: {},
          actions: {
            charge: async (params: any) => {
              throw new Error('Payment service unavailable');
            },
            refund: async (params: any) => {
              throw new Error('Refund service unavailable');
            },
          },
        },
        {
          id: 'email-service',
          name: 'Email Service',
          version: '1.0.0',
          config: {},
          auth: {},
          actions: {
            send: async (params: any) => {
              throw new Error('Email service unavailable');
            },
          },
        },
      ],
    });

    serviceTestWorkflow
      .step('process-payment', async ctx => {
        const paymentService = ctx.services['payment-service'];
        const result = await paymentService.actions.charge({
          amount: ctx.payload.amount,
          currency: 'USD',
        });
        return result;
      })
      .step('send-confirmation', async ctx => {
        const emailService = ctx.services['email-service'];
        const result = await emailService.actions.send({
          to: 'customer@example.com',
          subject: 'Payment Confirmed',
        });
        return result;
      });

    const advancedTestRun = await serviceTestWorkflow
      .advancedTest()
      .trigger({ amount: 100 })
      .mockService('payment-service', {
        charge: async (params: any) => ({
          transactionId: 'txn_test_123',
          status: 'success',
          amount: params.amount,
        }),
        refund: async (params: any) => ({
          refundId: 'refund_test_123',
          status: 'success',
        }),
      })
      .mockServiceAction('email-service', 'send', {
        messageId: 'msg_test_123',
        status: 'sent',
      })
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .run();

    console.log('✅ Enhanced service mocking test completed');
    console.log(`📊 Test run status: ${advancedTestRun.status}`);
    console.log(
      `📊 Coverage: ${(advancedTestRun as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );
  } catch (error) {
    console.log('❌ Enhanced service mocking test failed:', error);
  }

  // Test 2: Trigger mocking
  console.log('\n✅ Test 2: Trigger mocking');
  try {
    const triggerTestWorkflow = cronflow.define({
      id: 'trigger-mock-test',
      name: 'Trigger Mock Test Workflow',
    });

    triggerTestWorkflow
      .step('process-webhook', ctx => {
        const headers = ctx.trigger.headers;
        const payload = ctx.payload;

        if (headers['x-webhook-signature']) {
          return { verified: true, processed: true, data: payload };
        } else {
          throw new Error('Invalid webhook signature');
        }
      })
      .step('process-schedule', ctx => {
        return { scheduled: true, timestamp: Date.now() };
      });

    const triggerTestRun = await triggerTestWorkflow
      .advancedTest()
      .trigger({ data: 'test' })
      .mockTrigger('webhook', {
        payload: { event: 'user.created', data: { userId: 'user_123' } },
        headers: { 'x-webhook-signature': 'valid-signature' },
        path: '/webhook/user-events',
      })
      .mockTrigger('schedule', {
        payload: { cron: '0 0 * * *', action: 'daily-cleanup' },
      })
      .expectStep('process-webhook')
      .toSucceed()
      .run();

    console.log('✅ Trigger mocking test completed');
    console.log(`📊 Test run status: ${triggerTestRun.status}`);
  } catch (error) {
    console.log('❌ Trigger mocking test failed:', error);
  }

  // Test 3: Test data generators
  console.log('\n✅ Test 3: Test data generators');
  try {
    const dataTestWorkflow = cronflow.define({
      id: 'data-generator-test',
      name: 'Data Generator Test Workflow',
    });

    dataTestWorkflow
      .step('process-user', ctx => {
        const user = ctx.payload.user;
        return { processed: true, userId: user.id, role: user.role };
      })
      .step('process-order', ctx => {
        const order = ctx.payload.order;
        return { processed: true, orderId: order.id, amount: order.amount };
      })
      .step('process-payment', ctx => {
        const payment = ctx.payload.payment;
        return {
          processed: true,
          paymentId: payment.id,
          status: payment.status,
        };
      });

    const dataTestRun = await dataTestWorkflow
      .advancedTest()
      .trigger((gen: TestDataGenerator) => ({
        user: gen.generateUser(),
        order: gen.generateOrder(),
        payment: gen.generatePayment(),
      }))
      .expectStep('process-user')
      .toSucceed()
      .expectStep('process-order')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .run();

    console.log('✅ Test data generators test completed');
    console.log(`📊 Test run status: ${dataTestRun.status}`);
  } catch (error) {
    console.log('❌ Test data generators test failed:', error);
  }

  // Test 4: Custom data generator
  console.log('\n✅ Test 4: Custom data generator');
  try {
    const customDataGenerator = {
      generateUser: () => ({
        id: 'custom_user',
        name: 'Custom User',
        email: 'custom@example.com',
        role: 'admin',
      }),
      generateOrder: () => ({
        id: 'custom_order',
        amount: 500,
        items: [],
        customerId: 'custom_customer',
      }),
      generatePayment: () => ({
        id: 'custom_payment',
        amount: 500,
        currency: 'USD',
        status: 'completed',
      }),
      generateProduct: () => ({
        id: 'custom_product',
        name: 'Custom Product',
        price: 100,
        category: 'Custom',
      }),
      generateEvent: () => ({
        id: 'custom_event',
        type: 'custom.event',
        data: {},
        timestamp: Date.now(),
      }),
      generateArray: (generator: () => any, count: number) =>
        Array.from({ length: count }, generator),
      generateObject: (generator: () => any, keys: string[]) => {
        const result: Record<string, any> = {};
        for (const key of keys) {
          result[key] = generator();
        }
        return result;
      },
    };

    const customDataTestWorkflow = cronflow.define({
      id: 'custom-data-test',
      name: 'Custom Data Test Workflow',
    });

    customDataTestWorkflow.step('process-custom-data', ctx => {
      return { processed: true, data: ctx.payload };
    });

    const customDataTestRun = await customDataTestWorkflow
      .advancedTest()
      .withDataGenerator(customDataGenerator)
      .trigger((gen: TestDataGenerator) => ({
        user: gen.generateUser(),
        products: gen.generateArray(() => gen.generateProduct(), 3),
      }))
      .expectStep('process-custom-data')
      .toSucceed()
      .run();

    console.log('✅ Custom data generator test completed');
    console.log(`📊 Test run status: ${customDataTestRun.status}`);
  } catch (error) {
    console.log('❌ Custom data generator test failed:', error);
  }

  // Test 5: Test coverage reporting
  console.log('\n✅ Test 5: Test coverage reporting');
  try {
    const coverageTestWorkflow = cronflow.define({
      id: 'coverage-test',
      name: 'Coverage Test Workflow',
      services: [
        {
          id: 'test-service',
          name: 'Test Service',
          version: '1.0.0',
          config: {},
          auth: {},
          actions: {
            action1: async () => ({ result: 'success' }),
            action2: async () => ({ result: 'success' }),
          },
        },
      ],
    });

    coverageTestWorkflow
      .step('step-1', ctx => ({ result: 'step1' }))
      .step('step-2', ctx => ({ result: 'step2' }))
      .step('step-3', ctx => ({ result: 'step3' }))
      .if('conditional', ctx => ctx.payload.shouldExecute)
      .step('conditional-step', ctx => ({ result: 'conditional' }))
      .endIf()
      .step('final-step', ctx => ({ result: 'final' }));

    const coverageTestRun = await coverageTestWorkflow
      .advancedTest()
      .trigger({ shouldExecute: true })
      .expectStep('step-1')
      .toSucceed()
      .expectStep('step-2')
      .toSucceed()
      .expectStep('step-3')
      .toSucceed()
      .expectStep('conditional-step')
      .toSucceed()
      .expectStep('final-step')
      .toSucceed()
      .run();

    console.log('✅ Test coverage reporting test completed');
    console.log(`📊 Test run status: ${coverageTestRun.status}`);
    console.log(
      `📊 Coverage: ${(coverageTestRun as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );
    console.log(
      `📊 Steps executed: ${(coverageTestRun as any).coverage.overall.executedSteps}/${(coverageTestRun as any).coverage.overall.totalSteps}`
    );
  } catch (error) {
    console.log('❌ Test coverage reporting test failed:', error);
  }

  // Test 6: Integration test utilities
  console.log('\n✅ Test 6: Integration test utilities');
  try {
    const integrationTestWorkflow = cronflow.define({
      id: 'integration-test',
      name: 'Integration Test Workflow',
      services: [
        {
          id: 'database-service',
          name: 'Database Service',
          version: '1.0.0',
          config: {},
          auth: { type: 'database' },
          actions: {
            save: async (data: any) => ({ saved: true, id: 'db_123' }),
            query: async (params: any) => ({ results: [] }),
          },
        },
        {
          id: 'cache-service',
          name: 'Cache Service',
          version: '1.0.0',
          config: {},
          auth: { type: 'cache' },
          actions: {
            set: async (key: string, value: any) => ({ cached: true }),
            get: async (key: string) => ({ value: null }),
          },
        },
      ],
    });

    integrationTestWorkflow
      .step('save-data', async ctx => {
        const dbService = ctx.services['database-service'];
        const result = await dbService.actions.save(ctx.payload);
        return result;
      })
      .step('cache-result', async ctx => {
        const cacheService = ctx.services['cache-service'];
        const result = await cacheService.actions.set(
          'key',
          ctx.steps['save-data'].output
        );
        return result;
      })
      .step('retrieve-data', async ctx => {
        const dbService = ctx.services['database-service'];
        const cacheService = ctx.services['cache-service'];

        const cached = await cacheService.actions.get('key');
        const dbData = await dbService.actions.query({ id: 'db_123' });

        return { cached, dbData };
      });

    const integrationTestRun = await integrationTestWorkflow
      .advancedTest()
      .trigger({ data: 'test-data' })
      .mockService('database-service', {
        save: async (data: any) => ({ saved: true, id: 'test_db_123' }),
        query: async (params: any) => ({
          results: [{ id: 'test_db_123', data: 'test-data' }],
        }),
      })
      .mockService('cache-service', {
        set: async (key: string, value: any) => ({ cached: true, key }),
        get: async (key: string) => ({
          value: { saved: true, id: 'test_db_123' },
        }),
      })
      .expectStep('save-data')
      .toSucceed()
      .expectStep('cache-result')
      .toSucceed()
      .expectStep('retrieve-data')
      .toSucceed()
      .run();

    console.log('✅ Integration test utilities test completed');
    console.log(`📊 Test run status: ${integrationTestRun.status}`);
    console.log(
      `📊 Service mocks: ${(integrationTestRun as any).serviceMocks.length}`
    );
  } catch (error) {
    console.log('❌ Integration test utilities test failed:', error);
  }

  // Test 7: Complex workflow with advanced features
  console.log('\n✅ Test 7: Complex workflow with advanced features');
  try {
    const complexAdvancedWorkflow = cronflow.define({
      id: 'complex-advanced-test',
      name: 'Complex Advanced Test Workflow',
      services: [
        {
          id: 'external-api',
          name: 'External API',
          version: '1.0.0',
          config: {},
          auth: {},
          actions: {
            fetch: async (url: string) => ({ data: 'external-data' }),
            post: async (url: string, data: any) => ({ success: true }),
          },
        },
      ],
    });

    complexAdvancedWorkflow
      .step('fetch-data', async ctx => {
        const apiService = ctx.services['external-api'];
        const result = await apiService.actions.fetch('/api/data');
        return result;
      })
      .step('process-data', ctx => {
        const data = ctx.steps['fetch-data'].output;
        return { processed: true, data };
      })
      .if('high-priority', ctx => ctx.payload.priority === 'high')
      .step('send-notification', async ctx => {
        const apiService = ctx.services['external-api'];
        const result = await apiService.actions.post('/api/notify', {
          message: 'High priority alert',
        });
        return result;
      })
      .endIf()
      .step('finalize', ctx => {
        return { finalized: true, timestamp: Date.now() };
      });

    const complexTestRun = await complexAdvancedWorkflow
      .advancedTest()
      .trigger((gen: TestDataGenerator) => ({
        user: gen.generateUser(),
        order: gen.generateOrder(),
        priority: 'high',
      }))
      .mockService('external-api', {
        fetch: async (url: string) => ({ data: 'mocked-external-data' }),
        post: async (url: string, data: any) => ({
          success: true,
          mocked: true,
        }),
      })
      .expectStep('fetch-data')
      .toSucceed()
      .expectStep('process-data')
      .toSucceed()
      .expectStep('send-notification')
      .toSucceed()
      .expectStep('finalize')
      .toSucceed()
      .run();

    console.log('✅ Complex workflow with advanced features test completed');
    console.log(`📊 Test run status: ${complexTestRun.status}`);
    console.log(
      `📊 Coverage: ${(complexTestRun as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );
  } catch (error) {
    console.log(
      '❌ Complex workflow with advanced features test failed:',
      error
    );
  }

  // Test 8: Performance and coverage analysis
  console.log('\n✅ Test 8: Performance and coverage analysis');
  try {
    const performanceTestWorkflow = cronflow.define({
      id: 'performance-test',
      name: 'Performance Test Workflow',
    });

    performanceTestWorkflow
      .step('fast-step', ctx => ({ result: 'fast' }))
      .step('slow-step', async ctx => {
        await new Promise(resolve => setTimeout(resolve, 50));
        return { result: 'slow' };
      })
      .step('final-step', ctx => ({ result: 'final' }));

    const performanceTestRun = await performanceTestWorkflow
      .advancedTest()
      .trigger({ test: true })
      .expectStep('fast-step')
      .toSucceed()
      .expectStep('slow-step')
      .toSucceed()
      .expectStep('final-step')
      .toSucceed()
      .run();

    console.log('✅ Performance and coverage analysis test completed');
    console.log(`📊 Test run status: ${performanceTestRun.status}`);
    console.log(`📊 Total duration: ${performanceTestRun.duration}ms`);
    console.log(
      `📊 Coverage: ${(performanceTestRun as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );

    console.log('📊 Step performance:');
    for (const [stepName, step] of Object.entries(performanceTestRun.steps)) {
      const stepData = step as any;
      console.log(`   ${stepName}: ${stepData.duration}ms`);
    }
  } catch (error) {
    console.log('❌ Performance and coverage analysis test failed:', error);
  }

  console.log('\n🎉 All advanced testing features tests completed!');
  console.log('\n📋 Summary of Advanced Testing Features:');
  console.log('✅ Enhanced service mocking with custom actions');
  console.log('✅ Trigger mocking with payload and configuration');
  console.log('✅ Test data generators with realistic data');
  console.log('✅ Custom data generator support');
  console.log('✅ Test coverage reporting and analysis');
  console.log('✅ Integration test utilities');
  console.log('✅ Complex workflow testing with advanced features');
  console.log('✅ Performance monitoring and analysis');
  console.log('✅ Comprehensive test result reporting');
  console.log('✅ Fluent API for advanced test composition');
  console.log('✅ Real-world testing patterns and best practices');
}

// Run the tests
runAdvancedTests().catch(console.error);
