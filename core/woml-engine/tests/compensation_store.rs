use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::{json, Map};
use uuid::Uuid;
use woml_engine::{
  CompiledWorkflowDefinition, DurableDagEngine, DurableEngineError, DurableEventStore,
  DurableStoreError, RunEventPayload, SagaIdentityData, StepAttemptSucceededData,
  TriggerAdmissionRequest, DURABLE_STORE_SCHEMA_VERSION,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/compensation-and-cleanup/model.v19.reviewed.json");
const HASH: &str = "sha256:2929292929292929292929292929292929292929292929292929292929292929";

struct TemporaryDatabase(PathBuf);

impl TemporaryDatabase {
  fn new(label: &str) -> Self {
    Self(std::env::temp_dir().join(format!(
      "woml-compensation-store-{label}-{}.sqlite",
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

fn object_exists(connection: &Connection, kind: &str, name: &str) -> bool {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2)",
      params![kind, name],
      |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn store_v16_migrates_additively_to_the_frozen_v17_schema() {
  let database = TemporaryDatabase::new("migration");
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  for table in [
    "woml_correction_manual_recovery",
    "woml_correction_attempts",
    "woml_correction_actions",
    "woml_saga_scopes",
  ] {
    connection
      .execute(&format!("DROP TABLE {table}"), [])
      .unwrap();
  }
  connection
    .execute(
      "UPDATE woml_store_metadata SET value = '16' WHERE key = 'schema_version'",
      [],
    )
    .unwrap();
  drop(connection);

  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  let version: String = connection
    .query_row(
      "SELECT value FROM woml_store_metadata WHERE key = 'schema_version'",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(version, DURABLE_STORE_SCHEMA_VERSION.to_string());
  for table in [
    "woml_saga_scopes",
    "woml_correction_actions",
    "woml_correction_attempts",
    "woml_correction_manual_recovery",
  ] {
    assert!(
      object_exists(&connection, "table", table),
      "missing {table}"
    );
  }
  for index in [
    "woml_correction_actions_pending",
    "woml_correction_attempts_active",
    "woml_correction_manual_recovery_pending",
  ] {
    assert!(
      object_exists(&connection, "index", index),
      "missing {index}"
    );
  }
}

#[test]
fn future_store_versions_fail_closed() {
  let database = TemporaryDatabase::new("future");
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  connection
    .execute(
      "UPDATE woml_store_metadata SET value = '18' WHERE key = 'schema_version'",
      [],
    )
    .unwrap();
  drop(connection);
  assert!(matches!(
    DurableEventStore::open(database.path()),
    Err(DurableStoreError::UnsupportedStoreVersion(version)) if version == "18"
  ));
}

#[test]
fn malformed_store_v17_schema_fails_closed() {
  let database = TemporaryDatabase::new("missing-index");
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  connection
    .execute("DROP INDEX woml_correction_actions_pending", [])
    .unwrap();
  drop(connection);
  assert!(matches!(
    DurableEventStore::open(database.path()),
    Err(DurableStoreError::Contract(message))
      if message.contains("woml_correction_actions_pending")
  ));
}

#[test]
fn invalid_registration_rolls_back_the_forward_success_atomically() {
  let database = TemporaryDatabase::new("atomic-registration");
  let workflow: CompiledWorkflowDefinition = serde_json::from_str(MODEL).unwrap();
  let now = chrono::Utc::now();
  let mut store = DurableEventStore::open(database.path()).unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: HASH.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: "atomic-registration-test".to_string(),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "atomic-registration-owner",
      now,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  engine
    .start_step_attempt(&run_id, "prepare", 1, "inv-prepare", now)
    .unwrap();
  engine
    .append_payload(
      "evt-prepare-succeeded",
      &run_id,
      now,
      RunEventPayload::StepAttemptSucceeded(StepAttemptSucceededData {
        node_id: "prepare".to_string(),
        attempt: 1,
        invocation_id: "inv-prepare".to_string(),
        output: json!({"pullRequest": 42}),
      }),
    )
    .unwrap();
  engine
    .append_payload(
      "evt-saga-started",
      &run_id,
      now,
      RunEventPayload::SagaStarted(SagaIdentityData {
        saga_id: "provision".to_string(),
      }),
    )
    .unwrap();
  engine
    .start_step_attempt(&run_id, "createNetwork", 1, "inv-network", now)
    .unwrap();
  let error = engine
    .record_step_success_with_compensation(
      &run_id,
      StepAttemptSucceededData {
        node_id: "createNetwork".to_string(),
        attempt: 1,
        invocation_id: "inv-network".to_string(),
        output: json!({"networkId": "network-42"}),
      },
      "provision",
      "provision:compensate:notCompiled",
    )
    .unwrap_err();
  assert!(matches!(
    error,
    DurableEngineError::Store(DurableStoreError::Contract(_))
  ));
  let projection = engine.projection(&run_id).unwrap();
  assert!(!projection.context.steps.contains_key("createNetwork"));
  assert!(
    !engine.events(&run_id).unwrap().iter().any(|event| matches!(
      &event.payload,
      RunEventPayload::StepAttemptSucceeded(data) if data.node_id == "createNetwork"
    ))
  );
}
