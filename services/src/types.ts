import { z } from 'zod';

export interface ServiceSchema {
  auth?: z.ZodSchema;
  config?: z.ZodSchema;
}

export interface ServiceActions {
  [key: string]: (...args: any[]) => Promise<any> | any;
}

export interface ServiceTriggers {
  [key: string]: (...args: any[]) => Promise<any> | any;
}

export interface ServiceSetupParams<TConfig = any, TAuth = any> {
  config: TConfig;
  auth: TAuth;
  engine: {
    registerTrigger: (
      name: string,
      handler: (...args: any[]) => Promise<any>
    ) => void;
    unregisterTrigger: (name: string) => void;
  };
}

export interface ServiceSetupResult {
  actions: ServiceActions;
  triggers?: ServiceTriggers;
}

export interface ServiceDefinition<TConfig = any, TAuth = any> {
  id: string;
  name: string;
  description: string;
  version: string;
  schema?: ServiceSchema;
  setup: (params: ServiceSetupParams<TConfig, TAuth>) => ServiceSetupResult;
}

export interface ConfiguredService<TConfig = any, TAuth = any> {
  id: string;
  name: string;
  version: string;
  config: TConfig;
  auth: TAuth;
  actions: ServiceActions;
  triggers?: ServiceTriggers;
}
