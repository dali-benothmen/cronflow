import type { ServiceDefinition } from './types';
import { validateServiceDefinition } from './validation';

export class ServiceRegistry {
  private services = new Map<string, ServiceDefinition>();

  register<TConfig = any, TAuth = any>(
    serviceDef: ServiceDefinition<TConfig, TAuth>
  ): void {
    const validation = validateServiceDefinition(serviceDef);
    if (!validation.valid) {
      throw new Error(
        `Invalid service definition: ${validation.errors.join(', ')}`
      );
    }

    this.services.set(serviceDef.id, serviceDef);
  }

  get(id: string): ServiceDefinition | undefined {
    return this.services.get(id);
  }

  list(): string[] {
    return Array.from(this.services.keys());
  }

  has(id: string): boolean {
    return this.services.has(id);
  }

  remove(id: string): boolean {
    return this.services.delete(id);
  }

  clear(): void {
    this.services.clear();
  }
}

export const serviceRegistry = new ServiceRegistry();

export function registerService<TConfig = any, TAuth = any>(
  serviceDef: ServiceDefinition<TConfig, TAuth>
): void {
  serviceRegistry.register(serviceDef);
}

export function getServiceDefinition(
  id: string
): ServiceDefinition | undefined {
  return serviceRegistry.get(id);
}

export function listServiceDefinitions(): string[] {
  return serviceRegistry.list();
}

export function hasServiceDefinition(id: string): boolean {
  return serviceRegistry.has(id);
}
