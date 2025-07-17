import { WebhookOptions } from './types';

export interface TriggerMethods {
  onWebhook(path: string, options?: WebhookOptions): this;

  onSchedule(cronExpression: string): this;

  onInterval(interval: string): this;

  onEvent(eventName: string): this;

  manual(): this;
}
