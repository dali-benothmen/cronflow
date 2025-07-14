import { defineService } from 'node-cronflow';
import Stripe from 'stripe';
import { z } from 'zod';

export const stripeServiceTemplate = defineService({
id: 'stripe',
name: 'Stripe',
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
webhooks: {
// A special action to securely validate the webhook signature
constructEvent: (payload: Buffer, signature: string) => {
return stripe.webhooks.constructEvent(payload, signature, auth.webhookSecret);
},
},
},
};
},
});
Use code with caution.
TypeScript
(You would create similar simple service definitions for slack.ts, jira.ts, and resend.ts)
Step 2: Create Configured Service Instances
This file configures the service templates with actual secrets for our specific workflow.
src/workflows/order-processing/services.ts
Generated typescript
import { stripeServiceTemplate } from '../../services/stripe';
// ... import other service templates

// Pre-configure all the services this workflow will need
export const stripeService = stripeServiceTemplate.withConfig({
auth: {
apiKey: process.env.STRIPE_API_KEY!,
webhookSecret: process.env.STRIPE_WEBHOOK_SECRET!,
},
});

export const slackService = /_ ... .withConfig({ ... }) _/;
export const jiraService = /_ ... .withConfig({ ... }) _/;
export const resendService = /_ ... .withConfig({ ... }) _/;
Use code with caution.
TypeScript
Step 3: Define the Main Workflow Logic
This is the heart of the example, bringing everything together.
src/workflows/order-processing/workflow.ts
Generated typescript
import { cronflow } from 'node-cronflow';
import { z } from 'zod';
import { db } from '../../lib/db'; // Your Prisma/Drizzle/etc. client
import { stripeService, slackService, jiraService, resendService } from './services';

// Define the workflow, its metadata, and its dependencies (services)
export const orderProcessingWorkflow = cronflow.define({
id: 'v1-order-processing',
name: 'Order Fulfillment Workflow',
tags: ['ecommerce', 'critical'],
services: [stripeService, slackService, jiraService, resendService],
concurrency: 20, // Handle up to 20 concurrent new orders
hooks: {
onFailure: (ctx) => {
console.error(`[Workflow Failed] Run ID: ${ctx.run.id}`, ctx.error);
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
.step('validate-stripe-signature', (ctx) => {
const signature = ctx.trigger.headers['stripe-signature'];
if (!signature) {
throw new Error('Missing Stripe signature.');
}
// Use the service action to validate the webhook securely
const event = ctx.services.stripe.webhooks.constructEvent(ctx.trigger.rawBody, signature);

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
(ctx) => ctx.services.stripe.customers.addTag({
customerId: ctx.last.user.stripeCustomerId,
tag: 'vip-customer',
}),
(ctx) => ctx.services.slack.sendMessage({
channel: '#vip-orders',
text: `💎 New VIP Order! Amount: $${ctx.last.totalAmount} from ${ctx.last.user.email}`,
}),
])
.endIf()

.parallel([
// After handling the conditional logic, run these two critical tasks concurrently
(ctx) => ctx.services.jira.createIssue({
project: 'FULFILL',
title: `Fulfill Order #${ctx.steps['fetch-order-and-user'].output.id}`,
description: `Customer: ${ctx.steps['fetch-order-and-user'].output.user.email}`,
}),
(ctx) => ctx.services.resend.send({
to: ctx.steps['fetch-order-and-user'].output.user.email,
subject: 'Your order is confirmed!',
html: `<h1>Thank you for your order!</h1><p>Order ID: ${ctx.steps['fetch-order-and-user'].output.id}</p>`,
}),
])

.action('log-completion', (ctx) => {
// The output of .parallel() is an array of the results
const [jiraResult, resendResult] = ctx.last;
console.log(`Workflow completed. JIRA issue ${jiraResult.key} created. Email sent with ID ${resendResult.id}.`);
});
