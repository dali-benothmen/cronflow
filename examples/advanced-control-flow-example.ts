import { cronflow } from '../sdk/src/index';

console.log('🚀 Advanced Control Flow Example\n');

// Define a comprehensive workflow that demonstrates all advanced control flow features
const advancedWorkflow = cronflow.define({
  id: 'advanced-control-flow-demo',
  name: 'Advanced Control Flow Demo',
  description:
    'Demonstrates all advanced control flow features in a real-world scenario',
});

// Build the workflow with advanced control flow features
advancedWorkflow
  // Step 1: Initial data gathering
  .step('fetch-orders', async ctx => {
    console.log('📦 Fetching orders...');
    // Simulate fetching orders from database
    return [
      { id: 1, customerId: 101, amount: 150, status: 'pending' },
      { id: 2, customerId: 102, amount: 2500, status: 'pending' }, // High value
      { id: 3, customerId: 103, amount: 75, status: 'pending' },
      { id: 4, customerId: 104, amount: 3000, status: 'pending' }, // High value
      { id: 5, customerId: 105, amount: 120, status: 'pending' },
    ];
  })

  // Step 2: Process orders with conditional logic
  .if('has-orders', ctx => ctx.last.length > 0)
  .step('validate-orders', ctx => {
    console.log('✅ Validating orders...');
    const orders = ctx.last;
    const validOrders = orders.filter(order => order.amount > 0);
    return { valid: validOrders, invalid: orders.length - validOrders.length };
  })
  .else()
  .step('no-orders', ctx => {
    console.log('⚠️ No orders found');
    return { message: 'No orders to process' };
  })
  .endIf()

  // Step 3: Process high-value orders with human approval
  .if('has-high-value-orders', ctx => {
    const orders = ctx.steps['validate-orders'].output.valid;
    return orders.some(order => order.amount > 1000);
  })
  .step('flag-high-value', ctx => {
    console.log('💰 Flagging high-value orders for approval...');
    const orders = ctx.steps['validate-orders'].output.valid;
    const highValueOrders = orders.filter(order => order.amount > 1000);
    return {
      highValueOrders,
      totalValue: highValueOrders.reduce((sum, order) => sum + order.amount, 0),
    };
  })
  .humanInTheLoop({
    timeout: '24h',
    description: 'Approve high-value orders processing',
    onPause: token => {
      console.log(`🛑 Human approval required for high-value orders`);
      console.log(`🔑 Approval token: ${token}`);
      console.log(`⏰ Timeout: 24 hours`);
    },
  })
  .step('process-high-value-approval', ctx => {
    console.log('✅ High-value orders approved');
    return {
      approved: ctx.last.approved,
      orders: ctx.steps['flag-high-value'].output.highValueOrders,
    };
  })
  .endIf()

  // Step 4: Process orders in parallel using forEach
  .forEach(
    'process-orders',
    ctx => {
      const orders = ctx.steps['validate-orders'].output.valid;
      // Filter out high-value orders that need special handling
      return orders.filter(order => order.amount <= 1000);
    },
    (order, flow) => {
      // This sub-workflow runs for each order in parallel
      flow
        .step('process-payment', async ctx => {
          console.log(
            `💳 Processing payment for order ${order.id} ($${order.amount})`
          );
          // Simulate payment processing
          await new Promise(resolve => setTimeout(resolve, 100));
          return {
            orderId: order.id,
            paymentStatus: 'success',
            amount: order.amount,
          };
        })
        .step('send-confirmation', async ctx => {
          console.log(`📧 Sending confirmation for order ${order.id}`);
          return { orderId: order.id, emailSent: true };
        })
        .step('update-inventory', async ctx => {
          console.log(`📦 Updating inventory for order ${order.id}`);
          return { orderId: order.id, inventoryUpdated: true };
        });
    }
  )

  // Step 5: Wait for external event (simulated)
  .step('wait-for-shipping', ctx => {
    console.log('🚚 Waiting for shipping confirmation...');
    return { status: 'waiting_for_shipping' };
  })
  .waitForEvent('shipping.confirmed', '1h')
  .step('process-shipping', ctx => {
    console.log('📦 Shipping confirmed, processing...');
    return { shippingStatus: 'confirmed', timestamp: Date.now() };
  })

  // Step 6: Process high-value orders in batches
  .if('has-approved-high-value', ctx => {
    const approvalStep = ctx.steps['process-high-value-approval'];
    return approvalStep && approvalStep.output.approved;
  })
  .step('prepare-high-value-batch', ctx => {
    console.log('📋 Preparing high-value orders for batch processing...');
    const orders = ctx.steps['process-high-value-approval'].output.orders;
    return orders;
  })
  .batch(
    'process-high-value-batches',
    {
      items: ctx => ctx.steps['prepare-high-value-batch'].output,
      size: 2, // Process 2 high-value orders at a time
    },
    (batch, flow) => {
      flow
        .step('enhanced-payment-processing', async ctx => {
          console.log(
            `💎 Enhanced payment processing for batch of ${batch.length} high-value orders`
          );
          const totalAmount = batch.reduce(
            (sum, order) => sum + order.amount,
            0
          );
          return { batchSize: batch.length, totalAmount, processed: true };
        })
        .step('fraud-check', async ctx => {
          console.log(`🔍 Fraud check for high-value batch`);
          return { fraudCheck: 'passed', batchSize: batch.length };
        })
        .step('send-vip-notification', async ctx => {
          console.log(`👑 Sending VIP notifications for high-value orders`);
          return { vipNotification: 'sent', batchSize: batch.length };
        });
    }
  )
  .endIf()

  // Step 7: Final cleanup and summary
  .sleep('5s') // Wait 5 seconds before final steps
  .step('generate-summary', ctx => {
    console.log('📊 Generating final summary...');

    const summary = {
      totalOrders: ctx.steps['validate-orders'].output.valid.length,
      highValueOrders:
        ctx.steps['flag-high-value']?.output?.highValueOrders?.length || 0,
      processedOrders: ctx.steps['process-orders']?.output?.totalItems || 0,
      shippingConfirmed:
        ctx.steps['process-shipping']?.output?.shippingStatus === 'confirmed',
      timestamp: Date.now(),
    };

    return summary;
  })
  .step('send-final-notification', ctx => {
    console.log('📢 Sending final notification...');
    return { notification: 'sent', summary: ctx.last };
  })
  .onError(ctx => {
    console.error('❌ Error in workflow:', ctx.error?.message);
    return { error: ctx.error?.message, recovered: true };
  })
  .step('workflow-complete', ctx => {
    console.log('🎉 Advanced control flow workflow completed successfully!');
    return {
      status: 'completed',
      finalSummary: ctx.steps['generate-summary'].output,
    };
  });

// Start the engine and trigger the workflow
async function runAdvancedControlFlowDemo() {
  try {
    console.log('🚀 Starting Advanced Control Flow Demo...\n');

    // Start the engine
    await cronflow.start();

    // Trigger the workflow
    const result = await cronflow.trigger('advanced-control-flow-demo', {
      demo: true,
      timestamp: Date.now(),
    });

    console.log('\n✅ Workflow triggered successfully!');
    console.log(`📋 Run ID: ${result}`);

    // Wait a moment for processing
    await new Promise(resolve => setTimeout(resolve, 2000));

    console.log('\n📊 Demo Summary:');
    console.log('✅ Advanced control flow features demonstrated:');
    console.log('  • Conditional logic (if/else)');
    console.log('  • Human-in-the-loop approval');
    console.log('  • Parallel processing (forEach)');
    console.log('  • Batch processing');
    console.log('  • Event waiting');
    console.log('  • Sleep/delays');
    console.log('  • Error handling');
    console.log('  • Complex workflow orchestration');
  } catch (error) {
    console.error('❌ Demo failed:', error);
  }
}

// Run the demo
runAdvancedControlFlowDemo();
