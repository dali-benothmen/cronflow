use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use chrono::{Duration, TimeZone, Utc};
use rusqlite::Connection;
use serde_json::Map;
use uuid::Uuid;
use woml_engine::model::ValueExpression;
use woml_engine::{
  execute_workflow_durable_outcome, resume_due_timer_waits_durable, CompiledWait,
  CompiledWorkflowDefinition, DurableEventStore, DurableWaitStatus, FixedEngineClock,
  RunEventPayload, RunStatus, RuntimeExecutionOptions, ScriptHostProcessOptions,
  TimerWaitSettlement, WorkflowRuntimeOutcome, DURABLE_STORE_SCHEMA_VERSION,
};

const WAIT_MODEL: &str =
  include_str!("../../../woml/tests/fixtures/durable-waits/model.v17.reviewed.json");
const WAIT_MODEL_HASH: &str =
  "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn wait_model() -> CompiledWorkflowDefinition {
  serde_json::from_str(WAIT_MODEL).expect("the reviewed Model v17 fixture must decode")
}

fn unavailable_host_options(now: chrono::DateTime<Utc>) -> RuntimeExecutionOptions {
  RuntimeExecutionOptions::new(
    ScriptHostProcessOptions::new(
      PathBuf::from("/woml-dw3-no-such-bun"),
      PathBuf::from("/woml-dw3-no-such-host.ts"),
    ),
    2_000,
  )
  .with_clock(Arc::new(FixedEngineClock::new(now)))
}

struct TemporaryDatabase {
  root: PathBuf,
  path: PathBuf,
}

impl TemporaryDatabase {
  fn new(label: &str) -> Self {
    let root = std::env::temp_dir().join(format!("woml-dw3-{label}-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("state.sqlite");
    Self { root, path }
  }

  fn path(&self) -> &Path {
    &self.path
  }
}

impl Drop for TemporaryDatabase {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.root);
  }
}

#[test]
fn model_v17_and_store_v15_are_strict_durable_wait_contracts() {
  let workflow = wait_model();
  workflow.validate_for_durable_execution().unwrap();
  assert_eq!(workflow.schema_version, 17);
  assert_eq!(workflow.graph.waits.as_ref().map(Vec::len), Some(1));

  let database = TemporaryDatabase::new("contracts");
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  let schema_version: String = connection
    .query_row(
      "SELECT value FROM woml_store_metadata WHERE key = 'schema_version'",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(schema_version, DURABLE_STORE_SCHEMA_VERSION.to_string());
  for table in [
    "woml_durable_waits",
    "woml_external_signals",
    "woml_signal_idempotency",
  ] {
    let exists: i64 = connection
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(exists, 1, "missing Store v15 table {table}");
  }
  for index in [
    "woml_durable_waits_due",
    "woml_durable_waits_active_target",
    "woml_external_signals_target",
  ] {
    let exists: i64 = connection
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [index],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(exists, 1, "missing Store v15 index {index}");
  }
}

#[tokio::test]
async fn timer_wait_persists_before_releasing_execution_resources() {
  let database = TemporaryDatabase::new("registration");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 8, 0, 0).unwrap();
  let outcome = execute_workflow_durable_outcome(
    wait_model(),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();

  let WorkflowRuntimeOutcome::WaitingTime { run_id, wait, .. } = outcome else {
    panic!("a future timer must return the durable waiting-time outcome");
  };
  assert_eq!(wait.wait_id, "pause");
  assert_eq!(wait.deadline, started_at + Duration::seconds(1));

  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&run_id).unwrap();
  assert!(events.iter().any(|event| matches!(
    event.payload,
    RunEventPayload::RunStarted(_) | RunEventPayload::RunExecutionStarted(_)
  )));
  assert!(events
    .iter()
    .any(|event| matches!(event.payload, RunEventPayload::WaitRegistered(_))));
  assert!(!events.iter().any(|event| matches!(
    event.payload,
    RunEventPayload::StepAttemptStarted(_)
      | RunEventPayload::StepAttemptSucceeded(_)
      | RunEventPayload::StepAttemptFailed(_)
  )));
  let projection = store.projection(&run_id).unwrap();
  assert_eq!(projection.status, RunStatus::Waiting);
  assert_eq!(
    projection.durable_waits["pause"].status,
    DurableWaitStatus::Registered
  );
  assert!(store.due_timer_waits(started_at, 10).unwrap().is_empty());
  assert_eq!(
    store
      .due_timer_waits(started_at + Duration::seconds(1), 10)
      .unwrap()
      .len(),
    1
  );
}

#[tokio::test]
async fn overdue_timer_recovers_after_restart_and_resumes_exactly_once() {
  let database = TemporaryDatabase::new("restart");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).unwrap();
  let waiting = execute_workflow_durable_outcome(
    wait_model(),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::WaitingTime { run_id, .. } = waiting else {
    panic!("the first runtime process must release a future wait");
  };

  // A new runtime instance uses only Store v15 and Event v16 to discover the
  // overdue wait; it does not retain an in-memory timer or a Bun worker.
  let resumed_at = started_at + Duration::seconds(2);
  let outcomes = resume_due_timer_waits_durable(
    database.path().to_path_buf(),
    unavailable_host_options(resumed_at),
    100,
  )
  .await
  .unwrap();
  assert_eq!(outcomes.len(), 1);
  let WorkflowRuntimeOutcome::Succeeded { execution, .. } = &outcomes[0] else {
    panic!("the winning timer settlement must continue the same run");
  };
  assert_eq!(execution.run_id, run_id);
  assert_eq!(execution.execution_order, ["pause"]);
  assert_eq!(execution.result["status"], "elapsed");
  assert_eq!(execution.result["scheduledAt"], "2026-10-13T09:00:01.000Z");
  assert_eq!(execution.result["resumedAt"], "2026-10-13T09:00:02.000Z");

  let losing_scan = resume_due_timer_waits_durable(
    database.path().to_path_buf(),
    unavailable_host_options(resumed_at + Duration::seconds(1)),
    100,
  )
  .await
  .unwrap();
  assert!(losing_scan.is_empty());

  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&run_id).unwrap();
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::WaitSatisfied(_)))
      .count(),
    1
  );
  let projection = store.projection(&run_id).unwrap();
  assert_eq!(projection.status, RunStatus::Succeeded);
  assert!(matches!(
    projection.durable_waits["pause"].status,
    DurableWaitStatus::Satisfied { .. }
  ));
}

#[tokio::test]
async fn wait_until_in_the_past_records_registration_and_resolution_without_a_worker() {
  let database = TemporaryDatabase::new("past-until");
  let now = Utc.with_ymd_and_hms(2026, 10, 13, 10, 0, 0).unwrap();
  let mut workflow = wait_model();
  workflow.graph.waits = Some(vec![CompiledWait::Until {
    wait_id: "pause".to_string(),
    node_id: "pause".to_string(),
    metadata: workflow.graph.nodes[0].metadata.clone(),
    at: ValueExpression::Literal {
      value: serde_json::json!("2026-10-13T09:59:59Z"),
    },
  }]);
  workflow.validate_for_durable_execution().unwrap();

  let outcome = execute_workflow_durable_outcome(
    workflow,
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(now),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::Succeeded { execution, .. } = outcome else {
    panic!("a past wait-until deadline must settle immediately");
  };
  assert_eq!(execution.execution_order, ["pause"]);
  assert_eq!(execution.result["status"], "elapsed");
  assert_eq!(
    execution
      .events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::WaitRegistered(_)))
      .count(),
    1
  );
  assert_eq!(
    execution
      .events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::WaitSatisfied(_)))
      .count(),
    1
  );
}

#[tokio::test]
async fn concurrent_timer_settlement_has_exactly_one_winner() {
  let database = TemporaryDatabase::new("one-winner");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 11, 0, 0).unwrap();
  let waiting = execute_workflow_durable_outcome(
    wait_model(),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::WaitingTime { run_id, .. } = waiting else {
    panic!("the timer must be registered before the settlement race");
  };

  let barrier = Arc::new(Barrier::new(2));
  let mut workers = Vec::new();
  for _ in 0..2 {
    let barrier = Arc::clone(&barrier);
    let path = database.path().to_path_buf();
    let run_id = run_id.clone();
    workers.push(std::thread::spawn(move || {
      let mut store = DurableEventStore::open(path).unwrap();
      barrier.wait();
      store
        .settle_timer_wait(&run_id, "pause", started_at + Duration::seconds(2))
        .unwrap()
    }));
  }
  let settlements = workers
    .into_iter()
    .map(|worker| worker.join().unwrap())
    .collect::<Vec<_>>();
  assert_eq!(
    settlements
      .iter()
      .filter(|settlement| matches!(settlement, TimerWaitSettlement::Resolved { .. }))
      .count(),
    1
  );
  assert_eq!(
    settlements
      .iter()
      .filter(|settlement| matches!(settlement, TimerWaitSettlement::AlreadyResolved { .. }))
      .count(),
    1
  );

  let store = DurableEventStore::open(database.path()).unwrap();
  assert_eq!(
    store
      .events(&run_id)
      .unwrap()
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::WaitSatisfied(_)))
      .count(),
    1
  );
}
