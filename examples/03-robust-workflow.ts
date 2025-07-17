import { Cronflow } from '../sdk/src/cronflow';
import { Context } from '../sdk/src/workflow/types';

// Create cronflow instance
const cronflow = new Cronflow();

// Example 3: Workflow with retry and timeout
const robustWorkflow = cronflow.define({
  id: 'robust-api-workflow',
  name: 'Robust API Workflow',
  description: 'Demonstrates retry and timeout features',
});

robustWorkflow
  .manual()
  .step('call-external-api', async (ctx: Context) => {
    console.log('🌐 Calling external API...');
    // Simulate API call
    await new Promise(resolve => setTimeout(resolve, 1000));
    return { apiResponse: 'success', data: { id: 123, name: 'test' } };
  })
  .timeout('30s')
  .retry({
    attempts: 3,
    backoff: { strategy: 'exponential', delay: '2s' },
  })
  .step('process-response', async (ctx: Context) => {
    console.log('🔄 Processing API response:', ctx.last.apiResponse);
    return { processed: true, result: ctx.last.data };
  });

console.log('✅ Robust Workflow created successfully!');
console.log('📋 Workflow ID:', robustWorkflow.getId());
console.log('📋 Steps:', robustWorkflow.getSteps().length);
console.log('📋 Triggers:', robustWorkflow.getTriggers().length);

export { robustWorkflow };
