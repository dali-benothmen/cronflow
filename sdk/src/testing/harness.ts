import { Context, WorkflowDefinition } from '../workflow/types';
import { createContext } from '../utils';
import { generateId } from '../utils';

export interface TestRun {
  id: string;
  workflowId: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  steps: Record<
    string,
    {
      name: string;
      status: 'pending' | 'running' | 'completed' | 'failed';
      output?: any;
      error?: Error;
      duration: number;
    }
  >;
  payload: any;
  error?: Error;
  duration: number;
  createdAt: Date;
  completedAt?: Date;
}

export interface TestStep {
  name: string;
  handler: (ctx: Context) => any | Promise<any>;
  originalHandler?: (ctx: Context) => any | Promise<any>;
  mocked: boolean;
}

export interface TestAssertion {
  stepName: string;
  expectedStatus: 'succeed' | 'fail';
  expectedError?: string;
}

export class TestHarness {
  protected workflow: WorkflowDefinition;
  private testSteps: Map<string, TestStep> = new Map();
  private assertions: TestAssertion[] = [];
  private payload: any = {};
  private runId: string;

  constructor(workflow: WorkflowDefinition) {
    this.workflow = workflow;
    this.runId = generateId('test');
    this.initializeTestSteps();
  }

  private initializeTestSteps(): void {
    for (const step of this.workflow.steps) {
      this.testSteps.set(step.name, {
        name: step.name,
        handler: step.handler,
        originalHandler: step.handler,
        mocked: false,
      });
    }
  }

  trigger(payload: any): TestHarness {
    this.payload = payload;
    return this;
  }

  mockStep(
    stepName: string,
    mockFn: (ctx: Context) => any | Promise<any>
  ): TestHarness {
    const testStep = this.testSteps.get(stepName);
    if (!testStep) {
      throw new Error(
        `Step '${stepName}' not found in workflow '${this.workflow.id}'`
      );
    }

    testStep.handler = mockFn;
    testStep.mocked = true;
    return this;
  }

  expectStep(stepName: string): TestAssertionBuilder {
    if (!this.testSteps.has(stepName)) {
      throw new Error(
        `Step '${stepName}' not found in workflow '${this.workflow.id}'`
      );
    }

    return new TestAssertionBuilder(this, stepName);
  }

  async run(): Promise<TestRun> {
    const startTime = Date.now();
    const testRun: TestRun = {
      id: this.runId,
      workflowId: this.workflow.id,
      status: 'running',
      steps: {},
      payload: this.payload,
      duration: 0,
      createdAt: new Date(),
    };

    try {
      const context = createContext(
        this.payload,
        this.workflow.id,
        this.runId,
        this.workflow.services || [],
        {},
        null,
        { headers: {} }
      );

      for (const step of this.workflow.steps) {
        const testStep = this.testSteps.get(step.name);
        if (!testStep) continue;

        const stepStartTime = Date.now();
        const stepResult: TestRun['steps'][string] = {
          name: step.name,
          status: 'pending',
          duration: 0,
        };

        testRun.steps[step.name] = stepResult;

        try {
          stepResult.status = 'running';
          const output = await testStep.handler(context);

          stepResult.status = 'completed';
          stepResult.output = output;
          stepResult.duration = Date.now() - stepStartTime;

          context.last = output;
          context.steps[step.name] = { output };
        } catch (error) {
          stepResult.status = 'failed';
          stepResult.error = error as Error;
          stepResult.duration = Date.now() - stepStartTime;

          const assertion = this.assertions.find(a => a.stepName === step.name);
          if (assertion && assertion.expectedStatus === 'fail') {
            if (assertion.expectedError) {
              const errorMessage = (error as Error).message;
              if (!errorMessage.includes(assertion.expectedError)) {
                throw new Error(
                  `Step '${step.name}' failed with unexpected error. Expected: ${assertion.expectedError}, Got: ${errorMessage}`
                );
              }
            }
          } else if (assertion && assertion.expectedStatus === 'succeed') {
            throw new Error(
              `Step '${step.name}' was expected to succeed but failed: ${(error as Error).message}`
            );
          } else {
            testRun.status = 'failed';
            testRun.error = error as Error;
            break;
          }
        }

        const assertion = this.assertions.find(a => a.stepName === step.name);
        if (assertion) {
          if (
            assertion.expectedStatus === 'succeed' &&
            stepResult.status !== 'completed'
          ) {
            throw new Error(
              `Step '${step.name}' was expected to succeed but failed`
            );
          }
          if (
            assertion.expectedStatus === 'fail' &&
            stepResult.status !== 'failed'
          ) {
            throw new Error(
              `Step '${step.name}' was expected to fail but succeeded`
            );
          }
        }
      }

      for (const assertion of this.assertions) {
        if (!testRun.steps[assertion.stepName]) {
          throw new Error(
            `Expected step '${assertion.stepName}' was not executed`
          );
        }
      }

      testRun.status = 'completed';
      testRun.completedAt = new Date();
      testRun.duration = Date.now() - startTime;
    } catch (error) {
      testRun.status = 'failed';
      testRun.error = error as Error;
      testRun.duration = Date.now() - startTime;
    }

    return testRun;
  }

  addAssertion(assertion: TestAssertion): void {
    this.assertions.push(assertion);
  }

  reset(): void {
    this.assertions = [];
    this.payload = {};
    this.runId = generateId('test');
    this.initializeTestSteps();
  }
}

export class TestAssertionBuilder {
  private harness: TestHarness;
  private stepName: string;

  constructor(harness: TestHarness, stepName: string) {
    this.harness = harness;
    this.stepName = stepName;
  }

  toSucceed(): TestHarness {
    this.harness.addAssertion({
      stepName: this.stepName,
      expectedStatus: 'succeed',
    });
    return this.harness;
  }

  toFailWith(errorMessage: string): TestHarness {
    this.harness.addAssertion({
      stepName: this.stepName,
      expectedStatus: 'fail',
      expectedError: errorMessage,
    });
    return this.harness;
  }

  toFail(): TestHarness {
    this.harness.addAssertion({
      stepName: this.stepName,
      expectedStatus: 'fail',
    });
    return this.harness;
  }
}

export function createTestHarness(workflow: WorkflowDefinition): TestHarness {
  return new TestHarness(workflow);
}
