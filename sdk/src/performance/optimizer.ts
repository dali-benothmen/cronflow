export interface PerformanceMetrics {
  databaseQueries: {
    total: number;
    cached: number;
    slow: number;
    averageTime: number;
  };
  cache: {
    hits: number;
    misses: number;
    hitRate: number;
    size: number;
  };
  serialization: {
    totalOperations: number;
    averageTime: number;
    compressedSize: number;
  };
  memory: {
    used: number;
    peak: number;
    garbageCollections: number;
  };
}

export interface CacheOptions {
  maxSize: number;
  ttl: string | number;
  compression?: boolean;
}

export interface DatabaseOptimizationOptions {
  connectionPoolSize: number;
  queryTimeout: number;
  enableQueryCache: boolean;
  slowQueryThreshold: number;
}

export class PerformanceOptimizer {
  private cache = new Map<
    string,
    { value: any; timestamp: number; ttl: number }
  >();
  private metrics: PerformanceMetrics = {
    databaseQueries: { total: 0, cached: 0, slow: 0, averageTime: 0 },
    cache: { hits: 0, misses: 0, hitRate: 0, size: 0 },
    serialization: { totalOperations: 0, averageTime: 0, compressedSize: 0 },
    memory: { used: 0, peak: 0, garbageCollections: 0 },
  };

  constructor(
    private cacheOptions: CacheOptions = {
      maxSize: 1000,
      ttl: '1h',
    },
    private dbOptions: DatabaseOptimizationOptions = {
      connectionPoolSize: 10,
      queryTimeout: 5000,
      enableQueryCache: true,
      slowQueryThreshold: 100,
    }
  ) {
    this.startMemoryMonitoring();
    this.startCacheCleanup();

    const memUsage = process.memoryUsage();
    this.metrics.memory.used = memUsage.heapUsed;
    this.metrics.memory.peak = memUsage.heapUsed;
  }

  async optimizeDatabaseQuery<T>(
    queryFn: () => Promise<T>,
    cacheKey?: string
  ): Promise<T> {
    const startTime = Date.now();
    this.metrics.databaseQueries.total++;

    if (cacheKey && this.dbOptions.enableQueryCache) {
      const cached = this.getFromCache(cacheKey);
      if (cached) {
        this.metrics.databaseQueries.cached++;
        return cached;
      }
    }

    try {
      const result = await queryFn();
      const duration = Date.now() - startTime;

      if (duration > this.dbOptions.slowQueryThreshold) {
        this.metrics.databaseQueries.slow++;
        console.warn(`⚠️  Slow database query detected: ${duration}ms`);
      }

      const currentAvg = this.metrics.databaseQueries.averageTime;
      const totalQueries = this.metrics.databaseQueries.total;
      this.metrics.databaseQueries.averageTime =
        (currentAvg * (totalQueries - 1) + duration) / totalQueries;

      if (cacheKey) {
        this.setCache(cacheKey, result);
      }

      return result;
    } catch (error) {
      console.error('❌ Database query failed:', error);
      throw error;
    }
  }

  private connectionPool: Array<{
    id: string;
    inUse: boolean;
    lastUsed: number;
  }> = [];

  async getConnection(): Promise<string> {
    const availableConnection = this.connectionPool.find(conn => !conn.inUse);

    if (availableConnection) {
      availableConnection.inUse = true;
      availableConnection.lastUsed = Date.now();
      return availableConnection.id;
    }

    if (this.connectionPool.length < this.dbOptions.connectionPoolSize) {
      const connectionId = `conn_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
      this.connectionPool.push({
        id: connectionId,
        inUse: true,
        lastUsed: Date.now(),
      });
      return connectionId;
    }

    return new Promise(resolve => {
      const checkForConnection = () => {
        const available = this.connectionPool.find(conn => !conn.inUse);
        if (available) {
          available.inUse = true;
          available.lastUsed = Date.now();
          resolve(available.id);
        } else {
          setTimeout(checkForConnection, 10);
        }
      };
      checkForConnection();
    });
  }

  releaseConnection(connectionId: string): void {
    const connection = this.connectionPool.find(
      conn => conn.id === connectionId
    );
    if (connection) {
      connection.inUse = false;
    }
  }

  private setCache(key: string, value: any): void {
    if (this.cache.size >= this.cacheOptions.maxSize) {
      this.evictOldestCacheEntry();
    }

    const ttl = this.parseDuration(this.cacheOptions.ttl);
    this.cache.set(key, {
      value,
      timestamp: Date.now(),
      ttl,
    });

    this.updateCacheMetrics();
  }

  private getFromCache(key: string): any | null {
    const entry = this.cache.get(key);

    if (!entry) {
      this.metrics.cache.misses++;
      this.updateCacheMetrics();
      return null;
    }

    if (Date.now() - entry.timestamp > entry.ttl) {
      this.cache.delete(key);
      this.metrics.cache.misses++;
      this.updateCacheMetrics();
      return null;
    }

    this.metrics.cache.hits++;
    this.updateCacheMetrics();
    return entry.value;
  }

  private evictOldestCacheEntry(): void {
    let oldestKey: string | null = null;
    let oldestTime = Date.now();

    for (const [key, entry] of this.cache.entries()) {
      if (entry.timestamp < oldestTime) {
        oldestTime = entry.timestamp;
        oldestKey = key;
      }
    }

    if (oldestKey) {
      this.cache.delete(oldestKey);
    }
  }

  private updateCacheMetrics(): void {
    const total = this.metrics.cache.hits + this.metrics.cache.misses;
    this.metrics.cache.hitRate =
      total > 0 ? this.metrics.cache.hits / total : 0;
    this.metrics.cache.size = this.cache.size;
  }

  private serializationCache = new Map<string, string>();

  optimizeSerialization<T>(data: T): string {
    const startTime = Date.now();
    this.metrics.serialization.totalOperations++;

    const dataHash = this.hashData(data);
    const cached = this.serializationCache.get(dataHash);

    if (cached) {
      this.metrics.serialization.averageTime =
        (this.metrics.serialization.averageTime *
          (this.metrics.serialization.totalOperations - 1) +
          0) /
        this.metrics.serialization.totalOperations;
      return cached;
    }

    const serialized = JSON.stringify(data);
    this.serializationCache.set(dataHash, serialized);

    const duration = Date.now() - startTime;
    this.metrics.serialization.averageTime =
      (this.metrics.serialization.averageTime *
        (this.metrics.serialization.totalOperations - 1) +
        duration) /
      this.metrics.serialization.totalOperations;

    return serialized;
  }

  private hashData(data: any): string {
    const str = JSON.stringify(data);
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      const char = str.charCodeAt(i);
      hash = (hash << 5) - hash + char;
      hash = hash & hash; // Convert to 32-bit integer
    }
    return hash.toString();
  }

  private startMemoryMonitoring(): void {
    setInterval(() => {
      const memUsage = process.memoryUsage();
      this.metrics.memory.used = memUsage.heapUsed;
      this.metrics.memory.peak = Math.max(
        this.metrics.memory.peak,
        memUsage.heapUsed
      );

      if (memUsage.heapUsed > 100 * 1024 * 1024) {
        // 100MB
        if (global.gc) {
          global.gc();
          this.metrics.memory.garbageCollections++;
        }
      }
    }, 5000);
  }

  private startCacheCleanup(): void {
    setInterval(() => {
      const now = Date.now();
      for (const [key, entry] of this.cache.entries()) {
        if (now - entry.timestamp > entry.ttl) {
          this.cache.delete(key);
        }
      }
      this.updateCacheMetrics();
    }, 60000);
  }

  private parseDuration(duration: string | number): number {
    if (typeof duration === 'number') {
      return duration;
    }

    const match = duration.match(/^(\d+)(ms|s|m|h|d)$/);
    if (!match) {
      throw new Error(`Invalid duration format: ${duration}`);
    }

    const value = parseInt(match[1], 10);
    const unit = match[2];

    switch (unit) {
      case 'ms':
        return value;
      case 's':
        return value * 1000;
      case 'm':
        return value * 60 * 1000;
      case 'h':
        return value * 60 * 60 * 1000;
      case 'd':
        return value * 24 * 60 * 60 * 1000;
      default:
        throw new Error(`Unknown duration unit: ${unit}`);
    }
  }

  getMetrics(): PerformanceMetrics {
    return { ...this.metrics };
  }

  resetMetrics(): void {
    this.metrics = {
      databaseQueries: { total: 0, cached: 0, slow: 0, averageTime: 0 },
      cache: { hits: 0, misses: 0, hitRate: 0, size: 0 },
      serialization: { totalOperations: 0, averageTime: 0, compressedSize: 0 },
      memory: { used: 0, peak: 0, garbageCollections: 0 },
    };
  }

  getPerformanceRecommendations(): string[] {
    const recommendations: string[] = [];

    if (this.metrics.databaseQueries.slow > 0) {
      recommendations.push('Consider adding database indexes for slow queries');
    }

    if (this.metrics.cache.hitRate < 0.5) {
      recommendations.push(
        'Cache hit rate is low - consider increasing cache size or TTL'
      );
    }

    if (this.metrics.memory.used > 50 * 1024 * 1024) {
      // 50MB
      recommendations.push(
        'Memory usage is high - consider optimizing data structures'
      );
    }

    if (this.metrics.serialization.averageTime > 10) {
      recommendations.push(
        'Serialization is slow - consider using more efficient data formats'
      );
    }

    return recommendations;
  }

  destroy(): void {
    this.cache.clear();
    this.serializationCache.clear();
    this.connectionPool = [];
  }
}
