import { describe, it, expect } from 'vitest';
import { version } from './index';

describe('SDK', () => {
  it('should export version', () => {
    expect(version).toBe('1.0.0');
  });

  it('should be a string', () => {
    expect(typeof version).toBe('string');
  });
});
