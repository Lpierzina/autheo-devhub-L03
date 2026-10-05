//! Marketplace commercial-authorization verification foundation.
//!
//! This module deliberately has no HTTP route and does not enable placement.
//! Marketplace has not yet supplied the request/response schemas or reverse
//! S2S authentication contract for its verification endpoints.  The exposed
//! client is therefore typed but unavailable, rather than sending guessed
//! bodies or unauthenticated requests.

use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

pub const COMMERCIAL_AUTHORIZATION_CHECK_PATH: &str =
    "/v1/marketplace/internal/commercial-authorizations/check";
pub const PROVIDER_ELIGIBILITY_CHECK_PATH: &str =
    "/v1/marketplace/internal/provider-eligibility/check";
pub const S2S_TIMEOUT_MS: u64 = 3_000;
pub const S2S_MAX_RETRIES: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedIssuerKey {
    pub issuer: String,
    pub key_id: String,
    public_key: VerifyingKey,
    pub active_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

#[derive(Clone, Debug)]
pub struct MarketplaceIssuerTrust {
    approved_issuer: String,
    keys: BTreeMap<String, TrustedIssuerKey>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAuthorization {
    pub issuer: String,
    pub key_id: String,
    pub alg: String,
    /// Base64url without padding.
    pub signature: String,
    /// Marketplace must sign this field alone.  This explicit separation avoids
    /// guessing which envelope fields Marketplace excludes from a signature.
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct VerifiedAuthorization {
    pub issuer: String,
    pub key_id: String,
    pub canonical_payload: Vec<u8>,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerificationError {
    TrustNotConfigured,
    MalformedTrust,
    UnknownIssuer,
    UnknownKey,
    KeyInactive,
    KeyExpired,
    KeyRevoked,
    UnsupportedAlgorithm,
    InvalidSignatureEncoding,
    InvalidSignature,
    DuplicateJsonKey,
    InvalidJson,
}

impl MarketplaceIssuerTrust {
    /// Loads operator-owned trust only.  Each key is one `|`-separated entry:
    ///
    /// `issuer|key-id|base64url-ed25519-public-key|active-rfc3339|expires-rfc3339-or-empty|active-or-revoked`
    ///
    /// Multiple active keys support rotation overlap.  This intentionally
    /// never reads `HIVE_MARKETPLACE_HMAC_KEYS`: those keys authenticate the
    /// independent Marketplace-to-DevHub request channel and are not signing
    /// keys for authorization evidence.
    pub fn from_env() -> Result<Self, VerificationError> {
        let approved_issuer = std::env::var("HIVE_MARKETPLACE_AUTH_ISSUER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(VerificationError::TrustNotConfigured)?;
        let entries = std::env::var("HIVE_MARKETPLACE_ED25519_KEYS")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(VerificationError::TrustNotConfigured)?;
        let mut keys = BTreeMap::new();
        for entry in entries.split(',') {
            let fields: Vec<_> = entry.split('|').collect();
            if fields.len() != 6
                || [fields[0], fields[1], fields[2], fields[3], fields[5]]
                    .iter()
                    .any(|field| field.is_empty())
                || fields[0] != approved_issuer
                || keys.contains_key(fields[1])
            {
                return Err(VerificationError::MalformedTrust);
            }
            let public_key = URL_SAFE_NO_PAD
                .decode(fields[2])
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .and_then(|bytes: [u8; 32]| VerifyingKey::from_bytes(&bytes).ok())
                .ok_or(VerificationError::MalformedTrust)?;
            let active_at = DateTime::parse_from_rfc3339(fields[3])
                .map_err(|_| VerificationError::MalformedTrust)?
                .with_timezone(&Utc);
            let expires_at = if fields[4].is_empty() {
                None
            } else {
                Some(
                    DateTime::parse_from_rfc3339(fields[4])
                        .map_err(|_| VerificationError::MalformedTrust)?
                        .with_timezone(&Utc),
                )
            };
            if expires_at.is_some_and(|expires| expires <= active_at) {
                return Err(VerificationError::MalformedTrust);
            }
            let revoked = match fields[5] {
                "active" => false,
                "revoked" => true,
                _ => return Err(VerificationError::MalformedTrust),
            };
            keys.insert(
                fields[1].to_owned(),
                TrustedIssuerKey {
                    issuer: fields[0].to_owned(),
                    key_id: fields[1].to_owned(),
                    public_key,
                    active_at,
                    expires_at,
                    revoked,
                },
            );
        }
        (!keys.is_empty())
            .then_some(Self {
                approved_issuer,
                keys,
            })
            .ok_or(VerificationError::TrustNotConfigured)
    }

    #[cfg(test)]
    pub(crate) fn from_keys(
        approved_issuer: impl Into<String>,
        keys: Vec<TrustedIssuerKey>,
    ) -> Self {
        Self {
            approved_issuer: approved_issuer.into(),
            keys: keys
                .into_iter()
                .map(|key| (key.key_id.clone(), key))
                .collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_key(
        issuer: impl Into<String>,
        key_id: impl Into<String>,
        public_key: VerifyingKey,
        active_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
        revoked: bool,
    ) -> TrustedIssuerKey {
        TrustedIssuerKey {
            issuer: issuer.into(),
            key_id: key_id.into(),
            public_key,
            active_at,
            expires_at,
            revoked,
        }
    }

    pub fn verify(
        &self,
        authorization: &SignedAuthorization,
        now: DateTime<Utc>,
    ) -> Result<VerifiedAuthorization, VerificationError> {
        if authorization.issuer != self.approved_issuer {
            return Err(VerificationError::UnknownIssuer);
        }
        if authorization.alg != "Ed25519" {
            return Err(VerificationError::UnsupportedAlgorithm);
        }
        let key = self
            .keys
            .get(&authorization.key_id)
            .filter(|key| key.issuer == authorization.issuer)
            .ok_or(VerificationError::UnknownKey)?;
        if key.revoked {
            return Err(VerificationError::KeyRevoked);
        }
        if now < key.active_at {
            return Err(VerificationError::KeyInactive);
        }
        if key.expires_at.is_some_and(|expires| now >= expires) {
            return Err(VerificationError::KeyExpired);
        }
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(&authorization.signature)
            .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
        // Serialize the Value then parse it again with duplicate detection.
        // Signed production input should arrive as raw JSON from Marketplace;
        // this defensive reparse also prevents callers from constructing an
        // ambiguous Value through a different parser.
        let payload_bytes = serde_json::to_vec(&authorization.payload)
            .map_err(|_| VerificationError::InvalidJson)?;
        let payload = parse_json_without_duplicate_keys(&payload_bytes)?;
        let canonical_payload = canonical_json(&payload);
        key.public_key
            .verify(&canonical_payload, &signature)
            .map_err(|_| VerificationError::InvalidSignature)?;
        Ok(VerifiedAuthorization {
            issuer: authorization.issuer.clone(),
            key_id: authorization.key_id.clone(),
            canonical_payload,
            payload,
        })
    }
}

/// Parses raw Marketplace JSON before envelope deserialization.  Callers that
/// receive the wire body must use this first; serde_json's ordinary `Value`
/// parser accepts last-key-wins duplicates.
pub fn parse_signed_authorization(bytes: &[u8]) -> Result<SignedAuthorization, VerificationError> {
    let value = parse_json_without_duplicate_keys(bytes)?;
    serde_json::from_value(value).map_err(|_| VerificationError::InvalidJson)
}

/// Marketplace's currently described algorithm: recursively sorted object
/// keys, preserved array order, and compact serde JSON.  It is intentionally
/// not labelled RFC 8785: Marketplace has not supplied authoritative vectors
/// for string escaping, Unicode, number representation, or signed-field
/// exclusion.  Production interoperability remains disabled until it does.
pub fn canonical_json(value: &Value) -> Vec<u8> {
    let mut out = String::new();
    append_canonical_json(value, &mut out);
    out.into_bytes()
}

fn append_canonical_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => out.push_str(&value.to_string()),
        Value::String(value) => {
            out.push_str(&serde_json::to_string(value).expect("strings serialize"))
        }
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                append_canonical_json(value, out);
            }
            out.push(']');
        }
        Value::Object(values) => {
            out.push('{');
            for (index, (key, value)) in values
                .iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .enumerate()
            {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("keys serialize"));
                out.push(':');
                append_canonical_json(value, out);
            }
            out.push('}');
        }
    }
}

fn parse_json_without_duplicate_keys(bytes: &[u8]) -> Result<Value, VerificationError> {
    struct NoDuplicates;
    impl<'de> DeserializeSeed<'de> for NoDuplicates {
        type Value = Value;
        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Value, D::Error> {
            deserializer.deserialize_any(NoDuplicateVisitor)
        }
    }
    struct NoDuplicateVisitor;
    impl<'de> Visitor<'de> for NoDuplicateVisitor {
        type Value = Value;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a JSON value without duplicate object keys")
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
            Ok(Value::Bool(value))
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
            Ok(Value::Number(value.into()))
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
            Ok(Value::Number(value.into()))
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| E::custom("non-finite JSON number"))
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
            Ok(Value::String(value.to_owned()))
        }
        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
            Ok(Value::String(value))
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element_seed(NoDuplicates)? {
                values.push(value);
            }
            Ok(Value::Array(values))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut keys = BTreeSet::new();
            let mut values = serde_json::Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key.clone()) {
                    return Err(A::Error::custom("duplicate object key"));
                }
                values.insert(key, map.next_value_seed(NoDuplicates)?);
            }
            Ok(Value::Object(values))
        }
    }
    serde_json::Deserializer::from_slice(bytes)
        .deserialize_any(NoDuplicateVisitor)
        .map_err(|error| {
            if error.to_string().contains("duplicate object key") {
                VerificationError::DuplicateJsonKey
            } else {
                VerificationError::InvalidJson
            }
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommercialAuthorizationCheck {
    pub buyer_tenant_id: String,
    pub workload_order_id: String,
    pub workload_class: String,
    pub policy_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderEligibilityCheck {
    pub buyer_tenant_id: String,
    pub workload_order_id: String,
    pub provider_id: String,
    pub workload_class: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferredNetworkCheck {
    pub buyer_tenant_id: String,
    pub workload_order_id: String,
    pub network_id: String,
    pub membership_revision: String,
    pub provider_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketplaceCheckError {
    AuthorizationUnavailable,
    PreferredNetworkUnsupported,
}

#[derive(Clone, Debug)]
pub struct MarketplaceS2sClient {
    base_url: reqwest::Url,
}

impl MarketplaceS2sClient {
    /// Parses only a private HTTPS base URL.  Reverse-direction authentication
    /// is deliberately not inferred from inbound HMAC or callback keys.
    pub fn from_env() -> Result<Self, MarketplaceCheckError> {
        let base_url = std::env::var("HIVE_MARKETPLACE_SERVICE_URL")
            .ok()
            .and_then(|value| reqwest::Url::parse(&value).ok())
            .filter(|url| url.scheme() == "https" && url.host_str().is_some())
            .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
        Ok(Self { base_url })
    }

    pub fn commercial_authorization_endpoint(&self) -> Result<reqwest::Url, MarketplaceCheckError> {
        self.base_url
            .join(COMMERCIAL_AUTHORIZATION_CHECK_PATH)
            .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)
    }

    pub fn provider_eligibility_endpoint(&self) -> Result<reqwest::Url, MarketplaceCheckError> {
        self.base_url
            .join(PROVIDER_ELIGIBILITY_CHECK_PATH)
            .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)
    }

    pub async fn check_commercial_authorization(
        &self,
        _request: &CommercialAuthorizationCheck,
    ) -> Result<(), MarketplaceCheckError> {
        Err(MarketplaceCheckError::AuthorizationUnavailable)
    }

    pub async fn check_provider_eligibility(
        &self,
        _request: &ProviderEligibilityCheck,
    ) -> Result<(), MarketplaceCheckError> {
        Err(MarketplaceCheckError::AuthorizationUnavailable)
    }

    pub async fn check_preferred_network(
        &self,
        _request: &PreferredNetworkCheck,
    ) -> Result<(), MarketplaceCheckError> {
        Err(MarketplaceCheckError::PreferredNetworkUnsupported)
    }
}
