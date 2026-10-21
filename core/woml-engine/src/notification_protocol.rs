use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::event::{
  valid_provider_message, ApprovalDecision, NotificationResolution, NotificationSafeFailure,
};
use crate::ProviderMessageIdentity;

pub const NOTIFICATION_PROVIDER_PROTOCOL: &str = "woml.notification-provider-host";
pub const NOTIFICATION_PROVIDER_PROTOCOL_VERSION: u32 = 1;
pub const INFORMATIONAL_NOTIFICATION_PROVIDER_PROTOCOL_VERSION: u32 = 2;
pub const RICH_APPROVAL_NOTIFICATION_PROVIDER_PROTOCOL_VERSION: u32 = 3;
pub const NOTIFICATION_PROVIDER_MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationReadyMessage {
  pub protocol: String,
  pub protocol_version: u32,
  pub message_type: String,
  pub host_instance_id: String,
  pub providers: Vec<String>,
}

impl NotificationReadyMessage {
  pub fn validate(&self) -> Result<(), String> {
    self.validate_for(NOTIFICATION_PROVIDER_PROTOCOL_VERSION)
  }

  pub fn validate_for(&self, protocol_version: u32) -> Result<(), String> {
    if self.protocol != NOTIFICATION_PROVIDER_PROTOCOL
      || self.protocol_version != protocol_version
      || self.message_type != "ready"
      || self.host_instance_id.is_empty()
      || self.host_instance_id.chars().count() > 320
      || (self.providers != ["slack"]
        && self.providers != ["slack", "telegram"]
        && self.providers != ["slack", "telegram", "discord"]
        && self.providers != ["slack", "telegram", "discord", "whatsapp"])
    {
      return Err(
        "The child did not send a valid notification-provider ready message.".to_string(),
      );
    }
    Ok(())
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSecretReference {
  pub kind: &'static str,
  pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum NotificationCredentials {
  Slack {
    #[serde(rename = "botToken")]
    bot_token: NotificationSecretReference,
    #[serde(rename = "appToken")]
    app_token: NotificationSecretReference,
  },
  Bot {
    #[serde(rename = "botToken")]
    bot_token: NotificationSecretReference,
  },
  WhatsApp {
    #[serde(rename = "accessToken")]
    access_token: NotificationSecretReference,
    #[serde(rename = "phoneNumberId")]
    phone_number_id: String,
  },
}

impl NotificationCredentials {
  pub fn from_symbolic(credentials: &BTreeMap<String, String>) -> Result<Self, String> {
    if let (Some(access_token), Some(phone_number_id)) = (
      credentials.get("accessToken"),
      credentials.get("phoneNumberId"),
    ) {
      if credentials.len() != 2 {
        return Err(
          "WhatsApp notification credentials contain an unsupported binding.".to_string(),
        );
      }
      return Ok(Self::whatsapp(
        access_token.clone(),
        phone_number_id.clone(),
      ));
    }
    let bot_token = credentials
      .get("botToken")
      .ok_or_else(|| "Notification credentials are missing botToken.".to_string())?;
    let bot_token = NotificationSecretReference {
      kind: "secretReference",
      name: bot_token.clone(),
    };
    match (credentials.len(), credentials.get("appToken")) {
      (1, None) => Ok(Self::Bot { bot_token }),
      (2, Some(app_token)) => Ok(Self::Slack {
        bot_token,
        app_token: NotificationSecretReference {
          kind: "secretReference",
          name: app_token.clone(),
        },
      }),
      _ => Err("Notification credentials contain an unsupported binding.".to_string()),
    }
  }

  pub fn whatsapp(access_token: String, phone_number_id: String) -> Self {
    Self::WhatsApp {
      access_token: NotificationSecretReference {
        kind: "secretReference",
        name: access_token,
      },
      phone_number_id,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationApprovalMessage {
  pub workflow_id: String,
  pub approval_name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub approval_description: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDeliverMessage {
  pub protocol: &'static str,
  pub protocol_version: u32,
  pub message_type: &'static str,
  pub invocation_id: String,
  pub run_id: String,
  pub approval_id: String,
  pub request_id: String,
  pub delivery_id: String,
  pub provider: String,
  pub destination: String,
  pub idempotency_key: String,
  pub credentials: NotificationCredentials,
  pub decision_capability: String,
  pub message: NotificationApprovalMessage,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub template_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InformationalNotificationDeliverMessage {
  pub protocol: &'static str,
  pub protocol_version: u32,
  pub message_type: &'static str,
  pub mode: &'static str,
  pub invocation_id: String,
  pub run_id: String,
  pub hook_invocation_id: String,
  pub action_id: String,
  pub delivery_id: String,
  pub provider: String,
  pub destination: String,
  pub idempotency_key: String,
  pub credentials: NotificationCredentials,
  pub message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub template_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RichApprovalNotificationAuthority {
  pub required: u32,
  pub received: u32,
  pub required_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RichApprovalNotificationPresentation {
  pub title: String,
  pub summary: String,
  pub risk: crate::ReviewRiskV1,
  pub authority: RichApprovalNotificationAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RichApprovalDecisionCapability {
  pub url: String,
  pub token: String,
  pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RichApprovalNotificationDeliverMessage {
  pub protocol: String,
  pub protocol_version: u32,
  pub message_type: String,
  pub mode: String,
  pub invocation_id: String,
  pub run_id: String,
  pub approval_id: String,
  pub request_id: String,
  pub delivery_id: String,
  pub provider: String,
  pub destination: String,
  pub review_digest: String,
  pub presentation: RichApprovalNotificationPresentation,
  pub decision_capability: RichApprovalDecisionCapability,
}

impl RichApprovalNotificationDeliverMessage {
  pub fn validate(&self) -> Result<(), String> {
    let valid_roles = self.presentation.authority.required_roles.len() <= 16
      && self
        .presentation
        .authority
        .required_roles
        .iter()
        .all(|role| {
          !role.is_empty()
            && role.chars().count() <= 64
            && role.chars().all(|character| {
              character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.')
            })
        });
    if self.protocol != NOTIFICATION_PROVIDER_PROTOCOL
      || self.protocol_version != RICH_APPROVAL_NOTIFICATION_PROVIDER_PROTOCOL_VERSION
      || self.message_type != "deliver"
      || self.mode != "rich-approval"
      || self.invocation_id.is_empty()
      || self.invocation_id.chars().count() > 320
      || self.run_id.is_empty()
      || self.run_id.chars().count() > 320
      || self.approval_id.is_empty()
      || self.request_id.is_empty()
      || self.delivery_id.is_empty()
      || !matches!(
        self.provider.as_str(),
        "slack" | "telegram" | "discord" | "custom"
      )
      || self.destination.is_empty()
      || self.destination.chars().count() > 320
      || !valid_sha256_digest(&self.review_digest)
      || self.presentation.title.is_empty()
      || self.presentation.title.chars().count() > 160
      || self.presentation.summary.chars().count() > 2_000
      || !(1..=20).contains(&self.presentation.authority.required)
      || self.presentation.authority.received > self.presentation.authority.required
      || !valid_roles
      || self.decision_capability.url.is_empty()
      || self.decision_capability.url.chars().count() > 4_096
      || self.decision_capability.token.len() < 32
      || self.decision_capability.token.len() > 4_096
    {
      return Err("The provider host received an invalid rich approval presentation.".to_string());
    }
    Ok(())
  }
}

fn valid_sha256_digest(value: &str) -> bool {
  value.len() == 71
    && value.starts_with("sha256:")
    && value[7..]
      .bytes()
      .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationUpdateMessage {
  pub protocol: &'static str,
  pub protocol_version: u32,
  pub message_type: &'static str,
  pub invocation_id: String,
  pub run_id: String,
  pub approval_id: String,
  pub request_id: String,
  pub delivery_id: String,
  pub update_id: String,
  pub idempotency_key: String,
  pub provider: String,
  pub credentials: NotificationCredentials,
  pub provider_message: ProviderMessageIdentity,
  pub resolution: NotificationResolution,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationCompletedMessage {
  pub protocol: String,
  pub protocol_version: u32,
  pub message_type: String,
  pub invocation_id: String,
  pub outcome: NotificationHostOutcome,
  pub duration_ms: f64,
}

impl NotificationCompletedMessage {
  pub fn validate(&self) -> Result<(), String> {
    self.validate_for(NOTIFICATION_PROVIDER_PROTOCOL_VERSION)
  }

  pub fn validate_for(&self, protocol_version: u32) -> Result<(), String> {
    if self.protocol != NOTIFICATION_PROVIDER_PROTOCOL
      || self.protocol_version != protocol_version
      || self.message_type != "completed"
      || self.invocation_id.is_empty()
      || self.invocation_id.chars().count() > 320
      || !self.duration_ms.is_finite()
      || self.duration_ms < 0.0
    {
      return Err("The provider host returned an invalid completion envelope.".to_string());
    }
    match &self.outcome {
      NotificationHostOutcome::DeliverySuccess { provider_message } => {
        if valid_provider_message(provider_message) {
          Ok(())
        } else {
          Err("The provider host returned an invalid provider message identity.".to_string())
        }
      }
      NotificationHostOutcome::UpdateSuccess => {
        if protocol_version == NOTIFICATION_PROVIDER_PROTOCOL_VERSION {
          Ok(())
        } else {
          Err("Informational notification delivery cannot update messages.".to_string())
        }
      }
      NotificationHostOutcome::Failure { error } => {
        if protocol_version == INFORMATIONAL_NOTIFICATION_PROVIDER_PROTOCOL_VERSION
          && error.kind == "update_failed"
        {
          return Err(
            "Informational notification delivery returned an approval-only failure kind."
              .to_string(),
          );
        }
        error.validate().map_err(|error| error.to_string())
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationHostOutcome {
  DeliverySuccess {
    #[serde(rename = "providerMessage")]
    provider_message: ProviderMessageIdentity,
  },
  UpdateSuccess,
  Failure {
    error: NotificationSafeFailure,
  },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationInteractionMessage {
  pub protocol: String,
  pub protocol_version: u32,
  pub message_type: String,
  pub interaction_id: String,
  pub delivery_id: String,
  pub provider: String,
  pub decision_capability: String,
  pub decision: ApprovalDecision,
  pub provider_actor_id: String,
  pub occurred_at: DateTime<Utc>,
}

impl NotificationInteractionMessage {
  pub fn validate(&self) -> Result<(), String> {
    if self.protocol != NOTIFICATION_PROVIDER_PROTOCOL
      || self.protocol_version != NOTIFICATION_PROVIDER_PROTOCOL_VERSION
      || self.message_type != "interaction"
      || self.interaction_id.is_empty()
      || self.interaction_id.chars().count() > 320
      || self.provider != "slack"
      || self.delivery_id.is_empty()
      || self.decision_capability.len() < 43
      || self.decision_capability.len() > 512
      || !valid_slack_actor(&self.provider_actor_id)
    {
      return Err("The provider host returned an invalid interaction message.".to_string());
    }
    Ok(())
  }
}

fn valid_slack_actor(value: &str) -> bool {
  value.len() >= 9
    && value.len() <= 32
    && value.starts_with('U')
    && value[1..]
      .bytes()
      .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn provider_host_accepts_discord_readiness_and_message_identity() {
    let ready: NotificationReadyMessage = serde_json::from_value(serde_json::json!({
      "protocol": NOTIFICATION_PROVIDER_PROTOCOL,
      "protocolVersion": NOTIFICATION_PROVIDER_PROTOCOL_VERSION,
      "messageType": "ready",
      "hostInstanceId": "notification-host-discord",
      "providers": ["slack", "telegram", "discord"]
    }))
    .expect("Discord-ready host message");
    ready.validate().expect("Discord-ready provider host");

    let completed: NotificationCompletedMessage = serde_json::from_value(serde_json::json!({
      "protocol": NOTIFICATION_PROVIDER_PROTOCOL,
      "protocolVersion": NOTIFICATION_PROVIDER_PROTOCOL_VERSION,
      "messageType": "completed",
      "invocationId": "notification-discord-delivery",
      "outcome": {
        "kind": "delivery_success",
        "providerMessage": {
          "provider": "discord",
          "accountId": "123456789012345678",
          "conversationId": "345678901234567890",
          "messageId": "456789012345678901"
        }
      },
      "durationMs": 2.5
    }))
    .expect("Discord completion message");
    completed.validate().expect("Discord delivery identity");
  }

  #[test]
  fn provider_host_v3_accepts_only_bounded_digest_bound_presentations() {
    let message: RichApprovalNotificationDeliverMessage =
      serde_json::from_value(serde_json::json!({
        "protocol": NOTIFICATION_PROVIDER_PROTOCOL,
        "protocolVersion": RICH_APPROVAL_NOTIFICATION_PROVIDER_PROTOCOL_VERSION,
        "messageType": "deliver",
        "mode": "rich-approval",
        "invocationId": "inv_review_1",
        "runId": "run_review_1",
        "approvalId": "approveDeployment",
        "requestId": "aprreq_review_1",
        "deliveryId": "approveDeployment:notify:0:channel:0",
        "provider": "slack",
        "destination": "C0123456789",
        "reviewDigest": format!("sha256:{}", "a".repeat(64)),
        "presentation": {
          "title": "Deploy payments-api to production",
          "summary": "Create 2, modify 4, and destroy 1 infrastructure resource.",
          "risk": "high",
          "authority": {
            "required": 2,
            "received": 1,
            "requiredRoles": ["production-owner", "security-owner"]
          }
        },
        "decisionCapability": {
          "url": "http://127.0.0.1:7331/api/v2/approvals/run_review_1/aprreq_review_1",
          "token": "a".repeat(32),
          "expiresAt": "2026-10-21T12:30:00Z"
        }
      }))
      .expect("Provider Host v3 fixture");
    message
      .validate()
      .expect("bounded rich approval presentation");

    let mut stale = message.clone();
    stale.review_digest = "sha256:stale".to_string();
    assert!(stale.validate().is_err());
    let mut over_quorum = message;
    over_quorum.presentation.authority.received = 3;
    assert!(over_quorum.validate().is_err());
  }
}
