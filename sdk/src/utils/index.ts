// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/**
 * Convert interval string to cron expression
 * @param interval - The interval string (e.g., "5m", "1h", "1d")
 * @returns The corresponding cron expression
 * @throws {Error} When interval format is invalid
 */
export function intervalToCron(interval: string): string {
  const match = interval.match(/^(\d+)([mhd])$/);
  if (!match) {
    throw new Error(
      `Invalid interval format: ${interval}. Expected format like "5m", "1h", "1d"`
    );
  }

  const [, amount, unit] = match;
  const num = parseInt(amount);

  switch (unit) {
    case 'm': // minutes
      return `*/${num} * * * *`;
    case 'h': // hours
      return `0 */${num} * * *`;
    case 'd': // days
      return `0 0 */${num} * *`;
    default:
      throw new Error(`Unsupported interval unit: ${unit}`);
  }
}

/**
 * Parse duration string to milliseconds
 * @param duration - The duration string (e.g., "5s", "1m", "1h", "1d")
 * @returns The duration in milliseconds
 * @throws {Error} When duration format is invalid
 */
export function parseDuration(duration: string): number {
  const match = duration.match(/^(\d+)([smhd])$/);
  if (!match) {
    throw new Error(
      `Invalid duration format: ${duration}. Expected format like "5s", "1m", "1h", "1d"`
    );
  }

  const [, amount, unit] = match;
  const num = parseInt(amount);

  switch (unit) {
    case 's': // seconds
      return num * 1000;
    case 'm': // minutes
      return num * 60 * 1000;
    case 'h': // hours
      return num * 60 * 60 * 1000;
    case 'd': // days
      return num * 24 * 60 * 60 * 1000;
    default:
      throw new Error(`Unsupported duration unit: ${unit}`);
  }
}

/**
 * Generate a unique ID for workflows, steps, or runs
 * @param prefix - Optional prefix for the ID (default: 'id')
 * @returns A unique string ID
 */
export function generateId(prefix: string = 'id'): string {
  const timestamp = Date.now().toString(36);
  const random = Math.random().toString(36).substring(2, 8);
  return `${prefix}_${timestamp}_${random}`;
}

/**
 * Deep clone an object (for workflow definitions)
 * @param obj - The object to clone
 * @returns A deep copy of the object
 */
export function deepClone<T>(obj: T): T {
  return JSON.parse(JSON.stringify(obj));
}

/**
 * Check if a value is a valid JSON
 * @param value - The value to check
 * @returns True if the value can be serialized to JSON
 */
export function isValidJSON(value: any): boolean {
  try {
    JSON.stringify(value);
    return true;
  } catch {
    return false;
  }
}

/**
 * Format a date for display
 * @param date - The date to format
 * @returns ISO string representation of the date
 */
export function formatDate(date: Date): string {
  return date.toISOString();
}

/**
 * Get the current timestamp
 * @returns Current timestamp in milliseconds
 */
export function getCurrentTimestamp(): number {
  return Date.now();
}
