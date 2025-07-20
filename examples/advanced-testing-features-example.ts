import { cronflow } from '../sdk/src/index';

console.log('🚀 Advanced Testing Features Example\n');

// Example 1: E-commerce Order Processing with Advanced Testing
console.log('📦 Example 1: E-commerce Order Processing with Advanced Testing');

const ecommerceWorkflow = cronflow.define({
  id: 'ecommerce-order-processing',
  name: 'E-commerce Order Processing',
  description:
    'Process customer orders with payment, inventory, and notification',
  services: [
    {
      id: 'payment-service',
      name: 'Payment Service',
      version: '1.0.0',
      config: { apiKey: 'test_key' },
      auth: { type: 'api_key' },
      actions: {
        charge: async (params: any) => {
          // Simulate external payment service
          throw new Error('Payment service unavailable');
        },
        refund: async (params: any) => {
          throw new Error('Refund service unavailable');
        },
      },
    },
    {
      id: 'inventory-service',
      name: 'Inventory Service',
      version: '1.0.0',
      config: { database: 'inventory_db' },
      auth: { type: 'database' },
      actions: {
        checkStock: async (productId: string) => {
          throw new Error('Inventory service unavailable');
        },
        reserveStock: async (productId: string, quantity: number) => {
          throw new Error('Stock reservation failed');
        },
      },
    },
    {
      id: 'notification-service',
      name: 'Notification Service',
      version: '1.0.0',
      config: { provider: 'email' },
      auth: { type: 'oauth' },
      actions: {
        sendEmail: async (params: any) => {
          throw new Error('Email service unavailable');
        },
        sendSMS: async (params: any) => {
          throw new Error('SMS service unavailable');
        },
      },
    },
  ],
});

ecommerceWorkflow
  .step('validate-order', ctx => {
    const { orderId, items, customerId } = ctx.payload;

    if (!orderId || !items || !customerId) {
      throw new Error('Invalid order data');
    }

    if (items.length === 0) {
      throw new Error('Order must contain at least one item');
    }

    return { validated: true, orderId, items, customerId };
  })
  .step('check-inventory', async ctx => {
    const inventoryService = ctx.services['inventory-service'];
    const { items } = ctx.steps['validate-order'].output;

    const stockChecks = await Promise.all(
      items.map(async (item: any) => {
        const stock = await inventoryService.actions.checkStock(item.productId);
        return { productId: item.productId, available: stock.available };
      })
    );

    const unavailableItems = stockChecks.filter(
      (check: any) => !check.available
    );
    if (unavailableItems.length > 0) {
      throw new Error(
        `Items out of stock: ${unavailableItems.map((item: any) => item.productId).join(', ')}`
      );
    }

    return { stockChecks, allAvailable: true };
  })
  .step('process-payment', async ctx => {
    const paymentService = ctx.services['payment-service'];
    const { orderId, items } = ctx.steps['validate-order'].output;

    const totalAmount = items.reduce(
      (sum: number, item: any) => sum + item.price * item.quantity,
      0
    );

    const payment = await paymentService.actions.charge({
      orderId,
      amount: totalAmount,
      currency: 'USD',
      description: `Order ${orderId}`,
    });

    return {
      paymentId: payment.id,
      amount: totalAmount,
      status: payment.status,
    };
  })
  .step('reserve-inventory', async ctx => {
    const inventoryService = ctx.services['inventory-service'];
    const { items } = ctx.steps['validate-order'].output;

    const reservations = await Promise.all(
      items.map(async (item: any) => {
        const reservation = await inventoryService.actions.reserveStock(
          item.productId,
          item.quantity
        );
        return { productId: item.productId, reservationId: reservation.id };
      })
    );

    return { reservations };
  })
  .step('send-confirmation', async ctx => {
    const notificationService = ctx.services['notification-service'];
    const { customerId } = ctx.steps['validate-order'].output;
    const { paymentId } = ctx.steps['process-payment'].output;

    const email = await notificationService.actions.sendEmail({
      to: `customer-${customerId}@example.com`,
      subject: 'Order Confirmed',
      template: 'order-confirmation',
      data: { paymentId, orderId: ctx.steps['validate-order'].output.orderId },
    });

    return { emailId: email.id, sent: true };
  })
  .step('update-order-status', ctx => {
    const { orderId } = ctx.steps['validate-order'].output;
    const { paymentId } = ctx.steps['process-payment'].output;
    const { reservations } = ctx.steps['reserve-inventory'].output;

    return {
      orderId,
      status: 'confirmed',
      paymentId,
      reservations: reservations.map((r: any) => r.reservationId),
      confirmedAt: new Date().toISOString(),
    };
  });

// Advanced Testing Example
async function runAdvancedTestingExample() {
  console.log('\n🧪 Running Advanced Testing Example...\n');

  // Test 1: Successful order processing with mocked services
  console.log('✅ Test 1: Successful order processing');
  try {
    const testRun1 = await ecommerceWorkflow
      .advancedTest()
      .trigger({
        orderId: 'ord_123',
        customerId: 'cust_456',
        items: [
          { productId: 'prod_1', name: 'Laptop', price: 999, quantity: 1 },
          { productId: 'prod_2', name: 'Mouse', price: 29, quantity: 2 },
        ],
      })
      .mockService('payment-service', {
        charge: async (params: any) => ({
          id: 'pay_789',
          status: 'succeeded',
          amount: params.amount,
        }),
        refund: async (params: any) => ({
          id: 'ref_123',
          status: 'succeeded',
        }),
      })
      .mockService('inventory-service', {
        checkStock: async (productId: string) => ({
          available: true,
          quantity: 10,
        }),
        reserveStock: async (productId: string, quantity: number) => ({
          id: `res_${productId}_${Date.now()}`,
          status: 'reserved',
        }),
      })
      .mockService('notification-service', {
        sendEmail: async (params: any) => ({
          id: 'email_123',
          status: 'sent',
          to: params.to,
        }),
        sendSMS: async (params: any) => ({
          id: 'sms_123',
          status: 'sent',
        }),
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('reserve-inventory')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-order-status')
      .toSucceed()
      .run();

    console.log(`📊 Test 1 Status: ${testRun1.status}`);
    console.log(
      `📊 Coverage: ${(testRun1 as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );
    console.log(`📊 Duration: ${testRun1.duration}ms`);
  } catch (error) {
    console.log('❌ Test 1 failed:', error);
  }

  // Test 2: Order with insufficient inventory
  console.log('\n✅ Test 2: Order with insufficient inventory');
  try {
    const testRun2 = await ecommerceWorkflow
      .advancedTest()
      .trigger({
        orderId: 'ord_124',
        customerId: 'cust_456',
        items: [
          { productId: 'prod_3', name: 'Rare Item', price: 1999, quantity: 5 },
        ],
      })
      .mockService('inventory-service', {
        checkStock: async (productId: string) => ({
          available: false,
          quantity: 2,
        }),
        reserveStock: async (productId: string, quantity: number) => ({
          id: `res_${productId}_${Date.now()}`,
          status: 'reserved',
        }),
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toFailWith('Items out of stock')
      .run();

    console.log(`📊 Test 2 Status: ${testRun2.status}`);
  } catch (error) {
    console.log('❌ Test 2 failed:', error);
  }

  // Test 3: Payment failure scenario
  console.log('\n✅ Test 3: Payment failure scenario');
  try {
    const testRun3 = await ecommerceWorkflow
      .advancedTest()
      .trigger({
        orderId: 'ord_125',
        customerId: 'cust_456',
        items: [
          { productId: 'prod_1', name: 'Laptop', price: 999, quantity: 1 },
        ],
      })
      .mockService('inventory-service', {
        checkStock: async (productId: string) => ({
          available: true,
          quantity: 10,
        }),
        reserveStock: async (productId: string, quantity: number) => ({
          id: `res_${productId}_${Date.now()}`,
          status: 'reserved',
        }),
      })
      .mockService('payment-service', {
        charge: async (params: any) => {
          throw new Error('Insufficient funds');
        },
        refund: async (params: any) => ({
          id: 'ref_123',
          status: 'succeeded',
        }),
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toFailWith('Insufficient funds')
      .run();

    console.log(`📊 Test 3 Status: ${testRun3.status}`);
  } catch (error) {
    console.log('❌ Test 3 failed:', error);
  }

  // Test 4: Custom data generator with realistic test data
  console.log('\n✅ Test 4: Custom data generator with realistic test data');
  try {
    const customDataGenerator = {
      generateUser: () => ({
        id: 'test_user',
        name: 'Test User',
        email: 'test@example.com',
        role: 'customer',
      }),
      generateOrder: () => ({
        id: `order_${Date.now()}`,
        amount: Math.floor(Math.random() * 1000) + 100,
        items: [
          { productId: 'prod_1', name: 'Test Product', price: 99, quantity: 1 },
        ],
        customerId: 'test_customer',
      }),
      generatePayment: () => ({
        id: `payment_${Date.now()}`,
        amount: Math.floor(Math.random() * 1000) + 100,
        currency: 'USD',
        status: 'pending',
      }),
      generateProduct: () => ({
        id: `prod_${Date.now()}`,
        name: 'Test Product',
        price: Math.floor(Math.random() * 500) + 50,
        category: 'Electronics',
      }),
      generateEvent: () => ({
        id: `event_${Date.now()}`,
        type: 'order.created',
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

    const testRun4 = await ecommerceWorkflow
      .advancedTest()
      .withDataGenerator(customDataGenerator)
      .trigger((gen: any) => ({
        orderId: gen.generateOrder().id,
        customerId: gen.generateUser().id,
        items: gen
          .generateArray(() => gen.generateProduct(), 2)
          .map((product: any) => ({
            productId: product.id,
            name: product.name,
            price: product.price,
            quantity: Math.floor(Math.random() * 3) + 1,
          })),
      }))
      .mockService('payment-service', {
        charge: async (params: any) => ({
          id: `pay_${Date.now()}`,
          status: 'succeeded',
          amount: params.amount,
        }),
      })
      .mockService('inventory-service', {
        checkStock: async (productId: string) => ({
          available: true,
          quantity: 10,
        }),
        reserveStock: async (productId: string, quantity: number) => ({
          id: `res_${productId}_${Date.now()}`,
          status: 'reserved',
        }),
      })
      .mockService('notification-service', {
        sendEmail: async (params: any) => ({
          id: `email_${Date.now()}`,
          status: 'sent',
        }),
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('reserve-inventory')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-order-status')
      .toSucceed()
      .run();

    console.log(`📊 Test 4 Status: ${testRun4.status}`);
    console.log(
      `📊 Coverage: ${(testRun4 as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );
    console.log(`📊 Service Mocks: ${(testRun4 as any).serviceMocks.length}`);
  } catch (error) {
    console.log('❌ Test 4 failed:', error);
  }

  // Test 5: Performance and coverage analysis
  console.log('\n✅ Test 5: Performance and coverage analysis');
  try {
    const testRun5 = await ecommerceWorkflow
      .advancedTest()
      .trigger({
        orderId: 'ord_perf_test',
        customerId: 'cust_perf',
        items: [
          {
            productId: 'prod_perf_1',
            name: 'Performance Test Item',
            price: 100,
            quantity: 1,
          },
        ],
      })
      .mockService('payment-service', {
        charge: async (params: any) => {
          // Simulate network delay
          await new Promise(resolve => setTimeout(resolve, 50));
          return { id: 'pay_perf', status: 'succeeded', amount: params.amount };
        },
      })
      .mockService('inventory-service', {
        checkStock: async (productId: string) => ({
          available: true,
          quantity: 10,
        }),
        reserveStock: async (productId: string, quantity: number) => ({
          id: `res_perf_${productId}`,
          status: 'reserved',
        }),
      })
      .mockService('notification-service', {
        sendEmail: async (params: any) => ({
          id: 'email_perf',
          status: 'sent',
        }),
      })
      .expectStep('validate-order')
      .toSucceed()
      .expectStep('check-inventory')
      .toSucceed()
      .expectStep('process-payment')
      .toSucceed()
      .expectStep('reserve-inventory')
      .toSucceed()
      .expectStep('send-confirmation')
      .toSucceed()
      .expectStep('update-order-status')
      .toSucceed()
      .run();

    console.log(`📊 Test 5 Status: ${testRun5.status}`);
    console.log(`📊 Total Duration: ${testRun5.duration}ms`);
    console.log(
      `📊 Coverage: ${(testRun5 as any).coverage.overall.coveragePercentage.toFixed(1)}%`
    );

    console.log('📊 Step Performance:');
    for (const [stepName, step] of Object.entries(testRun5.steps)) {
      const stepData = step as any;
      console.log(`   ${stepName}: ${stepData.duration}ms`);
    }
  } catch (error) {
    console.log('❌ Test 5 failed:', error);
  }

  console.log('\n🎉 Advanced Testing Features Example Completed!');
  console.log('\n📋 Key Features Demonstrated:');
  console.log('✅ Enhanced service mocking with custom actions');
  console.log('✅ Comprehensive test scenarios (success, failure, edge cases)');
  console.log('✅ Custom data generators with realistic test data');
  console.log('✅ Performance monitoring and analysis');
  console.log('✅ Test coverage reporting');
  console.log('✅ Fluent API for complex test composition');
  console.log('✅ Real-world e-commerce workflow testing');
  console.log('✅ Error handling and validation testing');
  console.log('✅ Service integration testing');
  console.log('✅ Step-by-step performance analysis');
}

// Run the example
runAdvancedTestingExample().catch(console.error);
