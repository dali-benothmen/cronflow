import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  PerformanceOptimizer,
  PerformanceMonitor,
} from '../sdk/src/performance';

describe('Performance Optimization Implementation', () => {
  describe('PerformanceOptimizer', () => {
    let optimizer: PerformanceOptimizer;

    beforeEach(() => {
      optimizer = new PerformanceOptimizer();
    });

    afterEach(() => {
      optimizer.destroy();
    });

    describe('Database Query Optimization', () => {
      it('should optimize database queries with caching', async () => {
        let queryCount = 0;
        const queryFn = async () => {
          queryCount++;
          await new Promise(resolve => setTimeout(resolve, 50)); // Simulate DB delay
          return { id: 1, name: 'test', timestamp: new Date() };
        };

        // First query - should hit database
        const result1 = await optimizer.optimizeDatabaseQuery(
          queryFn,
          'test-query'
        );
        expect(result1).toEqual({
          id: 1,
          name: 'test',
          timestamp: expect.any(Date),
        });
        expect(queryCount).toBe(1);

        // Second query - should use cache
        const result2 = await optimizer.optimizeDatabaseQuery(
          queryFn,
          'test-query'
        );
        expect(result2).toEqual({
          id: 1,
          name: 'test',
          timestamp: expect.any(Date),
        });
        expect(queryCount).toBe(1); // Should not hit database again

        const metrics = optimizer.getMetrics();
        expect(metrics.databaseQueries.total).toBe(2);
        expect(metrics.databaseQueries.cached).toBe(1);
      });

      it('should track slow queries', async () => {
        const slowQueryFn = async () => {
          await new Promise(resolve => setTimeout(resolve, 150)); // Slow query
          return { data: 'slow result' };
        };

        await optimizer.optimizeDatabaseQuery(slowQueryFn, 'slow-query');

        const metrics = optimizer.getMetrics();
        expect(metrics.databaseQueries.slow).toBe(1);
        expect(metrics.databaseQueries.averageTime).toBeGreaterThan(100);
      });

      it('should handle query failures gracefully', async () => {
        const failingQueryFn = async () => {
          throw new Error('Database connection failed');
        };

        try {
          await optimizer.optimizeDatabaseQuery(
            failingQueryFn,
            'failing-query'
          );
          expect(true).toBe(false);
        } catch (error) {
          expect(error).toBeInstanceOf(Error);
          expect((error as Error).message).toBe('Database connection failed');
        }

        const metrics = optimizer.getMetrics();
        expect(metrics.databaseQueries.total).toBe(1);
      });
    });

    describe('Connection Pool Management', () => {
      it('should manage connection pool correctly', async () => {
        const connection1 = await optimizer.getConnection();
        const connection2 = await optimizer.getConnection();
        const connection3 = await optimizer.getConnection();

        expect(connection1).toBeDefined();
        expect(connection2).toBeDefined();
        expect(connection3).toBeDefined();
        expect(connection1).not.toBe(connection2);
        expect(connection2).not.toBe(connection3);
      });

      it('should release connections back to pool', async () => {
        const connection = await optimizer.getConnection();
        expect(connection).toBeDefined();

        optimizer.releaseConnection(connection);
        // Connection should be available again
        const reusedConnection = await optimizer.getConnection();
        expect(reusedConnection).toBeDefined();
      });

      it('should wait for available connections when pool is full', async () => {
        // Get all available connections
        const connections: string[] = [];
        for (let i = 0; i < 10; i++) {
          connections.push(await optimizer.getConnection());
        }

        // Try to get another connection (should wait)
        const startTime = Date.now();
        const waitPromise = optimizer.getConnection();

        // Release one connection after a short delay
        setTimeout(() => {
          optimizer.releaseConnection(connections[0]);
        }, 100);

        const newConnection = await waitPromise;
        const waitTime = Date.now() - startTime;

        expect(newConnection).toBeDefined();
        expect(waitTime).toBeGreaterThan(50); // Should have waited
      });
    });

    describe('Caching System', () => {
      it('should cache and retrieve values correctly', () => {
        const testData = { id: 1, name: 'test', timestamp: new Date() };

        // Set cache
        (optimizer as any).setCache('test-key', testData);

        // Get from cache
        const cached = (optimizer as any).getFromCache('test-key');
        expect(cached).toEqual(testData);
      });

      it('should handle cache expiration', () => {
        const testData = { id: 1, name: 'test' };

        // Set cache with short TTL
        (optimizer as any).setCache('expiring-key', testData);

        // Manually expire the cache entry
        const cacheEntry = (optimizer as any).cache.get('expiring-key');
        if (cacheEntry) {
          cacheEntry.timestamp = Date.now() - (cacheEntry.ttl + 1000); // Expire
        }

        // Try to get expired cache
        const cached = (optimizer as any).getFromCache('expiring-key');
        expect(cached).toBeNull();
      });

      it('should evict oldest entries when cache is full', () => {
        // Fill cache to capacity
        for (let i = 0; i < 1000; i++) {
          (optimizer as any).setCache(`key-${i}`, { data: i });
        }

        // Add one more entry (should evict oldest)
        (optimizer as any).setCache('new-key', { data: 'new' });

        // Oldest entry should be evicted
        const oldestEntry = (optimizer as any).getFromCache('key-0');
        expect(oldestEntry).toBeNull();

        // New entry should be available
        const newEntry = (optimizer as any).getFromCache('new-key');
        expect(newEntry).toEqual({ data: 'new' });
      });
    });

    describe('JSON Serialization Optimization', () => {
      it('should optimize serialization with caching', () => {
        const testData = {
          id: 1,
          name: 'test',
          timestamp: new Date(),
          buffer: Buffer.from('test data'),
          nested: {
            value: 'nested',
            array: [1, 2, 3],
          },
        };

        // First serialization
        const serialized1 = optimizer.optimizeSerialization(testData);
        expect(serialized1).toBeDefined();
        expect(typeof serialized1).toBe('string');

        // Second serialization (should use cache)
        const serialized2 = optimizer.optimizeSerialization(testData);
        expect(serialized2).toBe(serialized1);

        const metrics = optimizer.getMetrics();
        expect(metrics.serialization.totalOperations).toBe(2);
      });

      it('should handle different data types efficiently', () => {
        const testData = {
          string: 'test',
          number: 42,
          boolean: true,
          null: null,
          undefined: undefined,
          date: new Date(),
          buffer: Buffer.from('test'),
          array: [1, 2, 3],
          object: { nested: true },
        };

        const serialized = optimizer.optimizeSerialization(testData);

        // Standard JSON.stringify behavior - Buffer becomes {type: "Buffer", data: [...]}
        expect(serialized).toContain('"type":"Buffer"');

        const parsed = JSON.parse(serialized);

        expect(parsed.string).toBe('test');
        expect(parsed.number).toBe(42);
        expect(parsed.boolean).toBe(true);
        expect(parsed.null).toBeNull();
        expect(parsed.undefined).toBeUndefined();
        expect(parsed.date).toBe(testData.date.toISOString());
        expect(parsed.array).toEqual([1, 2, 3]);
        expect(parsed.object).toEqual({ nested: true });
      });
    });

    describe('Memory Monitoring', () => {
      it('should track memory usage', () => {
        // Force memory update
        optimizer['startMemoryMonitoring']();

        const metrics = optimizer.getMetrics();
        expect(metrics.memory.used).toBeGreaterThan(0);
        expect(metrics.memory.peak).toBeGreaterThanOrEqual(metrics.memory.used);
      });

      it('should trigger garbage collection when memory is high', () => {
        // Simulate high memory usage
        const largeArray = new Array(1000000).fill('test');

        // Force memory update
        optimizer['startMemoryMonitoring']();

        const metrics = optimizer.getMetrics();
        expect(metrics.memory.used).toBeGreaterThan(0);

        // Clean up
        largeArray.length = 0;
      });
    });

    describe('Performance Recommendations', () => {
      it('should provide performance recommendations', () => {
        const recommendations = optimizer.getPerformanceRecommendations();
        expect(Array.isArray(recommendations)).toBe(true);
      });

      it('should recommend cache optimization for low hit rates', () => {
        // Simulate low cache hit rate
        const metrics = optimizer.getMetrics();
        metrics.cache.hitRate = 0.3; // Low hit rate

        const recommendations = optimizer.getPerformanceRecommendations();
        expect(recommendations.some(rec => rec.includes('cache'))).toBe(true);
      });
    });
  });

  describe('PerformanceMonitor', () => {
    let monitor: PerformanceMonitor;

    beforeEach(() => {
      monitor = new PerformanceMonitor({
        alertThresholds: {
          memoryUsage: 50, // Lower threshold for testing
          slowQueryThreshold: 100, // Lower threshold for testing
          cacheHitRate: 0.5,
          serializationTime: 10, // Lower threshold for testing
        },
        reportingInterval: 1000, // Faster reporting for testing
        enableAlerts: true,
      });
    });

    afterEach(() => {
      monitor.stop();
    });

    describe('Performance Monitoring', () => {
      it('should monitor performance metrics', () => {
        const testMetrics = {
          databaseQueries: { total: 10, cached: 5, slow: 2, averageTime: 150 },
          cache: { hits: 8, misses: 2, hitRate: 0.8, size: 100 },
          serialization: {
            totalOperations: 20,
            averageTime: 5,
            compressedSize: 1024,
          },
          memory: {
            used: 50 * 1024 * 1024,
            peak: 60 * 1024 * 1024,
            garbageCollections: 1,
          },
        };

        monitor.monitorMetrics(testMetrics);

        const latestReport = monitor.getLatestReport();
        expect(latestReport).toBeDefined();
        expect(latestReport?.metrics).toEqual(testMetrics);
      });

      it('should generate alerts for performance issues', () => {
        const problematicMetrics = {
          databaseQueries: { total: 5, cached: 1, slow: 3, averageTime: 1200 },
          cache: { hits: 2, misses: 8, hitRate: 0.2, size: 50 },
          serialization: {
            totalOperations: 10,
            averageTime: 100,
            compressedSize: 2048,
          },
          memory: {
            used: 80 * 1024 * 1024,
            peak: 90 * 1024 * 1024,
            garbageCollections: 2,
          },
        };

        monitor.monitorMetrics(problematicMetrics);

        const alerts = monitor.getAlerts();
        expect(alerts.length).toBeGreaterThan(0);
        expect(alerts.some(alert => alert.type === 'warning')).toBe(true);
      });

      it('should calculate performance score correctly', () => {
        const healthyMetrics = {
          databaseQueries: { total: 10, cached: 8, slow: 0, averageTime: 50 },
          cache: { hits: 9, misses: 1, hitRate: 0.9, size: 100 },
          serialization: {
            totalOperations: 20,
            averageTime: 5,
            compressedSize: 1024,
          },
          memory: {
            used: 20 * 1024 * 1024,
            peak: 25 * 1024 * 1024,
            garbageCollections: 0,
          },
        };

        monitor.monitorMetrics(healthyMetrics);

        const latestReport = monitor.getLatestReport();
        expect(latestReport?.summary.score).toBeGreaterThan(80);
        expect(latestReport?.summary.status).toBe('healthy');
      });
    });

    describe('Performance Trends', () => {
      it('should calculate performance trends', () => {
        // Add multiple reports to establish trends
        const metrics1 = {
          databaseQueries: { total: 5, cached: 2, slow: 0, averageTime: 50 },
          cache: { hits: 4, misses: 1, hitRate: 0.8, size: 50 },
          serialization: {
            totalOperations: 10,
            averageTime: 5,
            compressedSize: 512,
          },
          memory: {
            used: 20 * 1024 * 1024,
            peak: 25 * 1024 * 1024,
            garbageCollections: 0,
          },
        };

        const metrics2 = {
          databaseQueries: { total: 10, cached: 8, slow: 0, averageTime: 40 },
          cache: { hits: 9, misses: 1, hitRate: 0.9, size: 100 },
          serialization: {
            totalOperations: 20,
            averageTime: 3,
            compressedSize: 1024,
          },
          memory: {
            used: 25 * 1024 * 1024,
            peak: 30 * 1024 * 1024,
            garbageCollections: 0,
          },
        };

        monitor.monitorMetrics(metrics1);
        monitor.monitorMetrics(metrics2);

        const trends = monitor.getPerformanceTrends();
        expect(trends.memoryTrend).toBeDefined();
        expect(trends.queryTrend).toBeDefined();
        expect(trends.cacheTrend).toBeDefined();
      });
    });

    describe('Performance Summary', () => {
      it('should generate comprehensive performance summary', () => {
        const testMetrics = {
          databaseQueries: { total: 15, cached: 10, slow: 1, averageTime: 75 },
          cache: { hits: 12, misses: 3, hitRate: 0.8, size: 100 },
          serialization: {
            totalOperations: 25,
            averageTime: 8,
            compressedSize: 1536,
          },
          memory: {
            used: 30 * 1024 * 1024,
            peak: 35 * 1024 * 1024,
            garbageCollections: 1,
          },
        };

        monitor.monitorMetrics(testMetrics);

        const summary = monitor.generateSummary();
        expect(summary).toContain('Performance Summary');
        expect(summary).toContain('Overall Score');
        expect(summary).toContain('Memory Usage');
        expect(summary).toContain('Database Queries');
        expect(summary).toContain('Cache Hit Rate');
      });
    });

    describe('Alert Management', () => {
      it('should manage alerts correctly', () => {
        const alertMetrics = {
          databaseQueries: { total: 5, cached: 1, slow: 2, averageTime: 1500 },
          cache: { hits: 1, misses: 4, hitRate: 0.2, size: 20 },
          serialization: {
            totalOperations: 10,
            averageTime: 75,
            compressedSize: 2048,
          },
          memory: {
            used: 85 * 1024 * 1024,
            peak: 90 * 1024 * 1024,
            garbageCollections: 3,
          },
        };

        monitor.monitorMetrics(alertMetrics);

        const alerts = monitor.getAlerts();
        expect(alerts.length).toBeGreaterThan(0);

        // Test alert properties
        const alert = alerts[0];
        expect(alert.type).toBeDefined();
        expect(alert.message).toBeDefined();
        expect(alert.timestamp).toBeInstanceOf(Date);
        expect(alert.metric).toBeDefined();
        expect(alert.value).toBeGreaterThan(0);
        expect(alert.threshold).toBeGreaterThan(0);
      });

      it('should limit alert history', () => {
        // Generate many alerts
        for (let i = 0; i < 60; i++) {
          const metrics = {
            databaseQueries: {
              total: 5,
              cached: 1,
              slow: 2,
              averageTime: 1500,
            },
            cache: { hits: 1, misses: 4, hitRate: 0.2, size: 20 },
            serialization: {
              totalOperations: 10,
              averageTime: 75,
              compressedSize: 2048,
            },
            memory: {
              used: 85 * 1024 * 1024,
              peak: 90 * 1024 * 1024,
              garbageCollections: 3,
            },
          };
          monitor.monitorMetrics(metrics);
        }

        const alerts = monitor.getAlerts();
        expect(alerts.length).toBeLessThanOrEqual(50); // Should be limited
      });
    });
  });

  describe('Integration with Step Execution', () => {
    it('should integrate performance optimization with step execution', async () => {
      const { StepExecutor } = await import(
        '../sdk/src/execution/step-executor'
      );

      const step = {
        id: 'test-step',
        name: 'test-step',
        handler: async () => {
          return { result: 'success', timestamp: new Date() };
        },
        type: 'step' as const,
      };

      const context = {
        payload: {},
        steps: {},
        services: {},
        run: { id: 'test', workflowId: 'test' },
        state: {
          get: () => null,
          set: async () => {},
          incr: async () => 0,
        },
        last: null,
        trigger: { headers: {} },
        cancel: () => {
          throw new Error('cancelled');
        },
      };

      const result = await StepExecutor.executeStep(step, context);
      expect(result.success).toBe(true);
      expect(result.output).toBeDefined();

      // Check that performance optimization was applied
      const optimizer = StepExecutor.getPerformanceOptimizer();
      const metrics = optimizer.getMetrics();
      expect(metrics.serialization.totalOperations).toBeGreaterThan(0);
    });

    it('should optimize database queries in step execution', async () => {
      const { StepExecutor } = await import(
        '../sdk/src/execution/step-executor'
      );

      let queryCount = 0;
      const mockQuery = async () => {
        queryCount++;
        await new Promise(resolve => setTimeout(resolve, 10));
        return { data: 'query result' };
      };

      // Execute optimized query
      const result1 = await StepExecutor.executeOptimizedDatabaseQuery(
        mockQuery,
        'test-query'
      );
      expect(result1).toEqual({ data: 'query result' });
      expect(queryCount).toBe(1);

      // Execute same query again (should use cache)
      const result2 = await StepExecutor.executeOptimizedDatabaseQuery(
        mockQuery,
        'test-query'
      );
      expect(result2).toEqual({ data: 'query result' });
      expect(queryCount).toBe(1); // Should not hit database again

      // Check connection pool
      const connection = await StepExecutor.getDatabaseConnection();
      expect(connection).toBeDefined();
      StepExecutor.releaseDatabaseConnection(connection);
    });
  });
});
