//! Frozen Worker Protocol v1 messages and fail-closed decoding.
//!
//! RET3 deliberately contains no executable dispatch handler. A successfully
//! authenticated worker may establish, renew, poll, and close a session only.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const WORKER_PROTOCOL_PROFILE: &str = "woml.worker-protocol/v1";
pub const WORKER_PROTOCOL_VERSION: u32 = 1;
pub const WORKER_PROTOCOL_MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const WORKER_PROTOCOL_MAX_TARGETS: usize = 32;
pub const WORKER_PROTOCOL_MAX_CAPABILITIES: usize = 128;
pub const WORKER_PROTOCOL_MAX_CAPACITY: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerPlatformV1 {
  pub os: WorkerOperatingSystemV1,
  pub arch: WorkerArchitectureV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerOperatingSystemV1 {
  Linux,
  Darwin,
  Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerArchitectureV1 {
  X64,
  Arm64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerCapabilityV1 {
  pub id: String,
  pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerHelloV1 {
  pub profile: String,
  pub protocol_version: u32,
  pub message_id: String,
  pub message_type: String,
  pub worker_id: String,
  pub sent_at: DateTime<Utc>,
  pub worker_version: String,
  pub platform: WorkerPlatformV1,
  pub targets: Vec<String>,
  pub capacity: u32,
  pub capabilities: Vec<WorkerCapabilityV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerPollV1 {
  pub profile: String,
  pub protocol_version: u32,
  pub message_id: String,
  pub message_type: String,
  pub worker_id: String,
  pub session_id: String,
  pub sent_at: DateTime<Utc>,
  pub target: String,
  pub dispatch_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerShutdownV1 {
  pub profile: String,
  pub protocol_version: u32,
  pub message_id: String,
  pub message_type: String,
  pub worker_id: String,
  pub session_id: String,
  pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSessionV1 {
  pub profile: &'static str,
  pub protocol_version: u32,
  pub session_id: String,
  pub worker_id: String,
  pub targets: Vec<String>,
  pub expires_at: DateTime<Utc>,
  pub dispatch_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPollResultV1 {
  pub profile: &'static str,
  pub session_id: String,
  pub status: &'static str,
  pub dispatch: Option<Value>,
  pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RemoteProtocolError {
  #[error("the worker protocol message exceeds its size limit")]
  TooLarge,
  #[error("the worker protocol is incompatible")]
  Incompatible,
  #[error("the worker protocol message is invalid: {0}")]
  Invalid(String),
}

impl RemoteProtocolError {
  pub const fn code(&self) -> &'static str {
    match self {
      Self::TooLarge | Self::Invalid(_) => "WOML_REMOTE_PROTOCOL_INVALID",
      Self::Incompatible => "WOML_TARGET_INCOMPATIBLE",
    }
  }
}

pub fn decode_worker_hello(source: &[u8]) -> Result<WorkerHelloV1, RemoteProtocolError> {
  let message: WorkerHelloV1 = decode(source)?;
  validate_base(
    &message.profile,
    message.protocol_version,
    &message.message_id,
    &message.worker_id,
  )?;
  if message.message_type != "hello" || !valid_semver(&message.worker_version) {
    return Err(invalid("hello type or worker version is invalid"));
  }
  if message.targets.is_empty() || message.targets.len() > WORKER_PROTOCOL_MAX_TARGETS {
    return Err(invalid("hello targets exceed their bounds"));
  }
  unique_identifiers(&message.targets, false, "hello targets")?;
  if message.capacity == 0 || message.capacity > WORKER_PROTOCOL_MAX_CAPACITY {
    return Err(invalid("hello capacity is outside 1 through 64"));
  }
  if message.capabilities.is_empty()
    || message.capabilities.len() > WORKER_PROTOCOL_MAX_CAPABILITIES
  {
    return Err(invalid("hello capabilities exceed their bounds"));
  }
  let mut capabilities = BTreeSet::new();
  for capability in &message.capabilities {
    validate_identifier(&capability.id, true, "capability")?;
    if capability.version == 0 || capability.version > 65_535 {
      return Err(invalid("capability version is outside 1 through 65535"));
    }
    if !capabilities.insert(capability.id.as_str()) {
      return Err(invalid("hello contains a duplicate capability"));
    }
  }
  Ok(message)
}

pub fn decode_worker_poll(source: &[u8]) -> Result<WorkerPollV1, RemoteProtocolError> {
  let message: WorkerPollV1 = decode(source)?;
  validate_base(
    &message.profile,
    message.protocol_version,
    &message.message_id,
    &message.worker_id,
  )?;
  if message.message_type != "poll"
    || !valid_protocol_id(&message.session_id)
    || !valid_identifier(&message.target, false)
    || !valid_protocol_id(&message.dispatch_id)
  {
    return Err(invalid("poll identity is invalid"));
  }
  Ok(message)
}

pub fn decode_worker_shutdown(source: &[u8]) -> Result<WorkerShutdownV1, RemoteProtocolError> {
  let message: WorkerShutdownV1 = decode(source)?;
  validate_base(
    &message.profile,
    message.protocol_version,
    &message.message_id,
    &message.worker_id,
  )?;
  if message.message_type != "shutdown" || !valid_protocol_id(&message.session_id) {
    return Err(invalid("shutdown identity is invalid"));
  }
  Ok(message)
}

fn decode<T: for<'de> Deserialize<'de>>(source: &[u8]) -> Result<T, RemoteProtocolError> {
  if source.len() > WORKER_PROTOCOL_MAX_MESSAGE_BYTES {
    return Err(RemoteProtocolError::TooLarge);
  }
  serde_json::from_slice(source)
    .map_err(|_| invalid("JSON has malformed, duplicate, missing, or unknown fields"))
}

fn validate_base(
  profile: &str,
  version: u32,
  message_id: &str,
  worker_id: &str,
) -> Result<(), RemoteProtocolError> {
  if profile != WORKER_PROTOCOL_PROFILE || version != WORKER_PROTOCOL_VERSION {
    return Err(RemoteProtocolError::Incompatible);
  }
  if !valid_protocol_id(message_id) || !valid_identifier(worker_id, false) {
    return Err(invalid("message or worker identity is invalid"));
  }
  Ok(())
}

fn unique_identifiers(
  values: &[String],
  capability: bool,
  label: &str,
) -> Result<(), RemoteProtocolError> {
  let mut seen = BTreeSet::new();
  for value in values {
    validate_identifier(value, capability, label)?;
    if !seen.insert(value) {
      return Err(invalid(&format!("{label} contain a duplicate")));
    }
  }
  Ok(())
}

fn validate_identifier(
  value: &str,
  capability: bool,
  label: &str,
) -> Result<(), RemoteProtocolError> {
  if valid_identifier(value, capability) {
    Ok(())
  } else {
    Err(invalid(&format!("{label} contains an invalid identifier")))
  }
}

fn valid_identifier(value: &str, capability: bool) -> bool {
  let bytes = value.as_bytes();
  let separators: &[u8] = if capability { b".-" } else { b"-" };
  !bytes.is_empty()
    && bytes.len() <= 128
    && bytes[0].is_ascii_lowercase()
    && bytes
      .iter()
      .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || separators.contains(byte))
    && !separators.contains(bytes.last().expect("non-empty"))
    && !bytes
      .windows(2)
      .any(|pair| separators.contains(&pair[0]) && separators.contains(&pair[1]))
}

fn valid_protocol_id(value: &str) -> bool {
  !value.is_empty()
    && value.len() <= 256
    && value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-'))
}

pub(crate) fn valid_semver(value: &str) -> bool {
  if value.is_empty() || value.len() > 64 || !value.is_ascii() {
    return false;
  }
  let core = value.split_once('-').map_or(value, |(core, suffix)| {
    if suffix.is_empty()
      || !suffix
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
      return "";
    }
    core
  });
  let parts: Vec<_> = core.split('.').collect();
  parts.len() == 3
    && parts
      .iter()
      .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn invalid(message: &str) -> RemoteProtocolError {
  RemoteProtocolError::Invalid(message.to_string())
}
