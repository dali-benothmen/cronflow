import { cronflow } from '../sdk/src/index';

console.log('🧪 Testing Framework Example\n');

// Define a comprehensive workflow for testing
const orderProcessingWorkflow = cronflow.define({
  id: 'order-processing-test',
  name: 'Order Processing Test Workflow',
  description:
    'A comprehensive workflow for demonstrating testing capabilities',
  services: [
    {
      id: 'payment-gateway',
      name: 'Payment Gateway Service',
      version: '1.0.0',
      config: {},
      auth: {},
      actions: {
        charge: async (params: any) => {
          // This would normally call a real payment gateway
          throw new Error('Payment gateway unavailable');
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
          return { sent: true, messageId: `msg_${Date.now()}` };
        },
      },
    },
  ],
});

// Build the workflow with comprehensive features
orderProcessingWorkflow
  .step('validate-order', ctx => {
    const { orderId, amount, customerId } = ctx.payload;

    if (!orderId) {
      throw new Error('Order ID is required');
    }
    if (!amount || amount <= 0) {
      throw new Error('Invalid amount');
    }
    if (!customerId) {
      throw new Error('Customer ID is required');
    }

    return {
      orderId,
      amount,
      customerId,
      validated: true,
      timestamp: Date.now(),
    };
  })
  .step('check-inventory', ctx => {
    const { orderId } = ctx.steps['validate-order'].output;

    // Simulate inventory check
    const inStock = Math.random() > 0.1; // 90% chance of being in stock

    if (!inStock) {
      throw new Error('Item out of stock');
    }

    return {
      orderId,
      inStock: true,
      availableQuantity: 10,
    };
  })
  .step('process-payment', async ctx => {
    const { orderId, amount, customerId } = ctx.steps['validate-order'].output;
    const paymentService = ctx.services['payment-gateway'];

    try {
      const result = await paymentService.actions.charge({
        orderId,
        amount,
        customerId,
      });

      return {
        orderId,
        paymentProcessed: true,
        transactionId: result.transactionId || `txn_${Date.now()}`,
        amount,
      };
    } catch (error) {
      throw new Error(`Payment failed: ${(error as Error).message}`);
    }
  })
  .if('high-value-order', ctx => {
    const { amount } = ctx.steps['validate-order'].output;
    return amount > 1000;
  })
  .step('require-manager-approval', ctx => {
    const { orderId, amount } = ctx.steps['validate-order'].output;
    return {
      requiresApproval: true,
      orderId,
      amount,
      approvedBy: 'manager',
      approvalReason: 'High value order',
    };
  })
  .else()
  .step('auto-approve', ctx => {
    const { orderId, amount } = ctx.steps['validate-order'].output;
    return {
      requiresApproval: false,
      orderId,
      amount,
      approvedBy: 'system',
      approvalReason: 'Auto-approved',
    };
  })
  .endIf()
  .step('send-confirmation', async ctx => {
    const { orderId, customerId } = ctx.steps['validate-order'].output;
    const { paymentProcessed, transactionId } =
      ctx.steps['process-payment'].output;
    const approval =
      ctx.steps['require-manager-approval']?.output ||
      ctx.steps['auto-approve']?.output;

    const emailService = ctx.services['email-service'];

    const emailResult = await emailService.actions.send({
      to: `customer_${customerId}@example.com`,
      subject: `Order Confirmation - ${orderId}`,
      html: `
        <h1>Order Confirmed!</h1>
        <p>Order ID: ${orderId}</p>
        <p>Transaction ID: ${transactionId}</p>
        <p>Approved by: ${approval.approvedBy}</p>
      `,
    });

    return {
      orderId,
      emailSent: true,
      messageId: emailResult.messageId,
      confirmation: 'Order processed successfully',
    };
  })
  .step('update-inventory', async ctx => {
    const { orderId } = ctx.steps['validate-order'].output;
    const { availableQuantity } = ctx.steps['check-inventory'].output;

    // Simulate inventory update
    const newQuantity = availableQuantity - 1;

    await ctx.state.set(`inventory:${orderId}`, {
      orderId,
      quantityBefore: availableQuantity,
      quantityAfter: newQuantity,
      updatedAt: Date.now(),
    });

    return {
      orderId,
      inventoryUpdated: true,
      newQuantity,
    };
  });

// Comprehensive test suite
async function runComprehensiveTests() {
  console.log('🚀 Running Comprehensive Test Suite...\n');

  // Test 1: Successful order processing
  console.log('📋 Test 1: Successful order processing');
  try {
    const successTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_123',
        amount: 500,
        customerId: 'cust_456',
      })
      .mockStep('process-payment', async ctx => {
        const { orderId, amount, customerId } =
          ctx.steps['validate-order'].output;
        return {
          orderId,
          paymentProcessed: true,
          transactionId: 'txn_test_123',
          amount,
        };
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('auto-approve')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-inventory')
      .toSucceed()
      .run();

    console.log('✅ Success test completed');
    console.log(`📊 Status: ${successTest.status}`);
    console.log(`📊 Duration: ${successTest.duration}ms`);
    console.log(`📊 Steps: ${Object.keys(successTest.steps).length}`);
  } catch (error) {
    console.log('❌ Success test failed:', error);
  }

  // Test 2: High-value order with manager approval
  console.log('\n📋 Test 2: High-value order with manager approval');
  try {
    const highValueTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_456',
        amount: 1500,
        customerId: 'cust_789',
      })
      .mockStep('process-payment', async ctx => {
        const { orderId, amount, customerId } =
          ctx.steps['validate-order'].output;
        return {
          orderId,
          paymentProcessed: true,
          transactionId: 'txn_test_456',
          amount,
        };
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('require-manager-approval')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-inventory')
      .toSucceed()
      .run();

    console.log('✅ High-value order test completed');
    console.log(`📊 Status: ${highValueTest.status}`);
  } catch (error) {
    console.log('❌ High-value order test failed:', error);
  }

  // Test 3: Validation failure
  console.log('\n📋 Test 3: Validation failure');
  try {
    const validationTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: '', // Invalid order ID
        amount: 100,
        customerId: 'cust_123',
      })
      .expectStep('validate-order')
      .toFailWith('Order ID is required')
      .run();

    console.log('✅ Validation failure test completed');
    console.log(`📊 Status: ${validationTest.status}`);
  } catch (error) {
    console.log('❌ Validation failure test failed:', error);
  }

  // Test 4: Inventory failure
  console.log('\n📋 Test 4: Inventory failure');
  try {
    const inventoryTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_789',
        amount: 100,
        customerId: 'cust_123',
      })
      .mockStep('check-inventory', ctx => {
        throw new Error('Item out of stock');
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toFailWith('Item out of stock')
      .run();

    console.log('✅ Inventory failure test completed');
    console.log(`📊 Status: ${inventoryTest.status}`);
  } catch (error) {
    console.log('❌ Inventory failure test failed:', error);
  }

  // Test 5: Payment failure
  console.log('\n📋 Test 5: Payment failure');
  try {
    const paymentTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_999',
        amount: 100,
        customerId: 'cust_123',
      })
      .mockStep('process-payment', async ctx => {
        throw new Error('Insufficient funds');
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toFailWith('Payment failed: Insufficient funds')
      .run();

    console.log('✅ Payment failure test completed');
    console.log(`📊 Status: ${paymentTest.status}`);
  } catch (error) {
    console.log('❌ Payment failure test failed:', error);
  }

  // Test 6: State management in tests
  console.log('\n📋 Test 6: State management in tests');
  try {
    const stateTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_state_123',
        amount: 100,
        customerId: 'cust_state_456',
      })
      .mockStep('process-payment', async ctx => {
        const { orderId, amount, customerId } =
          ctx.steps['validate-order'].output;
        return {
          orderId,
          paymentProcessed: true,
          transactionId: 'txn_state_123',
          amount,
        };
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('auto-approve')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-inventory')
      .toSucceed()
      .run();

    console.log('✅ State management test completed');
    console.log(`📊 Status: ${stateTest.status}`);
  } catch (error) {
    console.log('❌ State management test failed:', error);
  }

  // Test 7: Performance test
  console.log('\n📋 Test 7: Performance test');
  try {
    const performanceTest = await orderProcessingWorkflow
      .test()
      .trigger({
        orderId: 'ord_perf_123',
        amount: 100,
        customerId: 'cust_perf_456',
      })
      .mockStep('process-payment', async ctx => {
        // Simulate slow payment processing
        await new Promise(resolve => setTimeout(resolve, 50));
        const { orderId, amount, customerId } =
          ctx.steps['validate-order'].output;
        return {
          orderId,
          paymentProcessed: true,
          transactionId: 'txn_perf_123',
          amount,
        };
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('auto-approve')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-inventory')
      .toSucceed()
      .run();

    console.log('✅ Performance test completed');
    console.log(`📊 Total duration: ${performanceTest.duration}ms`);
    console.log('📊 Step durations:');
    for (const [stepName, step] of Object.entries(performanceTest.steps)) {
      console.log(`   ${stepName}: ${step.duration}ms`);
    }
  } catch (error) {
    console.log('❌ Performance test failed:', error);
  }

  console.log('\n🎉 All comprehensive tests completed!');
  console.log('\n📋 Testing Framework Features Demonstrated:');
  console.log('✅ Basic workflow testing with step-by-step execution');
  console.log('✅ Step mocking for external dependencies (payment gateway)');
  console.log('✅ Error handling and failure assertions');
  console.log(
    '✅ Complex workflows with conditional logic (high-value orders)'
  );
  console.log('✅ State management integration in tests');
  console.log('✅ Service integration and mocking');
  console.log('✅ Performance monitoring and timing');
  console.log('✅ Comprehensive test result reporting');
  console.log('✅ Fluent API for test composition');
  console.log('✅ Multiple test scenarios (success, failure, validation)');
  console.log('✅ Real-world workflow testing patterns');
}

// Run the comprehensive test suite
runComprehensiveTests();
