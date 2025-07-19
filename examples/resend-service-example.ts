// ============================================================================
// EXAMPLE: RESEND EMAIL SERVICE
// ============================================================================

import { defineService } from '../services/src/index';
import { z } from 'zod';

export const resendServiceTemplate = defineService({
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
  setup: ({ config, auth, engine }) => {
    const mockResendClient = {
      emails: {
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

          await new Promise(resolve => setTimeout(resolve, 100));

          return {
            id: `email_${Date.now()}`,
            to: params.to,
            subject: params.subject,
            status: 'sent',
          };
        },
      },
    };

    return {
      actions: {
        send: async (params: {
          to: string;
          subject: string;
          html: string;
          from?: string;
        }) => {
          return await mockResendClient.emails.send({
            ...params,
            from: params.from || config.defaultFrom,
          });
        },

        sendTemplate: async (params: {
          to: string;
          templateId: string;
          variables: Record<string, any>;
          from?: string;
        }) => {
          console.log('📧 Sending template email via Resend:', {
            to: params.to,
            templateId: params.templateId,
            variables: params.variables,
            from: params.from || config.defaultFrom,
          });

          await new Promise(resolve => setTimeout(resolve, 150));

          return {
            id: `template_${Date.now()}`,
            to: params.to,
            templateId: params.templateId,
            status: 'sent',
          };
        },
      },
      triggers: {
        onEmailSent: async (emailId: string) => {
          console.log(`📧 Email sent trigger: ${emailId}`);
          return { emailId, status: 'triggered' };
        },
      },
    };
  },
});

// Example usage with withConfig method
export const resendService = resendServiceTemplate.withConfig({
  auth: {
    apiKey: process.env.RESEND_API_KEY || 'mock-api-key-for-example',
  },
  config: {
    defaultFrom: 'noreply@mycompany.com',
    defaultReplyTo: 'support@mycompany.com',
  },
});
