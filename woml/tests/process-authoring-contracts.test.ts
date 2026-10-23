import { describe, expect, test } from 'bun:test';
import { join, resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';

import {
  analyzeWomlScript,
  compileWoml,
  generateWomlEditorDeclarations,
  parseWoml,
  WomlDiagnosticError,
} from '../src';

const schemaDirectory = resolve(import.meta.dir, '../../docs/schemas');
const invalidFixtureDirectory = resolve(
  import.meta.dir,
  'fixtures/services-contracts-invalid'
);

async function processValidators() {
  const ajv = new Ajv2020({
    allErrors: true,
    strict: false,
    validateFormats: false,
  });
  for (const name of [
    'process.v1.schema.json',
    'process-policy.v1.schema.json',
  ]) {
    ajv.addSchema(await Bun.file(join(schemaDirectory, name)).json());
  }
  return {
    process: ajv.getSchema('https://woml.dev/schemas/process/v1')!,
    policy: ajv.getSchema('https://woml.dev/schemas/process-policy/v1')!,
  };
}

describe('Process v1 authoring contracts', () => {
  test('rejects reviewed traversal and policy-escalation fixtures', async () => {
    const validators = await processValidators();
    const traversal = await Bun.file(
      join(invalidFixtureDirectory, 'process-request-invalid-path.v1.json')
    ).json();
    const escalation = await Bun.file(
      join(invalidFixtureDirectory, 'process-policy-unknown-field.v1.json')
    ).json();

    expect(validators.process(traversal)).toBe(false);
    expect(validators.policy(escalation)).toBe(false);
  });

  test('rejects unknown fields, shell strings, duplicate exit codes, and invalid results', async () => {
    const { process } = await processValidators();
    const request = await Bun.file(
      resolve(
        import.meta.dir,
        'fixtures/services-contracts/process-request.v1.json'
      )
    ).json();
    const result = await Bun.file(
      resolve(
        import.meta.dir,
        'fixtures/services-contracts/process-result.v1.json'
      )
    ).json();

    expect(process({ ...request, shell: true })).toBe(false);
    expect(process({ ...request, executable: 'sh -c' })).toBe(false);
    expect(process({ ...request, acceptExitCodes: [0, 0] })).toBe(false);
    expect(
      process({ ...request, output: { ...request.output, maxBytes: 262_145 } })
    ).toBe(false);
    expect(process({ ...result, exitCode: null, signal: null })).toBe(false);
  });

  test('recognizes process as a reserved built-in service and selects Model v8', () => {
    const source = `<woml>
<workflow id="process-authoring" version="1.0.0">
  <triggers><manual id="start" /></triggers>
  <steps><step id="check"><script>
    return services.process.run({ executable: "bun", args: ["test"] });
  </script></step></steps>
</workflow>
</woml>`;
    const analysis = analyzeWomlScript(
      'return services.process.run({ executable: "bun" });'
    );

    expect(analysis.requiredServices).toEqual(['process']);
    expect(analysis.usesServices).toBe(true);
    expect(compileWoml(parseWoml(source)).schemaVersion).toBe(8);

    const shadowing = parseWoml(`<woml>
<imports><module name="process" from="./process.ts" /></imports>
<workflow id="process-shadow">
  <triggers><manual id="start" /></triggers>
  <steps><step id="done"><script>return true;</script></step></steps>
</workflow>
</woml>`);
    expect(() => compileWoml(shadowing)).toThrow(WomlDiagnosticError);
  });

  test('generates the frozen Process v1 editor surface without a shell API', () => {
    const declarations = generateWomlEditorDeclarations([]);

    for (const expected of [
      'interface WomlProcessRunRequest',
      'interface WomlProcessRunResult',
      'interface WomlProcessService',
      'readonly process: WomlProcessService;',
      'readonly timeout?: number | string;',
      'readonly stream?: boolean;',
      'readonly streamStartSequence?: number | null;',
      'interface WomlProgressService',
      'readonly progress: WomlProgressService;',
      ') => Promise<WomlProcessRunResult>;',
    ]) {
      expect(declarations).toContain(expected);
    }
    expect(declarations).not.toContain('readonly shell:');
  });
});
