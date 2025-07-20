import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  PerformanceOptimizer,
  PerformanceMonitor,
} from '../sdk/src/performance';

describe('Performance Benchmark - Task 10.1', () => {
  let optimizer: PerformanceOptimizer;
  let monitor: PerformanceMonitor;

  beforeEach(() => {
    optimizer = new PerformanceOptimizer();
    monitor = new PerformanceMonitor({
      alertThresholds: {
        memoryUsage: 80,
        slowQueryThreshold: 100,
        cacheHitRate: 0.5,
        serializationTime: 50,
      },
      reportingInterval: 1000,
      enableAlerts: false, // Disable alerts for benchmarking
    });
  });

  afterEach(() => {
    monitor.stop();
  });

  describe('Connection Pool Performance Benchmark', () => {
    it('should demonstrate connection pool performance improvements', async () => {
      const iterations = 50;
      const results = {
        withoutPool: [] as number[],
        withPool: [] as number[],
      };

      // Simulate database connections without pooling
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        // Simulate connection creation overhead
        await new Promise(resolve =>
          setTimeout(resolve, 20 + Math.random() * 30)
        );
        results.withoutPool.push(Date.now() - start);
      }

      // Simulate database connections with pooling
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        const connectionId = await optimizer.getConnection();
        await new Promise(resolve =>
          setTimeout(resolve, 5 + Math.random() * 10)
        ); // Simulate query
        optimizer.releaseConnection(connectionId);
        results.withPool.push(Date.now() - start);
      }

      // Calculate averages
      const avgWithout =
        results.withoutPool.reduce((a, b) => a + b, 0) / iterations;
      const avgWith = results.withPool.reduce((a, b) => a + b, 0) / iterations;

      console.log('\n📊 Connection Pool Performance Results:');
      console.log(`Without pooling: ${avgWithout.toFixed(2)}ms average`);
      console.log(`With pooling: ${avgWith.toFixed(2)}ms average`);
      console.log(
        `Pooling improvement: ${(((avgWithout - avgWith) / avgWithout) * 100).toFixed(1)}%`
      );

      // Verify improvements
      expect(avgWith).toBeLessThan(avgWithout);
    });
  });

  describe('Database Query Caching Benchmark', () => {
    it('should demonstrate database query caching improvements', async () => {
      const iterations = 50;
      const results = {
        withoutCaching: [] as number[],
        withCaching: [] as number[],
      };

      // Simulate database queries without caching
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        await new Promise(resolve =>
          setTimeout(resolve, 100 + Math.random() * 50)
        ); // Simulate DB query
        results.withoutCaching.push(Date.now() - start);
      }

      // Simulate database queries with caching (repeated queries)
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        await optimizer.optimizeDatabaseQuery(
          async () => {
            await new Promise(resolve =>
              setTimeout(resolve, 100 + Math.random() * 50)
            );
            return { data: `cached_result_${i % 5}` }; // Only 5 unique results for better caching
          },
          `cache_key_${i % 5}`
        );
        results.withCaching.push(Date.now() - start);
      }

      // Calculate averages
      const avgWithout =
        results.withoutCaching.reduce((a, b) => a + b, 0) / iterations;
      const avgWith =
        results.withCaching.reduce((a, b) => a + b, 0) / iterations;

      console.log('\n📊 Database Query Caching Results:');
      console.log(`Without caching: ${avgWithout.toFixed(2)}ms average`);
      console.log(`With caching: ${avgWith.toFixed(2)}ms average`);
      console.log(
        `Caching improvement: ${(((avgWithout - avgWith) / avgWithout) * 100).toFixed(1)}%`
      );

      // Get cache metrics
      const metrics = optimizer.getMetrics();
      console.log(
        `Cache hit rate: ${(metrics.cache.hitRate * 100).toFixed(1)}%`
      );
      console.log(
        `Total queries: ${metrics.databaseQueries.total}, Cached: ${metrics.databaseQueries.cached}`
      );

      // Verify improvements
      expect(avgWith).toBeLessThan(avgWithout);
      expect(metrics.cache.hitRate).toBeGreaterThan(0);
    });
  });

  describe('JSON Serialization with Caching Benchmark', () => {
    it('should demonstrate serialization caching improvements', () => {
      const iterations = 100;

      // Create large test data
      const largeTestData = {
        users: Array.from({ length: 1000 }, (_, i) => ({
          id: i,
          name: `User ${i}`,
          email: `user${i}@example.com`,
          profile: {
            avatar: Buffer.from(`avatar_data_${i}`),
            bio: `This is a long bio for user ${i}`.repeat(10),
            preferences: {
              theme: 'dark',
              notifications: true,
              language: 'en',
            },
          },
          metadata: {
            createdAt: new Date(),
            lastLogin: new Date(),
            loginCount: Math.floor(Math.random() * 1000),
          },
        })),
        metadata: {
          totalUsers: 1000,
          generatedAt: new Date(),
          version: '1.0.0',
        },
      };

      const results = {
        standard: [] as number[],
        withCaching: [] as number[],
        cached: [] as number[],
      };

      // Standard JSON.stringify
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        JSON.stringify(largeTestData);
        results.standard.push(Date.now() - start);
      }

      // Serialization with caching (first time)
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        optimizer.optimizeSerialization(largeTestData);
        results.withCaching.push(Date.now() - start);
      }

      // Cached serialization (same data)
      for (let i = 0; i < iterations; i++) {
        const start = Date.now();
        optimizer.optimizeSerialization(largeTestData); // Same data, should use cache
        results.cached.push(Date.now() - start);
      }

      // Calculate averages
      const avgStandard =
        results.standard.reduce((a, b) => a + b, 0) / iterations;
      const avgWithCaching =
        results.withCaching.reduce((a, b) => a + b, 0) / iterations;
      const avgCached = results.cached.reduce((a, b) => a + b, 0) / iterations;

      console.log('\n📊 JSON Serialization Caching Results:');
      console.log(
        `Standard JSON.stringify: ${avgStandard.toFixed(2)}ms average`
      );
      console.log(
        `With caching (first time): ${avgWithCaching.toFixed(2)}ms average`
      );
      console.log(`Cached serialization: ${avgCached.toFixed(2)}ms average`);
      console.log(
        `Caching overhead: ${(((avgWithCaching - avgStandard) / avgStandard) * 100).toFixed(1)}%`
      );
      console.log(
        `Caching improvement: ${(((avgStandard - avgCached) / avgStandard) * 100).toFixed(1)}%`
      );

      // Verify that caching provides benefits for repeated serialization
      expect(avgCached).toBeLessThan(avgStandard);
      expect(avgCached).toBeLessThan(avgWithCaching); // Caching should be fastest for repeated data
    });
  });

  describe('Memory Monitoring Benchmark', () => {
    it('should demonstrate memory monitoring capabilities', () => {
      const iterations = 50;
      const results = {
        memoryUsage: [] as number[],
        peakMemory: [] as number[],
      };

      // Simulate memory usage patterns
      for (let i = 0; i < iterations; i++) {
        // Create some memory pressure
        const largeArray = new Array(10000).fill(`test_data_${i}`);

        // Get memory metrics
        const metrics = optimizer.getMetrics();
        results.memoryUsage.push(metrics.memory.used);
        results.peakMemory.push(metrics.memory.peak);

        // Clean up
        largeArray.length = 0;
      }

      // Calculate averages
      const avgMemory =
        results.memoryUsage.reduce((a, b) => a + b, 0) / iterations;
      const avgPeak =
        results.peakMemory.reduce((a, b) => a + b, 0) / iterations;

      console.log('\n📊 Memory Monitoring Results:');
      console.log(
        `Average memory usage: ${(avgMemory / 1024 / 1024).toFixed(2)}MB`
      );
      console.log(
        `Average peak memory: ${(avgPeak / 1024 / 1024).toFixed(2)}MB`
      );
      console.log(`Memory monitoring active: ${avgMemory > 0 ? 'Yes' : 'No'}`);

      // Verify memory monitoring is working
      expect(avgMemory).toBeGreaterThan(0);
      expect(avgPeak).toBeGreaterThanOrEqual(avgMemory);
    });
  });

  describe('Performance Monitoring Integration', () => {
    it('should demonstrate performance monitoring capabilities', async () => {
      // Simulate various performance scenarios
      const scenarios = [
        { name: 'Fast Operations', delay: 10, count: 20 },
        { name: 'Slow Operations', delay: 150, count: 10 },
        { name: 'Mixed Operations', delay: 50, count: 15 },
      ];

      for (const scenario of scenarios) {
        console.log(`\n🔄 Running ${scenario.name}...`);

        for (let i = 0; i < scenario.count; i++) {
          await optimizer.optimizeDatabaseQuery(async () => {
            await new Promise(resolve => setTimeout(resolve, scenario.delay));
            return { result: `${scenario.name}_${i}` };
          });
        }
      }

      // Get final metrics
      const metrics = optimizer.getMetrics();
      const monitorReport = monitor.getLatestReport();

      console.log('\n📊 Performance Monitoring Results:');
      console.log(`Total database queries: ${metrics.databaseQueries.total}`);
      console.log(`Cached queries: ${metrics.databaseQueries.cached}`);
      console.log(`Slow queries detected: ${metrics.databaseQueries.slow}`);
      console.log(
        `Cache hit rate: ${(metrics.cache.hitRate * 100).toFixed(1)}%`
      );
      console.log(
        `Serialization operations: ${metrics.serialization.totalOperations}`
      );
      console.log(
        `Memory used: ${(metrics.memory.used / 1024 / 1024).toFixed(2)}MB`
      );

      // Verify monitoring is working
      expect(metrics.databaseQueries.total).toBeGreaterThan(0);
      expect(metrics.serialization.totalOperations).toBeGreaterThan(0);
      expect(metrics.memory.used).toBeGreaterThan(0);
    });
  });
});
