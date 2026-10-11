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
  #[error("WOML_PROCESS_DISABLED: process execution is disabled by runtime policy")]
  Disabled,
  #[error("WOML_PROCESS_MODE_DENIED: process execution is not allowed in {0} mode")]
  ModeDenied(&'static str),
  #[error("WOML_PROCESS_POLICY_INVALID: {0}")]
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
  executables: HashMap<String, PathBuf>,
  working_directories: Vec<PathBuf>,
  inherited_environment: BTreeMap<String, String>,
  allowed_environment: BTreeSet<String>,
  limits: ProcessPolicyLimits,
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
      .cloned()
      .ok_or_else(|| {
        rejected(
          "WOML_PROCESS_EXECUTABLE_DENIED",
          "The requested executable alias is not allowed by runtime policy.",
        )
      })?;
    verify_executable(&executable)?;
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
      executable,
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
      verify_executable_policy(&canonical, &entry.alias)?;
      if executables.insert(entry.alias.clone(), canonical).is_some() {
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
      false,
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
  ));
  let stderr_task = tokio::spawn(drain_output(
    stderr,
    request.output.stderr,
    request.output.max_bytes,
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
  let stdout = collect_output_task(stdout_task).await?;
  let stderr = collect_output_task(stderr_task).await?;
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
) -> Result<CapturedOutput, CapabilityFailure> {
  let mut captured = Vec::with_capacity(match mode {
    OutputMode::Capture => max_bytes.min(64 * 1024),
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
    if matches!(mode, OutputMode::Capture) && captured.len() < max_bytes {
      let remaining = max_bytes - captured.len();
      captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
  }
  Ok(CapturedOutput {
    text: String::from_utf8_lossy(&captured).into_owned(),
    bytes: total,
    truncated: matches!(mode, OutputMode::Capture) && total > captured.len() as u64,
  })
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
    || value.split(['/', '\\']).any(|component| component == "..")
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

fn verify_executable_policy(path: &Path, alias: &str) -> Result<(), ProcessPolicyError> {
  let metadata = std::fs::metadata(path).map_err(|_| {
    ProcessPolicyError::Invalid(format!("executable alias '{alias}' is unavailable"))
  })?;
  if !metadata.is_file() || !platform_executable(&metadata) {
    return Err(ProcessPolicyError::Invalid(format!(
      "executable alias '{alias}' does not point to an executable file"
    )));
  }
  Ok(())
}

fn verify_executable(path: &Path) -> Result<(), CapabilityFailure> {
  let canonical = std::fs::canonicalize(path).map_err(|_| {
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
  if canonical != path || !metadata.is_file() || !platform_executable(&metadata) {
    return Err(rejected(
      "WOML_PROCESS_EXECUTABLE_CHANGED",
      "The approved executable changed after runtime preflight.",
    ));
  }
  Ok(())
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
    "WOML_PROCESS_INVALID_INPUT",
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
    message: message.to_string(),
    retryable,
    ambiguous,
    details,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
