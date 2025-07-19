#!/usr/bin/env bun

import { cronflow, WorkflowDefinitionSchema } from '../sdk/src/index';
import { z } from 'zod';

async function runTests() {
  console.log('🧪 Testing WorkflowDefinitionSchema Export...\n');

  console.log('✅ Test 1: Import WorkflowDefinitionSchema');
  console.log('Schema imported successfully:', typeof WorkflowDefinitionSchema);

  console.log('\n✅ Test 2: Create a valid workflow definition');
  const validWorkflow = {
    id: 'test-workflow',
    name: 'Test Workflow',
    description: 'A test workflow',
    steps: [
      {
        id: 'test-step',
        name: 'test-step',
        handler: () => 'test',
        type: 'step' as const,
      },
    ],
    triggers: [{ type: 'manual' as const }],
    created_at: new Date(),
    updated_at: new Date(),
  };

  console.log('✅ Valid workflow definition created');

  console.log('\n✅ Test 3: Validate workflow with schema');
  try {
    WorkflowDefinitionSchema.parse(validWorkflow);
    console.log('✅ Workflow validation passed');
  } catch (error) {
    console.error('❌ Workflow validation failed:', error);
    throw error;
  }

  console.log('\n✅ Test 4: Test invalid workflow (missing required fields)');
  const invalidWorkflow = {
    id: '', // Empty ID should fail
    steps: [], // Empty steps should fail
    triggers: [],
    created_at: new Date(),
    updated_at: new Date(),
  };

  try {
    WorkflowDefinitionSchema.parse(invalidWorkflow);
    console.error('❌ Invalid workflow should have failed validation');
    throw new Error('Invalid workflow passed validation');
  } catch (error) {
    if (error instanceof z.ZodError) {
      console.log('✅ Invalid workflow correctly failed validation');
      console.log('Validation errors:', error.errors.length);
    } else {
      throw error;
    }
  }

  console.log('\n✅ Test 5: Test workflow with services');
  const workflowWithServices = {
    id: 'service-workflow',
    name: 'Service Workflow',
    description: 'A workflow with services',
    services: [
      {
        id: 'email',
        name: 'Email Service',
        version: '1.0.0',
        config: {},
        auth: {},
        actions: {},
      },
    ],
    steps: [
      {
        id: 'test-step',
        name: 'test-step',
        handler: () => 'test',
        type: 'step' as const,
      },
    ],
    triggers: [{ type: 'manual' as const }],
    created_at: new Date(),
    updated_at: new Date(),
  };

  try {
    WorkflowDefinitionSchema.parse(workflowWithServices);
    console.log('✅ Workflow with services validation passed');
  } catch (error) {
    console.error('❌ Workflow with services validation failed:', error);
    throw error;
  }

  console.log('\n✅ Test 6: Test schema in cronflow.define');
  const workflow = cronflow.define({
    id: 'schema-test-workflow',
    name: 'Schema Test Workflow',
    description: 'Testing schema validation',
  });

  workflow.step('test-step', async ctx => {
    return { test: true };
  });

  console.log('✅ Workflow created and step added successfully');

  console.log('\n✅ Test 7: Validate complete workflow');
  try {
    const definition = workflow.getDefinition();
    WorkflowDefinitionSchema.parse(definition);
    console.log('✅ Complete workflow validation passed');
  } catch (error) {
    console.error('❌ Complete workflow validation failed:', error);
    throw error;
  }

  console.log('\n🎉 All schema export tests passed!');
}

runTests().catch(console.error);
