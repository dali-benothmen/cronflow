# Node-Cronflow Example Usage

This document provides comprehensive examples of how to use the node-cronflow SDK to build reliable, scalable workflow automation.

## Table of Contents

1. [Basic Setup](#basic-setup)
2. [Service Definitions](#service-definitions)
3. [Workflow Definition](#workflow-definition)
4. [Complete Order Processing Example](#complete-order-processing-example)
5. [Testing Workflows](#testing-workflows)

---

## Basic Setup

First, install and import the SDK:

```typescript
import { cronflow } from 'node-cronflow';
```

---

## Service Definitions

Define reusable service integrations that can be used across workflows.

### Step 1: Create Service Templates

Create service definition files for each integration:

**services/stripe.ts**
```typescript
import { defineService } from 'node-cronflow';
import Stripe from 'stripe';
import { z } from 'zod';

export const stripeServiceTemplate = defineService({
  id: 'stripe',
  name: 'Stripe',
  description: 'Payment processing service',
  version: '1.0.0',
  schema: {
    auth: z.object({
      apiKey: z.string(),
      webhookSecret: z.string(),
    }),
  },
  setup: ({ auth }) => {
    const stripe = new Stripe(auth.apiKey);
    
    return {
      actions: {
        customers: {
          addTag: async (params: { customerId: string; tag: string }) => {
            return await stripe.customers.update(params.customerId, {
              metadata: { [params.tag]: 'true' },
            });
          },
        },
      },
      webhooks: {
        // A special action to securely validate the webhook signature
        constructEvent: (payload: Buffer, signature: string) => {
          return stripe.webhooks.constructEvent(payload, signature, auth.webhookSecret);
        },
      },
    };
  },
});
```

**services/slack.ts**
```typescript
import { defineService } from 'node-cronflow';
import { z } from 'zod';

export const slackServiceTemplate = defineService({
  id: 'slack',
  name: 'Slack',
  description: 'Team communication service',
  version: '1.0.0',
  schema: {
    auth: z.object({
      token: z.string(),
    }),
  },
  setup: ({ auth }) => {
    return {
      actions: {
        sendMessage: async (params: { channel: string; text: string }) => {
          // Simulate Slack API call
          console.log(`Sending to #${params.channel}: ${params.text}`);
          return { ok: true, ts: Date.now().toString() };
        },
      },
    };
  },
});
```

**services/jira.ts**
```typescript
import { defineService } from 'node-cronflow';
import { z } from 'zod';

export const jiraServiceTemplate = defineService({
  id: 'jira',
  name: 'JIRA',
  description: 'Project management service',
  version: '1.0.0',
  schema: {
    auth: z.object({
      email: z.string().email(),
      apiToken: z.string(),
      baseUrl: z.string().url(),
    }),
  },
  setup: ({ auth }) => {
    return {
      actions: {
        createIssue: async (params: { project: string; title: string; description: string }) => {
          // Simulate JIRA API call
          console.log(`Creating issue in ${params.project}: ${params.title}`);
          return { key: 'FULFILL-123', id: '12345' };
        },
      },
    };
  },
});
```

**services/resend.ts**
```typescript
import { defineService } from 'node-cronflow';
import { z } from 'zod';

export const resendServiceTemplate = defineService({
  id: 'resend',
  name: 'Resend',
  description: 'Email delivery service',
  version: '1.0.0',
  schema: {
    auth: z.object({
      apiKey: z.string(),
    }),
  },
  setup: ({ auth }) => {
    return {
      actions: {
        send: async (params: { to: string; subject: string; html: string }) => {
          // Simulate email sending
          console.log(`Sending email to ${params.to}: ${params.subject}`);
          return { id: 'email_123', status: 'sent' };
        },
      },
    };
  },
});
```

### Step 2: Create Configured Service Instances

Configure the service templates with actual secrets for your specific workflow:

**workflows/order-processing/services.ts**
```typescript
import { stripeServiceTemplate } from '../../services/stripe';
import { slackServiceTemplate } from '../../services/slack';
import { jiraServiceTemplate } from '../../services/jira';
import { resendServiceTemplate } from '../../services/resend';

// Pre-configure all the services this workflow will need
export const stripeService = stripeServiceTemplate.withConfig({
  auth: {
    apiKey: process.env.STRIPE_API_KEY!,
    webhookSecret: process.env.STRIPE_WEBHOOK_SECRET!,
  },
});

export const slackService = slackServiceTemplate.withConfig({
  auth: {
    token: process.env.SLACK_BOT_TOKEN!,
  },
});

export const jiraService = jiraServiceTemplate.withConfig({
  auth: {
    email: process.env.JIRA_EMAIL!,
    apiToken: process.env.JIRA_API_TOKEN!,
    baseUrl: process.env.JIRA_BASE_URL!,
  },
});

export const resendService = resendServiceTemplate.withConfig({
  auth: {
    apiKey: process.env.RESEND_API_KEY!,
  },
});
```

---

## Workflow Definition

Define the main workflow logic using the fluent API:

**workflows/order-processing/workflow.ts**
```typescript
import { cronflow } from 'node-cronflow';
import { z } from 'zod';
import { db } from '../../lib/db'; // Your Prisma/Drizzle/etc. client
import { stripeService, slackService, jiraService, resendService } from './services';

// Define the workflow, its metadata, and its dependencies (services)
export const orderProcessingWorkflow = cronflow.define({
  id: 'v1-order-processing',
  name: 'Order Fulfillment Workflow',
  description: 'Process orders from Stripe webhooks and send notifications',
  tags: ['ecommerce', 'critical'],
  services: [stripeService, slackService, jiraService, resendService],
  concurrency: 20, // Handle up to 20 concurrent new orders
  timeout: '5m',
  hooks: {
    onSuccess: (ctx) => {
      console.log(`✅ Workflow completed successfully: ${ctx.run.id}`);
    },
    onFailure: (ctx) => {
      console.error(`❌ Workflow failed: ${ctx.run.id}`, ctx.error);
      // Use the configured Slack service to send an alert
      ctx.services.slack.sendMessage({
        channel: '#ops-alerts',
        text: `🚨 **Order workflow failed!**\nRun ID: \`${ctx.run.id}\`\nError: ${ctx.error?.message}`,
      });
    },
  },
});

// Define the trigger: a Stripe webhook
orderProcessingWorkflow.onWebhook('/webhooks/stripe', {
  // We expect a raw buffer to validate the signature
  parseRawBody: true,
});

// Define the sequence of steps
orderProcessingWorkflow
  .step('validate-stripe-signature', async (ctx) => {
    const signature = ctx.trigger.headers['stripe-signature'];
    if (!signature) {
      throw new Error('Missing Stripe signature.');
    }

    // Use the service action to validate the webhook securely
    const event = ctx.services.stripe.webhooks.constructEvent(
      ctx.trigger.rawBody!,
      signature
    );

    // Only proceed if it's the event we care about
    if (event.type !== 'checkout.session.completed') {
      return ctx.cancel({ reason: `Ignoring event type: ${event.type}` });
    }

    return event.data.object; // Return the checkout session object
  })

  .step('fetch-order-and-user', async (ctx) => {
    // ctx.last is a handy alias for the output of the previous step
    const checkoutSession = ctx.last;
    const orderId = checkoutSession.metadata?.orderId;

    if (!orderId) throw new Error('Missing orderId in webhook metadata.');

    const order = await db.order.findUnique({
      where: { id: orderId },
      include: { user: true },
    });

    if (!order) throw new Error(`Order ${orderId} not found in database.`);
    return order; // The full order object with user data
  })

  .if('is-high-value-order', (ctx) => ctx.last.totalAmount > 500)
  .parallel([
    // Run these two actions concurrently for high-value orders
    async (ctx) => ctx.services.stripe.customers.addTag({
      customerId: ctx.last.user.stripeCustomerId,
      tag: 'vip-customer',
    }),
    async (ctx) => ctx.services.slack.sendMessage({
      channel: '#vip-orders',
      text: `💎 New VIP Order! Amount: $${ctx.last.totalAmount} from ${ctx.last.user.email}`,
    }),
  ])
  .endIf()

  .parallel([
    // After handling the conditional logic, run these two critical tasks concurrently
    async (ctx) => ctx.services.jira.createIssue({
      project: 'FULFILL',
      title: `Fulfill Order #${ctx.steps['fetch-order-and-user'].output.id}`,
      description: `Customer: ${ctx.steps['fetch-order-and-user'].output.user.email}`,
    }),
    async (ctx) => ctx.services.resend.send({
      to: ctx.steps['fetch-order-and-user'].output.user.email,
      subject: 'Your order is confirmed!',
      html: `<h1>Thank you for your order!</h1><p>Order ID: ${ctx.steps['fetch-order-and-user'].output.id}</p>`,
    }),
  ])

  .action('log-completion', (ctx) => {
    // The output of .parallel() is an array of the results
    const [jiraResult, resendResult] = ctx.last;
    console.log(`✅ Workflow completed. JIRA issue ${jiraResult.key} created. Email sent with ID ${resendResult.id}.`);
  });
```

---

## Complete Order Processing Example

Here's a complete example showing how to set up and run the order processing workflow:

**examples/order-processing.ts**
```typescript
import { cronflow } from 'node-cronflow';
import { orderProcessingWorkflow } from '../workflows/order-processing/workflow';

async function main() {
  // Start the cronflow engine
  await cronflow.start();
  console.log('🚀 Cronflow engine started successfully');

  // The workflow is now listening for webhooks at /webhooks/stripe
  console.log('📡 Webhook endpoint ready: http://localhost:3000/webhooks/stripe');

  // You can also manually trigger the workflow for testing
  await cronflow.trigger('v1-order-processing', {
    type: 'checkout.session.completed',
    data: {
      object: {
        id: 'cs_test_123',
        metadata: { orderId: 'ord_123' },
        amount_total: 60000, // $600
      },
    },
  });

  console.log('✅ Manual trigger sent successfully');
}

main().catch(console.error);
```

---

## Testing Workflows

Write comprehensive tests for your workflows:

**tests/order-processing.test.ts**
```typescript
import { describe, it, expect } from 'vitest';
import { orderProcessingWorkflow } from '../workflows/order-processing/workflow';

describe('Order Processing Workflow', () => {
  it('should process a high-value order', async () => {
    const testRun = await orderProcessingWorkflow
      .test()
      .trigger({
        type: 'checkout.session.completed',
        data: {
          object: {
            id: 'cs_test_123',
            metadata: { orderId: 'ord_123' },
            amount_total: 60000, // $600
          },
        },
      })
      .mockStep('fetch-order-and-user', async (ctx) => ({
        id: 'ord_123',
        totalAmount: 600,
        user: {
          email: 'vip@example.com',
          stripeCustomerId: 'cus_123',
        },
      }))
      .expectStep('validate-stripe-signature')
      .toSucceed()
      .expectStep('fetch-order-and-user')
      .toSucceed()
      .expectStep('is-high-value-order')
      .toSucceed()
      .run();

    expect(testRun.status).toBe('completed');
    expect(testRun.steps['log-completion']).toBeDefined();
  });

  it('should handle webhook validation errors', async () => {
    const testRun = await orderProcessingWorkflow
      .test()
      .trigger({
        type: 'checkout.session.completed',
        data: {
          object: {
            id: 'cs_test_123',
            // Missing metadata.orderId
          },
        },
      })
      .expectStep('validate-stripe-signature')
      .toSucceed()
      .expectStep('fetch-order-and-user')
      .toFailWith('Missing orderId in webhook metadata')
      .run();

    expect(testRun.status).toBe('failed');
  });
});
```

---

## Key Features Demonstrated

### 1. **Service Integration**
- Pre-configured services with type-safe actions
- Dependency injection via `ctx.services`
- Secure webhook validation

### 2. **Conditional Logic**
- `.if()` blocks for conditional execution
- `.parallel()` for concurrent operations
- Proper step dependency management

### 3. **Error Handling**
- Comprehensive error handling with hooks
- Automatic retry configuration
- Graceful failure notifications

### 4. **Testing**
- In-memory workflow testing
- Step mocking and expectations
- End-to-end test scenarios

### 5. **Context Object**
- Rich context with `ctx.payload`, `ctx.steps`, `ctx.services`
- State management with `ctx.state`
- Trigger information with `ctx.trigger`

This example demonstrates the full power of node-cronflow for building production-ready workflow automation with excellent developer experience and reliability.
