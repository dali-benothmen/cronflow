import { PerformanceMetrics } from './optimizer';

export interface PerformanceAlert {
  type: 'warning' | 'error' | 'info';
  message: string;
  timestamp: Date;
  metric: string;
  value: number;
  threshold: number;
}

export interface PerformanceReport {
  timestamp: Date;
  metrics: PerformanceMetrics;
  alerts: PerformanceAlert[];
  recommendations: string[];
  summary: {
    status: 'healthy' | 'warning' | 'critical';
    score: number;
  };
}

export interface MonitoringConfig {
  alertThresholds: {
    memoryUsage: number;
    slowQueryThreshold: number;
    cacheHitRate: number;
    serializationTime: number;
  };
  reportingInterval: number;
  enableAlerts: boolean;
}

export class PerformanceMonitor {
  private alerts: PerformanceAlert[] = [];
  private reports: PerformanceReport[] = [];
  private config: MonitoringConfig;
  private monitoringInterval?: NodeJS.Timeout;

  constructor(
    config: MonitoringConfig = {
      alertThresholds: {
        memoryUsage: 80, // 80% of available memory
        slowQueryThreshold: 1000, // 1 second
        cacheHitRate: 0.5, // 50%
        serializationTime: 50, // 50ms
      },
      reportingInterval: 30000, // 30 seconds
      enableAlerts: true,
    }
  ) {
    this.config = config;
    this.startMonitoring();
  }

  monitorMetrics(metrics: PerformanceMetrics): void {
    const alerts: PerformanceAlert[] = [];
    const recommendations: string[] = [];

    const memoryUsagePercent =
      (metrics.memory.used / (1024 * 1024 * 1024)) * 100;
    if (memoryUsagePercent > this.config.alertThresholds.memoryUsage) {
      alerts.push({
        type: 'warning',
        message: `High memory usage: ${memoryUsagePercent.toFixed(2)}%`,
        timestamp: new Date(),
        metric: 'memoryUsage',
        value: memoryUsagePercent,
        threshold: this.config.alertThresholds.memoryUsage,
      });
      recommendations.push(
        'Consider optimizing memory usage or increasing available memory'
      );
    }

    if (
      metrics.databaseQueries.averageTime >
      this.config.alertThresholds.slowQueryThreshold
    ) {
      alerts.push({
        type: 'warning',
        message: `Slow database queries detected: ${metrics.databaseQueries.averageTime.toFixed(2)}ms average`,
        timestamp: new Date(),
        metric: 'slowQueries',
        value: metrics.databaseQueries.averageTime,
        threshold: this.config.alertThresholds.slowQueryThreshold,
      });
      recommendations.push(
        'Consider adding database indexes or optimizing query patterns'
      );
    }

    if (metrics.cache.hitRate < this.config.alertThresholds.cacheHitRate) {
      alerts.push({
        type: 'warning',
        message: `Low cache hit rate: ${(metrics.cache.hitRate * 100).toFixed(2)}%`,
        timestamp: new Date(),
        metric: 'cacheHitRate',
        value: metrics.cache.hitRate,
        threshold: this.config.alertThresholds.cacheHitRate,
      });
      recommendations.push(
        'Consider increasing cache size or adjusting cache TTL'
      );
    }

    if (
      metrics.serialization.averageTime >
      this.config.alertThresholds.serializationTime
    ) {
      alerts.push({
        type: 'warning',
        message: `Slow serialization detected: ${metrics.serialization.averageTime.toFixed(2)}ms average`,
        timestamp: new Date(),
        metric: 'serializationTime',
        value: metrics.serialization.averageTime,
        threshold: this.config.alertThresholds.serializationTime,
      });
      recommendations.push(
        'Consider optimizing data structures or using more efficient serialization'
      );
    }

    this.alerts.push(...alerts);

    const report: PerformanceReport = {
      timestamp: new Date(),
      metrics,
      alerts,
      recommendations,
      summary: this.calculatePerformanceScore(metrics),
    };

    this.reports.push(report);

    if (this.config.enableAlerts && alerts.length > 0) {
      this.logAlerts(alerts);
    }

    if (this.reports.length > 100) {
      this.reports = this.reports.slice(-100);
    }
  }

  private calculatePerformanceScore(metrics: PerformanceMetrics): {
    status: 'healthy' | 'warning' | 'critical';
    score: number;
  } {
    let score = 100;
    let status: 'healthy' | 'warning' | 'critical' = 'healthy';

    const memoryUsagePercent =
      (metrics.memory.used / (1024 * 1024 * 1024)) * 100;
    if (memoryUsagePercent > 80) {
      score -= 30;
      status = 'critical';
    } else if (memoryUsagePercent > 60) {
      score -= 15;
      status = 'warning';
    }

    if (metrics.databaseQueries.averageTime > 1000) {
      score -= 25;
      status = 'critical';
    } else if (metrics.databaseQueries.averageTime > 500) {
      score -= 12;
      status = 'warning';
    }

    if (metrics.cache.hitRate < 0.3) {
      score -= 25;
      status = 'critical';
    } else if (metrics.cache.hitRate < 0.5) {
      score -= 12;
      status = 'warning';
    }

    if (metrics.serialization.averageTime > 100) {
      score -= 20;
      status = 'critical';
    } else if (metrics.serialization.averageTime > 50) {
      score -= 10;
      status = 'warning';
    }

    return {
      status,
      score: Math.max(0, score),
    };
  }

  private logAlerts(alerts: PerformanceAlert[]): void {
    console.log('\n🚨 Performance Alerts:');
    for (const alert of alerts) {
      const icon =
        alert.type === 'error' ? '❌' : alert.type === 'warning' ? '⚠️' : 'ℹ️';
      console.log(
        `${icon} ${alert.message} (${alert.metric}: ${alert.value} > ${alert.threshold})`
      );
    }
  }

  private startMonitoring(): void {
    this.monitoringInterval = setInterval(() => {
      console.log('\n📊 Performance Monitor Status:');
      console.log(`- Total Reports: ${this.reports.length}`);
      console.log(`- Total Alerts: ${this.alerts.length}`);

      if (this.reports.length > 0) {
        const latestReport = this.reports[this.reports.length - 1];
        console.log(
          `- Latest Score: ${latestReport.summary.score}/100 (${latestReport.summary.status})`
        );
      }
    }, this.config.reportingInterval);
  }

  getReports(limit: number = 10): PerformanceReport[] {
    return this.reports.slice(-limit);
  }

  getAlerts(limit: number = 50): PerformanceAlert[] {
    return this.alerts.slice(-limit);
  }

  getLatestReport(): PerformanceReport | null {
    return this.reports.length > 0
      ? this.reports[this.reports.length - 1]
      : null;
  }

  getPerformanceTrends(): {
    memoryTrend: 'increasing' | 'decreasing' | 'stable';
    queryTrend: 'increasing' | 'decreasing' | 'stable';
    cacheTrend: 'increasing' | 'decreasing' | 'stable';
  } {
    if (this.reports.length < 2) {
      return {
        memoryTrend: 'stable',
        queryTrend: 'stable',
        cacheTrend: 'stable',
      };
    }

    const recent = this.reports.slice(-5);
    const memoryValues = recent.map(r => r.metrics.memory.used);
    const queryValues = recent.map(r => r.metrics.databaseQueries.averageTime);
    const cacheValues = recent.map(r => r.metrics.cache.hitRate);

    return {
      memoryTrend: this.calculateTrend(memoryValues),
      queryTrend: this.calculateTrend(queryValues),
      cacheTrend: this.calculateTrend(cacheValues),
    };
  }

  private calculateTrend(
    values: number[]
  ): 'increasing' | 'decreasing' | 'stable' {
    if (values.length < 2) return 'stable';

    const firstHalf = values.slice(0, Math.floor(values.length / 2));
    const secondHalf = values.slice(Math.floor(values.length / 2));

    const firstAvg = firstHalf.reduce((a, b) => a + b, 0) / firstHalf.length;
    const secondAvg = secondHalf.reduce((a, b) => a + b, 0) / secondHalf.length;

    const change = ((secondAvg - firstAvg) / firstAvg) * 100;

    if (change > 10) return 'increasing';
    if (change < -10) return 'decreasing';
    return 'stable';
  }

  generateSummary(): string {
    const latestReport = this.getLatestReport();
    if (!latestReport) return 'No performance data available';

    const trends = this.getPerformanceTrends();
    const alerts = this.getAlerts(5);

    let summary = `📊 Performance Summary (${latestReport.timestamp.toLocaleString()})\n`;
    summary += `Overall Score: ${latestReport.summary.score}/100 (${latestReport.summary.status})\n\n`;

    summary += `Memory Usage: ${(latestReport.metrics.memory.used / (1024 * 1024)).toFixed(2)} MB (${trends.memoryTrend})\n`;
    summary += `Database Queries: ${latestReport.metrics.databaseQueries.averageTime.toFixed(2)}ms avg (${trends.queryTrend})\n`;
    summary += `Cache Hit Rate: ${(latestReport.metrics.cache.hitRate * 100).toFixed(2)}% (${trends.cacheTrend})\n`;
    summary += `Serialization: ${latestReport.metrics.serialization.averageTime.toFixed(2)}ms avg\n\n`;

    if (alerts.length > 0) {
      summary += `Recent Alerts:\n`;
      alerts.forEach(alert => {
        summary += `- ${alert.message}\n`;
      });
    }

    if (latestReport.recommendations.length > 0) {
      summary += `\nRecommendations:\n`;
      latestReport.recommendations.forEach(rec => {
        summary += `- ${rec}\n`;
      });
    }

    return summary;
  }

  stop(): void {
    if (this.monitoringInterval) {
      clearInterval(this.monitoringInterval);
    }
  }

  clearHistory(): void {
    this.alerts = [];
    this.reports = [];
  }
}
