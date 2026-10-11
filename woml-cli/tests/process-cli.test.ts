import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const cli = resolve(import.meta.dir, '../dist/cli.js');
let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-process-cli-'));
});

afterAll(async () => {
  if (temporaryDirectory !== undefined) {
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
});

function run(...args: string[]): ReturnType<typeof Bun.spawnSync> {
  return Bun.spawnSync([process.execPath, cli, ...args], {
    cwd: temporaryDirectory,
    env: { ...process.env, NO_COLOR: '1' },
    stdout: 'pipe',
    stderr: 'pipe',
  });
}

describe('secure Process v1 CLI journey', () => {
  test('executes a real workflow and preserves every argument as literal data', async () => {
    const helperName = 'echo arguments 雪.ts';
    const marker = join(temporaryDirectory, 'shell-must-not-run');
    await writeFile(
      join(temporaryDirectory, helperName),
      'process.stdout.write(JSON.stringify(Bun.argv.slice(2)) + "\\r\\n");\n'
    );
    const literalArguments = [
      'space value',
      'café 雪',
      '"quoted"',
      'line\r\nbreak',
      `$(touch ${marker})`,
      `; touch ${marker}`,
    ];
    const workflow = `<woml>
  <workflow id="process-literal-arguments" name="Process literal arguments" version="1.0.0">
    <triggers><manual id="start" /></triggers>
    <steps>
      <step id="run" name="Run safe argument fixture">
        <script>
          const result = await services.process.run({
            executable: "bun",
            args: ${JSON.stringify(['run', helperName, ...literalArguments])},
            timeout: "10s"
          }, { name: "literal-arguments" });
          return {
            exitCode: result.exitCode,
            arguments: JSON.parse(result.stdout),
            endedWithCrlf: result.stdout.endsWith("\\r\\n")
          };
        </script>
      </step>
    </steps>
  </workflow>
</woml>\n`;
    const workflowPath = join(temporaryDirectory, 'process arguments.woml');
    await writeFile(workflowPath, workflow);

    const denied = run('test', workflowPath);
    expect(denied.exitCode).not.toBe(0);
    expect(denied.stderr?.toString() ?? '').toContain('services.process');

    const executed = run(
      'test',
      workflowPath,
      '--allow-process',
      'bun',
      '--state',
      join(temporaryDirectory, 'workflow-history.sqlite')
    );
    expect(executed.exitCode, executed.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(executed.stdout?.toString() ?? '')).toEqual({
      exitCode: 0,
      arguments: literalArguments,
      endedWithCrlf: true,
    });
    expect(await Bun.file(marker).exists()).toBe(false);
  }, 30_000);
});
