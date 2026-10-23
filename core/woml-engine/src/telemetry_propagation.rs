//! Transient W3C trace-context handling for reviewed WOML boundaries.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{span_id_v1, trace_id_v1, TelemetrySignalKind};

pub const TELEMETRY_ADMISSION_LINK_PROFILE: &str = "woml.telemetry-admission-link/v1";
pub const MAX_TRACESTATE_BYTES: usize = 512;
const MAX_TRACESTATE_MEMBERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct W3cTraceContext {
  pub trace_id: String,
  pub span_id: String,
  pub sampled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryAdmissionLinkStatus {
  Linked,
  Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryAdmissionLinkV1 {
  pub profile: &'static str,
  pub occurred_at: DateTime<Utc>,
  pub runtime_instance_id: String,
  pub deployment_id: String,
  pub run_id: String,
  pub status: TelemetryAdmissionLinkStatus,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trace_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub span_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub sampled: Option<bool>,
}

pub type TelemetryAdmissionLinkReporter = Arc<dyn Fn(TelemetryAdmissionLinkV1) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundTraceContext {
  Absent,
  Valid(W3cTraceContext),
  Invalid,
}

/// Parses only the reviewed W3C v00 shape. Header values are normalized into
/// identifiers and are never retained verbatim.
pub fn parse_w3c_trace_context(
  traceparent: Option<&str>,
  tracestate: Option<&str>,
) -> InboundTraceContext {
  let Some(traceparent) = traceparent else {
    return if tracestate.is_some() {
      InboundTraceContext::Invalid
    } else {
      InboundTraceContext::Absent
    };
  };
  if !valid_tracestate(tracestate) {
    return InboundTraceContext::Invalid;
  }
  let bytes = traceparent.as_bytes();
  if bytes.len() != 55 || bytes[2] != b'-' || bytes[35] != b'-' || bytes[52] != b'-' {
    return InboundTraceContext::Invalid;
  }
  let version = &traceparent[0..2];
  let trace_id = &traceparent[3..35];
  let span_id = &traceparent[36..52];
  let flags = &traceparent[53..55];
  if version.eq_ignore_ascii_case("ff")
    || ![version, trace_id, span_id, flags]
      .into_iter()
      .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
    || trace_id.bytes().all(|byte| byte == b'0')
    || span_id.bytes().all(|byte| byte == b'0')
  {
    return InboundTraceContext::Invalid;
  }
  let flags = u8::from_str_radix(flags, 16).expect("validated W3C flags");
  InboundTraceContext::Valid(W3cTraceContext {
    trace_id: trace_id.to_ascii_lowercase(),
    span_id: span_id.to_ascii_lowercase(),
    sampled: flags & 1 == 1,
  })
}

fn valid_tracestate(value: Option<&str>) -> bool {
  let Some(value) = value else { return true };
  !value.is_empty()
    && value.len() <= MAX_TRACESTATE_BYTES
    && value.split(',').count() <= MAX_TRACESTATE_MEMBERS
    && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    && value
      .split(',')
      .all(|member| !member.trim().is_empty() && member.contains('='))
}

pub fn woml_trace_sampled(run_id: &str, ratio: f64) -> bool {
  if ratio <= 0.0 {
    return false;
  }
  if ratio >= 1.0 {
    return true;
  }
  let trace_id = trace_id_v1(run_id);
  let prefix = u64::from_str_radix(&trace_id[..13], 16).expect("WOML trace ID is hexadecimal");
  (prefix as f64) / 4_503_599_627_370_496.0 < ratio
}

pub fn woml_traceparent(
  run_id: &str,
  node_id: &str,
  attempt: u32,
  invocation_id: &str,
  sampled: bool,
) -> String {
  let operation = format!("{node_id}:{attempt}:{invocation_id}");
  format!(
    "00-{}-{}-{}",
    trace_id_v1(run_id),
    span_id_v1(run_id, TelemetrySignalKind::Attempt.as_str(), &operation),
    if sampled { "01" } else { "00" }
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn accepts_bounded_v00_context_without_retaining_tracestate() {
    assert_eq!(
      parse_w3c_trace_context(
        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        Some("vendor=value")
      ),
      InboundTraceContext::Valid(W3cTraceContext {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
        span_id: "00f067aa0ba902b7".to_string(),
        sampled: true,
      })
    );
  }

  #[test]
  fn ignores_absence_and_rejects_spoofed_or_oversized_context() {
    assert_eq!(
      parse_w3c_trace_context(None, None),
      InboundTraceContext::Absent
    );
    for value in [
      "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
      "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
      "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
      "not-a-traceparent",
    ] {
      assert_eq!(
        parse_w3c_trace_context(Some(value), None),
        InboundTraceContext::Invalid
      );
    }
    assert_eq!(
      parse_w3c_trace_context(
        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        Some(&"x".repeat(MAX_TRACESTATE_BYTES + 1))
      ),
      InboundTraceContext::Invalid
    );
  }

  #[test]
  fn creates_a_woml_owned_attempt_context() {
    let value = woml_traceparent("run_42", "deploy", 2, "inv_7", true);
    assert!(value.starts_with("00-"));
    assert_eq!(value.len(), 55);
    assert!(matches!(
      parse_w3c_trace_context(Some(&value), None),
      InboundTraceContext::Valid(_)
    ));
    assert!(woml_trace_sampled("run_42", 1.0));
    assert!(!woml_trace_sampled("run_42", 0.0));
  }
}
