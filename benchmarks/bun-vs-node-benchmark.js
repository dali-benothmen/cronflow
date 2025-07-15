#!/usr/bin/env bun

/**
 * Bun vs Node.js + Node-API Performance Benchmark
 *
 * This benchmark compares the performance of our Rust core engine
 * when called from Bun vs Node.js using Node-API.
 */

const core = require('../core/core.node');

// Test data
const testWorkflow = {
  id: 'benchmark-workflow',
  name: 'Benchmark Workflow',
  description: 'A workflow for performance testing',
  steps: [
    {
      id: 'step1',
      name: 'First Step',
      action: 'test_action',
      timeout: 5000,
      retry: {
        max_attempts: 3,
        backoff_ms: 1000,
      },
      depends_on: [],
    },
    {
      id: 'step2',
      name: 'Second Step',
      action: 'test_action_2',
      timeout: 3000,
      retry: {
        max_attempts: 2,
        backoff_ms: 500,
      },
      depends_on: ['step1'],
    },
  ],
  triggers: [
    {
      Webhook: {
        path: '/webhook/benchmark',
        method: 'POST',
      },
    },
  ],
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const testPayload = {
  test: 'data',
  timestamp: Date.now(),
  benchmark: true,
  iterations: 1000,
};

// Benchmark functions
function benchmarkWorkflowRegistration(iterations = 100) {
  console.log(
    `\n📊 Benchmarking Workflow Registration (${iterations} iterations)`
  );

  const start = performance.now();

  for (let i = 0; i < iterations; i++) {
    const workflowId = `benchmark-workflow-${i}`;
    const workflowJson = JSON.stringify({
      ...testWorkflow,
      id: workflowId,
    });

    const result = core.registerWorkflow(workflowJson, './benchmark.db');

    if (!result.success) {
      console.error(
        `❌ Registration failed on iteration ${i}:`,
        result.message
      );
      return null;
    }
  }

  const end = performance.now();
  const duration = end - start;
  const avgTime = duration / iterations;

  console.log(`✅ Completed ${iterations} workflow registrations`);
  console.log(`⏱️  Total time: ${duration.toFixed(2)}ms`);
  console.log(`📈 Average time per registration: ${avgTime.toFixed(2)}ms`);
  console.log(
    `🚀 Throughput: ${(iterations / (duration / 1000)).toFixed(2)} registrations/second`
  );

  return { duration, avgTime, throughput: iterations / (duration / 1000) };
}

function benchmarkRunCreation(iterations = 100) {
  console.log(`\n📊 Benchmarking Run Creation (${iterations} iterations)`);

  const start = performance.now();

  for (let i = 0; i < iterations; i++) {
    const payloadJson = JSON.stringify({
      ...testPayload,
      iteration: i,
      timestamp: Date.now(),
    });

    const result = core.createRun(
      'benchmark-workflow-0',
      payloadJson,
      './benchmark.db'
    );

    if (!result.success) {
      console.error(
        `❌ Run creation failed on iteration ${i}:`,
        result.message
      );
      return null;
    }
  }

  const end = performance.now();
  const duration = end - start;
  const avgTime = duration / iterations;

  console.log(`✅ Completed ${iterations} run creations`);
  console.log(`⏱️  Total time: ${duration.toFixed(2)}ms`);
  console.log(`📈 Average time per creation: ${avgTime.toFixed(2)}ms`);
  console.log(
    `🚀 Throughput: ${(iterations / (duration / 1000)).toFixed(2)} creations/second`
  );

  return { duration, avgTime, throughput: iterations / (duration / 1000) };
}

function benchmarkStatusRetrieval(iterations = 100) {
  console.log(`\n📊 Benchmarking Status Retrieval (${iterations} iterations)`);

  // First register the workflow
  const workflowJson = JSON.stringify({
    ...testWorkflow,
    id: 'benchmark-workflow-0',
  });
  const registerResult = core.registerWorkflow(workflowJson, './benchmark.db');
  if (!registerResult.success) {
    console.error('❌ Failed to register workflow for status retrieval test');
    return null;
  }

  // Then create a run to retrieve status for
  const createResult = core.createRun(
    'benchmark-workflow-0',
    JSON.stringify(testPayload),
    './benchmark.db'
  );
  if (!createResult.success) {
    console.error('❌ Failed to create run for status retrieval test');
    return null;
  }

  const runId = createResult.runId;
  if (!runId) {
    console.error('❌ No run ID returned from createRun');
    return null;
  }

  console.log(`🔍 Using run ID: ${runId}`);

  const start = performance.now();

  for (let i = 0; i < iterations; i++) {
    const result = core.getRunStatus(runId, './benchmark.db');

    if (!result.success) {
      console.error(
        `❌ Status retrieval failed on iteration ${i}:`,
        result.message
      );
      return null;
    }
  }

  const end = performance.now();
  const duration = end - start;
  const avgTime = duration / iterations;

  console.log(`✅ Completed ${iterations} status retrievals`);
  console.log(`⏱️  Total time: ${duration.toFixed(2)}ms`);
  console.log(`📈 Average time per retrieval: ${avgTime.toFixed(2)}ms`);
  console.log(
    `🚀 Throughput: ${(iterations / (duration / 1000)).toFixed(2)} retrievals/second`
  );

  return { duration, avgTime, throughput: iterations / (duration / 1000) };
}

function benchmarkStepExecution(iterations = 50) {
  console.log(`\n📊 Benchmarking Step Execution (${iterations} iterations)`);

  // First register the workflow
  const workflowJson = JSON.stringify({
    ...testWorkflow,
    id: 'benchmark-workflow-0',
  });
  const registerResult = core.registerWorkflow(workflowJson, './benchmark.db');
  if (!registerResult.success) {
    console.error('❌ Failed to register workflow for step execution test');
    return null;
  }

  // Then create a run to execute steps for
  const createResult = core.createRun(
    'benchmark-workflow-0',
    JSON.stringify(testPayload),
    './benchmark.db'
  );
  if (!createResult.success) {
    console.error('❌ Failed to create run for step execution test');
    return null;
  }

  const runId = createResult.runId;
  if (!runId) {
    console.error('❌ No run ID returned from createRun');
    return null;
  }

  console.log(`🔍 Using run ID: ${runId}`);

  const start = performance.now();

  for (let i = 0; i < iterations; i++) {
    const result = core.executeStep(runId, 'step1', './benchmark.db');

    if (!result.success) {
      console.error(
        `❌ Step execution failed on iteration ${i}:`,
        result.message
      );
      return null;
    }
  }

  const end = performance.now();
  const duration = end - start;
  const avgTime = duration / iterations;

  console.log(`✅ Completed ${iterations} step executions`);
  console.log(`⏱️  Total time: ${duration.toFixed(2)}ms`);
  console.log(`📈 Average time per execution: ${avgTime.toFixed(2)}ms`);
  console.log(
    `🚀 Throughput: ${(iterations / (duration / 1000)).toFixed(2)} executions/second`
  );

  return { duration, avgTime, throughput: iterations / (duration / 1000) };
}

function benchmarkStartupTime() {
  console.log(`\n📊 Benchmarking Startup Time`);

  const start = performance.now();

  // Test module loading
  const core = require('../core/core.node');

  const end = performance.now();
  const duration = end - start;

  console.log(`✅ Module loaded successfully`);
  console.log(`⏱️  Startup time: ${duration.toFixed(2)}ms`);

  return { duration };
}

function benchmarkMemoryUsage() {
  console.log(`\n📊 Benchmarking Memory Usage`);

  const initialMemory = process.memoryUsage();
  console.log(`💾 Initial memory usage:`);
  console.log(`   RSS: ${(initialMemory.rss / 1024 / 1024).toFixed(2)} MB`);
  console.log(
    `   Heap Used: ${(initialMemory.heapUsed / 1024 / 1024).toFixed(2)} MB`
  );
  console.log(
    `   Heap Total: ${(initialMemory.heapTotal / 1024 / 1024).toFixed(2)} MB`
  );

  // Perform some operations
  for (let i = 0; i < 100; i++) {
    const workflowJson = JSON.stringify({
      ...testWorkflow,
      id: `memory-test-${i}`,
    });
    core.registerWorkflow(workflowJson, './benchmark.db');
  }

  const finalMemory = process.memoryUsage();
  console.log(`💾 Final memory usage:`);
  console.log(`   RSS: ${(finalMemory.rss / 1024 / 1024).toFixed(2)} MB`);
  console.log(
    `   Heap Used: ${(finalMemory.heapUsed / 1024 / 1024).toFixed(2)} MB`
  );
  console.log(
    `   Heap Total: ${(finalMemory.heapTotal / 1024 / 1024).toFixed(2)} MB`
  );

  const memoryIncrease = {
    rss: finalMemory.rss - initialMemory.rss,
    heapUsed: finalMemory.heapUsed - initialMemory.heapUsed,
    heapTotal: finalMemory.heapTotal - initialMemory.heapTotal,
  };

  console.log(`📈 Memory increase:`);
  console.log(`   RSS: +${(memoryIncrease.rss / 1024 / 1024).toFixed(2)} MB`);
  console.log(
    `   Heap Used: +${(memoryIncrease.heapUsed / 1024 / 1024).toFixed(2)} MB`
  );
  console.log(
    `   Heap Total: +${(memoryIncrease.heapTotal / 1024 / 1024).toFixed(2)} MB`
  );

  return { initialMemory, finalMemory, memoryIncrease };
}

// Main benchmark function
async function runBenchmarks() {
  console.log('🚀 Starting Bun + Node-API Performance Benchmark');
  console.log('='.repeat(60));

  const runtime = process.versions.bun ? 'Bun' : 'Node.js';
  const version = process.versions.bun || process.versions.node;
  console.log(`🔧 Runtime: ${runtime} ${version}`);
  console.log(`🖥️  Platform: ${process.platform} ${process.arch}`);

  const results = {};

  // Run benchmarks
  results.startup = benchmarkStartupTime();
  results.registration = benchmarkWorkflowRegistration(50);
  results.runCreation = benchmarkRunCreation(50);
  results.statusRetrieval = benchmarkStatusRetrieval(100);
  results.stepExecution = benchmarkStepExecution(25);
  results.memory = benchmarkMemoryUsage();

  // Summary
  console.log('\n' + '='.repeat(60));
  console.log('📊 BENCHMARK SUMMARY');
  console.log('='.repeat(60));

  if (results.startup) {
    console.log(`🚀 Startup Time: ${results.startup.duration.toFixed(2)}ms`);
  }

  if (results.registration) {
    console.log(
      `📝 Workflow Registration: ${results.registration.avgTime.toFixed(2)}ms avg (${results.registration.throughput.toFixed(2)}/sec)`
    );
  }

  if (results.runCreation) {
    console.log(
      `▶️  Run Creation: ${results.runCreation.avgTime.toFixed(2)}ms avg (${results.runCreation.throughput.toFixed(2)}/sec)`
    );
  }

  if (results.statusRetrieval) {
    console.log(
      `📊 Status Retrieval: ${results.statusRetrieval.avgTime.toFixed(2)}ms avg (${results.statusRetrieval.throughput.toFixed(2)}/sec)`
    );
  }

  if (results.stepExecution) {
    console.log(
      `⚡ Step Execution: ${results.stepExecution.avgTime.toFixed(2)}ms avg (${results.stepExecution.throughput.toFixed(2)}/sec)`
    );
  }

  console.log('\n✅ Benchmark completed successfully!');

  return results;
}

// Run benchmarks if this file is executed directly
if (import.meta.main) {
  runBenchmarks().catch(console.error);
}

module.exports = { runBenchmarks };
