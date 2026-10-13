use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use chrono::{Duration, TimeZone, Utc};
use rusqlite::Connection;
use serde_json::Map;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use woml_engine::model::ValueExpression;
use woml_engine::{
  admit_external_signal_durable, execute_workflow_durable_outcome, resume_due_timer_waits_durable,
  resume_workflow_durable_outcome, CompiledWait, CompiledWaitSignal, CompiledWorkflowDefinition,
  DurableDagEngine, DurableEventStore, DurableWaitKind, DurableWaitStatus,
  ExternalSignalAdmissionRequest, ExternalSignalAdmissionStatus, ExternalSignalResolution,
  FixedEngineClock, InspectedWaitStatusV1, RunCancellationStatus, RunEventPayload, RunStatus,
  RuntimeExecutionOptions, ScriptHostProcessOptions, SignalWaitTimeoutSettlement,
  TimerWaitSettlement, TriggerAdmissionRequest, WaitRegisteredData, WorkflowRuntimeOutcome,
  DURABLE_STORE_SCHEMA_VERSION,
};

const WAIT_MODEL: &str =
  include_str!("../../../woml/tests/fixtures/durable-waits/model.v17.reviewed.json");
const WAIT_MODEL_HASH: &str =
  "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn wait_model() -> CompiledWorkflowDefinition {
  serde_json::from_str(WAIT_MODEL).expect("the reviewed Model v17 fixture must decode")
}

fn signal_model(on_timeout: &str) -> CompiledWorkflowDefinition {
  let mut workflow = wait_model();
  workflow.graph.waits = Some(vec![CompiledWait::Signal {
    wait_id: "pause".to_string(),
    node_id: "pause".to_string(),
    metadata: workflow.graph.nodes[0].metadata.clone(),
    signals: vec![CompiledWaitSignal {
      signal: "job.completed".to_string(),
      schema: serde_json::json!({
        "type": "object",
        "properties": { "status": { "type": "string" } },
        "required": ["status"],
        "additionalProperties": false
      }),
    }],
    correlation: ValueExpression::Literal {
      value: serde_json::json!("job-42"),
    },
    timeout_ms: Some(1_000),
    on_timeout: on_timeout.to_string(),
  }]);
  workflow.validate_for_durable_execution().unwrap();
  workflow
}

fn signal_request(
  run_id: &str,
  idempotency_key: &str,
  received_at: chrono::DateTime<Utc>,
) -> ExternalSignalAdmissionRequest {
  ExternalSignalAdmissionRequest {
    profile: woml_engine::EXTERNAL_SIGNAL_ADMISSION_PROFILE.to_string(),
    signal: "job.completed".to_string(),
    run_id: run_id.to_string(),
    wait_id: "pause".to_string(),
    correlation: "job-42".to_string(),
    idempotency_key: idempotency_key.to_string(),
    payload: serde_json::json!({ "status": "passed" }),
    received_at,
    authentication_class: "test".to_string(),
  }
}

fn sha256_prefixed(value: &str) -> String {
  format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
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
  let inspection = store.inspect_waits_v1(&run_id).unwrap();
  assert_eq!(inspection.profile, "woml.wait-inspection/v1");
  assert_eq!(inspection.pending_waits, 1);
  assert_eq!(inspection.waits.len(), 1);
  assert_eq!(inspection.waits[0].wait_id, "pause");
  assert_eq!(inspection.waits[0].status, InspectedWaitStatusV1::Pending);
  assert!(inspection.waits[0].correlation_digest.is_none());
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

#[tokio::test]
async fn active_signal_resumes_once_and_preserves_idempotency_results() {
  let database = TemporaryDatabase::new("active-signal");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap();
  let waiting = execute_workflow_durable_outcome(
    signal_model("continue"),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::WaitingSignal { run_id, wait, .. } = waiting else {
    panic!("a signal wait must release execution resources");
  };
  assert_eq!(wait.signals, ["job.completed"]);

  let request = signal_request(
    &run_id,
    "callback-1",
    started_at + Duration::milliseconds(50),
  );
  let admitted = admit_external_signal_durable(
    database.path().to_path_buf(),
    request.clone(),
    unavailable_host_options(started_at + Duration::milliseconds(50)),
  )
  .await
  .unwrap();
  assert_eq!(
    admitted.admission.status,
    ExternalSignalAdmissionStatus::Accepted
  );
  assert_eq!(
    admitted.admission.resolution,
    Some(ExternalSignalResolution::Resolved)
  );
  let Some(Ok(WorkflowRuntimeOutcome::Succeeded { execution, .. })) = admitted.continuation else {
    panic!("the accepted active signal must continue the same run");
  };
  assert_eq!(execution.run_id, run_id);
  assert_eq!(execution.result["status"], "signaled");
  assert_eq!(execution.result["signal"], "job.completed");
  assert_eq!(execution.result["correlation"], "job-42");
  assert_eq!(execution.result["payload"]["status"], "passed");

  let duplicate = admit_external_signal_durable(
    database.path().to_path_buf(),
    request.clone(),
    unavailable_host_options(started_at + Duration::milliseconds(60)),
  )
  .await
  .unwrap();
  assert_eq!(
    duplicate.admission.status,
    ExternalSignalAdmissionStatus::Duplicate
  );
  assert!(duplicate.continuation.is_none());

  let mut conflicting = request;
  conflicting.payload = serde_json::json!({ "status": "failed" });
  let conflict = admit_external_signal_durable(
    database.path().to_path_buf(),
    conflicting,
    unavailable_host_options(started_at + Duration::milliseconds(70)),
  )
  .await
  .unwrap();
  assert_eq!(
    conflict.admission.status,
    ExternalSignalAdmissionStatus::IdentityConflict
  );

  let late = admit_external_signal_durable(
    database.path().to_path_buf(),
    signal_request(
      &run_id,
      "callback-2",
      started_at + Duration::milliseconds(80),
    ),
    unavailable_host_options(started_at + Duration::milliseconds(80)),
  )
  .await
  .unwrap();
  assert_eq!(
    late.admission.status,
    ExternalSignalAdmissionStatus::AlreadyResolved
  );
}

#[tokio::test]
async fn early_signal_is_consumed_atomically_when_the_wait_registers() {
  let database = TemporaryDatabase::new("early-signal");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 13, 0, 0).unwrap();
  let workflow = signal_model("continue");
  let mut store = DurableEventStore::open(database.path()).unwrap();
  store
    .register_definition(&workflow, WAIT_MODEL_HASH)
    .unwrap();
  let admission = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: WAIT_MODEL_HASH.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: "early-signal-test".to_string(),
      payload: Map::new(),
      received_at: started_at,
    })
    .unwrap();
  let run_id = admission.run_id;
  let claim = store
    .claim_policy_run(
      &run_id,
      "early-signal-owner",
      started_at,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let early = store
    .admit_external_signal_atomically(
      signal_request(
        &run_id,
        "callback-early",
        started_at + Duration::milliseconds(10),
      ),
      &[],
    )
    .unwrap();
  assert_eq!(early.status, ExternalSignalAdmissionStatus::Accepted);
  assert_eq!(early.resolution, Some(ExternalSignalResolution::Early));

  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  let projection = engine
    .register_durable_wait(
      &run_id,
      started_at + Duration::milliseconds(20),
      WaitRegisteredData {
        wait_id: "pause".to_string(),
        node_id: "pause".to_string(),
        kind: DurableWaitKind::Signal,
        registered_at: started_at + Duration::milliseconds(20),
        deadline: Some(started_at + Duration::seconds(1)),
        signals: Some(vec!["job.completed".to_string()]),
        correlation_digest: Some(sha256_prefixed("job-42")),
      },
    )
    .unwrap();
  assert_eq!(projection.status, RunStatus::Running);
  assert_eq!(projection.context.steps["pause"]["status"], "signaled");
  assert!(matches!(
    projection.durable_waits["pause"].status,
    DurableWaitStatus::Satisfied { .. }
  ));

  let mut store = engine.into_store();
  assert!(store
    .release_policy_claim(&run_id, &claim.owner_id, &claim.claim_id)
    .unwrap());
  drop(store);
  let resumed = resume_workflow_durable_outcome(
    database.path().to_path_buf(),
    &run_id,
    unavailable_host_options(started_at + Duration::milliseconds(30)),
  )
  .await
  .unwrap();
  assert!(matches!(resumed, WorkflowRuntimeOutcome::Succeeded { .. }));
}

#[tokio::test]
async fn signal_timeout_continue_publishes_result_and_fail_decides_run_outcome() {
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 14, 0, 0).unwrap();
  for (behavior, should_continue) in [("continue", true), ("fail", false)] {
    let database = TemporaryDatabase::new(behavior);
    let waiting = execute_workflow_durable_outcome(
      signal_model(behavior),
      WAIT_MODEL_HASH.to_string(),
      Map::new(),
      unavailable_host_options(started_at),
      database.path().to_path_buf(),
    )
    .await
    .unwrap();
    let WorkflowRuntimeOutcome::WaitingSignal { run_id, .. } = waiting else {
      panic!("signal workflow must wait before timing out");
    };
    let mut store = DurableEventStore::open(database.path()).unwrap();
    let settlement = store
      .settle_signal_wait_timeout(&run_id, "pause", started_at + Duration::seconds(1))
      .unwrap();
    if should_continue {
      let SignalWaitTimeoutSettlement::Continued { projection } = settlement else {
        panic!("continue timeout must reopen the DAG");
      };
      assert_eq!(projection.context.steps["pause"]["status"], "timed_out");
      drop(store);
      let resumed = resume_workflow_durable_outcome(
        database.path().to_path_buf(),
        &run_id,
        unavailable_host_options(started_at + Duration::seconds(1)),
      )
      .await
      .unwrap();
      assert!(matches!(resumed, WorkflowRuntimeOutcome::Succeeded { .. }));
    } else {
      let SignalWaitTimeoutSettlement::Failed { projection } = settlement else {
        panic!("fail timeout must decide the run failure");
      };
      assert!(projection.business_outcome.is_some());
      assert!(!projection.context.steps.contains_key("pause"));
    }
  }
}

#[tokio::test]
async fn cancellation_wins_atomically_over_a_late_signal() {
  let database = TemporaryDatabase::new("cancel-signal-race");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 15, 0, 0).unwrap();
  let waiting = execute_workflow_durable_outcome(
    signal_model("continue"),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::WaitingSignal { run_id, .. } = waiting else {
    panic!("signal workflow must wait before cancellation");
  };
  let mut store = DurableEventStore::open(database.path()).unwrap();
  let cancellation = store
    .request_run_cancellation(
      &run_id,
      "cancel_signal_wait",
      started_at + Duration::milliseconds(10),
    )
    .unwrap();
  assert_eq!(cancellation.status, RunCancellationStatus::Accepted);
  drop(store);

  let late = admit_external_signal_durable(
    database.path().to_path_buf(),
    signal_request(
      &run_id,
      "callback-after-cancel",
      started_at + Duration::milliseconds(20),
    ),
    unavailable_host_options(started_at + Duration::milliseconds(20)),
  )
  .await
  .unwrap();
  assert_eq!(
    late.admission.status,
    ExternalSignalAdmissionStatus::AlreadyResolved
  );
  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&run_id).unwrap();
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::WaitCancelled(_)))
      .count(),
    1
  );
  assert!(!events
    .iter()
    .any(|event| matches!(event.payload, RunEventPayload::ExternalSignalReceived(_))));
}

#[tokio::test]
async fn concurrent_signal_and_timeout_have_exactly_one_durable_winner() {
  let database = TemporaryDatabase::new("signal-timeout-race");
  let started_at = Utc.with_ymd_and_hms(2026, 10, 13, 16, 0, 0).unwrap();
  let waiting = execute_workflow_durable_outcome(
    signal_model("continue"),
    WAIT_MODEL_HASH.to_string(),
    Map::new(),
    unavailable_host_options(started_at),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::WaitingSignal { run_id, .. } = waiting else {
    panic!("signal workflow must wait before the race");
  };
  let barrier = Arc::new(Barrier::new(2));
  let signal_worker = {
    let barrier = Arc::clone(&barrier);
    let path = database.path().to_path_buf();
    let run_id = run_id.clone();
    std::thread::spawn(move || {
      let mut store = DurableEventStore::open(path).unwrap();
      barrier.wait();
      store
        .admit_external_signal_atomically(
          signal_request(&run_id, "callback-race", started_at + Duration::seconds(1)),
          &[],
        )
        .unwrap()
        .status
    })
  };
  let timeout_worker = {
    let barrier = Arc::clone(&barrier);
    let path = database.path().to_path_buf();
    let run_id = run_id.clone();
    std::thread::spawn(move || {
      let mut store = DurableEventStore::open(path).unwrap();
      barrier.wait();
      store
        .settle_signal_wait_timeout(&run_id, "pause", started_at + Duration::seconds(1))
        .unwrap()
    })
  };
  let signal_status = signal_worker.join().unwrap();
  let timeout_status = timeout_worker.join().unwrap();
  assert!(matches!(
    (signal_status, timeout_status),
    (
      ExternalSignalAdmissionStatus::Accepted,
      SignalWaitTimeoutSettlement::AlreadyResolved { .. }
    ) | (
      ExternalSignalAdmissionStatus::AlreadyResolved,
      SignalWaitTimeoutSettlement::Continued { .. }
    )
  ));

  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events(&run_id).unwrap();
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(
        event.payload,
        RunEventPayload::WaitSatisfied(_) | RunEventPayload::WaitTimedOut(_)
      ))
      .count(),
    1
  );
}
