//! Frozen wire types for Rich Plan Approvals v1.
//!
//! RPA0 owns these language-neutral shapes. RPA2 adds bounded semantic
//! validation, artifact resolution, secret scanning, and immutable binding.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_json_canonicalizer::to_vec as canonical_json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::model::ValueExpression;
use crate::{ArtifactReferenceV1, WorkflowContext};

pub const RICH_PLAN_APPROVAL_PROFILE: &str = "woml.rich-plan-approvals/v1";
pub const REVIEW_PAYLOAD_VERSION: u32 = 1;
pub const REVIEWER_DIRECTORY_PROFILE: &str = "woml.reviewer-directory/v1";
pub const REVIEW_PAYLOAD_MAX_BYTES: usize = 65_536;
pub const REVIEW_PAYLOAD_MAX_NESTING: usize = 8;
pub const RICH_APPROVAL_MAX_PRINCIPALS: u32 = 20;
pub const RICH_APPROVAL_MAX_ROLES: usize = 16;
pub const REVIEW_FACTS_MAX_ITEMS: usize = 32;
pub const REVIEW_CHANGES_MAX_ITEMS: usize = 100;
pub const REVIEW_CHECKS_MAX_ITEMS: usize = 64;
pub const REVIEW_ARTIFACTS_MAX_ITEMS: usize = 32;
pub const REVIEW_SECRET_MIN_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewRiskV1 {
  Low,
  Medium,
  High,
  Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFactV1 {
  pub label: String,
  pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewChangeActionV1 {
  Create,
  Update,
  Replace,
  Destroy,
  NoOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewChangeV1 {
  pub action: ReviewChangeActionV1,
  pub target: String,
  pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewCheckStatusV1 {
  Passed,
  Failed,
  Warning,
  Skipped,
  Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCheckV1 {
  pub name: String,
  pub status: ReviewCheckStatusV1,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPayloadV1 {
  pub title: String,
  pub summary: String,
  pub risk: ReviewRiskV1,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub facts: Vec<ReviewFactV1>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub changes: Vec<ReviewChangeV1>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub checks: Vec<ReviewCheckV1>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub artifacts: Vec<ArtifactReferenceV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewerProviderV1 {
  Slack,
  Telegram,
  Discord,
  TrustedHttp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerIdentityV1 {
  pub provider: ReviewerProviderV1,
  pub account_id: String,
  pub actor_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerPrincipalV1 {
  pub id: String,
  pub roles: Vec<String>,
  pub identities: Vec<ReviewerIdentityV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerDirectoryV1 {
  pub profile: String,
  pub version: String,
  pub principals: Vec<ReviewerPrincipalV1>,
}

impl ReviewerProviderV1 {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Slack => "slack",
      Self::Telegram => "telegram",
      Self::Discord => "discord",
      Self::TrustedHttp => "trusted-http",
    }
  }
}

pub const REVIEWER_DIRECTORY_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const REVIEWER_DIRECTORY_MAX_PRINCIPALS: usize = 10_000;
pub const REVIEWER_DIRECTORY_MAX_ROLES: usize = 64;
pub const REVIEWER_DIRECTORY_MAX_IDENTITIES: usize = 32;
const REVIEWER_DIRECTORY_RETAINED_VERSIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedReviewerV1 {
  pub principal_id: String,
  pub roles: Vec<String>,
  pub provider: ReviewerProviderV1,
  pub account_id: String,
  pub actor_id: String,
  pub directory_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerIdentityClaimV1 {
  pub provider: ReviewerProviderV1,
  pub account_id: String,
  pub actor_id: String,
}

#[derive(Debug, Clone)]
struct PreparedReviewerDirectoryV1 {
  document: ReviewerDirectoryV1,
  content_hash: String,
  identities: HashMap<(ReviewerProviderV1, String, String), (String, Vec<String>)>,
}

#[derive(Debug, Default)]
struct ReviewerDirectoryStateV1 {
  active_version: String,
  snapshots: BTreeMap<String, Arc<PreparedReviewerDirectoryV1>>,
}

/// Operator-owned reviewer authority. Reload first validates a complete new
/// snapshot, then swaps it atomically. Older versions remain available for
/// already-pending approvals, so changing the active directory cannot change
/// who was authorized for an immutable review.
#[derive(Debug, Clone, Default)]
pub struct ReviewerDirectoryAuthorityV1 {
  state: Arc<RwLock<ReviewerDirectoryStateV1>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReviewerDirectoryError {
  #[error("Reviewer Directory v1 is unavailable")]
  Unavailable,
  #[error("Reviewer Directory v1 is invalid")]
  Invalid,
  #[error("Reviewer Directory v1 exceeds its frozen size limit")]
  TooLarge,
  #[error("a reviewer identity is unknown")]
  ReviewerUnknown,
  #[error("the reviewer is not eligible for this approval")]
  ReviewerForbidden,
  #[error("the approval is pinned to an unavailable reviewer-directory version")]
  StaleDirectory,
  #[error("a reviewer-directory version was reused with different content")]
  VersionConflict,
  #[error("the reviewer directory cannot satisfy the requested quorum or roles")]
  RequirementsUnsatisfied,
}

impl ReviewerDirectoryError {
  pub const fn code(&self) -> &'static str {
    match self {
      Self::Unavailable | Self::StaleDirectory | Self::RequirementsUnsatisfied => {
        "WOML_APPROVAL_AUTHORITY_UNAVAILABLE"
      }
      Self::ReviewerUnknown => "WOML_APPROVAL_REVIEWER_UNKNOWN",
      Self::ReviewerForbidden => "WOML_APPROVAL_REVIEWER_FORBIDDEN",
      Self::Invalid | Self::TooLarge | Self::VersionConflict => {
        "WOML_APPROVAL_AUTHORITY_UNAVAILABLE"
      }
    }
  }
}

impl ReviewerDirectoryAuthorityV1 {
  pub fn from_document(document: ReviewerDirectoryV1) -> Result<Self, ReviewerDirectoryError> {
    let authority = Self::default();
    authority.reload(document)?;
    Ok(authority)
  }

  pub fn load(path: &Path) -> Result<Self, ReviewerDirectoryError> {
    let bytes = fs::read(path).map_err(|_| ReviewerDirectoryError::Unavailable)?;
    if bytes.len() > REVIEWER_DIRECTORY_MAX_BYTES {
      return Err(ReviewerDirectoryError::TooLarge);
    }
    let document = serde_json::from_slice(&bytes).map_err(|_| ReviewerDirectoryError::Invalid)?;
    Self::from_document(document)
  }

  pub fn reload_from_path(&self, path: &Path) -> Result<String, ReviewerDirectoryError> {
    let bytes = fs::read(path).map_err(|_| ReviewerDirectoryError::Unavailable)?;
    if bytes.len() > REVIEWER_DIRECTORY_MAX_BYTES {
      return Err(ReviewerDirectoryError::TooLarge);
    }
    let document = serde_json::from_slice(&bytes).map_err(|_| ReviewerDirectoryError::Invalid)?;
    self.reload(document)
  }

  pub fn reload(&self, document: ReviewerDirectoryV1) -> Result<String, ReviewerDirectoryError> {
    let prepared = Arc::new(prepare_reviewer_directory(document)?);
    let version = prepared.document.version.clone();
    let mut state = self
      .state
      .write()
      .map_err(|_| ReviewerDirectoryError::Unavailable)?;
    if let Some(existing) = state.snapshots.get(&version) {
      if existing.content_hash != prepared.content_hash {
        return Err(ReviewerDirectoryError::VersionConflict);
      }
      state.active_version = version.clone();
      return Ok(version);
    }
    state.snapshots.insert(version.clone(), prepared);
    state.active_version = version.clone();
    while state.snapshots.len() > REVIEWER_DIRECTORY_RETAINED_VERSIONS {
      let removable = state
        .snapshots
        .keys()
        .find(|candidate| candidate.as_str() != state.active_version)
        .cloned();
      let Some(removable) = removable else { break };
      state.snapshots.remove(&removable);
    }
    Ok(version)
  }

  pub fn active_version(&self) -> Result<String, ReviewerDirectoryError> {
    let state = self
      .state
      .read()
      .map_err(|_| ReviewerDirectoryError::Unavailable)?;
    if state.active_version.is_empty() {
      return Err(ReviewerDirectoryError::Unavailable);
    }
    Ok(state.active_version.clone())
  }

  pub fn active_content_hash(&self) -> Result<String, ReviewerDirectoryError> {
    let state = self
      .state
      .read()
      .map_err(|_| ReviewerDirectoryError::Unavailable)?;
    state
      .snapshots
      .get(&state.active_version)
      .map(|directory| directory.content_hash.clone())
      .ok_or(ReviewerDirectoryError::Unavailable)
  }

  pub fn validate_requirements(
    &self,
    version: &str,
    required: u32,
    required_roles: &[String],
  ) -> Result<(), ReviewerDirectoryError> {
    let directory = self.snapshot(version)?;
    let eligible = directory
      .document
      .principals
      .iter()
      .filter(|principal| reviewer_is_eligible(&principal.roles, required_roles))
      .count();
    if required == 0
      || required > RICH_APPROVAL_MAX_PRINCIPALS
      || eligible < required as usize
      || required_roles.iter().any(|role| {
        !directory
          .document
          .principals
          .iter()
          .any(|principal| principal.roles.contains(role))
      })
    {
      return Err(ReviewerDirectoryError::RequirementsUnsatisfied);
    }
    Ok(())
  }

  pub fn resolve(
    &self,
    directory_version: &str,
    claim: &ReviewerIdentityClaimV1,
    required_roles: &[String],
  ) -> Result<VerifiedReviewerV1, ReviewerDirectoryError> {
    let directory = self.snapshot(directory_version)?;
    let key = (
      claim.provider,
      claim.account_id.clone(),
      claim.actor_id.clone(),
    );
    let (principal_id, roles) = directory
      .identities
      .get(&key)
      .ok_or(ReviewerDirectoryError::ReviewerUnknown)?;
    if !reviewer_is_eligible(roles, required_roles) {
      return Err(ReviewerDirectoryError::ReviewerForbidden);
    }
    Ok(VerifiedReviewerV1 {
      principal_id: principal_id.clone(),
      roles: roles.clone(),
      provider: claim.provider,
      account_id: claim.account_id.clone(),
      actor_id: claim.actor_id.clone(),
      directory_version: directory_version.to_string(),
    })
  }

  fn snapshot(
    &self,
    version: &str,
  ) -> Result<Arc<PreparedReviewerDirectoryV1>, ReviewerDirectoryError> {
    self
      .state
      .read()
      .map_err(|_| ReviewerDirectoryError::Unavailable)?
      .snapshots
      .get(version)
      .cloned()
      .ok_or(ReviewerDirectoryError::StaleDirectory)
  }
}

fn prepare_reviewer_directory(
  document: ReviewerDirectoryV1,
) -> Result<PreparedReviewerDirectoryV1, ReviewerDirectoryError> {
  if document.profile != REVIEWER_DIRECTORY_PROFILE
    || !valid_bounded_text(&document.version, 128)
    || document.principals.is_empty()
    || document.principals.len() > REVIEWER_DIRECTORY_MAX_PRINCIPALS
  {
    return Err(ReviewerDirectoryError::Invalid);
  }
  let canonical = canonical_json(&document).map_err(|_| ReviewerDirectoryError::Invalid)?;
  if canonical.len() > REVIEWER_DIRECTORY_MAX_BYTES {
    return Err(ReviewerDirectoryError::TooLarge);
  }
  let content_hash = format!("sha256:{:x}", Sha256::digest(canonical));
  let mut principal_ids = HashSet::new();
  let mut identities = HashMap::new();
  for principal in &document.principals {
    if !valid_principal_id(&principal.id)
      || !principal_ids.insert(principal.id.clone())
      || principal.roles.is_empty()
      || principal.roles.len() > REVIEWER_DIRECTORY_MAX_ROLES
      || !principal.roles.windows(2).all(|roles| roles[0] < roles[1])
      || principal.roles.iter().any(|role| !valid_role(role))
      || principal.identities.is_empty()
      || principal.identities.len() > REVIEWER_DIRECTORY_MAX_IDENTITIES
    {
      return Err(ReviewerDirectoryError::Invalid);
    }
    for identity in &principal.identities {
      if !valid_bounded_text(&identity.account_id, 256)
        || !valid_bounded_text(&identity.actor_id, 256)
        || identities
          .insert(
            (
              identity.provider,
              identity.account_id.clone(),
              identity.actor_id.clone(),
            ),
            (principal.id.clone(), principal.roles.clone()),
          )
          .is_some()
      {
        return Err(ReviewerDirectoryError::Invalid);
      }
    }
  }
  Ok(PreparedReviewerDirectoryV1 {
    document,
    content_hash,
    identities,
  })
}

fn reviewer_is_eligible(roles: &[String], required_roles: &[String]) -> bool {
  required_roles.is_empty() || roles.iter().any(|role| required_roles.contains(role))
}

fn valid_bounded_text(value: &str, maximum: usize) -> bool {
  !value.is_empty()
    && value.len() <= maximum
    && value.trim() == value
    && !value.chars().any(char::is_control)
}

fn valid_principal_id(value: &str) -> bool {
  if value.is_empty() || value.len() > 128 {
    return false;
  }
  let mut segments = value.split(['-', '_', '.', ':']);
  let Some(first) = segments.next() else {
    return false;
  };
  !first.is_empty()
    && first.as_bytes()[0].is_ascii_lowercase()
    && first
      .bytes()
      .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    && segments.all(|segment| {
      !segment.is_empty()
        && segment
          .bytes()
          .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// Inputs whose exact values are authorized by one rich approval. The review
/// value has already become durable workflow context before this authority is
/// invoked; provider callbacks can never supply or replace it.
#[derive(Debug, Clone, PartialEq)]
pub struct RichApprovalBindingInput {
  pub definition_hash: String,
  pub workflow_id: String,
  pub workflow_version: String,
  pub run_id: String,
  pub approval_id: String,
  pub request_id: String,
  pub review: Value,
  pub required_approvals: u32,
  pub required_roles: Vec<String>,
  pub directory_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundRichApprovalReview {
  pub profile: String,
  pub definition_hash: String,
  pub workflow_id: String,
  pub workflow_version: String,
  pub run_id: String,
  pub approval_id: String,
  pub request_id: String,
  pub review: ReviewPayloadV1,
  pub artifacts: Vec<ArtifactReferenceV1>,
  pub required_approvals: u32,
  pub required_roles: Vec<String>,
  pub directory_version: String,
  pub review_digest: String,
}

/// The artifact implementation remains the sole authority for ownership,
/// availability, durable metadata, and backend integrity. Tests may provide a
/// deliberately small verifier without opening an artifact backend.
pub trait RichApprovalArtifactVerifier {
  fn verify_review_artifact(
    &self,
    run_id: &str,
    artifact: &ArtifactReferenceV1,
  ) -> Result<ArtifactReferenceV1, RichApprovalError>;
}

impl RichApprovalArtifactVerifier for crate::ManagedArtifactStore {
  fn verify_review_artifact(
    &self,
    run_id: &str,
    artifact: &ArtifactReferenceV1,
  ) -> Result<ArtifactReferenceV1, RichApprovalError> {
    self
      .verify_review_reference(run_id, artifact)
      .map_err(|_| RichApprovalError::ArtifactInvalid)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RichApprovalError {
  #[error("the review does not match Review Payload v1")]
  ReviewInvalid,
  #[error("the review exceeds a frozen Rich Plan Approval limit")]
  ReviewTooLarge,
  #[error("a known secret value appears in reviewable content")]
  ReviewSecretDetected,
  #[error("a reviewed artifact is unavailable, unauthorized, corrupt, or forged")]
  ArtifactInvalid,
  #[error("the immutable approval binding inputs are invalid")]
  BindingInvalid,
}

/// Resolves the compiler-reviewed reference against the event-projected
/// workflow context. Callers then pass this value to the immutable binder;
/// providers never participate in review construction or replacement.
pub fn resolve_rich_approval_review(
  reference: &ValueExpression,
  context: &WorkflowContext,
) -> Result<Value, RichApprovalError> {
  crate::engine::resolve_context_reference(reference, context)
    .map_err(|_| RichApprovalError::ReviewInvalid)
}

impl RichApprovalError {
  pub const fn code(&self) -> &'static str {
    match self {
      Self::ReviewInvalid => "WOML_APPROVAL_REVIEW_INVALID",
      Self::ReviewTooLarge => "WOML_APPROVAL_REVIEW_TOO_LARGE",
      Self::ReviewSecretDetected => "WOML_APPROVAL_REVIEW_SECRET_DETECTED",
      Self::ArtifactInvalid => "WOML_APPROVAL_ARTIFACT_INVALID",
      Self::BindingInvalid => "WOML_APPROVAL_BINDING_MISMATCH",
    }
  }
}

pub fn bind_rich_approval_review(
  input: RichApprovalBindingInput,
  artifact_verifier: &dyn RichApprovalArtifactVerifier,
  known_secrets: &[String],
) -> Result<BoundRichApprovalReview, RichApprovalError> {
  validate_binding_identity(&input)?;
  validate_json_depth(&input.review, 1)?;
  let review_bytes = canonical_json(&input.review).map_err(|_| RichApprovalError::ReviewInvalid)?;
  if review_bytes.len() > REVIEW_PAYLOAD_MAX_BYTES {
    return Err(RichApprovalError::ReviewTooLarge);
  }
  if contains_known_secret(&input.review, known_secrets) {
    return Err(RichApprovalError::ReviewSecretDetected);
  }
  let mut review: ReviewPayloadV1 =
    serde_json::from_value(input.review).map_err(|_| RichApprovalError::ReviewInvalid)?;
  validate_review_payload(&review)?;

  let mut artifact_ids = HashSet::new();
  let mut verified_artifacts = Vec::with_capacity(review.artifacts.len());
  for artifact in &review.artifacts {
    if !artifact_ids.insert(artifact.id.as_str()) {
      return Err(RichApprovalError::ArtifactInvalid);
    }
    let verified = artifact_verifier.verify_review_artifact(&input.run_id, artifact)?;
    if &verified != artifact {
      return Err(RichApprovalError::ArtifactInvalid);
    }
    verified_artifacts.push(verified);
  }
  review.artifacts = verified_artifacts.clone();

  let binding = json!({
    "definitionHash": input.definition_hash,
    "workflowId": input.workflow_id,
    "workflowVersion": input.workflow_version,
    "runId": input.run_id,
    "approvalId": input.approval_id,
    "requestId": input.request_id,
    "review": review,
    "artifacts": verified_artifacts,
    "requiredApprovals": input.required_approvals,
    "requiredRoles": input.required_roles,
    "directoryVersion": input.directory_version,
  });
  let canonical = canonical_json(&binding).map_err(|_| RichApprovalError::BindingInvalid)?;
  let review_digest = format!("sha256:{:x}", Sha256::digest(canonical));
  let Value::Object(mut binding) = binding else {
    unreachable!("rich approval binding is always an object")
  };
  let review = serde_json::from_value(
    binding
      .remove("review")
      .ok_or(RichApprovalError::BindingInvalid)?,
  )
  .map_err(|_| RichApprovalError::BindingInvalid)?;
  let artifacts = serde_json::from_value(
    binding
      .remove("artifacts")
      .ok_or(RichApprovalError::BindingInvalid)?,
  )
  .map_err(|_| RichApprovalError::BindingInvalid)?;

  Ok(BoundRichApprovalReview {
    profile: RICH_PLAN_APPROVAL_PROFILE.to_string(),
    definition_hash: take_string(&mut binding, "definitionHash")?,
    workflow_id: take_string(&mut binding, "workflowId")?,
    workflow_version: take_string(&mut binding, "workflowVersion")?,
    run_id: take_string(&mut binding, "runId")?,
    approval_id: take_string(&mut binding, "approvalId")?,
    request_id: take_string(&mut binding, "requestId")?,
    review,
    artifacts,
    required_approvals: take_u32(&mut binding, "requiredApprovals")?,
    required_roles: serde_json::from_value(
      binding
        .remove("requiredRoles")
        .ok_or(RichApprovalError::BindingInvalid)?,
    )
    .map_err(|_| RichApprovalError::BindingInvalid)?,
    directory_version: take_string(&mut binding, "directoryVersion")?,
    review_digest,
  })
}

/// Recomputes the canonical binding at the durable-store boundary. This does
/// not replace artifact verification or secret scanning in the binder; it
/// prevents callers from mutating an already-bound review before persistence.
pub fn verify_bound_rich_approval_review(
  binding: &BoundRichApprovalReview,
) -> Result<(), RichApprovalError> {
  if binding.profile != RICH_PLAN_APPROVAL_PROFILE || binding.review.artifacts != binding.artifacts
  {
    return Err(RichApprovalError::BindingInvalid);
  }
  validate_review_payload(&binding.review)?;
  let review =
    serde_json::to_value(&binding.review).map_err(|_| RichApprovalError::ReviewInvalid)?;
  validate_json_depth(&review, 1)?;
  if canonical_json(&review)
    .map_err(|_| RichApprovalError::ReviewInvalid)?
    .len()
    > REVIEW_PAYLOAD_MAX_BYTES
  {
    return Err(RichApprovalError::ReviewTooLarge);
  }
  validate_binding_identity(&RichApprovalBindingInput {
    definition_hash: binding.definition_hash.clone(),
    workflow_id: binding.workflow_id.clone(),
    workflow_version: binding.workflow_version.clone(),
    run_id: binding.run_id.clone(),
    approval_id: binding.approval_id.clone(),
    request_id: binding.request_id.clone(),
    review,
    required_approvals: binding.required_approvals,
    required_roles: binding.required_roles.clone(),
    directory_version: binding.directory_version.clone(),
  })?;
  let canonical = canonical_json(&json!({
    "definitionHash": &binding.definition_hash,
    "workflowId": &binding.workflow_id,
    "workflowVersion": &binding.workflow_version,
    "runId": &binding.run_id,
    "approvalId": &binding.approval_id,
    "requestId": &binding.request_id,
    "review": &binding.review,
    "artifacts": &binding.artifacts,
    "requiredApprovals": binding.required_approvals,
    "requiredRoles": &binding.required_roles,
    "directoryVersion": &binding.directory_version,
  }))
  .map_err(|_| RichApprovalError::BindingInvalid)?;
  let digest = format!("sha256:{:x}", Sha256::digest(canonical));
  if digest != binding.review_digest {
    return Err(RichApprovalError::BindingInvalid);
  }
  Ok(())
}

fn validate_binding_identity(input: &RichApprovalBindingInput) -> Result<(), RichApprovalError> {
  let roles = input.required_roles.iter().collect::<BTreeSet<_>>();
  if !valid_sha256(&input.definition_hash)
    || input.workflow_id.is_empty()
    || input.workflow_id.len() > 256
    || input.workflow_version.is_empty()
    || input.workflow_version.len() > 128
    || input.run_id.is_empty()
    || input.run_id.len() > 320
    || !valid_public_id(&input.approval_id)
    || !input.request_id.starts_with("aprreq_")
    || input.request_id.len() > 256
    || input.directory_version.is_empty()
    || input.directory_version.len() > 128
    || !(1..=RICH_APPROVAL_MAX_PRINCIPALS).contains(&input.required_approvals)
    || input.required_roles.len() > RICH_APPROVAL_MAX_ROLES
    || roles.len() != input.required_roles.len()
    || !input
      .required_roles
      .windows(2)
      .all(|pair| pair[0] < pair[1])
    || input.required_roles.iter().any(|role| !valid_role(role))
  {
    return Err(RichApprovalError::BindingInvalid);
  }
  Ok(())
}

fn validate_review_payload(review: &ReviewPayloadV1) -> Result<(), RichApprovalError> {
  if !valid_review_string(&review.title, 160)
    || !valid_review_string(&review.summary, 2_000)
    || review.facts.len() > REVIEW_FACTS_MAX_ITEMS
    || review.changes.len() > REVIEW_CHANGES_MAX_ITEMS
    || review.checks.len() > REVIEW_CHECKS_MAX_ITEMS
    || review.artifacts.len() > REVIEW_ARTIFACTS_MAX_ITEMS
    || review
      .facts
      .iter()
      .any(|fact| !valid_review_string(&fact.label, 64) || !valid_review_string(&fact.value, 512))
    || review.changes.iter().any(|change| {
      !valid_review_string(&change.target, 256) || !valid_review_string(&change.summary, 1_000)
    })
    || review.checks.iter().any(|check| {
      !valid_review_string(&check.name, 128)
        || check
          .explanation
          .as_ref()
          .is_some_and(|value| !valid_review_string(value, 1_000))
    })
  {
    return Err(RichApprovalError::ReviewInvalid);
  }
  Ok(())
}

fn validate_json_depth(value: &Value, depth: usize) -> Result<(), RichApprovalError> {
  if depth > REVIEW_PAYLOAD_MAX_NESTING {
    return Err(RichApprovalError::ReviewTooLarge);
  }
  match value {
    Value::Array(values) => {
      for value in values {
        validate_json_depth(value, depth + 1)?;
      }
    }
    Value::Object(values) => {
      for value in values.values() {
        validate_json_depth(value, depth + 1)?;
      }
    }
    _ => {}
  }
  Ok(())
}

fn contains_known_secret(value: &Value, secrets: &[String]) -> bool {
  match value {
    Value::String(value) => secrets
      .iter()
      .any(|secret| secret.len() >= REVIEW_SECRET_MIN_BYTES && value.contains(secret.as_str())),
    Value::Array(values) => values
      .iter()
      .any(|value| contains_known_secret(value, secrets)),
    Value::Object(values) => values
      .values()
      .any(|value| contains_known_secret(value, secrets)),
    _ => false,
  }
}

fn valid_review_string(value: &str, max_chars: usize) -> bool {
  let length = value.chars().count();
  length > 0
    && length <= max_chars
    && value
      .chars()
      .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
}

fn valid_role(value: &str) -> bool {
  if value.is_empty() || value.len() > 64 {
    return false;
  }
  let mut segments = value.split(['-', '_', '.']);
  let Some(first) = segments.next() else {
    return false;
  };
  !first.is_empty()
    && first.as_bytes()[0].is_ascii_lowercase()
    && first
      .bytes()
      .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    && segments.all(|segment| {
      !segment.is_empty()
        && segment
          .bytes()
          .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn valid_public_id(value: &str) -> bool {
  let mut bytes = value.bytes();
  bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
    && value.len() <= 256
    && bytes.all(|byte| byte.is_ascii_alphanumeric())
}

fn valid_sha256(value: &str) -> bool {
  value.len() == 71
    && value.starts_with("sha256:")
    && value[7..]
      .bytes()
      .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn take_string(
  object: &mut serde_json::Map<String, Value>,
  key: &str,
) -> Result<String, RichApprovalError> {
  object
    .remove(key)
    .and_then(|value| value.as_str().map(str::to_string))
    .ok_or(RichApprovalError::BindingInvalid)
}

fn take_u32(
  object: &mut serde_json::Map<String, Value>,
  key: &str,
) -> Result<u32, RichApprovalError> {
  object
    .remove(key)
    .and_then(|value| value.as_u64())
    .and_then(|value| u32::try_from(value).ok())
    .ok_or(RichApprovalError::BindingInvalid)
}
