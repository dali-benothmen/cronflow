import { z } from 'zod';
import type { ServiceDefinition } from './types';

export function defineService<TConfig = any, TAuth = any>(
  options: ServiceDefinition<TConfig, TAuth>
): ServiceDefinition<TConfig, TAuth> {
  if (!options.id || options.id.trim() === '') {
    throw new Error('Service ID cannot be empty');
  }

  if (!options.name || options.name.trim() === '') {
    throw new Error('Service name cannot be empty');
  }

  if (!options.description || options.description.trim() === '') {
    throw new Error('Service description cannot be empty');
  }

  if (!options.version || options.version.trim() === '') {
    throw new Error('Service version cannot be empty');
  }

  if (!options.setup || typeof options.setup !== 'function') {
    throw new Error('Service setup function is required');
  }

  if (options.schema) {
    if (options.schema.auth && !(options.schema.auth instanceof z.ZodSchema)) {
      throw new Error('Service auth schema must be a Zod schema');
    }
    if (
      options.schema.config &&
      !(options.schema.config instanceof z.ZodSchema)
    ) {
      throw new Error('Service config schema must be a Zod schema');
    }
  }

  return {
    id: options.id,
    name: options.name,
    description: options.description,
    version: options.version,
    schema: options.schema,
    setup: options.setup,
  };
}
