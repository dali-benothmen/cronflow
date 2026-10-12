import { describe, expect, test } from 'bun:test';
import { resolve } from 'node:path';

import {
  analyzeWomlScript,
  compileWoml,
  generateWomlEditorDeclarations,
  parseWoml,
  WomlDiagnosticError,
} from '../src';

const reviewedFixture = resolve(
  import.meta.dir,
  'fixtures/durable-artifacts/reviewed.woml'
);

describe('Durable Artifacts DA2 authoring', () => {
  test('recognizes artifacts as a reserved built-in and compiles the reviewed workflow', async () => {
    const source = await Bun.file(reviewedFixture).text();
    const compiled = compileWoml(
      parseWoml(source, { file: reviewedFixture })
    );
    const analysis = analyzeWomlScript(`
      const artifact = await services.artifacts.put("reports/result.json", {
        name: "result",
        contentType: "application/json",
        retention: "30d"
      });
      return services.artifacts.head(artifact);
    `);

    expect(compiled.schemaVersion).toBe(8);
    expect(analysis.issue).toBeUndefined();
    expect(analysis.requiredServices).toEqual(['artifacts']);
    expect(analysis.usesServices).toBe(true);

    const shadowing = parseWoml(`<woml>
<imports><module name="artifacts" from="./artifacts.ts" /></imports>
<workflow id="artifact-shadow">
  <triggers><manual id="start" /></triggers>
  <steps><step id="done"><script>return true;</script></step></steps>
</workflow>
</woml>`);
    expect(() => compileWoml(shadowing)).toThrow(WomlDiagnosticError);
  });

  test('rejects unsupported operations and invalid literal put shapes with source diagnostics', () => {
    const cases = [
      ['return services.artifacts.delete({});', 'WOML_ARTIFACT_OPERATION_UNSUPPORTED'],
      ['return services.artifacts.put("report.txt");', 'WOML_ARTIFACT_PUT_ARGUMENTS_INVALID'],
      [
        'return services.artifacts.put("../report.txt", { name: "report" });',
        'WOML_ARTIFACT_PATH_INVALID',
      ],
      [
        'return services.artifacts.put("report.txt", { contentType: "text/plain" });',
        'WOML_ARTIFACT_NAME_REQUIRED',
      ],
      [
        'return services.artifacts.put("report.txt", { name: "report", retention: "1.5d" });',
        'WOML_ARTIFACT_RETENTION_INVALID',
      ],
      [
        'return services.artifacts.put("report.txt", { name: "report", unknown: true });',
        'WOML_ARTIFACT_PUT_OPTION_UNKNOWN',
      ],
      [
        'return services.artifacts.materialize();',
        'WOML_ARTIFACT_REFERENCE_ARGUMENTS_INVALID',
      ],
    ] as const;

    for (const [source, code] of cases) {
      expect(analyzeWomlScript(source).issue?.code).toBe(code);
    }
  });

  test('generates the frozen Artifact v1 editor surface automatically', () => {
    const declarations = generateWomlEditorDeclarations([]);
    for (const expected of [
      'interface WomlArtifactPutOptions',
      'interface WomlArtifactReferenceV1',
      'interface WomlArtifactHeadV1',
      'interface WomlArtifactsService',
      'readonly artifacts: WomlArtifactsService;',
      ') => Promise<WomlArtifactReferenceV1>;',
      ') => Promise<string>;',
    ]) {
      expect(declarations).toContain(expected);
    }
  });
});
