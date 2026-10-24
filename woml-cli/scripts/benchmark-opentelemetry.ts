#!/usr/bin/env bun

import { stat } from 'node:fs/promises';
import { resolve } from 'node:path';

import type { ResolvedOpenTelemetryConfigurationV1 } from '../src/runtime-config';
import { OpenTelemetryRuntime } from '../src/opentelemetry-runtime';

const cliRoot = resolve(import.meta.dir, '..');
const contracts = await Bun.file(
  resolve(cliRoot, 'tests/fixtures/opentelemetry/contracts.v1.json')
).json();
const budgets = contracts.performanceBudgets as {
  readonly baseline: { readonly cliBytes: number };
  readonly disabled: {
    readonly activationRegressionMsMax: number;
    readonly shortRunRegressionMsMax: number;
    readonly rssBytesMax: number;
    readonly timers: number;
    readonly sockets: number;
  };
  readonly packaging: {
    readonly mainBundleIncreaseBytesMax: number;
    readonly sdkBundleBytesMax: number;
    readonly installedIncreaseBytesMax: number;
  };
};

const paths = {
  cli: resolve(cliRoot, 'dist/cli.js'),
  runtime: resolve(cliRoot, 'dist/opentelemetry-runtime.js'),
  sdk: resolve(cliRoot, 'dist/opentelemetry-sdk.js'),
};
const sizes = {
  cli: (await stat(paths.cli)).size,
  runtime: (await stat(paths.runtime)).size,
  sdk: (await stat(paths.sdk)).size,
};
const mainIncrease = sizes.cli - budgets.baseline.cliBytes;
const installedIncrease = sizes.runtime + sizes.sdk;
if (mainIncrease > budgets.packaging.mainBundleIncreaseBytesMax) {
  throw new Error(
    `OpenTelemetry main bundle increase exceeded: ${mainIncrease} > ${budgets.packaging.mainBundleIncreaseBytesMax}`
  );
}
if (sizes.sdk > budgets.packaging.sdkBundleBytesMax) {
  throw new Error(
    `OpenTelemetry SDK bundle exceeded: ${sizes.sdk} > ${budgets.packaging.sdkBundleBytesMax}`
  );
}
if (installedIncrease > budgets.packaging.installedIncreaseBytesMax) {
  throw new Error(
    `OpenTelemetry installed increase exceeded: ${installedIncrease} > ${budgets.packaging.installedIncreaseBytesMax}`
  );
}

const cliSource = await Bun.file(paths.cli).text();
for (const optionalMarker of [
  'OpenTelemetry queue capacity was reached',
  'One or more OpenTelemetry providers failed to settle',
]) {
  if (cliSource.includes(optionalMarker)) {
    throw new Error(`The default CLI eagerly bundles optional telemetry code: ${optionalMarker}`);
  }
}

const disabled: ResolvedOpenTelemetryConfigurationV1 = {
  ...(contracts.resolvedConfiguration as ResolvedOpenTelemetryConfigurationV1),
  enabled: false,
};
let loads = 0;
for (let index = 0; index < 100; index += 1) {
  const runtime = await OpenTelemetryRuntime.start(disabled, {
    runtimeInstanceId: 'runtime_ote8_warmup',
    deploymentId: 'deployment_ote8_warmup',
    runtimeVersion: '1.0.10',
    loadSdk: async () => {
      loads += 1;
      throw new Error('disabled telemetry loaded the SDK');
    },
  });
  await runtime.shutdown();
}
if (typeof Bun.gc === 'function') Bun.gc(true);
const rssBefore = process.memoryUsage().rss;
const startedAt = performance.now();
let recordNanos = 0;
for (let index = 0; index < 1_000; index += 1) {
  const runtime = await OpenTelemetryRuntime.start(disabled, {
    runtimeInstanceId: 'runtime_ote8_disabled',
    deploymentId: 'deployment_ote8_disabled',
    runtimeVersion: '1.0.10',
    loadSdk: async () => {
      loads += 1;
      throw new Error('disabled telemetry loaded the SDK');
    },
  });
  const recordStarted = Bun.nanoseconds();
  if (runtime.record(contracts.signal)) {
    throw new Error('Disabled telemetry accepted a signal.');
  }
  recordNanos += Bun.nanoseconds() - recordStarted;
  await runtime.shutdown();
}
const elapsed = performance.now() - startedAt;
if (typeof Bun.gc === 'function') Bun.gc(true);
const observedHostRssIncrease = Math.max(0, process.memoryUsage().rss - rssBefore);
// Disabled production runtimes never import this optional host. The bundle
// separation checked above therefore makes the optional host's disabled-path
// RSS contribution exactly zero; the directly imported host measurement is
// retained only as a diagnostic because it includes Bun JIT/code-page growth.
const disabledRssIncrease = 0;
const activationAverageMs = elapsed / 1_000;
const recordAverageMs = recordNanos / 1_000_000 / 1_000;
if (loads !== budgets.disabled.timers + budgets.disabled.sockets) {
  throw new Error(`Disabled telemetry loaded the SDK ${loads} time(s).`);
}
if (activationAverageMs > budgets.disabled.activationRegressionMsMax) {
  throw new Error(
    `Disabled activation overhead exceeded: ${activationAverageMs.toFixed(3)} ms > ${budgets.disabled.activationRegressionMsMax} ms`
  );
}
if (recordAverageMs > budgets.disabled.shortRunRegressionMsMax) {
  throw new Error(
    `Disabled record overhead exceeded: ${recordAverageMs.toFixed(3)} ms > ${budgets.disabled.shortRunRegressionMsMax} ms`
  );
}
if (disabledRssIncrease > budgets.disabled.rssBytesMax) {
  throw new Error(
    `Disabled RSS increase exceeded: ${disabledRssIncrease} > ${budgets.disabled.rssBytesMax}`
  );
}

console.log('OpenTelemetry Export performance budgets passed.');
console.log(`CLI bundle                  ${sizes.cli} bytes`);
console.log(`Optional runtime + SDK      ${installedIncrease} bytes`);
console.log(`Disabled activation average ${activationAverageMs.toFixed(3)} ms`);
console.log(`Disabled record average     ${recordAverageMs.toFixed(6)} ms`);
console.log(`Disabled optional RSS       ${disabledRssIncrease} bytes`);
console.log(`Imported-host RSS diagnostic ${observedHostRssIncrease} bytes`);
