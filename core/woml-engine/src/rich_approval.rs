//! Frozen wire types for Rich Plan Approvals v1.
//!
//! RPA0 owns these language-neutral shapes. RPA2 adds bounded semantic
//! validation, artifact resolution, secret scanning, and immutable binding.

use serde::{Deserialize, Serialize};

use crate::ArtifactReferenceV1;

pub const RICH_PLAN_APPROVAL_PROFILE: &str = "woml.rich-plan-approvals/v1";
pub const REVIEW_PAYLOAD_VERSION: u32 = 1;
pub const REVIEWER_DIRECTORY_PROFILE: &str = "woml.reviewer-directory/v1";
pub const REVIEW_PAYLOAD_MAX_BYTES: usize = 65_536;
pub const REVIEW_PAYLOAD_MAX_NESTING: usize = 8;
pub const RICH_APPROVAL_MAX_PRINCIPALS: u32 = 20;
pub const RICH_APPROVAL_MAX_ROLES: usize = 16;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
