import { resourceFromAttributes } from '@opentelemetry/resources';
import {
  AlwaysOffSampler,
  AlwaysOnSampler,
  BatchSpanProcessor,
  ParentBasedSampler,
  TraceIdRatioBasedSampler,
  TracerProvider,
  type Sampler,
} from '@opentelemetry/sdk-trace';
import {
  MeterProvider,
  PeriodicExportingMetricReader,
} from '@opentelemetry/sdk-metrics';
import {
  BatchLogRecordProcessor,
  LoggerProvider,
} from '@opentelemetry/sdk-logs';
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-proto';
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-proto';
import { OTLPLogExporter } from '@opentelemetry/exporter-logs-otlp-proto';

import type {
  OpenTelemetrySdkHandle,
  OpenTelemetrySdkStartOptions,
} from './opentelemetry-runtime';

function mergedHeaders(
  common: Readonly<Record<string, string>>,
  signal: Readonly<Record<string, string>>
): Record<string, string> {
  return { ...common, ...signal };
}

function sampler(
  configuration: OpenTelemetrySdkStartOptions['configuration']
): Sampler {
  if (configuration.sampler.name === 'always_on') return new AlwaysOnSampler();
  if (configuration.sampler.name === 'always_off') return new AlwaysOffSampler();
  if (configuration.sampler.name !== 'parentbased_traceidratio') {
    return new AlwaysOffSampler();
  }
  return new ParentBasedSampler({
    root: new TraceIdRatioBasedSampler(configuration.sampler.ratio),
  });
}

async function settleAll(
  operations: readonly (() => Promise<void>)[]
): Promise<void> {
  const results = await Promise.allSettled(operations.map(operation => operation()));
  const failures = results.filter(
    (result): result is PromiseRejectedResult => result.status === 'rejected'
  );
  if (failures.length > 0) {
    throw new AggregateError(
      failures.map(failure => failure.reason),
      'One or more OpenTelemetry providers failed to settle.'
    );
  }
}

export async function startOpenTelemetrySdk(
  options: OpenTelemetrySdkStartOptions
): Promise<OpenTelemetrySdkHandle> {
  const { configuration, secrets } = options;
  const resource = resourceFromAttributes({
    'service.name': configuration.serviceName,
    'service.version': options.resource.runtimeVersion,
    'service.instance.id': options.resource.runtimeInstanceId,
    'woml.deployment.id': options.resource.deploymentId,
    'woml.runtime.version': options.resource.runtimeVersion,
    ...configuration.resourceAttributes,
  });
  const providers: {
    forceFlush(): Promise<void>;
    shutdown(): Promise<void>;
  }[] = [];

  if (configuration.signals.traces) {
    const exporter = new OTLPTraceExporter({
      url: configuration.endpoints.traces,
      headers: mergedHeaders(secrets.headers.common, secrets.headers.traces),
      timeoutMillis: configuration.exportTimeoutMs,
      concurrencyLimit: 1,
    });
    providers.push(
      new TracerProvider({
        resource,
        sampler: sampler(configuration),
        spanLimits: {
          attributeCountLimit: 32,
          attributeValueLengthLimit: 256,
          eventCountLimit: 16,
          linkCountLimit: 16,
          attributePerEventCountLimit: 16,
          attributePerLinkCountLimit: 8,
        },
        spanProcessors: [
          new BatchSpanProcessor({
            exporter,
            maxQueueSize: configuration.maxQueueSize,
            maxExportBatchSize: configuration.maxExportBatchSize,
            scheduledDelayMillis: configuration.exportIntervalMs,
            exportTimeoutMillis: configuration.exportTimeoutMs,
          }),
        ],
      })
    );
  }

  if (configuration.signals.metrics) {
    const exporter = new OTLPMetricExporter({
      url: configuration.endpoints.metrics,
      headers: mergedHeaders(secrets.headers.common, secrets.headers.metrics),
      timeoutMillis: configuration.exportTimeoutMs,
      concurrencyLimit: 1,
    });
    providers.push(
      new MeterProvider({
        resource,
        readers: [
          new PeriodicExportingMetricReader({
            exporter,
            exportIntervalMillis: configuration.exportIntervalMs,
            exportTimeoutMillis: configuration.exportTimeoutMs,
          }),
        ],
      })
    );
  }

  if (configuration.signals.logs) {
    const exporter = new OTLPLogExporter({
      url: configuration.endpoints.logs,
      headers: mergedHeaders(secrets.headers.common, secrets.headers.logs),
      timeoutMillis: configuration.exportTimeoutMs,
      concurrencyLimit: 1,
    });
    providers.push(
      new LoggerProvider({
        resource,
        logRecordLimits: {
          attributeCountLimit: 32,
          attributeValueLengthLimit: 256,
        },
        processors: [
          new BatchLogRecordProcessor({
            exporter,
            maxQueueSize: configuration.maxQueueSize,
            maxExportBatchSize: configuration.maxExportBatchSize,
            scheduledDelayMillis: configuration.exportIntervalMs,
            exportTimeoutMillis: configuration.exportTimeoutMs,
          }),
        ],
      })
    );
  }

  return {
    forceFlush: async () => {
      await settleAll(providers.map(provider => () => provider.forceFlush()));
    },
    shutdown: async () => {
      await settleAll(providers.map(provider => () => provider.shutdown()));
    },
  };
}
