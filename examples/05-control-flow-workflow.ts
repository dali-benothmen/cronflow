import { Cronflow } from '../sdk/src/cronflow';
import { Context } from '../sdk/src/workflow/types';

// Create cronflow instance
const cronflow = new Cronflow();

// Example: Order Processing Workflow with Control Flow
const orderProcessingWorkflow = cronflow.define({
  id: 'order-processing-with-control-flow',
  name: 'Order Processing with Control Flow',
  description: 'Demonstrates advanced control flow methods',
  tags: ['ecommerce', 'control-flow', 'example'],
});

// Define the workflow with control flow methods
orderProcessingWorkflow
  .step('validate-order', async (ctx: Context) => {
    const { orderId, amount, customerType } = ctx.payload;

    if (!orderId || !amount) {
      throw new Error('Invalid order data');
    }

    return {
      orderId,
      amount,
      customerType,
      isValid: true,
      timestamp: new Date().toISOString(),
    };
  })

  // Conditional flow based on order amount
  .if('is-high-value-order', (ctx: Context) => ctx.last.amount > 500)
  .step('send-vip-notification', async (ctx: Context) => {
    console.log('🤑 Processing VIP order:', ctx.last.orderId);
    return {
      type: 'vip',
      message: `VIP order ${ctx.last.orderId} processed`,
      priority: 'high',
    };
  })
  .parallel([
    async (ctx: Context) => {
      // Simulate sending VIP email
      await new Promise(resolve => setTimeout(resolve, 100));
      return { emailSent: true, type: 'vip-email' };
    },
    async (ctx: Context) => {
      // Simulate SMS notification
      await new Promise(resolve => setTimeout(resolve, 50));
      return { smsSent: true, type: 'vip-sms' };
    },
    async (ctx: Context) => {
      // Simulate Slack notification
      await new Promise(resolve => setTimeout(resolve, 75));
      return { slackSent: true, type: 'vip-slack' };
    },
  ])
  .elseIf('is-medium-value-order', (ctx: Context) => ctx.last.amount > 100)
  .step('send-standard-notification', async (ctx: Context) => {
    console.log('📦 Processing standard order:', ctx.last.orderId);
    return {
      type: 'standard',
      message: `Standard order ${ctx.last.orderId} processed`,
      priority: 'medium',
    };
  })
  .parallel([
    async (ctx: Context) => {
      // Simulate sending standard email
      await new Promise(resolve => setTimeout(resolve, 80));
      return { emailSent: true, type: 'standard-email' };
    },
    async (ctx: Context) => {
      // Simulate SMS notification
      await new Promise(resolve => setTimeout(resolve, 40));
      return { smsSent: true, type: 'standard-sms' };
    },
  ])
  .else()
  .step('send-basic-notification', async (ctx: Context) => {
    console.log('📝 Processing basic order:', ctx.last.orderId);
    return {
      type: 'basic',
      message: `Basic order ${ctx.last.orderId} processed`,
      priority: 'low',
    };
  })
  .step('send-basic-email', async (ctx: Context) => {
    await new Promise(resolve => setTimeout(resolve, 60));
    return { emailSent: true, type: 'basic-email' };
  })
  .endIf()

  // Race condition: Try multiple payment processors
  .race([
    async (ctx: Context) => {
      // Simulate primary payment processor
      await new Promise(resolve => setTimeout(resolve, 200));
      return {
        processor: 'stripe',
        success: true,
        transactionId: 'txn_stripe_123',
      };
    },
    async (ctx: Context) => {
      // Simulate backup payment processor
      await new Promise(resolve => setTimeout(resolve, 150));
      return {
        processor: 'paypal',
        success: true,
        transactionId: 'txn_paypal_456',
      };
    },
    async (ctx: Context) => {
      // Simulate alternative payment processor
      await new Promise(resolve => setTimeout(resolve, 300));
      return {
        processor: 'square',
        success: true,
        transactionId: 'txn_square_789',
      };
    },
  ])

  .step('process-payment-result', async (ctx: Context) => {
    const paymentResult = ctx.last;
    console.log('💳 Payment processed:', paymentResult.processor);

    return {
      orderId: ctx.steps['validate-order'].output.orderId,
      paymentProcessor: paymentResult.processor,
      transactionId: paymentResult.transactionId,
      status: 'paid',
    };
  })

  // While loop: Process inventory updates
  .while(
    'update-inventory',
    (ctx: Context) => ctx.state.get('inventory-updates-pending', 5) > 0,
    (ctx: Context) => {
      const pending = ctx.state.get('inventory-updates-pending', 5);
      console.log(`📦 Processing inventory update ${6 - pending}/5`);
      ctx.state.set('inventory-updates-pending', pending - 1);
    }
  )

  .step('finalize-order', async (ctx: Context) => {
    const orderData = ctx.steps['validate-order'].output;
    const paymentData = ctx.steps['process-payment-result'].output;

    return {
      orderId: orderData.orderId,
      amount: orderData.amount,
      paymentProcessor: paymentData.paymentProcessor,
      transactionId: paymentData.transactionId,
      status: 'completed',
      completedAt: new Date().toISOString(),
    };
  })

  .log('Order processing completed successfully!', 'info');

console.log('✅ Control Flow Workflow created successfully!');
console.log('📋 Workflow ID:', orderProcessingWorkflow.getId());
console.log('📋 Steps:', orderProcessingWorkflow.getSteps().length);
console.log('📋 Triggers:', orderProcessingWorkflow.getTriggers().length);

export { orderProcessingWorkflow };
