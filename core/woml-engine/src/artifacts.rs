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
use hmac::{Hmac, Mac};
use reqwest::{Method, StatusCode, Url};
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
const ARTIFACT_MAINTENANCE_BATCH: usize = 250;
const ARTIFACT_SCRUB_BATCH: usize = 64;
const ARTIFACT_ORPHAN_SCAN_BATCH: usize = 1_000;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactMaintenanceV1 {
  pub profile: String,
  pub kind: String,
  pub completed_at: String,
  pub eligible_artifacts: u64,
  pub eligible_blobs: u64,
  pub estimated_bytes: u64,
  pub deleted_artifacts: u64,
  pub deleted_blobs: u64,
  pub deleted_bytes: u64,
  pub scrubbed_blobs: u64,
  pub corrupt_blobs: u64,
  pub collected_orphans: u64,
  pub live_artifacts: u64,
  pub live_blobs: u64,
  pub live_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactBackupInventoryV1 {
  pub profile: String,
  pub directory: String,
  pub blob_count: u64,
  pub total_bytes: u64,
  pub inventory_digest: String,
  pub verified: bool,
}

/// Body-free metadata returned to an authenticated local operator. This is
/// deliberately richer than Artifact Reference v1 but never contains a
/// backend location, materialized path, signed URL, credential, or body data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactInspectionV1 {
  pub profile: String,
  pub artifact: ArtifactReferenceV1,
  pub digest_prefix: String,
  pub status: String,
  pub created_at: String,
  pub expires_at: String,
  pub retention_ms: u64,
  pub provenance: ArtifactProvenanceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactProvenanceV1 {
  pub workflow_id: String,
  pub run_id: String,
  pub node_id: String,
  pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRunInspectionV1 {
  pub profile: String,
  pub run_id: String,
  pub artifacts: Vec<ArtifactInspectionV1>,
  pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactExportV1 {
  pub profile: String,
  pub audit_id: String,
  pub artifact_id: String,
  pub name: String,
  pub size: u64,
  pub digest_prefix: String,
  pub exported_at: String,
  pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactObservationV1 {
  pub profile: String,
  pub backend: String,
  pub backend_healthy: bool,
  pub transfers: u64,
  pub transferred_bytes: u64,
  pub transfer_duration_ms: u64,
  pub failed_transfers: u64,
  pub corrupt_artifacts: u64,
  pub orphan_blobs: u64,
  pub live_artifacts: u64,
  pub live_bytes: u64,
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

#[derive(Clone)]
pub(crate) struct PreparedArtifactPolicy {
  project_root: PathBuf,
  read_roots: Vec<PathBuf>,
  backend: PreparedArtifactBackend,
  materialization_directory: PathBuf,
  max_materialization_bytes_per_attempt: u64,
  limits: ArtifactLimits,
  retention: ArtifactRetentionPolicy,
  secret_action: KnownSecretAction,
  minimum_secret_bytes: usize,
}

impl std::fmt::Debug for PreparedArtifactPolicy {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("PreparedArtifactPolicy")
      .field("project_root", &self.project_root)
      .field("read_roots", &self.read_roots)
      .field("backend", &self.backend)
      .field("materialization_directory", &self.materialization_directory)
      .field("limits", &self.limits)
      .field("retention", &self.retention)
      .field("secret_action", &self.secret_action)
      .finish()
  }
}

#[derive(Clone)]
enum PreparedArtifactBackend {
  Local { directory: PathBuf },
  S3(Box<PreparedS3Backend>),
}

impl std::fmt::Debug for PreparedArtifactBackend {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Local { directory } => formatter
        .debug_struct("Local")
        .field("directory", directory)
        .finish(),
      Self::S3(config) => formatter
        .debug_struct("S3")
        .field("bucket", &config.bucket)
        .field("region", &config.region)
        .field("prefix", &config.prefix)
        .field("endpoint", &config.endpoint)
        .field("force_path_style", &config.force_path_style)
        .field("server_side_encryption", &config.server_side_encryption)
        .finish_non_exhaustive(),
    }
  }
}

#[derive(Clone)]
struct PreparedS3Backend {
  bucket: String,
  region: String,
  prefix: String,
  endpoint: Url,
  force_path_style: bool,
  credentials: S3Credentials,
  server_side_encryption: S3ServerSideEncryption,
}

#[derive(Clone)]
struct S3Credentials {
  access_key: String,
  secret_key: String,
  session_token: Option<String>,
}

#[derive(Debug, Clone)]
enum S3ServerSideEncryption {
  Aes256,
  AwsKms { key_id: String },
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

type StoredArtifactRow = (String, String, u64, String, String, String, String, String);

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
  fn identity(&self) -> &str;
  fn bind_store(&self, connection: &Connection, store_id: &str) -> Result<(), CapabilityFailure>;
  fn begin_stage(&self) -> Result<ArtifactStage, CapabilityFailure>;
  fn publish(
    &self,
    stage: ArtifactStage,
    sha256: &str,
    size: u64,
    cancellation: &CapabilityCancellationToken,
  ) -> Result<(), CapabilityFailure>;
  fn cleanup_startup(&self, connection: &Connection) -> Result<(), CapabilityFailure>;
  fn verify(&self, sha256: &str, size: u64) -> Result<VerifiedArtifactBlob, CapabilityFailure>;
  fn delete(&self, sha256: &str) -> Result<(), CapabilityFailure>;
  fn inventory(&self, limit: usize) -> Result<Vec<String>, CapabilityFailure>;
}

#[derive(Debug)]
struct VerifiedArtifactBlob {
  path: PathBuf,
  temporary: bool,
}

impl Drop for VerifiedArtifactBlob {
  fn drop(&mut self) {
    if self.temporary {
      let _ = fs::remove_file(&self.path);
    }
  }
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
  fn identity(&self) -> &str {
    self.root.to_str().unwrap_or("local")
  }

  fn bind_store(&self, connection: &Connection, store_id: &str) -> Result<(), CapabilityFailure> {
    LocalArtifactBackend::bind_store(self, connection, store_id)
  }

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
    cancellation: &CapabilityCancellationToken,
  ) -> Result<(), CapabilityFailure> {
    if cancellation.is_cancelled() {
      return Err(artifact_cancelled());
    }
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

  fn verify(&self, sha256: &str, size: u64) -> Result<VerifiedArtifactBlob, CapabilityFailure> {
    let path = self.blob_path(sha256);
    self.verify_blob(&path, sha256, size)?;
    Ok(VerifiedArtifactBlob {
      path,
      temporary: false,
    })
  }

  fn delete(&self, sha256: &str) -> Result<(), CapabilityFailure> {
    let path = self.blob_path(sha256);
    match fs::remove_file(&path) {
      Ok(()) => {
        if let Some(parent) = path.parent() {
          sync_directory(parent)?;
          if fs::read_dir(parent)
            .map_err(|_| backend_unavailable())?
            .next()
            .is_none()
          {
            let _ = fs::remove_dir(parent);
          }
        }
        Ok(())
      }
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(_) => Err(backend_unavailable()),
    }
  }

  fn inventory(&self, limit: usize) -> Result<Vec<String>, CapabilityFailure> {
    let mut digests = Vec::new();
    for prefix in fs::read_dir(&self.blobs).map_err(|_| backend_unavailable())? {
      let prefix = prefix.map_err(|_| backend_unavailable())?;
      if !prefix
        .file_type()
        .map_err(|_| backend_unavailable())?
        .is_dir()
      {
        continue;
      }
      let prefix_name = prefix.file_name().to_string_lossy().into_owned();
      for entry in fs::read_dir(prefix.path()).map_err(|_| backend_unavailable())? {
        let entry = entry.map_err(|_| backend_unavailable())?;
        if entry
          .file_type()
          .map_err(|_| backend_unavailable())?
          .is_file()
        {
          let digest = format!("{}{}", prefix_name, entry.file_name().to_string_lossy());
          if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            digests.push(digest);
            if digests.len() >= limit {
              digests.sort();
              return Ok(digests);
            }
          }
        }
      }
    }
    digests.sort();
    Ok(digests)
  }
}

const S3_MULTIPART_THRESHOLD_BYTES: u64 = 64 * 1024 * 1024;
const S3_MULTIPART_PART_BYTES: usize = 8 * 1024 * 1024;
const S3_RECOVERY_ATTEMPTS: usize = 4;
const S3_CONTROL_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct S3ArtifactBackend {
  config: PreparedS3Backend,
  client: reqwest::Client,
  staging: PathBuf,
  identity: String,
}

impl S3ArtifactBackend {
  fn open(config: PreparedS3Backend, materializations: &Path) -> Result<Self, CapabilityFailure> {
    let staging = materializations.join(".s3-staging");
    ensure_private_directory(&staging)?;
    let client = reqwest::Client::builder()
      .https_only(config.endpoint.scheme() == "https")
      .connect_timeout(Duration::from_secs(15))
      .timeout(Duration::from_secs(60 * 60))
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .map_err(|_| backend_unavailable())?;
    let identity = format!(
      "s3://{}/{}@{}",
      config.bucket,
      config.prefix,
      config.endpoint.origin().ascii_serialization()
    );
    Ok(Self {
      config,
      client,
      staging,
      identity,
    })
  }

  fn object_key(&self, suffix: &str) -> String {
    format!("{}{}", self.config.prefix, suffix)
  }

  fn blob_key(&self, digest: &str) -> String {
    self.object_key(&format!("blobs/sha256/{}/{}", &digest[..2], &digest[2..]))
  }

  fn endpoint(
    &self,
    key: &str,
    query: &[(String, String)],
  ) -> Result<(Url, String, String), CapabilityFailure> {
    let mut url = self.config.endpoint.clone();
    let base = url.path().trim_end_matches('/').to_string();
    let path = if self.config.force_path_style {
      format!("{base}/{}/{key}", self.config.bucket)
    } else {
      let host = url.host_str().ok_or_else(backend_unavailable)?;
      url
        .set_host(Some(&format!("{}.{}", self.config.bucket, host)))
        .map_err(|_| backend_unavailable())?;
      format!("{base}/{key}")
    };
    url.set_path(&path);
    let mut encoded_query = query
      .iter()
      .map(|(name, value)| {
        format!(
          "{}={}",
          aws_percent_encode(name.as_bytes()),
          aws_percent_encode(value.as_bytes())
        )
      })
      .collect::<Vec<_>>();
    encoded_query.sort();
    let canonical_query = encoded_query.join("&");
    url.set_query((!canonical_query.is_empty()).then_some(&canonical_query));
    let canonical_uri = if url.path().is_empty() {
      "/".to_string()
    } else {
      url.path().to_string()
    };
    Ok((url, canonical_uri, canonical_query))
  }

  async fn send(
    &self,
    method: Method,
    key: &str,
    query: Vec<(String, String)>,
    payload_hash: String,
    body: Option<reqwest::Body>,
    mut extra_headers: BTreeMap<String, String>,
  ) -> Result<reqwest::Response, CapabilityFailure> {
    let (url, canonical_uri, canonical_query) = self.endpoint(key, &query)?;
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let host = match url.port() {
      Some(port) => format!("{}:{port}", url.host_str().ok_or_else(backend_unavailable)?),
      None => url.host_str().ok_or_else(backend_unavailable)?.to_string(),
    };
    extra_headers.insert("host".to_string(), host);
    extra_headers.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
    extra_headers.insert("x-amz-date".to_string(), amz_date.clone());
    if let Some(token) = &self.config.credentials.session_token {
      extra_headers.insert("x-amz-security-token".to_string(), token.clone());
    }
    let canonical_headers = extra_headers
      .iter()
      .map(|(name, value)| format!("{name}:{}\n", value.trim()))
      .collect::<String>();
    let signed_headers = extra_headers.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical_request = format!(
      "{}\n{}\n{}\n{}\n{}\n{}",
      method.as_str(),
      canonical_uri,
      canonical_query,
      canonical_headers,
      signed_headers,
      payload_hash
    );
    let scope = format!("{date}/{}/s3/aws4_request", self.config.region);
    let string_to_sign = format!(
      "AWS4-HMAC-SHA256\n{}\n{}\n{}",
      amz_date,
      scope,
      hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let date_key = hmac_sha256(
      format!("AWS4{}", self.config.credentials.secret_key).as_bytes(),
      date.as_bytes(),
    )?;
    let region_key = hmac_sha256(&date_key, self.config.region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    let signing_key = hmac_sha256(&service_key, b"aws4_request")?;
    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes())?);
    let authorization = format!(
      "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
      self.config.credentials.access_key, scope, signed_headers, signature
    );
    let mut request = self
      .client
      .request(method, url)
      .header("authorization", authorization);
    for (name, value) in extra_headers {
      request = request.header(name, value);
    }
    if let Some(body) = body {
      request = request.body(body);
    }
    request.send().await.map_err(|_| backend_unavailable())
  }

  fn encryption_headers(&self) -> BTreeMap<String, String> {
    match &self.config.server_side_encryption {
      S3ServerSideEncryption::Aes256 => BTreeMap::from([(
        "x-amz-server-side-encryption".to_string(),
        "AES256".to_string(),
      )]),
      S3ServerSideEncryption::AwsKms { key_id } => BTreeMap::from([
        (
          "x-amz-server-side-encryption".to_string(),
          "aws:kms".to_string(),
        ),
        (
          "x-amz-server-side-encryption-aws-kms-key-id".to_string(),
          key_id.clone(),
        ),
      ]),
    }
  }

  fn run<T, F>(&self, future: F) -> Result<T, CapabilityFailure>
  where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, CapabilityFailure>> + Send + 'static,
  {
    std::thread::spawn(move || {
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| backend_unavailable())?
        .block_on(future)
    })
    .join()
    .map_err(|_| backend_unavailable())?
  }

  fn upload(
    &self,
    path: &Path,
    key: &str,
    digest: &str,
    size: u64,
    cancellation: &CapabilityCancellationToken,
  ) -> Result<(), CapabilityFailure> {
    if size >= S3_MULTIPART_THRESHOLD_BYTES {
      return self.upload_multipart(path, key, size, cancellation);
    }
    for attempt in 0..S3_RECOVERY_ATTEMPTS {
      if cancellation.is_cancelled() {
        return Err(artifact_cancelled());
      }
      let backend = self.clone();
      let path = path.to_path_buf();
      let key = key.to_string();
      let digest = digest.to_string();
      let cancellation = cancellation.clone();
      let outcome = self.run(async move {
        let body = streaming_request_body(path, cancellation)?;
        let mut headers = backend.encryption_headers();
        headers.insert("if-none-match".to_string(), "*".to_string());
        headers.insert("content-length".to_string(), size.to_string());
        let response = backend
          .send(Method::PUT, &key, vec![], digest, Some(body), headers)
          .await?;
        if response.status().is_success() || response.status() == StatusCode::PRECONDITION_FAILED {
          Ok(())
        } else {
          Err(backend_unavailable())
        }
      });
      if outcome.is_ok() {
        return outcome;
      }
      if attempt + 1 < S3_RECOVERY_ATTEMPTS {
        std::thread::sleep(Duration::from_millis(25 * (attempt as u64 + 1)));
      }
    }
    Err(backend_unavailable())
  }

  fn upload_multipart(
    &self,
    path: &Path,
    key: &str,
    size: u64,
    cancellation: &CapabilityCancellationToken,
  ) -> Result<(), CapabilityFailure> {
    let backend = self.clone();
    let path = path.to_path_buf();
    let key = key.to_string();
    let cancellation = cancellation.clone();
    self.run(async move {
      let empty_hash = hex::encode(Sha256::digest([]));
      let initiated = backend
        .send(
          Method::POST,
          &key,
          vec![("uploads".to_string(), String::new())],
          empty_hash.clone(),
          None,
          backend.encryption_headers(),
        )
        .await?;
      if !initiated.status().is_success() {
        return Err(backend_unavailable());
      }
      let initiated = bounded_response_text(initiated, S3_CONTROL_RESPONSE_MAX_BYTES).await?;
      let upload_id = xml_values(&initiated, "UploadId")
        .into_iter()
        .next()
        .ok_or_else(backend_unavailable)?;
      let outcome = async {
        let mut source = File::open(&path).map_err(|_| backend_unavailable())?;
        let mut parts = Vec::new();
        let mut uploaded = 0_u64;
        let mut part_number = 1_u32;
        while uploaded < size {
          if cancellation.is_cancelled() {
            return Err(artifact_cancelled());
          }
          let remaining = usize::try_from((size - uploaded).min(S3_MULTIPART_PART_BYTES as u64))
            .map_err(|_| backend_unavailable())?;
          let mut bytes = vec![0_u8; remaining];
          source
            .read_exact(&mut bytes)
            .map_err(|_| backend_unavailable())?;
          let part_hash = hex::encode(Sha256::digest(&bytes));
          let response = backend
            .send(
              Method::PUT,
              &key,
              vec![
                ("partNumber".to_string(), part_number.to_string()),
                ("uploadId".to_string(), upload_id.clone()),
              ],
              part_hash,
              Some(reqwest::Body::from(bytes)),
              BTreeMap::from([("content-length".to_string(), remaining.to_string())]),
            )
            .await?;
          if !response.status().is_success() {
            return Err(backend_unavailable());
          }
          let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .ok_or_else(backend_unavailable)?;
          parts.push((part_number, etag));
          uploaded += remaining as u64;
          part_number += 1;
        }
        let completion = format!(
          "<CompleteMultipartUpload>{}</CompleteMultipartUpload>",
          parts
            .iter()
            .map(|(number, etag)| format!(
              "<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>",
              xml_escape(etag)
            ))
            .collect::<String>()
        );
        let completion_hash = hex::encode(Sha256::digest(completion.as_bytes()));
        let response = backend
          .send(
            Method::POST,
            &key,
            vec![("uploadId".to_string(), upload_id.clone())],
            completion_hash,
            Some(reqwest::Body::from(completion)),
            BTreeMap::from([("content-type".to_string(), "application/xml".to_string())]),
          )
          .await?;
        if response.status().is_success() {
          Ok(())
        } else {
          Err(backend_unavailable())
        }
      }
      .await;
      if outcome.is_err() {
        let _ = backend
          .send(
            Method::DELETE,
            &key,
            vec![("uploadId".to_string(), upload_id)],
            empty_hash,
            None,
            BTreeMap::new(),
          )
          .await;
      }
      outcome
    })
  }

  fn download_verified(
    &self,
    key: &str,
    digest: &str,
    size: u64,
  ) -> Result<VerifiedArtifactBlob, CapabilityFailure> {
    let backend = self.clone();
    let key = key.to_string();
    let digest = digest.to_string();
    let path = self
      .staging
      .join(format!("{}.download", uuid::Uuid::new_v4().simple()));
    let download_path = path.clone();
    let outcome = self.run(async move {
      let empty_hash = hex::encode(Sha256::digest([]));
      let mut response = None;
      let mut final_status = None;
      for attempt in 0..S3_RECOVERY_ATTEMPTS {
        match backend
          .send(
            Method::GET,
            &key,
            vec![],
            empty_hash.clone(),
            None,
            BTreeMap::new(),
          )
          .await
        {
          Ok(candidate) if candidate.status().is_success() => {
            response = Some(candidate);
            break;
          }
          Ok(candidate) => final_status = Some(candidate.status()),
          Err(_) => final_status = None,
        }
        if attempt + 1 < S3_RECOVERY_ATTEMPTS {
          tokio::time::sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
        }
      }
      let mut response = response.ok_or_else(|| {
        if final_status == Some(StatusCode::NOT_FOUND) {
          artifact_corrupt()
        } else {
          backend_unavailable()
        }
      })?;
      let mut options = OpenOptions::new();
      options.create_new(true).write(true);
      #[cfg(unix)]
      {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
      }
      let mut target = options
        .open(&download_path)
        .map_err(|_| backend_unavailable())?;
      let mut hasher = Sha256::new();
      let mut received = 0_u64;
      while let Some(chunk) = response.chunk().await.map_err(|_| backend_unavailable())? {
        received = received.saturating_add(chunk.len() as u64);
        if received > size {
          return Err(artifact_corrupt());
        }
        hasher.update(&chunk);
        target
          .write_all(&chunk)
          .map_err(|_| backend_unavailable())?;
      }
      target.sync_all().map_err(|_| backend_unavailable())?;
      if received != size || hex::encode(hasher.finalize()) != digest {
        return Err(artifact_corrupt());
      }
      Ok(())
    });
    if let Err(error) = outcome {
      let _ = fs::remove_file(&path);
      return Err(error);
    }
    Ok(VerifiedArtifactBlob {
      path,
      temporary: true,
    })
  }

  fn list_inventory(&self, limit: usize) -> Result<Vec<String>, CapabilityFailure> {
    let backend = self.clone();
    let prefix = self.object_key("blobs/sha256/");
    self.run(async move {
      let empty_hash = hex::encode(Sha256::digest([]));
      for attempt in 0..S3_RECOVERY_ATTEMPTS {
        let response = backend
          .send(
            Method::GET,
            "",
            vec![
              ("list-type".to_string(), "2".to_string()),
              ("max-keys".to_string(), limit.min(1_000).to_string()),
              ("prefix".to_string(), prefix.clone()),
            ],
            empty_hash.clone(),
            None,
            BTreeMap::new(),
          )
          .await;
        match response {
          Ok(response) if response.status().is_success() => {
            let body = bounded_response_text(response, S3_CONTROL_RESPONSE_MAX_BYTES).await?;
            let mut digests = xml_values(&body, "Key")
              .into_iter()
              .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
              .map(|suffix| suffix.replace('/', ""))
              .filter(|digest| {
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
              })
              .take(limit)
              .collect::<Vec<_>>();
            digests.sort();
            return Ok(digests);
          }
          Ok(response)
            if !response.status().is_server_error()
              && response.status() != StatusCode::REQUEST_TIMEOUT
              && response.status() != StatusCode::TOO_MANY_REQUESTS =>
          {
            return Err(backend_unavailable());
          }
          Ok(_) | Err(_) => {}
        }
        if attempt + 1 < S3_RECOVERY_ATTEMPTS {
          tokio::time::sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
        }
      }
      Err(backend_unavailable())
    })
  }
}

impl ArtifactBackend for S3ArtifactBackend {
  fn identity(&self) -> &str {
    &self.identity
  }

  fn bind_store(&self, _connection: &Connection, store_id: &str) -> Result<(), CapabilityFailure> {
    let backend = self.clone();
    let key = self.object_key("store-id");
    let expected = format!("woml.artifact-store/v1\n{store_id}\n");
    self.run(async move {
      let empty_hash = hex::encode(Sha256::digest([]));
      let digest = hex::encode(Sha256::digest(expected.as_bytes()));
      for attempt in 0..S3_RECOVERY_ATTEMPTS {
        let get = backend
          .send(
            Method::GET,
            &key,
            vec![],
            empty_hash.clone(),
            None,
            BTreeMap::new(),
          )
          .await;
        match get {
          Ok(response) if response.status().is_success() => {
            let body = bounded_response_text(response, 1024).await?;
            return if body == expected {
              Ok(())
            } else {
              Err(backend_unavailable())
            };
          }
          Ok(response) if response.status() == StatusCode::NOT_FOUND => {
            let mut headers = backend.encryption_headers();
            headers.insert("if-none-match".to_string(), "*".to_string());
            headers.insert("content-length".to_string(), expected.len().to_string());
            if let Ok(response) = backend
              .send(
                Method::PUT,
                &key,
                vec![],
                digest.clone(),
                Some(reqwest::Body::from(expected.clone())),
                headers,
              )
              .await
            {
              if response.status().is_success() {
                return Ok(());
              }
              if response.status() != StatusCode::PRECONDITION_FAILED {
                return Err(backend_unavailable());
              }
            }
          }
          Ok(_) | Err(_) => {}
        }
        if attempt + 1 < S3_RECOVERY_ATTEMPTS {
          tokio::time::sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
        }
      }
      Err(backend_unavailable())
    })
  }

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
    cancellation: &CapabilityCancellationToken,
  ) -> Result<(), CapabilityFailure> {
    let file = stage.file.take().ok_or_else(backend_unavailable)?;
    file.sync_all().map_err(|_| backend_unavailable())?;
    drop(file);
    self.upload(
      &stage.path,
      &self.blob_key(sha256),
      sha256,
      size,
      cancellation,
    )?;
    let verified = self.verify(sha256, size)?;
    drop(verified);
    fs::remove_file(&stage.path).map_err(|_| backend_unavailable())
  }

  fn cleanup_startup(&self, connection: &Connection) -> Result<(), CapabilityFailure> {
    cleanup_directory_contents(&self.staging)?;
    for digest in self.inventory(ARTIFACT_ORPHAN_SCAN_BATCH)? {
      let reachable: bool = connection
        .query_row(
          "SELECT EXISTS(SELECT 1 FROM woml_artifact_blobs WHERE sha256 = ?1)",
          [&digest],
          |row| row.get(0),
        )
        .map_err(|_| backend_unavailable())?;
      if !reachable {
        self.delete(&digest)?;
      }
    }
    Ok(())
  }

  fn verify(&self, sha256: &str, size: u64) -> Result<VerifiedArtifactBlob, CapabilityFailure> {
    self.download_verified(&self.blob_key(sha256), sha256, size)
  }

  fn delete(&self, sha256: &str) -> Result<(), CapabilityFailure> {
    let backend = self.clone();
    let key = self.blob_key(sha256);
    self.run(async move {
      let empty_hash = hex::encode(Sha256::digest([]));
      for attempt in 0..S3_RECOVERY_ATTEMPTS {
        if let Ok(response) = backend
          .send(
            Method::DELETE,
            &key,
            vec![],
            empty_hash.clone(),
            None,
            BTreeMap::new(),
          )
          .await
        {
          if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            return Ok(());
          }
        }
        if attempt + 1 < S3_RECOVERY_ATTEMPTS {
          tokio::time::sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
        }
      }
      Err(backend_unavailable())
    })
  }

  fn inventory(&self, limit: usize) -> Result<Vec<String>, CapabilityFailure> {
    self.list_inventory(limit)
  }
}

fn streaming_request_body(
  path: PathBuf,
  cancellation: CapabilityCancellationToken,
) -> Result<reqwest::Body, CapabilityFailure> {
  let source = File::open(path).map_err(|_| backend_unavailable())?;
  let stream = futures_util::stream::try_unfold(
    (source, cancellation),
    |(mut source, cancellation)| async move {
      if cancellation.is_cancelled() {
        return Err(std::io::Error::new(
          std::io::ErrorKind::Interrupted,
          "artifact upload cancelled",
        ));
      }
      let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
      let read = source.read(&mut buffer)?;
      if read == 0 {
        return Ok(None);
      }
      buffer.truncate(read);
      Ok(Some((bytes::Bytes::from(buffer), (source, cancellation))))
    },
  );
  Ok(reqwest::Body::wrap_stream(stream))
}

async fn bounded_response_text(
  mut response: reqwest::Response,
  maximum_bytes: usize,
) -> Result<String, CapabilityFailure> {
  let mut bytes = Vec::new();
  while let Some(chunk) = response.chunk().await.map_err(|_| backend_unavailable())? {
    if bytes.len().saturating_add(chunk.len()) > maximum_bytes {
      return Err(backend_unavailable());
    }
    bytes.extend_from_slice(&chunk);
  }
  String::from_utf8(bytes).map_err(|_| backend_unavailable())
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> Result<Vec<u8>, CapabilityFailure> {
  let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| backend_unavailable())?;
  mac.update(value);
  Ok(mac.finalize().into_bytes().to_vec())
}

fn aws_percent_encode(value: &[u8]) -> String {
  let mut encoded = String::new();
  for byte in value {
    if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
      encoded.push(*byte as char);
    } else {
      encoded.push_str(&format!("%{byte:02X}"));
    }
  }
  encoded
}

fn xml_values(source: &str, tag: &str) -> Vec<String> {
  let start = format!("<{tag}>");
  let end = format!("</{tag}>");
  let mut values = Vec::new();
  let mut remaining = source;
  while let Some(offset) = remaining.find(&start) {
    let value_start = offset + start.len();
    let Some(value_end) = remaining[value_start..].find(&end) else {
      break;
    };
    values.push(xml_unescape(
      &remaining[value_start..value_start + value_end],
    ));
    remaining = &remaining[value_start + value_end + end.len()..];
  }
  values
}

fn xml_escape(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
    .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
  value
    .replace("&lt;", "<")
    .replace("&gt;", ">")
    .replace("&quot;", "\"")
    .replace("&apos;", "'")
    .replace("&amp;", "&")
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
      if existing.state_path == store.state_path
        && existing.backend.identity() == store.backend.identity()
      {
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

  pub(crate) fn operational_redactions(&self, invocation_id: &str) -> Vec<String> {
    let Ok(store) = self.configured() else {
      return Vec::new();
    };
    vec![store
      .invocation_materialization_directory(invocation_id)
      .to_string_lossy()
      .into_owned()]
  }

  /// Verifies an artifact reference for an immutable rich-approval review.
  /// This intentionally exposes metadata only; artifact bytes never enter the
  /// review event or provider payload.
  pub fn verify_review_reference(
    &self,
    run_id: &str,
    supplied: &ArtifactReferenceV1,
  ) -> Result<ArtifactReferenceV1, CapabilityFailure> {
    let store = self.configured()?;
    let artifact = store.authorize_reference(run_id, supplied)?;
    store
      .backend
      .verify(&artifact.reference.sha256, artifact.reference.size)?;
    Ok(artifact.reference)
  }

  /// Revalidates an artifact pinned by the durable rich-approval authority.
  /// The caller supplies only the immutable ID and digest stored in the pin;
  /// every remaining field, run grant, retention state, and backend byte
  /// digest is recovered and checked by the trusted artifact store.
  pub(crate) fn verify_policy_evidence(
    &self,
    run_id: &str,
    artifact_id: &str,
    expected_sha256: &str,
  ) -> Result<ArtifactReferenceV1, CapabilityFailure> {
    let store = self.configured()?;
    let connection = store.connection()?;
    validate_schema(&connection)?;
    let supplied = connection
      .query_row(
        "SELECT artifact_id, name, size, sha256, content_type
         FROM woml_artifacts WHERE artifact_id = ?1",
        [artifact_id],
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
      .optional()
      .map_err(|_| backend_unavailable())?
      .ok_or_else(artifact_not_found)?;
    if supplied.sha256 != expected_sha256 {
      return Err(artifact_access_denied());
    }
    let artifact = store.authorize_reference(run_id, &supplied)?;
    store
      .backend
      .verify(&artifact.reference.sha256, artifact.reference.size)?;
    Ok(artifact.reference)
  }
}

struct ArtifactStore {
  state_path: PathBuf,
  policy: Arc<PreparedArtifactPolicy>,
  backend: Arc<dyn ArtifactBackend>,
  materializations: PathBuf,
}

impl std::fmt::Debug for ArtifactStore {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ArtifactStore")
      .field("state_path", &self.state_path)
      .field("backend", &self.backend.identity())
      .finish()
  }
}

impl ArtifactStore {
  fn open(
    state_path: &Path,
    policy: Arc<PreparedArtifactPolicy>,
  ) -> Result<Self, CapabilityFailure> {
    Self::open_with_startup_cleanup(state_path, policy, true)
  }

  fn open_with_startup_cleanup(
    state_path: &Path,
    policy: Arc<PreparedArtifactPolicy>,
    startup_cleanup: bool,
  ) -> Result<Self, CapabilityFailure> {
    DurableEventStore::open(state_path).map_err(|_| backend_unavailable())?;
    let state_path = fs::canonicalize(state_path).map_err(|_| backend_unavailable())?;
    harden_file_permissions(&state_path)?;
    ensure_private_directory(&policy.materialization_directory)?;
    let materializations = fs::canonicalize(&policy.materialization_directory)
      .map_err(|_| materialization_unavailable())?;
    if startup_cleanup {
      cleanup_directory_contents(&materializations)?;
    }
    let backend: Arc<dyn ArtifactBackend> = match &policy.backend {
      PreparedArtifactBackend::Local { directory } => {
        Arc::new(LocalArtifactBackend::open(directory)?)
      }
      PreparedArtifactBackend::S3(config) => Arc::new(S3ArtifactBackend::open(
        config.as_ref().clone(),
        &materializations,
      )?),
    };
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
    if startup_cleanup {
      store.backend.cleanup_startup(&connection)?;
    }
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

  fn maintenance(
    &self,
    now: chrono::DateTime<Utc>,
    dry_run: bool,
  ) -> Result<ArtifactMaintenanceV1, CapabilityFailure> {
    let mut connection = self.connection()?;
    validate_schema(&connection)?;
    let candidates = artifact_retention_candidates(
      &connection,
      &now.to_rfc3339_opts(SecondsFormat::Millis, true),
      ARTIFACT_MAINTENANCE_BATCH,
    )?;
    let mut report = ArtifactMaintenanceV1 {
      profile: "woml.artifact-maintenance/v1".to_string(),
      kind: if dry_run { "plan" } else { "result" }.to_string(),
      completed_at: timestamp(),
      eligible_artifacts: candidates.len() as u64,
      eligible_blobs: candidates
        .iter()
        .map(|(_, digest, _)| digest)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64,
      estimated_bytes: candidates.iter().map(|(_, _, size)| *size).sum(),
      deleted_artifacts: 0,
      deleted_blobs: 0,
      deleted_bytes: 0,
      scrubbed_blobs: 0,
      corrupt_blobs: 0,
      collected_orphans: 0,
      live_artifacts: 0,
      live_blobs: 0,
      live_bytes: 0,
    };
    if !dry_run && !candidates.is_empty() {
      let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| backend_unavailable())?;
      let current = artifact_retention_candidates(
        &transaction,
        &now.to_rfc3339_opts(SecondsFormat::Millis, true),
        ARTIFACT_MAINTENANCE_BATCH,
      )?;
      let artifact_ids = current
        .iter()
        .map(|(artifact_id, _, _)| artifact_id.clone())
        .collect::<Vec<_>>();
      let digests = current
        .iter()
        .map(|(_, digest, size)| (digest.clone(), *size))
        .collect::<std::collections::BTreeMap<_, _>>();
      for artifact_id in &artifact_ids {
        transaction
          .execute(
            "DELETE FROM woml_artifact_transfers WHERE artifact_id = ?1",
            [artifact_id],
          )
          .map_err(|_| backend_unavailable())?;
        for table in [
          "woml_artifact_access",
          "woml_artifact_retention",
          "woml_artifact_provenance",
        ] {
          transaction
            .execute(
              &format!("DELETE FROM {table} WHERE artifact_id = ?1"),
              [artifact_id],
            )
            .map_err(|_| backend_unavailable())?;
        }
        transaction
          .execute(
            "DELETE FROM woml_artifacts WHERE artifact_id = ?1",
            [artifact_id],
          )
          .map_err(|_| backend_unavailable())?;
      }
      let mut unreachable = Vec::new();
      for (digest, size) in digests {
        let referenced: bool = transaction
          .query_row(
            "SELECT EXISTS(SELECT 1 FROM woml_artifacts WHERE sha256 = ?1)",
            [&digest],
            |row| row.get(0),
          )
          .map_err(|_| backend_unavailable())?;
        if !referenced {
          transaction
            .execute(
              "DELETE FROM woml_artifact_blobs WHERE sha256 = ?1",
              [&digest],
            )
            .map_err(|_| backend_unavailable())?;
          unreachable.push((digest, size));
        }
      }
      transaction.commit().map_err(|_| backend_unavailable())?;
      report.deleted_artifacts = artifact_ids.len() as u64;
      for (digest, size) in unreachable {
        self.backend.delete(&digest)?;
        report.deleted_blobs += 1;
        report.deleted_bytes = report.deleted_bytes.saturating_add(size);
      }
    }
    if !dry_run {
      let mut statement = connection
        .prepare("SELECT sha256, size FROM woml_artifact_blobs ORDER BY sha256 LIMIT ?1")
        .map_err(|_| backend_unavailable())?;
      let live = statement
        .query_map([ARTIFACT_SCRUB_BATCH as u64], |row| {
          Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })
        .map_err(|_| backend_unavailable())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| backend_unavailable())?;
      drop(statement);
      for (digest, size) in live {
        report.scrubbed_blobs += 1;
        if self.backend.verify(&digest, size).is_err() {
          connection
            .execute(
              "UPDATE woml_artifacts SET status = 'corrupt'
               WHERE sha256 = ?1 AND status != 'deleted'",
              [&digest],
            )
            .map_err(|_| backend_unavailable())?;
          report.corrupt_blobs += 1;
        }
      }
      for digest in self.backend.inventory(ARTIFACT_ORPHAN_SCAN_BATCH)? {
        let reachable: bool = connection
          .query_row(
            "SELECT EXISTS(SELECT 1 FROM woml_artifact_blobs WHERE sha256 = ?1)",
            [&digest],
            |row| row.get(0),
          )
          .map_err(|_| backend_unavailable())?;
        if !reachable {
          self.backend.delete(&digest)?;
          report.collected_orphans += 1;
        }
      }
    }
    let (live_artifacts, live_blobs, live_bytes): (u64, u64, u64) = connection
      .query_row(
        "SELECT
           (SELECT COUNT(*) FROM woml_artifacts WHERE status != 'deleted'),
           (SELECT COUNT(*) FROM woml_artifact_blobs),
           (SELECT COALESCE(SUM(size), 0) FROM woml_artifact_blobs)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .map_err(|_| backend_unavailable())?;
    report.live_artifacts = live_artifacts;
    report.live_blobs = live_blobs;
    report.live_bytes = live_bytes;
    report.completed_at = timestamp();
    if !dry_run {
      let encoded = serde_json::to_string(&report).map_err(|_| backend_unavailable())?;
      connection
        .execute(
          "INSERT INTO woml_artifact_store_metadata(key, value)
           VALUES ('last_maintenance_v1', ?1)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value",
          [encoded],
        )
        .map_err(|_| backend_unavailable())?;
    }
    Ok(report)
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
    self.backend.publish(stage, &sha256, size, cancellation)?;
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
    let source = self
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
        &source.path,
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
    let stored: Option<StoredArtifactRow> = connection
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
    let policy = Arc::new(PreparedArtifactPolicy::prepare(
      policy,
      mode,
      resolved_secrets,
    )?);
    let concurrency = Arc::new(Semaphore::new(policy.limits.max_concurrent_transfers));
    let mut known_secrets = resolved_secrets
      .values()
      .map(|secret| secret.as_bytes().to_vec())
      .filter(|secret| secret.len() >= policy.minimum_secret_bytes)
      .collect::<Vec<_>>();
    if let PreparedArtifactBackend::S3(backend) = &policy.backend {
      for value in [
        Some(&backend.credentials.access_key),
        Some(&backend.credentials.secret_key),
        backend.credentials.session_token.as_ref(),
      ]
      .into_iter()
      .flatten()
      {
        if value.len() >= policy.minimum_secret_bytes {
          let bytes = value.as_bytes().to_vec();
          if !known_secrets.contains(&bytes) {
            known_secrets.push(bytes);
          }
        }
      }
    }
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

pub fn maintain_artifacts(
  state_path: impl AsRef<Path>,
  policy: ArtifactPolicyV1,
  mode: ArtifactRuntimeMode,
  resolved_secrets: &BTreeMap<String, String>,
  now: chrono::DateTime<Utc>,
  dry_run: bool,
) -> Result<ArtifactMaintenanceV1, CapabilityFailure> {
  let managed = Arc::new(ManagedArtifactStore::default());
  let handler =
    ManagedArtifactPutHandler::new(Arc::clone(&managed), policy, mode, resolved_secrets).map_err(
      |error| {
        artifact_failure(
          CapabilityFailureKind::InvalidInput,
          "WOML_ARTIFACT_POLICY_INVALID",
          &error.to_string(),
          false,
          false,
        )
      },
    )?;
  managed.configure_for_state(state_path.as_ref(), handler.policy())?;
  managed.configured()?.maintenance(now, dry_run)
}

const OPERATOR_ARTIFACT_INSPECTION_LIMIT: usize = 100;

pub fn inspect_artifact(
  state_path: impl AsRef<Path>,
  artifact_id: &str,
) -> Result<ArtifactInspectionV1, CapabilityFailure> {
  validate_artifact_id(artifact_id)?;
  DurableEventStore::open(state_path.as_ref()).map_err(|_| backend_unavailable())?;
  let connection = Connection::open_with_flags(
    state_path.as_ref(),
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
  )
  .map_err(|_| backend_unavailable())?;
  initialize_schema(&connection)?;
  validate_schema(&connection)?;
  let inspection =
    artifact_inspection_row(&connection, artifact_id)?.ok_or_else(artifact_not_found)?;
  record_operator_audit(&connection, "inspect", artifact_id)?;
  Ok(inspection)
}

pub fn inspect_run_artifacts(
  state_path: impl AsRef<Path>,
  run_id: &str,
) -> Result<ArtifactRunInspectionV1, CapabilityFailure> {
  if run_id.is_empty() || run_id.len() > 320 {
    return Err(artifact_access_denied());
  }
  DurableEventStore::open(state_path.as_ref()).map_err(|_| backend_unavailable())?;
  let connection = Connection::open_with_flags(
    state_path.as_ref(),
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
  )
  .map_err(|_| backend_unavailable())?;
  initialize_schema(&connection)?;
  validate_schema(&connection)?;
  let mut statement = connection
    .prepare(
      "SELECT artifact_id FROM woml_artifact_provenance
       WHERE run_id = ?1 ORDER BY artifact_id LIMIT ?2",
    )
    .map_err(|_| backend_unavailable())?;
  let artifact_ids = statement
    .query_map(
      params![run_id, (OPERATOR_ARTIFACT_INSPECTION_LIMIT + 1) as u64],
      |row| row.get::<_, String>(0),
    )
    .map_err(|_| backend_unavailable())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|_| backend_unavailable())?;
  let truncated = artifact_ids.len() > OPERATOR_ARTIFACT_INSPECTION_LIMIT;
  let artifacts = artifact_ids
    .into_iter()
    .take(OPERATOR_ARTIFACT_INSPECTION_LIMIT)
    .map(|artifact_id| {
      artifact_inspection_row(&connection, &artifact_id)?.ok_or_else(artifact_corrupt)
    })
    .collect::<Result<Vec<_>, _>>()?;
  record_operator_audit(&connection, "inspect_run", run_id)?;
  Ok(ArtifactRunInspectionV1 {
    profile: "woml.artifact-run-inspection/v1".to_string(),
    run_id: run_id.to_string(),
    artifacts,
    truncated,
  })
}

pub fn export_artifact(
  state_path: impl AsRef<Path>,
  destination: impl AsRef<Path>,
  artifact_id: &str,
  policy: ArtifactPolicyV1,
  mode: ArtifactRuntimeMode,
  resolved_secrets: &BTreeMap<String, String>,
) -> Result<ArtifactExportV1, CapabilityFailure> {
  validate_artifact_id(artifact_id)?;
  let policy = Arc::new(
    PreparedArtifactPolicy::prepare(policy, mode, resolved_secrets).map_err(|error| {
      artifact_failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_ARTIFACT_POLICY_INVALID",
        &error.to_string(),
        false,
        false,
      )
    })?,
  );
  let store = ArtifactStore::open_with_startup_cleanup(state_path.as_ref(), policy, false)?;
  let connection = store.connection()?;
  let inspection =
    artifact_inspection_row(&connection, artifact_id)?.ok_or_else(artifact_not_found)?;
  if inspection.status != "available" {
    return Err(artifact_access_denied());
  }
  let verified = store
    .backend
    .verify(&inspection.artifact.sha256, inspection.artifact.size)?;
  let destination = destination.as_ref();
  let parent = destination
    .parent()
    .ok_or_else(materialization_unavailable)?;
  let parent = fs::canonicalize(parent).map_err(|_| materialization_unavailable())?;
  let file_name = destination
    .file_name()
    .and_then(|value| value.to_str())
    .filter(|value| !value.is_empty() && *value != "." && *value != "..")
    .ok_or_else(materialization_unavailable)?;
  let final_path = parent.join(file_name);
  if fs::symlink_metadata(&final_path).is_ok() {
    return Err(artifact_failure(
      CapabilityFailureKind::ServiceRejected,
      "WOML_ARTIFACT_EXPORT_EXISTS",
      "The export destination already exists; choose a new path.",
      false,
      false,
    ));
  }
  stream_materialization(
    &verified.path,
    &final_path,
    &inspection.artifact.sha256,
    inspection.artifact.size,
    &CapabilityCancellationToken::default(),
  )?;
  harden_file_permissions(&final_path)?;
  let audit_id = record_operator_audit(&connection, "export", artifact_id)?;
  Ok(ArtifactExportV1 {
    profile: "woml.artifact-export/v1".to_string(),
    audit_id,
    artifact_id: inspection.artifact.id,
    name: inspection.artifact.name,
    size: inspection.artifact.size,
    digest_prefix: inspection.digest_prefix,
    exported_at: timestamp(),
    verified: true,
  })
}

pub fn observe_artifacts(
  state_path: impl AsRef<Path>,
  policy: ArtifactPolicyV1,
  mode: ArtifactRuntimeMode,
  resolved_secrets: &BTreeMap<String, String>,
) -> Result<ArtifactObservationV1, CapabilityFailure> {
  let policy = Arc::new(
    PreparedArtifactPolicy::prepare(policy, mode, resolved_secrets).map_err(|error| {
      artifact_failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_ARTIFACT_POLICY_INVALID",
        &error.to_string(),
        false,
        false,
      )
    })?,
  );
  let backend_name = match &policy.backend {
    PreparedArtifactBackend::Local { .. } => "local",
    PreparedArtifactBackend::S3(_) => "s3",
  };
  let store = ArtifactStore::open_with_startup_cleanup(state_path.as_ref(), policy, false)?;
  let connection = store.connection()?;
  let (transfers, transferred_bytes, transfer_duration_ms, failed_transfers): (u64, u64, u64, u64) = connection
    .query_row(
      "SELECT COUNT(*),
              COALESCE(SUM(CASE WHEN status = 'succeeded' THEN size ELSE 0 END), 0),
              COALESCE(CAST(SUM(CASE WHEN settled_at IS NOT NULL
                THEN (julianday(settled_at) - julianday(started_at)) * 86400000
                ELSE 0 END) AS INTEGER), 0),
              COALESCE(SUM(CASE WHEN status IN ('failed','cancelled','interrupted') THEN 1 ELSE 0 END), 0)
       FROM woml_artifact_transfers",
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .map_err(|_| backend_unavailable())?;
  let (corrupt_artifacts, live_artifacts, live_bytes): (u64, u64, u64) = connection
    .query_row(
      "SELECT
         COALESCE(SUM(CASE WHEN status = 'corrupt' THEN 1 ELSE 0 END), 0),
         COALESCE(SUM(CASE WHEN status = 'available' THEN 1 ELSE 0 END), 0),
         COALESCE(SUM(CASE WHEN status = 'available' THEN size ELSE 0 END), 0)
       FROM woml_artifacts",
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|_| backend_unavailable())?;
  let orphan_blobs = connection
    .query_row(
      "SELECT COUNT(*) FROM woml_artifact_blobs AS blobs
       WHERE NOT EXISTS (SELECT 1 FROM woml_artifacts AS artifacts WHERE artifacts.sha256 = blobs.sha256)",
      [],
      |row| row.get::<_, u64>(0),
    )
    .map_err(|_| backend_unavailable())?;
  let backend_healthy = store.backend.inventory(1).is_ok();
  Ok(ArtifactObservationV1 {
    profile: "woml.artifact-observation/v1".to_string(),
    backend: backend_name.to_string(),
    backend_healthy,
    transfers,
    transferred_bytes,
    transfer_duration_ms,
    failed_transfers,
    corrupt_artifacts,
    orphan_blobs,
    live_artifacts,
    live_bytes,
  })
}

fn artifact_inspection_row(
  connection: &Connection,
  artifact_id: &str,
) -> Result<Option<ArtifactInspectionV1>, CapabilityFailure> {
  connection
    .query_row(
      "SELECT a.artifact_id, a.name, a.size, a.sha256, a.content_type,
              a.status, a.created_at, retention.expires_at, retention.effective_ms,
              provenance.workflow_id, provenance.run_id, provenance.node_id,
              provenance.attempt_number
       FROM woml_artifacts AS a
       JOIN woml_artifact_retention AS retention ON retention.artifact_id = a.artifact_id
       JOIN woml_artifact_provenance AS provenance ON provenance.artifact_id = a.artifact_id
       WHERE a.artifact_id = ?1",
      [artifact_id],
      |row| {
        let digest: String = row.get(3)?;
        Ok(ArtifactInspectionV1 {
          profile: "woml.artifact-inspection/v1".to_string(),
          artifact: ArtifactReferenceV1 {
            contract: ARTIFACT_REFERENCE_CONTRACT.to_string(),
            contract_version: ARTIFACT_REFERENCE_CONTRACT_VERSION,
            id: row.get(0)?,
            name: row.get(1)?,
            size: row.get(2)?,
            sha256: digest.clone(),
            content_type: row.get(4)?,
          },
          digest_prefix: digest.chars().take(12).collect(),
          status: row.get(5)?,
          created_at: row.get(6)?,
          expires_at: row.get(7)?,
          retention_ms: row.get(8)?,
          provenance: ArtifactProvenanceV1 {
            workflow_id: row.get(9)?,
            run_id: row.get(10)?,
            node_id: row.get(11)?,
            attempt: row.get(12)?,
          },
        })
      },
    )
    .optional()
    .map_err(|_| backend_unavailable())
}

fn record_operator_audit(
  connection: &Connection,
  operation: &str,
  subject_id: &str,
) -> Result<String, CapabilityFailure> {
  let audit_id = format!("audit_{}", uuid::Uuid::new_v4().simple());
  connection
    .execute(
      "INSERT INTO woml_artifact_operator_audit(audit_id, operation, subject_id, occurred_at)
       VALUES (?1, ?2, ?3, ?4)",
      params![audit_id, operation, subject_id, timestamp()],
    )
    .map_err(|_| backend_unavailable())?;
  Ok(audit_id)
}

fn validate_artifact_id(artifact_id: &str) -> Result<(), CapabilityFailure> {
  let encoded = artifact_id.strip_prefix("artifact_").unwrap_or_default();
  if encoded.len() != 26 || !encoded.bytes().all(|byte| CROCKFORD.contains(&byte)) {
    return Err(artifact_access_denied());
  }
  Ok(())
}

pub fn export_artifact_backup(
  state_path: impl AsRef<Path>,
  destination: impl AsRef<Path>,
  policy: ArtifactPolicyV1,
  mode: ArtifactRuntimeMode,
  resolved_secrets: &BTreeMap<String, String>,
) -> Result<ArtifactBackupInventoryV1, CapabilityFailure> {
  let policy = Arc::new(
    PreparedArtifactPolicy::prepare(policy, mode, resolved_secrets).map_err(|error| {
      artifact_failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_ARTIFACT_POLICY_INVALID",
        &error.to_string(),
        false,
        false,
      )
    })?,
  );
  let store = ArtifactStore::open_with_startup_cleanup(state_path.as_ref(), policy, false)?;
  let destination = destination.as_ref();
  ensure_private_directory(destination)?;
  if fs::read_dir(destination)
    .map_err(|_| backend_unavailable())?
    .next()
    .is_some()
  {
    return Err(backend_unavailable());
  }
  let connection = store.connection()?;
  let inventory = artifact_blob_inventory(&connection)?;
  let cancellation = CapabilityCancellationToken::default();
  for (digest, size) in &inventory {
    let source = store.backend.verify(digest, *size)?;
    let target = destination
      .join("blobs/sha256")
      .join(&digest[..2])
      .join(&digest[2..]);
    ensure_private_directory(target.parent().ok_or_else(backend_unavailable)?)?;
    stream_materialization(&source.path, &target, digest, *size, &cancellation)?;
  }
  sync_directory(destination)?;
  Ok(artifact_backup_inventory(&inventory))
}

pub fn restore_artifact_backup(
  state_path: impl AsRef<Path>,
  source: impl AsRef<Path>,
  policy: ArtifactPolicyV1,
  mode: ArtifactRuntimeMode,
  resolved_secrets: &BTreeMap<String, String>,
) -> Result<ArtifactBackupInventoryV1, CapabilityFailure> {
  let policy = Arc::new(
    PreparedArtifactPolicy::prepare(policy, mode, resolved_secrets).map_err(|error| {
      artifact_failure(
        CapabilityFailureKind::InvalidInput,
        "WOML_ARTIFACT_POLICY_INVALID",
        &error.to_string(),
        false,
        false,
      )
    })?,
  );
  let store = ArtifactStore::open_with_startup_cleanup(state_path.as_ref(), policy, false)?;
  let connection = store.connection()?;
  let inventory = artifact_blob_inventory(&connection)?;
  let source = fs::canonicalize(source.as_ref()).map_err(|_| backend_unavailable())?;
  let metadata = fs::symlink_metadata(&source).map_err(|_| backend_unavailable())?;
  if !metadata.is_dir() || metadata.file_type().is_symlink() || is_windows_reparse(&metadata) {
    return Err(backend_unavailable());
  }
  let cancellation = CapabilityCancellationToken::default();
  for (digest, size) in &inventory {
    let backup_path = source
      .join("blobs/sha256")
      .join(&digest[..2])
      .join(&digest[2..]);
    verify_materialized_file(&backup_path, digest, *size)?;
    let mut stage = store.backend.begin_stage()?;
    let target = stage.file.as_mut().ok_or_else(backend_unavailable)?;
    let mut input = File::open(&backup_path).map_err(|_| backend_unavailable())?;
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    loop {
      let read = input.read(&mut buffer).map_err(|_| backend_unavailable())?;
      if read == 0 {
        break;
      }
      target
        .write_all(&buffer[..read])
        .map_err(|_| backend_unavailable())?;
    }
    store.backend.publish(stage, digest, *size, &cancellation)?;
  }
  for (digest, size) in &inventory {
    drop(store.backend.verify(digest, *size)?);
  }
  Ok(artifact_backup_inventory(&inventory))
}

pub fn verify_artifact_backup(
  state_path: impl AsRef<Path>,
  source: impl AsRef<Path>,
) -> Result<ArtifactBackupInventoryV1, CapabilityFailure> {
  DurableEventStore::open(state_path.as_ref()).map_err(|_| backend_unavailable())?;
  let connection = Connection::open_with_flags(
    state_path.as_ref(),
    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
  )
  .map_err(|_| backend_unavailable())?;
  let inventory = artifact_blob_inventory(&connection)?;
  let source = fs::canonicalize(source.as_ref()).map_err(|_| backend_unavailable())?;
  let metadata = fs::symlink_metadata(&source).map_err(|_| backend_unavailable())?;
  if !metadata.is_dir() || metadata.file_type().is_symlink() || is_windows_reparse(&metadata) {
    return Err(backend_unavailable());
  }
  for (digest, size) in &inventory {
    verify_materialized_file(
      &source
        .join("blobs/sha256")
        .join(&digest[..2])
        .join(&digest[2..]),
      digest,
      *size,
    )?;
  }
  let expected = inventory
    .iter()
    .map(|(digest, _)| digest.clone())
    .collect::<std::collections::BTreeSet<_>>();
  let mut actual = std::collections::BTreeSet::new();
  let blobs = source.join("blobs/sha256");
  if blobs.exists() {
    for prefix in fs::read_dir(&blobs).map_err(|_| backend_unavailable())? {
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
        actual.insert(format!(
          "{}{}",
          prefix.file_name().to_string_lossy(),
          entry.file_name().to_string_lossy()
        ));
      }
    }
  }
  if actual != expected {
    return Err(artifact_corrupt());
  }
  Ok(artifact_backup_inventory(&inventory))
}

fn artifact_blob_inventory(
  connection: &Connection,
) -> Result<Vec<(String, u64)>, CapabilityFailure> {
  validate_schema(connection)?;
  let mut statement = connection
    .prepare("SELECT sha256, size FROM woml_artifact_blobs ORDER BY sha256")
    .map_err(|_| backend_unavailable())?;
  let inventory = statement
    .query_map([], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })
    .map_err(|_| backend_unavailable())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|_| backend_unavailable())?;
  Ok(inventory)
}

fn artifact_backup_inventory(inventory: &[(String, u64)]) -> ArtifactBackupInventoryV1 {
  let mut digest = Sha256::new();
  digest.update(b"woml.artifact-backup-inventory\0v1\0");
  for (sha256, size) in inventory {
    digest.update((sha256.len() as u64).to_be_bytes());
    digest.update(sha256.as_bytes());
    digest.update(size.to_be_bytes());
  }
  ArtifactBackupInventoryV1 {
    profile: "woml.artifact-backup-inventory/v1".to_string(),
    directory: "artifacts".to_string(),
    blob_count: inventory.len() as u64,
    total_bytes: inventory.iter().map(|(_, size)| *size).sum(),
    inventory_digest: format!("sha256:{}", hex::encode(digest.finalize())),
    verified: true,
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
    resolved_secrets: &BTreeMap<String, String>,
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
    let backend = match &policy.backend {
      ArtifactBackendPolicy::Local {
        contract,
        contract_version,
        directory,
      } if contract == "woml.artifact-backend" && *contract_version == 1 => {
        let authored = Path::new(directory);
        let directory = if authored.is_absolute() {
          authored.to_path_buf()
        } else {
          project_root.join(authored)
        };
        PreparedArtifactBackend::Local { directory }
      }
      ArtifactBackendPolicy::Local { .. } => {
        return Err(ArtifactPolicyError::Invalid(
          "the backend does not match Artifact Backend v1".to_string(),
        ))
      }
      ArtifactBackendPolicy::S3 {
        contract,
        contract_version,
        bucket,
        region,
        prefix,
        endpoint,
        force_path_style,
        authentication,
        server_side_encryption,
      } if contract == "woml.artifact-backend" && *contract_version == 1 => {
        PreparedArtifactBackend::S3(Box::new(prepare_s3_backend(
          bucket,
          region,
          prefix,
          endpoint.as_deref(),
          *force_path_style,
          authentication,
          server_side_encryption,
          resolved_secrets,
        )?))
      }
      ArtifactBackendPolicy::S3 { .. } => {
        return Err(ArtifactPolicyError::Invalid(
          "the S3 backend does not match Artifact Backend v1".to_string(),
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
      backend,
      materialization_directory,
      max_materialization_bytes_per_attempt: policy.materialization.max_bytes_per_attempt,
      limits: policy.limits,
      retention: policy.retention,
      secret_action: policy.secret_scanning.known_secret_action,
      minimum_secret_bytes: policy.secret_scanning.minimum_secret_bytes,
    })
  }
}

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
enum AuthoredS3Authentication {
  Ambient,
  Secrets {
    #[serde(rename = "accessKeySecret")]
    access_key_secret: String,
    #[serde(rename = "secretKeySecret")]
    secret_key_secret: String,
    #[serde(rename = "sessionTokenSecret")]
    session_token_secret: Option<String>,
  },
}

#[derive(Deserialize)]
#[serde(tag = "mode", deny_unknown_fields)]
enum AuthoredS3Encryption {
  #[serde(rename = "aes256")]
  Aes256,
  #[serde(rename = "aws-kms")]
  AwsKms {
    #[serde(rename = "keyId")]
    key_id: String,
  },
}

#[allow(clippy::too_many_arguments)]
fn prepare_s3_backend(
  bucket: &str,
  region: &str,
  prefix: &str,
  endpoint: Option<&str>,
  force_path_style: bool,
  authentication: &Value,
  server_side_encryption: &Value,
  resolved_secrets: &BTreeMap<String, String>,
) -> Result<PreparedS3Backend, ArtifactPolicyError> {
  if bucket.len() < 3
    || bucket.len() > 63
    || !bucket.bytes().all(|byte| {
      byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
    })
    || !bucket
      .as_bytes()
      .first()
      .is_some_and(u8::is_ascii_alphanumeric)
    || !bucket
      .as_bytes()
      .last()
      .is_some_and(u8::is_ascii_alphanumeric)
    || region.is_empty()
    || region.len() > 128
    || prefix.len() > 512
    || !prefix
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || b"!_.*'()/-".contains(&byte))
  {
    return Err(ArtifactPolicyError::Invalid(
      "the S3 bucket, region, or prefix is invalid".to_string(),
    ));
  }
  let endpoint = endpoint
    .map(str::to_string)
    .unwrap_or_else(|| format!("https://s3.{region}.amazonaws.com"));
  let endpoint = Url::parse(&endpoint)
    .map_err(|_| ArtifactPolicyError::Invalid("the S3 endpoint is not a valid URL".to_string()))?;
  if endpoint.scheme() != "https"
    || endpoint.username() != ""
    || endpoint.password().is_some()
    || endpoint.query().is_some()
    || endpoint.fragment().is_some()
  {
    return Err(ArtifactPolicyError::Invalid(
      "the S3 endpoint must be a credential-free HTTPS URL".to_string(),
    ));
  }
  let authentication: AuthoredS3Authentication = serde_json::from_value(authentication.clone())
    .map_err(|_| {
      ArtifactPolicyError::Invalid("the S3 authentication contract is invalid".to_string())
    })?;
  let secret = |name: &str| {
    resolved_secrets
      .get(name)
      .filter(|value| !value.is_empty() && value.len() <= 16_384)
      .cloned()
      .ok_or_else(|| {
        ArtifactPolicyError::Invalid(format!("the S3 credential secret {name} is unavailable"))
      })
  };
  let credentials = match authentication {
    AuthoredS3Authentication::Ambient => S3Credentials {
      access_key: std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| {
        ArtifactPolicyError::Invalid("ambient AWS_ACCESS_KEY_ID is unavailable".to_string())
      })?,
      secret_key: std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| {
        ArtifactPolicyError::Invalid("ambient AWS_SECRET_ACCESS_KEY is unavailable".to_string())
      })?,
      session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
    },
    AuthoredS3Authentication::Secrets {
      access_key_secret,
      secret_key_secret,
      session_token_secret,
    } => S3Credentials {
      access_key: secret(&access_key_secret)?,
      secret_key: secret(&secret_key_secret)?,
      session_token: session_token_secret.as_deref().map(secret).transpose()?,
    },
  };
  if credentials.access_key.is_empty() || credentials.secret_key.is_empty() {
    return Err(ArtifactPolicyError::Invalid(
      "the S3 credentials are empty".to_string(),
    ));
  }
  let encryption: AuthoredS3Encryption = serde_json::from_value(server_side_encryption.clone())
    .map_err(|_| {
      ArtifactPolicyError::Invalid("the S3 encryption contract is invalid".to_string())
    })?;
  let server_side_encryption = match encryption {
    AuthoredS3Encryption::Aes256 => S3ServerSideEncryption::Aes256,
    AuthoredS3Encryption::AwsKms { key_id } if !key_id.is_empty() && key_id.len() <= 2_048 => {
      S3ServerSideEncryption::AwsKms { key_id }
    }
    AuthoredS3Encryption::AwsKms { .. } => {
      return Err(ArtifactPolicyError::Invalid(
        "the S3 KMS key ID is invalid".to_string(),
      ))
    }
  };
  Ok(PreparedS3Backend {
    bucket: bucket.to_string(),
    region: region.to_string(),
    prefix: prefix.to_string(),
    endpoint,
    force_path_style,
    credentials,
    server_side_encryption,
  })
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
       );
       CREATE TABLE IF NOT EXISTS woml_artifact_operator_audit(
         audit_id TEXT PRIMARY KEY NOT NULL,
         operation TEXT NOT NULL CHECK(operation IN ('inspect','inspect_run','export')),
         subject_id TEXT NOT NULL,
         occurred_at TEXT NOT NULL
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

fn artifact_retention_candidates(
  connection: &Connection,
  now: &str,
  limit: usize,
) -> Result<Vec<(String, String, u64)>, CapabilityFailure> {
  let mut statement = connection
    .prepare(
      "SELECT a.artifact_id, a.sha256, a.size
       FROM woml_artifacts AS a
       JOIN woml_artifact_retention AS retention
         ON retention.artifact_id = a.artifact_id
       WHERE retention.expires_at <= ?1
         AND a.status IN ('available','quarantined','expired','corrupt')
         AND NOT EXISTS(
           SELECT 1 FROM woml_approval_artifact_pins AS pins
           WHERE pins.artifact_id = a.artifact_id AND pins.released_at IS NULL
         )
         AND NOT EXISTS(
           SELECT 1
           FROM woml_artifact_access AS access
           JOIN woml_runs AS runs ON runs.run_id = access.run_id
           LEFT JOIN woml_run_summaries AS summaries ON summaries.run_id = runs.run_id
           WHERE access.artifact_id = a.artifact_id
             AND access.revoked_at IS NULL
             AND (
               summaries.run_id IS NULL
               OR summaries.status NOT IN ('succeeded','failed','cancelled')
             )
         )
       ORDER BY retention.expires_at, a.artifact_id
       LIMIT ?2",
    )
    .map_err(|_| backend_unavailable())?;
  let candidates = statement
    .query_map(params![now, limit as u64], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, u64>(2)?,
      ))
    })
    .map_err(|_| backend_unavailable())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|_| backend_unavailable())?;
  Ok(candidates)
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

#[cfg(unix)]
fn harden_file_permissions(path: &Path) -> Result<(), CapabilityFailure> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|_| backend_unavailable())?;
  Ok(())
}

#[cfg(not(unix))]
fn harden_file_permissions(_path: &Path) -> Result<(), CapabilityFailure> {
  Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), CapabilityFailure> {
  File::open(path)
    .and_then(|file| file.sync_all())
    .map_err(|_| backend_unavailable())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), CapabilityFailure> {
  // Windows does not support opening a directory as a regular File for
  // sync_all. File contents are still flushed before atomic publication.
  Ok(())
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
  use std::{
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread::JoinHandle,
  };

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

  #[derive(Debug, Clone)]
  struct MockS3Request {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
  }

  struct MockS3Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<MockS3Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
  }

  impl MockS3Server {
    fn start() -> Self {
      let listener = TcpListener::bind("127.0.0.1:0").unwrap();
      listener.set_nonblocking(true).unwrap();
      let address = listener.local_addr().unwrap();
      let objects = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
      let requests = Arc::new(Mutex::new(Vec::new()));
      let stop = Arc::new(AtomicBool::new(false));
      let failed_blob_puts = Arc::new(AtomicUsize::new(1));
      let invisible_blob_reads = Arc::new(AtomicUsize::new(1));
      let failed_store_id_reads = Arc::new(AtomicUsize::new(1));
      let failed_deletes = Arc::new(AtomicUsize::new(1));
      let failed_inventory_reads = Arc::new(AtomicUsize::new(1));
      let thread_objects = Arc::clone(&objects);
      let thread_requests = Arc::clone(&requests);
      let thread_stop = Arc::clone(&stop);
      let thread = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
          match listener.accept() {
            Ok((stream, _)) => handle_mock_s3_connection(
              stream,
              &thread_objects,
              &thread_requests,
              &failed_blob_puts,
              &invisible_blob_reads,
              &failed_store_id_reads,
              &failed_deletes,
              &failed_inventory_reads,
            ),
            Err(_) => {
              // A nonblocking Windows listener can surface an aborted or
              // reset connection from a client pool through accept(). Those
              // errors affect only that connection and must not terminate the
              // mock S3 service before a rotated client reconnects. The stop
              // flag, not a connection-scoped accept error, owns shutdown.
              std::thread::sleep(Duration::from_millis(2));
            }
          }
        }
      });
      Self {
        endpoint: format!("http://{address}"),
        requests,
        stop,
        thread: Some(thread),
      }
    }
  }

  impl Drop for MockS3Server {
    fn drop(&mut self) {
      self.stop.store(true, Ordering::SeqCst);
      let _ = TcpStream::connect(self.endpoint.trim_start_matches("http://"));
      if let Some(thread) = self.thread.take() {
        let _ = thread.join();
      }
    }
  }

  fn handle_mock_s3_connection(
    mut stream: TcpStream,
    objects: &Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    requests: &Arc<Mutex<Vec<MockS3Request>>>,
    failed_blob_puts: &AtomicUsize,
    invisible_blob_reads: &AtomicUsize,
    failed_store_id_reads: &AtomicUsize,
    failed_deletes: &AtomicUsize,
    failed_inventory_reads: &AtomicUsize,
  ) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut received = Vec::new();
    let mut buffer = [0_u8; 8192];
    let (header_end, content_length) = loop {
      let Ok(read) = stream.read(&mut buffer) else {
        return;
      };
      if read == 0 {
        return;
      }
      received.extend_from_slice(&buffer[..read]);
      if received.len() > 2 * 1024 * 1024 {
        return;
      }
      if let Some(position) = received.windows(4).position(|value| value == b"\r\n\r\n") {
        let header_end = position + 4;
        let headers = String::from_utf8_lossy(&received[..position]);
        let content_length = headers
          .lines()
          .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name
              .eq_ignore_ascii_case("content-length")
              .then(|| value.trim().parse::<usize>().ok())
              .flatten()
          })
          .unwrap_or(0);
        break (header_end, content_length);
      }
    };
    while received.len() < header_end + content_length {
      let Ok(read) = stream.read(&mut buffer) else {
        return;
      };
      if read == 0 {
        return;
      }
      received.extend_from_slice(&buffer[..read]);
    }
    let head = String::from_utf8_lossy(&received[..header_end - 4]);
    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
      return;
    };
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let target = request_parts.next().unwrap_or_default().to_string();
    let headers = lines
      .filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        Some((name.to_ascii_lowercase(), value.trim().to_string()))
      })
      .collect::<BTreeMap<_, _>>();
    requests.lock().unwrap().push(MockS3Request {
      method: method.clone(),
      target: target.clone(),
      headers: headers.clone(),
    });
    let body = received[header_end..header_end + content_length].to_vec();
    let path = target.split('?').next().unwrap_or_default();
    let key = path.strip_prefix("/woml-test/").unwrap_or_default();
    let is_blob = key.contains("blobs/sha256/");

    if method == "GET" && key.ends_with("store-id") && decrement_if_positive(failed_store_id_reads)
    {
      write_mock_response(&mut stream, 503, "", &[]);
      return;
    }
    if method == "DELETE" && decrement_if_positive(failed_deletes) {
      write_mock_response(&mut stream, 503, "", &[]);
      return;
    }
    if method == "GET"
      && target.contains("list-type=2")
      && decrement_if_positive(failed_inventory_reads)
    {
      write_mock_response(&mut stream, 503, "", &[]);
      return;
    }

    if method == "GET" && target.contains("list-type=2") {
      let prefix = target
        .split('?')
        .nth(1)
        .unwrap_or_default()
        .split('&')
        .find_map(|part| part.strip_prefix("prefix="))
        .map(percent_decode_test)
        .unwrap_or_default();
      let contents = objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .map(|key| format!("<Contents><Key>{key}</Key></Contents>"))
        .collect::<String>();
      write_mock_response(
        &mut stream,
        200,
        &format!("<ListBucketResult>{contents}</ListBucketResult>"),
        &[],
      );
      return;
    }
    if method == "PUT" && is_blob && decrement_if_positive(failed_blob_puts) {
      write_mock_response(&mut stream, 503, "", &[]);
      return;
    }
    match method.as_str() {
      "PUT" => {
        let mut objects = objects.lock().unwrap();
        if headers.get("if-none-match").map(String::as_str) == Some("*")
          && objects.contains_key(key)
        {
          write_mock_response(&mut stream, 412, "", &[]);
        } else {
          objects.insert(key.to_string(), body);
          write_mock_response(&mut stream, 200, "", &[("ETag", "\"mock\"")]);
        }
      }
      "GET" => {
        if is_blob && decrement_if_positive(invisible_blob_reads) {
          write_mock_response(&mut stream, 404, "", &[]);
          return;
        }
        match objects.lock().unwrap().get(key).cloned() {
          Some(value) => write_mock_bytes(&mut stream, 200, &value, &[]),
          None => write_mock_response(&mut stream, 404, "", &[]),
        }
      }
      "DELETE" => {
        objects.lock().unwrap().remove(key);
        write_mock_response(&mut stream, 204, "", &[]);
      }
      _ => write_mock_response(&mut stream, 400, "", &[]),
    }
  }

  fn decrement_if_positive(value: &AtomicUsize) -> bool {
    let mut current = value.load(Ordering::SeqCst);
    while current > 0 {
      match value.compare_exchange_weak(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => return true,
        Err(observed) => current = observed,
      }
    }
    false
  }

  fn write_mock_response(
    stream: &mut TcpStream,
    status: u16,
    body: &str,
    headers: &[(&str, &str)],
  ) {
    write_mock_bytes(stream, status, body.as_bytes(), headers);
  }

  fn write_mock_bytes(stream: &mut TcpStream, status: u16, body: &[u8], headers: &[(&str, &str)]) {
    let reason = match status {
      200 => "OK",
      204 => "No Content",
      400 => "Bad Request",
      404 => "Not Found",
      412 => "Precondition Failed",
      503 => "Service Unavailable",
      _ => "Unknown",
    };
    let extra = headers
      .iter()
      .map(|(name, value)| format!("{name}: {value}\r\n"))
      .collect::<String>();
    let response = format!(
      "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
      body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
  }

  fn percent_decode_test(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
      if bytes[index] == b'%' && index + 2 < bytes.len() {
        let encoded = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or_default();
        if let Ok(byte) = u8::from_str_radix(encoded, 16) {
          decoded.push(byte);
          index += 3;
          continue;
        }
      }
      decoded.push(bytes[index]);
      index += 1;
    }
    String::from_utf8(decoded).unwrap()
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

  #[tokio::test]
  async fn maintenance_protects_active_runs_then_collects_expired_artifacts_and_orphans() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact").unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let artifact_policy = policy(temp.path(), &backend);
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      artifact_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
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
    drop(put);
    drop(managed);

    let connection = Connection::open(&state).unwrap();
    connection
      .execute(
        "UPDATE woml_artifact_retention SET expires_at = '2000-01-01T00:00:00.000Z'",
        [],
      )
      .unwrap();
    let definition_hash = format!("sha256:{}", "a".repeat(64));
    connection
      .execute(
        "INSERT INTO woml_definitions(
           definition_hash, workflow_id, schema_version, model_json, created_at
         ) VALUES (?1, 'workflow', 1, '{}', ?2)",
        params![definition_hash, timestamp()],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO woml_runs(run_id, workflow_id, definition_hash, created_at)
         VALUES ('run-artifact', 'workflow', ?1, ?2)",
        params![definition_hash, timestamp()],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO woml_run_summaries(
           run_id, workflow_id, status, admitted_at, updated_at
         ) VALUES ('run-artifact', 'workflow', 'waiting', ?1, ?1)",
        [timestamp()],
      )
      .unwrap();
    drop(connection);

    let protected = maintain_artifacts(
      &state,
      artifact_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
      Utc::now(),
      true,
    )
    .unwrap();
    assert_eq!(protected.eligible_artifacts, 0);

    Connection::open(&state)
      .unwrap()
      .execute(
        "UPDATE woml_run_summaries SET status = 'succeeded', updated_at = ?1
         WHERE run_id = 'run-artifact'",
        [timestamp()],
      )
      .unwrap();
    let orphan_digest = hex::encode(Sha256::digest(b"orphan"));
    let orphan = backend
      .join("blobs/sha256")
      .join(&orphan_digest[..2])
      .join(&orphan_digest[2..]);
    fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    fs::write(&orphan, b"orphan").unwrap();
    let collected = maintain_artifacts(
      &state,
      artifact_policy,
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
      Utc::now(),
      false,
    )
    .unwrap();
    assert_eq!(collected.deleted_artifacts, 1);
    assert_eq!(collected.deleted_blobs, 1);
    assert_eq!(collected.deleted_bytes, reference.size);
    // Startup recovery is allowed to collect the orphan before the bounded
    // maintenance scan accounts for it.
    assert!(collected.collected_orphans <= 1);
    assert_eq!(collected.live_artifacts, 0);
    assert_eq!(collected.live_blobs, 0);
    assert!(!orphan.exists());
  }

  #[tokio::test]
  async fn artifact_backup_round_trips_verified_bytes_and_rejects_corruption() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("report.txt"), b"durable artifact backup").unwrap();
    let state = temp.path().join("state.sqlite");
    let source_backend = temp.path().join("source-artifacts-v1");
    let source_policy = policy(temp.path(), &source_backend);
    let managed = Arc::new(ManagedArtifactStore::default());
    let put = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      source_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
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
    drop(put);
    drop(managed);

    let backup = temp.path().join("backup-artifacts");
    let exported = export_artifact_backup(
      &state,
      &backup,
      source_policy,
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(exported.blob_count, 1);
    assert_eq!(exported.total_bytes, reference.size);
    assert_eq!(verify_artifact_backup(&state, &backup).unwrap(), exported);

    let restored_backend = temp.path().join("restored-artifacts-v1");
    let restored = restore_artifact_backup(
      &state,
      &backup,
      policy(temp.path(), &restored_backend),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(restored, exported);
    let restored_blob = restored_backend
      .join("blobs/sha256")
      .join(&reference.sha256[..2])
      .join(&reference.sha256[2..]);
    assert_eq!(
      fs::read(&restored_blob).unwrap(),
      b"durable artifact backup"
    );

    let backup_blob = backup
      .join("blobs/sha256")
      .join(&reference.sha256[..2])
      .join(&reference.sha256[2..]);
    fs::write(backup_blob, b"corrupt").unwrap();
    let failure = verify_artifact_backup(&state, &backup).unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_CORRUPT");
  }

  fn assert_backend_conformance(backend: &dyn ArtifactBackend, store_id: &str) {
    let connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    backend.bind_store(&connection, store_id).unwrap();
    let bytes = b"portable artifact backend";
    let digest = hex::encode(Sha256::digest(bytes));

    let mut stage = backend.begin_stage().unwrap();
    stage.file.as_mut().unwrap().write_all(bytes).unwrap();
    backend
      .publish(
        stage,
        &digest,
        bytes.len() as u64,
        &CapabilityCancellationToken::default(),
      )
      .unwrap();
    let verified = backend.verify(&digest, bytes.len() as u64).unwrap();
    assert_eq!(fs::read(&verified.path).unwrap(), bytes);
    drop(verified);
    assert_eq!(backend.inventory(10).unwrap(), vec![digest.clone()]);

    let mut duplicate = backend.begin_stage().unwrap();
    duplicate.file.as_mut().unwrap().write_all(bytes).unwrap();
    backend
      .publish(
        duplicate,
        &digest,
        bytes.len() as u64,
        &CapabilityCancellationToken::default(),
      )
      .unwrap();
    assert_eq!(backend.inventory(10).unwrap(), vec![digest.clone()]);

    let mut cancelled = backend.begin_stage().unwrap();
    cancelled
      .file
      .as_mut()
      .unwrap()
      .write_all(b"cancelled")
      .unwrap();
    let cancellation = CapabilityCancellationToken::default();
    cancellation.cancel();
    let failure = backend
      .publish(
        cancelled,
        &hex::encode(Sha256::digest(b"cancelled")),
        9,
        &cancellation,
      )
      .unwrap_err();
    assert_eq!(failure.code, "WOML_ARTIFACT_CANCELLED");

    backend.delete(&digest).unwrap();
    backend.delete(&digest).unwrap();
    assert!(backend.inventory(10).unwrap().is_empty());
  }

  #[test]
  fn local_and_s3_backends_share_the_frozen_conformance_suite() {
    let local_root = TempDir::new().unwrap();
    let local = LocalArtifactBackend::open(&local_root.path().join("local")).unwrap();
    assert_backend_conformance(&local, "store-local");

    let s3_root = TempDir::new().unwrap();
    let server = MockS3Server::start();
    let s3_config = |access_key: &str| PreparedS3Backend {
      bucket: "woml-test".to_string(),
      region: "eu-central-1".to_string(),
      prefix: "tenant-a/".to_string(),
      endpoint: Url::parse(&server.endpoint).unwrap(),
      force_path_style: true,
      credentials: S3Credentials {
        access_key: access_key.to_string(),
        secret_key: "test-secret-key".to_string(),
        session_token: Some("test-session-token".to_string()),
      },
      server_side_encryption: S3ServerSideEncryption::Aes256,
    };
    let s3 = S3ArtifactBackend::open(s3_config("ACCESS_ONE"), s3_root.path()).unwrap();
    assert_backend_conformance(&s3, "store-s3");

    // A new backend instance resolves credentials again, which permits
    // operator-controlled rotation without changing durable metadata.
    let rotated = S3ArtifactBackend::open(s3_config("ACCESS_TWO"), s3_root.path()).unwrap();
    let connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    rotated.bind_store(&connection, "store-s3").unwrap();

    let requests = server.requests.lock().unwrap().clone();
    let blob_puts = requests
      .iter()
      .filter(|request| request.method == "PUT" && request.target.contains("blobs/sha256"))
      .count();
    let blob_gets = requests
      .iter()
      .filter(|request| request.method == "GET" && request.target.contains("blobs/sha256"))
      .count();
    assert!(
      blob_puts >= 3,
      "transient upload and duplicate publication must retry safely"
    );
    assert!(
      blob_gets >= 3,
      "eventual visibility must recover before verification succeeds"
    );
    assert!(requests.iter().any(|request| {
      request
        .headers
        .get("authorization")
        .is_some_and(|value| value.contains("Credential=ACCESS_ONE/"))
        && request
          .headers
          .get("x-amz-server-side-encryption")
          .map(String::as_str)
          == Some("AES256")
    }));
    assert!(requests.iter().any(|request| {
      request
        .headers
        .get("authorization")
        .is_some_and(|value| value.contains("Credential=ACCESS_TWO/"))
    }));
    let wire_log = format!("{requests:?}");
    assert!(!wire_log.contains("test-secret-key"));
  }

  #[test]
  fn rust_resolves_s3_credentials_symbolically_and_redacts_policy_debugging() {
    let temp = TempDir::new().unwrap();
    let mut authored = policy(temp.path(), &temp.path().join("unused-local"));
    authored.backend = ArtifactBackendPolicy::S3 {
      contract: "woml.artifact-backend".to_string(),
      contract_version: 1,
      bucket: "woml-production-artifacts".to_string(),
      region: "eu-central-1".to_string(),
      prefix: "runtime-a/".to_string(),
      endpoint: Some("https://objects.example.com".to_string()),
      force_path_style: true,
      authentication: Box::new(json!({
        "mode": "secrets",
        "accessKeySecret": "S3_ACCESS_KEY",
        "secretKeySecret": "S3_SECRET_KEY",
        "sessionTokenSecret": "S3_SESSION_TOKEN"
      })),
      server_side_encryption: Box::new(json!({
        "mode": "aws-kms",
        "keyId": "alias/woml-artifacts"
      })),
    };
    let missing = PreparedArtifactPolicy::prepare(
      authored.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap_err();
    assert!(missing.to_string().contains("S3_ACCESS_KEY"));

    let secrets = BTreeMap::from([
      ("S3_ACCESS_KEY".to_string(), "access-value".to_string()),
      ("S3_SECRET_KEY".to_string(), "secret-value".to_string()),
      ("S3_SESSION_TOKEN".to_string(), "session-value".to_string()),
    ]);
    let prepared =
      PreparedArtifactPolicy::prepare(authored.clone(), ArtifactRuntimeMode::Foreground, &secrets)
        .unwrap();
    let debug = format!("{prepared:?}");
    for value in secrets.values() {
      assert!(!debug.contains(value));
    }

    if let ArtifactBackendPolicy::S3 { endpoint, .. } = &mut authored.backend {
      *endpoint = Some("http://objects.example.com".to_string());
    }
    assert!(
      PreparedArtifactPolicy::prepare(authored, ArtifactRuntimeMode::Foreground, &secrets,)
        .is_err()
    );
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

  #[tokio::test]
  async fn operator_inspection_export_and_observation_are_bounded_and_audited() {
    let temp = TempDir::new().unwrap();
    fs::write(
      temp.path().join("plan.bin"),
      b"reviewed infrastructure plan",
    )
    .unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let authored_policy = policy(temp.path(), &backend);
    let managed = Arc::new(ManagedArtifactStore::default());
    let handler = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      authored_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    managed
      .configure_for_state(&state, handler.policy())
      .unwrap();
    let created = handler
      .execute_request_scoped(
        &call("plan.bin", "terraform-plan"),
        Some("infrastructure-review".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let artifact_id = created["data"]["id"].as_str().unwrap();

    let inspection = inspect_artifact(&state, artifact_id).unwrap();
    assert_eq!(inspection.artifact.name, "terraform-plan");
    assert_eq!(inspection.digest_prefix.len(), 12);
    assert_eq!(inspection.provenance.run_id, "run-artifact");
    assert_eq!(inspection.provenance.node_id, "create");
    let run = inspect_run_artifacts(&state, "run-artifact").unwrap();
    assert_eq!(run.artifacts, vec![inspection.clone()]);
    assert!(!run.truncated);

    let export_path = temp.path().join("reviewed-plan.bin");
    let exported = export_artifact(
      &state,
      &export_path,
      artifact_id,
      authored_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    assert!(exported.verified);
    assert_eq!(
      fs::read(&export_path).unwrap(),
      b"reviewed infrastructure plan"
    );
    assert!(!format!("{inspection:?}{exported:?}").contains(export_path.to_string_lossy().as_ref()));

    let observation = observe_artifacts(
      &state,
      authored_policy,
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    assert!(observation.backend_healthy);
    assert_eq!(observation.backend, "local");
    assert_eq!(observation.transfers, 1);
    assert_eq!(observation.transferred_bytes, 28);
    assert_eq!(observation.failed_transfers, 0);
    assert_eq!(observation.corrupt_artifacts, 0);
    assert_eq!(observation.orphan_blobs, 0);

    let connection = Connection::open(&state).unwrap();
    let audits: u64 = connection
      .query_row(
        "SELECT COUNT(*) FROM woml_artifact_operator_audit",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(audits, 3);
  }

  #[tokio::test]
  async fn streaming_startup_and_metadata_stay_within_publication_budgets() {
    const FIXTURE_BYTES: usize = 8 * 1024 * 1024;
    const MINIMUM_BYTES_PER_SECOND: f64 = 1024.0 * 1024.0;
    const STARTUP_BUDGET: Duration = Duration::from_secs(5);
    const INSPECTION_BUDGET: Duration = Duration::from_secs(1);
    assert!(STREAM_BUFFER_BYTES <= 64 * 1024);

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("large-report.bin");
    fs::write(&source, vec![0x5a; FIXTURE_BYTES]).unwrap();
    let state = temp.path().join("state.sqlite");
    let backend = temp.path().join("artifacts-v1");
    let mut authored_policy = policy(temp.path(), &backend);
    authored_policy.limits.max_artifact_bytes = 16 * 1024 * 1024;
    authored_policy.limits.max_run_bytes = 32 * 1024 * 1024;
    authored_policy.limits.max_store_bytes = 64 * 1024 * 1024;
    let managed = Arc::new(ManagedArtifactStore::default());
    let handler = ManagedArtifactPutHandler::new(
      Arc::clone(&managed),
      authored_policy.clone(),
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    managed
      .configure_for_state(&state, handler.policy())
      .unwrap();
    let started = std::time::Instant::now();
    let created = handler
      .execute_request_scoped(
        &call("large-report.bin", "large-report"),
        Some("performance-workflow".to_string()),
        CapabilityCancellationToken::default(),
      )
      .await
      .unwrap();
    let elapsed = started.elapsed();
    let throughput = FIXTURE_BYTES as f64 / elapsed.as_secs_f64().max(0.001);
    assert!(throughput >= MINIMUM_BYTES_PER_SECOND, "{throughput} B/s");

    drop(handler);
    drop(managed);
    let startup = std::time::Instant::now();
    let recovered = Arc::new(ManagedArtifactStore::default());
    let recovered_handler = ManagedArtifactPutHandler::new(
      Arc::clone(&recovered),
      authored_policy,
      ArtifactRuntimeMode::Foreground,
      &BTreeMap::new(),
    )
    .unwrap();
    recovered
      .configure_for_state(&state, recovered_handler.policy())
      .unwrap();
    assert!(startup.elapsed() <= STARTUP_BUDGET);

    let artifact_id = created["data"]["id"].as_str().unwrap();
    let inspection = std::time::Instant::now();
    let artifact = inspect_artifact(&state, artifact_id).unwrap();
    let run = inspect_run_artifacts(&state, "run-artifact").unwrap();
    assert!(inspection.elapsed() <= INSPECTION_BUDGET);
    assert_eq!(artifact.artifact.size, FIXTURE_BYTES as u64);
    assert_eq!(run.artifacts.len(), 1);
  }
}
