import { cronflow } from '../sdk/src/index';
import { Context } from '../sdk/src/workflow/types';

// Example 4: Complex workflow with multiple triggers
const complexWorkflow = cronflow.define({
  id: 'complex-multi-trigger-workflow',
  name: 'Complex Multi-Trigger Workflow',
  description: 'Workflow with multiple trigger types and advanced features',
});

complexWorkflow
  .onWebhook('/webhooks/complex')
  .onSchedule('0 */6 * * *') // Every 6 hours
  .onInterval('1h') // Every hour
  .manual()
  .step('validate-input', async (ctx: Context) => {
    console.log('🔍 Validating input...');
    const { data } = ctx.payload;
    if (!data) throw new Error('Missing data');
    return { validated: true, data };
  })
  .step('process-data', async (ctx: Context) => {
    console.log('⚙️ Processing data...');
    const processedData = { ...ctx.last.data, processed: true };
    return { processedData, timestamp: new Date().toISOString() };
  })
  .cache({
    key: (ctx: Context) => `cache-${JSON.stringify(ctx.payload)}`,
    ttl: '1h',
  })
  .step('send-notification', async (ctx: Context) => {
    console.log('📢 Sending notification...');
    return { notificationSent: true, message: 'Processing completed' };
  })
  .log('Complex workflow completed successfully!', 'info');

console.log('✅ Complex Workflow created successfully!');
console.log('📋 Workflow ID:', complexWorkflow.getId());
console.log('📋 Steps:', complexWorkflow.getSteps().length);
console.log('📋 Triggers:', complexWorkflow.getTriggers().length);

export { complexWorkflow };
