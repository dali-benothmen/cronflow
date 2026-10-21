use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Map};
use uuid::Uuid;
use woml_engine::{
  create_online_backup, fold_events, step_effect_idempotency_key, CompensationRegisteredData,
  CompiledWorkflowDefinition, CorrectionAttemptIdentityData, CorrectionRecoveryActionV1,
  CorrectionRecoveryRequestV1, CorrectionRecoveryStatusV1, CorrectiveActionKind, DurableDagEngine,
  DurableEngineError, DurableEventStore, DurableStoreError, QueueDiscipline, RunAdmissionQueue,
  RunAdmissionTrigger, RunAdmittedData, RunEvent, RunEventPayload, RunExecutionStartedData,
  SagaCorrectionReason, SagaCorrectionStartedData, SagaIdentityData, StepAttemptStartedData,
  StepAttemptSucceededData, TriggerAdmissionRequest, CORRECTION_RECOVERY_REQUEST_PROFILE,
  DURABLE_STORE_SCHEMA_VERSION, RUN_EVENT_SCHEMA_VERSION_V18,
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

fn interrupted_correction(database: &Path) -> (String, &'static str) {
  let workflow: CompiledWorkflowDefinition = serde_json::from_str(MODEL).unwrap();
  let now = Utc::now();
  let mut store = DurableEventStore::open(database).unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: HASH.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: format!("correction-recovery-{}", Uuid::new_v4()),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "correction-recovery-owner",
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
  engine
    .record_step_success_with_compensation(
      &run_id,
      StepAttemptSucceededData {
        node_id: "createNetwork".to_string(),
        attempt: 1,
        invocation_id: "inv-network".to_string(),
        output: json!({"networkId": "network-42"}),
      },
      "provision",
      "provision:compensate:createNetwork",
    )
    .unwrap();
  engine
    .append_payload(
      "evt-correction-started",
      &run_id,
      now,
      RunEventPayload::SagaCorrectionStarted(SagaCorrectionStartedData {
        saga_id: "provision".to_string(),
        reason: SagaCorrectionReason::Failure,
      }),
    )
    .unwrap();
  engine
    .append_payload(
      "evt-correction-attempt-started",
      &run_id,
      now,
      RunEventPayload::CorrectionAttemptStarted(CorrectionAttemptIdentityData {
        saga_id: "provision".to_string(),
        action_id: "provision:compensate:createNetwork".to_string(),
        action_kind: CorrectiveActionKind::Compensation,
        attempt: 1,
      }),
    )
    .unwrap();
  drop(engine);
  (run_id, "provision:compensate:createNetwork")
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
      "UPDATE woml_store_metadata SET value = '19' WHERE key = 'schema_version'",
      [],
    )
    .unwrap();
  drop(connection);
  assert!(matches!(
    DurableEventStore::open(database.path()),
    Err(DurableStoreError::UnsupportedStoreVersion(version)) if version == "19"
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

#[test]
fn restart_fails_an_inflight_correction_closed_and_is_idempotent() {
  let database = TemporaryDatabase::new("restart-manual-recovery");
  let (run_id, action_id) = interrupted_correction(database.path());
  let mut store = DurableEventStore::open(database.path()).unwrap();
  let report = store.recover_interrupted_runs().unwrap();
  assert_eq!(report.recovered_runs, 1);
  assert_eq!(report.interrupted_attempts, 1);
  let inspection = store.inspect_correction_v1(&run_id).unwrap();
  assert_eq!(inspection.phase, "manual_recovery");
  assert_eq!(inspection.unresolved_actions.len(), 1);
  assert_eq!(inspection.unresolved_actions[0].action_id, action_id);
  assert!(inspection.unresolved_actions[0].ambiguous);
  assert_eq!(
    store
      .correction_observation_v1()
      .unwrap()
      .manual_recovery_actions,
    1
  );

  let second = store.recover_interrupted_runs().unwrap();
  assert_eq!(second.recovered_runs, 0);
  assert_eq!(second.interrupted_attempts, 0);
  assert_eq!(
    store
      .events(&run_id)
      .unwrap()
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::ManualRecoveryRequired(_)))
      .count(),
    1
  );
}

#[test]
fn operator_retry_is_audited_and_reenters_bounded_correction() {
  let database = TemporaryDatabase::new("operator-retry");
  let (run_id, action_id) = interrupted_correction(database.path());
  let mut store = DurableEventStore::open(database.path()).unwrap();
  store.recover_interrupted_runs().unwrap();
  assert!(store.correction_recovery_batch_v1(0).is_err());
  assert!(store.correction_recovery_batch_v1(101).is_err());
  assert!(store.correction_recovery_batch_v1(100).unwrap().is_empty());

  let result = store
    .recover_correction_v1(
      CorrectionRecoveryRequestV1 {
        profile: CORRECTION_RECOVERY_REQUEST_PROFILE.to_string(),
        run_id: run_id.clone(),
        action: CorrectionRecoveryActionV1::Retry,
        action_id: action_id.to_string(),
        note: None,
      },
      Utc::now(),
    )
    .unwrap();
  assert_eq!(result.status, CorrectionRecoveryStatusV1::RetryScheduled);
  assert!(result.audit_recorded);
  let events = store.events(&run_id).unwrap();
  assert!(events.iter().any(|event| matches!(
    &event.payload,
    RunEventPayload::CorrectionRetryScheduled(data)
      if data.action_id == action_id && data.attempt == 1
  )));
  assert_eq!(
    store.inspect_correction_v1(&run_id).unwrap().phase,
    "compensating"
  );
  assert!(store
    .correction_recovery_batch_v1(100)
    .unwrap()
    .iter()
    .any(|candidate| { candidate.run_id == run_id && candidate.action_id == action_id }));
}

#[test]
fn external_resolution_is_single_writer_redacted_and_survives_backup_and_reopen() {
  let database = TemporaryDatabase::new("external-resolution");
  let backup = TemporaryDatabase::new("external-resolution-backup");
  let (run_id, action_id) = interrupted_correction(database.path());
  let mut first = DurableEventStore::open(database.path()).unwrap();
  first.recover_interrupted_runs().unwrap();
  let mut competing = DurableEventStore::open(database.path()).unwrap();
  let note = "ticket SEC-42 confirms the network was removed with secret-token-123";
  let request = CorrectionRecoveryRequestV1 {
    profile: CORRECTION_RECOVERY_REQUEST_PROFILE.to_string(),
    run_id: run_id.clone(),
    action: CorrectionRecoveryActionV1::ExternallyResolve,
    action_id: action_id.to_string(),
    note: Some(note.to_string()),
  };
  let result = first
    .recover_correction_v1(request.clone(), Utc::now())
    .unwrap();
  assert_eq!(
    result.status,
    CorrectionRecoveryStatusV1::ExternallyResolved
  );
  assert!(competing
    .recover_correction_v1(request, Utc::now())
    .is_err());
  assert!(first
    .correction_action_externally_resolved(&run_id, "provision", action_id)
    .unwrap());
  drop(competing);
  drop(first);

  let raw = std::fs::read(database.path()).unwrap();
  assert!(!raw
    .windows(note.len())
    .any(|window| window == note.as_bytes()));
  let reopened = DurableEventStore::open(database.path()).unwrap();
  assert!(reopened
    .correction_action_externally_resolved(&run_id, "provision", action_id)
    .unwrap());
  drop(reopened);

  create_online_backup(
    database.path(),
    backup.path(),
    "lease-correction-backup",
    "compensation-store-test",
    "compensation-deployment",
  )
  .unwrap();
  let restored = DurableEventStore::open(backup.path()).unwrap();
  assert!(restored
    .correction_action_externally_resolved(&run_id, "provision", action_id)
    .unwrap());
}

#[test]
fn one_thousand_registrations_project_within_the_reviewed_budget_and_reject_corruption() {
  let run_id = "run-correction-benchmark";
  let at = Utc::now();
  let mut sequence = 1_u64;
  let mut event = |payload| {
    let current = sequence;
    sequence += 1;
    RunEvent {
      event_schema_version: RUN_EVENT_SCHEMA_VERSION_V18,
      event_id: format!("evt-correction-benchmark-{current}"),
      run_id: run_id.to_string(),
      sequence: current,
      occurred_at: at + chrono::Duration::milliseconds(i64::try_from(current).unwrap()),
      iteration: None,
      payload,
    }
  };
  let mut events = vec![
    event(RunEventPayload::RunAdmitted(RunAdmittedData {
      definition_hash: HASH.to_string(),
      policy_hash: format!("sha256:{}", "a".repeat(64)),
      trigger: RunAdmissionTrigger {
        id: "start".to_string(),
        handler: "trigger.manual".to_string(),
      },
      payload: Map::new(),
      queue: RunAdmissionQueue {
        name: "workflow:correction-benchmark".to_string(),
        discipline: QueueDiscipline::WorkConservingFifo,
      },
      admitted_at: at + chrono::Duration::milliseconds(1),
      occurrence_sequence: 1,
    })),
    event(RunEventPayload::RunExecutionStarted(
      RunExecutionStartedData {
        started_at: at + chrono::Duration::milliseconds(2),
        timeout_at: None,
      },
    )),
    event(RunEventPayload::SagaStarted(SagaIdentityData {
      saga_id: "bulk".to_string(),
    })),
  ];
  for index in 0..1_000_u32 {
    let node_id = format!("effect{index}");
    let invocation_id = format!("inv-{index}");
    let success_sequence = u64::try_from(events.len() + 2).unwrap();
    events.push(event(RunEventPayload::StepAttemptStarted(
      StepAttemptStartedData {
        node_id: node_id.clone(),
        attempt: 1,
        invocation_id: invocation_id.clone(),
        handler: "runtime.script".to_string(),
        idempotency_key: Some(step_effect_idempotency_key(run_id, HASH, &node_id)),
      },
    )));
    events.push(event(RunEventPayload::StepAttemptSucceeded(
      StepAttemptSucceededData {
        node_id: node_id.clone(),
        attempt: 1,
        invocation_id,
        output: json!({"index": index}),
      },
    )));
    events.push(event(RunEventPayload::CompensationRegistered(
      CompensationRegisteredData {
        saga_id: "bulk".to_string(),
        action_id: format!("bulk:compensate:{node_id}"),
        for_step_id: node_id,
        completion_sequence: success_sequence,
      },
    )));
  }
  let started = std::time::Instant::now();
  let projection = fold_events(&events).unwrap();
  let elapsed = started.elapsed();
  assert_eq!(projection.sagas["bulk"].actions.len(), 1_000);
  assert!(
    elapsed < std::time::Duration::from_secs(10),
    "1,000 registrations projected in {elapsed:?}"
  );
  eprintln!("CC7 benchmark: 1,000 correction registrations projected in {elapsed:?}");

  for removal in [4_usize, 1_504, 3_001] {
    let mut corrupted = events.clone();
    corrupted.remove(removal);
    for (index, event) in corrupted.iter_mut().enumerate() {
      event.sequence = u64::try_from(index + 1).unwrap();
    }
    assert!(fold_events(&corrupted).is_err());
  }
}
