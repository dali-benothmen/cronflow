/**
 * Node-Cronflow Services Package
 *
 * This package provides built-in service integrations for the Node-Cronflow workflow engine.
 * Services are factory functions that create configured service instances for use in workflows.
 */

// Placeholder types (will be imported from SDK later)
export interface WorkflowDefinition {
  id: string;
  name: string;
  steps: StepDefinition[];
  triggers: TriggerDefinition[];
}

export interface StepDefinition {
  name: string;
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  action: (_ctx: any) => any;
}

export interface TriggerDefinition {
  type: string;
  config: any;
}

// Service definition types
export interface ServiceDefinition<TConfig = any, TInstance = any> {
  name: string;
  version: string;
  configSchema: any; // Zod schema for configuration
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  createInstance: (_config: TConfig) => TInstance;
}

export interface ServiceInstance {
  name: string;
  version: string;
  config: any;
}

// Service registry
const serviceRegistry = new Map<string, ServiceDefinition>();

/**
 * Define a new service
 */
export function defineService<TConfig = any, TInstance = any>(
  name: string,
  definition: Omit<ServiceDefinition<TConfig, TInstance>, 'name'>
): ServiceDefinition<TConfig, TInstance> {
  const serviceDef: ServiceDefinition<TConfig, TInstance> = {
    name,
    ...definition,
  };

  serviceRegistry.set(name, serviceDef);
  return serviceDef;
}

/**
 * Get a service definition by name
 */
export function getServiceDefinition(
  name: string
): ServiceDefinition | undefined {
  return serviceRegistry.get(name);
}

/**
 * List all available service definitions
 */
export function listServiceDefinitions(): string[] {
  return Array.from(serviceRegistry.keys());
}

// Export the registry for internal use
export { serviceRegistry };

// Default export
export default {
  defineService,
  getServiceDefinition,
  listServiceDefinitions,
};
