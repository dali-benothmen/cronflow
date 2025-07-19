import { parseDuration } from '../utils';

export interface StateValue {
  value: any;
  createdAt: number;
  expiresAt?: number;
  ttl?: number;
}

export interface StateOptions {
  ttl?: string | number;
  namespace?: string;
}

export class StateManager {
  private storage: Map<string, StateValue> = new Map();
  private namespace: string;
  private dbPath: string;

  constructor(namespace: string = 'default', dbPath: string = './cronflow.db') {
    this.namespace = namespace;
    this.dbPath = dbPath;
    this.loadState();
  }

  async get(key: string, defaultValue?: any): Promise<any> {
    const fullKey = this.getFullKey(key);
    const stateValue = this.storage.get(fullKey);

    if (!stateValue) {
      return defaultValue;
    }

    if (stateValue.expiresAt && Date.now() > stateValue.expiresAt) {
      this.storage.delete(fullKey);
      return defaultValue;
    }

    return stateValue.value;
  }

  async set(key: string, value: any, options?: StateOptions): Promise<void> {
    const fullKey = this.getFullKey(key);
    const now = Date.now();

    let expiresAt: number | undefined;
    let ttl: number | undefined;

    if (options?.ttl) {
      const ttlMs =
        typeof options.ttl === 'string'
          ? parseDuration(options.ttl)
          : options.ttl;
      expiresAt = now + ttlMs;
      ttl = ttlMs;
    }

    const stateValue: StateValue = {
      value,
      createdAt: now,
      expiresAt,
      ttl,
    };

    this.storage.set(fullKey, stateValue);
    await this.saveState();
  }

  async incr(key: string, amount: number = 1): Promise<number> {
    const currentValue = await this.get(key, 0);

    if (typeof currentValue !== 'number') {
      throw new Error(`Cannot increment non-numeric value for key: ${key}`);
    }

    const newValue = currentValue + amount;
    await this.set(key, newValue);

    return newValue;
  }

  async delete(key: string): Promise<boolean> {
    const fullKey = this.getFullKey(key);
    const deleted = this.storage.delete(fullKey);

    if (deleted) {
      await this.saveState();
    }

    return deleted;
  }

  async exists(key: string): Promise<boolean> {
    const fullKey = this.getFullKey(key);
    const stateValue = this.storage.get(fullKey);

    if (!stateValue) {
      return false;
    }

    if (stateValue.expiresAt && Date.now() > stateValue.expiresAt) {
      this.storage.delete(fullKey);
      return false;
    }

    return true;
  }

  async keys(pattern?: string): Promise<string[]> {
    const keys: string[] = [];
    const now = Date.now();

    for (const [fullKey, stateValue] of this.storage.entries()) {
      if (stateValue.expiresAt && now > stateValue.expiresAt) {
        this.storage.delete(fullKey);
        continue;
      }

      const key = this.getShortKey(fullKey);

      if (!pattern || key.includes(pattern)) {
        keys.push(key);
      }
    }

    return keys;
  }

  async mget(keys: string[]): Promise<Record<string, any>> {
    const result: Record<string, any> = {};

    for (const key of keys) {
      result[key] = await this.get(key);
    }

    return result;
  }

  async mset(
    values: Record<string, any>,
    options?: StateOptions
  ): Promise<void> {
    for (const [key, value] of Object.entries(values)) {
      await this.set(key, value, options);
    }
  }

  async clear(pattern?: string): Promise<number> {
    let deletedCount = 0;
    const keys = await this.keys(pattern);

    for (const key of keys) {
      if (await this.delete(key)) {
        deletedCount++;
      }
    }

    return deletedCount;
  }

  async stats(): Promise<{
    totalKeys: number;
    expiredKeys: number;
    namespace: string;
    dbPath: string;
  }> {
    const now = Date.now();
    let totalKeys = 0;
    let expiredKeys = 0;

    for (const [fullKey, stateValue] of this.storage.entries()) {
      totalKeys++;

      if (stateValue.expiresAt && now > stateValue.expiresAt) {
        expiredKeys++;
      }
    }

    return {
      totalKeys,
      expiredKeys,
      namespace: this.namespace,
      dbPath: this.dbPath,
    };
  }

  async cleanup(): Promise<number> {
    const now = Date.now();
    let cleanedCount = 0;

    for (const [fullKey, stateValue] of this.storage.entries()) {
      if (stateValue.expiresAt && now > stateValue.expiresAt) {
        this.storage.delete(fullKey);
        cleanedCount++;
      }
    }

    if (cleanedCount > 0) {
      await this.saveState();
    }

    return cleanedCount;
  }

  private getFullKey(key: string): string {
    return `${this.namespace}:${key}`;
  }

  private getShortKey(fullKey: string): string {
    return fullKey.replace(`${this.namespace}:`, '');
  }

  private async loadState(): Promise<void> {
    try {
      // TODO: Implement actual database loading
      // For now, use in-memory storage
      console.log(
        `📦 State manager initialized for namespace: ${this.namespace}`
      );
    } catch (error) {
      console.warn(
        `⚠️ Failed to load state for namespace ${this.namespace}:`,
        error
      );
    }
  }

  private async saveState(): Promise<void> {
    try {
      // TODO: Implement actual database saving
      // For now, use in-memory storage
      const stats = await this.stats();
      console.log(
        `💾 State saved: ${stats.totalKeys} keys in namespace ${this.namespace}`
      );
    } catch (error) {
      console.warn(
        `⚠️ Failed to save state for namespace ${this.namespace}:`,
        error
      );
    }
  }
}

export function createStateManager(
  namespace?: string,
  dbPath?: string
): StateManager {
  return new StateManager(namespace, dbPath);
}
