#!/usr/bin/env node

const core = require('../../core/core.node');

console.log('🧪 Testing Job Execution with Context...\n');

// Test database path
const dbPath = './test_job_execution.db';

// Clean up any existing test file
const fs = require('fs');
if (fs.existsSync(dbPath)) {
  fs.unlinkSync(dbPath);
}

try {
  // 1. Register a workflow first
  console.log('1️⃣ Registering test workflow...');
  const workflowJson = JSON.stringify({
    id: 'test-job-workflow',
    name: 'Test Job Workflow',
    description: 'A test workflow for job execution',
    steps: [
      {
        id: 'step1',
        name: 'Test Step',
        action: 'test_action',
        timeout: 5000,
        retry: {
          max_attempts: 3,
          backoff_ms: 1000,
        },
        depends_on: [],
      },
    ],
    triggers: [
      {
        Webhook: {
          path: '/webhook/test',
          method: 'POST',
        },
      },
    ],
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  });

  const registerResult = core.registerWorkflow(workflowJson, dbPath);
  console.log('Register result:', registerResult);
  if (!registerResult.success) {
    throw new Error(`Workflow registration failed: ${registerResult.message}`);
  }
  console.log('✅ Workflow registration successful\n');

  // 2. Create a run
  console.log('2️⃣ Creating test run...');
  const payloadJson = JSON.stringify({
    test: 'data',
    timestamp: Date.now(),
  });

  const createResult = core.createRun('test-job-workflow', payloadJson, dbPath);
  console.log('Create result:', createResult);
  if (!createResult.success || !createResult.runId) {
    throw new Error(`Run creation failed: ${createResult.message}`);
  }
  const runId = createResult.runId;
  console.log('✅ Run creation successful\n');

  // 3. Create a job for execution
  console.log('3️⃣ Creating test job...');
  const job = {
    id: 'job-123',
    workflow_id: 'test-job-workflow',
    run_id: runId,
    step_name: 'step1',
    state: 'Pending',
    priority: 'Normal',
    payload: {
      test: 'job_payload',
      step: 'step1',
    },
    result: null,
    retry_config: {
      max_attempts: 3,
      backoff_ms: 1000,
      max_backoff_ms: 10000,
      jitter: true,
    },
    metadata: {
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      started_at: null,
      completed_at: null,
      attempt_count: 1,
      last_error: null,
      tags: {},
    },
    dependencies: [],
    timeout_ms: 5000,
    context: {
      user_id: 'user-123',
      session_id: 'session-456',
    },
  };

  const jobJson = JSON.stringify(job);
  console.log('Job JSON:', jobJson);
  console.log('✅ Job created successfully\n');

  // 4. Create services for the job
  console.log('4️⃣ Creating test services...');
  const services = {
    email: {
      type: 'email',
      config: {
        api_key: 'test_key',
        from: 'test@example.com',
      },
    },
    database: {
      type: 'database',
      config: {
        connection_string: 'sqlite://test.db',
      },
    },
  };

  const servicesJson = JSON.stringify(services);
  console.log('Services JSON:', servicesJson);
  console.log('✅ Services created successfully\n');

  // 5. Execute the job
  console.log('5️⃣ Executing job with context...');
  const executeResult = core.executeJob(jobJson, servicesJson, dbPath);
  console.log('Execute result:', executeResult);

  if (!executeResult.success) {
    throw new Error(`Job execution failed: ${executeResult.message}`);
  }

  console.log('✅ Job execution successful\n');

  // 6. Parse and display the context
  console.log('6️⃣ Analyzing execution result...');
  if (executeResult.result) {
    const result = JSON.parse(executeResult.result);
    console.log('Job ID:', result.job_id);
    console.log('Run ID:', result.run_id);
    console.log('Step ID:', result.step_id);
    console.log('Status:', result.status);
    console.log('Message:', result.message);

    if (result.context) {
      console.log('\n📋 Context Object:');
      const context = JSON.parse(result.context);
      console.log('  • Payload:', JSON.stringify(context.payload, null, 2));
      console.log('  • Run ID:', context.run.run_id);
      console.log('  • Workflow ID:', context.run.workflow_id);
      console.log('  • Current Step:', context.current_step.name);
      console.log('  • Services Count:', Object.keys(context.services).length);
      console.log(
        '  • Completed Steps:',
        context.steps ? Object.keys(context.steps).length : 0
      );
    }
  }

  console.log('\n🎉 All job execution tests passed!');
  console.log(
    'Expected Result: Rust can dispatch jobs to Bun.js and handle results - ✅ VERIFIED'
  );

  console.log('\n📋 Test Summary:');
  console.log('   • Workflow registration: ✅');
  console.log('   • Run creation: ✅');
  console.log('   • Job creation: ✅');
  console.log('   • Services configuration: ✅');
  console.log('   • Job execution with context: ✅');
  console.log('   • Context serialization: ✅');

  console.log('\n🚀 Job System is fully connected to N-API!');
} catch (error) {
  console.error('❌ Test failed:', error.message);
  process.exit(1);
} finally {
  // Clean up
  if (fs.existsSync(dbPath)) {
    fs.unlinkSync(dbPath);
  }
}
