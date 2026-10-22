use std::{collections::BTreeMap, fs, path::PathBuf};

use serde_json::Value;
use woml_engine::{
  parse_infrastructure_policy, InfrastructurePolicyError, InfrastructurePolicySignatureStatusV1,
  InfrastructurePolicyTrustV1,
};

fn fixture(name: &str) -> Vec<u8> {
  fs::read(
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("../../woml/tests/fixtures/infrastructure-policies")
      .join(name),
  )
  .unwrap()
}

fn identity() -> Value {
  serde_json::from_slice(&fixture("identity.v1.json")).unwrap()
}

#[test]
fn unsigned_bundle_has_the_frozen_cross_language_identity() {
  let prepared = parse_infrastructure_policy(
    &fixture("production.unsigned.v1.json"),
    &InfrastructurePolicyTrustV1::default(),
  )
  .unwrap();
  assert_eq!(prepared.digest, identity()["digest"]);
  assert_eq!(
    prepared.signature_status,
    InfrastructurePolicySignatureStatusV1::Unsigned
  );
}

#[test]
fn signed_bundle_verifies_with_operator_owned_ed25519_trust() {
  let identity = identity();
  let key_id = identity["signingKeyId"].as_str().unwrap().to_string();
  let public_key = identity["publicKey"].as_str().unwrap().to_string();
  let prepared = parse_infrastructure_policy(
    &fixture("production.signed.v1.json"),
    &InfrastructurePolicyTrustV1 {
      keys: BTreeMap::from([(key_id.clone(), public_key)]),
      require_signature: true,
      ..InfrastructurePolicyTrustV1::default()
    },
  )
  .unwrap();
  assert_eq!(prepared.digest, identity["digest"]);
  assert_eq!(
    prepared.signature_status,
    InfrastructurePolicySignatureStatusV1::Verified
  );
  assert_eq!(prepared.signing_key_id.as_deref(), Some(key_id.as_str()));
}

#[test]
fn signatures_fail_closed_when_missing_unknown_revoked_or_altered() {
  let identity = identity();
  let key_id = identity["signingKeyId"].as_str().unwrap().to_string();
  let public_key = identity["publicKey"].as_str().unwrap().to_string();
  let required = InfrastructurePolicyTrustV1 {
    require_signature: true,
    ..InfrastructurePolicyTrustV1::default()
  };
  assert!(matches!(
    parse_infrastructure_policy(&fixture("production.unsigned.v1.json"), &required),
    Err(InfrastructurePolicyError::SignatureInvalid(_))
  ));
  assert!(matches!(
    parse_infrastructure_policy(
      &fixture("production.signed.v1.json"),
      &InfrastructurePolicyTrustV1::default()
    ),
    Err(InfrastructurePolicyError::SignatureInvalid(_))
  ));
  let revoked = InfrastructurePolicyTrustV1 {
    keys: BTreeMap::from([(key_id.clone(), public_key.clone())]),
    revoked_key_ids: [key_id.clone()].into_iter().collect(),
    require_signature: true,
  };
  assert!(matches!(
    parse_infrastructure_policy(&fixture("production.signed.v1.json"), &revoked),
    Err(InfrastructurePolicyError::SignatureInvalid(_))
  ));
  let altered = String::from_utf8(fixture("production.signed.v1.json"))
    .unwrap()
    .replace("2026-10-22.1", "2026-10-22.2");
  let trusted = InfrastructurePolicyTrustV1 {
    keys: BTreeMap::from([(key_id, public_key)]),
    require_signature: true,
    ..InfrastructurePolicyTrustV1::default()
  };
  assert!(matches!(
    parse_infrastructure_policy(altered.as_bytes(), &trusted),
    Err(InfrastructurePolicyError::SignatureInvalid(_))
  ));
}

#[test]
fn strict_contract_rejects_duplicates_unknown_fields_and_ambiguous_rules() {
  let source = String::from_utf8(fixture("production.unsigned.v1.json")).unwrap();
  let duplicate = source.replace(
    "\"contractVersion\": 1,",
    "\"contractVersion\": 1, \"contractVersion\": 1,",
  );
  assert!(parse_infrastructure_policy(
    duplicate.as_bytes(),
    &InfrastructurePolicyTrustV1::default()
  )
  .is_err());

  let mut unknown: Value = serde_json::from_str(&source).unwrap();
  unknown["surprise"] = Value::Bool(true);
  assert!(parse_infrastructure_policy(
    serde_json::to_vec(&unknown).unwrap().as_slice(),
    &InfrastructurePolicyTrustV1::default()
  )
  .is_err());

  let mut empty_match: Value = serde_json::from_str(&source).unwrap();
  empty_match["rules"][0]["match"] = serde_json::json!({});
  assert!(parse_infrastructure_policy(
    serde_json::to_vec(&empty_match).unwrap().as_slice(),
    &InfrastructurePolicyTrustV1::default()
  )
  .is_err());

  let mut empty_set: Value = serde_json::from_str(&source).unwrap();
  empty_set["rules"][0]["match"] = serde_json::json!({
    "capabilities": [],
    "effects": ["unsafe-write"]
  });
  assert!(parse_infrastructure_policy(
    serde_json::to_vec(&empty_set).unwrap().as_slice(),
    &InfrastructurePolicyTrustV1::default()
  )
  .is_err());

  let mut future: Value = serde_json::from_str(&source).unwrap();
  future["contractVersion"] = serde_json::json!(2);
  assert!(parse_infrastructure_policy(
    serde_json::to_vec(&future).unwrap().as_slice(),
    &InfrastructurePolicyTrustV1::default()
  )
  .is_err());

  assert!(matches!(
    parse_infrastructure_policy(
      &vec![b' '; woml_engine::INFRASTRUCTURE_POLICY_MAX_BYTES + 1],
      &InfrastructurePolicyTrustV1::default()
    ),
    Err(InfrastructurePolicyError::TooLarge)
  ));
}

#[test]
fn ip0_inventory_keeps_effect_paths_and_trust_invariants_reviewable() {
  let coverage: Value = serde_json::from_slice(&fixture("coverage.v1.json")).unwrap();
  let threat_model: Value = serde_json::from_slice(&fixture("threat-model.v1.json")).unwrap();
  assert!(coverage["paths"].as_array().unwrap().len() >= 16);
  assert!(threat_model["invariants"].as_array().unwrap().len() >= 10);
  assert!(threat_model["untrustedSources"]
    .as_array()
    .unwrap()
    .iter()
    .any(|source| source == "ai-output"));
}
