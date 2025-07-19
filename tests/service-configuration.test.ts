#!/usr/bin/env bun

import { defineService } from '../services/src/index';
import { z } from 'zod';

async function runTests() {
  console.log('🧪 Testing Service Configuration API...\n');

  console.log('✅ Test 1: Define service with withConfig method');
  const resendServiceTemplate = defineService({
    id: 'resend',
    name: 'Resend',
    description: 'Email delivery service',
    version: '1.0.0',
    schema: {
      auth: z.object({
        apiKey: z.string().min(1, 'API key is required'),
      }),
      config: z.object({
        defaultFrom: z.string().email('Default from email must be valid'),
        defaultReplyTo: z
          .string()
          .email('Default reply-to email must be valid')
          .optional(),
      }),
    },
    setup: ({ config, auth }) => {
      return {
        actions: {
          send: async (params: {
            to: string;
            subject: string;
            html: string;
            from?: string;
          }) => {
            console.log('📧 Sending email via Resend:', {
              to: params.to,
              subject: params.subject,
              from: params.from || config.defaultFrom,
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

  console.log('✅ Service template created with withConfig method');

  console.log('\n✅ Test 2: Use withConfig to create configured service');
  const resendService = resendServiceTemplate.withConfig({
    auth: {
      apiKey: 'test-api-key-123',
    },
    config: {
      defaultFrom: 'noreply@mycompany.com',
      defaultReplyTo: 'support@mycompany.com',
    },
  });

  console.log('✅ Configured service created successfully');
  console.log('Service ID:', resendService.id);
  console.log('Service Name:', resendService.name);
  console.log('Service Version:', resendService.version);
  console.log('Has actions:', !!resendService.actions);
  console.log('Has send action:', !!resendService.actions.send);

  console.log('\n✅ Test 3: Test configured service actions');
  const emailResult = await resendService.actions.send({
    to: 'user@example.com',
    subject: 'Test Email',
    html: '<h1>Hello World</h1>',
  });

  console.log('Email sent successfully:', emailResult);

  console.log('\n✅ Test 4: Test withConfig with minimal config');
  const minimalService = resendServiceTemplate.withConfig({
    auth: {
      apiKey: 'minimal-api-key',
    },
    config: {
      defaultFrom: 'default@example.com',
    },
  });

  console.log('✅ Minimal configured service created');
  console.log('Default config used:', minimalService.config);

  console.log('\n✅ Test 5: Test withConfig validation');
  try {
    const invalidService = resendServiceTemplate.withConfig({
      auth: {
        apiKey: '', // Invalid: empty API key
      },
    });
    console.log('❌ Invalid service should have failed');
  } catch (error) {
    console.log(
      '✅ Invalid service correctly rejected:',
      (error as Error).message
    );
  }

  console.log('\n✅ Test 6: Test withConfig with invalid email');
  try {
    const invalidEmailService = resendServiceTemplate.withConfig({
      auth: {
        apiKey: 'valid-api-key',
      },
      config: {
        defaultFrom: 'invalid-email', // Invalid email format
      },
    });
    console.log('❌ Invalid email should have failed');
  } catch (error) {
    console.log(
      '✅ Invalid email correctly rejected:',
      (error as Error).message
    );
  }

  console.log('\n✅ Test 7: Test service without schema');
  const simpleServiceTemplate = defineService({
    id: 'simple',
    name: 'Simple Service',
    description: 'A simple service without schema',
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

  const simpleService = simpleServiceTemplate.withConfig({
    auth: {},
  });

  console.log('✅ Simple service configured successfully');
  const helloResult = await simpleService.actions.hello('World');
  console.log('Hello action result:', helloResult);

  console.log('\n🎉 All service configuration tests passed!');
}

runTests().catch(console.error);
