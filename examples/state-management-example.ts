import { cronflow } from '../sdk/src/index';

console.log('🚀 State Management Example\n');

// Define a comprehensive workflow that demonstrates state management features
const stateManagementWorkflow = cronflow.define({
  id: 'state-management-demo',
  name: 'State Management Demo',
  description:
    'Demonstrates comprehensive state management capabilities with persistence, TTL, and conditional logic',
});

// Build the workflow with state management features
stateManagementWorkflow
  // Step 1: Initialize application state
  .step('initialize-app-state', async ctx => {
    console.log('📦 Initializing application state...');

    const initialState = {
      users: [],
      stats: {
        totalRequests: 0,
        successfulRequests: 0,
        failedRequests: 0,
        averageResponseTime: 0,
      },
      settings: {
        theme: 'dark',
        notifications: true,
        autoSave: true,
        maxRetries: 3,
      },
      cache: {
        hits: 0,
        misses: 0,
        hitRate: 0,
      },
      system: {
        lastMaintenance: Date.now(),
        uptime: 0,
        version: '1.0.0',
      },
    };

    await ctx.state.set('app-state', initialState);
    await ctx.state.set('session-start', Date.now());
    await ctx.state.set('user-sessions', 0);

    return { message: 'Application state initialized', timestamp: Date.now() };
  })

  // Step 2: Process user request with state tracking
  .step('process-user-request', async ctx => {
    console.log('👤 Processing user request...');

    const appState = await ctx.state.get('app-state');
    const sessionCount = await ctx.state.incr('user-sessions', 1);

    // Update request statistics
    appState.stats.totalRequests += 1;
    appState.stats.successfulRequests += 1;

    // Simulate response time
    const responseTime = Math.random() * 100 + 50; // 50-150ms
    appState.stats.averageResponseTime =
      (appState.stats.averageResponseTime *
        (appState.stats.successfulRequests - 1) +
        responseTime) /
      appState.stats.successfulRequests;

    // Update cache statistics
    if (Math.random() > 0.3) {
      // 70% cache hit rate
      appState.cache.hits += 1;
    } else {
      appState.cache.misses += 1;
    }

    appState.cache.hitRate =
      appState.cache.hits / (appState.cache.hits + appState.cache.misses);

    await ctx.state.set('app-state', appState);

    return {
      sessionCount,
      responseTime,
      cacheHit: appState.cache.hits > appState.cache.misses,
      totalRequests: appState.stats.totalRequests,
    };
  })

  // Step 3: Handle errors with state-based recovery
  .step('handle-errors', async ctx => {
    console.log('⚠️ Checking for errors...');

    const appState = await ctx.state.get('app-state');
    const errorCount = await ctx.state.get('error-count', 0);

    // Simulate occasional errors
    if (Math.random() < 0.2) {
      // 20% error rate
      const newErrorCount = await ctx.state.incr('error-count', 1);
      appState.stats.failedRequests += 1;

      await ctx.state.set('app-state', appState);
      await ctx.state.set('last-error', Date.now());

      return {
        error: true,
        errorCount: newErrorCount,
        message: 'Error occurred and tracked',
      };
    }

    return { error: false, errorCount };
  })

  // Step 4: Conditional error recovery
  .if('needs-recovery', async ctx => {
    const errorCount = await ctx.state.get('error-count', 0);
    return errorCount > 2;
  })
  .step('activate-recovery-mode', async ctx => {
    console.log('🔄 Activating recovery mode...');

    const appState = await ctx.state.get('app-state');
    appState.settings.maxRetries = 5;
    appState.system.lastMaintenance = Date.now();

    await ctx.state.set('app-state', appState);
    await ctx.state.set('recovery-mode', true, { ttl: '30m' }); // Auto-disable after 30 minutes
    await ctx.state.set('recovery-activated', Date.now());

    return {
      action: 'recovery-mode-activated',
      maxRetries: appState.settings.maxRetries,
      recoveryUntil: Date.now() + 30 * 60 * 1000, // 30 minutes
    };
  })
  .step('reset-error-count', async ctx => {
    await ctx.state.set('error-count', 0);
    return { message: 'Error count reset after recovery activation' };
  })
  .else()
  .step('normal-operation', async ctx => {
    console.log('✅ Normal operation continuing...');
    return { message: 'System operating normally' };
  })
  .endIf()

  // Step 5: Periodic maintenance with state cleanup
  .step('periodic-maintenance', async ctx => {
    console.log('🔧 Performing periodic maintenance...');

    const appState = await ctx.state.get('app-state');
    const sessionStart = await ctx.state.get('session-start');
    const uptime = Date.now() - sessionStart;

    // Update system uptime
    appState.system.uptime = uptime;

    // Clean up old session data (simulate cleanup)
    const oldSessions = await ctx.state.get('old-sessions', 0);
    await ctx.state.set('old-sessions', oldSessions + 1);

    // Set maintenance flag with TTL
    await ctx.state.set('maintenance-active', true, { ttl: '5m' });

    await ctx.state.set('app-state', appState);
    await ctx.state.set('last-maintenance', Date.now());

    return {
      uptime: Math.floor(uptime / 1000), // seconds
      maintenanceActive: true,
      oldSessionsCleaned: oldSessions + 1,
    };
  })

  // Step 6: User session management with TTL
  .step('manage-user-sessions', async ctx => {
    console.log('👥 Managing user sessions...');

    const sessionCount = await ctx.state.get('user-sessions', 0);

    // Create session data with TTL
    const sessionId = `session_${Date.now()}`;
    const sessionData = {
      userId: `user_${Math.floor(Math.random() * 1000)}`,
      createdAt: Date.now(),
      lastActivity: Date.now(),
      preferences: {
        theme: 'dark',
        language: 'en',
      },
    };

    await ctx.state.set(`session:${sessionId}`, sessionData, { ttl: '2h' });
    await ctx.state.set('active-sessions', sessionCount);

    return {
      sessionId,
      sessionData,
      totalSessions: sessionCount,
    };
  })

  // Step 7: Analytics and reporting
  .step('generate-analytics', async ctx => {
    console.log('📊 Generating analytics report...');

    const appState = await ctx.state.get('app-state');
    const sessionCount = await ctx.state.get('user-sessions', 0);
    const errorCount = await ctx.state.get('error-count', 0);
    const recoveryMode = await ctx.state.get('recovery-mode', false);

    const analytics = {
      timestamp: Date.now(),
      performance: {
        totalRequests: appState.stats.totalRequests,
        successRate:
          (appState.stats.successfulRequests / appState.stats.totalRequests) *
          100,
        averageResponseTime: appState.stats.averageResponseTime,
        errorRate:
          (appState.stats.failedRequests / appState.stats.totalRequests) * 100,
      },
      cache: {
        hits: appState.cache.hits,
        misses: appState.cache.misses,
        hitRate: appState.cache.hitRate * 100,
      },
      system: {
        uptime: appState.system.uptime,
        activeSessions: sessionCount,
        errorCount,
        recoveryMode,
        version: appState.system.version,
      },
      settings: appState.settings,
    };

    // Store analytics with TTL (keep for 24 hours)
    await ctx.state.set('analytics-report', analytics, { ttl: '24h' });

    return analytics;
  })

  // Step 8: Final state summary
  .step('final-state-summary', async ctx => {
    console.log('📋 Generating final state summary...');

    const appState = await ctx.state.get('app-state');
    const sessionCount = await ctx.state.get('user-sessions', 0);
    const errorCount = await ctx.state.get('error-count', 0);
    const recoveryMode = await ctx.state.get('recovery-mode', false);
    const maintenanceActive = await ctx.state.get('maintenance-active', false);

    const summary = {
      workflowId: ctx.run.workflowId,
      runId: ctx.run.id,
      timestamp: Date.now(),
      state: {
        totalRequests: appState.stats.totalRequests,
        successRate:
          (appState.stats.successfulRequests / appState.stats.totalRequests) *
          100,
        errorCount,
        recoveryMode,
        maintenanceActive,
        activeSessions: sessionCount,
      },
      performance: {
        averageResponseTime: appState.stats.averageResponseTime,
        cacheHitRate: appState.cache.hitRate * 100,
        uptime: appState.system.uptime,
      },
    };

    console.log('📊 Final State Summary:', summary);
    return summary;
  });

// Simulate multiple workflow runs to demonstrate state persistence
async function simulateMultipleRuns() {
  console.log('\n🔄 Simulating multiple workflow runs...');

  for (let i = 1; i <= 3; i++) {
    console.log(`\n--- Run ${i} ---`);

    try {
      const result = await cronflow.trigger('state-management-demo', {
        runNumber: i,
        timestamp: Date.now(),
      });

      console.log(`✅ Run ${i} completed with ID: ${result}`);

      // Wait between runs to see state persistence
      await new Promise(resolve => setTimeout(resolve, 1000));
    } catch (error) {
      console.error(`❌ Run ${i} failed:`, error);
    }
  }
}

// Start the engine and run the demo
async function runStateManagementDemo() {
  try {
    console.log('🚀 Starting State Management Demo...\n');

    // Start the engine
    await cronflow.start();

    // Set some global state
    await cronflow.setGlobalState('demo-started', Date.now());
    await cronflow.setGlobalState('demo-version', '1.0.0');

    // Run the workflow
    const result = await cronflow.trigger('state-management-demo', {
      demo: true,
      timestamp: Date.now(),
    });

    console.log('\n✅ Workflow triggered successfully!');
    console.log(`📋 Run ID: ${result}`);

    // Simulate multiple runs
    await simulateMultipleRuns();

    // Get state statistics
    const stats = await cronflow.getStateStats();
    console.log('\n📊 State Statistics:', stats);

    // Cleanup expired state
    const cleanup = await cronflow.cleanupExpiredState();
    console.log('🧹 Cleanup Results:', cleanup);

    console.log('\n📊 Demo Summary:');
    console.log('✅ State management features demonstrated:');
    console.log('  • Persistent state across workflow runs');
    console.log('  • TTL support for automatic expiration');
    console.log('  • Global and workflow-specific state');
    console.log('  • Complex state operations with nested objects');
    console.log('  • Conditional state logic and recovery');
    console.log('  • State cleanup and statistics');
    console.log('  • Namespace isolation for different workflows');
    console.log('  • Session management with TTL');
    console.log('  • Analytics and reporting with state');
    console.log('  • Error tracking and recovery mechanisms');
  } catch (error) {
    console.error('❌ Demo failed:', error);
  }
}

// Run the demo
runStateManagementDemo();
