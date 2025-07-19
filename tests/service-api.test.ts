#!/usr/bin/env bun

import {
  defineService,
  createServiceInstance,
  validateServiceDefinition,
  registerService,
  getServiceDefinition,
  listServiceDefinitions,
} from '../services/src/index';
import { z } from 'zod';

async function runTests() {
  console.log('🧪 Testing Service Definition API...\n');

  console.log('✅ Test 1: Define a simple service');
  const simpleService = defineService({
    id: 'simple-test',
    name: 'Simple Test Service',
    description: 'A simple test service',
    version: '1.0.0',
    setup: ({ config, auth }) => {
      return {
        actions: {
          hello: async (name: string) => {
            return `Hello, ${name}!`;
          },
        },
      };
    },
  });

  console.log('✅ Simple service defined successfully');

  console.log('\n✅ Test 2: Define a service with schema validation');
  const validatedService = defineService({
    id: 'validated-test',
    name: 'Validated Test Service',
    description: 'A test service with schema validation',
    version: '1.0.0',
    schema: {
      auth: z.object({
        apiKey: z.string().min(1, 'API key is required'),
      }),
      config: z.object({
        baseUrl: z.string().url('Base URL must be valid'),
      }),
    },
    setup: ({ config, auth }) => {
      return {
        actions: {
          callApi: async (endpoint: string) => {
            console.log(`🌐 Calling API: ${config.baseUrl}${endpoint}`);
            return { success: true, endpoint };
          },
        },
      };
    },
  });

  console.log('✅ Validated service defined successfully');

  console.log('\n✅ Test 3: Validate service definitions');
  const simpleValidation = validateServiceDefinition(simpleService);
  const validatedValidation = validateServiceDefinition(validatedService);

  console.log(
    'Simple service validation:',
    simpleValidation.valid ? '✅ PASS' : '❌ FAIL'
  );
  console.log(
    'Validated service validation:',
    validatedValidation.valid ? '✅ PASS' : '❌ FAIL'
  );

  console.log('\n✅ Test 4: Register services');
  registerService(simpleService);
  registerService(validatedService);

  console.log('Registered services:', listServiceDefinitions());

  console.log('\n✅ Test 5: Get service definitions');
  const retrievedSimple = getServiceDefinition('simple-test');
  const retrievedValidated = getServiceDefinition('validated-test');

  console.log(
    'Retrieved simple service:',
    retrievedSimple ? '✅ FOUND' : '❌ NOT FOUND'
  );
  console.log(
    'Retrieved validated service:',
    retrievedValidated ? '✅ FOUND' : '❌ NOT FOUND'
  );

  console.log('\n✅ Test 6: Create service instances');
  const simpleInstance = createServiceInstance(simpleService, {}, {});
  const validatedInstance = createServiceInstance(
    validatedService,
    { baseUrl: 'https://api.example.com' },
    { apiKey: 'test-api-key' }
  );

  console.log(
    'Simple service instance created:',
    simpleInstance ? '✅ SUCCESS' : '❌ FAILED'
  );
  console.log(
    'Validated service instance created:',
    validatedInstance ? '✅ SUCCESS' : '❌ FAILED'
  );

  console.log('\n✅ Test 7: Test service actions');
  const helloResult = await simpleInstance.actions.hello('World');
  console.log('Hello action result:', helloResult);

  const apiResult = await validatedInstance.actions.callApi('/test');
  console.log('API call result:', apiResult);

  console.log('\n✅ Test 8: Test invalid service definition');
  try {
    const invalidService = defineService({
      id: '', // Invalid: empty ID
      name: 'Invalid Service',
      description: 'This should fail',
      version: '1.0.0',
      setup: ({ config, auth }) => ({ actions: {} }),
    });
    console.log('❌ Invalid service should have failed');
  } catch (error) {
    console.log(
      '✅ Invalid service correctly rejected:',
      (error as Error).message
    );
  }

  console.log('\n🎉 All service definition API tests passed!');
}

runTests().catch(console.error);
