//! Frozen authoring and identity contracts for Infrastructure Policy v1.
//!
//! IP0/IP1 intentionally stop at parsing, validation, canonical identity, and
//! signature verification. Evaluation and pre-effect enforcement arrive in
//! later phases and must consume only trusted operation descriptors.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json_canonicalizer::to_vec as canonical_json;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const INFRASTRUCTURE_POLICY_CONTRACT: &str = "woml.infrastructure-policy";
pub const INFRASTRUCTURE_POLICY_CONTRACT_VERSION: u32 = 1;
pub const INFRASTRUCTURE_POLICY_MAX_BYTES: usize = 1024 * 1024;
pub const INFRASTRUCTURE_POLICY_MAX_RULES: usize = 1024;
pub const INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES: usize = 128;
pub const INFRASTRUCTURE_POLICY_MAX_ROLES: usize = 64;
pub const INFRASTRUCTURE_POLICY_MAX_JSON_DEPTH: usize = 16;
pub const INFRASTRUCTURE_POLICY_MAX_JSON_VALUES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InfrastructurePolicyEffectV1 {
  Read,
  IdempotentWrite,
  UnsafeWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InfrastructurePolicyDefaultV1 {
  Allow,
  Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InfrastructurePolicyDecisionV1 {
  Allow,
  Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InfrastructurePolicyExecutionModeV1 {
  Normal,
  Compensation,
  Cleanup,
  Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicySignatureV1 {
  pub algorithm: String,
  pub key_id: String,
  pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyDefaultsV1 {
  pub read: InfrastructurePolicyDefaultV1,
  #[serde(rename = "idempotent-write")]
  pub idempotent_write: InfrastructurePolicyDefaultV1,
  #[serde(rename = "unsafe-write")]
  pub unsafe_write: InfrastructurePolicyDefaultV1,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyRuleMatchV1 {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capabilities: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub operations: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub effects: Option<Vec<InfrastructurePolicyEffectV1>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub workflows: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub targets: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub executables: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub argument_prefix: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub execution_modes: Option<Vec<InfrastructurePolicyExecutionModeV1>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyApprovalRequirementV1 {
  pub minimum: u32,
  pub roles: Vec<String>,
  pub bind_review_digest: bool,
  pub max_age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyArtifactRequirementV1 {
  pub minimum: u32,
  pub verified_integrity: bool,
  pub bind_review_digest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyLockRequirementV1 {
  pub key: String,
  pub minimum_remaining_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyValidityRequirementV1 {
  pub not_before: String,
  pub not_after: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyRequirementsV1 {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approval: Option<InfrastructurePolicyApprovalRequirementV1>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub artifacts: Option<InfrastructurePolicyArtifactRequirementV1>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub lock: Option<InfrastructurePolicyLockRequirementV1>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub valid_between: Option<InfrastructurePolicyValidityRequirementV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyRuleV1 {
  pub id: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,
  #[serde(rename = "match")]
  pub matcher: InfrastructurePolicyRuleMatchV1,
  pub decision: InfrastructurePolicyDecisionV1,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub requires: Option<InfrastructurePolicyRequirementsV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InfrastructurePolicyBundleV1 {
  pub contract: String,
  pub contract_version: u32,
  pub id: String,
  pub version: String,
  pub environment: String,
  pub defaults: InfrastructurePolicyDefaultsV1,
  pub rules: Vec<InfrastructurePolicyRuleV1>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub signature: Option<InfrastructurePolicySignatureV1>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfrastructurePolicyTrustV1 {
  /// Canonical base64url raw Ed25519 public keys indexed by operator key ID.
  pub keys: BTreeMap<String, String>,
  pub revoked_key_ids: BTreeSet<String>,
  pub require_signature: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfrastructurePolicySignatureStatusV1 {
  Unsigned,
  Verified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInfrastructurePolicyV1 {
  pub bundle: InfrastructurePolicyBundleV1,
  pub digest: String,
  pub canonical_payload: Vec<u8>,
  pub signature_status: InfrastructurePolicySignatureStatusV1,
  pub signing_key_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InfrastructurePolicyError {
  #[error("the infrastructure policy is unavailable")]
  Unavailable,
  #[error("the infrastructure policy exceeds its frozen size limit")]
  TooLarge,
  #[error("the infrastructure policy is invalid: {0}")]
  Invalid(String),
  #[error("the infrastructure policy signature is invalid: {0}")]
  SignatureInvalid(String),
}

impl InfrastructurePolicyError {
  pub const fn code(&self) -> &'static str {
    match self {
      Self::Unavailable => "WOML_POLICY_UNAVAILABLE",
      Self::TooLarge | Self::Invalid(_) => "WOML_POLICY_INVALID",
      Self::SignatureInvalid(_) => "WOML_POLICY_SIGNATURE_INVALID",
    }
  }
}

pub fn parse_infrastructure_policy(
  source: &[u8],
  trust: &InfrastructurePolicyTrustV1,
) -> Result<PreparedInfrastructurePolicyV1, InfrastructurePolicyError> {
  if source.len() > INFRASTRUCTURE_POLICY_MAX_BYTES {
    return Err(InfrastructurePolicyError::TooLarge);
  }
  let value: Value =
    serde_json::from_slice(source).map_err(|_| invalid("the policy is not valid strict JSON"))?;
  let (depth, values) = json_complexity(&value);
  if depth > INFRASTRUCTURE_POLICY_MAX_JSON_DEPTH || values > INFRASTRUCTURE_POLICY_MAX_JSON_VALUES
  {
    return Err(invalid("the policy exceeds its JSON complexity limit"));
  }
  // Deserialize from the original bytes as well. Struct deserialization
  // rejects duplicate known fields rather than accepting parser-dependent
  // last-key-wins behavior.
  let bundle: InfrastructurePolicyBundleV1 = serde_json::from_slice(source)
    .map_err(|_| invalid("the policy has missing, duplicate, unknown, or invalid fields"))?;
  prepare_infrastructure_policy(bundle, trust)
}

pub fn prepare_infrastructure_policy(
  bundle: InfrastructurePolicyBundleV1,
  trust: &InfrastructurePolicyTrustV1,
) -> Result<PreparedInfrastructurePolicyV1, InfrastructurePolicyError> {
  validate_bundle(&bundle)?;
  let mut payload = serde_json::to_value(&bundle)
    .map_err(|_| invalid("the policy cannot be serialized canonically"))?;
  payload
    .as_object_mut()
    .ok_or_else(|| invalid("the policy payload is not an object"))?
    .remove("signature");
  let canonical_payload =
    canonical_json(&payload).map_err(|_| invalid("the policy cannot be serialized canonically"))?;
  if canonical_payload.len() > INFRASTRUCTURE_POLICY_MAX_BYTES {
    return Err(InfrastructurePolicyError::TooLarge);
  }
  let digest = format!("sha256:{:x}", Sha256::digest(&canonical_payload));
  let (signature_status, signing_key_id) = match &bundle.signature {
    None if trust.require_signature => {
      return Err(signature_invalid("a signature is required"));
    }
    None => (InfrastructurePolicySignatureStatusV1::Unsigned, None),
    Some(signature) => {
      if trust.revoked_key_ids.contains(&signature.key_id) {
        return Err(signature_invalid("the signing key is revoked"));
      }
      let encoded_key = trust
        .keys
        .get(&signature.key_id)
        .ok_or_else(|| signature_invalid("the signing key is not trusted"))?;
      let public_key = decode_base64url(encoded_key, 32, "public key")?;
      let signature_bytes = decode_base64url(&signature.value, 64, "signature")?;
      UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&canonical_payload, &signature_bytes)
        .map_err(|_| signature_invalid("signature verification failed"))?;
      (
        InfrastructurePolicySignatureStatusV1::Verified,
        Some(signature.key_id.clone()),
      )
    }
  };
  Ok(PreparedInfrastructurePolicyV1 {
    bundle,
    digest,
    canonical_payload,
    signature_status,
    signing_key_id,
  })
}

/// Prepares an emergency overlay after proving that it can only remove
/// authority. A non-match is neutral and a matching rule can only deny.
pub fn prepare_emergency_deny_overlay(
  bundle: InfrastructurePolicyBundleV1,
  trust: &InfrastructurePolicyTrustV1,
) -> Result<PreparedInfrastructurePolicyV1, InfrastructurePolicyError> {
  let prepared = prepare_infrastructure_policy(bundle, trust)?;
  let defaults = &prepared.bundle.defaults;
  if defaults.read != InfrastructurePolicyDefaultV1::Allow
    || defaults.idempotent_write != InfrastructurePolicyDefaultV1::Allow
    || defaults.unsafe_write != InfrastructurePolicyDefaultV1::Allow
  {
    return Err(invalid(
      "an emergency deny overlay must use allow for every default",
    ));
  }
  if prepared
    .bundle
    .rules
    .iter()
    .any(|rule| rule.decision != InfrastructurePolicyDecisionV1::Deny || rule.requires.is_some())
  {
    return Err(invalid(
      "an emergency deny overlay may contain only deny rules without requirements",
    ));
  }
  Ok(prepared)
}

fn validate_bundle(bundle: &InfrastructurePolicyBundleV1) -> Result<(), InfrastructurePolicyError> {
  if bundle.contract != INFRASTRUCTURE_POLICY_CONTRACT
    || bundle.contract_version != INFRASTRUCTURE_POLICY_CONTRACT_VERSION
  {
    return Err(invalid(
      "the document does not match Infrastructure Policy v1",
    ));
  }
  validate_identifier(&bundle.id, "policy id")?;
  validate_pattern_string(&bundle.version, "policy version", false)?;
  validate_identifier(&bundle.environment, "policy environment")?;
  if bundle.rules.len() > INFRASTRUCTURE_POLICY_MAX_RULES {
    return Err(invalid("the policy contains too many rules"));
  }
  let mut rule_ids = HashSet::new();
  for rule in &bundle.rules {
    validate_identifier(&rule.id, "rule id")?;
    if !rule_ids.insert(rule.id.as_str()) {
      return Err(invalid("policy rule IDs must be unique"));
    }
    if rule
      .description
      .as_ref()
      .is_some_and(|value| value.is_empty() || value.len() > 512 || value.contains('\0'))
    {
      return Err(invalid(
        "rule descriptions must contain 1 through 512 bytes",
      ));
    }
    validate_matcher(&rule.matcher)?;
    match (rule.decision, &rule.requires) {
      (InfrastructurePolicyDecisionV1::Deny, Some(_)) => {
        return Err(invalid("deny rules cannot carry requirements"));
      }
      (_, Some(requirements)) => validate_requirements(requirements)?,
      _ => {}
    }
  }
  if let Some(signature) = &bundle.signature {
    if signature.algorithm != "ed25519" {
      return Err(signature_invalid(
        "Infrastructure Policy v1 supports only Ed25519",
      ));
    }
    validate_identifier(&signature.key_id, "signature key ID")?;
    if signature.value.is_empty() || signature.value.len() > 128 {
      return Err(signature_invalid("the signature encoding is invalid"));
    }
  }
  Ok(())
}

fn validate_matcher(
  matcher: &InfrastructurePolicyRuleMatchV1,
) -> Result<(), InfrastructurePolicyError> {
  if matcher.capabilities.is_none()
    && matcher.operations.is_none()
    && matcher.effects.is_none()
    && matcher.workflows.is_none()
    && matcher.targets.is_none()
    && matcher.executables.is_none()
    && matcher.argument_prefix.is_none()
    && matcher.execution_modes.is_none()
  {
    return Err(invalid("a rule match must contain at least one matcher"));
  }
  for (values, label) in [
    (&matcher.capabilities, "capability matcher"),
    (&matcher.operations, "operation matcher"),
    (&matcher.workflows, "workflow matcher"),
    (&matcher.targets, "target matcher"),
    (&matcher.executables, "executable matcher"),
  ] {
    if let Some(values) = values {
      validate_identifier_set(values, label)?;
    }
  }
  if let Some(effects) = &matcher.effects {
    validate_enum_set(effects, "effect matcher")?;
  }
  if let Some(execution_modes) = &matcher.execution_modes {
    validate_enum_set(execution_modes, "execution-mode matcher")?;
  }
  if let Some(arguments) = &matcher.argument_prefix {
    if arguments.is_empty()
      || arguments.len() > INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES
      || arguments
        .iter()
        .any(|value| value.is_empty() || value.len() > 4096 || value.contains('\0'))
    {
      return Err(invalid("argument prefixes exceed their frozen limits"));
    }
  }
  Ok(())
}

fn validate_requirements(
  requirements: &InfrastructurePolicyRequirementsV1,
) -> Result<(), InfrastructurePolicyError> {
  if requirements.approval.is_none()
    && requirements.artifacts.is_none()
    && requirements.lock.is_none()
    && requirements.valid_between.is_none()
  {
    return Err(invalid("policy requirements cannot be empty"));
  }
  if let Some(approval) = &requirements.approval {
    if !(1..=20).contains(&approval.minimum)
      || approval.max_age_ms == 0
      || approval.max_age_ms > 365 * 24 * 60 * 60 * 1000
    {
      return Err(invalid("approval requirements exceed their frozen limits"));
    }
    validate_role_set(&approval.roles)?;
  }
  if let Some(artifacts) = &requirements.artifacts {
    if !(1..=32).contains(&artifacts.minimum) || !artifacts.verified_integrity {
      return Err(invalid("artifact requirements exceed their frozen limits"));
    }
  }
  if let Some(lock) = &requirements.lock {
    if lock.key.is_empty()
      || lock.key.len() > 256
      || lock.key.contains('\0')
      || lock.minimum_remaining_ms == 0
      || lock.minimum_remaining_ms > 86_400_000
    {
      return Err(invalid("lock requirements exceed their frozen limits"));
    }
  }
  if let Some(validity) = &requirements.valid_between {
    let not_before = parse_timestamp(&validity.not_before)?;
    let not_after = parse_timestamp(&validity.not_after)?;
    if not_before >= not_after {
      return Err(invalid("validBetween must end after it starts"));
    }
  }
  Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), InfrastructurePolicyError> {
  validate_pattern_string(value, label, true)
}

fn validate_pattern_string(
  value: &str,
  label: &str,
  lowercase_first: bool,
) -> Result<(), InfrastructurePolicyError> {
  if value.is_empty() || value.len() > 128 || value.contains('\0') {
    return Err(invalid(&format!("{label} has an invalid length")));
  }
  let mut bytes = value.bytes();
  let first = bytes.next().ok_or_else(|| invalid(label))?;
  let first_valid = if lowercase_first {
    first.is_ascii_lowercase()
  } else {
    first.is_ascii_alphanumeric()
  };
  if !first_valid
    || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
  {
    return Err(invalid(&format!("{label} has an invalid format")));
  }
  Ok(())
}

fn validate_identifier_set(
  values: &[String],
  label: &str,
) -> Result<(), InfrastructurePolicyError> {
  if values.is_empty() || values.len() > INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES {
    return Err(invalid(&format!("{label} contains too many values")));
  }
  let mut unique = HashSet::new();
  for value in values {
    validate_identifier(value, label)?;
    if !unique.insert(value.as_str()) {
      return Err(invalid(&format!("{label} contains duplicates")));
    }
  }
  Ok(())
}

fn validate_role_set(values: &[String]) -> Result<(), InfrastructurePolicyError> {
  if values.len() > INFRASTRUCTURE_POLICY_MAX_ROLES {
    return Err(invalid("approval roles exceed their frozen limit"));
  }
  let mut unique = HashSet::new();
  for value in values {
    if value.is_empty()
      || value.len() > 128
      || !value.bytes().enumerate().all(|(index, byte)| {
        if index == 0 {
          byte.is_ascii_lowercase()
        } else {
          byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-')
        }
      })
      || !unique.insert(value.as_str())
    {
      return Err(invalid("approval roles are invalid or duplicated"));
    }
  }
  Ok(())
}

fn validate_enum_set<T>(values: &[T], label: &str) -> Result<(), InfrastructurePolicyError>
where
  T: Copy + Eq + std::hash::Hash,
{
  if values.is_empty() || values.len() > INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES {
    return Err(invalid(&format!("{label} contains too many values")));
  }
  let mut unique = HashSet::new();
  if values.iter().any(|value| !unique.insert(*value)) {
    return Err(invalid(&format!("{label} contains duplicates")));
  }
  Ok(())
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, InfrastructurePolicyError> {
  if value.len() > 64 || !value.ends_with('Z') {
    return Err(invalid(
      "validBetween timestamps must be bounded UTC RFC 3339",
    ));
  }
  DateTime::parse_from_rfc3339(value)
    .map(|parsed| parsed.with_timezone(&Utc))
    .map_err(|_| invalid("validBetween timestamps must be valid RFC 3339"))
}

fn decode_base64url(
  value: &str,
  expected_length: usize,
  label: &str,
) -> Result<Vec<u8>, InfrastructurePolicyError> {
  if value.is_empty()
    || !value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
  {
    return Err(signature_invalid(&format!(
      "the {label} is not canonical base64url"
    )));
  }
  let decoded = URL_SAFE_NO_PAD
    .decode(value)
    .map_err(|_| signature_invalid(&format!("the {label} cannot be decoded")))?;
  if decoded.len() != expected_length || URL_SAFE_NO_PAD.encode(&decoded) != value {
    return Err(signature_invalid(&format!(
      "the {label} has an invalid length or encoding"
    )));
  }
  Ok(decoded)
}

fn json_complexity(value: &Value) -> (usize, usize) {
  fn visit(value: &Value, depth: usize, maximum: &mut usize, count: &mut usize) {
    *count = count.saturating_add(1);
    *maximum = (*maximum).max(depth);
    match value {
      Value::Array(values) => {
        for value in values {
          visit(value, depth + 1, maximum, count);
        }
      }
      Value::Object(values) => {
        for value in values.values() {
          visit(value, depth + 1, maximum, count);
        }
      }
      _ => {}
    }
  }
  let mut maximum = 0;
  let mut count = 0;
  visit(value, 0, &mut maximum, &mut count);
  (maximum, count)
}

fn invalid(message: &str) -> InfrastructurePolicyError {
  InfrastructurePolicyError::Invalid(message.to_string())
}

fn signature_invalid(message: &str) -> InfrastructurePolicyError {
  InfrastructurePolicyError::SignatureInvalid(message.to_string())
}
