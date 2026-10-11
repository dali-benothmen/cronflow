//! Durable Artifacts v1 metadata authority and local content-addressed backend.

// CapabilityFailure is the frozen cross-capability error envelope. Boxing it
// only in this module would complicate every CapabilityHandler boundary.
#![allow(clippy::result_large_err)]

use std::{
  collections::BTreeMap,
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  path::{Component, Path, PathBuf},
  sync::{Arc, Mutex},
  time::Duration,
};

use chrono::{SecondsFormat, Utc};
use futures_util::future::BoxFuture;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use serde_json_canonicalizer::to_vec as canonical_json;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::{
  capability::CapabilityIdentityMode, CapabilityCallRequest, CapabilityCancellationToken,
  CapabilityDescriptor, CapabilityEffect, CapabilityFailure, CapabilityFailureKind,
  CapabilityHandler, DurableEventStore,
};

pub const ARTIFACTS_CONTRACT: &str = "woml.artifacts";
pub const ARTIFACTS_CONTRACT_VERSION: u32 = 1;
pub const ARTIFACT_POLICY_CONTRACT: &str = "woml.artifact-policy";
pub const ARTIFACT_POLICY_CONTRACT_VERSION: u32 = 1;
const ARTIFACT_REFERENCE_CONTRACT: &str = "woml.artifact-reference";
const ARTIFACT_REFERENCE_CONTRACT_VERSION: u32 = 1;
const ARTIFACT_STORE_SCHEMA_VERSION: u32 = 1;
const ARTIFACT_BUSY_TIMEOUT_MS: u64 = 5_000;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 1_099_511_627_776;
const MAX_RUN_BYTES: u64 = 4_398_046_511_104;
const MAX_STORE_BYTES: u64 = 1_125_899_906_842_624;
const MAX_TRANSFER_TIMEOUT_MS: u64 = 86_400_000;
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactRuntimeMode {
  Foreground,
  Background,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactPolicyV1 {
  pub contract: String,
  pub contract_version: u32,
  pub enabled: bool,
  pub modes: ArtifactPolicyModes,
  pub project_root: String,
  pub read_roots: Vec<String>,
  pub materialization: ArtifactMaterializationPolicy,
  pub limits: ArtifactLimits,
  pub retention: ArtifactRetentionPolicy,
  pub secret_scanning: ArtifactSecretScanningPolicy,
  pub backend: ArtifactBackendPolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPolicyModes {
  pub foreground: bool,
  pub background: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactMaterializationPolicy {
  pub directory: String,
  pub cleanup_on_start: bool,
  pub max_bytes_per_attempt: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactLimits {
  pub max_artifact_bytes: u64,
  pub max_run_bytes: u64,
  pub max_store_bytes: u64,
  pub max_concurrent_transfers: usize,
  pub transfer_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRetentionPolicy {
  pub default_ms: u64,
  pub minimum_ms: u64,
  pub maximum_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownSecretAction {
  Reject,
  Quarantine,
  Allow,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactSecretScanningPolicy {
  pub known_secret_action: KnownSecretAction,
  pub minimum_secret_bytes: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ArtifactBackendPolicy {
  Local {
    contract: String,
    #[serde(rename = "contractVersion")]
    contract_version: u32,
    directory: String,
  },
  S3 {
    contract: String,
    #[serde(rename = "contractVersion")]
    contract_version: u32,
    bucket: String,
    region: String,
    prefix: String,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(rename = "forcePathStyle")]
    force_path_style: bool,
    authentication: Box<Value>,
    #[serde(rename = "serverSideEncryption")]
    server_side_encryption: Box<Value>,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactReferenceV1 {
  pub contract: String,
  pub contract_version: u32,
  pub id: String,
  pub name: String,
  pub size: u64,
  pub sha256: String,
  pub content_type: String,
}

#[derive(Debug, Error)]
pub enum ArtifactPolicyError {
  #[error("WOML_ARTIFACT_SERVICE_DISABLED: durable artifacts are disabled by runtime policy. Corrective action: provide a reviewed Artifact Policy v1 document")]
  Disabled,
  #[error("WOML_ARTIFACT_SERVICE_DISABLED: durable artifacts are not allowed in {0} mode. Corrective action: enable that mode in Artifact Policy v1")]
  ModeDenied(&'static str),
  #[error("WOML_ARTIFACT_POLICY_INVALID: {0}. Corrective action: repair Artifact Policy v1 and run WOML again")]
  Invalid(String),
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedArtifactPolicy {
  project_root: PathBuf,
  read_roots: Vec<PathBuf>,
  backend_directory: PathBuf,
  materialization_directory: PathBuf,
  max_materialization_bytes_per_attempt: u64,
  limits: ArtifactLimits,
  retention: ArtifactRetentionPolicy,
  secret_action: KnownSecretAction,
  minimum_secret_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactRequestV1 {
  contract: String,
  contract_version: u32,
  kind: String,
  operation: String,
  input: ArtifactPutInputV1,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactPutInputV1 {
  source_path: String,
  name: String,
  content_type: String,
  retention_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactReferenceRequestV1 {
  contract: String,
  contract_version: u32,
  kind: String,
  operation: String,
  input: ArtifactReferenceInputV1,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactReferenceInputV1 {
  artifact: ArtifactReferenceV1,
}

#[derive(Debug, Clone)]
struct AuthorizedArtifact {
  reference: ArtifactReferenceV1,
  created_at: String,
  expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceIdentity {
  length: u64,
  modified: Option<std::time::SystemTime>,
  #[cfg(unix)]
  device: u64,
  #[cfg(unix)]
  inode: u64,
}

#[derive(Debug, Clone)]
struct StoredSettlement {
  status: String,
  input_digest: String,
  artifact: Option<ArtifactReferenceV1>,
}

/// Rust-owned backend boundary. Logical identity and authorization never live
/// in implementations of this trait; they remain in SQLite.
trait ArtifactBackend: Send + Sync {
  fn begin_stage(&self) -> Result<ArtifactStage, CapabilityFailure>;
  fn publish(&self, stage: ArtifactStage, sha256: &str, size: u64)
    -> Result<(), CapabilityFailure>;
  fn cleanup_startup(&self, connection: &Connection) -> Result<(), CapabilityFailure>;
  fn verify(&self, sha256: &str, size: u64) -> Result<PathBuf, CapabilityFailure>;
}

#[derive(Debug)]
struct LocalArtifactBackend {
  root: PathBuf,
  staging: PathBuf,
  blobs: PathBuf,
}

#[derive(Debug)]
struct ArtifactStage {
  path: PathBuf,
  file: Option<File>,
}

impl Drop for ArtifactStage {
  fn drop(&mut self) {
    self.file.take();
    let _ = fs::remove_file(&self.path);
  }
}

impl LocalArtifactBackend {
  fn open(path: &Path) -> Result<Self, CapabilityFailure> {
    ensure_private_directory(path)?;
    let root = fs::canonicalize(path).map_err(|_| backend_unavailable())?;
    let staging = root.join("staging");
    let blobs = root.join("blobs").join("sha256");
    ensure_private_directory(&staging)?;
    ensure_private_directory(&root.join("blobs"))?;
    ensure_private_directory(&blobs)?;
    Ok(Self {
      root,
      staging,
      blobs,
    })
  }

  fn blob_path(&self, digest: &str) -> PathBuf {
    self.blobs.join(&digest[..2]).join(&digest[2..])
  }

  fn verify_blob(
    &self,
    path: &Path,
    expected_digest: &str,
    expected_size: u64,
  ) -> Result<(), CapabilityFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| backend_unavailable())?;
    if metadata.file_type().is_symlink()
      || !metadata.is_file()
      || is_windows_reparse(&metadata)
      || metadata.len() != expected_size
    {
      return Err(artifact_corrupt());
    }
    let mut file = File::open(path).map_err(|_| backend_unavailable())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    let mut size = 0_u64;
    loop {
      let read = file.read(&mut buffer).map_err(|_| artifact_corrupt())?;
      if read == 0 {
        break;
      }
      size = size.saturating_add(read as u64);
      digest.update(&buffer[..read]);
    }
    if size != expected_size || hex::encode(digest.finalize()) != expected_digest {
      return Err(artifact_corrupt());
    }
    Ok(())
  }

  fn bind_store(&self, connection: &Connection, store_id: &str) -> Result<(), CapabilityFailure> {
    let marker = self.root.join("store-id");
    match fs::read_to_string(&marker) {
      Ok(value) => {
        if value != format!("woml.artifact-store/v1\n{store_id}\n") {
          return Err(backend_unavailable());
        }
        return Ok(());
      }
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
      Err(_) => return Err(backend_unavailable()),
    }
    // A pre-marker DA3 store may be adopted only when every published blob is
    // already reachable from this SQLite authority. This prevents a newly
    // selected state database from claiming or collecting another store.
    for prefix in fs::read_dir(&self.blobs).map_err(|_| backend_unavailable())? {
      let prefix = prefix.map_err(|_| backend_unavailable())?;
      if !prefix
        .file_type()
        .map_err(|_| backend_unavailable())?
        .is_dir()
      {
        return Err(backend_unavailable());
      }
      for entry in fs::read_dir(prefix.path()).map_err(|_| backend_unavailable())? {
        let entry = entry.map_err(|_| backend_unavailable())?;
        if !entry
          .file_type()
          .map_err(|_| backend_unavailable())?
          .is_file()
        {
          return Err(backend_unavailable());
        }
        let digest = format!(
          "{}{}",
          prefix.file_name().to_string_lossy(),
          entry.file_name().to_string_lossy()
        );
        let reachable: bool = connection
          .query_row(
            "SELECT EXISTS(SELECT 1 FROM woml_artifact_blobs WHERE sha256 = ?1)",
            [&digest],
            |row| row.get(0),
          )
          .map_err(|_| backend_unavailable())?;
        if !reachable {
          return Err(backend_unavailable());
        }
      }
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let mut file = options.open(&marker).map_err(|_| backend_unavailable())?;
    file
      .write_all(format!("woml.artifact-store/v1\n{store_id}\n").as_bytes())
      .map_err(|_| backend_unavailable())?;
    file.sync_all().map_err(|_| backend_unavailable())?;
    sync_directory(&self.root)
  }
}

impl ArtifactBackend for LocalArtifactBackend {
  fn begin_stage(&self) -> Result<ArtifactStage, CapabilityFailure> {
    let path = self
      .staging
      .join(format!("{}.part", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true).read(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let file = options.open(&path).map_err(|_| backend_unavailable())?;
    Ok(ArtifactStage {
      path,
      file: Some(file),
    })
  }

  fn publish(
    &self,
    mut stage: ArtifactStage,
    sha256: &str,
    size: u64,
  ) -> Result<(), CapabilityFailure> {
    let file = stage.file.take().ok_or_else(backend_unavailable)?;
    file.sync_all().map_err(|_| backend_unavailable())?;
    drop(file);
    let directory = self.blobs.join(&sha256[..2]);
    ensure_private_directory(&directory)?;
    let target = self.blob_path(sha256);
    if target.exists() {
      self.verify_blob(&target, sha256, size)?;
      fs::remove_file(&stage.path).map_err(|_| backend_unavailable())?;
      sync_directory(&directory)?;
      return Ok(());
    }
    match fs::hard_link(&stage.path, &target) {
      Ok(()) => {}
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
      Err(_) => return Err(backend_unavailable()),
    }
    self.verify_blob(&target, sha256, size)?;
    fs::remove_file(&stage.path).map_err(|_| backend_unavailable())?;
    sync_directory(&directory)?;
    sync_directory(&self.staging)?;
    Ok(())
  }

  fn cleanup_startup(&self, connection: &Connection) -> Result<(), CapabilityFailure> {
    for entry in fs::read_dir(&self.staging).map_err(|_| backend_unavailable())? {
      let path = entry.map_err(|_| backend_unavailable())?.path();
      let metadata = fs::symlink_metadata(&path).map_err(|_| backend_unavailable())?;
      if metadata.is_file() && !metadata.file_type().is_symlink() {
        fs::remove_file(path).map_err(|_| backend_unavailable())?;
      }
    }
    // Published-but-uncommitted blobs are unreachable and safe to collect.
    for prefix in fs::read_dir(&self.blobs).map_err(|_| backend_unavailable())? {
      let prefix = prefix.map_err(|_| backend_unavailable())?;
      if !prefix
        .file_type()
        .map_err(|_| backend_unavailable())?
        .is_dir()
      {
        continue;
      }
      for entry in fs::read_dir(prefix.path()).map_err(|_| backend_unavailable())? {
        let entry = entry.map_err(|_| backend_unavailable())?;
        let digest = format!(
          "{}{}",
          prefix.file_name().to_string_lossy(),
          entry.file_name().to_string_lossy()
        );
        let reachable: bool = connection
          .query_row(
            "SELECT EXISTS(SELECT 1 FROM woml_artifact_blobs WHERE sha256 = ?1)",
            [&digest],
            |row| row.get(0),
          )
          .map_err(|_| backend_unavailable())?;
        if !reachable {
          let metadata = entry.metadata().map_err(|_| backend_unavailable())?;
          if metadata.is_file() {
            fs::remove_file(entry.path()).map_err(|_| backend_unavailable())?;
          }
        }
      }
    }
    sync_directory(&self.root)
  }

  fn verify(&self, sha256: &str, size: u64) -> Result<PathBuf, CapabilityFailure> {
    let path = self.blob_path(sha256);
    self.verify_blob(&path, sha256, size)?;
    Ok(path)
  }
}

#[derive(Default)]
pub struct ManagedArtifactStore {
  store: Mutex<Option<Arc<ArtifactStore>>>,
}

impl std::fmt::Debug for ManagedArtifactStore {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ManagedArtifactStore")
      .field(
        "configured",
        &self.store.lock().map(|v| v.is_some()).unwrap_or(false),
      )
      .finish()
  }
}

impl ManagedArtifactStore {
  pub(crate) fn configure_for_state(
    &self,
    state_path: &Path,
    policy: Arc<PreparedArtifactPolicy>,
  ) -> Result<(), CapabilityFailure> {
    let store = Arc::new(ArtifactStore::open(state_path, policy)?);
    let mut configured = self.store.lock().map_err(|_| backend_unavailable())?;
    if let Some(existing) = configured.as_ref() {
      if existing.state_path == store.state_path && existing.backend.root == store.backend.root {
        return Ok(());
      }
      return Err(artifact_failure(
        CapabilityFailureKind::ServiceRejected,
        "WOML_ARTIFACT_BACKEND_UNAVAILABLE",
        "The artifact backend is already bound to another durable state store.",
        false,
        false,
      ));
    }
    *configured = Some(store);
    Ok(())
  }

  fn configured(&self) -> Result<Arc<ArtifactStore>, CapabilityFailure> {
    self
      .store
      .lock()
      .map_err(|_| backend_unavailable())?
      .clone()
      .ok_or_else(backend_unavailable)
  }

  pub(crate) fn cleanup_invocation(&self, invocation_id: &str) {
    let Ok(store) = self.configured() else {
      return;
    };
    let _ = store.cleanup_materialization(invocation_id);
  }
}

struct ArtifactStore {
  state_path: PathBuf,
  policy: Arc<PreparedArtifactPolicy>,
  backend: LocalArtifactBackend,
  materializations: PathBuf,
}

impl std::fmt::Debug for ArtifactStore {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ArtifactStore")
      .field("state_path", &self.state_path)
      .field("backend", &self.backend.root)
      .finish()
  }
}

impl ArtifactStore {
  fn open(
    state_path: &Path,
    policy: Arc<PreparedArtifactPolicy>,
  ) -> Result<Self, CapabilityFailure> {
    DurableEventStore::open(state_path).map_err(|_| backend_unavailable())?;
    let state_path = fs::canonicalize(state_path).map_err(|_| backend_unavailable())?;
    harden_file_permissions(&state_path)?;
    let backend = LocalArtifactBackend::open(&policy.backend_directory)?;
    ensure_private_directory(&policy.materialization_directory)?;
    let materializations = fs::canonicalize(&policy.materialization_directory)
      .map_err(|_| materialization_unavailable())?;
    cleanup_directory_contents(&materializations)?;
    let store = Self {
      state_path,
      policy,
      backend,
      materializations,
    };
    let connection = store.connection()?;
    initialize_schema(&connection)?;
    validate_schema(&connection)?;
    let store_id = ensure_store_identity(&connection)?;
    store.backend.bind_store(&connection, &store_id)?;
    store.backend.cleanup_startup(&connection)?;
    Ok(store)
  }

  fn invocation_materialization_directory(&self, invocation_id: &str) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(b"woml.artifact-materialization\0v1\0");
    digest.update(invocation_id.as_bytes());
    self.materializations.join(hex::encode(digest.finalize()))
  }

  fn cleanup_materialization(&self, invocation_id: &str) -> Result<(), CapabilityFailure> {
    let directory = self.invocation_materialization_directory(invocation_id);
    match fs::symlink_metadata(&directory) {
      Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
        fs::remove_dir_all(directory).map_err(|_| materialization_unavailable())
      }
      Ok(_) => Err(materialization_unavailable()),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(_) => Err(materialization_unavailable()),
    }
  }

  fn connection(&self) -> Result<Connection, CapabilityFailure> {
    let connection = Connection::open_with_flags(
      &self.state_path,
      OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )
    .map_err(|_| backend_unavailable())?;
    connection
      .busy_timeout(Duration::from_millis(ARTIFACT_BUSY_TIMEOUT_MS))
      .map_err(|_| backend_unavailable())?;
    connection
      .execute_batch("PRAGMA foreign_keys = ON;")
      .map_err(|_| backend_unavailable())?;
    Ok(connection)
  }

  fn put(
    &self,
    request: &CapabilityCallRequest,
    workflow_id: &str,
    parsed: &ArtifactRequestV1,
    cancellation: &CapabilityCancellationToken,
    known_secrets: &[Vec<u8>],
  ) -> Result<ArtifactReferenceV1, CapabilityFailure> {
    let input_digest = canonical_digest(b"woml.artifact-input\0v1\0", &request.input)?;
    let started_at = timestamp();
    let mut connection = self.connection()?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(|_| backend_unavailable())?;
    validate_schema(&transaction)?;
    if let Some(settlement) = read_settlement(&transaction, &request.identity.operation_key)? {
      validate_settlement_identity(&settlement, &input_digest)?;
      transaction.commit().map_err(|_| backend_unavailable())?;
      return match settlement.status.as_str() {
        "succeeded" => {
          let artifact = settlement.artifact.ok_or_else(artifact_corrupt)?;
          let (_, digest, size) =
            self.read_source(&parsed.input.source_path, cancellation, known_secrets, None)?;
          if digest != artifact.sha256 || size != artifact.size {
            Err(operation_identity_conflict())
          } else {
            Ok(artifact)
          }
        }
        _ => Err(artifact_interrupted()),
      };
    }
    transaction
      .execute(
        "INSERT INTO woml_artifact_transfers(
           operation_key, run_id, node_id, attempt_number, operation, input_digest,
           status, started_at
         ) VALUES (?1, ?2, ?3, ?4, 'put', ?5, 'started', ?6)",
        params![
          request.identity.operation_key,
          request.run_id,
          request.node_id,
          request.attempt_number,
          input_digest,
          started_at,
        ],
      )
      .map_err(|_| backend_unavailable())?;
    transaction.commit().map_err(|_| backend_unavailable())?;

    let outcome = self.put_started(request, workflow_id, parsed, cancellation, known_secrets);
    if let Err(failure) = &outcome {
      self.settle_failure(&request.identity.operation_key, failure)?;
    }
    outcome
  }

  fn put_started(
    &self,
    request: &CapabilityCallRequest,
    workflow_id: &str,
    parsed: &ArtifactRequestV1,
    cancellation: &CapabilityCancellationToken,
    known_secrets: &[Vec<u8>],
  ) -> Result<ArtifactReferenceV1, CapabilityFailure> {
    let mut stage = self.backend.begin_stage()?;
    let (source_identity, sha256, size) = self.read_source(
      &parsed.input.source_path,
      cancellation,
      known_secrets,
      stage.file.as_mut(),
    )?;
    let _ = source_identity;
    if cancellation.is_cancelled() {
      return Err(artifact_cancelled());
    }
    let artifact = ArtifactReferenceV1 {
      contract: ARTIFACT_REFERENCE_CONTRACT.to_string(),
      contract_version: ARTIFACT_REFERENCE_CONTRACT_VERSION,
      id: new_artifact_id()?,
      name: parsed.input.name.clone(),
      size,
      sha256: sha256.clone(),
      content_type: parsed.input.content_type.clone(),
    };
    let created_at = timestamp();
    let expires_at = Utc::now()
      .checked_add_signed(chrono::Duration::milliseconds(
        parsed.input.retention_ms as i64,
      ))
      .ok_or_else(retention_invalid)?
      .to_rfc3339_opts(SecondsFormat::Millis, true);
    let result = json!({
      "contract": ARTIFACTS_CONTRACT,
      "contractVersion": ARTIFACTS_CONTRACT_VERSION,
      "kind": "result",
      "operation": "put",
      "data": serde_json::to_value(&artifact).map_err(|_| artifact_corrupt())?
    });
    let result_digest = canonical_digest(b"woml.artifact-result\0v1\0", &result)?;

    let mut connection = self.connection()?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(|_| backend_unavailable())?;
    validate_schema(&transaction)?;
    enforce_quotas(
      &transaction,
      &request.run_id,
      &sha256,
      size,
      &self.policy.limits,
    )?;
    self.backend.publish(stage, &sha256, size)?;
    transaction
      .execute(
        "INSERT OR IGNORE INTO woml_artifact_blobs(sha256, size, created_at)
         VALUES (?1, ?2, ?3)",
        params![sha256, size, created_at],
      )
      .map_err(|_| backend_unavailable())?;
    transaction
      .execute(
        "INSERT INTO woml_artifacts(
           artifact_id, name, size, sha256, content_type, status, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'available', ?6)",
        params![
          artifact.id,
          artifact.name,
          artifact.size,
          artifact.sha256,
          artifact.content_type,
          created_at,
        ],
      )
      .map_err(|_| backend_unavailable())?;
    transaction
      .execute(
        "INSERT INTO woml_artifact_provenance(
           artifact_id, workflow_id, run_id, node_id, attempt_number, operation_key
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
          artifact.id,
          workflow_id,
          request.run_id,
          request.node_id,
          request.attempt_number,
          request.identity.operation_key,
        ],
      )
      .map_err(|_| backend_unavailable())?;
    transaction
      .execute(
        "INSERT INTO woml_artifact_access(artifact_id, run_id, granted_at)
         VALUES (?1, ?2, ?3)",
        params![artifact.id, request.run_id, created_at],
      )
      .map_err(|_| backend_unavailable())?;
    transaction
      .execute(
        "INSERT INTO woml_artifact_retention(
           artifact_id, requested_ms, effective_ms, expires_at
         ) VALUES (?1, ?2, ?2, ?3)",
        params![artifact.id, parsed.input.retention_ms, expires_at],
      )
      .map_err(|_| backend_unavailable())?;
    transaction
      .execute(
        "UPDATE woml_artifact_transfers
         SET status = 'succeeded', artifact_id = ?2, content_digest = ?3,
             size = ?4, result_digest = ?5, settled_at = ?6
         WHERE operation_key = ?1 AND status = 'started'",
        params![
          request.identity.operation_key,
          artifact.id,
          format!("sha256:{sha256}"),
          size,
          result_digest,
          timestamp(),
        ],
      )
      .map_err(|_| backend_unavailable())?;
    transaction.commit().map_err(|_| backend_unavailable())?;
    Ok(artifact)
  }

  fn settle_failure(
    &self,
    operation_key: &str,
    failure: &CapabilityFailure,
  ) -> Result<(), CapabilityFailure> {
    let status = if failure.kind == CapabilityFailureKind::Cancelled {
      "cancelled"
    } else if failure.code == "WOML_ARTIFACT_INTERRUPTED" || failure.ambiguous {
      "interrupted"
    } else {
      "failed"
    };
    let connection = self.connection()?;
    connection
      .execute(
        "UPDATE woml_artifact_transfers
         SET status = ?2, error_code = ?3, ambiguous = ?4, settled_at = ?5
         WHERE operation_key = ?1 AND status = 'started'",
        params![
          operation_key,
          status,
          failure.code,
          i64::from(failure.ambiguous),
          timestamp(),
        ],
      )
      .map_err(|_| backend_unavailable())?;
    Ok(())
  }

  fn head(
    &self,
    request: &CapabilityCallRequest,
    parsed: &ArtifactReferenceRequestV1,
  ) -> Result<Value, CapabilityFailure> {
    let artifact = self.authorize_reference(&request.run_id, &parsed.input.artifact)?;
    self
      .backend
      .verify(&artifact.reference.sha256, artifact.reference.size)?;
    Ok(json!({
      "contract": ARTIFACTS_CONTRACT,
      "contractVersion": ARTIFACTS_CONTRACT_VERSION,
      "kind": "result",
      "operation": "head",
      "data": {
        "artifact": artifact.reference,
        "createdAt": artifact.created_at,
        "expiresAt": artifact.expires_at,
        "integrity": "verified"
      }
    }))
  }

  fn materialize(
    &self,
    request: &CapabilityCallRequest,
    parsed: &ArtifactReferenceRequestV1,
    cancellation: &CapabilityCancellationToken,
  ) -> Result<Value, CapabilityFailure> {
    let artifact = self.authorize_reference(&request.run_id, &parsed.input.artifact)?;
    if artifact.reference.size > self.policy.max_materialization_bytes_per_attempt {
      return Err(artifact_too_large());
    }
    let source_path = self
      .backend
      .verify(&artifact.reference.sha256, artifact.reference.size)?;
    let directory = self.invocation_materialization_directory(&request.invocation_id);
    ensure_private_directory(&directory).map_err(|_| materialization_unavailable())?;
    let existing_bytes = directory_bytes(&directory)?;
    let final_path = directory.join(&artifact.reference.id);
    if final_path.exists() {
      verify_materialized_file(
        &final_path,
        &artifact.reference.sha256,
        artifact.reference.size,
      )?;
    } else {
      if existing_bytes.saturating_add(artifact.reference.size)
        > self.policy.max_materialization_bytes_per_attempt
      {
        return Err(artifact_too_large());
      }
      stream_materialization(
        &source_path,
        &final_path,
        &artifact.reference.sha256,
        artifact.reference.size,
        cancellation,
      )?;
    }
    let path = final_path
      .to_str()
      .filter(|path| path.len() <= 4_096)
      .ok_or_else(materialization_unavailable)?;
    Ok(json!({
      "contract": ARTIFACTS_CONTRACT,
      "contractVersion": ARTIFACTS_CONTRACT_VERSION,
      "kind": "result",
      "operation": "materialize",
      "data": {
        "artifact": artifact.reference,
        "path": path
      }
    }))
  }

  fn authorize_reference(
    &self,
    run_id: &str,
    supplied: &ArtifactReferenceV1,
  ) -> Result<AuthorizedArtifact, CapabilityFailure> {
    validate_reference(supplied)?;
    let connection = self.connection()?;
    validate_schema(&connection)?;
    let stored: Option<(String, String, u64, String, String, String, String, String)> = connection
      .query_row(
        "SELECT a.artifact_id, a.name, a.size, a.sha256, a.content_type,
                a.status, a.created_at, r.expires_at
         FROM woml_artifacts a
         JOIN woml_artifact_retention r ON r.artifact_id = a.artifact_id
         WHERE a.artifact_id = ?1",
        [&supplied.id],
        |row| {
          Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
          ))
        },
      )
      .optional()
      .map_err(|_| backend_unavailable())?;
    let Some((id, name, size, sha256, content_type, status, created_at, expires_at)) = stored
    else {
      return Err(artifact_not_found());
    };
    let authorized: bool = connection
      .query_row(
        "SELECT EXISTS(
           SELECT 1 FROM woml_artifact_access
           WHERE artifact_id = ?1 AND run_id = ?2 AND revoked_at IS NULL
         )",
        params![id, run_id],
        |row| row.get(0),
      )
      .map_err(|_| backend_unavailable())?;
    if !authorized {
      return Err(artifact_access_denied());
    }
    match status.as_str() {
      "available" => {}
      "expired" => return Err(artifact_expired()),
      "quarantined" => return Err(artifact_quarantined()),
      "corrupt" => return Err(artifact_corrupt()),
      "deleted" => return Err(artifact_not_found()),
      _ => return Err(artifact_corrupt()),
    }
    let expires =
      chrono::DateTime::parse_from_rfc3339(&expires_at).map_err(|_| artifact_corrupt())?;
    if expires <= Utc::now() {
      return Err(artifact_expired());
    }
    let reference = ArtifactReferenceV1 {
      contract: ARTIFACT_REFERENCE_CONTRACT.to_string(),
      contract_version: ARTIFACT_REFERENCE_CONTRACT_VERSION,
      id,
      name,
      size,
      sha256,
      content_type,
    };
    if &reference != supplied {
      return Err(artifact_access_denied());
    }
    Ok(AuthorizedArtifact {
      reference,
      created_at,
      expires_at,
    })
  }

  fn read_source(
    &self,
    authored: &str,
    cancellation: &CapabilityCancellationToken,
    known_secrets: &[Vec<u8>],
    mut destination: Option<&mut File>,
  ) -> Result<(SourceIdentity, String, u64), CapabilityFailure> {
    let path = authorize_source(&self.policy, authored)?;
    let mut source = File::open(&path).map_err(|_| source_denied())?;
    let before = source_identity(&source)?;
    if before.length > self.policy.limits.max_artifact_bytes {
      return Err(artifact_too_large());
    }
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    let overlap_size = known_secrets
      .iter()
      .map(Vec::len)
      .max()
      .unwrap_or(1)
      .saturating_sub(1);
    let mut overlap = Vec::new();
    loop {
      if cancellation.is_cancelled() {
        return Err(artifact_cancelled());
      }
      let read = source.read(&mut buffer).map_err(|_| source_changed())?;
      if read == 0 {
        break;
      }
      size = size.saturating_add(read as u64);
      if size > self.policy.limits.max_artifact_bytes {
        return Err(artifact_too_large());
      }
      let chunk = &buffer[..read];
      if self.policy.secret_action != KnownSecretAction::Allow && !known_secrets.is_empty() {
        let mut scan = overlap.clone();
        scan.extend_from_slice(chunk);
        if known_secrets
          .iter()
          .any(|secret| contains_bytes(&scan, secret))
        {
          return Err(artifact_quarantined());
        }
        let keep = scan.len().min(overlap_size);
        overlap = scan[scan.len() - keep..].to_vec();
      }
      digest.update(chunk);
      if let Some(file) = destination.as_deref_mut() {
        file.write_all(chunk).map_err(|_| backend_unavailable())?;
      }
    }
    let after = source_identity(&source)?;
    if before != after || size != before.length {
      return Err(source_changed());
    }
    reject_link_components(&self.policy.project_root, &path)?;
    if fs::canonicalize(&path).map_err(|_| source_changed())? != path {
      return Err(source_changed());
    }
    Ok((before, hex::encode(digest.finalize()), size))
  }
}

#[derive(Debug)]
pub struct ManagedArtifactPutHandler {
  store: Arc<ManagedArtifactStore>,
  policy: Arc<PreparedArtifactPolicy>,
  concurrency: Arc<Semaphore>,
  known_secrets: Arc<Vec<Vec<u8>>>,
}

impl ManagedArtifactPutHandler {
  pub fn new(
    store: Arc<ManagedArtifactStore>,
    policy: ArtifactPolicyV1,
    mode: ArtifactRuntimeMode,
    resolved_secrets: &BTreeMap<String, String>,
  ) -> Result<Self, ArtifactPolicyError> {
    let policy = Arc::new(PreparedArtifactPolicy::prepare(policy, mode)?);
    let concurrency = Arc::new(Semaphore::new(policy.limits.max_concurrent_transfers));
    let known_secrets = resolved_secrets
      .values()
      .map(|secret| secret.as_bytes().to_vec())
      .filter(|secret| secret.len() >= policy.minimum_secret_bytes)
      .collect();
    Ok(Self {
      store,
      policy,
      concurrency,
      known_secrets: Arc::new(known_secrets),
    })
  }

  pub(crate) fn policy(&self) -> Arc<PreparedArtifactPolicy> {
    Arc::clone(&self.policy)
  }

  pub fn head_handler(&self) -> ManagedArtifactReadHandler {
    ManagedArtifactReadHandler {
      store: Arc::clone(&self.store),
      policy: Arc::clone(&self.policy),
      concurrency: Arc::clone(&self.concurrency),
      operation: ArtifactReadOperation::Head,
    }
  }

  pub fn materialize_handler(&self) -> ManagedArtifactReadHandler {
    ManagedArtifactReadHandler {
      store: Arc::clone(&self.store),
      policy: Arc::clone(&self.policy),
      concurrency: Arc::clone(&self.concurrency),
      operation: ArtifactReadOperation::Materialize,
    }
  }
}

impl CapabilityHandler for ManagedArtifactPutHandler {
  fn descriptor(&self) -> CapabilityDescriptor {
    CapabilityDescriptor {
      capability: "artifacts".to_string(),
      operation: "put".to_string(),
      input_contract_version: ARTIFACTS_CONTRACT_VERSION,
      result_contract_version: ARTIFACTS_CONTRACT_VERSION,
      effect: CapabilityEffect::IdempotentWrite,
      supports_cancellation: true,
      supports_provider_idempotency: false,
    }
  }

  fn validate_request(&self, request: &CapabilityCallRequest) -> Result<(), CapabilityFailure> {
    let parsed = parse_put(&request.input, &self.policy)?;
    if request.identity.mode != CapabilityIdentityMode::Named
      || request.identity.operation_name != parsed.input.name
    {
      return Err(operation_identity_conflict());
    }
    Ok(())
  }

  fn safe_request_metadata(
    &self,
    _request: &CapabilityCallRequest,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    Ok(Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!("put")),
      ("outcome".to_string(), json!("started")),
    ]))
  }

  fn safe_result_metadata(&self, result: &Value) -> Map<String, Value> {
    let mut metadata = Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!("put")),
      ("outcome".to_string(), json!("succeeded")),
    ]);
    let result = result.get("data").unwrap_or(result);
    for (from, to) in [("id", "artifactId"), ("size", "size")] {
      if let Some(value) = result.get(from) {
        metadata.insert(to.to_string(), value.clone());
      }
    }
    if let Some(digest) = result.get("sha256").and_then(Value::as_str) {
      metadata.insert(
        "contentDigest".to_string(),
        json!(format!("sha256:{digest}")),
      );
    }
    metadata
  }

  fn safe_failure_metadata(&self, failure: &CapabilityFailure) -> Map<String, Value> {
    Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!("put")),
      (
        "outcome".to_string(),
        json!(if failure.kind == CapabilityFailureKind::Cancelled {
          "cancelled"
        } else if failure.code == "WOML_ARTIFACT_INTERRUPTED" {
          "interrupted"
        } else {
          "failed"
        }),
      ),
      ("errorCode".to_string(), json!(failure.code)),
      ("ambiguous".to_string(), json!(failure.ambiguous)),
    ])
  }

  fn execute(
    &self,
    _input: Value,
    _cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    Box::pin(async { Err(backend_unavailable()) })
  }

  fn execute_request_scoped(
    &self,
    request: &CapabilityCallRequest,
    workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    let request = request.clone();
    let parsed = match parse_put(&request.input, &self.policy) {
      Ok(parsed) => parsed,
      Err(error) => return Box::pin(async move { Err(error) }),
    };
    let store = match self.store.configured() {
      Ok(store) => store,
      Err(error) => return Box::pin(async move { Err(error) }),
    };
    let Some(workflow_scope) = workflow_scope else {
      return Box::pin(async { Err(backend_unavailable()) });
    };
    let concurrency = Arc::clone(&self.concurrency);
    let known_secrets = Arc::clone(&self.known_secrets);
    let timeout = Duration::from_millis(self.policy.limits.transfer_timeout_ms);
    Box::pin(async move {
      let permit = tokio::select! {
        permit = concurrency.acquire_owned() => permit.map_err(|_| backend_unavailable())?,
        _ = cancellation.cancelled() => return Err(artifact_cancelled()),
      };
      let task_cancellation = cancellation.clone();
      let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        store
          .put(
            &request,
            &workflow_scope,
            &parsed,
            &task_cancellation,
            &known_secrets,
          )
          .and_then(|reference| {
            Ok(json!({
              "contract": ARTIFACTS_CONTRACT,
              "contractVersion": ARTIFACTS_CONTRACT_VERSION,
              "kind": "result",
              "operation": "put",
              "data": serde_json::to_value(reference).map_err(|_| artifact_corrupt())?
            }))
          })
      });
      match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(artifact_failure(
          CapabilityFailureKind::HandlerCrashed,
          "WOML_ARTIFACT_BACKEND_UNAVAILABLE",
          "The artifact backend is unavailable.",
          true,
          true,
        )),
        Err(_) => {
          cancellation.cancel();
          Err(artifact_timed_out())
        }
      }
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactReadOperation {
  Head,
  Materialize,
}

impl ArtifactReadOperation {
  fn as_str(self) -> &'static str {
    match self {
      Self::Head => "head",
      Self::Materialize => "materialize",
    }
  }
}

#[derive(Debug)]
pub struct ManagedArtifactReadHandler {
  store: Arc<ManagedArtifactStore>,
  policy: Arc<PreparedArtifactPolicy>,
  concurrency: Arc<Semaphore>,
  operation: ArtifactReadOperation,
}

impl CapabilityHandler for ManagedArtifactReadHandler {
  fn descriptor(&self) -> CapabilityDescriptor {
    CapabilityDescriptor {
      capability: "artifacts".to_string(),
      operation: self.operation.as_str().to_string(),
      input_contract_version: ARTIFACTS_CONTRACT_VERSION,
      result_contract_version: ARTIFACTS_CONTRACT_VERSION,
      effect: CapabilityEffect::Read,
      supports_cancellation: true,
      supports_provider_idempotency: false,
    }
  }

  fn validate_request(&self, request: &CapabilityCallRequest) -> Result<(), CapabilityFailure> {
    parse_reference_request(&request.input, self.operation.as_str())?;
    if request.identity.mode != CapabilityIdentityMode::Automatic {
      return Err(artifact_access_denied());
    }
    Ok(())
  }

  fn safe_request_metadata(
    &self,
    _request: &CapabilityCallRequest,
  ) -> Result<Map<String, Value>, CapabilityFailure> {
    Ok(Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!(self.operation.as_str())),
      ("outcome".to_string(), json!("started")),
    ]))
  }

  fn safe_result_metadata(&self, result: &Value) -> Map<String, Value> {
    let mut metadata = Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!(self.operation.as_str())),
      ("outcome".to_string(), json!("succeeded")),
    ]);
    let artifact = result.get("data").and_then(|data| data.get("artifact"));
    if let Some(artifact) = artifact {
      for (from, to) in [("id", "artifactId"), ("size", "size")] {
        if let Some(value) = artifact.get(from) {
          metadata.insert(to.to_string(), value.clone());
        }
      }
      if let Some(digest) = artifact.get("sha256").and_then(Value::as_str) {
        metadata.insert(
          "contentDigest".to_string(),
          json!(format!("sha256:{digest}")),
        );
      }
    }
    metadata
  }

  fn safe_failure_metadata(&self, failure: &CapabilityFailure) -> Map<String, Value> {
    Map::from_iter([
      (
        "profile".to_string(),
        json!("woml.artifact-operation-metadata/v1"),
      ),
      ("operation".to_string(), json!(self.operation.as_str())),
      (
        "outcome".to_string(),
        json!(if failure.kind == CapabilityFailureKind::Cancelled {
          "cancelled"
        } else {
          "failed"
        }),
      ),
      ("errorCode".to_string(), json!(failure.code)),
      ("ambiguous".to_string(), json!(failure.ambiguous)),
    ])
  }

  fn execute(
    &self,
    _input: Value,
    _cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    Box::pin(async { Err(backend_unavailable()) })
  }

  fn execute_request_scoped(
    &self,
    request: &CapabilityCallRequest,
    _workflow_scope: Option<String>,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    let request = request.clone();
    let parsed = match parse_reference_request(&request.input, self.operation.as_str()) {
      Ok(parsed) => parsed,
      Err(error) => return Box::pin(async move { Err(error) }),
    };
    let store = match self.store.configured() {
      Ok(store) => store,
      Err(error) => return Box::pin(async move { Err(error) }),
    };
    let operation = self.operation;
    let concurrency = Arc::clone(&self.concurrency);
    let timeout = Duration::from_millis(self.policy.limits.transfer_timeout_ms);
    Box::pin(async move {
      let permit = tokio::select! {
        permit = concurrency.acquire_owned() => permit.map_err(|_| backend_unavailable())?,
        _ = cancellation.cancelled() => return Err(artifact_cancelled()),
      };
      let task_cancellation = cancellation.clone();
      let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        match operation {
          ArtifactReadOperation::Head => store.head(&request, &parsed),
          ArtifactReadOperation::Materialize => {
            store.materialize(&request, &parsed, &task_cancellation)
          }
        }
      });
      match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(artifact_failure(
          CapabilityFailureKind::HandlerCrashed,
          "WOML_ARTIFACT_BACKEND_UNAVAILABLE",
          "The artifact backend is unavailable.",
          true,
          operation == ArtifactReadOperation::Materialize,
        )),
        Err(_) => {
          cancellation.cancel();
          Err(artifact_timed_out())
        }
      }
    })
  }
}

impl PreparedArtifactPolicy {
  fn prepare(
    policy: ArtifactPolicyV1,
    mode: ArtifactRuntimeMode,
  ) -> Result<Self, ArtifactPolicyError> {
    if policy.contract != ARTIFACT_POLICY_CONTRACT
      || policy.contract_version != ARTIFACT_POLICY_CONTRACT_VERSION
    {
      return Err(ArtifactPolicyError::Invalid(
        "the document does not match Artifact Policy v1".to_string(),
      ));
    }
    if !policy.enabled {
      return Err(ArtifactPolicyError::Disabled);
    }
    let allowed = match mode {
      ArtifactRuntimeMode::Foreground => policy.modes.foreground,
      ArtifactRuntimeMode::Background => policy.modes.background,
    };
    if !allowed {
      return Err(ArtifactPolicyError::ModeDenied(match mode {
        ArtifactRuntimeMode::Foreground => "foreground",
        ArtifactRuntimeMode::Background => "background",
      }));
    }
    validate_policy_limits(&policy)?;
    let project_root = canonical_directory(Path::new(&policy.project_root), "project root")?;
    let mut read_roots = Vec::new();
    for root in &policy.read_roots {
      let candidate = Path::new(root);
      let path = if candidate.is_absolute() {
        candidate.to_path_buf()
      } else {
        project_root.join(candidate)
      };
      let canonical = canonical_directory(&path, "read root")?;
      if !canonical.starts_with(&project_root) {
        return Err(ArtifactPolicyError::Invalid(
          "every read root must remain beneath the project root".to_string(),
        ));
      }
      if !read_roots.contains(&canonical) {
        read_roots.push(canonical);
      }
    }
    let backend_directory = match &policy.backend {
      ArtifactBackendPolicy::Local {
        contract,
        contract_version,
        directory,
      } if contract == "woml.artifact-backend" && *contract_version == 1 => {
        let path = Path::new(directory);
        if path.is_absolute() {
          path.to_path_buf()
        } else {
          project_root.join(path)
        }
      }
      ArtifactBackendPolicy::Local { .. } => {
        return Err(ArtifactPolicyError::Invalid(
          "the backend does not match Artifact Backend v1".to_string(),
        ))
      }
      ArtifactBackendPolicy::S3 { .. } => {
        return Err(ArtifactPolicyError::Invalid(
          "the S3-compatible backend is reserved but not executable in DA3".to_string(),
        ))
      }
    };
    let materialization_path = Path::new(&policy.materialization.directory);
    let materialization_directory = if materialization_path.is_absolute() {
      materialization_path.to_path_buf()
    } else {
      project_root.join(materialization_path)
    };
    Ok(Self {
      project_root,
      read_roots,
      backend_directory,
      materialization_directory,
      max_materialization_bytes_per_attempt: policy.materialization.max_bytes_per_attempt,
      limits: policy.limits,
      retention: policy.retention,
      secret_action: policy.secret_scanning.known_secret_action,
      minimum_secret_bytes: policy.secret_scanning.minimum_secret_bytes,
    })
  }
}

fn validate_policy_limits(policy: &ArtifactPolicyV1) -> Result<(), ArtifactPolicyError> {
  let limits = &policy.limits;
  let retention = &policy.retention;
  if policy.read_roots.is_empty()
    || policy.read_roots.len() > 128
    || limits.max_artifact_bytes == 0
    || limits.max_artifact_bytes > MAX_ARTIFACT_BYTES
    || limits.max_run_bytes < limits.max_artifact_bytes
    || limits.max_run_bytes > MAX_RUN_BYTES
    || limits.max_store_bytes < limits.max_artifact_bytes
    || limits.max_store_bytes > MAX_STORE_BYTES
    || !(1..=64).contains(&limits.max_concurrent_transfers)
    || !(1_000..=MAX_TRANSFER_TIMEOUT_MS).contains(&limits.transfer_timeout_ms)
    || retention.minimum_ms < 3_600_000
    || retention.minimum_ms > retention.default_ms
    || retention.default_ms > retention.maximum_ms
    || retention.maximum_ms > 31_536_000_000
    || policy.secret_scanning.minimum_secret_bytes != 8
    || !policy.materialization.cleanup_on_start
    || policy.materialization.max_bytes_per_attempt == 0
    || policy.materialization.max_bytes_per_attempt > MAX_RUN_BYTES
  {
    return Err(ArtifactPolicyError::Invalid(
      "the policy contains inconsistent or out-of-range limits".to_string(),
    ));
  }
  Ok(())
}

fn initialize_schema(connection: &Connection) -> Result<(), CapabilityFailure> {
  connection
    .execute_batch(
      "CREATE TABLE IF NOT EXISTS woml_artifact_store_metadata(
         key TEXT PRIMARY KEY NOT NULL,
         value TEXT NOT NULL
       );
       INSERT OR IGNORE INTO woml_artifact_store_metadata(key, value)
       VALUES ('schema_version', '1');
       CREATE TABLE IF NOT EXISTS woml_artifact_blobs(
         sha256 TEXT PRIMARY KEY NOT NULL,
         size INTEGER NOT NULL CHECK(size >= 0),
         created_at TEXT NOT NULL
       );
       CREATE TABLE IF NOT EXISTS woml_artifacts(
         artifact_id TEXT PRIMARY KEY NOT NULL,
         name TEXT NOT NULL,
         size INTEGER NOT NULL CHECK(size >= 0),
         sha256 TEXT NOT NULL REFERENCES woml_artifact_blobs(sha256),
         content_type TEXT NOT NULL,
         status TEXT NOT NULL CHECK(status IN ('available','quarantined','expired','corrupt','deleted')),
         created_at TEXT NOT NULL
       );
       CREATE INDEX IF NOT EXISTS woml_artifacts_sha256 ON woml_artifacts(sha256);
       CREATE TABLE IF NOT EXISTS woml_artifact_provenance(
         artifact_id TEXT PRIMARY KEY NOT NULL REFERENCES woml_artifacts(artifact_id),
         workflow_id TEXT NOT NULL,
         run_id TEXT NOT NULL,
         node_id TEXT NOT NULL,
         attempt_number INTEGER NOT NULL,
         operation_key TEXT NOT NULL UNIQUE
       );
       CREATE INDEX IF NOT EXISTS woml_artifact_provenance_run ON woml_artifact_provenance(run_id);
       CREATE TABLE IF NOT EXISTS woml_artifact_access(
         artifact_id TEXT NOT NULL REFERENCES woml_artifacts(artifact_id),
         run_id TEXT NOT NULL,
         granted_at TEXT NOT NULL,
         revoked_at TEXT,
         PRIMARY KEY(artifact_id, run_id)
       );
       CREATE TABLE IF NOT EXISTS woml_artifact_transfers(
         operation_key TEXT PRIMARY KEY NOT NULL,
         run_id TEXT NOT NULL,
         node_id TEXT NOT NULL,
         attempt_number INTEGER NOT NULL,
         operation TEXT NOT NULL CHECK(operation IN ('put','head','materialize')),
         input_digest TEXT NOT NULL,
         status TEXT NOT NULL CHECK(status IN ('started','succeeded','failed','cancelled','interrupted')),
         artifact_id TEXT REFERENCES woml_artifacts(artifact_id),
         content_digest TEXT,
         size INTEGER,
         result_digest TEXT,
         error_code TEXT,
         ambiguous INTEGER,
         started_at TEXT NOT NULL,
         settled_at TEXT
       );
       CREATE INDEX IF NOT EXISTS woml_artifact_transfers_run ON woml_artifact_transfers(run_id);
       CREATE TABLE IF NOT EXISTS woml_artifact_retention(
         artifact_id TEXT PRIMARY KEY NOT NULL REFERENCES woml_artifacts(artifact_id),
         requested_ms INTEGER NOT NULL,
         effective_ms INTEGER NOT NULL,
         expires_at TEXT NOT NULL
       );",
    )
    .map_err(|_| backend_unavailable())?;
  Ok(())
}

fn ensure_store_identity(connection: &Connection) -> Result<String, CapabilityFailure> {
  let existing: Option<String> = connection
    .query_row(
      "SELECT value FROM woml_artifact_store_metadata WHERE key = 'store_id'",
      [],
      |row| row.get(0),
    )
    .optional()
    .map_err(|_| backend_unavailable())?;
  if let Some(store_id) = existing {
    if store_id.starts_with("artifact-store-") && store_id.len() == 47 {
      return Ok(store_id);
    }
    return Err(backend_unavailable());
  }
  let store_id = format!("artifact-store-{}", uuid::Uuid::new_v4().simple());
  connection
    .execute(
      "INSERT INTO woml_artifact_store_metadata(key, value) VALUES ('store_id', ?1)",
      [&store_id],
    )
    .map_err(|_| backend_unavailable())?;
  Ok(store_id)
}

fn validate_schema(connection: &Connection) -> Result<(), CapabilityFailure> {
  let version: Option<String> = connection
    .query_row(
      "SELECT value FROM woml_artifact_store_metadata WHERE key = 'schema_version'",
      [],
      |row| row.get(0),
    )
    .optional()
    .map_err(|_| backend_unavailable())?;
  if version.as_deref() != Some(&ARTIFACT_STORE_SCHEMA_VERSION.to_string()) {
    return Err(artifact_failure(
      CapabilityFailureKind::UnsupportedVersion,
      "WOML_ARTIFACT_BACKEND_UNAVAILABLE",
      "The artifact metadata store uses an unsupported schema version.",
      false,
      false,
    ));
  }
  Ok(())
}

fn read_settlement(
  connection: &Connection,
  operation_key: &str,
) -> Result<Option<StoredSettlement>, CapabilityFailure> {
  let row: Option<(String, String, Option<String>)> = connection
    .query_row(
      "SELECT status, input_digest, artifact_id FROM woml_artifact_transfers
       WHERE operation_key = ?1",
      [operation_key],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|_| backend_unavailable())?;
  let Some((status, input_digest, artifact_id)) = row else {
    return Ok(None);
  };
  let artifact = artifact_id
    .map(|id| {
      connection
        .query_row(
          "SELECT artifact_id, name, size, sha256, content_type
           FROM woml_artifacts WHERE artifact_id = ?1 AND status = 'available'",
          [id],
          |row| {
            Ok(ArtifactReferenceV1 {
              contract: ARTIFACT_REFERENCE_CONTRACT.to_string(),
              contract_version: ARTIFACT_REFERENCE_CONTRACT_VERSION,
              id: row.get(0)?,
              name: row.get(1)?,
              size: row.get(2)?,
              sha256: row.get(3)?,
              content_type: row.get(4)?,
            })
          },
        )
        .map_err(|_| artifact_corrupt())
    })
    .transpose()?;
  Ok(Some(StoredSettlement {
    status,
    input_digest,
    artifact,
  }))
}

fn validate_settlement_identity(
  settlement: &StoredSettlement,
  input_digest: &str,
) -> Result<(), CapabilityFailure> {
  if settlement.input_digest != input_digest {
    return Err(operation_identity_conflict());
  }
  Ok(())
}

fn enforce_quotas(
  connection: &Connection,
  run_id: &str,
  digest: &str,
  size: u64,
  limits: &ArtifactLimits,
) -> Result<(), CapabilityFailure> {
  let run_bytes: u64 = connection
    .query_row(
      "SELECT COALESCE(SUM(a.size), 0)
       FROM woml_artifacts a
       JOIN woml_artifact_provenance p ON p.artifact_id = a.artifact_id
       WHERE p.run_id = ?1 AND a.status = 'available'",
      [run_id],
      |row| row.get(0),
    )
    .map_err(|_| backend_unavailable())?;
  if run_bytes.saturating_add(size) > limits.max_run_bytes {
    return Err(quota_exceeded());
  }
  let existing_size: Option<u64> = connection
    .query_row(
      "SELECT size FROM woml_artifact_blobs WHERE sha256 = ?1",
      [digest],
      |row| row.get(0),
    )
    .optional()
    .map_err(|_| backend_unavailable())?;
  if let Some(existing_size) = existing_size {
    if existing_size != size {
      return Err(artifact_corrupt());
    }
  } else {
    let store_bytes: u64 = connection
      .query_row(
        "SELECT COALESCE(SUM(size), 0) FROM woml_artifact_blobs",
        [],
        |row| row.get(0),
      )
      .map_err(|_| backend_unavailable())?;
    if store_bytes.saturating_add(size) > limits.max_store_bytes {
      return Err(quota_exceeded());
    }
  }
  Ok(())
}

fn parse_put(
  value: &Value,
  policy: &PreparedArtifactPolicy,
) -> Result<ArtifactRequestV1, CapabilityFailure> {
  let request: ArtifactRequestV1 =
    serde_json::from_value(value.clone()).map_err(|_| artifact_path_invalid())?;
  if request.contract != ARTIFACTS_CONTRACT
    || request.contract_version != ARTIFACTS_CONTRACT_VERSION
    || request.kind != "request"
    || request.operation != "put"
  {
    return Err(artifact_path_invalid());
  }
  validate_authored_path(&request.input.source_path)?;
  validate_name(&request.input.name)?;
  validate_content_type(&request.input.content_type)?;
  if request.input.retention_ms < policy.retention.minimum_ms
    || request.input.retention_ms > policy.retention.maximum_ms
  {
    return Err(retention_invalid());
  }
  Ok(request)
}

fn parse_reference_request(
  value: &Value,
  expected_operation: &str,
) -> Result<ArtifactReferenceRequestV1, CapabilityFailure> {
  let request: ArtifactReferenceRequestV1 =
    serde_json::from_value(value.clone()).map_err(|_| artifact_access_denied())?;
  if request.contract != ARTIFACTS_CONTRACT
    || request.contract_version != ARTIFACTS_CONTRACT_VERSION
    || request.kind != "request"
    || request.operation != expected_operation
  {
    return Err(artifact_access_denied());
  }
  validate_reference(&request.input.artifact)?;
  Ok(request)
}

fn validate_reference(reference: &ArtifactReferenceV1) -> Result<(), CapabilityFailure> {
  let encoded_id = reference.id.strip_prefix("artifact_");
  if reference.contract != ARTIFACT_REFERENCE_CONTRACT
    || reference.contract_version != ARTIFACT_REFERENCE_CONTRACT_VERSION
    || encoded_id
      .is_none_or(|id| id.len() != 26 || !id.bytes().all(|byte| CROCKFORD.contains(&byte)))
    || reference.sha256.len() != 64
    || !reference
      .sha256
      .bytes()
      .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    || reference.size > MAX_ARTIFACT_BYTES
  {
    return Err(artifact_access_denied());
  }
  validate_name(&reference.name).map_err(|_| artifact_access_denied())?;
  validate_content_type(&reference.content_type).map_err(|_| artifact_access_denied())
}

fn authorize_source(
  policy: &PreparedArtifactPolicy,
  authored: &str,
) -> Result<PathBuf, CapabilityFailure> {
  validate_authored_path(authored)?;
  let lexical = policy.project_root.join(authored);
  reject_link_components(&policy.project_root, &lexical)?;
  let canonical = fs::canonicalize(&lexical).map_err(|_| source_denied())?;
  if !policy
    .read_roots
    .iter()
    .any(|root| canonical.starts_with(root))
  {
    return Err(source_denied());
  }
  let metadata = fs::symlink_metadata(&canonical).map_err(|_| source_denied())?;
  if metadata.file_type().is_symlink() || !metadata.is_file() || is_windows_reparse(&metadata) {
    return Err(source_type_unsupported());
  }
  Ok(canonical)
}

fn validate_authored_path(value: &str) -> Result<(), CapabilityFailure> {
  if value.is_empty()
    || value.len() > 4096
    || value.contains('\\')
    || value.chars().any(|c| c.is_control())
    || Path::new(value).is_absolute()
    || value
      .split('/')
      .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    || Path::new(value)
      .components()
      .any(|component| !matches!(component, Component::Normal(_)))
  {
    return Err(artifact_path_invalid());
  }
  Ok(())
}

fn validate_name(value: &str) -> Result<(), CapabilityFailure> {
  if value.is_empty() || value.len() > 256 || value.chars().any(|character| character.is_control())
  {
    return Err(artifact_failure(
      CapabilityFailureKind::InvalidInput,
      "WOML_ARTIFACT_PATH_INVALID",
      "The artifact name is invalid.",
      false,
      false,
    ));
  }
  Ok(())
}

fn validate_content_type(value: &str) -> Result<(), CapabilityFailure> {
  let mut parts = value.split('/');
  let valid_part = |part: &str| {
    !part.is_empty()
      && part.len() <= 127
      && part.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
          || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
          )
      })
  };
  if value.len() > 255
    || !parts.next().is_some_and(valid_part)
    || !parts.next().is_some_and(valid_part)
    || parts.next().is_some()
  {
    return Err(artifact_failure(
      CapabilityFailureKind::InvalidInput,
      "WOML_ARTIFACT_PATH_INVALID",
      "The artifact content type is invalid.",
      false,
      false,
    ));
  }
  Ok(())
}

fn source_identity(file: &File) -> Result<SourceIdentity, CapabilityFailure> {
  let metadata = file.metadata().map_err(|_| source_changed())?;
  if !metadata.is_file() {
    return Err(source_type_unsupported());
  }
  #[cfg(unix)]
  use std::os::unix::fs::MetadataExt;
  Ok(SourceIdentity {
    length: metadata.len(),
    modified: metadata.modified().ok(),
    #[cfg(unix)]
    device: metadata.dev(),
    #[cfg(unix)]
    inode: metadata.ino(),
  })
}

fn reject_link_components(root: &Path, target: &Path) -> Result<(), CapabilityFailure> {
  let relative = target.strip_prefix(root).map_err(|_| source_denied())?;
  let mut current = root.to_path_buf();
  for component in relative.components() {
    let Component::Normal(component) = component else {
      return Err(artifact_path_invalid());
    };
    current.push(component);
    let metadata = fs::symlink_metadata(&current).map_err(|_| source_denied())?;
    if metadata.file_type().is_symlink() || is_windows_reparse(&metadata) {
      return Err(source_denied());
    }
  }
  Ok(())
}

fn cleanup_directory_contents(directory: &Path) -> Result<(), CapabilityFailure> {
  for entry in fs::read_dir(directory).map_err(|_| materialization_unavailable())? {
    let entry = entry.map_err(|_| materialization_unavailable())?;
    let path = entry.path();
    let metadata = fs::symlink_metadata(&path).map_err(|_| materialization_unavailable())?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
      fs::remove_file(path).map_err(|_| materialization_unavailable())?;
    } else if metadata.is_dir() && !is_windows_reparse(&metadata) {
      fs::remove_dir_all(path).map_err(|_| materialization_unavailable())?;
    } else {
      return Err(materialization_unavailable());
    }
  }
  sync_directory(directory)
}

fn directory_bytes(directory: &Path) -> Result<u64, CapabilityFailure> {
  let mut total = 0_u64;
  for entry in fs::read_dir(directory).map_err(|_| materialization_unavailable())? {
    let entry = entry.map_err(|_| materialization_unavailable())?;
    let metadata = fs::symlink_metadata(entry.path()).map_err(|_| materialization_unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || is_windows_reparse(&metadata) {
      return Err(materialization_unavailable());
    }
    total = total.saturating_add(metadata.len());
  }
  Ok(total)
}

fn verify_materialized_file(
  path: &Path,
  expected_digest: &str,
  expected_size: u64,
) -> Result<(), CapabilityFailure> {
  let metadata = fs::symlink_metadata(path).map_err(|_| materialization_unavailable())?;
  if metadata.file_type().is_symlink()
    || !metadata.is_file()
    || is_windows_reparse(&metadata)
    || metadata.len() != expected_size
  {
    return Err(artifact_corrupt());
  }
  let mut source = File::open(path).map_err(|_| materialization_unavailable())?;
  let mut digest = Sha256::new();
  let mut size = 0_u64;
  let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
  loop {
    let read = source.read(&mut buffer).map_err(|_| artifact_corrupt())?;
    if read == 0 {
      break;
    }
    size = size.saturating_add(read as u64);
    digest.update(&buffer[..read]);
  }
  if size != expected_size || hex::encode(digest.finalize()) != expected_digest {
    return Err(artifact_corrupt());
  }
  Ok(())
}

fn stream_materialization(
  source_path: &Path,
  final_path: &Path,
  expected_digest: &str,
  expected_size: u64,
  cancellation: &CapabilityCancellationToken,
) -> Result<(), CapabilityFailure> {
  let stage_path = final_path.with_extension(format!("{}.part", uuid::Uuid::new_v4().simple()));
  let mut options = OpenOptions::new();
  options.create_new(true).write(true).read(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let destination = options
    .open(&stage_path)
    .map_err(|_| materialization_unavailable())?;
  let mut stage = ArtifactStage {
    path: stage_path,
    file: Some(destination),
  };
  let mut source = File::open(source_path).map_err(|_| backend_unavailable())?;
  let mut digest = Sha256::new();
  let mut size = 0_u64;
  let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
  loop {
    if cancellation.is_cancelled() {
      return Err(artifact_cancelled());
    }
    let read = source.read(&mut buffer).map_err(|_| artifact_corrupt())?;
    if read == 0 {
      break;
    }
    size = size.saturating_add(read as u64);
    if size > expected_size {
      return Err(artifact_corrupt());
    }
    digest.update(&buffer[..read]);
    stage
      .file
      .as_mut()
      .ok_or_else(materialization_unavailable)?
      .write_all(&buffer[..read])
      .map_err(|_| materialization_unavailable())?;
  }
  if size != expected_size || hex::encode(digest.finalize()) != expected_digest {
    return Err(artifact_corrupt());
  }
  let file = stage.file.take().ok_or_else(materialization_unavailable)?;
  file.sync_all().map_err(|_| materialization_unavailable())?;
  drop(file);
  match fs::hard_link(&stage.path, final_path) {
    Ok(()) => {}
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
      verify_materialized_file(final_path, expected_digest, expected_size)?;
    }
    Err(_) => return Err(materialization_unavailable()),
  }
  fs::remove_file(&stage.path).map_err(|_| materialization_unavailable())?;
  sync_directory(
    final_path
      .parent()
      .ok_or_else(materialization_unavailable)?,
  )?;
  verify_materialized_file(final_path, expected_digest, expected_size)
}

#[cfg(windows)]
fn is_windows_reparse(metadata: &fs::Metadata) -> bool {
  use std::os::windows::fs::MetadataExt;
  metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_windows_reparse(_metadata: &fs::Metadata) -> bool {
  false
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, ArtifactPolicyError> {
  let canonical = fs::canonicalize(path)
    .map_err(|_| ArtifactPolicyError::Invalid(format!("the {label} is missing")))?;
  let metadata = fs::symlink_metadata(&canonical)
    .map_err(|_| ArtifactPolicyError::Invalid(format!("the {label} is unavailable")))?;
  if !metadata.is_dir() || metadata.file_type().is_symlink() || is_windows_reparse(&metadata) {
    return Err(ArtifactPolicyError::Invalid(format!(
      "the {label} is not a safe directory"
    )));
  }
  Ok(canonical)
}

fn ensure_private_directory(path: &Path) -> Result<(), CapabilityFailure> {
  if let Ok(metadata) = fs::symlink_metadata(path) {
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_windows_reparse(&metadata) {
      return Err(backend_unavailable());
    }
  } else {
    fs::create_dir_all(path).map_err(|_| backend_unavailable())?;
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
      .map_err(|_| backend_unavailable())?;
  }
  Ok(())
}

fn harden_file_permissions(path: &Path) -> Result<(), CapabilityFailure> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
      .map_err(|_| backend_unavailable())?;
  }
  Ok(())
}

fn sync_directory(path: &Path) -> Result<(), CapabilityFailure> {
  File::open(path)
    .and_then(|file| file.sync_all())
    .map_err(|_| backend_unavailable())
}

fn new_artifact_id() -> Result<String, CapabilityFailure> {
  let mut bytes = [0_u8; 16];
  getrandom::getrandom(&mut bytes).map_err(|_| backend_unavailable())?;
  let mut encoded = String::with_capacity(26);
  for group in 0..26 {
    let mut value = 0_u8;
    for offset in 0..5 {
      let bit = group * 5 + offset;
      value <<= 1;
      if bit >= 2 {
        let source = bit - 2;
        value |= (bytes[source / 8] >> (7 - source % 8)) & 1;
      }
    }
    encoded.push(CROCKFORD[value as usize] as char);
  }
  Ok(format!("artifact_{encoded}"))
}

fn canonical_digest(domain: &[u8], value: &Value) -> Result<String, CapabilityFailure> {
  let encoded = canonical_json(value).map_err(|_| artifact_path_invalid())?;
  let mut digest = Sha256::new();
  digest.update(domain);
  digest.update(encoded);
  Ok(format!("sha256:{}", hex::encode(digest.finalize())))
}

fn timestamp() -> String {
  Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
  !needle.is_empty()
    && haystack
      .windows(needle.len())
      .any(|window| window == needle)
}

fn artifact_failure(
  kind: CapabilityFailureKind,
  code: &str,
  message: &str,
  retryable: bool,
  ambiguous: bool,
) -> CapabilityFailure {
  CapabilityFailure {
    kind,
    code: code.to_string(),
    message: message.to_string(),
    retryable,
    ambiguous,
    details: None,
  }
}

fn artifact_path_invalid() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::InvalidInput,
    "WOML_ARTIFACT_PATH_INVALID",
    "The artifact source path is invalid.",
    false,
    false,
  )
}
fn source_denied() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_SOURCE_DENIED",
    "The artifact source is outside approved roots.",
    false,
    false,
  )
}
fn source_changed() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_SOURCE_CHANGED",
    "The artifact source changed while it was imported.",
    false,
    true,
  )
}
fn source_type_unsupported() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_SOURCE_TYPE_UNSUPPORTED",
    "Artifact v1 accepts regular files only.",
    false,
    false,
  )
}
fn artifact_too_large() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::InputTooLarge,
    "WOML_ARTIFACT_TOO_LARGE",
    "The artifact exceeds its configured byte limit.",
    false,
    false,
  )
}
fn quota_exceeded() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_QUOTA_EXCEEDED",
    "The artifact quota would be exceeded.",
    false,
    false,
  )
}
fn retention_invalid() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::InvalidInput,
    "WOML_ARTIFACT_RETENTION_INVALID",
    "The requested retention violates artifact policy.",
    false,
    false,
  )
}
fn artifact_not_found() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_NOT_FOUND",
    "The artifact does not exist in this durable store.",
    false,
    false,
  )
}
fn artifact_access_denied() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_ACCESS_DENIED",
    "The current run is not authorized to access this artifact.",
    false,
    false,
  )
}
fn artifact_expired() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_EXPIRED",
    "The artifact has expired and is no longer readable.",
    false,
    false,
  )
}
fn artifact_quarantined() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_QUARANTINED",
    "Artifact policy quarantined this body.",
    false,
    false,
  )
}
fn artifact_corrupt() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_CORRUPT",
    "Artifact integrity verification failed.",
    false,
    false,
  )
}
fn backend_unavailable() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::TransportFailed,
    "WOML_ARTIFACT_BACKEND_UNAVAILABLE",
    "The artifact backend is unavailable.",
    true,
    false,
  )
}
fn materialization_unavailable() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::TransportFailed,
    "WOML_ARTIFACT_MATERIALIZATION_UNAVAILABLE",
    "A private artifact materialization could not be created.",
    true,
    false,
  )
}
fn artifact_timed_out() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::TimedOut,
    "WOML_ARTIFACT_TIMED_OUT",
    "The artifact transfer exceeded its deadline.",
    false,
    true,
  )
}
fn artifact_cancelled() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::Cancelled,
    "WOML_ARTIFACT_CANCELLED",
    "The artifact transfer was cancelled.",
    false,
    true,
  )
}
fn operation_identity_conflict() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::ServiceRejected,
    "WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT",
    "The artifact operation name was reused with different input.",
    false,
    false,
  )
}
fn artifact_interrupted() -> CapabilityFailure {
  artifact_failure(
    CapabilityFailureKind::Interrupted,
    "WOML_ARTIFACT_INTERRUPTED",
    "The artifact write outcome cannot be proven.",
    false,
    true,
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{derive_operation_key, CapabilityCallIdentity, CapabilityCallLimits};

  struct TempDir(PathBuf);

  impl TempDir {
    fn new() -> std::io::Result<Self> {
      let path = std::env::temp_dir().join(format!(
        "woml-durable-artifacts-{}",
        uuid::Uuid::new_v4().simple()
      ));
      fs::create_dir(&path)?;
      Ok(Self(path))
    }

    fn path(&self) -> &Path {
      &self.0
    }
  }

  impl Drop for TempDir {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  fn policy(root: &Path, backend: &Path) -> ArtifactPolicyV1 {
    ArtifactPolicyV1 {
      contract: ARTIFACT_POLICY_CONTRACT.to_string(),
      contract_version: 1,
      enabled: true,
      modes: ArtifactPolicyModes {
        foreground: true,
        background: false,
      },
      project_root: root.to_string_lossy().into_owned(),
      read_roots: vec![".".to_string()],
      materialization: ArtifactMaterializationPolicy {
        directory: root.join("materialized").to_string_lossy().into_owned(),
        cleanup_on_start: true,
        max_bytes_per_attempt: 1_048_576,
      },
      limits: ArtifactLimits {
        max_artifact_bytes: 1_048_576,
        max_run_bytes: 2_097_152,
        max_store_bytes: 4_194_304,
        max_concurrent_transfers: 2,
        transfer_timeout_ms: 30_000,
      },
      retention: ArtifactRetentionPolicy {
        default_ms: 86_400_000,
        minimum_ms: 3_600_000,
        maximum_ms: 31_536_000_000,
      },
      secret_scanning: ArtifactSecretScanningPolicy {
        known_secret_action: KnownSecretAction::Reject,
        minimum_secret_bytes: 8,
      },
      backend: ArtifactBackendPolicy::Local {
        contract: "woml.artifact-backend".to_string(),
        contract_version: 1,
        directory: backend.to_string_lossy().into_owned(),
      },
    }
  }

  fn call(path: &str, name: &str) -> CapabilityCallRequest {
    let step_key = "step-key".to_string();
    CapabilityCallRequest {
      contract: "woml.capability-call".to_string(),
      contract_version: 1,
      message_type: "request".to_string(),
      invocation_id: "invocation-artifact".to_string(),
      call_id: "call-artifact".to_string(),
      run_id: "run-artifact".to_string(),
      node_id: "create".to_string(),
      attempt_number: 1,
      capability: "artifacts".to_string(),
      operation: "put".to_string(),
      input_contract_version: 1,
      result_contract_version: 1,
      identity: CapabilityCallIdentity {
        mode: CapabilityIdentityMode::Named,
        step_idempotency_key: step_key.clone(),
        operation_name: name.to_string(),
        operation_key: derive_operation_key(&step_key, name),
        provider_idempotency_key: None,
      },
      limits: CapabilityCallLimits {
        input_bytes: 4096,
        result_bytes: 16384,
        timeout_ms: 30_000,
      },
      input: json!({
        "contract": "woml.artifacts",
        "contractVersion": 1,
        "kind": "request",
        "operation": "put",
        "input": {
          "sourcePath": path,
          "name": name,
          "contentType": "text/plain",
          "retentionMs": 86400000
        }
      }),
    }
  }

  fn reference_call(
    operation: &str,
    artifact: &ArtifactReferenceV1,
    run_id: &str,
    invocation_id: &str,
  ) -> CapabilityCallRequest {
    let step_key = format!("step-key-{run_id}");
    let operation_name = format!("artifacts.{operation}#1");
    CapabilityCallRequest {
      contract: "woml.capability-call".to_string(),
      contract_version: 1,
      message_type: "request".to_string(),
      invocation_id: invocation_id.to_string(),
      call_id: format!("call-{operation}"),
      run_id: run_id.to_string(),
      node_id: "consume".to_string(),
      attempt_number: 1,
      capability: "artifacts".to_string(),
      operation: operation.to_string(),
      input_contract_version: 1,
      result_contract_version: 1,
      identity: CapabilityCallIdentity {
        mode: CapabilityIdentityMode::Automatic,
        step_idempotency_key: step_key.clone(),
        operation_name: operation_name.clone(),
        operation_key: derive_operation_key(&step_key, &operation_name),
        provider_idempotency_key: None,
      },
      limits: CapabilityCallLimits {
        input_bytes: 16_384,
        result_bytes: 16_384,
        timeout_ms: 30_000,
      },
      input: json!({
        "contract": "woml.artifacts",
        "contractVersion": 1,
        "kind": "request",
        "operation": operation,
        "input": { "artifact": artifact }
      }),
    }
  }

  #[tokio::test]
  async fn put_is_durable_deduplicated_and_identity_safe() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact").unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    fs::create_dir_all(backend.join("staging")).unwrap();
    fs::write(backend.join("staging/orphan.part"), b"partial").unwrap();
    let managed = Arc::new(ManagedArtifactStore::default());
    let handler = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    managed
      .configure_for_state(&state, handler.policy())
      .unwrap();
    assert!(!backend.join("staging/orphan.part").exists());
    let request = call("report.txt", "report");
    let first = handler
      .execute_request_scoped(
        &request,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let second = handler
      .execute_request_scoped(
        &request,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["contract"], "woml.artifacts");
    assert_eq!(first["data"]["contract"], "woml.artifact-reference");
    assert_eq!(first["data"]["name"], "report");
    assert_eq!(first["data"]["size"], 16);
    let digest = first["data"]["sha256"].as_str().unwrap();
    assert!(backend
      .join("blobs/sha256")
      .join(&digest[..2])
      .join(&digest[2..])
      .is_file());
    let connection = Connection::open(&state).unwrap();
    for table in [
      "woml_artifacts",
      "woml_artifact_blobs",
      "woml_artifact_provenance",
      "woml_artifact_access",
      "woml_artifact_transfers",
      "woml_artifact_retention",
    ] {
      let count: u64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
          row.get(0)
        })
        .unwrap();
      assert_eq!(count, 1, "{table}");
    }
    fs::write(temp.path().join("report.txt"), b"changed bytes").unwrap();
    let failure = handler
      .execute_request_scoped(
        &request,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT");
  }

  #[tokio::test]
  async fn put_enforces_byte_limits_and_known_secret_scanning() {
    let temp = TempDir::new().unwrap();
    fs::write(
      temp.path().join("secret.txt"),
      b"prefix super-secret-value suffix",
    )
    .unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let managed = Arc::new(ManagedArtifactStore::default());
    let mut secrets = BTreeMap::new();
    secrets.insert("TOKEN".to_string(), "super-secret-value".to_string());
    let handler = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &secrets,
    )
    .unwrap();
    managed
      .configure_for_state(&state, handler.policy())
      .unwrap();
    let request = call("secret.txt", "secret-report");
    let failure = handler
      .execute_request_scoped(
        &request,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_QUARANTINED");
    let connection = Connection::open(&state).unwrap();
    let settlement: (String, String, i64) = connection
      .query_row(
        "SELECT status, error_code, ambiguous FROM woml_artifact_transfers",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .unwrap();
    assert_eq!(
      settlement,
      (
        "failed".to_string(),
        "WOML_ARTIFACT_QUARANTINED".to_string(),
        0
      )
    );

    let second_root = TempDir::new().unwrap();
    fs::write(second_root.path().join("large.txt"), b"12345").unwrap();
    let mut limited = policy(second_root.path(), &second_root.path().join("artifacts-v1"));
    limited.limits.max_artifact_bytes = 4;
    limited.limits.max_run_bytes = 4;
    limited.limits.max_store_bytes = 4;
    let limited_store = Arc::new(ManagedArtifactStore::default());
    let limited_handler = ManagedArtifactPutHandler::new(
      Arc::clone(&limited_store),
      limited,
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    limited_store
      .configure_for_state(
        &second_root.path().join("state.sqlite"),
        limited_handler.policy(),
      )
      .unwrap();
    let failure = limited_handler
      .execute_request_scoped(
        &call("large.txt", "large-report"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_TOO_LARGE");
  }

  #[tokio::test]
  async fn head_and_materialize_verify_same_run_access_and_cleanup() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact").unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    let head = put.head_handler();
    let materialize = put.materialize_handler();
    managed.configure_for_state(&state, put.policy()).unwrap();
    let stored = put
      .execute_request_scoped(
        &call("report.txt", "report"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let reference: ArtifactReferenceV1 = serde_json::from_value(stored["data"].clone()).unwrap();

    let head_result = head
      .execute_request_scoped(
        &reference_call("head", &reference, "run-artifact", "inv-head"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    assert_eq!(head_result["data"]["integrity"], "verified");
    assert_eq!(head_result["data"]["artifact"]["id"], reference.id);

    let invocation_id = "inv-materialize";
    let materialized = materialize
      .execute_request_scoped(
        &reference_call("materialize", &reference, "run-artifact", invocation_id),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let path = PathBuf::from(materialized["data"]["path"].as_str().unwrap());
    assert_eq!(fs::read(&path).unwrap(), b"durable artifact");
    managed.cleanup_invocation(invocation_id);
    assert!(!path.exists());
  }

  #[tokio::test]
  async fn artifact_reads_reject_unauthorized_forged_expired_and_corrupt_references() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact").unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    let head = put.head_handler();
    managed.configure_for_state(&state, put.policy()).unwrap();
    let stored = put
      .execute_request_scoped(
        &call("report.txt", "report"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let reference: ArtifactReferenceV1 = serde_json::from_value(stored["data"].clone()).unwrap();

    let failure = head
      .execute_request_scoped(
        &reference_call("head", &reference, "another-run", "inv-other"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_ACCESS_DENIED");

    let mut forged = reference.clone();
    forged.name = "forged".to_string();
    let failure = head
      .execute_request_scoped(
        &reference_call("head", &forged, "run-artifact", "inv-forged"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_ACCESS_DENIED");

    Connection::open(&state)
      .unwrap()
      .execute(
        "UPDATE woml_artifact_retention SET expires_at = '2000-01-01T00:00:00.000Z'",
        [],
      )
      .unwrap();
    let failure = head
      .execute_request_scoped(
        &reference_call("head", &reference, "run-artifact", "inv-expired"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_EXPIRED");

    Connection::open(&state)
      .unwrap()
      .execute(
        "UPDATE woml_artifact_retention SET expires_at = '2999-01-01T00:00:00.000Z'",
        [],
      )
      .unwrap();
    let blob = backend
      .join("blobs/sha256")
      .join(&reference.sha256[..2])
      .join(&reference.sha256[2..]);
    fs::write(blob, b"corrupt artifact").unwrap();
    let failure = head
      .execute_request_scoped(
        &reference_call("head", &reference, "run-artifact", "inv-corrupt"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_CORRUPT");

    let other = TempDir::new().unwrap();
    let other_store = Arc::new(ManagedArtifactStore::default());
    let other_put = ManagedArtifactPutHandler::new(
      Arc::clone(&other_store),
      policy(other.path(), &other.path().join("artifacts-v1")),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    let other_head = other_put.head_handler();
    other_store
      .configure_for_state(&other.path().join("state.sqlite"), other_put.policy())
      .unwrap();
    let failure = other_head
      .execute_request_scoped(
        &reference_call("head", &reference, "run-artifact", "inv-other-store"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_NOT_FOUND");
  }

  #[tokio::test]
  async fn materialization_cleans_partial_files_and_fails_closed_on_cancellation() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.bin"), vec![b'x'; 512 * 1024]).unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    let materialize = put.materialize_handler();
    managed.configure_for_state(&state, put.policy()).unwrap();
    let stored = put
      .execute_request_scoped(
        &call("report.bin", "report"),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let reference: ArtifactReferenceV1 = serde_json::from_value(stored["data"].clone()).unwrap();
    let store = managed.configured().unwrap();

    let corrupt_invocation = "inv-partial";
    let corrupt_directory = store.invocation_materialization_directory(corrupt_invocation);
    ensure_private_directory(&corrupt_directory).unwrap();
    fs::write(corrupt_directory.join(&reference.id), b"partial").unwrap();
    let failure = materialize
      .execute_request_scoped(
        &reference_call(
          "materialize",
          &reference,
          "run-artifact",
          corrupt_invocation,
        ),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_CORRUPT");
    managed.cleanup_invocation(corrupt_invocation);

    let unavailable_invocation = "inv-unavailable";
    let unavailable_path = store.invocation_materialization_directory(unavailable_invocation);
    fs::write(&unavailable_path, b"directory collision").unwrap();
    let failure = materialize
      .execute_request_scoped(
        &reference_call(
          "materialize",
          &reference,
          "run-artifact",
          unavailable_invocation,
        ),
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_MATERIALIZATION_UNAVAILABLE");
    fs::remove_file(unavailable_path).unwrap();

    let cancellation = CapabilityCancellationToken::default();
    cancellation.cancel();
    let failure = materialize
      .execute_request_scoped(
        &reference_call("materialize", &reference, "run-artifact", "inv-cancelled"),
        Some("workflow".to_string()),
        cancellation,
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_CANCELLED");

    let startup_partial = store.materializations.join("abandoned-attempt");
    ensure_private_directory(&startup_partial).unwrap();
    fs::write(startup_partial.join("download.part"), b"partial").unwrap();
    drop(store);
    drop(materialize);
    drop(put);
    drop(managed);
    let recovered = Arc::new(ManagedArtifactStore::default());
    let recovered_put = ManagedArtifactPutHandler::new(
      Arc::clone(&recovered),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    recovered
      .configure_for_state(&state, recovered_put.policy())
      .unwrap();
    assert!(!startup_partial.exists());
  }

  #[tokio::test]
  async fn retries_reconnect_and_startup_recovers_uncommitted_files_fail_closed() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact").unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    managed.configure_for_state(&state, put.policy()).unwrap();
    let original = call("report.txt", "report");
    let first = put
      .execute_request_scoped(
        &original,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let mut retry = original.clone();
    retry.attempt_number = 2;
    retry.invocation_id = "invocation-artifact-retry".to_string();
    retry.call_id = "call-artifact-retry".to_string();
    let replayed = put
      .execute_request_scoped(
        &retry,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    assert_eq!(first, replayed);
    let connection = Connection::open(&state).unwrap();
    let artifacts: u64 = connection
      .query_row("SELECT COUNT(*) FROM woml_artifacts", [], |row| row.get(0))
      .unwrap();
    assert_eq!(artifacts, 1);

    let crash_root = TempDir::new().unwrap();
    let crash_state = crash_root.path().join("state.sqlite");
    let crash_backend = crash_root.path().join("artifacts-v1");
    let initial_store = Arc::new(ManagedArtifactStore::default());
    let initial_put = ManagedArtifactPutHandler::new(
      Arc::clone(&initial_store),
      policy(crash_root.path(), &crash_backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    initial_store
      .configure_for_state(&crash_state, initial_put.policy())
      .unwrap();
    drop(initial_put);
    drop(initial_store);
    let connection = Connection::open(&crash_state).unwrap();
    let interrupted = call("report.txt", "interrupted");
    connection
      .execute(
        "INSERT INTO woml_artifact_transfers(
           operation_key, run_id, node_id, attempt_number, operation,
           input_digest, status, started_at
         ) VALUES (?1, ?2, ?3, 1, 'put', ?4, 'started', ?5)",
        params![
          interrupted.identity.operation_key,
          interrupted.run_id,
          interrupted.node_id,
          canonical_digest(b"woml.artifact-input\0v1\0", &interrupted.input).unwrap(),
          timestamp(),
        ],
      )
      .unwrap();
    fs::create_dir_all(crash_backend.join("staging")).unwrap();
    fs::write(
      crash_backend.join("staging/before-publish.part"),
      b"partial",
    )
    .unwrap();
    let orphan_digest = hex::encode(Sha256::digest(b"published without metadata"));
    let orphan = crash_backend
      .join("blobs/sha256")
      .join(&orphan_digest[..2])
      .join(&orphan_digest[2..]);
    fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    fs::write(&orphan, b"published without metadata").unwrap();
    drop(connection);

    let recovered = Arc::new(ManagedArtifactStore::default());
    let recovered_put = ManagedArtifactPutHandler::new(
      Arc::clone(&recovered),
      policy(crash_root.path(), &crash_backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    recovered
      .configure_for_state(&crash_state, recovered_put.policy())
      .unwrap();
    assert!(!crash_backend.join("staging/before-publish.part").exists());
    assert!(!orphan.exists());
    let failure = recovered_put
      .execute_request_scoped(
        &interrupted,
        Some("workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_INTERRUPTED");
  }

  #[test]
  fn artifact_backend_is_bound_to_one_durable_store() {
    let temp = TempDir::new().unwrap();
    let backend = temp.path().join("artifacts-v1");
    let first_store = Arc::new(ManagedArtifactStore::default());
    let first_put = ManagedArtifactPutHandler::new(
      Arc::clone(&first_store),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    first_store
      .configure_for_state(&temp.path().join("first.sqlite"), first_put.policy())
      .unwrap();
    drop(first_put);
    drop(first_store);

    let second_store = Arc::new(ManagedArtifactStore::default());
    let second_put = ManagedArtifactPutHandler::new(
      Arc::clone(&second_store),
      policy(temp.path(), &backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    let failure = second_store
      .configure_for_state(&temp.path().join("second.sqlite"), second_put.policy())
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_BACKEND_UNAVAILABLE");
  }

  #[test]
  fn future_artifact_schema_is_rejected() {
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state.sqlite");
    DurableEventStore::open(&state).unwrap();
    let connection = Connection::open(&state).unwrap();
    initialize_schema(&connection).unwrap();
    connection
      .execute(
        "UPDATE woml_artifact_store_metadata SET value = '2' WHERE key = 'schema_version'",
        [],
      )
      .unwrap();
    assert_eq!(
      validate_schema(&connection).unwrap_err().kind,
      CapabilityFailureKind::UnsupportedVersion
    );
  }
}
