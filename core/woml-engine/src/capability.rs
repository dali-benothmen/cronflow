//! Provider-neutral capability execution contracts.
//!
//! This module owns generic operation validation, dispatch, limits, cancellation,
//! and safe failures. Capability-specific policy belongs in registered handlers,
//! never in the workflow DAG traversal.

use std::{
  collections::HashMap,
  panic::AssertUnwindSafe,
  sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
  },
  time::Instant,
};

use chrono::{DateTime, Utc};
use futures_util::{future::BoxFuture, FutureExt};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
  sync::{Mutex as AsyncMutex, Notify},
  time,
};
use uuid::Uuid;

use crate::{
  infrastructure_policy_runtime::{
    adapt_infrastructure_policy_metadata, apply_emergency_deny_overlay,
    InfrastructurePolicyDecisionOutcomeV1, InfrastructurePolicyDescriptorContextV1,
    InfrastructurePolicyEvaluatorCacheV1, InfrastructurePolicyEvidenceAuthorityV1,
    InfrastructurePolicyEvidenceKindV1, InfrastructurePolicyEvidenceReferenceV1,
    InfrastructurePolicyOperationDescriptorV1, InfrastructurePolicyOperationMetadataV1,
  },
  DurableEventStore, DurableStoreError, InfrastructurePolicyDecisionRecordedData,
  InfrastructurePolicyExecutionModeV1, OperationExecutionMode, OperationFailedData,
  OperationStartedData, OperationSucceededData, PreparedInfrastructurePolicyV1, RunEventPayload,
};

pub const CAPABILITY_CALL_CONTRACT: &str = "woml.capability-call";
pub const CAPABILITY_CALL_CONTRACT_VERSION: u32 = 1;
pub const DEFAULT_CAPABILITY_INPUT_BYTES: u64 = 1_048_576;
pub const DEFAULT_CAPABILITY_RESULT_BYTES: u64 = 4_194_304;
pub const DEFAULT_CAPABILITY_FRAME_BYTES: u64 = 8_388_608;
pub const DEFAULT_CAPABILITY_TIMEOUT_MS: u64 = 30_000;
pub const MAX_CAPABILITY_TIMEOUT_MS: u64 = 86_400_000;
pub const DEFAULT_CAPABILITY_IN_FLIGHT_PER_INVOCATION: usize = 32;
pub const DEFAULT_CAPABILITY_IN_FLIGHT_GLOBAL: usize = 256;
const CAPABILITY_CANCELLATION_CLEANUP_MS: u64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityIdentityMode {
  Automatic,
  Named,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityCallIdentity {
  pub mode: CapabilityIdentityMode,
  pub step_idempotency_key: String,
  pub operation_name: String,
  pub operation_key: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityCallLimits {
  pub input_bytes: u64,
  pub result_bytes: u64,
  pub timeout_ms: u64,
}

impl Default for CapabilityCallLimits {
  fn default() -> Self {
    Self {
      input_bytes: DEFAULT_CAPABILITY_INPUT_BYTES,
      result_bytes: DEFAULT_CAPABILITY_RESULT_BYTES,
      timeout_ms: DEFAULT_CAPABILITY_TIMEOUT_MS,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityCallRequest {
  pub contract: String,
  pub contract_version: u32,
  pub message_type: String,
  pub invocation_id: String,
  pub call_id: String,
  pub run_id: String,
  pub node_id: String,
  pub attempt_number: u32,
  pub capability: String,
  pub operation: String,
  pub input_contract_version: u32,
  pub result_contract_version: u32,
  pub identity: CapabilityCallIdentity,
  pub limits: CapabilityCallLimits,
  pub input: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityFailureKind {
  InvalidInput,
  InvalidResult,
  UnsupportedCapability,
  UnsupportedOperation,
  UnsupportedVersion,
  InputTooLarge,
  ResultTooLarge,
  FrameTooLarge,
  TimedOut,
  Cancelled,
  HandlerCrashed,
  WorkerCrashed,
  HostCrashed,
  TransportFailed,
  ServiceRejected,
  Interrupted,
  Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityFailure {
  pub kind: CapabilityFailureKind,
  pub code: String,
  pub message: String,
  pub retryable: bool,
  pub ambiguous: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub details: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityCallSucceeded {
  pub contract: String,
  pub contract_version: u32,
  pub message_type: String,
  pub invocation_id: String,
  pub call_id: String,
  pub outcome: String,
  pub result_contract_version: u32,
  pub result_bytes: u64,
  pub duration_ms: f64,
  pub result: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityCallFailed {
  pub contract: String,
  pub contract_version: u32,
  pub message_type: String,
  pub invocation_id: String,
  pub call_id: String,
  pub outcome: String,
  pub duration_ms: f64,
  pub error: CapabilityFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CapabilityCallResult {
  Succeeded(CapabilityCallSucceeded),
  Failed(CapabilityCallFailed),
  Cancelled(CapabilityCallFailed),
}

impl<'de> Deserialize<'de> for CapabilityCallResult {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let value = Value::deserialize(deserializer)?;
    match value.get("outcome").and_then(Value::as_str) {
      Some("succeeded") => serde_json::from_value(value)
        .map(Self::Succeeded)
        .map_err(D::Error::custom),
      Some("failed") => serde_json::from_value(value)
        .map(Self::Failed)
        .map_err(D::Error::custom),
      Some("cancelled") => serde_json::from_value(value)
        .map(Self::Cancelled)
        .map_err(D::Error::custom),
      _ => Err(D::Error::custom(
        "Capability Call v1 result has an invalid outcome.",
      )),
    }
  }
}

impl CapabilityCallResult {
  pub fn invocation_id(&self) -> &str {
    match self {
      Self::Succeeded(result) => &result.invocation_id,
      Self::Failed(result) | Self::Cancelled(result) => &result.invocation_id,
    }
  }

  pub fn call_id(&self) -> &str {
    match self {
      Self::Succeeded(result) => &result.call_id,
      Self::Failed(result) | Self::Cancelled(result) => &result.call_id,
    }
  }

  pub fn validate(&self) -> Result<(), String> {
    match self {
      Self::Succeeded(result) => {
        validate_result_base(
          &result.contract,
          result.contract_version,
          &result.message_type,
          &result.invocation_id,
          &result.call_id,
          result.duration_ms,
        )?;
        if result.outcome != "succeeded"
          || result.result_contract_version == 0
          || result.result_bytes > DEFAULT_CAPABILITY_FRAME_BYTES
        {
          return Err("Capability success does not match Capability Call v1.".to_string());
        }
      }
      Self::Failed(result) | Self::Cancelled(result) => {
        validate_result_base(
          &result.contract,
          result.contract_version,
          &result.message_type,
          &result.invocation_id,
          &result.call_id,
          result.duration_ms,
        )?;
        let expected = if matches!(self, Self::Cancelled(_)) {
          "cancelled"
        } else {
          "failed"
        };
        if result.outcome != expected {
          return Err("Capability failure outcome does not match its result variant.".to_string());
        }
        result.error.validate()?;
      }
    }
    Ok(())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityEffect {
  Read,
  IdempotentWrite,
  UnsafeWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDescriptor {
  pub capability: String,
  pub operation: String,
  pub input_contract_version: u32,
  pub result_contract_version: u32,
  pub effect: CapabilityEffect,
  pub supports_cancellation: bool,
  pub supports_provider_idempotency: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityCancellationToken {
  state: Arc<CancellationState>,
}

#[derive(Debug, Default)]
struct CancellationState {
  cancelled: AtomicBool,
  notify: Notify,
}

impl CapabilityCancellationToken {
  pub fn cancel(&self) {
    if !self.state.cancelled.swap(true, Ordering::SeqCst) {
      self.state.notify.notify_waiters();
    }
  }

  pub fn is_cancelled(&self) -> bool {
    self.state.cancelled.load(Ordering::SeqCst)
  }

  pub async fn cancelled(&self) {
    while !self.is_cancelled() {
      self.state.notify.notified().await;
    }
  }
}

pub trait CapabilityHandler: Send + Sync + 'static {
  fn descriptor(&self) -> CapabilityDescriptor;

  fn validate_request(&self, _request: &CapabilityCallRequest) -> Result<(), CapabilityFailure> {
    Ok(())
  }

  fn effect(&self, _input: &Value) -> CapabilityEffect {
    self.descriptor().effect
  }

  fn safe_metadata(&self, _input: &Value) -> Map<String, Value> {
    Map::new()
  }

  /// Produces safe start metadata with the verified engine call identity.
  /// Existing handlers remain input-only; identity-sensitive handlers such as
  /// Workflow Calls can bind a deterministic child before dispatch.
  fn safe_request_metadata(
    &self,
    request: &CapabilityCallRequest,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    Ok(self.safe_metadata(&request.input))
  }

  /// Produces bounded policy-only facts after the request has passed the
  /// registered handler's normal validation. Policy facts are deliberately
  /// separate from presentation and durable operation metadata.
  fn infrastructure_policy_metadata(
    &self,
    request: &CapabilityCallRequest,
  ) -> Result<InfrastructurePolicyOperationMetadataV1, CapabilityFailure> {
    adapt_infrastructure_policy_metadata(&self.descriptor(), request)
  }

  fn safe_result_metadata(&self, _result: &Value) -> Map<String, Value> {
    Map::new()
  }

  fn safe_failure_metadata(&self, _failure: &CapabilityFailure) -> Map<String, Value> {
    Map::new()
  }

  fn execute(
    &self,
    input: Value,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>>;

  /// Executes with engine-owned identity that is deliberately absent from the
  /// frozen Capability Call v1 wire protocol. Most handlers do not need it.
  fn execute_scoped(
    &self,
    input: Value,
    workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    let _ = workflow_scope;
    self.execute(input, cancellation)
  }

  /// Executes with the complete engine-verified call identity. This remains an
  /// internal Rust boundary: it does not add fields to Capability Call v1.
  /// Handlers whose correctness depends on the originating run or stable
  /// operation identity may override it; all other handlers keep the scoped
  /// behavior above.
  fn execute_request_scoped(
    &self,
    request: &CapabilityCallRequest,
    workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    self.execute_scoped(request.input.clone(), workflow_scope, cancellation)
  }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityRegistryError {
  #[error("capability handler {capability}.{operation} is already registered")]
  DuplicateHandler {
    capability: String,
    operation: String,
  },
}

pub struct CapabilityRegistry {
  handlers: Mutex<HashMap<(String, String), Arc<dyn CapabilityHandler>>>,
  in_flight_by_invocation: Mutex<HashMap<String, usize>>,
  in_flight_global: AtomicUsize,
  max_per_invocation: usize,
  max_global: usize,
}

#[derive(Debug, Error)]
pub enum DurableCapabilityAuthorityError {
  #[error(transparent)]
  Store(#[from] DurableStoreError),
}

pub const NATIVE_FETCH_OBSERVATION_CONTRACT: &str = "woml.native-fetch-observation";
pub const NATIVE_FETCH_OBSERVATION_CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFetchInvocationIdentity {
  pub run_id: String,
  pub node_id: String,
  pub attempt_number: u32,
  pub invocation_id: String,
  pub step_idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFetchFailureKind {
  TrackingFailed,
  FetchRejected,
  TimedOut,
  Cancelled,
  WorkerCrashed,
  HostCrashed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeFetchFailure {
  pub kind: NativeFetchFailureKind,
  pub code: String,
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
  tag = "observationType",
  rename_all = "snake_case",
  rename_all_fields = "camelCase"
)]
pub enum NativeFetchObservation {
  Started {
    contract: String,
    contract_version: u32,
    invocation_id: String,
    request_id: String,
    method: String,
    origin: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_body_bytes: Option<u64>,
    started_at: DateTime<Utc>,
  },
  Completed {
    contract: String,
    contract_version: u32,
    invocation_id: String,
    request_id: String,
    status: u16,
    response_body_bytes: Value,
    duration_ms: f64,
    completed_at: DateTime<Utc>,
  },
  Failed {
    contract: String,
    contract_version: u32,
    invocation_id: String,
    request_id: String,
    duration_ms: f64,
    failed_at: DateTime<Utc>,
    error: NativeFetchFailure,
  },
}

impl NativeFetchObservation {
  pub fn invocation_id(&self) -> &str {
    match self {
      Self::Started { invocation_id, .. }
      | Self::Completed { invocation_id, .. }
      | Self::Failed { invocation_id, .. } => invocation_id,
    }
  }

  pub fn request_id(&self) -> &str {
    match self {
      Self::Started { request_id, .. }
      | Self::Completed { request_id, .. }
      | Self::Failed { request_id, .. } => request_id,
    }
  }

  pub fn validate(&self) -> Result<(), String> {
    let (contract, contract_version, invocation_id, request_id) = match self {
      Self::Started {
        contract,
        contract_version,
        invocation_id,
        request_id,
        method,
        origin,
        path,
        ..
      } => {
        let origin_tail = origin
          .strip_prefix("https://")
          .or_else(|| origin.strip_prefix("http://"));
        if method.is_empty()
          || method.len() > 32
          || !method.bytes().all(|byte| byte.is_ascii_uppercase())
          || origin.len() > 2048
          || origin_tail.is_none_or(|tail| {
            tail.is_empty()
              || tail
                .bytes()
                .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@'))
          })
          || path.len() > 2048
          || !path.starts_with('/')
          || path.bytes().any(|byte| matches!(byte, b'?' | b'#'))
        {
          return Err("Native Fetch start metadata is not safely redacted.".to_string());
        }
        (contract, contract_version, invocation_id, request_id)
      }
      Self::Completed {
        contract,
        contract_version,
        invocation_id,
        request_id,
        status,
        response_body_bytes,
        duration_ms,
        ..
      } => {
        if !(100..=599).contains(status)
          || !(response_body_bytes.is_null() || response_body_bytes.as_u64().is_some())
          || !duration_ms.is_finite()
          || *duration_ms < 0.0
        {
          return Err("Native Fetch completion metadata is invalid.".to_string());
        }
        (contract, contract_version, invocation_id, request_id)
      }
      Self::Failed {
        contract,
        contract_version,
        invocation_id,
        request_id,
        duration_ms,
        error,
        ..
      } => {
        if !duration_ms.is_finite()
          || *duration_ms < 0.0
          || error.code.is_empty()
          || !error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
          || error.message.is_empty()
          || error.message.len() > 2048
        {
          return Err("Native Fetch failure metadata is invalid.".to_string());
        }
        (contract, contract_version, invocation_id, request_id)
      }
    };
    if contract != NATIVE_FETCH_OBSERVATION_CONTRACT
      || *contract_version != NATIVE_FETCH_OBSERVATION_CONTRACT_VERSION
      || invocation_id.is_empty()
      || invocation_id.len() > 256
      || request_id.is_empty()
      || request_id.len() > 256
    {
      return Err("Message does not match Native Fetch Observation v1.".to_string());
    }
    Ok(())
  }
}

/// Rust's durable authority for managed capability operations. It records a
/// start before dispatch and a terminal event before exposing the result.
fn iteration_attempt_identity(
  projection: &crate::RunProjection,
  run_id: &str,
  definition_hash: &str,
  node_id: &str,
  attempt_number: u32,
  invocation_id: &str,
) -> Option<(crate::event::ForEachIterationScope, String)> {
  projection.for_each.values().find_map(|loop_| {
    loop_.executions.iter().find_map(|(index, execution)| {
      let attempt = execution.attempts.iter().find_map(|attempt| {
        (attempt.identity.node_id == node_id
          && attempt.identity.attempt == attempt_number
          && attempt.identity.invocation_id == invocation_id
          && attempt.status == crate::projection::AttemptStatus::Started)
          .then(|| {
            attempt.idempotency_key.clone().map(|key| {
              (
                crate::event::ForEachIterationScope {
                  for_each_id: loop_.for_each_id.clone(),
                  index: *index,
                },
                key,
              )
            })
          })
          .flatten()
      });
      attempt.or_else(|| {
        let lifecycle = execution.lifecycle_hooks.values().find_map(|hook| {
          hook.actions.get(node_id).and_then(|action| {
            (action.attempt == attempt_number
              && action.status == crate::projection::LifecycleActionStatus::Started)
              .then(|| {
                (
                  crate::event::ForEachIterationScope {
                    for_each_id: loop_.for_each_id.clone(),
                    index: *index,
                  },
                  crate::runtime::for_each_effect_key(
                    run_id,
                    definition_hash,
                    &loop_.for_each_id,
                    *index,
                    node_id,
                  ),
                )
              })
          })
        });
        lifecycle.or_else(|| {
          execution
            .reusable_lifecycle_hooks
            .values()
            .any(|hook| hook.active_action_id.as_deref() == Some(node_id))
            .then(|| {
              (
                crate::event::ForEachIterationScope {
                  for_each_id: loop_.for_each_id.clone(),
                  index: *index,
                },
                crate::runtime::for_each_effect_key(
                  run_id,
                  definition_hash,
                  &loop_.for_each_id,
                  *index,
                  node_id,
                ),
              )
            })
        })
      })
    })
  })
}

fn operational_invocation_is_active(
  projection: &crate::RunProjection,
  identity: &NativeFetchInvocationIdentity,
) -> bool {
  let root_attempt = projection.attempts.iter().any(|attempt| {
    attempt.identity.node_id == identity.node_id
      && attempt.identity.attempt == identity.attempt_number
      && attempt.identity.invocation_id == identity.invocation_id
      && attempt.status == crate::projection::AttemptStatus::Started
  });
  let root_lifecycle = projection.lifecycle_hooks.values().any(|hook| {
    hook.actions.get(&identity.node_id).is_some_and(|action| {
      action.attempt == identity.attempt_number
        && action.status == crate::projection::LifecycleActionStatus::Started
    })
  });
  let root_reusable = projection
    .reusable_lifecycle_hooks
    .values()
    .any(|hook| hook.active_action_id.as_deref() == Some(identity.node_id.as_str()));
  let iteration = projection.for_each.values().any(|loop_| {
    loop_.executions.values().any(|execution| {
      execution.attempts.iter().any(|attempt| {
        attempt.identity.node_id == identity.node_id
          && attempt.identity.attempt == identity.attempt_number
          && attempt.identity.invocation_id == identity.invocation_id
          && attempt.status == crate::projection::AttemptStatus::Started
      }) || execution.lifecycle_hooks.values().any(|hook| {
        hook.actions.get(&identity.node_id).is_some_and(|action| {
          action.attempt == identity.attempt_number
            && action.status == crate::projection::LifecycleActionStatus::Started
        })
      }) || execution
        .reusable_lifecycle_hooks
        .values()
        .any(|hook| hook.active_action_id.as_deref() == Some(identity.node_id.as_str()))
    })
  });
  let correction = projection.sagas.values().any(|saga| {
    saga.actions.get(&identity.node_id).is_some_and(|action| {
      action.active_attempt == Some(identity.attempt_number)
        && action.status == crate::projection::CorrectionActionStatus::Started
        && crate::durable::correction_invocation_id(
          &identity.run_id,
          &saga.saga_id,
          &identity.node_id,
          identity.attempt_number,
        ) == identity.invocation_id
    })
  });
  root_attempt || root_lifecycle || root_reusable || iteration || correction
}

fn operation_with_iteration_scope(
  projection: &crate::RunProjection,
  identity: &crate::projection::OperationIdentity,
) -> Option<(
  Option<crate::event::ForEachIterationScope>,
  crate::projection::OperationProjection,
)> {
  if let Some(operation) = projection.operations.get(identity) {
    return Some((None, operation.clone()));
  }
  projection.for_each.values().find_map(|loop_| {
    loop_.executions.iter().find_map(|(index, execution)| {
      execution.operations.get(identity).map(|operation| {
        (
          Some(crate::event::ForEachIterationScope {
            for_each_id: loop_.for_each_id.clone(),
            index: *index,
          }),
          operation.clone(),
        )
      })
    })
  })
}

fn infrastructure_policy_execution_mode(
  projection: &crate::RunProjection,
  node_id: &str,
  attempt: u32,
) -> InfrastructurePolicyExecutionModeV1 {
  for saga in projection.sagas.values() {
    if let Some(action) = saga.actions.get(node_id) {
      if action.active_attempt == Some(attempt) {
        if saga.phase == crate::projection::SagaPhase::ManualRecovery {
          return InfrastructurePolicyExecutionModeV1::Recovery;
        }
        return match action.action_kind {
          crate::CorrectiveActionKind::Compensation => {
            InfrastructurePolicyExecutionModeV1::Compensation
          }
          crate::CorrectiveActionKind::Cleanup => InfrastructurePolicyExecutionModeV1::Cleanup,
        };
      }
    }
  }
  InfrastructurePolicyExecutionModeV1::Normal
}

fn infrastructure_policy_evidence(
  store: &DurableEventStore,
  artifact_store: Option<&Arc<crate::ManagedArtifactStore>>,
  run_id: &str,
  now: chrono::DateTime<Utc>,
) -> Result<(Option<String>, Vec<InfrastructurePolicyEvidenceReferenceV1>), DurableStoreError> {
  let snapshot = store.infrastructure_policy_evidence_snapshot(run_id, now)?;
  let authority = InfrastructurePolicyEvidenceAuthorityV1::engine();
  let mut review_digests = std::collections::BTreeSet::new();
  let mut evidence = Vec::new();
  for approval in snapshot.approvals {
    review_digests.insert(approval.review_digest.clone());
    let identity = format!(
      "{}\0{}\0{}",
      approval.request_id, approval.review_digest, approval.principal_id
    );
    if let Ok(reference) = authority.issue(
      InfrastructurePolicyEvidenceKindV1::Approval,
      format!("approval_{:x}", Sha256::digest(identity.as_bytes())),
      "woml.rich-approval-authority/v1".to_string(),
      approval.decided_at,
      None,
      run_id.to_string(),
      None,
      None,
      Some(approval.review_digest),
      Some(approval.principal_id),
      approval.roles,
      true,
      None,
    ) {
      evidence.push(reference);
    }
  }
  if let Some(artifact_store) = artifact_store {
    for artifact in snapshot.artifacts {
      if artifact_store
        .verify_policy_evidence(run_id, &artifact.artifact_id, &artifact.sha256)
        .is_err()
      {
        continue;
      }
      review_digests.insert(artifact.review_digest.clone());
      if let Ok(reference) = authority.issue(
        InfrastructurePolicyEvidenceKindV1::Artifact,
        artifact.artifact_id,
        "woml.artifact-store/v1".to_string(),
        now,
        None,
        run_id.to_string(),
        None,
        None,
        Some(artifact.review_digest),
        None,
        std::collections::BTreeSet::new(),
        true,
        None,
      ) {
        evidence.push(reference);
      }
    }
  }
  for lock in snapshot.locks {
    let identity = format!("{}\0{}\0{}", lock.key, lock.lease_id, lock.fencing_token);
    if let Ok(reference) = authority.issue(
      InfrastructurePolicyEvidenceKindV1::Lock,
      format!("lock_{:x}", Sha256::digest(identity.as_bytes())),
      "woml.lock-authority/v1".to_string(),
      lock.acquired_at,
      Some(lock.expires_at),
      run_id.to_string(),
      None,
      None,
      None,
      None,
      std::collections::BTreeSet::new(),
      true,
      Some(lock.key),
    ) {
      evidence.push(reference);
    }
  }
  let review_digest = if review_digests.len() == 1 {
    review_digests.into_iter().next()
  } else {
    None
  };
  Ok((review_digest, evidence))
}

pub struct DurableCapabilityAuthority {
  registry: Arc<CapabilityRegistry>,
  store: Arc<AsyncMutex<DurableEventStore>>,
  locks: Arc<crate::ManagedLocksService>,
  infrastructure_policy_required: bool,
  infrastructure_policy_evaluators: Arc<InfrastructurePolicyEvaluatorCacheV1>,
  artifact_store: Option<Arc<crate::ManagedArtifactStore>>,
  emergency_deny_overlay: Option<Arc<PreparedInfrastructurePolicyV1>>,
  operational_output_store: Option<Arc<crate::LazyOperationalOutputStore>>,
  operational_redactions: Arc<std::collections::BTreeMap<String, String>>,
}

impl DurableCapabilityAuthority {
  pub fn new(registry: Arc<CapabilityRegistry>, store: Arc<AsyncMutex<DurableEventStore>>) -> Self {
    Self {
      registry,
      store,
      locks: Arc::new(crate::ManagedLocksService::default()),
      infrastructure_policy_required: false,
      infrastructure_policy_evaluators: Arc::new(InfrastructurePolicyEvaluatorCacheV1::default()),
      artifact_store: None,
      emergency_deny_overlay: None,
      operational_output_store: None,
      operational_redactions: Arc::new(std::collections::BTreeMap::new()),
    }
  }

  pub fn new_with_locks(
    registry: Arc<CapabilityRegistry>,
    store: Arc<AsyncMutex<DurableEventStore>>,
    locks: Arc<crate::ManagedLocksService>,
  ) -> Self {
    Self {
      registry,
      store,
      locks,
      infrastructure_policy_required: false,
      infrastructure_policy_evaluators: Arc::new(InfrastructurePolicyEvaluatorCacheV1::default()),
      artifact_store: None,
      emergency_deny_overlay: None,
      operational_output_store: None,
      operational_redactions: Arc::new(std::collections::BTreeMap::new()),
    }
  }

  pub fn new_with_locks_and_infrastructure_policy(
    registry: Arc<CapabilityRegistry>,
    store: Arc<AsyncMutex<DurableEventStore>>,
    locks: Arc<crate::ManagedLocksService>,
    infrastructure_policy_required: bool,
  ) -> Self {
    Self::new_with_infrastructure_policy_authorities(
      registry,
      store,
      locks,
      infrastructure_policy_required,
      None,
      None,
    )
  }

  pub fn new_with_infrastructure_policy_authorities(
    registry: Arc<CapabilityRegistry>,
    store: Arc<AsyncMutex<DurableEventStore>>,
    locks: Arc<crate::ManagedLocksService>,
    infrastructure_policy_required: bool,
    artifact_store: Option<Arc<crate::ManagedArtifactStore>>,
    emergency_deny_overlay: Option<Arc<PreparedInfrastructurePolicyV1>>,
  ) -> Self {
    Self {
      registry,
      store,
      locks,
      infrastructure_policy_required,
      infrastructure_policy_evaluators: Arc::new(InfrastructurePolicyEvaluatorCacheV1::default()),
      artifact_store,
      emergency_deny_overlay,
      operational_output_store: None,
      operational_redactions: Arc::new(std::collections::BTreeMap::new()),
    }
  }

  pub fn with_operational_output(
    mut self,
    store: Arc<crate::LazyOperationalOutputStore>,
    redactions: Arc<std::collections::BTreeMap<String, String>>,
  ) -> Self {
    self.operational_output_store = Some(store);
    self.operational_redactions = redactions;
    self
  }

  pub async fn append_script_operational_record(
    &self,
    identity: &NativeFetchInvocationIdentity,
    body: crate::OperationalBodyV1,
  ) -> Result<(), CapabilityFailure> {
    self
      .append_script_operational_record_inner(identity, body, false)
      .await
  }

  pub async fn force_append_script_operational_record(
    &self,
    identity: &NativeFetchInvocationIdentity,
    body: crate::OperationalBodyV1,
  ) -> Result<(), CapabilityFailure> {
    self
      .append_script_operational_record_inner(identity, body, true)
      .await
  }

  async fn append_script_operational_record_inner(
    &self,
    identity: &NativeFetchInvocationIdentity,
    mut body: crate::OperationalBodyV1,
    force_append: bool,
  ) -> Result<(), CapabilityFailure> {
    let workflow_id = {
      let store = self.store.lock().await;
      let projection = store
        .projection(&identity.run_id)
        .map_err(|_| operational_output_failure("WOML_OPERATIONAL_IDENTITY_INVALID", false))?;
      if !operational_invocation_is_active(&projection, identity) {
        return Err(operational_output_failure(
          "WOML_OPERATIONAL_INVOCATION_CLOSED",
          false,
        ));
      }
      store
        .run_binding(&identity.run_id)
        .map_err(|_| operational_output_failure("WOML_OPERATIONAL_IDENTITY_INVALID", false))?
        .workflow_id
    };
    let mut redactions = self
      .operational_redactions
      .values()
      .cloned()
      .collect::<Vec<_>>();
    if let Some(artifacts) = self.artifact_store.as_ref() {
      redactions.extend(artifacts.operational_redactions(&identity.invocation_id));
    }
    redact_operational_body(&mut body, redactions.iter());
    let store = self
      .operational_output_store
      .as_ref()
      .ok_or_else(|| operational_output_failure("WOML_OUTPUT_STORE_UNAVAILABLE", false))?
      .activate()
      .map_err(|_| operational_output_failure("WOML_OUTPUT_STORE_UNAVAILABLE", false))?;
    let draft = crate::OperationalRecordDraftV1 {
      identity: crate::OperationalIdentityV1 {
        workflow_id,
        run_id: identity.run_id.clone(),
        node_id: Some(identity.node_id.clone()),
        attempt: Some(identity.attempt_number),
        invocation_id: Some(identity.invocation_id.clone()),
      },
      body,
    };
    if !force_append
      && matches!(&draft.body, crate::OperationalBodyV1::Progress { .. })
      && store
        .coalesce_progress_snapshot(&draft)
        .map_err(|_| operational_output_failure("WOML_OUTPUT_STORE_UNAVAILABLE", true))?
    {
      return Ok(());
    }
    store
      .append(draft)
      .map(|_| ())
      .map_err(|_| operational_output_failure("WOML_OUTPUT_STORE_UNAVAILABLE", true))
  }

  pub fn registry(&self) -> &Arc<CapabilityRegistry> {
    &self.registry
  }

  pub fn store(&self) -> &Arc<AsyncMutex<DurableEventStore>> {
    &self.store
  }

  /// Called only after the host has reported the invocation terminal or the
  /// host connection itself has ended. At that point Bun can no longer issue
  /// protected work, so Rust may safely stop renewal and release the session.
  pub fn close_programmatic_lock_sessions(&self, invocation_id: &str) {
    self.locks.close_invocation(invocation_id);
  }

  pub async fn execute(
    &self,
    request: CapabilityCallRequest,
    cancellation: CapabilityCancellationToken,
  ) -> Result<CapabilityCallResult, DurableCapabilityAuthorityError> {
    let (workflow_scope, definition_hash, iteration_scope, policy_execution_mode) = {
      let mut store = self.store.lock().await;
      let projection = store.projection(&request.run_id)?;
      let binding = store.run_binding(&request.run_id)?;
      let attempt = projection.attempts.iter().find(|attempt| {
        attempt.identity.node_id == request.node_id
          && attempt.identity.attempt == request.attempt_number
          && attempt.identity.invocation_id == request.invocation_id
          && attempt.status == crate::projection::AttemptStatus::Started
      });
      let lifecycle_key = projection.lifecycle_hooks.values().find_map(|hook| {
        hook.actions.get(&request.node_id).and_then(|action| {
          (action.attempt == request.attempt_number
            && action.status == crate::projection::LifecycleActionStatus::Started)
            .then(|| {
              crate::step_effect_idempotency_key(
                &request.run_id,
                &binding.definition_hash,
                &request.node_id,
              )
            })
        })
      });
      let root_key = attempt
        .and_then(|attempt| attempt.idempotency_key.as_deref())
        .map(str::to_string)
        .or(lifecycle_key);
      let iteration = iteration_attempt_identity(
        &projection,
        &request.run_id,
        &binding.definition_hash,
        &request.node_id,
        request.attempt_number,
        &request.invocation_id,
      );
      let (iteration_scope, iteration_key) = iteration
        .map(|(scope, key)| (Some(scope), Some(key)))
        .unwrap_or((None, None));
      if root_key.or(iteration_key).as_deref()
        != Some(request.identity.step_idempotency_key.as_str())
      {
        return Err(
          DurableStoreError::Contract(
            "Capability call step identity does not match its active durable attempt.".to_string(),
          )
          .into(),
        );
      }
      if !store.managed_effect_has_current_lock(
        &request.run_id,
        &request.node_id,
        &request.invocation_id,
        Utc::now(),
      )? {
        return Ok(failed_result(
          &request,
          Instant::now(),
          failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_LOCK_STALE_OWNER",
            "The managed effect was rejected because its lock lease is no longer current.",
            false,
            false,
          ),
        ));
      }
      let policy_execution_mode =
        infrastructure_policy_execution_mode(&projection, &request.node_id, request.attempt_number);
      (
        binding.workflow_id,
        binding.definition_hash,
        iteration_scope,
        policy_execution_mode,
      )
    };
    match self.locks.managed_effect_is_current(&request.invocation_id) {
      Ok(true) => {}
      Ok(false) => {
        return Ok(failed_result(
          &request,
          Instant::now(),
          failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_LOCK_STALE_OWNER",
            "The managed effect was rejected because its programmatic lease is no longer current.",
            false,
            false,
          ),
        ));
      }
      Err(error) => {
        return Ok(failed_result(&request, Instant::now(), error));
      }
    }
    let metadata = match self.registry.safe_metadata(&request) {
      Ok(metadata) => metadata,
      Err(error) => {
        return Ok(CapabilityCallResult::Failed(failed_fields(
          &request,
          Instant::now(),
          "failed",
          error,
        )))
      }
    };
    let policy_decision = {
      let mut store = self.store.lock().await;
      let admitted_policy = match store.infrastructure_policy_for_run(&request.run_id) {
        Ok(policy) => policy,
        Err(error) => {
          return Ok(failed_result(
            &request,
            Instant::now(),
            failure(
              CapabilityFailureKind::ServiceRejected,
              "WOML_POLICY_UNAVAILABLE",
              format!("The admitted infrastructure policy is unavailable: {error}"),
              false,
              false,
            ),
          ));
        }
      };
      if admitted_policy.is_none() && self.infrastructure_policy_required {
        return Ok(failed_result(
          &request,
          Instant::now(),
          failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_POLICY_REQUIRED",
            "This runtime requires an admitted infrastructure policy before protected effects may start.",
            false,
            false,
          ),
        ));
      }
      if let Some(policy) = admitted_policy {
        let evaluated_at = Utc::now();
        let (review_digest, evidence) = match infrastructure_policy_evidence(
          &store,
          self.artifact_store.as_ref(),
          &request.run_id,
          evaluated_at,
        ) {
          Ok(evidence) => evidence,
          Err(error) => {
            return Ok(failed_result(
              &request,
              Instant::now(),
              failure(
                CapabilityFailureKind::ServiceRejected,
                "WOML_POLICY_DECISION_UNAVAILABLE",
                format!("Trusted infrastructure-policy evidence is unavailable: {error}"),
                false,
                false,
              ),
            ));
          }
        };
        let descriptor = match self.registry.infrastructure_policy_descriptor(
          &request,
          InfrastructurePolicyDescriptorContextV1 {
            workflow_id: workflow_scope.clone(),
            definition_hash: definition_hash.clone(),
            environment: policy.bundle.environment.clone(),
            effective_target: None,
            execution_mode: policy_execution_mode,
            review_digest,
          },
        ) {
          Ok(descriptor) => descriptor,
          Err(error) => {
            let effect = self
              .registry
              .descriptor(&request.capability, &request.operation)
              .map_or(CapabilityEffect::UnsafeWrite, |descriptor| {
                descriptor.effect
              });
            let evaluator = self.infrastructure_policy_evaluators.prepare(policy);
            let decision = evaluator.input_unsupported_decision(
              &request.run_id,
              &workflow_scope,
              &definition_hash,
              &request.node_id,
              request.attempt_number,
              &request.invocation_id,
              &request.identity.operation_key,
              &request.capability,
              &request.operation,
              effect,
              policy_execution_mode,
              evaluated_at,
            );
            store.append_payload(
              request.run_id.clone(),
              generated_event_id(),
              evaluated_at,
              RunEventPayload::InfrastructurePolicyDecisionRecorded(
                InfrastructurePolicyDecisionRecordedData {
                  invocation_id: request.invocation_id.clone(),
                  call_id: request.call_id.clone(),
                  operation_key: request.identity.operation_key.clone(),
                  decision,
                },
              ),
            )?;
            return Ok(failed_result(&request, Instant::now(), error));
          }
        };
        let evaluator = self.infrastructure_policy_evaluators.prepare(policy);
        let base = evaluator.evaluate(&descriptor, &evidence, evaluated_at);
        Some(
          if let Some(overlay) = self.emergency_deny_overlay.as_ref() {
            let overlay = self
              .infrastructure_policy_evaluators
              .prepare(overlay.as_ref().clone());
            let overlay_decision = overlay.evaluate(&descriptor, &[], evaluated_at);
            apply_emergency_deny_overlay(base, overlay_decision)
          } else {
            base
          },
        )
      } else {
        None
      }
    };
    if cancellation.is_cancelled() {
      return Ok(CapabilityCallResult::Cancelled(failed_fields(
        &request,
        Instant::now(),
        "cancelled",
        failure(
          CapabilityFailureKind::Cancelled,
          "WOML_CAPABILITY_CANCELLED",
          "The capability call was cancelled before policy-authorized dispatch.",
          false,
          false,
        ),
      )));
    }
    {
      let mut store = self.store.lock().await;
      let payload = RunEventPayload::OperationStarted(OperationStartedData {
        node_id: request.node_id.clone(),
        attempt_number: request.attempt_number,
        invocation_id: request.invocation_id.clone(),
        call_id: request.call_id.clone(),
        operation_key: request.identity.operation_key.clone(),
        capability: request.capability.clone(),
        operation: request.operation.clone(),
        execution_mode: OperationExecutionMode::Managed,
        metadata: metadata.clone(),
      });
      if let Some(decision) = policy_decision.as_ref() {
        store.append_payload(
          request.run_id.clone(),
          generated_event_id(),
          decision.evaluated_at,
          RunEventPayload::InfrastructurePolicyDecisionRecorded(
            InfrastructurePolicyDecisionRecordedData {
              invocation_id: request.invocation_id.clone(),
              call_id: request.call_id.clone(),
              operation_key: request.identity.operation_key.clone(),
              decision: decision.clone(),
            },
          ),
        )?;
        if decision.outcome != InfrastructurePolicyDecisionOutcomeV1::Allow {
          let (code, message) = match decision.outcome {
            InfrastructurePolicyDecisionOutcomeV1::Deny => (
              "WOML_POLICY_DENIED",
              "The admitted infrastructure policy denied this protected effect.",
            ),
            InfrastructurePolicyDecisionOutcomeV1::RequirementsUnsatisfied => (
              "WOML_POLICY_REQUIREMENTS_UNSATISFIED",
              "Trusted evidence required by the infrastructure policy is missing or insufficient.",
            ),
            InfrastructurePolicyDecisionOutcomeV1::Allow => unreachable!(),
          };
          return Ok(failed_result(
            &request,
            Instant::now(),
            failure(
              CapabilityFailureKind::ServiceRejected,
              code,
              message,
              false,
              false,
            ),
          ));
        }
      }
      if let Some(scope) = iteration_scope.clone() {
        store.append_payload_scoped(
          request.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          Some(scope),
          payload,
        )?;
      } else {
        store.append_payload(
          request.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          payload,
        )?;
      }
    }

    let result = self
      .registry
      .execute_scoped(request.clone(), Some(workflow_scope), cancellation)
      .await;
    let payload = match &result {
      CapabilityCallResult::Succeeded(success) => {
        let encoded = serde_json::to_vec(&success.result).unwrap_or_default();
        let mut result_metadata = if matches!(request.capability.as_str(), "state" | "process") {
          metadata.clone()
        } else {
          Map::new()
        };
        result_metadata.extend(
          self
            .registry
            .safe_result_metadata(&request, &success.result)
            .unwrap_or_default(),
        );
        if matches!(request.capability.as_str(), "state" | "artifacts") {
          result_metadata.insert("durationMs".to_string(), Value::from(success.duration_ms));
        }
        RunEventPayload::OperationSucceeded(OperationSucceededData {
          node_id: request.node_id.clone(),
          attempt_number: request.attempt_number,
          invocation_id: request.invocation_id.clone(),
          call_id: request.call_id.clone(),
          operation_key: request.identity.operation_key.clone(),
          capability: request.capability.clone(),
          operation: request.operation.clone(),
          execution_mode: OperationExecutionMode::Managed,
          metadata: result_metadata,
          duration_ms: success.duration_ms,
          result_bytes: success.result_bytes,
          result_digest: format!("sha256:{}", hex::encode(Sha256::digest(&encoded))),
        })
      }
      CapabilityCallResult::Failed(failed) | CapabilityCallResult::Cancelled(failed) => {
        let mut metadata = metadata;
        metadata.extend(
          self
            .registry
            .safe_failure_metadata(&request, &failed.error)
            .unwrap_or_default(),
        );
        if request.capability == "process" {
          metadata.insert("durationMs".to_string(), Value::from(failed.duration_ms));
        }
        if request.capability == "state" {
          let outcome = if failed.error.kind == CapabilityFailureKind::Cancelled {
            "cancelled"
          } else if failed.error.code == "WOML_STATE_CONFLICT" {
            "conflict"
          } else if failed.error.code == "WOML_STATE_QUOTA_EXCEEDED" {
            "quota_exceeded"
          } else {
            "failed"
          };
          metadata.insert("outcome".to_string(), Value::String(outcome.to_string()));
          metadata.insert("durationMs".to_string(), Value::from(failed.duration_ms));
        }
        if request.capability == "artifacts" {
          metadata.insert("durationMs".to_string(), Value::from(failed.duration_ms));
        }
        RunEventPayload::OperationFailed(OperationFailedData {
          node_id: request.node_id.clone(),
          attempt_number: request.attempt_number,
          invocation_id: request.invocation_id.clone(),
          call_id: request.call_id.clone(),
          operation_key: request.identity.operation_key.clone(),
          capability: request.capability.clone(),
          operation: request.operation.clone(),
          execution_mode: OperationExecutionMode::Managed,
          metadata,
          duration_ms: failed.duration_ms,
          failure: failed.error.clone(),
        })
      }
    };
    {
      let mut store = self.store.lock().await;
      if let Some(scope) = iteration_scope {
        store.append_payload_scoped(
          request.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          Some(scope),
          payload,
        )?;
      } else {
        store.append_payload(
          request.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          payload,
        )?;
      }
    }
    Ok(result)
  }

  pub async fn observe_native_fetch(
    &self,
    identity: &NativeFetchInvocationIdentity,
    observation: NativeFetchObservation,
  ) -> Result<(), DurableCapabilityAuthorityError> {
    observation
      .validate()
      .map_err(DurableStoreError::Contract)?;
    if observation.invocation_id() != identity.invocation_id {
      return Err(
        DurableStoreError::Contract(
          "Native Fetch observation crossed invocation boundaries.".to_string(),
        )
        .into(),
      );
    }
    let event_schema_version = {
      let store = self.store.lock().await;
      store
        .events(&identity.run_id)?
        .first()
        .map(|event| event.event_schema_version)
        .ok_or_else(|| {
          DurableStoreError::Contract(
            "Native Fetch observation references a run without durable history.".to_string(),
          )
        })?
    };
    if event_schema_version < crate::RUN_EVENT_SCHEMA_VERSION_V8 {
      // Models v1-v7 predate native-Fetch observations. Their stored behavior
      // remains executable; every newly compiled Fetch script targets Model v8.
      return Ok(());
    }

    match observation {
      NativeFetchObservation::Started {
        request_id,
        method,
        origin,
        path,
        request_body_bytes,
        ..
      } => {
        let operation_key = derive_operation_key(
          &identity.step_idempotency_key,
          &format!("native-fetch.{request_id}"),
        );
        {
          let mut store = self.store.lock().await;
          if self.infrastructure_policy_required
            || store
              .infrastructure_policy_binding(&identity.run_id)?
              .is_some()
          {
            if let Some(policy) = store.infrastructure_policy_for_run(&identity.run_id)? {
              let binding = store.run_binding(&identity.run_id)?;
              let projection = store.projection(&identity.run_id)?;
              let execution_mode = infrastructure_policy_execution_mode(
                &projection,
                &identity.node_id,
                identity.attempt_number,
              );
              let evaluated_at = Utc::now();
              let decision = self
                .infrastructure_policy_evaluators
                .prepare(policy)
                .input_unsupported_decision(
                  &identity.run_id,
                  &binding.workflow_id,
                  &binding.definition_hash,
                  &identity.node_id,
                  identity.attempt_number,
                  &identity.invocation_id,
                  &operation_key,
                  "http",
                  "fetch",
                  CapabilityEffect::UnsafeWrite,
                  execution_mode,
                  evaluated_at,
                );
              store.append_payload(
                identity.run_id.clone(),
                generated_event_id(),
                evaluated_at,
                RunEventPayload::InfrastructurePolicyDecisionRecorded(
                  InfrastructurePolicyDecisionRecordedData {
                    invocation_id: identity.invocation_id.clone(),
                    call_id: request_id.clone(),
                    operation_key: operation_key.clone(),
                    decision,
                  },
                ),
              )?;
            }
            return Err(
              DurableStoreError::Contract(
                "WOML_POLICY_INPUT_UNSUPPORTED: native fetch is blocked before network I/O while Infrastructure Policy enforcement is active; use services.http.request()."
                  .to_string(),
              )
              .into(),
            );
          }
        }
        let iteration_scope = {
          let store = self.store.lock().await;
          let projection = store.projection(&identity.run_id)?;
          let binding = store.run_binding(&identity.run_id)?;
          let attempt = projection.attempts.iter().find(|attempt| {
            attempt.identity.node_id == identity.node_id
              && attempt.identity.attempt == identity.attempt_number
              && attempt.identity.invocation_id == identity.invocation_id
              && attempt.status == crate::projection::AttemptStatus::Started
          });
          let lifecycle_key = projection.lifecycle_hooks.values().find_map(|hook| {
            hook.actions.get(&identity.node_id).and_then(|action| {
              (action.attempt == identity.attempt_number
                && action.status == crate::projection::LifecycleActionStatus::Started)
                .then(|| {
                  crate::step_effect_idempotency_key(
                    &identity.run_id,
                    &binding.definition_hash,
                    &identity.node_id,
                  )
                })
            })
          });
          let root_key = attempt
            .and_then(|attempt| attempt.idempotency_key.as_deref())
            .map(str::to_string)
            .or(lifecycle_key);
          let iteration = iteration_attempt_identity(
            &projection,
            &identity.run_id,
            &binding.definition_hash,
            &identity.node_id,
            identity.attempt_number,
            &identity.invocation_id,
          );
          let (iteration_scope, iteration_key) = iteration
            .map(|(scope, key)| (Some(scope), Some(key)))
            .unwrap_or((None, None));
          if root_key.or(iteration_key).as_deref() != Some(identity.step_idempotency_key.as_str()) {
            return Err(
              DurableStoreError::Contract(
                "Native Fetch identity does not match its active durable attempt.".to_string(),
              )
              .into(),
            );
          }
          iteration_scope
        };
        let mut metadata = Map::new();
        metadata.insert("method".to_string(), Value::String(method));
        metadata.insert("origin".to_string(), Value::String(origin));
        metadata.insert("path".to_string(), Value::String(path));
        if let Some(bytes) = request_body_bytes {
          metadata.insert("requestBodyBytes".to_string(), Value::from(bytes));
        }
        validate_safe_metadata(&metadata).map_err(DurableStoreError::Contract)?;
        let payload = RunEventPayload::OperationStarted(OperationStartedData {
          node_id: identity.node_id.clone(),
          attempt_number: identity.attempt_number,
          invocation_id: identity.invocation_id.clone(),
          call_id: request_id,
          operation_key,
          capability: "http".to_string(),
          operation: "fetch".to_string(),
          execution_mode: OperationExecutionMode::Observed,
          metadata,
        });
        let mut store = self.store.lock().await;
        if let Some(scope) = iteration_scope {
          store.append_payload_scoped(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            Some(scope),
            payload,
          )?;
        } else {
          store.append_payload(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            payload,
          )?;
        }
      }
      NativeFetchObservation::Completed {
        request_id,
        status,
        response_body_bytes,
        duration_ms,
        ..
      } => {
        let (iteration_scope, operation) = {
          let store = self.store.lock().await;
          operation_with_iteration_scope(
            &store.projection(&identity.run_id)?,
            &crate::projection::OperationIdentity {
              invocation_id: identity.invocation_id.clone(),
              call_id: request_id.clone(),
            },
          )
          .ok_or_else(|| {
            DurableStoreError::Contract("Native Fetch completion has no durable start.".to_string())
          })?
        };
        let mut metadata = Map::new();
        metadata.insert("status".to_string(), Value::from(status));
        if let Some(bytes) = response_body_bytes.as_u64() {
          metadata.insert("responseBodyBytes".to_string(), Value::from(bytes));
        }
        let payload = RunEventPayload::OperationSucceeded(OperationSucceededData {
          node_id: operation.node_id,
          attempt_number: operation.attempt_number,
          invocation_id: operation.identity.invocation_id,
          call_id: operation.identity.call_id,
          operation_key: operation.operation_key,
          capability: operation.capability,
          operation: operation.operation,
          execution_mode: operation.execution_mode,
          metadata,
          duration_ms,
          result_bytes: 0,
          result_digest: format!("sha256:{}", hex::encode(Sha256::digest([]))),
        });
        let mut store = self.store.lock().await;
        if let Some(scope) = iteration_scope {
          store.append_payload_scoped(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            Some(scope),
            payload,
          )?;
        } else {
          store.append_payload(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            payload,
          )?;
        }
      }
      NativeFetchObservation::Failed {
        request_id,
        duration_ms,
        error,
        ..
      } => {
        let (iteration_scope, operation) = {
          let store = self.store.lock().await;
          operation_with_iteration_scope(
            &store.projection(&identity.run_id)?,
            &crate::projection::OperationIdentity {
              invocation_id: identity.invocation_id.clone(),
              call_id: request_id.clone(),
            },
          )
          .ok_or_else(|| {
            DurableStoreError::Contract("Native Fetch failure has no durable start.".to_string())
          })?
        };
        let kind = match error.kind {
          NativeFetchFailureKind::TimedOut => CapabilityFailureKind::TimedOut,
          NativeFetchFailureKind::Cancelled => CapabilityFailureKind::Cancelled,
          NativeFetchFailureKind::WorkerCrashed => CapabilityFailureKind::WorkerCrashed,
          NativeFetchFailureKind::HostCrashed => CapabilityFailureKind::HostCrashed,
          NativeFetchFailureKind::TrackingFailed | NativeFetchFailureKind::FetchRejected => {
            CapabilityFailureKind::TransportFailed
          }
        };
        let payload = RunEventPayload::OperationFailed(OperationFailedData {
          node_id: operation.node_id,
          attempt_number: operation.attempt_number,
          invocation_id: operation.identity.invocation_id,
          call_id: operation.identity.call_id,
          operation_key: operation.operation_key,
          capability: operation.capability,
          operation: operation.operation,
          execution_mode: operation.execution_mode,
          metadata: Map::new(),
          duration_ms,
          failure: CapabilityFailure {
            kind,
            code: error.code,
            message: error.message,
            retryable: false,
            ambiguous: true,
            details: None,
          },
        });
        let mut store = self.store.lock().await;
        if let Some(scope) = iteration_scope {
          store.append_payload_scoped(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            Some(scope),
            payload,
          )?;
        } else {
          store.append_payload(
            identity.run_id.clone(),
            generated_event_id(),
            Utc::now(),
            payload,
          )?;
        }
      }
    }
    Ok(())
  }

  pub async fn close_active_native_fetches(
    &self,
    identity: &NativeFetchInvocationIdentity,
    failure: CapabilityFailure,
  ) -> Result<(), DurableCapabilityAuthorityError> {
    failure.validate().map_err(DurableStoreError::Contract)?;
    let operations = {
      let store = self.store.lock().await;
      let projection = store.projection(&identity.run_id)?;
      let mut operations = projection
        .operations
        .values()
        .filter(|operation| {
          operation.identity.invocation_id == identity.invocation_id
            && operation.execution_mode == OperationExecutionMode::Observed
            && operation.status == crate::projection::OperationStatus::Started
        })
        .cloned()
        .map(|operation| (None, operation))
        .collect::<Vec<_>>();
      for loop_ in projection.for_each.values() {
        for (index, execution) in &loop_.executions {
          operations.extend(
            execution
              .operations
              .values()
              .filter(|operation| {
                operation.identity.invocation_id == identity.invocation_id
                  && operation.execution_mode == OperationExecutionMode::Observed
                  && operation.status == crate::projection::OperationStatus::Started
              })
              .cloned()
              .map(|operation| {
                (
                  Some(crate::event::ForEachIterationScope {
                    for_each_id: loop_.for_each_id.clone(),
                    index: *index,
                  }),
                  operation,
                )
              }),
          );
        }
      }
      operations
    };
    for (iteration_scope, operation) in operations {
      let payload = RunEventPayload::OperationFailed(OperationFailedData {
        node_id: operation.node_id,
        attempt_number: operation.attempt_number,
        invocation_id: operation.identity.invocation_id,
        call_id: operation.identity.call_id,
        operation_key: operation.operation_key,
        capability: operation.capability,
        operation: operation.operation,
        execution_mode: operation.execution_mode,
        metadata: operation.metadata,
        duration_ms: 0.0,
        failure: failure.clone(),
      });
      let mut store = self.store.lock().await;
      if let Some(scope) = iteration_scope {
        store.append_payload_scoped(
          identity.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          Some(scope),
          payload,
        )?;
      } else {
        store.append_payload(
          identity.run_id.clone(),
          generated_event_id(),
          Utc::now(),
          payload,
        )?;
      }
    }
    Ok(())
  }
}

impl Default for CapabilityRegistry {
  fn default() -> Self {
    Self::new(
      DEFAULT_CAPABILITY_IN_FLIGHT_PER_INVOCATION,
      DEFAULT_CAPABILITY_IN_FLIGHT_GLOBAL,
    )
  }
}

impl CapabilityRegistry {
  pub fn new(max_per_invocation: usize, max_global: usize) -> Self {
    Self {
      handlers: Mutex::new(HashMap::new()),
      in_flight_by_invocation: Mutex::new(HashMap::new()),
      in_flight_global: AtomicUsize::new(0),
      max_per_invocation: max_per_invocation.max(1),
      max_global: max_global.max(1),
    }
  }

  pub fn register(
    &self,
    handler: Arc<dyn CapabilityHandler>,
  ) -> Result<(), CapabilityRegistryError> {
    let descriptor = handler.descriptor();
    let key = (descriptor.capability.clone(), descriptor.operation.clone());
    let mut handlers = self.handlers.lock().expect("capability registry lock");
    if handlers.contains_key(&key) {
      return Err(CapabilityRegistryError::DuplicateHandler {
        capability: key.0,
        operation: key.1,
      });
    }
    handlers.insert(key, handler);
    Ok(())
  }

  pub fn descriptor(&self, capability: &str, operation: &str) -> Option<CapabilityDescriptor> {
    self
      .handlers
      .lock()
      .expect("capability registry lock")
      .get(&(capability.to_string(), operation.to_string()))
      .map(|handler| handler.descriptor())
  }

  pub fn safe_metadata(
    &self,
    request: &CapabilityCallRequest,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    request.validate()?;
    let input_bytes = serialized_bytes(&request.input);
    if input_bytes > request.limits.input_bytes {
      return Err(failure_with_size(
        CapabilityFailureKind::InputTooLarge,
        "WOML_CAPABILITY_INPUT_TOO_LARGE",
        "The capability input exceeds its configured byte limit.",
        input_bytes,
        request.limits.input_bytes,
      ));
    }
    let handler = self.lookup_and_validate(request)?;
    let metadata = handler.safe_request_metadata(request)?;
    validate_safe_metadata(&metadata).map_err(|message| {
      failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_CAPABILITY_UNSAFE_METADATA",
        message,
        false,
        false,
      )
    })?;
    Ok(metadata)
  }

  /// Builds a trusted operation descriptor after both the generic capability
  /// contract and the concrete registered handler have accepted the request.
  pub fn infrastructure_policy_descriptor(
    &self,
    request: &CapabilityCallRequest,
    context: InfrastructurePolicyDescriptorContextV1,
  ) -> Result<InfrastructurePolicyOperationDescriptorV1, CapabilityFailure> {
    request.validate()?;
    let input_bytes = serialized_bytes(&request.input);
    if input_bytes > request.limits.input_bytes {
      return Err(failure_with_size(
        CapabilityFailureKind::InputTooLarge,
        "WOML_CAPABILITY_INPUT_TOO_LARGE",
        "The capability input exceeds its configured byte limit.",
        input_bytes,
        request.limits.input_bytes,
      ));
    }
    let handler = self.lookup_and_validate(request)?;
    let metadata = handler.infrastructure_policy_metadata(request)?;
    InfrastructurePolicyOperationDescriptorV1::build(
      request,
      &handler.descriptor(),
      handler.effect(&request.input),
      context,
      metadata,
    )
  }

  pub async fn execute(
    &self,
    request: CapabilityCallRequest,
    cancellation: CapabilityCancellationToken,
  ) -> CapabilityCallResult {
    self.execute_scoped(request, None, cancellation).await
  }

  pub(crate) async fn execute_scoped(
    &self,
    request: CapabilityCallRequest,
    workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> CapabilityCallResult {
    let started = Instant::now();
    let handler = match self.lookup_and_validate(&request) {
      Ok(handler) => handler,
      Err(error) => return failed_result(&request, started, error),
    };

    let input_bytes = serialized_bytes(&request.input);
    if input_bytes > request.limits.input_bytes {
      return failed_result(
        &request,
        started,
        failure_with_size(
          CapabilityFailureKind::InputTooLarge,
          "WOML_CAPABILITY_INPUT_TOO_LARGE",
          "The capability input exceeds its configured byte limit.",
          input_bytes,
          request.limits.input_bytes,
        ),
      );
    }
    let _permit = match self.acquire(&request.invocation_id) {
      Some(permit) => permit,
      None => {
        return failed_result(
          &request,
          started,
          failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_CAPABILITY_BACKPRESSURE",
            "The capability concurrency limit is currently full.",
            true,
            false,
          ),
        )
      }
    };
    let effect = handler.effect(&request.input);
    let supports_cancellation = handler.descriptor().supports_cancellation;
    let may_have_effect = effect != CapabilityEffect::Read;
    let safe_to_retry = effect != CapabilityEffect::UnsafeWrite;

    let handler_future = AssertUnwindSafe(handler.execute_request_scoped(
      &request,
      workflow_scope,
      cancellation.clone(),
    ))
    .catch_unwind();
    let timeout = time::sleep(time::Duration::from_millis(request.limits.timeout_ms));
    tokio::pin!(handler_future);
    tokio::pin!(timeout);
    let outcome = tokio::select! {
      biased;
      _ = cancellation.cancelled() => {
        let fallback = failure(
          CapabilityFailureKind::Cancelled,
          "WOML_CAPABILITY_CANCELLED",
          "The capability call was cancelled.",
          false,
          may_have_effect,
        );
        if supports_cancellation {
          match time::timeout(
            time::Duration::from_millis(CAPABILITY_CANCELLATION_CLEANUP_MS),
            &mut handler_future,
          ).await {
            Ok(Ok(Err(error))) if error.kind == CapabilityFailureKind::Cancelled => Err(error),
            _ => Err(fallback),
          }
        } else {
          Err(fallback)
        }
      },
      _ = &mut timeout => {
        let fallback = failure(
          CapabilityFailureKind::TimedOut,
          "WOML_CAPABILITY_TIMED_OUT",
          "The capability call exceeded its deadline.",
          safe_to_retry,
          may_have_effect,
        );
        if supports_cancellation {
          cancellation.cancel();
          match time::timeout(
            time::Duration::from_millis(CAPABILITY_CANCELLATION_CLEANUP_MS),
            &mut handler_future,
          ).await {
            Ok(Ok(Err(error))) if error.kind == CapabilityFailureKind::TimedOut => Err(error),
            _ => Err(fallback),
          }
        } else {
          Err(fallback)
        }
      },
      result = &mut handler_future => match result {
        Ok(result) => result,
        Err(_) => Err(failure(
          CapabilityFailureKind::HandlerCrashed,
          "WOML_CAPABILITY_HANDLER_CRASHED",
          "The capability handler crashed.",
          false,
          may_have_effect,
        )),
      }
    };

    match outcome {
      Ok(result) => {
        let result_bytes = serialized_bytes(&result);
        if result_bytes > request.limits.result_bytes {
          failed_result(
            &request,
            started,
            failure_with_size(
              CapabilityFailureKind::ResultTooLarge,
              "WOML_CAPABILITY_RESULT_TOO_LARGE",
              "The capability result exceeds its configured byte limit.",
              result_bytes,
              request.limits.result_bytes,
            ),
          )
        } else {
          CapabilityCallResult::Succeeded(CapabilityCallSucceeded {
            contract: CAPABILITY_CALL_CONTRACT.to_string(),
            contract_version: CAPABILITY_CALL_CONTRACT_VERSION,
            message_type: "result".to_string(),
            invocation_id: request.invocation_id,
            call_id: request.call_id,
            outcome: "succeeded".to_string(),
            result_contract_version: request.result_contract_version,
            result_bytes,
            duration_ms: duration_ms(started),
            result,
          })
        }
      }
      Err(error) if error.kind == CapabilityFailureKind::Cancelled => {
        CapabilityCallResult::Cancelled(failed_fields(&request, started, "cancelled", error))
      }
      Err(error) => failed_result(&request, started, error),
    }
  }

  fn lookup_and_validate(
    &self,
    request: &CapabilityCallRequest,
  ) -> Result<Arc<dyn CapabilityHandler>, CapabilityFailure> {
    request.validate()?;
    let handlers = self.handlers.lock().expect("capability registry lock");
    let handler = handlers
      .get(&(request.capability.clone(), request.operation.clone()))
      .cloned()
      .ok_or_else(|| {
        let kind = if handlers
          .keys()
          .any(|(capability, _)| capability == &request.capability)
        {
          CapabilityFailureKind::UnsupportedOperation
        } else {
          CapabilityFailureKind::UnsupportedCapability
        };
        failure(
          kind,
          if kind == CapabilityFailureKind::UnsupportedCapability {
            "WOML_CAPABILITY_UNSUPPORTED"
          } else {
            "WOML_CAPABILITY_OPERATION_UNSUPPORTED"
          },
          "No matching capability handler is registered.",
          false,
          false,
        )
      })?;
    let descriptor = handler.descriptor();
    if descriptor.input_contract_version != request.input_contract_version
      || descriptor.result_contract_version != request.result_contract_version
    {
      return Err(failure(
        CapabilityFailureKind::UnsupportedVersion,
        "WOML_CAPABILITY_VERSION_UNSUPPORTED",
        "The requested capability contract version is not supported.",
        false,
        false,
      ));
    }
    if request.identity.provider_idempotency_key.is_some()
      && !descriptor.supports_provider_idempotency
    {
      return Err(failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_CAPABILITY_IDEMPOTENCY_UNSUPPORTED",
        "This capability does not support provider idempotency keys.",
        false,
        false,
      ));
    }
    handler.validate_request(request)?;
    Ok(handler)
  }

  pub fn safe_result_metadata(
    &self,
    request: &CapabilityCallRequest,
    result: &Value,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    let handler = self.lookup_and_validate(request)?;
    let metadata = handler.safe_result_metadata(result);
    validate_safe_metadata(&metadata).map_err(|message| {
      failure(
        CapabilityFailureKind::InvalidResult,
        "WOML_CAPABILITY_UNSAFE_RESULT_METADATA",
        message,
        false,
        false,
      )
    })?;
    Ok(metadata)
  }

  pub fn safe_failure_metadata(
    &self,
    request: &CapabilityCallRequest,
    failure: &CapabilityFailure,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    let handler = self.lookup_and_validate(request)?;
    let metadata = handler.safe_failure_metadata(failure);
    validate_safe_metadata(&metadata).map_err(|message| {
      crate::capability::failure(
        CapabilityFailureKind::InvalidResult,
        "WOML_CAPABILITY_UNSAFE_FAILURE_METADATA",
        message,
        false,
        false,
      )
    })?;
    Ok(metadata)
  }

  fn acquire(&self, invocation_id: &str) -> Option<CapabilityPermit<'_>> {
    let mut per_invocation = self
      .in_flight_by_invocation
      .lock()
      .expect("capability in-flight lock");
    let current = per_invocation.get(invocation_id).copied().unwrap_or(0);
    if current >= self.max_per_invocation
      || self.in_flight_global.load(Ordering::SeqCst) >= self.max_global
    {
      return None;
    }
    per_invocation.insert(invocation_id.to_string(), current + 1);
    self.in_flight_global.fetch_add(1, Ordering::SeqCst);
    Some(CapabilityPermit {
      registry: self,
      invocation_id: invocation_id.to_string(),
    })
  }
}

struct CapabilityPermit<'a> {
  registry: &'a CapabilityRegistry,
  invocation_id: String,
}

impl Drop for CapabilityPermit<'_> {
  fn drop(&mut self) {
    let mut per_invocation = self
      .registry
      .in_flight_by_invocation
      .lock()
      .expect("capability in-flight lock");
    if let Some(count) = per_invocation.get_mut(&self.invocation_id) {
      *count -= 1;
      if *count == 0 {
        per_invocation.remove(&self.invocation_id);
      }
    }
    self
      .registry
      .in_flight_global
      .fetch_sub(1, Ordering::SeqCst);
  }
}

impl CapabilityCallRequest {
  pub fn validate(&self) -> Result<(), CapabilityFailure> {
    let valid = self.contract == CAPABILITY_CALL_CONTRACT
      && self.contract_version == CAPABILITY_CALL_CONTRACT_VERSION
      && self.message_type == "request"
      && [
        &self.invocation_id,
        &self.call_id,
        &self.run_id,
        &self.node_id,
      ]
      .into_iter()
      .all(|value| !value.is_empty() && value.len() <= 256)
      && (1..=10).contains(&self.attempt_number)
      && valid_name(&self.capability)
      && valid_name(&self.operation)
      && self.input_contract_version > 0
      && self.result_contract_version > 0
      && valid_digest(&self.identity.step_idempotency_key)
      && valid_digest(&self.identity.operation_key)
      && !self.identity.operation_name.is_empty()
      && self.identity.operation_name.len() <= 256
      && self
        .identity
        .provider_idempotency_key
        .as_ref()
        .is_none_or(|value| !value.is_empty() && value.len() <= 256)
      && (1..=DEFAULT_CAPABILITY_FRAME_BYTES).contains(&self.limits.input_bytes)
      && (1..=DEFAULT_CAPABILITY_FRAME_BYTES).contains(&self.limits.result_bytes)
      && (1..=MAX_CAPABILITY_TIMEOUT_MS).contains(&self.limits.timeout_ms);
    if valid {
      Ok(())
    } else {
      Err(failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_CAPABILITY_REQUEST_INVALID",
        "The capability call does not match Capability Call v1.",
        false,
        false,
      ))
    }
  }
}

impl CapabilityFailure {
  pub fn validate(&self) -> Result<(), String> {
    let code_valid = !self.code.is_empty()
      && self.code.len() <= 128
      && self
        .code
        .starts_with(|character: char| character.is_ascii_uppercase())
      && self
        .code
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    if !code_valid || self.message.is_empty() || self.message.len() > 2048 {
      return Err("Capability failure has an invalid code or message.".to_string());
    }
    if let Some(details) = &self.details {
      validate_safe_metadata(details)?;
    }
    Ok(())
  }
}

pub fn derive_operation_key(step_idempotency_key: &str, operation_name: &str) -> String {
  let mut digest = Sha256::new();
  digest.update(b"woml.capability-operation\0v1\0");
  digest.update(step_idempotency_key.as_bytes());
  digest.update(b"\0");
  digest.update(operation_name.as_bytes());
  format!("sha256:{}", hex::encode(digest.finalize()))
}

pub fn validate_safe_metadata(metadata: &Map<String, Value>) -> Result<(), String> {
  const FORBIDDEN: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "token",
    "secret",
    "password",
    "body",
    "query",
    "url",
    "headers",
    "input",
    "output",
    "data",
    "value",
  ];
  if metadata.len() > 32 {
    return Err("Safe metadata may contain at most 32 fields.".to_string());
  }
  for (key, value) in metadata {
    let valid_key = key.len() <= 64
      && key
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
      && key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
      && !FORBIDDEN.contains(&key.to_ascii_lowercase().as_str());
    if !valid_key
      || !matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
      )
    {
      return Err(
        "Capability metadata must be bounded, scalar, and free of sensitive field names."
          .to_string(),
      );
    }
  }
  Ok(())
}

fn valid_name(value: &str) -> bool {
  value.len() <= 128
    && value.split(['.', '_', '-']).all(|segment| {
      let mut chars = segment.chars();
      matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && chars.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    })
}

fn valid_digest(value: &str) -> bool {
  value.strip_prefix("sha256:").is_some_and(|digest| {
    digest.len() == 64
      && digest
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
  })
}

fn serialized_bytes(value: &Value) -> u64 {
  serde_json::to_vec(value)
    .map(|bytes| bytes.len() as u64)
    .unwrap_or(u64::MAX)
}

fn validate_result_base(
  contract: &str,
  contract_version: u32,
  message_type: &str,
  invocation_id: &str,
  call_id: &str,
  duration_ms: f64,
) -> Result<(), String> {
  if contract != CAPABILITY_CALL_CONTRACT
    || contract_version != CAPABILITY_CALL_CONTRACT_VERSION
    || message_type != "result"
    || invocation_id.is_empty()
    || invocation_id.len() > 256
    || call_id.is_empty()
    || call_id.len() > 256
    || !duration_ms.is_finite()
    || duration_ms < 0.0
  {
    return Err(
      "Capability result does not match the Capability Call v1 base contract.".to_string(),
    );
  }
  Ok(())
}

fn generated_event_id() -> String {
  format!("event_{}", Uuid::new_v4().simple())
}

fn duration_ms(started: Instant) -> f64 {
  started.elapsed().as_secs_f64() * 1_000.0
}

fn operational_output_failure(code: &str, ambiguous: bool) -> CapabilityFailure {
  failure(
    CapabilityFailureKind::TransportFailed,
    code,
    "The operational-output authority could not accept this script record.",
    false,
    ambiguous,
  )
}

fn redact_operational_body<'a>(
  body: &mut crate::OperationalBodyV1,
  values: impl Iterator<Item = &'a String>,
) {
  let mut redactions = values
    .filter(|value| !value.is_empty())
    .cloned()
    .collect::<Vec<_>>();
  redactions.sort_by_key(|value| std::cmp::Reverse(value.len()));
  let redact = |text: &mut String| {
    for value in &redactions {
      *text = text.replace(value, "[redacted]");
    }
  };
  match body {
    crate::OperationalBodyV1::Output { text, .. } => redact(text),
    crate::OperationalBodyV1::Progress { message, .. } => {
      if let Some(message) = message {
        redact(message);
      }
    }
    _ => {}
  }
}

fn failure(
  kind: CapabilityFailureKind,
  code: impl Into<String>,
  message: impl Into<String>,
  retryable: bool,
  ambiguous: bool,
) -> CapabilityFailure {
  CapabilityFailure {
    kind,
    code: code.into(),
    message: message.into(),
    retryable,
    ambiguous,
    details: None,
  }
}

fn failure_with_size(
  kind: CapabilityFailureKind,
  code: &str,
  message: &str,
  actual_bytes: u64,
  limit_bytes: u64,
) -> CapabilityFailure {
  let mut details = Map::new();
  details.insert("actualBytes".to_string(), Value::from(actual_bytes));
  details.insert("limitBytes".to_string(), Value::from(limit_bytes));
  CapabilityFailure {
    kind,
    code: code.to_string(),
    message: message.to_string(),
    retryable: false,
    ambiguous: false,
    details: Some(details),
  }
}

fn failed_fields(
  request: &CapabilityCallRequest,
  started: Instant,
  outcome: &str,
  error: CapabilityFailure,
) -> CapabilityCallFailed {
  CapabilityCallFailed {
    contract: CAPABILITY_CALL_CONTRACT.to_string(),
    contract_version: CAPABILITY_CALL_CONTRACT_VERSION,
    message_type: "result".to_string(),
    invocation_id: request.invocation_id.clone(),
    call_id: request.call_id.clone(),
    outcome: outcome.to_string(),
    duration_ms: duration_ms(started),
    error,
  }
}

fn failed_result(
  request: &CapabilityCallRequest,
  started: Instant,
  error: CapabilityFailure,
) -> CapabilityCallResult {
  CapabilityCallResult::Failed(failed_fields(request, started, "failed", error))
}

/// Builds a safe correlated failure when a capability request cannot reach a
/// durable authority. The original input is deliberately excluded.
pub fn capability_transport_failure(
  request: &CapabilityCallRequest,
  code: &str,
  message: &str,
  retryable: bool,
  ambiguous: bool,
) -> CapabilityCallResult {
  failed_result(
    request,
    Instant::now(),
    failure(
      CapabilityFailureKind::TransportFailed,
      code,
      message,
      retryable,
      ambiguous,
    ),
  )
}

/// Deterministic fake handler used by Rust conformance tests. It is never
/// registered in a production registry automatically.
#[derive(Debug, Default)]
pub struct TestCapabilityHandler;

impl CapabilityHandler for TestCapabilityHandler {
  fn descriptor(&self) -> CapabilityDescriptor {
    CapabilityDescriptor {
      capability: "test".to_string(),
      operation: "control".to_string(),
      input_contract_version: 1,
      result_contract_version: 1,
      effect: CapabilityEffect::Read,
      supports_cancellation: true,
      supports_provider_idempotency: false,
    }
  }

  fn safe_metadata(&self, input: &Value) -> Map<String, Value> {
    let mut metadata = Map::new();
    if let Some(mode) = input.get("mode").and_then(Value::as_str) {
      metadata.insert("mode".to_string(), Value::String(mode.to_string()));
    }
    metadata
  }

  fn execute(
    &self,
    input: Value,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    Box::pin(async move {
      match input.get("mode").and_then(Value::as_str).unwrap_or("echo") {
        "echo" => Ok(input.get("value").cloned().unwrap_or(Value::Null)),
        "delay" => {
          let delay_ms = input.get("delayMs").and_then(Value::as_u64).unwrap_or(1);
          tokio::select! {
            _ = time::sleep(time::Duration::from_millis(delay_ms)) => Ok(input.get("value").cloned().unwrap_or(Value::Null)),
            _ = cancellation.cancelled() => Err(failure(CapabilityFailureKind::Cancelled, "WOML_TEST_CANCELLED", "The test operation was cancelled.", false, false)),
          }
        }
        "fail" => Err(failure(
          CapabilityFailureKind::ServiceRejected,
          "WOML_TEST_FAILURE",
          "The test capability was instructed to fail.",
          false,
          false,
        )),
        "panic" => panic!("test capability panic"),
        _ => Err(failure(
          CapabilityFailureKind::InvalidInput,
          "WOML_TEST_MODE_INVALID",
          "The test capability mode is invalid.",
          false,
          false,
        )),
      }
    })
  }
}
