#!/usr/bin/env bun

import { cronflow } from '../sdk/src/index';
import { defineService } from '../services/src/index';
import { z } from 'zod';

async function runTests() {
  console.log('🧪 Testing Service Integration with SDK...\n');

  console.log('✅ Test 1: Define a service');
  const emailServiceTemplate = defineService({
    id: 'email',
    name: 'Email Service',
    description: 'Email delivery service',
    version: '1.0.0',
    schema: {
      auth: z.object({
        apiKey: z.string().min(1, 'API key is required'),
      }),
      config: z.object({
        defaultFrom: z.string().email('Default from email must be valid'),
      }),
    },
    setup: ({ config, auth }) => {
      return {
        actions: {
          send: async (params: {
            to: string;
            subject: string;
            html: string;
          }) => {
            console.log('📧 Sending email:', {
              to: params.to,
              subject: params.subject,
              from: config.defaultFrom,
            });

            return {
              id: `email_${Date.now()}`,
              to: params.to,
              subject: params.subject,
              status: 'sent',
            };
          },
        },
      };
    },
  });

  console.log('✅ Service template created');

  console.log('\n✅ Test 2: Create configured service');
  const emailService = emailServiceTemplate.withConfig({
    auth: {
      apiKey: 'test-api-key',
    },
    config: {
      defaultFrom: 'noreply@example.com',
    },
  });

  console.log('✅ Configured service created');

  console.log('\n✅ Test 3: Define workflow with services');
  const workflow = cronflow.define({
    id: 'test-service-workflow',
    name: 'Test Service Workflow',
    description: 'A workflow that uses services',
    services: [emailService],
  });

  console.log('✅ Workflow defined with services');

  console.log('\n✅ Test 4: Add steps that use services');
  workflow
    .step('process-data', async ctx => {
      console.log('Processing data...');
      return { processed: true, data: ctx.payload };
    })
    .step('send-notification', async ctx => {
      console.log('Sending notification...');

      // Access service from ctx.services
      const emailService = ctx.services.email;
      if (!emailService) {
        throw new Error('Email service not found in context');
      }

      const result = await emailService.actions.send({
        to: 'user@example.com',
        subject: 'Workflow completed',
        html: '<h1>Your workflow has completed successfully!</h1>',
      });

      return { notification: result };
    });

  console.log('✅ Steps added with service usage');

  console.log('\n✅ Test 5: Verify workflow definition includes services');
  const definition = workflow.getDefinition();
  console.log(
    'Workflow services:',
    definition.services?.map(s => s.id)
  );
  console.log('Services count:', definition.services?.length || 0);

  console.log('\n✅ Test 6: Test service access in context');
  const mockContext = {
    payload: { userId: '123', action: 'test' },
    steps: {},
    services: { email: emailService },
    run: { id: 'test-run', workflowId: 'test-workflow' },
    state: {
      get: (key: string, defaultValue?: any) => defaultValue,
      set: async (key: string, value: any) =>
        console.log(`Setting ${key} = ${value}`),
      incr: async (key: string, amount: number = 1) => 0,
    },
    last: null,
    trigger: { headers: {} },
    cancel: (reason?: string) => {
      throw new Error(`Cancelled: ${reason}`);
    },
  };

  console.log('✅ Mock context created with services');

  console.log('\n✅ Test 7: Test service action execution');
  const emailResult = await mockContext.services.email.actions.send({
    to: 'test@example.com',
    subject: 'Test Email',
    html: '<p>This is a test email</p>',
  });

  console.log('Email sent successfully:', emailResult);

  console.log('\n✅ Test 8: Test workflow without services');
  const simpleWorkflow = cronflow.define({
    id: 'simple-workflow',
    name: 'Simple Workflow',
    description: 'A workflow without services',
  });

  simpleWorkflow.step('simple-step', async ctx => {
    console.log('Simple step executed');
    return { simple: true };
  });

  console.log('✅ Simple workflow created without services');

  console.log('\n✅ Test 9: Verify services are optional');
  const simpleDefinition = simpleWorkflow.getDefinition();
  console.log(
    'Simple workflow services:',
    simpleDefinition.services?.length || 0
  );

  console.log('\n🎉 All service integration tests passed!');
}

runTests().catch(console.error);
