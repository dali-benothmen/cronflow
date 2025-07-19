import { z } from 'zod';

export interface ConfiguredService {
  id: string;
  name: string;
  version: string;
  config: any;
  auth: any;
  actions: Record<string, (...args: any[]) => any>;
  triggers?: Record<string, (...args: any[]) => any>;
}

export interface WorkflowDefinition {
  id: string;
  name?: string;
  description?: string;
  tags?: string[];
  services?: ConfiguredService[];
  hooks?: {
    onSuccess?: (ctx: Context) => void;
    onFailure?: (ctx: Context) => void;
  };
  timeout?: string | number;
  concurrency?: number;
  rateLimit?: {
    count: number;
    per: string;
  };
  queue?: string;
  version?: string;
  secrets?: object;
  steps: StepDefinition[];
  triggers: TriggerDefinition[];
  created_at: Date;
  updated_at: Date;
}

export interface StepDefinition {
  id: string;
  name: string;
  handler: (ctx: Context) => any | Promise<any>;
  type: 'step' | 'action';
  options?: StepOptions;
}

export interface StepOptions {
  timeout?: string | number;
  retry?: RetryConfig;
  cache?: CacheConfig;
  delay?: string | number;
  // Control flow options
  parallel?: boolean;
  race?: boolean;
  loop?: boolean;
  maxIterations?: number;
  controlFlow?: boolean;
  endIf?: boolean;
  conditional?: boolean;
  conditionType?: 'if' | 'elseIf' | 'else';
  stepCount?: number;
}

export interface RetryConfig {
  attempts: number;
  backoff: {
    strategy: 'exponential' | 'fixed';
    delay: string | number;
  };
}

export interface CacheConfig {
  key: (ctx: Context) => string;
  ttl: string;
}

export type TriggerDefinition =
  | { type: 'webhook'; path: string; options?: WebhookOptions }
  | { type: 'schedule'; cron_expression: string }
  | { type: 'manual' };

export interface WebhookOptions {
  method?: 'POST' | 'GET' | 'PUT' | 'DELETE';
  schema?: z.ZodObject<any>;
  idempotencyKey?: (ctx: Context) => string;
  parseRawBody?: boolean;
}

// Context object that gets passed to step handlers
export interface Context {
  payload: any;
  steps: Record<string, { output: any }>;
  services: Record<string, ConfiguredService>;
  run: {
    id: string;
    workflowId: string;
  };
  state: {
    get: (key: string, defaultValue?: any) => any;
    set: (key: string, value: any, options?: { ttl?: string }) => Promise<void>;
    incr: (key: string, amount?: number) => Promise<number>;
  };
  last: any; // Output from the previous step
  trigger: {
    headers: Record<string, string>;
    rawBody?: Buffer;
  };
  cancel: (reason?: string) => never;
  error?: Error;
}
