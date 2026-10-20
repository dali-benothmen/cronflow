//! Callback-scoped lock capability built on the durable markup-lock authority.
//!
//! Bun owns only the callback. Rust owns acquisition, renewal, fencing,
//! assertion, release, cancellation, and terminal cleanup.

use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
  },
  time::Duration,
};

use chrono::Utc;
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{
  CapabilityCallRequest, CapabilityCancellationToken, CapabilityDescriptor, CapabilityEffect,
  CapabilityFailure, CapabilityFailureKind, CapabilityHandler, DurableEventStore,
  LockLeaseMaintenance, ProgrammaticLockAcquireState,
};

pub const LOCKS_CAPABILITY_CONTRACT: &str = "woml.locks-capability";
pub const LOCKS_CAPABILITY_CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcquireInput {
  key: String,
  acquire_timeout_ms: u64,
  lease_ms: u64,
  operation_identity: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionInput {
  session_id: String,
}

#[derive(Debug, Clone)]
struct ActiveSession {
  run_id: String,
  invocation_id: String,
  key: String,
  lease_ms: u64,
  stop: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
pub struct ManagedLocksService {
  state_path: RwLock<Option<PathBuf>>,
  sessions: Mutex<HashMap<String, ActiveSession>>,
  active_invocations: Mutex<HashMap<String, String>>,
}

impl ManagedLocksService {
  pub fn configure_for_state(&self, path: &Path) -> Result<(), CapabilityFailure> {
    let mut configured = self.state_path.write().map_err(|_| unavailable())?;
    match configured.as_ref() {
      Some(current) if current != path => {
        let has_sessions = !self.sessions.lock().map_err(|_| unavailable())?.is_empty();
        if has_sessions {
          return Err(failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_LOCK_AUTHORITY_CONFLICT",
            "The lock capability is already attached to another active durable store.",
            false,
          ));
        }
        *configured = Some(path.to_path_buf());
      }
      None => *configured = Some(path.to_path_buf()),
      _ => {}
    }
    Ok(())
  }

  pub fn handlers(service: Arc<Self>) -> Vec<Arc<dyn CapabilityHandler>> {
    [
      LockOperation::Acquire,
      LockOperation::AssertCurrent,
      LockOperation::Release,
    ]
    .into_iter()
    .map(|operation| {
      Arc::new(ManagedLocksHandler {
        service: Arc::clone(&service),
        operation,
      }) as Arc<dyn CapabilityHandler>
    })
    .collect()
  }

  fn open_store(&self) -> Result<DurableEventStore, CapabilityFailure> {
    let path = self
      .state_path
      .read()
      .map_err(|_| unavailable())?
      .clone()
      .ok_or_else(unavailable)?;
    DurableEventStore::open(path).map_err(|_| unavailable())
  }

  async fn acquire(
    self: &Arc<Self>,
    request: CapabilityCallRequest,
    input: AcquireInput,
    cancellation: CapabilityCancellationToken,
  ) -> Result<Value, CapabilityFailure> {
    validate_acquire_input(&request, &input)?;
    let session_id = format!(
      "lock_{}",
      &hex::encode(Sha256::digest(
        format!(
          "{}\0{}",
          request.invocation_id, request.identity.operation_key
        )
        .as_bytes()
      ))[..32]
    );
    {
      let mut active = self.active_invocations.lock().map_err(|_| unavailable())?;
      if let Some(existing) = active.get(&request.invocation_id) {
        if existing != &session_id {
          return Err(failure(
            CapabilityFailureKind::ServiceRejected,
            "WOML_LOCK_NESTING_UNSUPPORTED",
            "One script attempt cannot hold or acquire more than one programmatic lease at a time.",
            false,
          ));
        }
      } else {
        active.insert(request.invocation_id.clone(), session_id.clone());
      }
    }

    let started_at = Utc::now();
    let mut store = match self.open_store() {
      Ok(store) => store,
      Err(error) => {
        self.release_reservation(&request.invocation_id, &session_id);
        return Err(error);
      }
    };
    let mut state = match store.acquire_programmatic_lock_atomically(
      &request.run_id,
      &request.node_id,
      &request.invocation_id,
      &session_id,
      &input.key,
      input.acquire_timeout_ms,
      input.lease_ms,
      started_at,
    ) {
      Ok(state) => state,
      Err(error) => {
        self.release_reservation(&request.invocation_id, &session_id);
        return Err(map_store_error(&error.to_string()));
      }
    };

    loop {
      match state.state {
        ProgrammaticLockAcquireState::Held => break,
        ProgrammaticLockAcquireState::TimedOut => {
          self.release_reservation(&request.invocation_id, &session_id);
          return Err(failure(
            CapabilityFailureKind::TimedOut,
            "WOML_LOCK_ACQUIRE_TIMED_OUT",
            "The lock was not acquired before acquireTimeout expired. Inspect the run to find the current holder or cancel this run safely.",
            false,
          ));
        }
        ProgrammaticLockAcquireState::Cancelled => {
          self.release_reservation(&request.invocation_id, &session_id);
          return Err(cancelled());
        }
        ProgrammaticLockAcquireState::Lost => {
          self.release_reservation(&request.invocation_id, &session_id);
          return Err(stale());
        }
        ProgrammaticLockAcquireState::Waiting => {}
      }
      tokio::select! {
        _ = cancellation.cancelled() => {
          let _ = store.cancel_programmatic_lock_waiter_atomically(
            &request.run_id,
            &session_id,
            input.lease_ms,
            Utc::now(),
          );
          self.release_reservation(&request.invocation_id, &session_id);
          return Err(cancelled());
        }
        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
      }
      state = store
        .programmatic_lock_state(&request.run_id, &session_id, input.lease_ms, Utc::now())
        .map_err(|error| map_store_error(&error.to_string()))?;
    }

    let lease = state.lease.ok_or_else(stale)?;
    let stop = Arc::new(AtomicBool::new(false));
    self.sessions.lock().map_err(|_| unavailable())?.insert(
      session_id.clone(),
      ActiveSession {
        run_id: request.run_id.clone(),
        invocation_id: request.invocation_id.clone(),
        key: lease.key.clone(),
        lease_ms: input.lease_ms,
        stop: Arc::clone(&stop),
      },
    );
    let service = Arc::clone(self);
    let renewal_session_id = session_id.clone();
    tokio::spawn(async move {
      service.renew_until_stopped(renewal_session_id, stop).await;
    });
    Ok(json!({
      "contract": LOCKS_CAPABILITY_CONTRACT,
      "contractVersion": LOCKS_CAPABILITY_CONTRACT_VERSION,
      "kind": "acquired",
      "sessionId": session_id,
      "key": lease.key,
      "fencingToken": lease.fencing_token,
      "acquiredAt": lease.acquired_at,
    }))
  }

  async fn renew_until_stopped(self: Arc<Self>, session_id: String, stop: Arc<AtomicBool>) {
    loop {
      let Some(session) = self
        .sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&session_id).cloned())
      else {
        return;
      };
      let interval = Duration::from_millis((session.lease_ms / 3).max(1_000));
      tokio::time::sleep(interval).await;
      if stop.load(Ordering::SeqCst) {
        return;
      }
      let maintenance = self.open_store().and_then(|mut store| {
        store
          .renew_programmatic_lock_atomically(
            &session.run_id,
            &session_id,
            session.lease_ms,
            Utc::now(),
          )
          .map_err(|_| unavailable())
      });
      if !matches!(maintenance, Ok(LockLeaseMaintenance::Renewed)) {
        stop.store(true, Ordering::SeqCst);
        // Keep the stale session correlated to its invocation until terminal
        // cleanup. The durable capability authority can then reject every
        // subsequent managed effect instead of treating lease loss as if no
        // protection had ever existed.
        return;
      }
    }
  }

  fn assert_current(
    &self,
    request: &CapabilityCallRequest,
    input: SessionInput,
  ) -> Result<Value, CapabilityFailure> {
    let session = self.owned_session(request, &input.session_id)?;
    let state = self
      .open_store()?
      .programmatic_lock_state(
        &session.run_id,
        &input.session_id,
        session.lease_ms,
        Utc::now(),
      )
      .map_err(|error| map_store_error(&error.to_string()))?;
    if state.state != ProgrammaticLockAcquireState::Held {
      return Err(stale());
    }
    Ok(json!({
      "contract": LOCKS_CAPABILITY_CONTRACT,
      "contractVersion": LOCKS_CAPABILITY_CONTRACT_VERSION,
      "kind": "current",
      "key": session.key,
    }))
  }

  fn release(
    &self,
    request: &CapabilityCallRequest,
    input: SessionInput,
  ) -> Result<Value, CapabilityFailure> {
    let session = self.owned_session(request, &input.session_id)?;
    session.stop.store(true, Ordering::SeqCst);
    let released_at = Utc::now();
    let released = self
      .open_store()?
      .release_programmatic_lock_atomically(
        &session.run_id,
        &input.session_id,
        session.lease_ms,
        released_at,
      )
      .map_err(|error| map_store_error(&error.to_string()))?;
    self
      .sessions
      .lock()
      .map_err(|_| unavailable())?
      .remove(&input.session_id);
    self.release_reservation(&session.invocation_id, &input.session_id);
    if !released {
      return Err(stale());
    }
    Ok(json!({
      "contract": LOCKS_CAPABILITY_CONTRACT,
      "contractVersion": LOCKS_CAPABILITY_CONTRACT_VERSION,
      "kind": "released",
      "sessionId": input.session_id,
      "key": session.key,
      "releasedAt": released_at,
    }))
  }

  fn owned_session(
    &self,
    request: &CapabilityCallRequest,
    session_id: &str,
  ) -> Result<ActiveSession, CapabilityFailure> {
    let session = self
      .sessions
      .lock()
      .map_err(|_| unavailable())?
      .get(session_id)
      .cloned()
      .ok_or_else(stale)?;
    if session.run_id != request.run_id || session.invocation_id != request.invocation_id {
      return Err(failure(
        CapabilityFailureKind::ServiceRejected,
        "WOML_LOCK_SESSION_FORBIDDEN",
        "The lease session does not belong to this script invocation.",
        false,
      ));
    }
    Ok(session)
  }

  pub fn close_invocation(&self, invocation_id: &str) {
    let sessions = self
      .sessions
      .lock()
      .ok()
      .map(|sessions| {
        sessions
          .iter()
          .filter(|(_, session)| session.invocation_id == invocation_id)
          .map(|(id, session)| (id.clone(), session.clone()))
          .collect::<Vec<_>>()
      })
      .unwrap_or_default();
    for (session_id, session) in sessions {
      session.stop.store(true, Ordering::SeqCst);
      if let Ok(mut store) = self.open_store() {
        let _ = store.release_programmatic_lock_atomically(
          &session.run_id,
          &session_id,
          session.lease_ms,
          Utc::now(),
        );
      }
      if let Ok(mut active) = self.sessions.lock() {
        active.remove(&session_id);
      }
      self.release_reservation(invocation_id, &session_id);
    }
  }

  pub fn managed_effect_is_current(&self, invocation_id: &str) -> Result<bool, CapabilityFailure> {
    let session_id = self
      .active_invocations
      .lock()
      .map_err(|_| unavailable())?
      .get(invocation_id)
      .cloned();
    let Some(session_id) = session_id else {
      return Ok(true);
    };
    let session = self
      .sessions
      .lock()
      .map_err(|_| unavailable())?
      .get(&session_id)
      .cloned();
    let Some(session) = session else {
      return Ok(false);
    };
    let state = self
      .open_store()?
      .programmatic_lock_state(&session.run_id, &session_id, session.lease_ms, Utc::now())
      .map_err(|error| map_store_error(&error.to_string()))?;
    Ok(state.state == ProgrammaticLockAcquireState::Held)
  }

  fn release_reservation(&self, invocation_id: &str, session_id: &str) {
    if let Ok(mut active) = self.active_invocations.lock() {
      if active
        .get(invocation_id)
        .is_some_and(|current| current == session_id)
      {
        active.remove(invocation_id);
      }
    }
  }
}

#[derive(Debug, Clone, Copy)]
enum LockOperation {
  Acquire,
  AssertCurrent,
  Release,
}

impl LockOperation {
  const fn name(self) -> &'static str {
    match self {
      Self::Acquire => "acquire",
      Self::AssertCurrent => "assert-current",
      Self::Release => "release",
    }
  }
}

#[derive(Debug)]
pub struct ManagedLocksHandler {
  service: Arc<ManagedLocksService>,
  operation: LockOperation,
}

impl CapabilityHandler for ManagedLocksHandler {
  fn descriptor(&self) -> CapabilityDescriptor {
    CapabilityDescriptor {
      capability: "locks".to_string(),
      operation: self.operation.name().to_string(),
      input_contract_version: 1,
      result_contract_version: 1,
      effect: match self.operation {
        LockOperation::AssertCurrent => CapabilityEffect::Read,
        LockOperation::Acquire | LockOperation::Release => CapabilityEffect::IdempotentWrite,
      },
      supports_cancellation: true,
      supports_provider_idempotency: false,
    }
  }

  fn validate_request(&self, request: &CapabilityCallRequest) -> Result<(), CapabilityFailure> {
    match self.operation {
      LockOperation::Acquire => {
        let input: AcquireInput = serde_json::from_value(request.input.clone()).map_err(|_| {
          invalid("services.locks.withLease() received an invalid acquire request.")
        })?;
        validate_acquire_input(request, &input)
      }
      LockOperation::AssertCurrent | LockOperation::Release => {
        let input: SessionInput = serde_json::from_value(request.input.clone())
          .map_err(|_| invalid("The lock session request is invalid."))?;
        if input.session_id.is_empty() || input.session_id.len() > 128 {
          return Err(invalid("The lock session ID is invalid."));
        }
        Ok(())
      }
    }
  }

  fn safe_request_metadata(
    &self,
    request: &CapabilityCallRequest,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    let mut metadata = Map::new();
    metadata.insert(
      "operation".to_string(),
      Value::String(self.operation.name().to_string()),
    );
    if let LockOperation::Acquire = self.operation {
      let input: AcquireInput = serde_json::from_value(request.input.clone())
        .map_err(|_| invalid("The lock acquire request is invalid."))?;
      metadata.insert("key".to_string(), Value::String(input.key));
      metadata.insert(
        "sessionRef".to_string(),
        Value::String(lock_session_ref(&format!(
          "lock_{}",
          &hex::encode(Sha256::digest(
            format!(
              "{}\0{}",
              request.invocation_id, request.identity.operation_key
            )
            .as_bytes()
          ))[..32]
        ))),
      );
    } else {
      let input: SessionInput = serde_json::from_value(request.input.clone())
        .map_err(|_| invalid("The lock session request is invalid."))?;
      metadata.insert(
        "sessionRef".to_string(),
        Value::String(lock_session_ref(&input.session_id)),
      );
    }
    Ok(metadata)
  }

  fn safe_result_metadata(&self, result: &Value) -> Map<String, Value> {
    let mut metadata = Map::new();
    let Some(object) = result.as_object() else {
      return metadata;
    };
    if let Some(session_id) = object.get("sessionId").and_then(Value::as_str) {
      metadata.insert(
        "sessionRef".to_string(),
        Value::String(lock_session_ref(session_id)),
      );
    }
    for field in ["acquiredAt", "releasedAt"] {
      if let Some(value) = object.get(field).and_then(Value::as_str) {
        metadata.insert(field.to_string(), Value::String(value.to_string()));
      }
    }
    if let Some(value) = object.get("fencingToken").and_then(Value::as_str) {
      metadata.insert("fence".to_string(), Value::String(value.to_string()));
    }
    metadata
  }

  fn execute(
    &self,
    _input: Value,
    _cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    Box::pin(async { Err(unavailable()) })
  }

  fn execute_request_scoped(
    &self,
    request: &CapabilityCallRequest,
    _workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    let service = Arc::clone(&self.service);
    let request = request.clone();
    let operation = self.operation;
    Box::pin(async move {
      match operation {
        LockOperation::Acquire => {
          let input = serde_json::from_value(request.input.clone())
            .map_err(|_| invalid("The lock acquire request is invalid."))?;
          service.acquire(request, input, cancellation).await
        }
        LockOperation::AssertCurrent => {
          let input = serde_json::from_value(request.input.clone())
            .map_err(|_| invalid("The lock assertion request is invalid."))?;
          service.assert_current(&request, input)
        }
        LockOperation::Release => {
          let input = serde_json::from_value(request.input.clone())
            .map_err(|_| invalid("The lock release request is invalid."))?;
          service.release(&request, input)
        }
      }
    })
  }
}

fn validate_acquire_input(
  request: &CapabilityCallRequest,
  input: &AcquireInput,
) -> Result<(), CapabilityFailure> {
  if input.key.is_empty()
    || input.key.len() > 256
    || input.key.trim() != input.key
    || input.key.chars().any(char::is_control)
    || input.key.split('/').any(str::is_empty)
    || !(1_000..=604_800_000).contains(&input.acquire_timeout_ms)
    || !(5_000..=600_000).contains(&input.lease_ms)
    || input.operation_identity != request.identity.operation_name
  {
    return Err(invalid(
      "services.locks.withLease() requires a valid key, acquireTimeout from 1s to 7d, lease from 5s to 10m, and stable operation identity.",
    ));
  }
  Ok(())
}

fn lock_session_ref(session_id: &str) -> String {
  format!(
    "sha256:{}",
    hex::encode(Sha256::digest(session_id.as_bytes()))
  )
}

fn map_store_error(message: &str) -> CapabilityFailure {
  if message.contains("inside a markup <lock>") {
    return failure(
      CapabilityFailureKind::ServiceRejected,
      "WOML_LOCK_NESTING_UNSUPPORTED",
      "services.locks.withLease() cannot be nested inside a markup <lock> scope.",
      false,
    );
  }
  if message.contains("identity is already bound") {
    return failure(
      CapabilityFailureKind::ServiceRejected,
      "WOML_LOCK_IDEMPOTENCY_CONFLICT",
      "The stable lease operation name was reused with different input.",
      false,
    );
  }
  unavailable()
}

fn invalid(message: &str) -> CapabilityFailure {
  failure(
    CapabilityFailureKind::InvalidInput,
    "WOML_LOCK_INVALID_REQUEST",
    message,
    false,
  )
}

fn stale() -> CapabilityFailure {
  failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_LOCK_STALE_OWNER",
    "The lease is no longer current. Stop the protected operation and inspect the run before retrying.",
    false,
  )
}

fn cancelled() -> CapabilityFailure {
  failure(
    CapabilityFailureKind::Cancelled,
    "WOML_LOCK_ACQUISITION_CANCELLED",
    "The lock acquisition was cancelled safely.",
    false,
  )
}

fn unavailable() -> CapabilityFailure {
  failure(
    CapabilityFailureKind::TransportFailed,
    "WOML_LOCK_AUTHORITY_UNAVAILABLE",
    "The durable lock authority is unavailable. Inspect the runtime and retry only after its state store is healthy.",
    true,
  )
}

fn failure(
  kind: CapabilityFailureKind,
  code: &str,
  message: &str,
  retryable: bool,
) -> CapabilityFailure {
  CapabilityFailure {
    kind,
    code: code.to_string(),
    message: message.to_string(),
    retryable,
    ambiguous: false,
    details: None,
  }
}
