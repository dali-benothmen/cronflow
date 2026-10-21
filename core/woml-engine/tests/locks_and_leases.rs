use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use woml_engine::event::{ForEachIterationIdentityData, ForEachIterationScope, ForEachOpenedData};
use woml_engine::{
  execute_workflow_durable_outcome, CompiledWorkflowDefinition, DurableDagEngine,
  DurableEventStore, LockStatus, RunEventPayload, RuntimeExecutionOptions,
  ScriptHostProcessOptions, StepAttemptSucceededData, TriggerAdmissionRequest,
  WorkflowRuntimeOutcome,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/locks-and-leases/model.v18.reviewed.json");
const HASH: &str = "sha256:1818181818181818181818181818181818181818181818181818181818181818";

fn model() -> CompiledWorkflowDefinition {
  serde_json::from_str(MODEL).expect("reviewed Model v18 fixture")
}

fn composition_model() -> Option<CompiledWorkflowDefinition> {
  host_options()?;
  let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let fixture = repository.join("woml/tests/fixtures/locks-and-leases/composition.reviewed.woml");
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

struct TemporaryDatabase(PathBuf);
impl TemporaryDatabase {
  fn new() -> Self {
    Self(std::env::temp_dir().join(format!("woml-locks-{}.sqlite", Uuid::new_v4().simple())))
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

fn host_options() -> Option<ScriptHostProcessOptions> {
  std::process::Command::new("bun")
    .arg("--version")
    .output()
    .ok()?
    .status
    .success()
    .then_some(())?;
  let host = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../woml-cli/src/script-host.ts");
  host
    .exists()
    .then(|| ScriptHostProcessOptions::new(PathBuf::from("bun"), host))
}

fn set_script(workflow: &mut CompiledWorkflowDefinition, node_id: &str, source: &str) {
  let node = workflow
    .graph
    .nodes
    .iter_mut()
    .find(|node| node.id == node_id)
    .unwrap();
  let woml_engine::model::ValueExpression::Object { fields } = &mut node.inputs else {
    panic!("script inputs")
  };
  fields.insert(
    "source".to_string(),
    woml_engine::model::ValueExpression::Literal {
      value: Value::String(source.to_string()),
    },
  );
}

fn prepare_run(engine: &mut DurableDagEngine, run_id: &str, at: chrono::DateTime<Utc>) {
  let invocation = format!("inv-{run_id}");
  engine
    .start_step_attempt(run_id, "prepare", 1, invocation.clone(), at)
    .unwrap();
  engine
    .append_payload(
      format!("{run_id}-prepared"),
      run_id,
      at,
      RunEventPayload::StepAttemptSucceeded(StepAttemptSucceededData {
        node_id: "prepare".to_string(),
        attempt: 1,
        invocation_id: invocation,
        output: json!({"service": "payments"}),
      }),
    )
    .unwrap();
}

fn admitted_runs(count: usize, now: chrono::DateTime<Utc>) -> (DurableDagEngine, Vec<String>) {
  let mut workflow = model();
  workflow.runtime_policy.as_mut().unwrap().concurrency = Some(u32::try_from(count).unwrap());
  workflow.validate_for_durable_execution().unwrap();
  let mut store = DurableEventStore::open_in_memory().unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let mut run_ids = Vec::new();
  for index in 0..count {
    let run_id = store
      .admit_trigger_occurrence(TriggerAdmissionRequest {
        workflow_id: workflow.workflow_id.clone(),
        definition_hash: HASH.to_string(),
        trigger_id: "start".to_string(),
        trigger_handler: "trigger.manual".to_string(),
        source_identity: format!("lock-test-{index}"),
        payload: Map::new(),
        received_at: now + Duration::milliseconds(i64::try_from(index).unwrap()),
      })
      .unwrap()
      .run_id;
    store
      .claim_policy_run(
        &run_id,
        &format!("owner-{index}"),
        now + Duration::milliseconds(i64::try_from(index).unwrap()),
        std::time::Duration::from_secs(30),
      )
      .unwrap();
    run_ids.push(run_id);
  }
  let mut engine = DurableDagEngine::resume(store, &run_ids[0]).unwrap();
  for run_id in &run_ids {
    prepare_run(&mut engine, run_id, now);
  }
  (engine, run_ids)
}

fn admitted_run_on_disk(path: &Path, now: chrono::DateTime<Utc>) -> (String, DurableDagEngine) {
  let workflow = model();
  let mut store = DurableEventStore::open(path).unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: HASH.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: format!("hardening-{}", Uuid::new_v4().simple()),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "hardening-owner",
      now,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  prepare_run(&mut engine, &run_id, now);
  (run_id, engine)
}

#[test]
fn programmatic_lease_is_fenced_renewable_releasable_and_publicly_inspectable() {
  let now = Utc::now();
  let workflow = model();
  let mut store = DurableEventStore::open_in_memory().unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: HASH.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: "programmatic-lock-test".to_string(),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "programmatic-owner",
      now,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  engine
    .start_step_attempt(&run_id, "prepare", 1, "inv-programmatic", now)
    .unwrap();
  let mut store = engine.into_store();
  let acquired = store
    .acquire_programmatic_lock_atomically(
      &run_id,
      "prepare",
      "inv-programmatic",
      "session_one",
      "cache/customer-42",
      10_000,
      5_000,
      now,
    )
    .unwrap();
  assert_eq!(
    acquired.state,
    woml_engine::ProgrammaticLockAcquireState::Held
  );
  assert_eq!(acquired.lease.as_ref().unwrap().fencing_token, "1");
  assert_eq!(
    store
      .renew_programmatic_lock_atomically(
        &run_id,
        "session_one",
        5_000,
        now + Duration::seconds(1),
      )
      .unwrap(),
    woml_engine::LockLeaseMaintenance::Renewed
  );

  let held = store.inspect_locks_v1(&run_id).unwrap();
  assert_eq!(held.len(), 1);
  assert_eq!(held[0].lock_id, "services.locks.withLease");
  assert_eq!(held[0].status, woml_engine::InspectedLockStatusV1::Held);
  let public = serde_json::to_string(&held).unwrap();
  for forbidden in [
    "leaseCredential",
    "renewalNonce",
    "ownerRuntimeId",
    "inputDigest",
    "session_one",
    "inv-programmatic",
  ] {
    assert!(!public.contains(forbidden));
  }

  assert!(
    store
      .release_programmatic_lock_atomically(
        &run_id,
        "session_one",
        5_000,
        now + Duration::seconds(2),
      )
      .unwrap()
  );
  let released = store.inspect_locks_v1(&run_id).unwrap();
  assert_eq!(
    released[0].status,
    woml_engine::InspectedLockStatusV1::Released
  );
  let metrics = store.lock_observation_v1().unwrap();
  assert_eq!(metrics.held_locks, 0);
  assert_eq!(metrics.waiting_acquisitions, 0);
}

#[test]
fn store_v16_serializes_fifo_owners_and_increments_fencing_tokens() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(3, now);
  let (first_run, second_run, third_run) = (&runs[0], &runs[1], &runs[2]);

  let first = engine
    .register_lock_waiter(
      first_run,
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  let second = engine
    .register_lock_waiter(
      second_run,
      "deployment",
      "production/payments",
      now + Duration::milliseconds(1),
      now + Duration::minutes(10),
    )
    .unwrap();
  let third = engine
    .register_lock_waiter(
      third_run,
      "deployment",
      "production/payments",
      now + Duration::milliseconds(2),
      now + Duration::minutes(10),
    )
    .unwrap();
  assert_eq!(first.locks["deployment"].status, LockStatus::Held);
  assert_eq!(
    first.locks["deployment"].fencing_token.as_deref(),
    Some("1")
  );
  assert_eq!(second.locks["deployment"].status, LockStatus::Waiting);
  assert_eq!(third.locks["deployment"].status, LockStatus::Waiting);

  engine
    .release_lock(
      first_run,
      "deployment",
      json!({"status":"released"}),
      now + Duration::seconds(1),
    )
    .unwrap();
  let second = engine.projection(second_run).unwrap();
  let third = engine.projection(third_run).unwrap();
  assert_eq!(second.locks["deployment"].status, LockStatus::Held);
  assert_eq!(
    second.locks["deployment"].fencing_token.as_deref(),
    Some("2")
  );
  assert_eq!(third.locks["deployment"].status, LockStatus::Waiting);

  engine
    .release_lock(
      second_run,
      "deployment",
      json!({"status":"released"}),
      now + Duration::seconds(2),
    )
    .unwrap();
  let third = engine.projection(third_run).unwrap();
  assert_eq!(third.locks["deployment"].status, LockStatus::Held);
  assert_eq!(
    third.locks["deployment"].fencing_token.as_deref(),
    Some("3")
  );
}

#[test]
fn duplicate_registration_reattaches_but_changed_input_conflicts() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(1, now);
  let run_id = &runs[0];
  let deadline = now + Duration::minutes(10);
  engine
    .register_lock_waiter(run_id, "deployment", "production/payments", now, deadline)
    .unwrap();
  let mut store = engine.into_store();
  let replay = store
    .register_lock_waiter_atomically(
      run_id,
      "deployment",
      "production/payments",
      "__woml_lock__deployment__acquire",
      now,
      deadline,
    )
    .unwrap();
  assert_eq!(replay.locks["deployment"].status, LockStatus::Held);
  let error = store
    .register_lock_waiter_atomically(
      run_id,
      "deployment",
      "production/other",
      "__woml_lock__deployment__acquire",
      now,
      deadline,
    )
    .unwrap_err();
  assert!(matches!(
    error,
    woml_engine::DurableStoreError::LockIdempotencyConflict
  ));
  let serialized = serde_json::to_string(&store.events(run_id).unwrap()).unwrap();
  assert!(!serialized.contains("secret"));
}

#[test]
fn expired_waiter_is_settled_and_cannot_acquire_later() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(2, now);
  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .register_lock_waiter(
      &runs[1],
      "deployment",
      "production/payments",
      now,
      now + Duration::milliseconds(10),
    )
    .unwrap();
  let timed_out = engine
    .timeout_lock_waiter(&runs[1], "deployment", now + Duration::milliseconds(11))
    .unwrap();
  assert_eq!(
    timed_out.locks["deployment"].status,
    LockStatus::AcquireTimedOut
  );
  engine
    .release_lock(
      &runs[0],
      "deployment",
      json!({"status":"released"}),
      now + Duration::seconds(1),
    )
    .unwrap();
  assert_eq!(
    engine.projection(&runs[1]).unwrap().locks["deployment"].status,
    LockStatus::AcquireTimedOut,
  );
}

#[test]
fn renewal_extends_exact_ownership_and_expiry_transfers_with_a_greater_fence() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(3, now);
  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .register_lock_waiter(
      &runs[1],
      "deployment",
      "production/payments",
      now + Duration::milliseconds(1),
      now + Duration::minutes(10),
    )
    .unwrap();
  assert_eq!(
    engine
      .renew_lock_lease(&runs[0], "deployment", now + Duration::seconds(10))
      .unwrap(),
    woml_engine::LockLeaseMaintenance::Renewed
  );

  // The original 30-second expiry is no longer authoritative after renewal.
  engine
    .register_lock_waiter(
      &runs[2],
      "deployment",
      "production/payments",
      now + Duration::seconds(31),
      now + Duration::minutes(10),
    )
    .unwrap();
  assert_eq!(
    engine.projection(&runs[1]).unwrap().locks["deployment"].status,
    LockStatus::Waiting
  );

  let mut store = engine.into_store();
  assert!(!store
    .managed_effect_has_current_lock(&runs[0], "deploy", "inv-stale", now + Duration::seconds(41),)
    .unwrap());
  let former = store.projection(&runs[0]).unwrap();
  let successor = store.projection(&runs[1]).unwrap();
  assert_eq!(former.locks["deployment"].status, LockStatus::LeaseLost);
  assert_eq!(successor.locks["deployment"].status, LockStatus::Held);
  assert_eq!(
    successor.locks["deployment"].fencing_token.as_deref(),
    Some("2")
  );
}

#[test]
fn late_release_loses_to_expiry_and_cannot_clear_the_successor() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(2, now);
  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .register_lock_waiter(
      &runs[1],
      "deployment",
      "production/payments",
      now + Duration::milliseconds(1),
      now + Duration::minutes(10),
    )
    .unwrap();
  let error = engine
    .release_lock(
      &runs[0],
      "deployment",
      json!({"status":"too-late"}),
      now + Duration::seconds(31),
    )
    .unwrap_err();
  assert!(matches!(
    error,
    woml_engine::DurableEngineError::Store(woml_engine::DurableStoreError::LockHistoryInvalid(_))
  ));
  assert_eq!(
    engine.projection(&runs[0]).unwrap().locks["deployment"].status,
    LockStatus::LeaseLost
  );
  let successor = engine.projection(&runs[1]).unwrap();
  assert_eq!(successor.locks["deployment"].status, LockStatus::Held);
  assert_eq!(
    successor.locks["deployment"].fencing_token.as_deref(),
    Some("2")
  );
}

#[test]
fn restart_reattaches_to_the_exact_lease_then_expires_it_fail_closed() {
  let now = Utc::now();
  let database = TemporaryDatabase::new();
  let mut workflow = model();
  workflow.runtime_policy.as_mut().unwrap().concurrency = Some(2);
  let mut store = DurableEventStore::open(database.path()).unwrap();
  store.register_definition(&workflow, HASH).unwrap();
  let mut runs = Vec::new();
  for index in 0..2 {
    let run_id = store
      .admit_trigger_occurrence(TriggerAdmissionRequest {
        workflow_id: workflow.workflow_id.clone(),
        definition_hash: HASH.to_string(),
        trigger_id: "start".to_string(),
        trigger_handler: "trigger.manual".to_string(),
        source_identity: format!("restart-lock-{index}"),
        payload: Map::new(),
        received_at: now + Duration::milliseconds(i64::from(index)),
      })
      .unwrap()
      .run_id;
    store
      .claim_policy_run(
        &run_id,
        &format!("restart-owner-{index}"),
        now,
        std::time::Duration::from_secs(30),
      )
      .unwrap();
    runs.push(run_id);
  }
  let mut engine = DurableDagEngine::resume(store, &runs[0]).unwrap();
  for run_id in &runs {
    prepare_run(&mut engine, run_id, now);
  }
  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .register_lock_waiter(
      &runs[1],
      "deployment",
      "production/payments",
      now + Duration::milliseconds(1),
      now + Duration::minutes(10),
    )
    .unwrap();
  drop(engine);

  let store = DurableEventStore::open(database.path()).unwrap();
  let mut resumed = DurableDagEngine::resume(store, &runs[0]).unwrap();
  assert_eq!(
    resumed
      .renew_lock_lease(&runs[0], "deployment", now + Duration::seconds(10))
      .unwrap(),
    woml_engine::LockLeaseMaintenance::Renewed
  );
  drop(resumed);

  let mut store = DurableEventStore::open(database.path()).unwrap();
  assert!(!store
    .managed_effect_has_current_lock(
      &runs[0],
      "deploy",
      "restart-stale-effect",
      now + Duration::seconds(41),
    )
    .unwrap());
  assert_eq!(
    store.projection(&runs[0]).unwrap().locks["deployment"].status,
    LockStatus::LeaseLost
  );
  assert_eq!(
    store.projection(&runs[1]).unwrap().locks["deployment"].status,
    LockStatus::Held
  );
  assert_eq!(
    store.projection(&runs[1]).unwrap().locks["deployment"]
      .fencing_token
      .as_deref(),
    Some("2")
  );
}

#[test]
fn cancellation_removes_a_waiter_but_does_not_release_an_unproven_owner() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(2, now);
  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .register_lock_waiter(
      &runs[1],
      "deployment",
      "production/payments",
      now + Duration::milliseconds(1),
      now + Duration::minutes(10),
    )
    .unwrap();
  let mut store = engine.into_store();
  store
    .request_run_cancellation(&runs[1], "cancel-waiter", now + Duration::seconds(1))
    .unwrap();
  let cancelling = store.projection(&runs[1]).unwrap();
  assert_eq!(cancelling.status, woml_engine::RunStatus::Cancelling);
  assert_eq!(
    cancelling.cancellation_request_id.as_deref(),
    Some("cancel-waiter")
  );
  let cancelled = store
    .cancel_lock_waiters_atomically(&runs[1], "cancel-waiter", now + Duration::seconds(1))
    .unwrap();
  assert_eq!(
    cancelled.locks["deployment"].status,
    LockStatus::AcquisitionCancelled
  );
  assert_eq!(
    store.projection(&runs[0]).unwrap().locks["deployment"].status,
    LockStatus::Held
  );
}

#[test]
fn for_each_iterations_have_distinct_fifo_acquisition_identities() {
  let Some(workflow) = composition_model() else {
    return;
  };
  let now = Utc::now();
  let hash = "sha256:1919191919191919191919191919191919191919191919191919191919191919";
  let mut store = DurableEventStore::open_in_memory().unwrap();
  store.register_definition(&workflow, hash).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: hash.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: "iteration-lock-test".to_string(),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "iteration-owner",
      now,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  engine
    .append_payload(
      "open-loop",
      &run_id,
      now,
      RunEventPayload::ForEachOpened(ForEachOpenedData {
        for_each_id: "items".to_string(),
        total: 2,
        items_digest: format!("sha256:{}", "1".repeat(64)),
        concurrency: 1,
      }),
    )
    .unwrap();
  for index in 0..2 {
    engine
      .append_scoped_payload(
        format!("start-iteration-{index}"),
        &run_id,
        now + Duration::milliseconds(i64::from(index)),
        ForEachIterationScope {
          for_each_id: "items".to_string(),
          index,
        },
        RunEventPayload::ForEachIterationStarted(ForEachIterationIdentityData {
          for_each_id: "items".to_string(),
        }),
      )
      .unwrap();
    engine
      .register_iteration_lock_waiter(
        &run_id,
        "iterationLock",
        "item/shared",
        ForEachIterationScope {
          for_each_id: "items".to_string(),
          index,
        },
        now + Duration::milliseconds(i64::from(index)),
        now + Duration::minutes(1),
      )
      .unwrap();
  }
  let projected = engine.projection(&run_id).unwrap();
  assert_eq!(
    projected.for_each["items"].executions[&0].locks["iterationLock"].status,
    LockStatus::Held
  );
  assert_eq!(
    projected.for_each["items"].executions[&1].locks["iterationLock"].status,
    LockStatus::Waiting
  );
  engine
    .release_iteration_lock(
      &run_id,
      "iterationLock",
      ForEachIterationScope {
        for_each_id: "items".to_string(),
        index: 0,
      },
      json!({"status":"released"}),
      now + Duration::seconds(1),
    )
    .unwrap();
  let projected = engine.projection(&run_id).unwrap();
  assert_eq!(
    projected.for_each["items"].executions[&1].locks["iterationLock"].status,
    LockStatus::Held
  );
  assert_eq!(
    projected.for_each["items"].executions[&1].locks["iterationLock"]
      .fencing_token
      .as_deref(),
    Some("2")
  );
}

#[test]
fn cancellation_removes_iteration_waiters_without_releasing_iteration_owners() {
  let Some(workflow) = composition_model() else {
    return;
  };
  let now = Utc::now();
  let hash = "sha256:2020202020202020202020202020202020202020202020202020202020202020";
  let mut store = DurableEventStore::open_in_memory().unwrap();
  store.register_definition(&workflow, hash).unwrap();
  let run_id = store
    .admit_trigger_occurrence(TriggerAdmissionRequest {
      workflow_id: workflow.workflow_id.clone(),
      definition_hash: hash.to_string(),
      trigger_id: "start".to_string(),
      trigger_handler: "trigger.manual".to_string(),
      source_identity: "iteration-lock-cancellation".to_string(),
      payload: Map::new(),
      received_at: now,
    })
    .unwrap()
    .run_id;
  store
    .claim_policy_run(
      &run_id,
      "iteration-owner",
      now,
      std::time::Duration::from_secs(30),
    )
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  engine
    .append_payload(
      "open-loop",
      &run_id,
      now,
      RunEventPayload::ForEachOpened(ForEachOpenedData {
        for_each_id: "items".to_string(),
        total: 2,
        items_digest: format!("sha256:{}", "2".repeat(64)),
        concurrency: 1,
      }),
    )
    .unwrap();
  for index in 0..2 {
    let scope = ForEachIterationScope {
      for_each_id: "items".to_string(),
      index,
    };
    engine
      .append_scoped_payload(
        format!("start-iteration-{index}"),
        &run_id,
        now + Duration::milliseconds(i64::from(index)),
        scope.clone(),
        RunEventPayload::ForEachIterationStarted(ForEachIterationIdentityData {
          for_each_id: "items".to_string(),
        }),
      )
      .unwrap();
    engine
      .register_iteration_lock_waiter(
        &run_id,
        "iterationLock",
        "item/shared",
        scope,
        now + Duration::milliseconds(i64::from(index)),
        now + Duration::minutes(1),
      )
      .unwrap();
  }
  let mut store = engine.into_store();
  store
    .request_run_cancellation(&run_id, "cancel-loop", now + Duration::seconds(1))
    .unwrap();
  let cancelled = store
    .cancel_lock_waiters_atomically(&run_id, "cancel-loop", now + Duration::seconds(1))
    .unwrap();
  assert_eq!(
    cancelled.for_each["items"].executions[&0].locks["iterationLock"].status,
    LockStatus::Held
  );
  assert_eq!(
    cancelled.for_each["items"].executions[&1].locks["iterationLock"].status,
    LockStatus::AcquisitionCancelled
  );
}

#[test]
fn store_v15_migrates_through_v16_to_v17_and_future_versions_fail_closed() {
  let database = TemporaryDatabase::new();
  drop(DurableEventStore::open(database.path()).unwrap());
  {
    let connection = Connection::open(database.path()).unwrap();
    connection
      .execute_batch(
        "DROP TABLE woml_lock_waiters;
       DROP TABLE woml_lock_resources;
       UPDATE woml_store_metadata SET value = '15' WHERE key = 'schema_version';",
      )
      .unwrap();
  }
  drop(DurableEventStore::open(database.path()).unwrap());
  {
    let connection = Connection::open(database.path()).unwrap();
    let version: String = connection
      .query_row(
        "SELECT value FROM woml_store_metadata WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(version, "17");
    let tables: i64 = connection.query_row(
      "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('woml_lock_resources', 'woml_lock_waiters')",
      [], |row| row.get(0),
    ).unwrap();
    assert_eq!(tables, 2);
    connection
      .execute(
        "UPDATE woml_store_metadata SET value = '999' WHERE key = 'schema_version'",
        [],
      )
      .unwrap();
  }
  assert!(matches!(
    DurableEventStore::open(database.path()),
    Err(woml_engine::DurableStoreError::UnsupportedStoreVersion(version)) if version == "999"
  ));
}

#[tokio::test]
async fn two_durable_runs_execute_one_protected_body_at_a_time() {
  let Some(first_host) = host_options() else {
    return;
  };
  let Some(second_host) = host_options() else {
    return;
  };
  let database = TemporaryDatabase::new();
  DurableEventStore::open(database.path()).unwrap();
  let mut workflow = model();
  workflow.runtime_policy.as_mut().unwrap().concurrency = Some(2);
  set_script(&mut workflow, "deploy", "await Bun.sleep(150); return { fencingToken: context.lock.fencingToken, lockId: context.lock.id, key: context.lock.key }; ");
  workflow.validate_for_durable_execution().unwrap();
  let first = execute_workflow_durable_outcome(
    workflow.clone(),
    HASH.to_string(),
    Map::new(),
    RuntimeExecutionOptions::new(first_host, 3_000),
    database.path().to_path_buf(),
  );
  let second = execute_workflow_durable_outcome(
    workflow,
    HASH.to_string(),
    Map::new(),
    RuntimeExecutionOptions::new(second_host, 3_000),
    database.path().to_path_buf(),
  );
  let (first, second) = tokio::join!(first, second);
  for outcome in [first.unwrap(), second.unwrap()] {
    let WorkflowRuntimeOutcome::Succeeded { execution, .. } = outcome else {
      panic!("lock run did not succeed")
    };
    assert_eq!(execution.result["status"], "released");
    assert_eq!(execution.result["key"], "production/payments");
  }
  let store = DurableEventStore::open(database.path()).unwrap();
  let runs = store.list_runs(10).unwrap();
  assert_eq!(runs.runs.len(), 2);
  let mut windows = runs
    .runs
    .iter()
    .map(|run| {
      let events = store.events(&run.run_id).unwrap();
      let acquired = events
        .iter()
        .find_map(|event| match &event.payload {
          RunEventPayload::LockAcquired(data) => {
            Some((data.acquired_at, data.fencing_token.clone()))
          }
          _ => None,
        })
        .unwrap();
      let released = events
        .iter()
        .find_map(|event| match &event.payload {
          RunEventPayload::LockReleased(data) => Some(data.released_at),
          _ => None,
        })
        .unwrap();
      let deploy = events
        .iter()
        .find_map(|event| match &event.payload {
          RunEventPayload::StepAttemptSucceeded(data) if data.node_id == "deploy" => {
            Some(data.output.clone())
          }
          _ => None,
        })
        .unwrap();
      assert_eq!(deploy["lockId"], "deployment");
      assert_eq!(deploy["key"], "production/payments");
      assert_eq!(deploy["fencingToken"], acquired.1);
      (acquired.0, released, acquired.1)
    })
    .collect::<Vec<_>>();
  windows.sort_by_key(|window| window.0);
  assert!(windows[0].1 <= windows[1].0);
  assert_eq!(
    windows
      .iter()
      .map(|window| window.2.as_str())
      .collect::<Vec<_>>(),
    vec!["1", "2"]
  );
}

#[tokio::test]
async fn a_failed_lock_body_releases_before_the_run_fails() {
  let Some(first_host) = host_options() else {
    return;
  };
  let Some(second_host) = host_options() else {
    return;
  };
  let database = TemporaryDatabase::new();
  DurableEventStore::open(database.path()).unwrap();
  let mut workflow = model();
  workflow.runtime_policy.as_mut().unwrap().concurrency = Some(1);
  set_script(&mut workflow, "deploy", "if (context.payload.fail) throw new Error('reviewed failure'); return { fencingToken: context.lock.fencingToken }; ");
  workflow.validate_for_durable_execution().unwrap();

  let mut failing_trigger = Map::new();
  failing_trigger.insert("fail".to_string(), Value::Bool(true));
  let failed = execute_workflow_durable_outcome(
    workflow.clone(),
    HASH.to_string(),
    failing_trigger,
    RuntimeExecutionOptions::new(first_host, 3_000),
    database.path().to_path_buf(),
  )
  .await;
  assert!(failed.is_err());

  let succeeded = execute_workflow_durable_outcome(
    workflow,
    HASH.to_string(),
    Map::new(),
    RuntimeExecutionOptions::new(second_host, 3_000),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  assert!(matches!(
    succeeded,
    WorkflowRuntimeOutcome::Succeeded { .. }
  ));

  let store = DurableEventStore::open(database.path()).unwrap();
  let runs = store.list_runs(10).unwrap();
  let mut tokens = Vec::new();
  for run in runs.runs {
    let events = store.events(&run.run_id).unwrap();
    let token = events
      .iter()
      .find_map(|event| match &event.payload {
        RunEventPayload::LockAcquired(data) => Some(data.fencing_token.clone()),
        _ => None,
      })
      .unwrap();
    assert!(events
      .iter()
      .any(|event| matches!(event.payload, RunEventPayload::LockReleased(_))));
    tokens.push(token);
  }
  tokens.sort();
  assert_eq!(tokens, vec!["1", "2"]);
}

#[test]
fn hostile_keys_are_rejected_and_clock_rollback_never_shortens_a_lease() {
  let now = Utc::now();
  let (mut engine, runs) = admitted_runs(1, now);
  let error = engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production//payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap_err();
  assert!(matches!(
    error,
    woml_engine::DurableEngineError::Store(woml_engine::DurableStoreError::Contract(_))
  ));

  engine
    .register_lock_waiter(
      &runs[0],
      "deployment",
      "production/payments",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  engine
    .renew_lock_lease(&runs[0], "deployment", now + Duration::seconds(10))
    .unwrap();
  let store = engine.into_store();
  let first_expiry = store.inspect_locks_v1(&runs[0]).unwrap()[0]
    .expires_at
    .unwrap();
  let mut engine = DurableDagEngine::resume(store, &runs[0]).unwrap();
  engine
    .renew_lock_lease(&runs[0], "deployment", now + Duration::seconds(5))
    .unwrap();
  let second_expiry = engine.into_store().inspect_locks_v1(&runs[0]).unwrap()[0]
    .expires_at
    .unwrap();
  assert_eq!(second_expiry, first_expiry);
}

#[test]
fn wait_limits_fail_closed_while_an_uncontended_key_can_still_start() {
  let now = Utc::now();
  let database = TemporaryDatabase::new();
  let (run_id, engine) = admitted_run_on_disk(database.path(), now);
  drop(engine);
  let mut connection = Connection::open(database.path()).unwrap();
  let transaction = connection.transaction().unwrap();
  for key_index in 0..10 {
    let key = format!("backlog/{key_index}");
    transaction
      .execute(
        "INSERT INTO woml_lock_resources(key, fencing_high_water, updated_at)
         VALUES (?1, 0, ?2)",
        params![key, now.to_rfc3339()],
      )
      .unwrap();
    for waiter_index in 0..1_000 {
      let ordinal = key_index * 1_000 + waiter_index;
      transaction
        .execute(
          "INSERT INTO woml_lock_waiters(
             waiter_id, key, run_id, node_execution_id, sequence,
             registered_at, deadline, status, input_digest
           ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'waiting', ?8)",
          params![
            format!("seed_waiter_{ordinal}"),
            key,
            run_id,
            format!("seed_execution_{ordinal}"),
            waiter_index + 1,
            now.to_rfc3339(),
            (now + Duration::minutes(10)).to_rfc3339(),
            format!("sha256:{ordinal:064x}")
          ],
        )
        .unwrap();
    }
  }
  transaction.commit().unwrap();
  drop(connection);

  let store = DurableEventStore::open(database.path()).unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  let acquired = engine
    .register_lock_waiter(
      &run_id,
      "deployment",
      "uncontended/deployment",
      now,
      now + Duration::minutes(10),
    )
    .unwrap();
  assert_eq!(acquired.locks["deployment"].status, LockStatus::Held);
}

#[test]
fn ten_thousand_due_waiters_are_recovered_in_bounded_batches() {
  let now = Utc::now();
  let database = TemporaryDatabase::new();
  let (run_id, engine) = admitted_run_on_disk(database.path(), now);
  drop(engine);
  let mut connection = Connection::open(database.path()).unwrap();
  let transaction = connection.transaction().unwrap();
  for key_index in 0..10 {
    let key = format!("recovery/{key_index}");
    transaction
      .execute(
        "INSERT INTO woml_lock_resources(key, fencing_high_water, updated_at)
         VALUES (?1, 0, ?2)",
        params![key, now.to_rfc3339()],
      )
      .unwrap();
    for waiter_index in 0..1_000 {
      let ordinal = key_index * 1_000 + waiter_index;
      transaction
        .execute(
          "INSERT INTO woml_lock_waiters(
             waiter_id, key, run_id, node_execution_id, sequence,
             registered_at, deadline, status, input_digest
           ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'waiting', ?8)",
          params![
            format!("recovery_waiter_{ordinal}"),
            key,
            run_id,
            format!("recovery_execution_{ordinal}"),
            waiter_index + 1,
            (now - Duration::seconds(2)).to_rfc3339(),
            (now - Duration::seconds(1)).to_rfc3339(),
            format!("sha256:{ordinal:064x}")
          ],
        )
        .unwrap();
    }
  }
  transaction.commit().unwrap();
  drop(connection);

  let store = DurableEventStore::open(database.path()).unwrap();
  let mut samples = Vec::with_capacity(30);
  let mut batch = Vec::new();
  for _ in 0..30 {
    let started = std::time::Instant::now();
    batch = store.lock_recovery_batch_v1(now).unwrap();
    samples.push(started.elapsed());
  }
  samples.sort();
  let p95 = samples[28];
  println!("10,000-waiter recovery scan p95: {p95:?}");
  assert!(p95 <= std::time::Duration::from_millis(100));
  assert_eq!(batch.len(), woml_engine::LOCK_RECOVERY_BATCH_LIMIT);
  assert!(batch
    .iter()
    .all(|candidate| { candidate.kind == woml_engine::LockRecoveryKindV1::AcquisitionTimeout }));
}

#[test]
fn queue_and_fencing_exhaustion_have_distinct_durable_failures() {
  let now = Utc::now();
  let queue_database = TemporaryDatabase::new();
  let (run_id, engine) = admitted_run_on_disk(queue_database.path(), now);
  drop(engine);
  let mut connection = Connection::open(queue_database.path()).unwrap();
  let transaction = connection.transaction().unwrap();
  transaction
    .execute(
      "INSERT INTO woml_lock_resources(key, fencing_high_water, updated_at)
       VALUES ('hot/key', 0, ?1)",
      [now.to_rfc3339()],
    )
    .unwrap();
  for index in 0..woml_engine::LOCK_WAITERS_PER_KEY_LIMIT {
    transaction
      .execute(
        "INSERT INTO woml_lock_waiters(
           waiter_id, key, run_id, node_execution_id, sequence,
           registered_at, deadline, status, input_digest
         ) VALUES (?1, 'hot/key', ?2, ?3, ?4, ?5, ?6, 'waiting', ?7)",
        params![
          format!("hot_waiter_{index}"),
          run_id,
          format!("hot_execution_{index}"),
          index + 1,
          now.to_rfc3339(),
          (now + Duration::minutes(10)).to_rfc3339(),
          format!("sha256:{index:064x}")
        ],
      )
      .unwrap();
  }
  transaction.commit().unwrap();
  drop(connection);
  let store = DurableEventStore::open(queue_database.path()).unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  assert!(matches!(
    engine.register_lock_waiter(
      &run_id,
      "deployment",
      "hot/key",
      now,
      now + Duration::minutes(10),
    ),
    Err(woml_engine::DurableEngineError::Store(
      woml_engine::DurableStoreError::LockWaitLimit
    ))
  ));

  let fence_database = TemporaryDatabase::new();
  let (run_id, engine) = admitted_run_on_disk(fence_database.path(), now);
  drop(engine);
  Connection::open(fence_database.path())
    .unwrap()
    .execute(
      "INSERT INTO woml_lock_resources(key, fencing_high_water, updated_at)
       VALUES ('exhausted/key', ?1, ?2)",
      params![i64::MAX, now.to_rfc3339()],
    )
    .unwrap();
  let store = DurableEventStore::open(fence_database.path()).unwrap();
  let mut engine = DurableDagEngine::resume(store, &run_id).unwrap();
  assert!(matches!(
    engine.register_lock_waiter(
      &run_id,
      "deployment",
      "exhausted/key",
      now,
      now + Duration::minutes(10),
    ),
    Err(woml_engine::DurableEngineError::Store(
      woml_engine::DurableStoreError::LockFencingExhausted
    ))
  ));
}
