use std::path::PathBuf;

use serde_json::Value;
use woml_engine::{
  fold_events, CompiledWorkflowDefinition, ReviewPayloadV1, ReviewerDirectoryV1, RunEvent,
  REVIEWER_DIRECTORY_PROFILE,
};

const MODEL: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/model.v20.reviewed.json");
const REVIEW: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/review.v1.json");
const DIRECTORY: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/reviewer-directory.v1.json");
const EVENTS: &str =
  include_str!("../../../woml/tests/fixtures/rich-plan-approvals/events.v19.json");

fn model() -> CompiledWorkflowDefinition {
  serde_json::from_str(MODEL).expect("reviewed Model v20 fixture")
}

#[test]
fn rust_and_typescript_share_fail_closed_review_and_directory_wire_shapes() {
  let review: ReviewPayloadV1 = serde_json::from_str(REVIEW).expect("Review Payload v1");
  assert_eq!(review.title, "Deploy payments-api to production");
  let directory: ReviewerDirectoryV1 =
    serde_json::from_str(DIRECTORY).expect("Reviewer Directory v1");
  assert_eq!(directory.profile, REVIEWER_DIRECTORY_PROFILE);
  assert_eq!(directory.principals.len(), 2);

  let mut unknown_review: Value = serde_json::from_str(REVIEW).unwrap();
  unknown_review["callbackHtml"] = Value::String("<button>approve</button>".to_string());
  assert!(serde_json::from_value::<ReviewPayloadV1>(unknown_review).is_err());
  let mut untrusted_directory: Value = serde_json::from_str(DIRECTORY).unwrap();
  untrusted_directory["principals"][0]["workflowGrantedRole"] = Value::Bool(true);
  assert!(serde_json::from_value::<ReviewerDirectoryV1>(untrusted_directory).is_err());
}

#[test]
fn reviewed_model_v20_is_structurally_valid_and_durably_executable() {
  let workflow = model();
  workflow.validate_structure().expect("Model v20 structure");
  workflow
    .validate_for_durable_execution()
    .expect("RPA5 enables Model v20 execution behind runtime reviewer authority");
}

#[test]
fn rust_rejects_malformed_or_backported_rich_approval_descriptors() {
  let mut malformed: Value = serde_json::from_str(MODEL).unwrap();
  let wait = malformed["graph"]["nodes"]
    .as_array_mut()
    .unwrap()
    .iter_mut()
    .find(|node| node["id"] == "approveDeployment")
    .unwrap();
  wait["inputs"]["fields"]["requiredApprovals"]["value"] = Value::from(0);
  let malformed: CompiledWorkflowDefinition = serde_json::from_value(malformed).unwrap();
  assert!(malformed.validate_structure().is_err());

  let mut backported: Value = serde_json::from_str(MODEL).unwrap();
  backported["schemaVersion"] = Value::from(19);
  let backported: CompiledWorkflowDefinition = serde_json::from_value(backported).unwrap();
  assert!(backported.validate_structure().is_err());
}

#[test]
fn event_v19_decoding_and_folding_fail_closed_under_adversarial_mutations() {
  let mut wire: Value = serde_json::from_str(EVENTS).expect("public Event v19 fixture");
  for (index, event) in wire.as_array_mut().unwrap().iter_mut().enumerate() {
    let object = event.as_object_mut().unwrap();
    let schema_version = object.remove("schemaVersion").unwrap();
    let occurred_at = object.remove("at").unwrap();
    object.insert("eventSchemaVersion".to_string(), schema_version);
    object.insert(
      "eventId".to_string(),
      Value::String(format!("evt-review-{index}")),
    );
    object.insert("occurredAt".to_string(), occurred_at);
  }
  let events: Vec<RunEvent> = serde_json::from_value(wire).expect("internal Event v19 wire shape");
  for event in &events {
    event.validate().expect("valid rich approval event");
  }
  assert!(
    fold_events(&events).is_err(),
    "a partial history must fail closed"
  );

  let mut unknown = serde_json::to_value(&events).unwrap();
  unknown[0]["credential"] = Value::String("must-not-enter-events".to_string());
  assert!(serde_json::from_value::<Vec<RunEvent>>(unknown).is_err());

  let mut wrong_digest = serde_json::to_value(&events).unwrap();
  wrong_digest[0]["data"]["reviewDigest"] = Value::String(format!("sha256:{}", "A".repeat(64)));
  let wrong_digest: Vec<RunEvent> = serde_json::from_value(wrong_digest).unwrap();
  assert!(wrong_digest[0].validate().is_err());

  let mut wrong_version = events.clone();
  wrong_version[0].event_schema_version -= 1;
  assert!(wrong_version[0].validate().is_err());

  let mut duplicate_sequence = events;
  duplicate_sequence[1].sequence = duplicate_sequence[0].sequence;
  assert!(fold_events(&duplicate_sequence).is_err());
}

#[test]
fn typescript_and_rust_share_the_exact_reviewed_model_v20_json() {
  let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
  let fixture = repository.join("woml/tests/fixtures/rich-plan-approvals/model.reviewed.woml");
  let output = std::process::Command::new("bun")
    .current_dir(&repository)
    .env("WOML_FIXTURE", fixture)
    .args([
      "-e",
      "import {readFileSync} from 'node:fs'; import {parseWoml,compileWoml} from './woml/src/index.ts'; process.stdout.write(JSON.stringify(compileWoml(parseWoml(readFileSync(process.env.WOML_FIXTURE,'utf8')))));",
    ])
    .output()
    .expect("run TypeScript compiler");
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let typescript: Value = serde_json::from_slice(&output.stdout).unwrap();
  let reviewed: Value = serde_json::from_str(MODEL).unwrap();
  assert_eq!(typescript, reviewed);
  let rust: CompiledWorkflowDefinition = serde_json::from_value(reviewed.clone()).unwrap();
  assert_eq!(serde_json::to_value(rust).unwrap(), reviewed);
}
