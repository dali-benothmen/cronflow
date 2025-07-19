import { cronflow } from '../sdk/src/index';
import { Context } from '../sdk/src/workflow/types';

// Example 1: Simple workflow with webhook trigger
const simpleWorkflow = cronflow.define({
  id: 'simple-webhook-workflow',
  name: 'Simple Webhook Workflow',
  description: 'A basic workflow triggered by webhook',
});

simpleWorkflow
  .onWebhook('/webhooks/simple')
  .step('process-webhook', async (ctx: Context) => {
    console.log('📥 Received webhook payload:', ctx.payload);
    return { processed: true, timestamp: new Date().toISOString() };
  })
  .action('log-success', (ctx: Context) => {
    console.log('✅ Webhook processed successfully');
  });

console.log('✅ Simple Webhook Workflow created successfully!');
console.log('📋 Workflow ID:', simpleWorkflow.getId());
console.log('📋 Steps:', simpleWorkflow.getSteps().length);
console.log('📋 Triggers:', simpleWorkflow.getTriggers().length);

export { simpleWorkflow };
