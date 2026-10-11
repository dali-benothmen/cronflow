//! Managed Process v1 capability executed and authorized by the Rust core.

use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  io,
  path::{Component, Path, PathBuf},
  process::{ExitStatus, Stdio},
  sync::Arc,
  time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;
use tokio::{
  io::{AsyncRead, AsyncReadExt},
  process::{Child, Command},
  sync::{OwnedSemaphorePermit, Semaphore},
  task::JoinHandle,
};

const PROCESS_TERMINATION_GRACE: Duration = Duration::from_millis(250);
const PROCESS_PIPE_DRAIN_GRACE: Duration = Duration::from_secs(1);

use crate::{
  CapabilityCallRequest, CapabilityCancellationToken, CapabilityDescriptor, CapabilityEffect,
  CapabilityFailure, CapabilityFailureKind, CapabilityHandler,
};

pub const PROCESS_CONTRACT: &str = "woml.process";
pub const PROCESS_CONTRACT_VERSION: u32 = 1;
pub const PROCESS_POLICY_CONTRACT: &str = "woml.process-policy";
pub const PROCESS_POLICY_CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessRuntimeMode {
  Foreground,
  Background,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessPolicyV1 {
  pub contract: String,
  pub contract_version: u32,
  pub enabled: bool,
  pub modes: ProcessPolicyModes,
  pub project_root: String,
  pub executables: Vec<ProcessPolicyExecutable>,
  pub working_directories: Vec<String>,
  pub environment: ProcessPolicyEnvironment,
  pub limits: ProcessPolicyLimits,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessPolicyModes {
  pub foreground: bool,
  pub background: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessPolicyExecutable {
  pub alias: String,
  pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessPolicyEnvironment {
  pub inherit: Vec<String>,
  pub allow: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessPolicyLimits {
  pub max_timeout_ms: u64,
  pub max_output_bytes: usize,
  pub max_concurrent: usize,
}

#[derive(Debug, Error)]
pub enum ProcessPolicyError {
  #[error("WOML_PROCESS_DISABLED: process execution is disabled by runtime policy. Corrective action: grant a reviewed foreground --allow-process list or configure Process Policy v1")]
  Disabled,
  #[error("WOML_PROCESS_MODE_DENIED: process execution is not allowed in {0} mode. Corrective action: enable that mode in the operator-owned Process Policy v1 document")]
  ModeDenied(&'static str),
  #[error("WOML_PROCESS_POLICY_INVALID: {0}. Corrective action: repair the Process Policy v1 document and run WOML again")]
  Invalid(String),
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OutputMode {
  Capture,
  Discard,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProcessOutputRequest {
  stdout: OutputMode,
  stderr: OutputMode,
  max_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProcessRunRequest {
  contract: String,
  contract_version: u32,
  kind: String,
  executable: String,
  args: Vec<String>,
  cwd: String,
  env: BTreeMap<String, String>,
  timeout_ms: u64,
  accept_exit_codes: Vec<i32>,
  output: ProcessOutputRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessRunResult {
  contract: &'static str,
  contract_version: u32,
  kind: &'static str,
  exit_code: Option<i32>,
  signal: Option<String>,
  stdout: String,
  stderr: String,
  stdout_bytes: u64,
  stderr_bytes: u64,
  stdout_truncated: bool,
  stderr_truncated: bool,
  duration_ms: f64,
}

#[derive(Debug)]
struct PreparedProcessPolicy {
  project_root: PathBuf,
  executables: HashMap<String, ApprovedExecutable>,
  working_directories: Vec<PathBuf>,
  inherited_environment: BTreeMap<String, String>,
  allowed_environment: BTreeSet<String>,
  limits: ProcessPolicyLimits,
}

#[derive(Debug)]
struct ApprovedExecutable {
  path: PathBuf,
  identity: ExecutableIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutableIdentity {
  length: u64,
  modified: Option<std::time::SystemTime>,
  #[cfg(unix)]
  device: u64,
  #[cfg(unix)]
  inode: u64,
  #[cfg(windows)]
  volume_serial_number: Option<u32>,
  #[cfg(windows)]
  file_index: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ManagedProcessHandler {
  policy: Arc<PreparedProcessPolicy>,
  concurrency: Arc<Semaphore>,
}

impl ManagedProcessHandler {
  pub fn new(
    policy: ProcessPolicyV1,
    mode: ProcessRuntimeMode,
  ) -> Result<Self, ProcessPolicyError> {
    let prepared = PreparedProcessPolicy::prepare(policy, mode)?;
    let max_concurrent = prepared.limits.max_concurrent;
    Ok(Self {
      policy: Arc::new(prepared),
      concurrency: Arc::new(Semaphore::new(max_concurrent)),
    })
  }

  fn parse_and_authorize(&self, input: &Value) -> Result<AuthorizedProcess, CapabilityFailure> {
    let mut request = parse_request(input)?;
    request.timeout_ms = request.timeout_ms.min(self.policy.limits.max_timeout_ms);
    if request.output.max_bytes > self.policy.limits.max_output_bytes {
      return Err(rejected(
        "WOML_PROCESS_OUTPUT_LIMIT_DENIED",
        "The requested process output limit exceeds the runtime policy limit.",
      ));
    }
    let executable = self
      .policy
      .executables
      .get(&request.executable)
      .ok_or_else(|| {
        rejected(
          "WOML_PROCESS_EXECUTABLE_DENIED",
          "The requested executable alias is not allowed by runtime policy.",
        )
      })?;
    verify_executable(executable)?;
    for name in request.env.keys() {
      if !self.policy.allowed_environment.contains(name) {
        return Err(rejected(
          "WOML_PROCESS_ENV_DENIED",
          "The process request contains an environment variable not allowed by runtime policy.",
        ));
      }
    }
    let cwd = authorize_cwd(&self.policy, &request.cwd)?;
    Ok(AuthorizedProcess {
      request,
      executable: executable.path.clone(),
      cwd,
    })
  }
}

impl PreparedProcessPolicy {
  fn prepare(
    policy: ProcessPolicyV1,
    mode: ProcessRuntimeMode,
  ) -> Result<Self, ProcessPolicyError> {
    if policy.contract != PROCESS_POLICY_CONTRACT
      || policy.contract_version != PROCESS_POLICY_CONTRACT_VERSION
    {
      return Err(ProcessPolicyError::Invalid(
        "the policy does not match Process Policy v1".to_string(),
      ));
    }
    if !policy.enabled {
      return Err(ProcessPolicyError::Disabled);
    }
    let allowed = match mode {
      ProcessRuntimeMode::Foreground => policy.modes.foreground,
      ProcessRuntimeMode::Background => policy.modes.background,
    };
    if !allowed {
      return Err(ProcessPolicyError::ModeDenied(match mode {
        ProcessRuntimeMode::Foreground => "foreground",
        ProcessRuntimeMode::Background => "background",
      }));
    }
    validate_limits(&policy.limits)?;
    if policy.project_root.is_empty()
      || policy.project_root.len() > 4096
      || policy.executables.len() > 128
      || policy.working_directories.len() > 128
      || policy.environment.inherit.len() > 128
      || policy.environment.allow.len() > 128
    {
      return Err(ProcessPolicyError::Invalid(
        "the policy exceeds Process Policy v1 collection or path bounds".to_string(),
      ));
    }
    let project_root = canonical_directory(Path::new(&policy.project_root), "project root")?;
    if policy.working_directories.is_empty() {
      return Err(ProcessPolicyError::Invalid(
        "at least one working-directory root is required".to_string(),
      ));
    }
    let mut working_directories = Vec::with_capacity(policy.working_directories.len());
    let mut authored_working_directories = BTreeSet::new();
    for value in &policy.working_directories {
      if value.is_empty()
        || value.len() > 4096
        || !authored_working_directories.insert(value.clone())
      {
        return Err(ProcessPolicyError::Invalid(
          "working-directory roots must be unique non-empty paths within the v1 size bound"
            .to_string(),
        ));
      }
      let candidate = Path::new(value);
      let path = if candidate.is_absolute() {
        candidate.to_path_buf()
      } else {
        project_root.join(candidate)
      };
      let canonical = canonical_directory(&path, "working-directory root")?;
      if !canonical.starts_with(&project_root) {
        return Err(ProcessPolicyError::Invalid(
          "a working-directory root escapes the project root".to_string(),
        ));
      }
      if !working_directories.contains(&canonical) {
        working_directories.push(canonical);
      }
    }
    let mut executables = HashMap::new();
    for entry in &policy.executables {
      validate_alias(&entry.alias)?;
      if entry.path.is_empty() || entry.path.len() > 4096 {
        return Err(ProcessPolicyError::Invalid(format!(
          "executable alias '{}' has an invalid path",
          entry.alias
        )));
      }
      let canonical = std::fs::canonicalize(&entry.path).map_err(|_| {
        ProcessPolicyError::Invalid(format!(
          "executable alias '{}' points to a missing path",
          entry.alias
        ))
      })?;
      let identity = verify_executable_policy(&canonical, &entry.alias)?;
      if executables
        .insert(
          entry.alias.clone(),
          ApprovedExecutable {
            path: canonical,
            identity,
          },
        )
        .is_some()
      {
        return Err(ProcessPolicyError::Invalid(format!(
          "executable alias '{}' is duplicated",
          entry.alias
        )));
      }
    }
    let mut inherited_environment = BTreeMap::new();
    let mut inherited_names = BTreeSet::new();
    for name in &policy.environment.inherit {
      validate_environment_name(name).map_err(ProcessPolicyError::Invalid)?;
      if !inherited_names.insert(name.clone()) {
        return Err(ProcessPolicyError::Invalid(
          "inherited environment-variable names must be unique".to_string(),
        ));
      }
      if let Ok(value) = std::env::var(name) {
        inherited_environment.insert(name.clone(), value);
      }
    }
    let mut allowed_environment = BTreeSet::new();
    for name in &policy.environment.allow {
      validate_environment_name(name).map_err(ProcessPolicyError::Invalid)?;
      if !allowed_environment.insert(name.clone()) {
        return Err(ProcessPolicyError::Invalid(
          "explicit environment-variable names must be unique".to_string(),
        ));
      }
    }
    Ok(Self {
      project_root,
      executables,
      working_directories,
      inherited_environment,
      allowed_environment,
      limits: policy.limits,
    })
  }
}

impl CapabilityHandler for ManagedProcessHandler {
  fn descriptor(&self) -> CapabilityDescriptor {
    CapabilityDescriptor {
      capability: "process".to_string(),
      operation: "run".to_string(),
      input_contract_version: PROCESS_CONTRACT_VERSION,
      result_contract_version: PROCESS_CONTRACT_VERSION,
      effect: CapabilityEffect::UnsafeWrite,
      supports_cancellation: true,
      supports_provider_idempotency: false,
    }
  }

  fn validate_request(&self, request: &CapabilityCallRequest) -> Result<(), CapabilityFailure> {
    self.parse_and_authorize(&request.input).map(|_| ())
  }

  fn safe_metadata(&self, input: &Value) -> Map<String, Value> {
    let Ok(request) = parse_request(input) else {
      return Map::new();
    };
    Map::from_iter([
      ("executable".to_string(), Value::String(request.executable)),
      ("argumentCount".to_string(), json!(request.args.len())),
      (
        "workingDirectory".to_string(),
        Value::String("policy-relative".to_string()),
      ),
      ("timeoutMs".to_string(), json!(request.timeout_ms)),
      (
        "acceptedExitCodeCount".to_string(),
        json!(request.accept_exit_codes.len()),
      ),
      (
        "outputMaxBytes".to_string(),
        json!(request.output.max_bytes),
      ),
      (
        "stdoutMode".to_string(),
        Value::String(output_mode_name(request.output.stdout).to_string()),
      ),
      (
        "stderrMode".to_string(),
        Value::String(output_mode_name(request.output.stderr).to_string()),
      ),
    ])
  }

  fn safe_result_metadata(&self, result: &Value) -> Map<String, Value> {
    let mut safe = Map::from_iter([(
      "outcome".to_string(),
      Value::String("succeeded".to_string()),
    )]);
    for field in [
      "exitCode",
      "signal",
      "durationMs",
      "stdoutBytes",
      "stderrBytes",
      "stdoutTruncated",
      "stderrTruncated",
    ] {
      if let Some(value) = result.get(field) {
        safe.insert(field.to_string(), value.clone());
      }
    }
    safe
  }

  fn safe_failure_metadata(&self, error: &CapabilityFailure) -> Map<String, Value> {
    let outcome = match error.kind {
      CapabilityFailureKind::Cancelled => "cancelled",
      CapabilityFailureKind::TimedOut => "timed_out",
      CapabilityFailureKind::Interrupted => "interrupted",
      _ => "failed",
    };
    let mut safe = Map::from_iter([("outcome".to_string(), Value::String(outcome.to_string()))]);
    if let Some(details) = error.details.as_ref() {
      for field in [
        "exitCode",
        "signal",
        "durationMs",
        "stdoutBytes",
        "stderrBytes",
        "stdoutTruncated",
        "stderrTruncated",
      ] {
        if let Some(value) = details.get(field) {
          safe.insert(field.to_string(), value.clone());
        }
      }
    }
    safe
  }

  fn execute(
    &self,
    input: Value,
    cancellation: CapabilityCancellationToken,
  ) -> BoxFuture<'static, Result<Value, CapabilityFailure>> {
    let authorized = match self.parse_and_authorize(&input) {
      Ok(authorized) => authorized,
      Err(error) => return Box::pin(async move { Err(error) }),
    };
    let policy = Arc::clone(&self.policy);
    let concurrency = Arc::clone(&self.concurrency);
    Box::pin(async move {
      let permit = concurrency.try_acquire_owned().map_err(|_| {
        rejected(
          "WOML_PROCESS_CONCURRENCY_LIMIT",
          "The runtime process concurrency limit has been reached.",
        )
      })?;
      execute_process(authorized, policy, cancellation, permit).await
    })
  }
}

#[derive(Debug)]
struct AuthorizedProcess {
  request: ProcessRunRequest,
  executable: PathBuf,
  cwd: PathBuf,
}

#[derive(Debug)]
struct CapturedOutput {
  text: String,
  bytes: u64,
  truncated: bool,
}

async fn execute_process(
  authorized: AuthorizedProcess,
  policy: Arc<PreparedProcessPolicy>,
  cancellation: CapabilityCancellationToken,
  _permit: OwnedSemaphorePermit,
) -> Result<Value, CapabilityFailure> {
  let started = Instant::now();
  let request = authorized.request;
  // Process output is ephemeral, but it is still user-visible. Operator-
  // inherited values are sensitive at this boundary. The Bun worker separately
  // redacts resolved WOML secrets because secret provenance intentionally does
  // not cross the Process v1 contract.
  let sensitive_values = sensitive_environment_values(&policy);
  let redaction_lookahead = sensitive_values
    .iter()
    .map(String::len)
    .max()
    .unwrap_or(0)
    .saturating_sub(1);
  let mut command = Command::new(&authorized.executable);
  command
    .args(&request.args)
    .current_dir(&authorized.cwd)
    .env_clear()
    .envs(&policy.inherited_environment)
    .envs(&request.env)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  prepare_process_containment(&mut command);
  let mut child = command.spawn().map_err(|_| {
    failure(
      CapabilityFailureKind::ServiceRejected,
      "WOML_PROCESS_SPAWN_FAILED",
      "The approved executable could not be started.",
      true,
      false,
      None,
    )
  })?;
  let mut containment = match ProcessContainment::attach(&child) {
    Ok(containment) => containment,
    Err(_) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(failure(
        CapabilityFailureKind::HandlerCrashed,
        "WOML_PROCESS_CONTAINMENT_FAILED",
        "The process could not be attached to its operating-system containment boundary.",
        false,
        true,
        None,
      ));
    }
  };
  let stdout = child.stdout.take().ok_or_else(|| {
    failure(
      CapabilityFailureKind::HandlerCrashed,
      "WOML_PROCESS_PIPE_UNAVAILABLE",
      "The process stdout pipe could not be initialized.",
      false,
      true,
      None,
    )
  })?;
  let stderr = child.stderr.take().ok_or_else(|| {
    failure(
      CapabilityFailureKind::HandlerCrashed,
      "WOML_PROCESS_PIPE_UNAVAILABLE",
      "The process stderr pipe could not be initialized.",
      false,
      true,
      None,
    )
  })?;
  let stdout_task = tokio::spawn(drain_output(
    stdout,
    request.output.stdout,
    request.output.max_bytes,
    redaction_lookahead,
  ));
  let stderr_task = tokio::spawn(drain_output(
    stderr,
    request.output.stderr,
    request.output.max_bytes,
    redaction_lookahead,
  ));
  let timeout = tokio::time::sleep(Duration::from_millis(request.timeout_ms));
  tokio::pin!(timeout);
  let status = tokio::select! {
    biased;
    _ = cancellation.cancelled() => {
      let _ = terminate_process_tree(&mut child, &mut containment).await;
      let _ = collect_output_task(stdout_task).await;
      let _ = collect_output_task(stderr_task).await;
      return Err(failure(
        CapabilityFailureKind::Cancelled,
        "WOML_PROCESS_CANCELLED",
        "The process was cancelled.",
        false,
        true,
        Some(safe_duration(started)),
      ));
    }
    _ = &mut timeout => {
      let _ = terminate_process_tree(&mut child, &mut containment).await;
      let _ = collect_output_task(stdout_task).await;
      let _ = collect_output_task(stderr_task).await;
      return Err(failure(
        CapabilityFailureKind::TimedOut,
        "WOML_PROCESS_TIMED_OUT",
        "The process exceeded its approved timeout.",
        false,
        true,
        Some(safe_duration(started)),
      ));
    }
    status = child.wait() => status.map_err(|_| failure(
      CapabilityFailureKind::HandlerCrashed,
      "WOML_PROCESS_WAIT_FAILED",
      "The runtime could not observe the process exit status.",
      false,
      true,
      Some(safe_duration(started)),
    ))?,
  };
  // Process v1 never permits detached work. If the leader exited after
  // starting descendants, close the containment boundary before waiting for
  // inherited stdout/stderr handles to reach EOF.
  containment.close_remaining_tree().await;
  let stdout = redact_and_bound_output(
    collect_output_task(stdout_task).await?,
    &sensitive_values,
    request.output.max_bytes,
  );
  let stderr = redact_and_bound_output(
    collect_output_task(stderr_task).await?,
    &sensitive_values,
    request.output.max_bytes,
  );
  let exit_code = status.code();
  let signal = exit_signal(&status);
  let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
  let result = ProcessRunResult {
    contract: PROCESS_CONTRACT,
    contract_version: PROCESS_CONTRACT_VERSION,
    kind: "result",
    exit_code,
    signal: signal.clone(),
    stdout: stdout.text,
    stderr: stderr.text,
    stdout_bytes: stdout.bytes,
    stderr_bytes: stderr.bytes,
    stdout_truncated: stdout.truncated,
    stderr_truncated: stderr.truncated,
    duration_ms,
  };
  if exit_code.is_none() || !request.accept_exit_codes.contains(&exit_code.unwrap_or(-1)) {
    let details = Map::from_iter([
      (
        "exitCode".to_string(),
        exit_code.map_or(Value::Null, Value::from),
      ),
      (
        "signal".to_string(),
        signal.map_or(Value::Null, Value::String),
      ),
      ("durationMs".to_string(), json!(duration_ms)),
      ("stdoutBytes".to_string(), json!(stdout.bytes)),
      ("stderrBytes".to_string(), json!(stderr.bytes)),
      ("stdoutTruncated".to_string(), json!(stdout.truncated)),
      ("stderrTruncated".to_string(), json!(stderr.truncated)),
    ]);
    let (code, message, ambiguous) = if exit_code.is_none() {
      (
        "WOML_PROCESS_SIGNALLED",
        "The process was terminated by a signal or platform-equivalent condition.",
        true,
      )
    } else {
      (
        "WOML_PROCESS_EXIT_CODE",
        "The process exited with a status not accepted by the workflow.",
        false,
      )
    };
    return Err(failure(
      CapabilityFailureKind::ServiceRejected,
      code,
      message,
      false,
      ambiguous,
      Some(details),
    ));
  }
  serde_json::to_value(result).map_err(|_| output_failure())
}

async fn collect_output_task(
  mut task: JoinHandle<Result<CapturedOutput, CapabilityFailure>>,
) -> Result<CapturedOutput, CapabilityFailure> {
  match tokio::time::timeout(PROCESS_PIPE_DRAIN_GRACE, &mut task).await {
    Ok(joined) => joined.map_err(|_| output_failure())?,
    Err(_) => {
      task.abort();
      let _ = task.await;
      Err(failure(
        CapabilityFailureKind::HandlerCrashed,
        "WOML_PROCESS_PIPE_DRAIN_TIMED_OUT",
        "The process output pipes did not close after the containment boundary terminated.",
        false,
        true,
        None,
      ))
    }
  }
}

async fn terminate_process_tree(
  child: &mut Child,
  containment: &mut ProcessContainment,
) -> io::Result<ExitStatus> {
  containment.graceful_terminate()?;
  tokio::time::sleep(PROCESS_TERMINATION_GRACE).await;
  containment.force_terminate()?;
  child.wait().await
}

#[cfg(unix)]
fn prepare_process_containment(command: &mut Command) {
  use std::os::unix::process::CommandExt;
  command.as_std_mut().process_group(0);
}

#[cfg(windows)]
fn prepare_process_containment(_command: &mut Command) {}

#[cfg(not(any(unix, windows)))]
fn prepare_process_containment(_command: &mut Command) {}

#[derive(Debug)]
struct ProcessContainment {
  #[cfg(unix)]
  process_group_id: i32,
  #[cfg(windows)]
  job: WindowsJob,
}

impl ProcessContainment {
  fn attach(child: &Child) -> io::Result<Self> {
    #[cfg(unix)]
    {
      let process_group_id = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .ok_or_else(|| io::Error::other("spawned process has no valid process-group id"))?;
      Ok(Self { process_group_id })
    }
    #[cfg(windows)]
    {
      Ok(Self {
        job: WindowsJob::attach(child)?,
      })
    }
    #[cfg(not(any(unix, windows)))]
    {
      let _ = child;
      Ok(Self {})
    }
  }

  fn graceful_terminate(&mut self) -> io::Result<()> {
    #[cfg(unix)]
    {
      signal_process_group(self.process_group_id, libc::SIGTERM)
    }
    #[cfg(windows)]
    {
      // Windows has no portable graceful signal for an arbitrary non-console
      // process. Job termination is therefore the reviewed forced boundary.
      self.job.terminate()
    }
    #[cfg(not(any(unix, windows)))]
    {
      Ok(())
    }
  }

  fn force_terminate(&mut self) -> io::Result<()> {
    #[cfg(unix)]
    {
      signal_process_group(self.process_group_id, libc::SIGKILL)
    }
    #[cfg(windows)]
    {
      self.job.terminate()
    }
    #[cfg(not(any(unix, windows)))]
    {
      Ok(())
    }
  }

  async fn close_remaining_tree(&mut self) {
    #[cfg(unix)]
    {
      if process_group_exists(self.process_group_id) {
        let _ = self.graceful_terminate();
        tokio::time::sleep(PROCESS_TERMINATION_GRACE).await;
        let _ = self.force_terminate();
      }
    }
    #[cfg(windows)]
    {
      // Closing a kill-on-close Job Object is the final descendant boundary.
      let _ = self.job.terminate();
    }
  }
}

impl Drop for ProcessContainment {
  fn drop(&mut self) {
    #[cfg(unix)]
    {
      // A dropped handler future (for example, host shutdown) must not bypass
      // containment cleanup. SIGKILL is the only synchronous fail-closed
      // action available during Drop.
      let _ = signal_process_group(self.process_group_id, libc::SIGKILL);
    }
    // On Windows, dropping the WindowsJob field closes its kill-on-close job.
  }
}

#[cfg(unix)]
fn signal_process_group(process_group_id: i32, signal: i32) -> io::Result<()> {
  // SAFETY: kill is called with a positive, engine-owned process-group id and
  // a platform signal constant. A negative pid targets only that group.
  let result = unsafe { libc::kill(-process_group_id, signal) };
  if result == 0 {
    return Ok(());
  }
  let error = io::Error::last_os_error();
  if error.raw_os_error() == Some(libc::ESRCH) {
    Ok(())
  } else {
    Err(error)
  }
}

#[cfg(unix)]
fn process_group_exists(process_group_id: i32) -> bool {
  // SAFETY: signal 0 performs existence/permission checking without sending a
  // signal. The id was captured from the child created by this handler.
  let result = unsafe { libc::kill(-process_group_id, 0) };
  result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob {
  handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsJob {
  fn attach(child: &Child) -> io::Result<Self> {
    use std::{mem::size_of, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
      Foundation::CloseHandle,
      System::JobObjects::{
        AssignProcessToJobObject, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
      },
    };
    // SAFETY: every handle is checked, the info pointer and byte size match,
    // and ownership of the created job is retained by WindowsJob.
    unsafe {
      let handle =
        windows_sys::Win32::System::JobObjects::CreateJobObjectW(ptr::null(), ptr::null());
      if handle.is_null() {
        return Err(io::Error::last_os_error());
      }
      let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
      information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
      if SetInformationJobObject(
        handle,
        JobObjectExtendedLimitInformation,
        &information as *const _ as *const _,
        size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
      ) == 0
        || AssignProcessToJobObject(handle, child.as_raw_handle() as _) == 0
      {
        let error = io::Error::last_os_error();
        CloseHandle(handle);
        return Err(error);
      }
      Ok(Self { handle })
    }
  }

  fn terminate(&self) -> io::Result<()> {
    // SAFETY: handle is a live Job Object owned by self.
    let result =
      unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle, 1) };
    if result == 0 {
      Err(io::Error::last_os_error())
    } else {
      Ok(())
    }
  }
}

#[cfg(windows)]
// SAFETY: WindowsJob exclusively owns a kernel handle. Moving that owned
// handle between executor threads does not change its validity or ownership.
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl Drop for WindowsJob {
  fn drop(&mut self) {
    // SAFETY: handle is owned by self and closed exactly once.
    unsafe {
      windows_sys::Win32::Foundation::CloseHandle(self.handle);
    }
  }
}

async fn drain_output<R: AsyncRead + Unpin>(
  mut reader: R,
  mode: OutputMode,
  max_bytes: usize,
  redaction_lookahead: usize,
) -> Result<CapturedOutput, CapabilityFailure> {
  let capture_limit = max_bytes.saturating_add(redaction_lookahead);
  let mut captured = Vec::with_capacity(match mode {
    OutputMode::Capture => capture_limit.min(64 * 1024),
    OutputMode::Discard => 0,
  });
  let mut total = 0_u64;
  let mut buffer = [0_u8; 8192];
  loop {
    let read = reader
      .read(&mut buffer)
      .await
      .map_err(|_| output_failure())?;
    if read == 0 {
      break;
    }
    total = total.saturating_add(read as u64);
    if matches!(mode, OutputMode::Capture) && captured.len() < capture_limit {
      let remaining = capture_limit - captured.len();
      captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
  }
  Ok(CapturedOutput {
    text: String::from_utf8_lossy(&captured).into_owned(),
    bytes: total,
    truncated: matches!(mode, OutputMode::Capture) && total > max_bytes as u64,
  })
}

fn sensitive_environment_values(policy: &PreparedProcessPolicy) -> Vec<String> {
  let mut values = policy
    .inherited_environment
    .values()
    .filter(|value| !value.is_empty())
    .cloned()
    .collect::<Vec<_>>();
  values.sort_by_key(|value| std::cmp::Reverse(value.len()));
  values.dedup();
  values
}

fn redact_and_bound_output(
  mut output: CapturedOutput,
  sensitive_values: &[String],
  max_bytes: usize,
) -> CapturedOutput {
  for value in sensitive_values {
    output.text = output.text.replace(value, "[redacted]");
  }
  if output.text.len() > max_bytes {
    let mut end = max_bytes;
    while !output.text.is_char_boundary(end) {
      end = end.saturating_sub(1);
    }
    output.text.truncate(end);
    output.truncated = true;
  }
  output
}

fn parse_request(input: &Value) -> Result<ProcessRunRequest, CapabilityFailure> {
  let request: ProcessRunRequest = serde_json::from_value(input.clone()).map_err(|_| {
    invalid_input("Process input does not match the frozen Process v1 request contract.")
  })?;
  if request.contract != PROCESS_CONTRACT
    || request.contract_version != PROCESS_CONTRACT_VERSION
    || request.kind != "request"
    || request.args.len() > 256
    || request.env.len() > 128
    || request.accept_exit_codes.is_empty()
    || request.accept_exit_codes.len() > 32
    || request.timeout_ms == 0
    || request.timeout_ms > 86_400_000
    || request.output.max_bytes == 0
    || request.output.max_bytes > 262_144
  {
    return Err(invalid_input(
      "Process input does not match the frozen Process v1 request contract.",
    ));
  }
  validate_alias_failure(&request.executable)?;
  validate_relative_cwd(&request.cwd)?;
  if request
    .args
    .iter()
    .any(|value| value.contains('\0') || value.len() > 32_768)
    || request.env.iter().any(|(name, value)| {
      validate_environment_name(name).is_err() || value.contains('\0') || value.len() > 32_768
    })
    || request
      .accept_exit_codes
      .iter()
      .any(|code| !(0..=255).contains(code))
  {
    return Err(invalid_input(
      "Process input contains an invalid argument, environment value, or exit code.",
    ));
  }
  let unique: BTreeSet<_> = request.accept_exit_codes.iter().collect();
  if unique.len() != request.accept_exit_codes.len() {
    return Err(invalid_input("Process accepted exit codes must be unique."));
  }
  Ok(request)
}

fn authorize_cwd(
  policy: &PreparedProcessPolicy,
  requested: &str,
) -> Result<PathBuf, CapabilityFailure> {
  validate_relative_cwd(requested)?;
  let candidate = policy.project_root.join(requested);
  let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
    rejected(
      "WOML_PROCESS_CWD_NOT_FOUND",
      "The requested process working directory does not exist.",
    )
  })?;
  if !canonical.is_dir()
    || !policy
      .working_directories
      .iter()
      .any(|root| canonical.starts_with(root))
  {
    return Err(rejected(
      "WOML_PROCESS_CWD_DENIED",
      "The requested process working directory is not allowed by runtime policy.",
    ));
  }
  Ok(canonical)
}

fn validate_relative_cwd(value: &str) -> Result<(), CapabilityFailure> {
  if value.is_empty() || value.len() > 4096 || value.contains('\0') {
    return Err(invalid_input(
      "Process cwd must be a non-empty relative path.",
    ));
  }
  let path = Path::new(value);
  if path.is_absolute()
    || value.starts_with(['/', '\\'])
    || (value.len() >= 3
      && value.as_bytes()[0].is_ascii_alphabetic()
      && value.as_bytes()[1] == b':'
      && matches!(value.as_bytes()[2], b'/' | b'\\'))
    || value
      .split(['/', '\\'])
      .any(|component| component == ".." || invalid_portable_path_component(component))
    || path.components().any(|component| {
      matches!(
        component,
        Component::ParentDir | Component::RootDir | Component::Prefix(_)
      )
    })
  {
    return Err(invalid_input(
      "Process cwd must be relative and must not contain parent traversal.",
    ));
  }
  Ok(())
}

fn invalid_portable_path_component(component: &str) -> bool {
  if component.is_empty() || component == "." {
    return false;
  }
  if component.contains(':') || component.ends_with(' ') || component.ends_with('.') {
    return true;
  }
  let stem = component
    .split('.')
    .next()
    .unwrap_or(component)
    .to_ascii_uppercase();
  matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
    || stem
      .strip_prefix("COM")
      .or_else(|| stem.strip_prefix("LPT"))
      .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn validate_alias(alias: &str) -> Result<(), ProcessPolicyError> {
  validate_alias_text(alias).map_err(ProcessPolicyError::Invalid)
}

fn validate_alias_failure(alias: &str) -> Result<(), CapabilityFailure> {
  validate_alias_text(alias).map_err(|_| invalid_input("Process executable alias is invalid."))
}

fn validate_alias_text(alias: &str) -> Result<(), String> {
  if alias.is_empty()
    || alias.len() > 128
    || !alias
      .bytes()
      .enumerate()
      .all(|(index, byte)| byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte)))
  {
    return Err("an executable alias is invalid".to_string());
  }
  Ok(())
}

fn validate_environment_name(name: &str) -> Result<(), String> {
  if name.is_empty()
    || name.len() > 128
    || !name.bytes().enumerate().all(|(index, byte)| {
      byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
    })
  {
    return Err("an environment-variable name is invalid".to_string());
  }
  Ok(())
}

fn validate_limits(limits: &ProcessPolicyLimits) -> Result<(), ProcessPolicyError> {
  if limits.max_timeout_ms == 0
    || limits.max_timeout_ms > 86_400_000
    || limits.max_output_bytes == 0
    || limits.max_output_bytes > 262_144
    || limits.max_concurrent == 0
    || limits.max_concurrent > 64
  {
    return Err(ProcessPolicyError::Invalid(
      "timeout, output, or concurrency limits are outside Process Policy v1 bounds".to_string(),
    ));
  }
  Ok(())
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, ProcessPolicyError> {
  let canonical = std::fs::canonicalize(path)
    .map_err(|_| ProcessPolicyError::Invalid(format!("the {label} does not exist")))?;
  if !canonical.is_dir() {
    return Err(ProcessPolicyError::Invalid(format!(
      "the {label} is not a directory"
    )));
  }
  Ok(canonical)
}

fn verify_executable_policy(
  path: &Path,
  alias: &str,
) -> Result<ExecutableIdentity, ProcessPolicyError> {
  let metadata = std::fs::metadata(path).map_err(|_| {
    ProcessPolicyError::Invalid(format!("executable alias '{alias}' is unavailable"))
  })?;
  if !metadata.is_file() || !platform_executable(&metadata) {
    return Err(ProcessPolicyError::Invalid(format!(
      "executable alias '{alias}' does not point to an executable file"
    )));
  }
  Ok(executable_identity(&metadata))
}

fn verify_executable(executable: &ApprovedExecutable) -> Result<(), CapabilityFailure> {
  let canonical = std::fs::canonicalize(&executable.path).map_err(|_| {
    rejected(
      "WOML_PROCESS_EXECUTABLE_NOT_FOUND",
      "The approved executable is no longer available.",
    )
  })?;
  let metadata = std::fs::metadata(&canonical).map_err(|_| {
    rejected(
      "WOML_PROCESS_EXECUTABLE_NOT_FOUND",
      "The approved executable is no longer available.",
    )
  })?;
  if canonical != executable.path
    || !metadata.is_file()
    || !platform_executable(&metadata)
    || executable_identity(&metadata) != executable.identity
  {
    return Err(rejected(
      "WOML_PROCESS_EXECUTABLE_CHANGED",
      "The approved executable changed after runtime preflight.",
    ));
  }
  Ok(())
}

fn executable_identity(metadata: &std::fs::Metadata) -> ExecutableIdentity {
  #[cfg(unix)]
  use std::os::unix::fs::MetadataExt;
  #[cfg(windows)]
  use std::os::windows::fs::MetadataExt;
  ExecutableIdentity {
    length: metadata.len(),
    modified: metadata.modified().ok(),
    #[cfg(unix)]
    device: metadata.dev(),
    #[cfg(unix)]
    inode: metadata.ino(),
    #[cfg(windows)]
    volume_serial_number: metadata.volume_serial_number(),
    #[cfg(windows)]
    file_index: metadata.file_index(),
  }
}

#[cfg(unix)]
fn platform_executable(metadata: &std::fs::Metadata) -> bool {
  use std::os::unix::fs::PermissionsExt;
  metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn platform_executable(_metadata: &std::fs::Metadata) -> bool {
  true
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<String> {
  use std::os::unix::process::ExitStatusExt;
  status.signal().map(|signal| signal.to_string())
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<String> {
  None
}

fn safe_duration(started: Instant) -> Map<String, Value> {
  Map::from_iter([(
    "durationMs".to_string(),
    json!(started.elapsed().as_secs_f64() * 1_000.0),
  )])
}

fn output_mode_name(mode: OutputMode) -> &'static str {
  match mode {
    OutputMode::Capture => "capture",
    OutputMode::Discard => "discard",
  }
}

fn invalid_input(message: &str) -> CapabilityFailure {
  failure(
    CapabilityFailureKind::InvalidInput,
    "WOML_PROCESS_INPUT_INVALID",
    message,
    false,
    false,
    None,
  )
}

fn rejected(code: &str, message: &str) -> CapabilityFailure {
  failure(
    CapabilityFailureKind::ServiceRejected,
    code,
    message,
    false,
    false,
    None,
  )
}

fn output_failure() -> CapabilityFailure {
  failure(
    CapabilityFailureKind::HandlerCrashed,
    "WOML_PROCESS_OUTPUT_FAILED",
    "The runtime could not safely capture process output.",
    false,
    true,
    None,
  )
}

fn failure(
  kind: CapabilityFailureKind,
  code: &str,
  message: &str,
  retryable: bool,
  ambiguous: bool,
  details: Option<Map<String, Value>>,
) -> CapabilityFailure {
  CapabilityFailure {
    kind,
    code: code.to_string(),
    message: format!("{message} Corrective action: {}", corrective_action(code)),
    retryable,
    ambiguous,
    details,
  }
}

fn corrective_action(code: &str) -> &'static str {
  match code {
    "WOML_PROCESS_EXECUTABLE_DENIED" => {
      "add the intended executable alias to the operator-owned process policy"
    }
    "WOML_PROCESS_EXECUTABLE_NOT_FOUND" | "WOML_PROCESS_EXECUTABLE_CHANGED" => {
      "restore the reviewed executable and restart the WOML runtime so it can preflight it again"
    }
    "WOML_PROCESS_CWD_NOT_FOUND" | "WOML_PROCESS_CWD_DENIED" => {
      "use an existing project-relative directory allowed by the process policy"
    }
    "WOML_PROCESS_ENV_DENIED" => {
      "remove the variable or allow its name in the operator-owned process policy"
    }
    "WOML_PROCESS_OUTPUT_LIMIT_DENIED" => {
      "lower output.maxBytes or raise the policy ceiling after review"
    }
    "WOML_PROCESS_CONCURRENCY_LIMIT" => {
      "wait for another managed process to finish or raise the reviewed concurrency limit"
    }
    "WOML_PROCESS_SPAWN_FAILED" => {
      "verify executable permissions and host resources, then start a new run"
    }
    "WOML_PROCESS_TIMED_OUT" => {
      "optimize the command or increase its timeout within the policy ceiling"
    }
    "WOML_PROCESS_CANCELLED" => {
      "inspect possible partial external effects before starting a new run"
    }
    "WOML_PROCESS_EXIT_CODE" => {
      "inspect the tool result or explicitly accept that exit code when it represents success"
    }
    "WOML_PROCESS_SIGNALLED" => {
      "inspect host resource pressure and possible partial effects before retrying"
    }
    "WOML_PROCESS_INPUT_INVALID" => "correct the Process v1 request fields and bounds",
    _ => "inspect the runtime diagnostics and start a new run only when the cause is understood",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn typescript_and_rust_share_the_frozen_process_v1_payloads() {
    let request: Value = serde_json::from_str(include_str!(
      "../../../woml/tests/fixtures/services-contracts/process-request.v1.json"
    ))
    .expect("standalone Process v1 request fixture");
    let capability_request: Value = serde_json::from_str(include_str!(
      "../../../woml/tests/fixtures/services-contracts/capability-process-request.v1.json"
    ))
    .expect("Capability Call v1 Process request fixture");
    assert_eq!(capability_request["input"], request);

    let parsed = parse_request(&request).expect("Rust accepts the TypeScript request fixture");
    assert_eq!(parsed.contract, PROCESS_CONTRACT);
    assert_eq!(parsed.contract_version, PROCESS_CONTRACT_VERSION);
    assert_eq!(parsed.kind, "request");
    assert_eq!(parsed.executable, "bun");
    assert_eq!(parsed.args, ["test", "--timeout", "5000"]);
    assert_eq!(parsed.cwd, ".");
    assert_eq!(parsed.env.get("CI").map(String::as_str), Some("true"));
    assert_eq!(parsed.timeout_ms, 300_000);
    assert_eq!(parsed.accept_exit_codes, [0]);

    let result: Value = serde_json::from_str(include_str!(
      "../../../woml/tests/fixtures/services-contracts/process-result.v1.json"
    ))
    .expect("standalone Process v1 result fixture");
    let capability_result: Value = serde_json::from_str(include_str!(
      "../../../woml/tests/fixtures/services-contracts/capability-process-succeeded.v1.json"
    ))
    .expect("Capability Call v1 Process result fixture");
    assert_eq!(capability_result["result"], result);
  }

  fn policy() -> ProcessPolicyV1 {
    ProcessPolicyV1 {
      contract: PROCESS_POLICY_CONTRACT.to_string(),
      contract_version: PROCESS_POLICY_CONTRACT_VERSION,
      enabled: true,
      modes: ProcessPolicyModes {
        foreground: true,
        background: false,
      },
      project_root: std::env::current_dir()
        .expect("current directory")
        .to_string_lossy()
        .into_owned(),
      executables: vec![ProcessPolicyExecutable {
        alias: "fixture".to_string(),
        path: std::env::current_exe()
          .expect("current test executable")
          .to_string_lossy()
          .into_owned(),
      }],
      working_directories: vec![".".to_string()],
      environment: ProcessPolicyEnvironment {
        inherit: Vec::new(),
        allow: vec![
          "WOML_PROCESS_FIXTURE".to_string(),
          "WOML_PROCESS_FIXTURE_TEXT".to_string(),
          "WOML_PROCESS_FIXTURE_EXIT".to_string(),
          "WOML_PROCESS_FIXTURE_TREE".to_string(),
          "WOML_PROCESS_FIXTURE_HEARTBEAT".to_string(),
          "WOML_PROCESS_FIXTURE_CWD".to_string(),
          "WOML_PROCESS_FIXTURE_ENV_NAME".to_string(),
          "WOML_PROCESS_FIXTURE_PRINT_ENV".to_string(),
          "PATH".to_string(),
        ],
      },
      limits: ProcessPolicyLimits {
        max_timeout_ms: 5_000,
        max_output_bytes: 1_024,
        max_concurrent: 1,
      },
    }
  }

  fn request(environment: Value, max_bytes: usize, accepted: Vec<i32>) -> Value {
    json!({
      "contract": PROCESS_CONTRACT,
      "contractVersion": PROCESS_CONTRACT_VERSION,
      "kind": "request",
      "executable": "fixture",
      "args": ["--exact", "process::tests::fixture_child", "--nocapture"],
      "cwd": ".",
      "env": environment,
      "timeoutMs": 5_000,
      "acceptExitCodes": accepted,
      "output": { "stdout": "capture", "stderr": "capture", "maxBytes": max_bytes }
    })
  }

  fn with_cwd(mut request: Value, cwd: &str) -> Value {
    request["cwd"] = json!(cwd);
    request
  }

  fn temporary_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "woml-process-{label}-{}",
      uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&path).expect("create temporary directory");
    path
  }

  #[test]
  fn fixture_child() {
    if std::env::var("WOML_PROCESS_FIXTURE").as_deref() != Ok("1") {
      return;
    }
    if let Ok(role) = std::env::var("WOML_PROCESS_FIXTURE_TREE") {
      let heartbeat =
        std::env::var("WOML_PROCESS_FIXTURE_HEARTBEAT").expect("tree fixture heartbeat path");
      if role == "parent" {
        std::process::Command::new(std::env::current_exe().expect("fixture executable"))
          .args(["--exact", "process::tests::fixture_child", "--nocapture"])
          .env_clear()
          .env("WOML_PROCESS_FIXTURE", "1")
          .env("WOML_PROCESS_FIXTURE_TREE", "grandchild")
          .env("WOML_PROCESS_FIXTURE_HEARTBEAT", &heartbeat)
          .stdin(Stdio::null())
          .stdout(Stdio::null())
          .stderr(Stdio::null())
          .spawn()
          .expect("spawn grandchild fixture");
      }
      let marker = if role == "parent" { b'P' } else { b'G' };
      loop {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
          .create(true)
          .append(true)
          .open(&heartbeat)
          .expect("open heartbeat");
        file.write_all(&[marker]).expect("write heartbeat");
        file.sync_all().expect("sync heartbeat");
        std::thread::sleep(Duration::from_millis(10));
      }
    }
    if std::env::var("WOML_PROCESS_FIXTURE_CWD").as_deref() == Ok("1") {
      print!(
        "{}",
        std::env::current_dir()
          .expect("fixture cwd")
          .to_string_lossy()
      );
      return;
    }
    if let Ok(name) = std::env::var("WOML_PROCESS_FIXTURE_ENV_NAME") {
      print!(
        "{}",
        if std::env::var_os(name).is_some() {
          "present"
        } else {
          "absent"
        }
      );
      return;
    }
    if let Ok(name) = std::env::var("WOML_PROCESS_FIXTURE_PRINT_ENV") {
      print!("{}", std::env::var(name).unwrap_or_default());
      return;
    }
    let text = std::env::var("WOML_PROCESS_FIXTURE_TEXT").unwrap_or_default();
    print!("{text}");
    if let Ok(code) = std::env::var("WOML_PROCESS_FIXTURE_EXIT") {
      std::process::exit(code.parse().expect("fixture exit code"));
    }
  }

  #[tokio::test]
  async fn approved_process_returns_bounded_output() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_TEXT": "abcdefghijklmnopqrstuvwxyz"
          }),
          10,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("approved process succeeds");
    assert_eq!(value["contract"], PROCESS_CONTRACT);
    assert_eq!(value["exitCode"], 0);
    assert_eq!(value["stdoutTruncated"], true);
    assert!(value["stdoutBytes"].as_u64().expect("byte count") > 10);
    assert!(value["stdout"].as_str().expect("stdout").len() <= 10);
  }

  #[tokio::test]
  async fn inherited_environment_values_are_redacted_from_captured_output() {
    let mut configured = policy();
    configured.environment.inherit = vec!["PATH".to_string()];
    let handler = ManagedProcessHandler::new(configured, ProcessRuntimeMode::Foreground)
      .expect("valid inherited environment policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_PRINT_ENV": "PATH"
          }),
          1_024,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("approved process succeeds");
    assert!(value["stdout"]
      .as_str()
      .expect("stdout")
      .contains("[redacted]"));
    assert!(!value
      .to_string()
      .contains(&std::env::var("PATH").expect("host PATH")));
  }

  #[tokio::test]
  async fn ambient_environment_is_cleared_unless_explicitly_inherited() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_ENV_NAME": "HOME"
          }),
          128,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("approved process succeeds");
    assert!(value["stdout"].as_str().expect("stdout").contains("absent"));
  }

  #[tokio::test]
  async fn policy_inherits_only_names_the_operator_approved() {
    let mut configured = policy();
    configured.environment.inherit = vec!["PATH".to_string()];
    let handler = ManagedProcessHandler::new(configured, ProcessRuntimeMode::Foreground)
      .expect("valid inherited environment policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_ENV_NAME": "PATH"
          }),
          128,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("approved inherited variable is available");
    assert!(value["stdout"]
      .as_str()
      .expect("stdout")
      .contains("present"));
  }

  #[tokio::test]
  async fn path_shadowing_cannot_change_the_preflighted_executable() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_TEXT": "approved-binary-ran",
            "PATH": "/definitely/not/the/operator/path"
          }),
          128,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("canonical executable is used directly");
    assert_eq!(value["exitCode"], 0);
  }

  #[tokio::test]
  async fn cwd_is_resolved_from_the_policy_project_root() {
    let root = temporary_directory("project-root");
    let mut configured = policy();
    configured.project_root = root.to_string_lossy().into_owned();
    configured.working_directories = vec![".".to_string()];
    let handler = ManagedProcessHandler::new(configured, ProcessRuntimeMode::Foreground)
      .expect("valid project policy");
    let value = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_CWD": "1"
          }),
          512,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect("approved process succeeds");
    assert!(value["stdout"]
      .as_str()
      .expect("stdout")
      .contains(root.to_string_lossy().as_ref()));
    let _ = std::fs::remove_dir(root);
  }

  #[cfg(unix)]
  #[test]
  fn symlink_cannot_escape_an_allowed_working_directory() {
    use std::os::unix::fs::symlink;
    let root = temporary_directory("symlink-root");
    let outside = temporary_directory("symlink-outside");
    symlink(&outside, root.join("escape")).expect("create symlink");
    let mut configured = policy();
    configured.project_root = root.to_string_lossy().into_owned();
    configured.working_directories = vec![".".to_string()];
    let handler = ManagedProcessHandler::new(configured, ProcessRuntimeMode::Foreground)
      .expect("valid project policy");
    let error = handler
      .parse_and_authorize(&with_cwd(request(json!({}), 128, vec![0]), "escape"))
      .expect_err("symlink escape denied");
    assert_eq!(error.code, "WOML_PROCESS_CWD_DENIED");
    let _ = std::fs::remove_file(root.join("escape"));
    let _ = std::fs::remove_dir(root);
    let _ = std::fs::remove_dir(outside);
  }

  #[cfg(unix)]
  #[test]
  fn executable_replacement_is_detected_after_preflight() {
    let root = temporary_directory("executable-identity");
    let executable = root.join("fixture");
    std::fs::copy(
      std::env::current_exe().expect("current executable"),
      &executable,
    )
    .expect("copy executable");
    let mut configured = policy();
    configured.executables[0].path = executable.to_string_lossy().into_owned();
    let handler = ManagedProcessHandler::new(configured, ProcessRuntimeMode::Foreground)
      .expect("valid executable policy");
    let replacement = root.join("replacement");
    std::fs::copy(
      std::env::current_exe().expect("current executable"),
      &replacement,
    )
    .expect("copy replacement");
    std::fs::rename(&replacement, &executable).expect("replace executable");
    let error = handler
      .parse_and_authorize(&request(json!({}), 128, vec![0]))
      .expect_err("replacement denied");
    assert_eq!(error.code, "WOML_PROCESS_EXECUTABLE_CHANGED");
    let _ = std::fs::remove_file(executable);
    let _ = std::fs::remove_dir(root);
  }

  #[test]
  fn validation_rejects_device_paths_and_resource_exhaustion() {
    for cwd in ["NUL", "con.txt", "COM1", "LPT9.log", "folder. "] {
      let error = parse_request(&with_cwd(request(json!({}), 128, vec![0]), cwd))
        .expect_err("portable device path denied");
      assert_eq!(error.code, "WOML_PROCESS_INPUT_INVALID");
    }
    let too_many_args = (0..257).map(|_| "x").collect::<Vec<_>>();
    let mut value = request(json!({}), 128, vec![0]);
    value["args"] = json!(too_many_args);
    assert_eq!(
      parse_request(&value)
        .expect_err("argument count bounded")
        .code,
      "WOML_PROCESS_INPUT_INVALID"
    );
    value = request(json!({}), 128, vec![0]);
    value["args"] = json!(["x".repeat(32_769)]);
    assert_eq!(
      parse_request(&value)
        .expect_err("argument size bounded")
        .code,
      "WOML_PROCESS_INPUT_INVALID"
    );
    let environment = (0..129)
      .map(|index| (format!("SAFE_{index}"), "x"))
      .collect::<BTreeMap<_, _>>();
    assert_eq!(
      parse_request(&request(json!(environment), 128, vec![0]))
        .expect_err("environment count bounded")
        .code,
      "WOML_PROCESS_INPUT_INVALID"
    );
    assert_eq!(
      parse_request(&request(
        json!({ "SAFE": "x".repeat(32_769) }),
        128,
        vec![0]
      ))
      .expect_err("environment value bounded")
      .code,
      "WOML_PROCESS_INPUT_INVALID"
    );
  }

  #[test]
  fn generated_process_requests_are_validated_without_panics() {
    let alphabet = [b'a', b'Z', b'0', b'.', b'/', b'\\', b':', b' ', 0, 0xff];
    let mut state = 0x5eed_u64;
    for length in 0..256 {
      let mut bytes = Vec::with_capacity(length);
      for _ in 0..length {
        state = state
          .wrapping_mul(6_364_136_223_846_793_005)
          .wrapping_add(1);
        bytes.push(alphabet[(state as usize) % alphabet.len()]);
      }
      let candidate = String::from_utf8_lossy(&bytes).into_owned();
      let value = with_cwd(request(json!({}), 128, vec![0]), &candidate);
      let _ = parse_request(&value);
    }
  }

  #[test]
  fn every_process_failure_has_an_author_facing_corrective_action() {
    for code in [
      "WOML_PROCESS_EXECUTABLE_DENIED",
      "WOML_PROCESS_EXECUTABLE_NOT_FOUND",
      "WOML_PROCESS_EXECUTABLE_CHANGED",
      "WOML_PROCESS_CWD_DENIED",
      "WOML_PROCESS_ENV_DENIED",
      "WOML_PROCESS_SPAWN_FAILED",
      "WOML_PROCESS_TIMED_OUT",
      "WOML_PROCESS_CANCELLED",
      "WOML_PROCESS_EXIT_CODE",
      "WOML_PROCESS_INPUT_INVALID",
    ] {
      let error = failure(
        CapabilityFailureKind::ServiceRejected,
        code,
        "Safe process failure.",
        false,
        false,
        None,
      );
      assert!(error.message.contains("Corrective action:"), "{code}");
      assert!(!error.message.contains('/') && !error.message.contains('\\'));
    }
  }

  #[tokio::test]
  async fn nonaccepted_exit_code_has_only_safe_failure_metadata() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let error = handler
      .execute(
        request(
          json!({
            "WOML_PROCESS_FIXTURE": "1",
            "WOML_PROCESS_FIXTURE_TEXT": "sensitive output",
            "WOML_PROCESS_FIXTURE_EXIT": "7"
          }),
          128,
          vec![0],
        ),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect_err("exit code is rejected");
    assert_eq!(error.code, "WOML_PROCESS_EXIT_CODE");
    let details = error.details.expect("safe details");
    assert_eq!(details.get("exitCode"), Some(&json!(7)));
    assert!(!details.contains_key("stdout"));
    assert!(!details.contains_key("stderr"));
  }

  #[tokio::test]
  async fn request_environment_is_enforced_again_in_rust() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let error = handler
      .execute(
        request(json!({ "UNAPPROVED_SECRET": "no" }), 128, vec![0]),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect_err("environment is denied");
    assert_eq!(error.code, "WOML_PROCESS_ENV_DENIED");
  }

  #[test]
  fn runtime_policy_caps_the_effective_process_timeout() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let mut input = request(json!({ "WOML_PROCESS_FIXTURE": "1" }), 128, vec![0]);
    input["timeoutMs"] = json!(10_000);
    let authorized = handler
      .parse_and_authorize(&input)
      .expect("timeout is bounded by policy");
    assert_eq!(authorized.request.timeout_ms, 5_000);
  }

  #[test]
  fn durable_metadata_excludes_arguments_environment_and_output() {
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let input = request(
      json!({
        "WOML_PROCESS_FIXTURE": "1",
        "WOML_PROCESS_FIXTURE_TEXT": "never-persist-this-secret"
      }),
      128,
      vec![0],
    );
    let metadata = handler.safe_metadata(&input);
    let encoded = serde_json::to_string(&metadata).expect("metadata JSON");
    assert_eq!(metadata.get("executable"), Some(&json!("fixture")));
    assert_eq!(metadata.get("argumentCount"), Some(&json!(3)));
    assert!(!encoded.contains("never-persist-this-secret"));
    assert!(!encoded.contains("--exact"));
    assert!(!metadata.contains_key("args"));
    assert!(!metadata.contains_key("env"));
    assert!(!metadata.contains_key("stdout"));
    assert!(!metadata.contains_key("stderr"));
  }

  async fn assert_heartbeat_stops(path: &Path) {
    tokio::time::sleep(Duration::from_millis(150)).await;
    let first = std::fs::metadata(path).expect("heartbeat exists").len();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let second = std::fs::metadata(path).expect("heartbeat remains").len();
    assert_eq!(first, second, "a descendant survived containment shutdown");
  }

  fn tree_request(path: &Path, timeout_ms: u64) -> Value {
    let mut value = request(
      json!({
        "WOML_PROCESS_FIXTURE": "1",
        "WOML_PROCESS_FIXTURE_TREE": "parent",
        "WOML_PROCESS_FIXTURE_HEARTBEAT": path.to_string_lossy()
      }),
      128,
      vec![0],
    );
    value["timeoutMs"] = json!(timeout_ms);
    value
  }

  #[cfg(any(unix, windows))]
  #[tokio::test]
  async fn timeout_terminates_child_and_grandchild_tree() {
    let heartbeat = std::env::temp_dir().join(format!(
      "woml-process-timeout-heartbeat-{}",
      uuid::Uuid::new_v4()
    ));
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let error = handler
      .execute(
        tree_request(&heartbeat, 250),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect_err("tree times out");
    assert_eq!(error.kind, CapabilityFailureKind::TimedOut);
    assert_eq!(error.code, "WOML_PROCESS_TIMED_OUT");
    assert!(error.ambiguous);
    assert_heartbeat_stops(&heartbeat).await;
    let _ = std::fs::remove_file(heartbeat);
  }

  #[cfg(any(unix, windows))]
  #[tokio::test]
  async fn cancellation_terminates_tree_and_cannot_report_success() {
    let heartbeat = std::env::temp_dir().join(format!(
      "woml-process-cancel-heartbeat-{}",
      uuid::Uuid::new_v4()
    ));
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let cancellation = CapabilityCancellationToken::default();
    let cancel = cancellation.clone();
    let request = tree_request(&heartbeat, 5_000);
    let execution = tokio::spawn(async move { handler.execute(request, cancellation).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    cancel.cancel();
    let error = execution
      .await
      .expect("execution task")
      .expect_err("tree is cancelled");
    assert_eq!(error.kind, CapabilityFailureKind::Cancelled);
    assert_eq!(error.code, "WOML_PROCESS_CANCELLED");
    assert!(error.ambiguous);
    assert_heartbeat_stops(&heartbeat).await;
    let _ = std::fs::remove_file(heartbeat);
  }

  #[cfg(any(unix, windows))]
  #[tokio::test]
  async fn concurrency_ceiling_rejects_excess_processes_without_queueing() {
    let heartbeat = std::env::temp_dir().join(format!(
      "woml-process-concurrency-heartbeat-{}",
      uuid::Uuid::new_v4()
    ));
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let cancellation = CapabilityCancellationToken::default();
    let running_handler = handler.clone();
    let running_cancellation = cancellation.clone();
    let running_request = tree_request(&heartbeat, 5_000);
    let running = tokio::spawn(async move {
      running_handler
        .execute(running_request, running_cancellation)
        .await
    });
    for _ in 0..100 {
      if heartbeat.exists() {
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(heartbeat.exists(), "first process did not start");
    let error = handler
      .execute(
        request(json!({ "WOML_PROCESS_FIXTURE": "1" }), 128, vec![0]),
        CapabilityCancellationToken::default(),
      )
      .await
      .expect_err("second process exceeds the policy ceiling");
    assert_eq!(error.code, "WOML_PROCESS_CONCURRENCY_LIMIT");
    cancellation.cancel();
    let _ = running.await;
    assert_heartbeat_stops(&heartbeat).await;
    let _ = std::fs::remove_file(heartbeat);
  }

  #[cfg(any(unix, windows))]
  #[tokio::test]
  async fn dropped_handler_future_closes_the_process_tree() {
    let heartbeat = std::env::temp_dir().join(format!(
      "woml-process-host-stop-heartbeat-{}",
      uuid::Uuid::new_v4()
    ));
    let handler =
      ManagedProcessHandler::new(policy(), ProcessRuntimeMode::Foreground).expect("valid policy");
    let request = tree_request(&heartbeat, 5_000);
    let execution = tokio::spawn(async move {
      handler
        .execute(request, CapabilityCancellationToken::default())
        .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    execution.abort();
    let _ = execution.await;
    assert_heartbeat_stops(&heartbeat).await;
    let _ = std::fs::remove_file(heartbeat);
  }
}
