import { Cronflow } from '../sdk/src/cronflow';
import { Context } from '../sdk/src/workflow/types';

// Create cronflow instance
const cronflow = new Cronflow();

// Example 2: Scheduled workflow
const scheduledWorkflow = cronflow.define({
  id: 'scheduled-daily-report',
  name: 'Daily Report Workflow',
  description: 'Generates daily reports on schedule',
});

scheduledWorkflow
  .onSchedule('0 9 * * *') // Run at 9 AM daily
  .step('generate-report', async (ctx: Context) => {
    console.log('📊 Generating daily report...');
    return { reportId: 'daily-2024-01-15', status: 'generated' };
  })
  .step('send-report', async (ctx: Context) => {
    console.log('📧 Sending report:', ctx.last.reportId);
    return { sent: true, recipient: 'admin@company.com' };
  });

console.log('✅ Scheduled Workflow created successfully!');
console.log('📋 Workflow ID:', scheduledWorkflow.getId());
console.log('📋 Steps:', scheduledWorkflow.getSteps().length);
console.log('📋 Triggers:', scheduledWorkflow.getTriggers().length);

export { scheduledWorkflow };
