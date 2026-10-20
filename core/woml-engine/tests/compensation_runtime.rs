use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{Map, Value};
use uuid::Uuid;
use woml_engine::{
  execute_workflow_durable, CompiledCorrectionIdentity, CompiledWorkflowDefinition,
  DurableEventStore, RunEventPayload, RuntimeExecutionError, RuntimeExecutionOptions,
  SagaCorrectionReason, ScriptHostProcessOptions,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/compensation-and-cleanup/model.v19.reviewed.json");
const HASH: &str = "sha256:1919191919191919191919191919191919191919191919191919191919191919";

fn model() -> CompiledWorkflowDefinition {
  serde_json::from_str(MODEL).expect("reviewed Model v19 fixture")
}

fn compile_reviewed_woml() -> Option<CompiledWorkflowDefinition> {
  runtime_options()?;
  let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let fixture = repository.join("woml/tests/fixtures/compensation-and-cleanup/model.reviewed.woml");
  let output = std::process::Command::new("bun")
    .current_dir(&repository)
    .env("WOML_FIXTURE", fixture)
    .args([
      "-e",
      "import {readFileSync} from 'node:fs'; import {parseWoml,compileWoml} from './woml/src/index.ts'; process.stdout.write(JSON.stringify(compileWoml(parseWoml(readFileSync(process.env.WOML_FIXTURE,'utf8')))));",
    ])
    .output()
    .ok()?;
  output
    .status
    .success()
    .then(|| serde_json::from_slice(&output.stdout).ok())
    .flatten()
}

fn two_effect_failure_model() -> CompiledWorkflowDefinition {
  let mut workflow = model();
  let mut verification = workflow
    .node("deployApplication")
    .expect("deployment node")
    .clone();
  verification.id = "verifyDeployment".to_string();
  let woml_engine::model::ValueExpression::Object { fields } = &mut verification.inputs else {
    panic!("verification inputs")
  };
  fields.insert(
    "source".to_string(),
    woml_engine::model::ValueExpression::Literal {
      value: Value::String("throw new Error('deployment rejected');".to_string()),
    },
  );
  workflow.graph.nodes.push(verification);
  let commit_edge = workflow
    .graph
    .edges
    .iter_mut()
    .find(|edge| edge.from == "deployApplication")
    .expect("saga commit edge");
  commit_edge.id = "verifyDeployment-to-saga-commit".to_string();
  commit_edge.from = "verifyDeployment".to_string();
  workflow
    .graph
    .edges
    .push(woml_engine::model::CompiledWorkflowEdge {
      id: "deployApplication-to-verifyDeployment".to_string(),
      from: "deployApplication".to_string(),
      to: "verifyDeployment".to_string(),
      condition: woml_engine::model::EdgeCondition::Always,
      branch_id: None,
      parallel_id: None,
      approval_id: None,
    });
  workflow
    .graph
    .context_visibility
    .as_mut()
    .expect("Model v19 visibility")
    .push(woml_engine::model::CompiledContextVisibility {
      node_id: "verifyDeployment".to_string(),
      step_ids: vec![
        "prepare".to_string(),
        "createNetwork".to_string(),
        "deployApplication".to_string(),
      ],
    });
  let saga = workflow
    .graph
    .sagas
    .as_mut()
    .and_then(|sagas| sagas.first_mut())
    .expect("reviewed saga");
  saga.body_terminal_node_id = "verifyDeployment".to_string();
  let mut deployment_compensation = saga.compensations[0].clone();
  deployment_compensation.action_id = "provision:compensate:deployApplication".to_string();
  deployment_compensation.for_step_id = Some("deployApplication".to_string());
  deployment_compensation.identity = CompiledCorrectionIdentity::Static {
    action_id: deployment_compensation.action_id.clone(),
  };
  deployment_compensation.node.id = deployment_compensation.action_id.clone();
  deployment_compensation.visible_step_ids = vec![
    "prepare".to_string(),
    "createNetwork".to_string(),
    "deployApplication".to_string(),
  ];
  let woml_engine::model::ValueExpression::Object { fields } =
    &mut deployment_compensation.node.inputs
  else {
    panic!("compensation inputs")
  };
  fields.insert(
    "source".to_string(),
    woml_engine::model::ValueExpression::Literal {
      value: Value::String(
        "return { removed: context.steps.deployApplication.deployed };".to_string(),
      ),
    },
  );
  saga.compensations.push(deployment_compensation);
  workflow
}

struct TemporaryDatabase(PathBuf);

impl TemporaryDatabase {
  fn new(label: &str) -> Self {
    Self(std::env::temp_dir().join(format!(
      "woml-compensation-{label}-{}.sqlite",
      Uuid::new_v4().simple()
    )))
  }

  fn path(&self) -> &Path {
    &self.0
  }
}

impl Drop for TemporaryDatabase {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.0);
    let _ = std::fs::remove_file(self.0.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(self.0.with_extension("sqlite-wal"));
  }
}

fn runtime_options() -> Option<RuntimeExecutionOptions> {
  std::process::Command::new("bun")
    .arg("--version")
    .output()
    .ok()?
    .status
    .success()
    .then_some(())?;
  let host = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../woml-cli/src/script-host.ts");
  host.exists().then(|| {
    RuntimeExecutionOptions::new(
      ScriptHostProcessOptions::new(PathBuf::from("bun"), host),
      5_000,
    )
    .with_shared_script_hosts()
  })
}

#[tokio::test]
async fn successful_saga_runs_cleanup_without_compensation_and_continues() {
  let Some(options) = runtime_options() else {
    return;
  };
  let database = TemporaryDatabase::new("success");
  let result = execute_workflow_durable(
    model(),
    HASH.to_string(),
    Map::new(),
    options,
    database.path().to_path_buf(),
  )
  .await
  .expect("successful saga");
  assert_eq!(result.result, serde_json::json!({"provisioned": true}));

  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&result.run_id).unwrap();
  assert!(events.iter().any(|event| matches!(
    &event.payload,
    RunEventPayload::SagaCorrectionStarted(data)
      if data.reason == SagaCorrectionReason::Success
  )));
  assert!(!events.iter().any(|event| matches!(
    &event.payload,
    RunEventPayload::CorrectionAttemptStarted(data)
      if data.action_id == "provision:compensate:createNetwork"
  )));
  assert!(events.iter().any(|event| matches!(
    &event.payload,
    RunEventPayload::CorrectionAttemptSucceeded(data)
      if data.action_id == "provision:cleanup:revokeCredentials"
  )));
  assert!(events
    .iter()
    .any(|event| matches!(event.payload, RunEventPayload::SagaCorrectionCompleted(_))));
  let inspection = store.inspect_correction_v1(&result.run_id).unwrap();
  assert_eq!(inspection.phase, "completed");
  assert_eq!(inspection.original_outcome, "succeeded");
  assert_eq!(inspection.sagas[0].completed_cleanup, 1);
  assert!(inspection.unresolved_actions.is_empty());
}

#[tokio::test]
async fn real_woml_text_compiles_to_model_v19_and_executes_through_rust() {
  let Some(workflow) = compile_reviewed_woml() else {
    return;
  };
  let Some(options) = runtime_options() else {
    return;
  };
  assert_eq!(workflow.schema_version, 19);
  let database = TemporaryDatabase::new("real-woml");
  let result = execute_workflow_durable(
    workflow,
    "sha256:3939393939393939393939393939393939393939393939393939393939393939".to_string(),
    Map::new(),
    options,
    database.path().to_path_buf(),
  )
  .await
  .expect("compiled WOML saga executes");
  assert_eq!(result.result, serde_json::json!({"provisioned": true}));
}

#[tokio::test]
async fn failed_saga_compensates_reverse_completion_then_cleans_up_and_keeps_primary_failure() {
  let Some(options) = runtime_options() else {
    return;
  };
  let database = TemporaryDatabase::new("failure");
  let workflow = two_effect_failure_model();
  let error = execute_workflow_durable(
    workflow,
    HASH.to_string(),
    Map::new(),
    options,
    database.path().to_path_buf(),
  )
  .await
  .expect_err("forward failure remains primary");
  let RuntimeExecutionError::RunFailed(details) = error else {
    panic!("expected durable run failure, received {error:?}")
  };
  assert_eq!(details.node_id.as_deref(), Some("verifyDeployment"));
  assert!(details.message.contains("deployment rejected"));

  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&details.events[0].run_id).unwrap();
  let corrective_order = events
    .iter()
    .filter_map(|event| match &event.payload {
      RunEventPayload::CorrectionAttemptStarted(data) => Some(data.action_id.as_str()),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(
    corrective_order,
    vec![
      "provision:compensate:deployApplication",
      "provision:compensate:createNetwork",
      "provision:cleanup:revokeCredentials"
    ]
  );
  assert!(events.iter().any(|event| matches!(
    &event.payload,
    RunEventPayload::CompensationRegistered(data)
      if data.for_step_id == "createNetwork"
  )));
  assert!(events
    .iter()
    .any(|event| matches!(event.payload, RunEventPayload::SagaCorrectionCompleted(_))));
  let inspection = store
    .inspect_correction_v1(&details.events[0].run_id)
    .unwrap();
  assert_eq!(inspection.phase, "completed");
  assert_eq!(inspection.original_outcome, "failed");
  assert_eq!(inspection.sagas[0].registered_compensations, 2);
  assert_eq!(inspection.sagas[0].completed_compensations, 2);
  assert_eq!(inspection.sagas[0].completed_cleanup, 1);
  drop(store);

  let connection = Connection::open(database.path()).unwrap();
  connection
    .execute("DELETE FROM woml_correction_attempts", [])
    .unwrap();
  connection
    .execute("DELETE FROM woml_correction_actions", [])
    .unwrap();
  connection
    .execute("DELETE FROM woml_saga_scopes", [])
    .unwrap();
  drop(connection);
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  let rebuilt_actions: i64 = connection
    .query_row("SELECT COUNT(*) FROM woml_correction_actions", [], |row| {
      row.get(0)
    })
    .unwrap();
  assert_eq!(rebuilt_actions, 3);
}
