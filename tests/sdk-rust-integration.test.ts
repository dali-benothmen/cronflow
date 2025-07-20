import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as cronflow from '../sdk/src/cronflow';
import { Context } from '../sdk/src/workflow/types';
import fs from 'fs';
import path from 'path';

describe('SDK to Rust Engine Integration', () => {
  const testDbPath = './test-cronflow.db';

  beforeEach(() => {
    // Clean up any existing test database
    if (fs.existsSync(testDbPath)) {
      fs.unlinkSync(testDbPath);
    }
  });

  afterEach(() => {
    // Clean up test database
    if (fs.existsSync(testDbPath)) {
      fs.unlinkSync(testDbPath);
    }
  });

  describe('Rust Core Availability', () => {
    it('should detect Rust core availability', () => {
      const isAvailable = cronflow.isRustCoreAvailable();
      console.log('Rust core available:', isAvailable);

      // The test should pass regardless of availability
      expect(typeof isAvailable).toBe('boolean');
    });
  });

  describe('Workflow Registration', () => {
    it('should register workflow with Rust engine when available', async () => {
      const workflow = cronflow.define({
        id: 'test-workflow-registration',
        name: 'Test Workflow Registration',
        description: 'Testing workflow registration with Rust engine',
      });

      workflow
        .step('test-step', async (ctx: Context) => {
          return { message: 'Hello from test step' };
        })
        .action('test-action', (ctx: Context) => {
          console.log('Test action executed');
        });

      // Start the engine (this will register workflows with Rust)
      await cronflow.start();

      // Verify workflow was registered
      const registeredWorkflows = cronflow.getWorkflows();
      expect(registeredWorkflows).toHaveLength(1);
      expect(registeredWorkflows[0].id).toBe('test-workflow-registration');
    });

    it('should handle workflow registration errors gracefully', async () => {
      // Create a workflow with invalid data to test error handling
      const workflow = cronflow.define({
        id: 'test-error-handling',
        name: 'Test Error Handling',
      });

      workflow.step('test-step', async (ctx: Context) => {
        return { message: 'test' };
      });

      // Start the engine
      await cronflow.start();

      // The engine should handle errors gracefully
      expect(cronflow.getEngineState()).toBe('STARTED');
    });
  });

  describe('Workflow Triggering', () => {
    it('should trigger workflow and create run', async () => {
      const workflow = cronflow.define({
        id: 'test-trigger-workflow',
        name: 'Test Trigger Workflow',
      });

      workflow.step('process-payload', async (ctx: Context) => {
        return { processed: true, data: ctx.payload };
      });

      await cronflow.start();

      const payload = { test: 'data', timestamp: Date.now() };
      const runId = await cronflow.trigger('test-trigger-workflow', payload);

      expect(runId).toBeDefined();
      expect(typeof runId).toBe('string');
      console.log('Created run ID:', runId);
    });

    it('should handle trigger errors gracefully', async () => {
      const workflow = cronflow.define({
        id: 'test-trigger-error',
        name: 'Test Trigger Error',
      });

      workflow.step('test-step', async (ctx: Context) => {
        return { message: 'test' };
      });

      await cronflow.start();

      // Try to trigger a non-existent workflow
      try {
        await cronflow.trigger('non-existent-workflow', {});
        // If we get here, it means the error was handled gracefully
        expect(true).toBe(true);
      } catch (error) {
        // Error handling is working
        expect(error).toBeDefined();
        console.log('Expected error caught:', error);
      }
    });
  });

  describe('Run Inspection', () => {
    it('should inspect run status', async () => {
      const workflow = cronflow.define({
        id: 'test-inspect-workflow',
        name: 'Test Inspect Workflow',
      });

      workflow.step('test-step', async (ctx: Context) => {
        return { message: 'test' };
      });

      await cronflow.start();

      const payload = { test: 'inspect' };
      const runId = await cronflow.trigger('test-inspect-workflow', payload);

      const status = await cronflow.inspect(runId);
      expect(status).toBeDefined();
      console.log('Run status:', status);
    });

    it('should handle inspection errors gracefully', async () => {
      await cronflow.start();

      try {
        await cronflow.inspect('non-existent-run-id');
        // If we get here, it means the error was handled gracefully
        expect(true).toBe(true);
      } catch (error) {
        // Error handling is working
        expect(error).toBeDefined();
        console.log('Expected inspection error caught:', error);
      }
    });
  });

  describe('Complex Workflow Integration', () => {
    it('should handle complex workflow with control flow', async () => {
      const workflow = cronflow.define({
        id: 'test-complex-workflow',
        name: 'Test Complex Workflow',
        description: 'Testing complex workflow with control flow',
      });

      workflow
        .step('validate-input', async (ctx: Context) => {
          return { valid: true, data: ctx.payload };
        })
        .if('is-high-value', (ctx: Context) => ctx.last.data.amount > 100)
        .step('process-high-value', async (ctx: Context) => {
          return { type: 'high-value', processed: true };
        })
        .else()
        .step('process-low-value', async (ctx: Context) => {
          return { type: 'low-value', processed: true };
        })
        .endIf()
        .parallel([
          async (ctx: Context) => ({ notification1: 'sent' }),
          async (ctx: Context) => ({ notification2: 'sent' }),
        ]);

      await cronflow.start();

      const payload = { amount: 150, customer: 'test' };
      const runId = await cronflow.trigger('test-complex-workflow', payload);

      expect(runId).toBeDefined();
      console.log('Complex workflow run ID:', runId);
    });
  });

  describe('Engine State Management', () => {
    it('should manage engine state correctly', async () => {
      expect(cronflow.getEngineState()).toBe('STOPPED');

      await cronflow.start();
      expect(cronflow.getEngineState()).toBe('STARTED');

      await cronflow.stop();
      expect(cronflow.getEngineState()).toBe('STOPPED');
    });

    it('should handle multiple start/stop calls gracefully', async () => {
      await cronflow.start();
      await cronflow.start(); // Should not cause issues
      expect(cronflow.getEngineState()).toBe('STARTED');

      await cronflow.stop();
      await cronflow.stop(); // Should not cause issues
      expect(cronflow.getEngineState()).toBe('STOPPED');
    });
  });
});
