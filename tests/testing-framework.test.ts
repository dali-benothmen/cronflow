import { cronflow } from '../sdk/src/index';

console.log('🧪 Testing Testing Framework Features...\n');

// Test 1: Basic workflow testing
console.log('✅ Test 1: Basic workflow testing');
try {
  const basicTestWorkflow = cronflow.define({
    id: 'basic-test-workflow',
    name: 'Basic Test Workflow',
  });

  basicTestWorkflow
    .step('validate-input', ctx => {
      if (!ctx.payload.orderId) {
        throw new Error('Order ID is required');
      }
      return { orderId: ctx.payload.orderId, validated: true };
    })
    .step('process-order', ctx => {
      const orderId = ctx.steps['validate-input'].output.orderId;
      return { processed: true, orderId, status: 'completed' };
    })
    .step('send-confirmation', ctx => {
      const orderId = ctx.steps['process-order'].output.orderId;
      return { sent: true, orderId, message: 'Order confirmed' };
    });

  // Test successful workflow execution
  const testRun = await basicTestWorkflow
    .test()
    .trigger({ orderId: 'ord_123', amount: 99.99 })
    .expectStep('validate-input')
    .toSucceed()
    .expectStep('process-order')
    .toSucceed()
    .expectStep('send-confirmation')
    .toSucceed()
    .run();

  console.log('✅ Basic workflow test completed successfully');
  console.log(`📊 Test run status: ${testRun.status}`);
  console.log(`📊 Test run duration: ${testRun.duration}ms`);
  console.log(`📊 Steps executed: ${Object.keys(testRun.steps).length}`);
} catch (error) {
  console.log('❌ Basic workflow test failed:', error);
}

// Test 2: Step mocking
console.log('\n✅ Test 2: Step mocking');
try {
  const mockTestWorkflow = cronflow.define({
    id: 'mock-test-workflow',
    name: 'Mock Test Workflow',
  });

  mockTestWorkflow
    .step('external-api-call', async ctx => {
      // This would normally call an external API
      throw new Error('External API unavailable');
    })
    .step('process-result', ctx => {
      const result = ctx.steps['external-api-call'].output;
      return { processed: true, data: result };
    });

  // Test with mocked step
  const mockTestRun = await mockTestWorkflow
    .test()
    .trigger({ userId: 'user_123' })
    .mockStep('external-api-call', async ctx => {
      return { status: 'success', data: { userId: 'user_123', balance: 1000 } };
    })
    .expectStep('external-api-call')
    .toSucceed()
    .expectStep('process-result')
    .toSucceed()
    .run();

  console.log('✅ Step mocking test completed successfully');
  console.log(`📊 Mock test run status: ${mockTestRun.status}`);
} catch (error) {
  console.log('❌ Step mocking test failed:', error);
}

// Test 3: Error handling and assertions
console.log('\n✅ Test 3: Error handling and assertions');
try {
  const errorTestWorkflow = cronflow.define({
    id: 'error-test-workflow',
    name: 'Error Test Workflow',
  });

  errorTestWorkflow
    .step('validate-payment', ctx => {
      if (ctx.payload.amount > 1000) {
        throw new Error('Payment amount exceeds limit');
      }
      return { approved: true, amount: ctx.payload.amount };
    })
    .step('process-payment', ctx => {
      const payment = ctx.steps['validate-payment'].output;
      return { processed: true, payment };
    });

  // Test expected failure
  const errorTestRun = await errorTestWorkflow
    .test()
    .trigger({ amount: 1500 })
    .expectStep('validate-payment')
    .toFailWith('Payment amount exceeds limit')
    .run();

  console.log('✅ Error handling test completed successfully');
  console.log(`📊 Error test run status: ${errorTestRun.status}`);
} catch (error) {
  console.log('❌ Error handling test failed:', error);
}

// Test 4: Complex workflow with conditional logic
console.log('\n✅ Test 4: Complex workflow with conditional logic');
try {
  const complexTestWorkflow = cronflow.define({
    id: 'complex-test-workflow',
    name: 'Complex Test Workflow',
  });

  complexTestWorkflow
    .step('analyze-risk', ctx => {
      const amount = ctx.payload.amount;
      const riskLevel = amount > 500 ? 'high' : 'low';
      return { riskLevel, amount };
    })
    .if('high-risk', ctx => ctx.last.riskLevel === 'high')
    .step('require-approval', ctx => {
      return { requiresApproval: true, reason: 'High amount' };
    })
    .else()
    .step('auto-approve', ctx => {
      return { requiresApproval: false, reason: 'Low risk' };
    })
    .endIf()
    .step('finalize', ctx => {
      const approval =
        ctx.steps['require-approval']?.output ||
        ctx.steps['auto-approve']?.output;
      return { finalized: true, approval };
    });

  // Test high-risk scenario
  const highRiskRun = await complexTestWorkflow
    .test()
    .trigger({ amount: 1000 })
    .expectStep('analyze-risk')
    .toSucceed()
    .expectStep('require-approval')
    .toSucceed()
    .expectStep('finalize')
    .toSucceed()
    .run();

  console.log('✅ Complex workflow test (high risk) completed successfully');

  // Test low-risk scenario
  const lowRiskRun = await complexTestWorkflow
    .test()
    .trigger({ amount: 100 })
    .expectStep('analyze-risk')
    .toSucceed()
    .expectStep('auto-approve')
    .toSucceed()
    .expectStep('finalize')
    .toSucceed()
    .run();

  console.log('✅ Complex workflow test (low risk) completed successfully');
} catch (error) {
  console.log('❌ Complex workflow test failed:', error);
}

// Test 5: State management in tests
console.log('\n✅ Test 5: State management in tests');
try {
  const stateTestWorkflow = cronflow.define({
    id: 'state-test-workflow',
    name: 'State Test Workflow',
  });

  stateTestWorkflow
    .step('initialize-state', async ctx => {
      await ctx.state.set('counter', 0);
      await ctx.state.set('last-updated', Date.now());
      return { initialized: true };
    })
    .step('increment-counter', async ctx => {
      const newCount = await ctx.state.incr('counter', 1);
      return { newCount };
    })
    .step('check-state', async ctx => {
      const counter = await ctx.state.get('counter', 0);
      const lastUpdated = await ctx.state.get('last-updated');
      return { counter, lastUpdated };
    });

  const stateTestRun = await stateTestWorkflow
    .test()
    .trigger({ test: true })
    .expectStep('initialize-state')
    .toSucceed()
    .expectStep('increment-counter')
    .toSucceed()
    .expectStep('check-state')
    .toSucceed()
    .run();

  console.log('✅ State management test completed successfully');
  console.log(`📊 State test run status: ${stateTestRun.status}`);
} catch (error) {
  console.log('❌ State management test failed:', error);
}

// Test 6: Service integration in tests
console.log('\n✅ Test 6: Service integration in tests');
try {
  const serviceTestWorkflow = cronflow.define({
    id: 'service-test-workflow',
    name: 'Service Test Workflow',
    services: [
      {
        id: 'mock-email',
        name: 'Mock Email Service',
        version: '1.0.0',
        config: {},
        auth: {},
        actions: {
          send: async (params: any) => {
            return { sent: true, to: params.to, subject: params.subject };
          },
        },
      },
    ],
  });

  serviceTestWorkflow.step('send-notification', async ctx => {
    const emailService = ctx.services['mock-email'];
    const result = await emailService.actions.send({
      to: 'test@example.com',
      subject: 'Test notification',
      html: '<p>Hello!</p>',
    });
    return result;
  });

  const serviceTestRun = await serviceTestWorkflow
    .test()
    .trigger({ userId: 'user_123' })
    .expectStep('send-notification')
    .toSucceed()
    .run();

  console.log('✅ Service integration test completed successfully');
  console.log(`📊 Service test run status: ${serviceTestRun.status}`);
} catch (error) {
  console.log('❌ Service integration test failed:', error);
}

// Test 7: Test utilities and assertions
console.log('\n✅ Test 7: Test utilities and assertions');
try {
  const utilityTestWorkflow = cronflow.define({
    id: 'utility-test-workflow',
    name: 'Utility Test Workflow',
  });

  utilityTestWorkflow
    .step('step-1', ctx => ({ result: 'success' }))
    .step('step-2', ctx => {
      if (ctx.payload.shouldFail) {
        throw new Error('Step 2 failed as expected');
      }
      return { result: 'success' };
    })
    .step('step-3', ctx => ({ result: 'success' }));

  // Test successful execution
  const successRun = await utilityTestWorkflow
    .test()
    .trigger({ shouldFail: false })
    .expectStep('step-1')
    .toSucceed()
    .expectStep('step-2')
    .toSucceed()
    .expectStep('step-3')
    .toSucceed()
    .run();

  console.log('✅ Success assertions test completed');

  // Test failure assertions
  const failureRun = await utilityTestWorkflow
    .test()
    .trigger({ shouldFail: true })
    .expectStep('step-1')
    .toSucceed()
    .expectStep('step-2')
    .toFailWith('Step 2 failed as expected')
    .run();

  console.log('✅ Failure assertions test completed');
} catch (error) {
  console.log('❌ Test utilities test failed:', error);
}

// Test 8: Performance and timing
console.log('\n✅ Test 8: Performance and timing');
try {
  const performanceTestWorkflow = cronflow.define({
    id: 'performance-test-workflow',
    name: 'Performance Test Workflow',
  });

  performanceTestWorkflow
    .step('fast-step', ctx => ({ result: 'fast' }))
    .step('slow-step', async ctx => {
      await new Promise(resolve => setTimeout(resolve, 100));
      return { result: 'slow' };
    })
    .step('final-step', ctx => ({ result: 'final' }));

  const performanceRun = await performanceTestWorkflow
    .test()
    .trigger({ test: true })
    .expectStep('fast-step')
    .toSucceed()
    .expectStep('slow-step')
    .toSucceed()
    .expectStep('final-step')
    .toSucceed()
    .run();

  console.log('✅ Performance test completed successfully');
  console.log(`📊 Total duration: ${performanceRun.duration}ms`);
  console.log(`📊 Step durations:`);
  for (const [stepName, step] of Object.entries(performanceRun.steps)) {
    console.log(`   ${stepName}: ${step.duration}ms`);
  }
} catch (error) {
  console.log('❌ Performance test failed:', error);
}

console.log('\n🎉 All testing framework tests completed!');
console.log('\n📋 Summary of Testing Framework Features:');
console.log('✅ Basic workflow testing with step-by-step execution');
console.log('✅ Step mocking for external dependencies');
console.log('✅ Error handling and failure assertions');
console.log('✅ Complex workflows with conditional logic');
console.log('✅ State management integration in tests');
console.log('✅ Service integration and mocking');
console.log('✅ Test utilities and assertion methods');
console.log('✅ Performance monitoring and timing');
console.log('✅ In-memory test execution');
console.log('✅ Comprehensive test result reporting');
console.log('✅ Fluent API for test composition');
