use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use chrono::{Duration, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use uuid::Uuid;
use woml_engine::{
  bind_rich_approval_review, create_online_backup, resolve_rich_approval_review, ApprovalDecision,
  ArtifactReferenceV1, BoundRichApprovalReview, CompiledWorkflowDefinition, DurableEventStore,
  DurableStoreError, RichApprovalArtifactVerifier, RichApprovalBindingInput,
  RichApprovalDecisionAdmission, RichApprovalDecisionStatus, RichApprovalError,
  RichApprovalResolutionReason, RunAdmissionQueue, RunAdmissionTrigger, RunAdmittedData,
  RunEventPayload, RunExecutionStartedData, ValueExpression, WorkflowContext,
  DURABLE_STORE_SCHEMA_VERSION, RUN_EVENT_SCHEMA_VERSION_V19,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/model.v20.reviewed.json");
const REVIEW: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/review.v1.json");
const BINDING_VECTOR: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/binding.v1.json");
const DEFINITION_HASH: &str =
  "sha256:2020202020202020202020202020202020202020202020202020202020202020";

#[derive(Default)]
struct ExactArtifacts {
  substitute: bool,
}

struct TemporaryDatabase(PathBuf);

impl TemporaryDatabase {
  fn new(label: &str) -> Self {
    Self(std::env::temp_dir().join(format!(
      "woml-rich-approval-{label}-{}.sqlite",
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

impl RichApprovalArtifactVerifier for ExactArtifacts {
  fn verify_review_artifact(
    &self,
    _run_id: &str,
    artifact: &ArtifactReferenceV1,
  ) -> Result<ArtifactReferenceV1, RichApprovalError> {
    let mut verified = artifact.clone();
    if self.substitute {
      verified.sha256 =
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    }
    Ok(verified)
  }
}

fn workflow() -> CompiledWorkflowDefinition {
  serde_json::from_str(MODEL).expect("reviewed Model v20")
}

fn review() -> Value {
  serde_json::from_str(REVIEW).expect("Review Payload v1")
}

fn binding_input(review: Value) -> RichApprovalBindingInput {
  RichApprovalBindingInput {
    definition_hash: DEFINITION_HASH.to_string(),
    workflow_id: "production-deployment".to_string(),
    workflow_version: "1.0.0".to_string(),
    run_id: "run_rich_review_1".to_string(),
    approval_id: "approveDeployment".to_string(),
    request_id: "aprreq_rich_review_1".to_string(),
    review,
    required_approvals: 2,
    required_roles: vec!["production-owner".to_string(), "security-owner".to_string()],
    directory_version: "directory-2026-10-21".to_string(),
  }
}

fn binding() -> BoundRichApprovalReview {
  bind_rich_approval_review(binding_input(review()), &ExactArtifacts::default(), &[])
    .expect("immutable review binding")
}

fn prepare_running_store(mut store: DurableEventStore) -> DurableEventStore {
  let workflow = workflow();
  let policy_hash = workflow
    .runtime_policy_hash()
    .expect("Model v20 policy hash");
  let queue_name = workflow
    .runtime_policy_queue_name()
    .expect("Model v20 queue name");
  store
    .register_definition(&workflow, DEFINITION_HASH)
    .expect("register Model v20");
  let at = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
  store
    .append_payload(
      "run_rich_review_1",
      "evt_rich_start",
      at,
      RunEventPayload::RunAdmitted(RunAdmittedData {
        definition_hash: DEFINITION_HASH.to_string(),
        policy_hash,
        trigger: RunAdmissionTrigger {
          id: "start".to_string(),
          handler: "trigger.manual".to_string(),
        },
        payload: serde_json::Map::new(),
        queue: RunAdmissionQueue {
          name: queue_name,
          discipline: woml_engine::QueueDiscipline::WorkConservingFifo,
        },
        admitted_at: at,
        occurrence_sequence: 0,
      }),
    )
    .expect("admit Event v19 run");
  store
    .append_payload(
      "run_rich_review_1",
      "evt_rich_execution_started",
      at,
      RunEventPayload::RunExecutionStarted(RunExecutionStartedData {
        started_at: at,
        timeout_at: None,
      }),
    )
    .expect("start Event v19 execution");
  store
}

fn running_store() -> DurableEventStore {
  prepare_running_store(DurableEventStore::open_in_memory().expect("Store v18"))
}

fn running_store_at(path: &Path) -> DurableEventStore {
  prepare_running_store(DurableEventStore::open(path).expect("Store v18"))
}

fn decision(
  principal_id: &str,
  decision: ApprovalDecision,
  roles: &[&str],
  minute: i64,
) -> RichApprovalDecisionAdmission {
  RichApprovalDecisionAdmission {
    run_id: "run_rich_review_1".to_string(),
    request_id: "aprreq_rich_review_1".to_string(),
    review_digest: binding().review_digest,
    principal_id: principal_id.to_string(),
    decision,
    provider: "slack".to_string(),
    provider_account_id: "workspace-1".to_string(),
    provider_actor_id: format!("actor-{principal_id}"),
    roles: roles.iter().map(|role| (*role).to_string()).collect(),
    decided_at: Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap() + Duration::minutes(minute),
  }
}

#[test]
fn rust_binds_equivalent_json_once_and_every_meaningful_change_differently() {
  let context = WorkflowContext {
    steps: serde_json::Map::from_iter([("buildReview".to_string(), json!({ "review": review() }))]),
    ..WorkflowContext::default()
  };
  let resolved = resolve_rich_approval_review(
    &ValueExpression::ContextReference {
      path: vec![
        "steps".to_string(),
        "buildReview".to_string(),
        "review".to_string(),
      ],
    },
    &context,
  )
  .unwrap();
  assert_eq!(resolved, review());
  let first = binding();
  let vector: Value = serde_json::from_str(BINDING_VECTOR).unwrap();
  assert_eq!(first.review_digest, vector["digest"]);
  let reordered = json!({
    "artifacts": [],
    "checks": [{"status": "passed", "name": "Backup verified"}],
    "changes": [{
      "summary": "Remove the retired reporting database",
      "target": "aws_db_instance.legacy",
      "action": "destroy"
    }],
    "facts": [{"value": "production", "label": "Environment"}],
    "risk": "high",
    "summary": "Create 2, modify 4, and destroy 1 infrastructure resource.",
    "title": "Deploy payments-api to production"
  });
  let equivalent =
    bind_rich_approval_review(binding_input(reordered), &ExactArtifacts::default(), &[]).unwrap();
  assert_eq!(first.review_digest, equivalent.review_digest);

  let mut changed = review();
  changed["risk"] = Value::String("critical".to_string());
  let changed =
    bind_rich_approval_review(binding_input(changed), &ExactArtifacts::default(), &[]).unwrap();
  assert_ne!(first.review_digest, changed.review_digest);
}

#[test]
fn rust_rejects_secrets_limits_control_characters_and_artifact_substitution() {
  let mut secret = review();
  secret["summary"] = Value::String("Deploy with token super-secret-value-now".to_string());
  assert_eq!(
    bind_rich_approval_review(
      binding_input(secret),
      &ExactArtifacts::default(),
      &["super-secret-value-now".to_string()],
    )
    .unwrap_err()
    .code(),
    "WOML_APPROVAL_REVIEW_SECRET_DETECTED"
  );

  let mut oversized = review();
  oversized["title"] = Value::String("x".repeat(161));
  assert_eq!(
    bind_rich_approval_review(binding_input(oversized), &ExactArtifacts::default(), &[],)
      .unwrap_err(),
    RichApprovalError::ReviewInvalid
  );

  let mut control = review();
  control["summary"] = Value::String("unsafe\u{0007}review".to_string());
  assert_eq!(
    bind_rich_approval_review(binding_input(control), &ExactArtifacts::default(), &[],)
      .unwrap_err(),
    RichApprovalError::ReviewInvalid
  );

  let artifact = json!({
    "contract": "woml.artifact-reference/v1",
    "contractVersion": 1,
    "id": "artifact-plan-1",
    "name": "plan.tfplan",
    "size": 42,
    "sha256": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "contentType": "application/octet-stream"
  });
  let mut forged = review();
  forged["artifacts"] = Value::Array(vec![artifact]);
  assert_eq!(
    bind_rich_approval_review(
      binding_input(forged),
      &ExactArtifacts { substitute: true },
      &[],
    )
    .unwrap_err(),
    RichApprovalError::ArtifactInvalid
  );
}

#[test]
fn store_v18_preserves_partial_quorum_idempotency_and_one_terminal_result() {
  let mut store = running_store();
  let mut tampered = binding();
  tampered.review.summary = "A provider tried to replace the reviewed plan.".to_string();
  assert!(matches!(
    store.create_rich_approval_request(
      &tampered,
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap()
    ),
    Err(DurableStoreError::RichApprovalBindingMismatch)
  ));
  let requested = store
    .create_rich_approval_request(
      &binding(),
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .expect("persist immutable review");
  assert_eq!(
    requested
      .rich_approval_requests
      .get("approveDeployment")
      .unwrap()
      .review_digest,
    binding().review_digest
  );

  let first = store
    .record_rich_approval_decision(decision(
      "person:alice",
      ApprovalDecision::Approved,
      &["production-owner"],
      2,
    ))
    .expect("first vote");
  assert_eq!(first.status, RichApprovalDecisionStatus::Pending);
  assert_eq!(first.received, 1);

  let replay = store
    .record_rich_approval_decision({
      let mut replay = decision(
        "person:alice",
        ApprovalDecision::Approved,
        &["production-owner"],
        2,
      );
      replay.provider = "telegram".to_string();
      replay.provider_account_id = "bot-1".to_string();
      replay.provider_actor_id = "telegram-alice".to_string();
      replay
    })
    .expect("idempotent replay");
  assert_eq!(replay.status, RichApprovalDecisionStatus::Idempotent);
  assert!(matches!(
    store.record_rich_approval_decision(decision(
      "person:alice",
      ApprovalDecision::Rejected,
      &["production-owner"],
      3,
    )),
    Err(DurableStoreError::RichApprovalReviewerConflict)
  ));

  let final_vote = store
    .record_rich_approval_decision(decision(
      "person:bob",
      ApprovalDecision::Approved,
      &["security-owner"],
      4,
    ))
    .expect("quorum vote");
  assert_eq!(final_vote.status, RichApprovalDecisionStatus::Approved);
  assert_eq!(final_vote.received, 2);
  assert_eq!(
    final_vote.represented_roles,
    vec!["production-owner", "security-owner"]
  );

  let late = store
    .record_rich_approval_decision(decision("person:carol", ApprovalDecision::Rejected, &[], 5))
    .expect("settled result");
  assert_eq!(late.status, RichApprovalDecisionStatus::AlreadyResolved);
  let inspection = store
    .inspect_rich_approval("run_rich_review_1", "aprreq_rich_review_1", 100)
    .expect("bounded inspection");
  assert_eq!(inspection.status, "approved");
  assert_eq!(inspection.total_decisions, 2);
  assert!(!inspection.decisions_truncated);
  let events = store.events("run_rich_review_1").unwrap();
  assert!(events
    .iter()
    .all(|event| event.event_schema_version == RUN_EVENT_SCHEMA_VERSION_V19));
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::ApprovalResolved(_)))
      .count(),
    1
  );
}

#[test]
fn concurrent_final_votes_have_one_atomic_terminal_winner() {
  let database = TemporaryDatabase::new("concurrent-votes");
  let mut store = running_store_at(database.path());
  store
    .create_rich_approval_request(
      &binding(),
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .unwrap();
  drop(store);

  let barrier = Arc::new(Barrier::new(2));
  let mut joins = Vec::new();
  for (principal, role, minute) in [
    ("person:alice", "production-owner", 2),
    ("person:bob", "security-owner", 3),
  ] {
    let path = database.path().to_path_buf();
    let barrier = Arc::clone(&barrier);
    joins.push(std::thread::spawn(move || {
      let mut store = DurableEventStore::open(path).unwrap();
      barrier.wait();
      store
        .record_rich_approval_decision(decision(
          principal,
          ApprovalDecision::Approved,
          &[role],
          minute,
        ))
        .unwrap()
    }));
  }
  let mut statuses = joins
    .into_iter()
    .map(|join| join.join().unwrap().status)
    .collect::<Vec<_>>();
  statuses.sort_by_key(|status| match status {
    RichApprovalDecisionStatus::Pending => 0,
    RichApprovalDecisionStatus::Approved => 1,
    _ => 2,
  });
  assert_eq!(
    statuses,
    vec![
      RichApprovalDecisionStatus::Pending,
      RichApprovalDecisionStatus::Approved
    ]
  );
  let store = DurableEventStore::open(database.path()).unwrap();
  let events = store.events("run_rich_review_1").unwrap();
  assert_eq!(
    events
      .iter()
      .filter(|event| matches!(event.payload, RunEventPayload::ApprovalResolved(_)))
      .count(),
    1
  );
}

#[test]
fn inspection_is_bounded_independently_of_adversarial_vote_count() {
  let database = TemporaryDatabase::new("bounded-inspection");
  let mut store = running_store_at(database.path());
  store
    .create_rich_approval_request(
      &binding(),
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .unwrap();
  drop(store);
  let mut connection = Connection::open(database.path()).unwrap();
  let transaction = connection.transaction().unwrap();
  for index in 0..10_000 {
    transaction
      .execute(
        "INSERT INTO woml_approval_reviewer_decisions(
           run_id, request_id, principal_id, decision, provider,
           provider_account_id, provider_actor_id, roles_json, decided_at
         ) VALUES (?1, ?2, ?3, 'approved', 'trusted-http', 'account', ?3, '[]', ?4)",
        params![
          "run_rich_review_1",
          "aprreq_rich_review_1",
          format!("person:adversarial-{index:05}"),
          Utc
            .with_ymd_and_hms(2026, 10, 21, 12, 2, 0)
            .unwrap()
            .to_rfc3339(),
        ],
      )
      .unwrap();
  }
  transaction.commit().unwrap();
  drop(connection);

  let store = DurableEventStore::open(database.path()).unwrap();
  let inspection = store
    .inspect_rich_approval("run_rich_review_1", "aprreq_rich_review_1", usize::MAX)
    .unwrap();
  assert_eq!(inspection.total_decisions, 10_000);
  assert_eq!(inspection.decisions.len(), 100);
  assert!(inspection.decisions_truncated);
}

#[test]
fn online_backup_preserves_the_review_and_partial_quorum_exactly() {
  let database = TemporaryDatabase::new("backup-source");
  let backup = TemporaryDatabase::new("backup-copy");
  let mut store = running_store_at(database.path());
  store
    .create_rich_approval_request(
      &binding(),
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .unwrap();
  store
    .record_rich_approval_decision(decision(
      "person:alice",
      ApprovalDecision::Approved,
      &["production-owner"],
      2,
    ))
    .unwrap();
  drop(store);
  create_online_backup(
    database.path(),
    backup.path(),
    "rich-approval-backup",
    "rpa3-test",
    "rpa3-deployment",
  )
  .unwrap();
  let copied = DurableEventStore::open(backup.path()).unwrap();
  let inspection = copied
    .inspect_rich_approval("run_rich_review_1", "aprreq_rich_review_1", 10)
    .unwrap();
  assert_eq!(inspection.review_digest, binding().review_digest);
  assert_eq!(inspection.status, "pending");
  assert_eq!(inspection.total_decisions, 1);
}

#[test]
fn partial_quorum_survives_restart_and_releases_artifact_pins_on_settlement() {
  let database = TemporaryDatabase::new("restart");
  let mut review = review();
  review["artifacts"] = json!([{
    "contract": "woml.artifact-reference/v1",
    "contractVersion": 1,
    "id": "artifact-plan-1",
    "name": "plan.tfplan",
    "size": 42,
    "sha256": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "contentType": "application/octet-stream"
  }]);
  let bound =
    bind_rich_approval_review(binding_input(review), &ExactArtifacts::default(), &[]).unwrap();
  let mut store = running_store_at(database.path());
  store
    .create_rich_approval_request(
      &bound,
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .unwrap();
  let mut first = decision(
    "person:alice",
    ApprovalDecision::Approved,
    &["production-owner"],
    2,
  );
  first.review_digest = bound.review_digest.clone();
  store.record_rich_approval_decision(first).unwrap();
  drop(store);

  let mut reopened = DurableEventStore::open(database.path()).unwrap();
  let inspection = reopened
    .inspect_rich_approval("run_rich_review_1", "aprreq_rich_review_1", 10)
    .unwrap();
  assert_eq!(inspection.status, "pending");
  assert_eq!(inspection.total_decisions, 1);
  let mut second = decision(
    "person:bob",
    ApprovalDecision::Approved,
    &["security-owner"],
    3,
  );
  second.review_digest = bound.review_digest;
  assert_eq!(
    reopened
      .record_rich_approval_decision(second)
      .unwrap()
      .status,
    RichApprovalDecisionStatus::Approved
  );
  drop(reopened);

  let connection = Connection::open(database.path()).unwrap();
  assert!(connection
    .execute(
      "UPDATE woml_approval_reviews SET review_digest = ?1 WHERE run_id = ?2",
      params![
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "run_rich_review_1"
      ],
    )
    .is_err());
  assert!(connection
    .execute(
      "UPDATE woml_approval_artifact_pins SET artifact_sha256 = ?1 WHERE run_id = ?2",
      params![
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "run_rich_review_1"
      ],
    )
    .is_err());
  let released: i64 = connection
    .query_row(
      "SELECT COUNT(*) FROM woml_approval_artifact_pins WHERE released_at IS NOT NULL",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(released, 1);
}

#[test]
fn store_v17_migrates_additively_to_v18_and_future_versions_fail_closed() {
  let database = TemporaryDatabase::new("migration");
  drop(DurableEventStore::open(database.path()).unwrap());
  let connection = Connection::open(database.path()).unwrap();
  connection
    .execute_batch(
      "DROP TRIGGER woml_approval_reviews_no_delete;
       DROP TRIGGER woml_approval_decisions_no_update;
       DROP TRIGGER woml_approval_decisions_no_delete;
       DROP TABLE woml_approval_artifact_pins;
       DROP TABLE woml_approval_reviewer_decisions;
       DROP TABLE woml_approval_reviews;
       UPDATE woml_store_metadata SET value = '17' WHERE key = 'schema_version';",
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
  for object in [
    "woml_approval_reviews",
    "woml_approval_reviewer_decisions",
    "woml_approval_artifact_pins",
  ] {
    let exists: bool = connection
      .query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![object],
        |row| row.get(0),
      )
      .unwrap();
    assert!(exists, "missing {object}");
  }
  connection
    .execute(
      "UPDATE woml_store_metadata SET value = '999' WHERE key = 'schema_version'",
      [],
    )
    .unwrap();
  drop(connection);
  assert!(matches!(
    DurableEventStore::open(database.path()),
    Err(DurableStoreError::UnsupportedStoreVersion(version)) if version == "999"
  ));
}

#[test]
fn rejection_timeout_cancellation_and_invalidation_share_the_terminal_race() {
  for reason in [
    RichApprovalResolutionReason::Timeout,
    RichApprovalResolutionReason::Cancellation,
    RichApprovalResolutionReason::ArtifactInvalidated,
  ] {
    let mut store = running_store();
    store
      .create_rich_approval_request(
        &binding(),
        Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
      )
      .unwrap();
    let outcome = store
      .settle_rich_approval_without_vote(
        "run_rich_review_1",
        "aprreq_rich_review_1",
        &binding().review_digest,
        reason,
        Utc.with_ymd_and_hms(2026, 10, 21, 12, 2, 0).unwrap(),
      )
      .unwrap();
    assert_eq!(outcome.status, RichApprovalDecisionStatus::Rejected);
    let late = store
      .record_rich_approval_decision(decision(
        "person:late",
        ApprovalDecision::Approved,
        &["production-owner", "security-owner"],
        3,
      ))
      .unwrap();
    assert_eq!(late.status, RichApprovalDecisionStatus::AlreadyResolved);
  }

  let mut store = running_store();
  store
    .create_rich_approval_request(
      &binding(),
      Utc.with_ymd_and_hms(2026, 10, 21, 12, 1, 0).unwrap(),
    )
    .unwrap();
  let rejected = store
    .record_rich_approval_decision(decision(
      "person:veto",
      ApprovalDecision::Rejected,
      &["security-owner"],
      2,
    ))
    .unwrap();
  assert_eq!(rejected.status, RichApprovalDecisionStatus::Rejected);
  assert_eq!(rejected.received, 0);
}
