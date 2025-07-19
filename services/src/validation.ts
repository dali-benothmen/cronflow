import { z } from 'zod';
import type { ServiceDefinition } from './types';

export function validateServiceDefinition<TConfig = any, TAuth = any>(
  serviceDef: ServiceDefinition<TConfig, TAuth>
): { valid: boolean; errors: string[] } {
  const errors: string[] = [];

  if (!serviceDef.id || serviceDef.id.trim() === '') {
    errors.push('Service ID is required');
  }

  if (!serviceDef.name || serviceDef.name.trim() === '') {
    errors.push('Service name is required');
  }

  if (!serviceDef.description || serviceDef.description.trim() === '') {
    errors.push('Service description is required');
  }

  if (!serviceDef.version || serviceDef.version.trim() === '') {
    errors.push('Service version is required');
  }

  if (!serviceDef.setup || typeof serviceDef.setup !== 'function') {
    errors.push('Service setup function is required');
  }

  if (serviceDef.schema) {
    if (
      serviceDef.schema.auth &&
      !(serviceDef.schema.auth instanceof z.ZodSchema)
    ) {
      errors.push('Service auth schema must be a Zod schema');
    }
    if (
      serviceDef.schema.config &&
      !(serviceDef.schema.config instanceof z.ZodSchema)
    ) {
      errors.push('Service config schema must be a Zod schema');
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}
