import { describe, it, expect } from 'vitest';
import { cronflow, VERSION, WorkflowDefinitionSchema } from './index';

describe('Node-Cronflow SDK', () => {
  it('should export correct version', () => {
    expect(VERSION).toBe('0.1.0');
  });

  it('should initialize cronflow singleton', () => {
    expect(cronflow).toBeDefined();
    expect(typeof cronflow.define).toBe('function');
    expect(typeof cronflow.start).toBe('function');
  });

  it('should validate workflow definitions', () => {
    const validWorkflow = {
      id: 'test-workflow',
      name: 'Test Workflow',
      description: 'A test workflow',
      steps: [],
      triggers: [],
      created_at: new Date(),
      updated_at: new Date(),
    };

    expect(() => WorkflowDefinitionSchema.parse(validWorkflow)).not.toThrow();
  });

  it('should define a workflow', () => {
    const workflow = cronflow.define('test', {
      name: 'Test Workflow',
      description: 'A test workflow',
      steps: [],
      triggers: [],
    });

    expect(workflow).toBeDefined();
    expect(workflow.getDefinition().id).toBe('test');
  });

  it('should get defined workflows', () => {
    const workflows = cronflow.getWorkflows();
    expect(Array.isArray(workflows)).toBe(true);
  });
});
