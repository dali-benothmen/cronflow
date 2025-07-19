import { StateManager, StateOptions } from './manager';

export interface StateWrapper {
  get: (key: string, defaultValue?: any) => any;
  set: (key: string, value: any, options?: { ttl?: string }) => Promise<void>;
  incr: (key: string, amount?: number) => Promise<number>;
  delete: (key: string) => Promise<boolean>;
  exists: (key: string) => Promise<boolean>;
  keys: (pattern?: string) => Promise<string[]>;
  mget: (keys: string[]) => Promise<Record<string, any>>;
  mset: (values: Record<string, any>, options?: StateOptions) => Promise<void>;
  clear: (pattern?: string) => Promise<number>;
  stats: () => Promise<{
    totalKeys: number;
    expiredKeys: number;
    namespace: string;
    dbPath: string;
  }>;
  cleanup: () => Promise<number>;
}

export class ContextStateWrapper implements StateWrapper {
  private stateManager: StateManager;

  constructor(workflowId: string, runId?: string, dbPath?: string) {
    const namespace = runId ? `${workflowId}:${runId}` : workflowId;
    this.stateManager = new StateManager(namespace, dbPath);
  }

  async get(key: string, defaultValue?: any): Promise<any> {
    return await this.stateManager.get(key, defaultValue);
  }

  async set(
    key: string,
    value: any,
    options?: { ttl?: string }
  ): Promise<void> {
    const stateOptions: StateOptions = {};

    if (options?.ttl) {
      stateOptions.ttl = options.ttl;
    }

    await this.stateManager.set(key, value, stateOptions);
  }

  async incr(key: string, amount: number = 1): Promise<number> {
    return await this.stateManager.incr(key, amount);
  }

  async delete(key: string): Promise<boolean> {
    return await this.stateManager.delete(key);
  }

  async exists(key: string): Promise<boolean> {
    return await this.stateManager.exists(key);
  }

  async keys(pattern?: string): Promise<string[]> {
    return await this.stateManager.keys(pattern);
  }

  async mget(keys: string[]): Promise<Record<string, any>> {
    return await this.stateManager.mget(keys);
  }

  async mset(
    values: Record<string, any>,
    options?: StateOptions
  ): Promise<void> {
    await this.stateManager.mset(values, options);
  }

  async clear(pattern?: string): Promise<number> {
    return await this.stateManager.clear(pattern);
  }

  async stats(): Promise<{
    totalKeys: number;
    expiredKeys: number;
    namespace: string;
    dbPath: string;
  }> {
    return await this.stateManager.stats();
  }

  async cleanup(): Promise<number> {
    return await this.stateManager.cleanup();
  }
}

export function createStateWrapper(
  workflowId: string,
  runId?: string,
  dbPath?: string
): StateWrapper {
  return new ContextStateWrapper(workflowId, runId, dbPath);
}
