import type { ServiceDefinition, ConfiguredService } from './types';

export function createServiceInstance<TConfig = any, TAuth = any>(
  serviceDef: ServiceDefinition<TConfig, TAuth>,
  config: TConfig,
  auth: TAuth
): ConfiguredService<TConfig, TAuth> {
  if (serviceDef.schema?.config) {
    try {
      serviceDef.schema.config.parse(config);
    } catch (error) {
      throw new Error(
        `Invalid service config for ${serviceDef.name}: ${error}`
      );
    }
  }

  if (serviceDef.schema?.auth) {
    try {
      serviceDef.schema.auth.parse(auth);
    } catch (error) {
      throw new Error(`Invalid service auth for ${serviceDef.name}: ${error}`);
    }
  }

  const mockEngine = {
    registerTrigger: (
      name: string,
      handler: (...args: any[]) => Promise<any>
    ) => {
      console.log(`Registering trigger: ${name}`);
    },
    unregisterTrigger: (name: string) => {
      console.log(`Unregistering trigger: ${name}`);
    },
  };

  const setupResult = serviceDef.setup({
    config,
    auth,
    engine: mockEngine,
  });

  return {
    id: serviceDef.id,
    name: serviceDef.name,
    version: serviceDef.version,
    config,
    auth,
    actions: setupResult.actions,
    triggers: setupResult.triggers,
  };
}
