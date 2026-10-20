use chrono::{Duration, Utc};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;
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
fn store_v15_migrates_to_v16_and_future_versions_fail_closed() {
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
    assert_eq!(version, "16");
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
