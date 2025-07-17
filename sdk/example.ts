import { cronflow } from './index';

// Example 1: Simple workflow with webhook trigger
const simpleWorkflow = cronflow.define({
  id: 'simple-workflow',
  name: 'Simple Email Workflow',
  description: 'A simple workflow that sends an email',
  tags: ['email', 'simple'],
  concurrency: 10,
  timeout: '5m',
  hooks: {
    onFailure: ctx => {
      console.error(`Workflow failed: ${ctx.run.id}`, ctx.error);
    },
  },
});

simpleWorkflow
  .onWebhook('/webhook/email', {
    method: 'POST',
    parseRawBody: true,
  })
  .step(
    'send-email',
    async ctx => {
      const { to, subject, body } = ctx.payload;

      // Simulate sending email
      console.log(`Sending email to ${to}: ${subject}`);

      return {
        messageId: 'msg_123',
        status: 'sent',
        timestamp: new Date().toISOString(),
      };
    },
    {
      timeout: '30s',
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '1s' },
      },
    }
  );

console.log('Simple Workflow JSON:');
console.log(simpleWorkflow.toJSON());

// Example 2: Complex workflow with multiple steps and triggers
const complexWorkflow = cronflow.define({
  id: 'complex-workflow',
  name: 'Data Processing Workflow',
  description: 'Process data, transform it, and send notifications',
  tags: ['data', 'processing', 'notifications'],
  concurrency: 5,
  timeout: '10m',
  hooks: {
    onSuccess: ctx => {
      console.log(`Workflow completed successfully: ${ctx.run.id}`);
    },
    onFailure: ctx => {
      console.error(`Workflow failed: ${ctx.run.id}`, ctx.error);
    },
  },
});

complexWorkflow
  .onWebhook('/webhook/data', {
    method: 'POST',
    parseRawBody: false,
  })
  .onSchedule('0 */5 * * *') // Every 5 minutes
  .step(
    'fetch-data',
    async ctx => {
      console.log('Fetching data...');

      // Simulate API call
      const data = await fetch('https://api.example.com/data');
      const result = await data.json();

      return {
        records: result.data,
        count: result.data.length,
        timestamp: new Date().toISOString(),
      };
    },
    {
      timeout: '2m',
      retry: {
        attempts: 3,
        backoff: { strategy: 'exponential', delay: '5s' },
      },
      cache: {
        key: ctx => `data-${new Date().toISOString().split('T')[0]}`,
        ttl: '1h',
      },
    }
  )
  .step(
    'transform-data',
    async ctx => {
      console.log('Transforming data...');

      const { records } = ctx.steps['fetch-data'].output;

      // Simulate data transformation
      const transformed = records.map((record: any) => ({
        ...record,
        processed: true,
        processedAt: new Date().toISOString(),
      }));

      return {
        transformedRecords: transformed,
        originalCount: records.length,
        processedCount: transformed.length,
      };
    },
    {
      timeout: '3m',
    }
  )
  .step(
    'send-notification',
    async ctx => {
      console.log('Sending notification...');

      const { processedCount } = ctx.steps['transform-data'].output;

      // Simulate notification
      return {
        notificationId: 'notif_456',
        recipients: ['admin@example.com'],
        message: `Processed ${processedCount} records successfully`,
        sentAt: new Date().toISOString(),
      };
    },
    {
      timeout: '1m',
      retry: {
        attempts: 2,
        backoff: { strategy: 'fixed', delay: '3s' },
      },
    }
  )
  .action('log-completion', ctx => {
    const { notificationId, message } = ctx.steps['send-notification'].output;
    console.log(
      `✅ Workflow completed: ${message} (Notification ID: ${notificationId})`
    );
  });

console.log('\nComplex Workflow JSON:');
console.log(complexWorkflow.toJSON());

// Example 3: Manual trigger workflow
const manualWorkflow = cronflow.define({
  id: 'manual-workflow',
  name: 'Manual Processing Workflow',
  description: 'Workflow that can be triggered manually',
  tags: ['manual', 'processing'],
  concurrency: 1, // Only one at a time
  timeout: '15m',
});

manualWorkflow
  .manual()
  .step(
    'process-task',
    async ctx => {
      console.log('Processing manual task...');

      const { taskId, priority } = ctx.payload;

      // Simulate task processing
      await new Promise(resolve => setTimeout(resolve, 2000));

      return {
        taskId,
        priority,
        status: 'completed',
        processedAt: new Date().toISOString(),
        result: `Task ${taskId} processed successfully`,
      };
    },
    {
      timeout: '10m',
      retry: {
        attempts: 5,
        backoff: { strategy: 'exponential', delay: '1s' },
      },
    }
  )
  .action('notify-completion', ctx => {
    const { taskId, result } = ctx.steps['process-task'].output;
    console.log(`🎉 Manual task completed: ${result}`);
  });

console.log('\nManual Workflow JSON:');
console.log(manualWorkflow.toJSON());

// Example 4: Workflow with conditional logic
const conditionalWorkflow = cronflow.define({
  id: 'conditional-workflow',
  name: 'Conditional Processing Workflow',
  description: 'Workflow demonstrating conditional logic',
  tags: ['conditional', 'logic'],
  timeout: '5m',
});

conditionalWorkflow
  .onWebhook('/webhook/order', {
    method: 'POST',
  })
  .step('validate-order', async ctx => {
    const { orderId, amount } = ctx.payload;

    if (!orderId || !amount) {
      throw new Error('Invalid order data');
    }

    return {
      orderId,
      amount,
      isValid: true,
      validatedAt: new Date().toISOString(),
    };
  })
  .if('is-high-value', ctx => ctx.last.amount > 500)
  .step('process-vip-order', async ctx => {
    console.log('Processing VIP order...');
    return {
      vip: true,
      specialHandling: true,
      processedAt: new Date().toISOString(),
    };
  })
  .endIf()
  .if('is-medium-value', ctx => ctx.last.amount > 100)
  .step('process-standard-order', async ctx => {
    console.log('Processing standard order...');
    return {
      standard: true,
      processedAt: new Date().toISOString(),
    };
  })
  .endIf()
  .step('send-confirmation', async ctx => {
    console.log('Sending confirmation...');
    return {
      confirmationId: 'conf_789',
      sentAt: new Date().toISOString(),
    };
  });

console.log('\nConditional Workflow JSON:');
console.log(conditionalWorkflow.toJSON());

// Example 5: Workflow with parallel execution
const parallelWorkflow = cronflow.define({
  id: 'parallel-workflow',
  name: 'Parallel Processing Workflow',
  description: 'Workflow demonstrating parallel execution',
  tags: ['parallel', 'processing'],
  timeout: '3m',
});

parallelWorkflow
  .onWebhook('/webhook/parallel', {
    method: 'POST',
  })
  .step('fetch-user-data', async ctx => {
    console.log('Fetching user data...');
    return { userId: ctx.payload.userId, userData: { name: 'John Doe' } };
  })
  .parallel([
    async ctx => {
      console.log('Processing in parallel 1...');
      return { result1: 'processed' };
    },
    async ctx => {
      console.log('Processing in parallel 2...');
      return { result2: 'processed' };
    },
    async ctx => {
      console.log('Processing in parallel 3...');
      return { result3: 'processed' };
    },
  ])
  .step('combine-results', ctx => {
    const [result1, result2, result3] = ctx.last;
    return {
      combined: { ...result1, ...result2, ...result3 },
      combinedAt: new Date().toISOString(),
    };
  });

console.log('\nParallel Workflow JSON:');
console.log(parallelWorkflow.toJSON());

// Example 6: Workflow with error handling
try {
  const errorWorkflow = cronflow.define({
    id: 'error-workflow',
    name: 'Error Handling Workflow',
    description: 'Workflow demonstrating error handling',
    tags: ['error', 'handling'],
  });

  errorWorkflow
    .onWebhook('/webhook/error-test')
    .step('risky-operation', async ctx => {
      // Simulate a risky operation that might fail
      if (Math.random() > 0.5) {
        throw new Error('Operation failed randomly');
      }
      return { success: true };
    })
    .onError(ctx => {
      console.error('Step failed, providing fallback:', ctx.error);
      return { fallback: true, error: ctx.error?.message || 'Unknown error' };
    });

  console.log('\nError Handling Workflow JSON:');
  console.log(errorWorkflow.toJSON());
} catch (error: unknown) {
  console.log('\nExpected validation error:');
  console.log(error instanceof Error ? error.message : String(error));
}

// Example 7: Workflow with logging and delays
const loggingWorkflow = cronflow.define({
  id: 'logging-workflow',
  name: 'Logging and Delay Workflow',
  description: 'Workflow demonstrating logging and delays',
  tags: ['logging', 'delays'],
});

loggingWorkflow
  .onWebhook('/webhook/logging')
  .log('Starting workflow execution')
  .step('process-data', async ctx => {
    console.log('Processing data...');
    return { processed: true };
  })
  .delay('2s') // Wait 2 seconds
  .log(ctx => `Data processed: ${ctx.last.processed}`)
  .sleep('1s') // Sleep for 1 second
  .action('final-log', ctx => {
    console.log('Workflow completed successfully!');
  });

console.log('\nLogging Workflow JSON:');
console.log(loggingWorkflow.toJSON());

// Example 8: Workflow statistics
console.log('\nWorkflow Statistics:');
console.log(`Total workflows: ${cronflow.getWorkflows().length}`);

const workflows = cronflow.getWorkflows();
workflows.forEach(workflow => {
  console.log(
    `- ${workflow.id}: ${workflow.steps.length} steps, ${workflow.triggers.length} triggers`
  );
});

// Example 9: Workflow validation
console.log('\nWorkflow Validation:');
workflows.forEach(workflow => {
  try {
    const instance = cronflow.getWorkflow(workflow.id);
    if (instance) {
      console.log(`✅ ${workflow.id}: Valid`);
    }
  } catch (error: unknown) {
    console.log(
      `❌ ${workflow.id}: Invalid - ${error instanceof Error ? error.message : String(error)}`
    );
  }
});

// Example 10: Demonstrate engine lifecycle
console.log('\nEngine Lifecycle Demo:');
console.log(`Engine state: ${cronflow.getState()}`);

// Wrap async operations in a function
async function demonstrateEngineLifecycle() {
  // Start the engine
  await cronflow.start();
  console.log(`Engine state after start: ${cronflow.getState()}`);

  // Trigger a workflow
  await cronflow.trigger('simple-workflow', {
    to: 'user@example.com',
    subject: 'Test Email',
    body: 'This is a test email from the workflow engine.',
  });

  // Stop the engine
  await cronflow.stop();
  console.log(`Engine state after stop: ${cronflow.getState()}`);
}

// Run the demonstration
demonstrateEngineLifecycle().catch(console.error);
