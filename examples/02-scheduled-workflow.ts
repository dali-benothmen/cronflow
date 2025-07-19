import { cronflow } from '../sdk/src/index';
import { Context } from '../sdk/src/workflow/types';

// Example 2: Scheduled workflow
const scheduledWorkflow = cronflow.define({
  id: 'scheduled-workflow',
  name: 'Scheduled Workflow',
  description: 'A workflow that runs on a schedule',
});

scheduledWorkflow
  .onSchedule('0 */6 * * *') // Every 6 hours
  .step('fetch-data', async (ctx: Context) => {
    console.log('🕐 Running scheduled task at:', new Date().toISOString());
    return { data: 'scheduled data', timestamp: Date.now() };
  })
  .step('process-data', async (ctx: Context) => {
    console.log('📊 Processing data from previous step:', ctx.last);
    return { processed: true, result: ctx.last.data };
  })
  .action('log-completion', (ctx: Context) => {
    console.log('✅ Scheduled workflow completed successfully');
  });

console.log('✅ Scheduled Workflow created successfully!');
console.log('📋 Workflow ID:', scheduledWorkflow.getId());
console.log('📋 Schedule:', '0 */6 * * * (every 6 hours)');

export { scheduledWorkflow };
