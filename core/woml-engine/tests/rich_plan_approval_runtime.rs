use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use serde_json::Map;
use uuid::Uuid;
use woml_engine::{
  execute_workflow_durable_outcome, resolve_rich_approval_durable,
  resume_workflow_durable_any_outcome, resume_workflow_durable_outcome, ApprovalDecision,
  CompiledWorkflowDefinition, DurableEventStore, ReviewerDirectoryAuthorityV1, ReviewerDirectoryV1,
  ReviewerIdentityClaimV1, ReviewerProviderV1, RichApprovalDecisionStatus,
  RichApprovalIdentityDecision, RichApprovalRequestStatus, RunStatus, RuntimeExecutionError,
  RuntimeExecutionOptions, ScriptHostProcessOptions, WorkflowRuntimeOutcome,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/model.v20.reviewed.json");
const DIRECTORY: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/reviewer-directory.v1.json");
const DEFINITION_HASH: &str =
  "sha256:2020202020202020202020202020202020202020202020202020202020202020";

struct TemporaryDatabase(PathBuf);

impl TemporaryDatabase {
  fn new(label: &str) -> Self {
    Self(std::env::temp_dir().join(format!(
      "woml-rpa5-{label}-{}.sqlite",
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
  }
}

fn host_options() -> Option<ScriptHostProcessOptions> {
  std::process::Command::new("bun")
    .arg("--version")
    .output()
    .ok()?
    .status
    .success()
    .then(|| {
      ScriptHostProcessOptions::new(
        PathBuf::from("bun"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../woml-cli/src/script-host.ts"),
      )
    })
}

fn authority() -> Arc<ReviewerDirectoryAuthorityV1> {
  let directory: ReviewerDirectoryV1 = serde_json::from_str(DIRECTORY).unwrap();
  Arc::new(ReviewerDirectoryAuthorityV1::from_document(directory).unwrap())
}

fn options(
  host: ScriptHostProcessOptions,
  authority: Arc<ReviewerDirectoryAuthorityV1>,
) -> RuntimeExecutionOptions {
  RuntimeExecutionOptions::new(host, 5_000).with_reviewer_directory(authority)
}

fn vote(
  database: &Path,
  run_id: &str,
  request_id: &str,
  review_digest: &str,
  identity: ReviewerIdentityClaimV1,
) -> woml_engine::RichApprovalDecisionOutcome {
  resolve_rich_approval_durable(
    database.to_path_buf(),
    RichApprovalIdentityDecision {
      run_id: run_id.to_string(),
      request_id: request_id.to_string(),
      review_digest: review_digest.to_string(),
      decision: ApprovalDecision::Approved,
      identity,
      decided_at: Utc::now(),
    },
  )
  .unwrap()
}

#[tokio::test]
async fn model_v20_preserves_partial_quorum_across_restart_and_continues_once() {
  let Some(host) = host_options() else { return };
  let database = TemporaryDatabase::new("quorum-restart");
  let reviewer_authority = authority();
  let workflow: CompiledWorkflowDefinition = serde_json::from_str(MODEL).unwrap();

  let first = execute_workflow_durable_outcome(
    workflow,
    DEFINITION_HASH.to_string(),
    Map::new(),
    options(host.clone(), Arc::clone(&reviewer_authority)),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::Waiting {
    run_id,
    approval: first_wait,
    ..
  } = first
  else {
    panic!("rich approval must suspend before either decision arm")
  };
  assert!(first_wait.token.is_none());
  assert!(first_wait.credential_expires_at.is_none());
  let digest = first_wait.review_digest.clone().unwrap();
  assert_eq!(first_wait.authority.as_ref().unwrap().received, 0);

  let first_vote = vote(
    database.path(),
    &run_id,
    &first_wait.request_id,
    &digest,
    ReviewerIdentityClaimV1 {
      provider: ReviewerProviderV1::Slack,
      account_id: "T0123456789".to_string(),
      actor_id: "U0123456789".to_string(),
    },
  );
  assert_eq!(first_vote.status, RichApprovalDecisionStatus::Pending);
  assert_eq!(first_vote.received, 1);

  // A new runtime instance reconstructs the immutable review and partial
  // quorum from SQLite; it does not mint a basic token or replay buildReview.
  let restarted = resume_workflow_durable_outcome(
    database.path().to_path_buf(),
    &run_id,
    options(host.clone(), Arc::clone(&reviewer_authority)),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::Waiting {
    approval: restarted_wait,
    ..
  } = restarted
  else {
    panic!("one of two required approvals must remain waiting")
  };
  assert_eq!(restarted_wait.request_id, first_wait.request_id);
  assert_eq!(
    restarted_wait.review_digest.as_deref(),
    Some(digest.as_str())
  );
  assert_eq!(restarted_wait.authority.as_ref().unwrap().received, 1);
  assert_eq!(
    restarted_wait.authority.as_ref().unwrap().represented_roles,
    vec!["production-owner"]
  );

  let second_vote = vote(
    database.path(),
    &run_id,
    &first_wait.request_id,
    &digest,
    ReviewerIdentityClaimV1 {
      provider: ReviewerProviderV1::Telegram,
      account_id: "bot-42".to_string(),
      actor_id: "812345678".to_string(),
    },
  );
  assert_eq!(second_vote.status, RichApprovalDecisionStatus::Approved);
  assert_eq!(second_vote.received, 2);

  let completed = resume_workflow_durable_outcome(
    database.path().to_path_buf(),
    &run_id,
    options(host, reviewer_authority),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::Succeeded { execution, .. } = completed else {
    panic!("quorum must continue the approved arm")
  };
  assert_eq!(execution.result["decision"], "approved");
  assert_eq!(execution.result["review"]["digest"], digest);
  assert_eq!(
    execution.result["review"]["title"],
    "Deploy payments-api to production"
  );
  assert_eq!(execution.result["authority"]["required"], 2);
  assert_eq!(execution.result["authority"]["received"], 2);
  let public = serde_json::to_string(&execution.result).unwrap();
  assert!(!public.contains("person:alice"));
  assert!(!public.contains("U0123456789"));
  assert_eq!(
    execution
      .execution_order
      .iter()
      .filter(|node| node.as_str() == "buildReview")
      .count(),
    1
  );
  assert_eq!(
    DurableEventStore::open(database.path())
      .unwrap()
      .projection(&run_id)
      .unwrap()
      .status,
    RunStatus::Succeeded
  );
}

#[tokio::test]
async fn cancelling_a_waiting_rich_approval_settles_it_before_run_cancellation() {
  let Some(host) = host_options() else { return };
  let database = TemporaryDatabase::new("cancel");
  let reviewer_authority = authority();
  let workflow: CompiledWorkflowDefinition = serde_json::from_str(MODEL).unwrap();
  let waiting = execute_workflow_durable_outcome(
    workflow,
    DEFINITION_HASH.to_string(),
    Map::new(),
    options(host.clone(), Arc::clone(&reviewer_authority)),
    database.path().to_path_buf(),
  )
  .await
  .unwrap();
  let WorkflowRuntimeOutcome::Waiting { run_id, .. } = waiting else {
    panic!("rich approval must wait")
  };
  let mut store = DurableEventStore::open(database.path()).unwrap();
  store
    .request_run_cancellation(&run_id, "cancel-rich-review", Utc::now())
    .unwrap();
  let error = resume_workflow_durable_any_outcome(
    database.path().to_path_buf(),
    &run_id,
    options(host, reviewer_authority),
  )
  .await
  .unwrap_err();
  assert!(matches!(error, RuntimeExecutionError::RunCancelled(_)));
  let projection = DurableEventStore::open(database.path())
    .unwrap()
    .projection(&run_id)
    .unwrap();
  assert_eq!(projection.status, RunStatus::Cancelled);
  assert!(projection
    .rich_approval_requests
    .values()
    .all(|request| { request.status == RichApprovalRequestStatus::Cancelled }));
}
