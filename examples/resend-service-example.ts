#!/usr/bin/env bun

import { cronflow } from '../sdk/src/index';
import { defineService } from '../services/src/index';
import { z } from 'zod';

// Define the Resend service template
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
      defaultFrom: z
        .string()
        .email('Default from email must be valid')
        .optional(),
    }),
  },
  setup: ({ config, auth }) => {
    // In a real implementation, we would import the actual Resend SDK
    // import { Resend } from 'resend';
    // const resend = new Resend(auth.apiKey);

    return {
      actions: {
        send: async (params: { to: string; subject: string; html: string }) => {
          console.log('📧 Sending email via Resend:', {
            to: params.to,
            subject: params.subject,
            from: config.defaultFrom || 'noreply@company.com',
          });

          // Simulate API call
          await new Promise(resolve => setTimeout(resolve, 100));

          return {
            id: `email_${Date.now()}`,
            to: params.to,
            subject: params.subject,
            status: 'sent',
            provider: 'resend',
          };
        },
      },
    };
  },
});

// Create configured service instance
const resendService = resendServiceTemplate.withConfig({
  auth: {
    apiKey: process.env.RESEND_API_KEY || 'test-api-key',
  },
  config: {
    defaultFrom: 'noreply@company.com',
  },
});

// Define a workflow that uses the service
const orderNotificationWorkflow = cronflow.define({
  id: 'order-notification',
  name: 'Order Notification Workflow',
  description: 'Sends notifications when orders are processed',
  services: [resendService],
  tags: ['ecommerce', 'notifications'],
});

// Define the workflow steps
orderNotificationWorkflow
  .step('validate-order', async ctx => {
    console.log('Validating order...');
    const order = ctx.payload;

    if (!order.orderId || !order.customerEmail) {
      throw new Error('Invalid order data');
    }

    return {
      orderId: order.orderId,
      customerEmail: order.customerEmail,
      amount: order.amount || 0,
      status: 'validated',
    };
  })
  .step('send-order-confirmation', async ctx => {
    console.log('Sending order confirmation...');

    const order = ctx.steps['validate-order'].output;

    // Access the Resend service from ctx.services
    const emailService = ctx.services.resend;
    if (!emailService) {
      throw new Error('Resend service not available');
    }

    const emailResult = await emailService.actions.send({
      to: order.customerEmail,
      subject: `Order Confirmation - #${order.orderId}`,
      html: `
        <h1>Thank you for your order!</h1>
        <p>Order ID: ${order.orderId}</p>
        <p>Amount: $${order.amount}</p>
        <p>We'll notify you when your order ships.</p>
      `,
    });

    return {
      emailSent: true,
      emailId: emailResult.id,
      customerEmail: order.customerEmail,
    };
  })
  .step('log-notification', async ctx => {
    console.log('Logging notification...');

    const emailResult = ctx.steps['send-order-confirmation'].output;

    return {
      logged: true,
      timestamp: new Date().toISOString(),
      emailId: emailResult.emailId,
      customerEmail: emailResult.customerEmail,
    };
  });

// Example usage
async function runExample() {
  console.log('🚀 Running Order Notification Workflow Example...\n');

  // Start the cronflow engine
  await cronflow.start();

  // Trigger the workflow with order data
  const runId = await cronflow.trigger('order-notification', {
    orderId: 'ORD-12345',
    customerEmail: 'customer@example.com',
    amount: 99.99,
  });

  console.log(`✅ Workflow triggered with run ID: ${runId}`);

  // In a real application, the workflow would execute automatically
  // For this example, we'll simulate the execution
  console.log('\n📋 Workflow Steps:');
  console.log('1. validate-order - Validates the order data');
  console.log('2. send-order-confirmation - Sends email via Resend service');
  console.log('3. log-notification - Logs the notification');

  console.log('\n🔧 Service Integration:');
  console.log('- Resend service is configured with API key');
  console.log('- Service is available in ctx.services.resend');
  console.log('- Service actions can be called from workflow steps');

  console.log('\n✨ Key Features Demonstrated:');
  console.log('✅ Service definition with validation');
  console.log('✅ Service configuration with .withConfig()');
  console.log('✅ Service integration in workflow definition');
  console.log('✅ Service access via ctx.services');
  console.log('✅ Service action execution in workflow steps');

  console.log('\n🎉 Example completed successfully!');
}

// Run the example
runExample().catch(console.error);
