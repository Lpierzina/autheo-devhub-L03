//! Marketplace commercial-authorization verification boundary.
//!
//! This module deliberately has no HTTP route and does not enable placement.
//! The imported Phase 4E contracts define both the protected envelope and the
//! reverse S2S HMAC checks. The typed client is fail-closed; until the
//! authoritative schemas are mutually interoperable, no scheduling path calls
//! it or treats a response as an authorization to place work.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use ring::rand::SecureRandom;
use serde::de::{DeserializeSeed, Deserializer as _, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const COMMERCIAL_AUTHORIZATION_CHECK_PATH: &str =
    "/v1/marketplace/internal/commercial-authorizations/check";
pub const PROVIDER_ELIGIBILITY_CHECK_PATH: &str =
    "/v1/marketplace/internal/provider-eligibility/check";
pub const S2S_TIMEOUT_MS: u64 = 3_000;
pub const S2S_MAX_RETRIES: u8 = 2;
const COMMERCIAL_AUTHORIZATION_SCHEMA_ID: &str =
    "https://marketplace-devhub.invalid/schemas/commercial-authorization-check-v1";
const PROVIDER_ELIGIBILITY_SCHEMA_ID: &str =
    "https://marketplace-devhub.invalid/schemas/provider-eligibility-check-v1";
const ED25519_ENVELOPE_SCHEMA_ID: &str =
    "https://marketplace-devhub.invalid/schemas/ed25519-envelope-v1";

struct MarketplaceSchemaValidators {
    commercial_authorization: jsonschema::Validator,
    provider_eligibility: jsonschema::Validator,
    #[allow(dead_code)]
    ed25519_envelope: jsonschema::Validator,
}

fn imported_schema(
    source: &str,
    expected_id: &str,
) -> Result<(String, Value), MarketplaceCheckError> {
    let schema: Value = serde_json::from_str(source)
        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
    let declared_id = schema
        .get("$id")
        .and_then(Value::as_str)
        .filter(|id| *id == expected_id)
        .map(ToOwned::to_owned)
        .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
    Ok((declared_id, schema))
}

fn marketplace_schema_validators(
) -> Result<&'static MarketplaceSchemaValidators, MarketplaceCheckError> {
    static VALIDATORS: OnceLock<Result<MarketplaceSchemaValidators, MarketplaceCheckError>> =
        OnceLock::new();
    match VALIDATORS
        .get_or_init(|| {
            let (commercial_id, commercial) = imported_schema(
                include_str!(
                    "../../../docs/integrations/devhub-marketplace-phase-4e/schemas/commercial-authorization-check.schema.json"
                ),
                COMMERCIAL_AUTHORIZATION_SCHEMA_ID,
            )?;
            let (provider_id, provider) = imported_schema(
                include_str!(
                    "../../../docs/integrations/devhub-marketplace-phase-4e/schemas/provider-eligibility-check.schema.json"
                ),
                PROVIDER_ELIGIBILITY_SCHEMA_ID,
            )?;
            let (envelope_id, envelope) = imported_schema(
                include_str!(
                    "../../../docs/integrations/devhub-marketplace-phase-4e/schemas/ed25519-envelope.schema.json"
                ),
                ED25519_ENVELOPE_SCHEMA_ID,
            )?;
            let registry = jsonschema::Registry::new()
                .add(commercial_id, &commercial)
                .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?
                .add(provider_id, &provider)
                .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?
                .add(envelope_id, &envelope)
                .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?
                .prepare()
                .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
            let options = jsonschema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .with_registry(&registry);
            Ok(MarketplaceSchemaValidators {
                commercial_authorization: options
                    .build(&commercial)
                    .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?,
                provider_eligibility: options
                    .build(&provider)
                    .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?,
                ed25519_envelope: options
                    .build(&envelope)
                    .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?,
            })
        })
        .as_ref()
    {
        Ok(validators) => Ok(validators),
        Err(_) => Err(MarketplaceCheckError::AuthorizationUnavailable),
    }
}

pub(crate) fn validate_commercial_request_value(
    request: &Value,
) -> Result<(), MarketplaceCheckError> {
    marketplace_schema_validators()?
        .commercial_authorization
        .is_valid(request)
        .then_some(())
        .ok_or(MarketplaceCheckError::InvalidRequest)
}

pub(crate) fn validate_provider_request_value(
    request: &Value,
) -> Result<(), MarketplaceCheckError> {
    marketplace_schema_validators()?
        .provider_eligibility
        .is_valid(request)
        .then_some(())
        .ok_or(MarketplaceCheckError::InvalidRequest)
}

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
    /// Authorization claims. The protected signing input also includes the
    /// envelope's `alg`, `issuer`, and `key_id` fields.
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct VerifiedAuthorization {
    pub issuer: String,
    pub key_id: String,
    /// Canonical UTF-8 bytes verified by Ed25519. This is the protected
    /// envelope without `signature`, never `payload` alone.
    pub canonical_signed_object: Vec<u8>,
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
    UnknownEnvelopeField,
    InvalidJson,
    CanonicalizationUnsupported,
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
        let canonical_signed_object = canonical_signed_authorization(
            &authorization.alg,
            &authorization.issuer,
            &authorization.key_id,
            &payload,
        )?;
        key.public_key
            .verify(&canonical_signed_object, &signature)
            .map_err(|_| VerificationError::InvalidSignature)?;
        Ok(VerifiedAuthorization {
            issuer: authorization.issuer.clone(),
            key_id: authorization.key_id.clone(),
            canonical_signed_object,
            payload,
        })
    }
}

/// Parses raw Marketplace JSON before envelope deserialization.  Callers that
/// receive the wire body must use this first; serde_json's ordinary `Value`
/// parser accepts last-key-wins duplicates.
pub fn parse_signed_authorization(bytes: &[u8]) -> Result<SignedAuthorization, VerificationError> {
    let value = parse_json_without_duplicate_keys(bytes)?;
    serde_json::from_value(value).map_err(|error| {
        if error.to_string().contains("unknown field") {
            VerificationError::UnknownEnvelopeField
        } else {
            VerificationError::InvalidJson
        }
    })
}

/// Constructs the exact protected object required by the supplied Marketplace
/// envelope contract. `signature` is intentionally absent; every other
/// envelope field is covered by the Ed25519 signature.
pub fn canonical_signed_authorization(
    alg: &str,
    issuer: &str,
    key_id: &str,
    payload: &Value,
) -> Result<Vec<u8>, VerificationError> {
    let mut signed = serde_json::Map::new();
    signed.insert("alg".into(), Value::String(alg.into()));
    signed.insert("issuer".into(), Value::String(issuer.into()));
    signed.insert("key_id".into(), Value::String(key_id.into()));
    signed.insert("payload".into(), payload.clone());
    canonical_json(&Value::Object(signed))
}

/// Marketplace's TypeScript-compatible recursive key-sort plus compact JSON
/// serialization. This is deliberately not RFC 8785: object keys compare by
/// UTF-16 code units, as JavaScript string sorting does, while arrays retain
/// their input order. Finite JSON numbers use an ECMAScript formatter.
///
/// The raw-envelope parser rejects duplicate keys before a `Value` exists.
/// Integer tokens outside JavaScript's safe range are refused so DevHub never
/// silently changes an exact authorization claim while canonicalizing it.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>, VerificationError> {
    let mut out = String::new();
    append_canonical_json(value, &mut out)?;
    Ok(out.into_bytes())
}

fn append_canonical_json(value: &Value, out: &mut String) -> Result<(), VerificationError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => out.push_str(&canonical_number(value)?),
        Value::String(value) => {
            out.push_str(&serde_json::to_string(value).expect("strings serialize"))
        }
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                append_canonical_json(value, out)?;
            }
            out.push(']');
        }
        Value::Object(values) => {
            out.push('{');
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| utf16_cmp(left, right));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("keys serialize"));
                out.push(':');
                append_canonical_json(value, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn canonical_number(value: &serde_json::Number) -> Result<String, VerificationError> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    if value
        .as_i64()
        .is_some_and(|integer| integer.unsigned_abs() > MAX_SAFE_INTEGER)
        || value
            .as_u64()
            .is_some_and(|integer| integer > MAX_SAFE_INTEGER)
    {
        return Err(VerificationError::CanonicalizationUnsupported);
    }
    let number = value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or(VerificationError::CanonicalizationUnsupported)?;
    let mut buffer = ryu_js::Buffer::new();
    Ok(buffer.format(number).to_owned())
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommercialPolicy {
    pub policy_id: String,
    pub policy_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommercialAuthorizationCheck {
    pub authorization_id: String,
    pub buyer_tenant_id: String,
    pub workload_order_id: String,
    pub workload_class: String,
    pub policy: CommercialPolicy,
    pub expected_authorization_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEligibilityCheck {
    pub authorization_id: String,
    pub buyer_tenant_id: String,
    pub workload_order_id: String,
    pub workload_class: String,
    pub policy: CommercialPolicy,
    pub expected_authorization_revision: u64,
    pub candidate_provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_network_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_provider_membership_revision: Option<u64>,
}

impl ProviderEligibilityCheck {
    /// Standard placement contains none of the Preferred Network binding
    /// fields. A Preferred Network placement carries every binding together.
    pub fn has_valid_network_binding(&self) -> bool {
        match (
            self.network_id.as_deref(),
            self.expected_network_revision,
            self.expected_provider_membership_revision,
        ) {
            (None, None, None) => true,
            (Some(network_id), Some(network_revision), Some(membership_revision)) => {
                !network_id.is_empty() && network_revision > 0 && membership_revision > 0
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketplaceCheckError {
    AuthorizationUnavailable,
    AuthorizationDenied,
    ProviderIneligible,
    InvalidRequest,
}

#[derive(Clone, Debug)]
pub struct MarketplaceS2sClient {
    base_url: reqwest::Url,
    key_id: String,
    secret: Vec<u8>,
    scopes: BTreeSet<String>,
}

impl MarketplaceS2sClient {
    /// Parses only a private HTTPS base URL.  Reverse-direction authentication
    /// is deliberately not inferred from inbound HMAC or callback keys.
    pub fn from_env() -> Result<Self, MarketplaceCheckError> {
        let base_url = std::env::var("HIVE_MARKETPLACE_SERVICE_URL")
            .ok()
            .and_then(|value| reqwest::Url::parse(&value).ok())
            .filter(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.query().is_none()
                    && url.fragment().is_none()
            })
            .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
        let key_id = std::env::var("DEVHUB_COMMERCIAL_AUTHORIZATION_KEY_ID")
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
        let secret = std::env::var("DEVHUB_COMMERCIAL_AUTHORIZATION_SIGNING_SECRET")
            .ok()
            .filter(|value| value.len() >= 32)
            .map(String::into_bytes)
            .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
        let scopes = std::env::var("DEVHUB_COMMERCIAL_AUTHORIZATION_SCOPES")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|scope| !scope.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .filter(|scopes| !scopes.is_empty())
            .ok_or(MarketplaceCheckError::AuthorizationUnavailable)?;
        Ok(Self {
            base_url,
            key_id,
            secret,
            scopes,
        })
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
        request: &CommercialAuthorizationCheck,
    ) -> Result<(), MarketplaceCheckError> {
        validate_commercial_request(request)?;
        let response: CommercialAuthorizationResponse = self
            .post_check(
                "authorization:check",
                COMMERCIAL_AUTHORIZATION_CHECK_PATH,
                request,
            )
            .await?;
        match response.result {
            CommercialAuthorizationDecision::Active => Ok(()),
            CommercialAuthorizationDecision::Expired
            | CommercialAuthorizationDecision::Revoked
            | CommercialAuthorizationDecision::Stale
            | CommercialAuthorizationDecision::BindingMismatch
            | CommercialAuthorizationDecision::PolicyMismatch => {
                Err(MarketplaceCheckError::AuthorizationDenied)
            }
            CommercialAuthorizationDecision::Unavailable => {
                Err(MarketplaceCheckError::AuthorizationUnavailable)
            }
        }
    }

    pub async fn check_provider_eligibility(
        &self,
        request: &ProviderEligibilityCheck,
    ) -> Result<(), MarketplaceCheckError> {
        validate_provider_request(request)?;
        let response: ProviderEligibilityResponse = self
            .post_check("provider:check", PROVIDER_ELIGIBILITY_CHECK_PATH, request)
            .await?;
        match response.result {
            ProviderEligibilityDecision::Eligible => Ok(()),
            ProviderEligibilityDecision::Denied
            | ProviderEligibilityDecision::Stale
            | ProviderEligibilityDecision::BindingMismatch
            | ProviderEligibilityDecision::PolicyMismatch => {
                Err(MarketplaceCheckError::ProviderIneligible)
            }
            ProviderEligibilityDecision::Unavailable => {
                Err(MarketplaceCheckError::AuthorizationUnavailable)
            }
        }
    }

    async fn post_check<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        required_scope: &str,
        path: &str,
        request: &T,
    ) -> Result<R, MarketplaceCheckError> {
        if !self.scopes.contains(required_scope) {
            return Err(MarketplaceCheckError::AuthorizationUnavailable);
        }
        let endpoint = self
            .base_url
            .join(path)
            .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
        if endpoint.path() != path || endpoint.query().is_some() {
            return Err(MarketplaceCheckError::AuthorizationUnavailable);
        }
        let body = serde_json::to_vec(request)
            .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
        let body_sha256 = hex::encode(Sha256::digest(&body));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(S2S_TIMEOUT_MS))
            .build()
            .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
        let mut last_error = MarketplaceCheckError::AuthorizationUnavailable;
        for _ in 0..S2S_MAX_RETRIES {
            // A retry is a fresh authenticated request: the Marketplace
            // contract consumes nonces atomically, so it must never reuse one.
            let timestamp = Utc::now().timestamp_millis().to_string();
            let nonce = random_nonce()?;
            let signature = reverse_hmac_signature(
                &self.secret,
                "POST",
                path,
                &timestamp,
                &nonce,
                &self.key_id,
                &body_sha256,
            )?;
            match client
                .post(endpoint.clone())
                .header("content-type", "application/json")
                .header("x-devhub-timestamp", &timestamp)
                .header("x-devhub-nonce", &nonce)
                .header("x-devhub-key-id", &self.key_id)
                .header("x-devhub-content-sha256", &body_sha256)
                .header("x-devhub-signature", &signature)
                .body(body.clone())
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    return response
                        .json::<R>()
                        .await
                        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable);
                }
                Ok(_) | Err(_) => last_error = MarketplaceCheckError::AuthorizationUnavailable,
            }
        }
        Err(last_error)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommercialAuthorizationResponse {
    pub result: CommercialAuthorizationDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommercialAuthorizationDecision {
    Active,
    Expired,
    Revoked,
    Stale,
    BindingMismatch,
    PolicyMismatch,
    Unavailable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderEligibilityResponse {
    pub result: ProviderEligibilityDecision,
    #[serde(default)]
    pub checks: Option<BTreeMap<String, bool>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderEligibilityDecision {
    Eligible,
    Denied,
    Stale,
    BindingMismatch,
    PolicyMismatch,
    Unavailable,
}

pub(crate) fn validate_commercial_request(
    request: &CommercialAuthorizationCheck,
) -> Result<(), MarketplaceCheckError> {
    let value = serde_json::to_value(request)
        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
    validate_commercial_request_value(&value)?;
    (valid_contract_identifier(&request.authorization_id)
        && valid_contract_identifier(&request.workload_order_id)
        && valid_contract_identifier(&request.buyer_tenant_id)
        && valid_contract_identifier(&request.workload_class)
        && !request.policy.policy_id.is_empty()
        && request.policy.policy_version > 0
        && request.expected_authorization_revision > 0)
        .then_some(())
        .ok_or(MarketplaceCheckError::InvalidRequest)
}

pub(crate) fn validate_provider_request(
    request: &ProviderEligibilityCheck,
) -> Result<(), MarketplaceCheckError> {
    let value = serde_json::to_value(request)
        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
    validate_provider_request_value(&value)?;
    validate_commercial_request(&CommercialAuthorizationCheck {
        authorization_id: request.authorization_id.clone(),
        buyer_tenant_id: request.buyer_tenant_id.clone(),
        workload_order_id: request.workload_order_id.clone(),
        workload_class: request.workload_class.clone(),
        policy: request.policy.clone(),
        expected_authorization_revision: request.expected_authorization_revision,
    })?;
    (valid_contract_identifier(&request.candidate_provider_id)
        && request.has_valid_network_binding())
    .then_some(())
    .ok_or(MarketplaceCheckError::InvalidRequest)
}

fn valid_contract_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}

fn random_nonce() -> Result<String, MarketplaceCheckError> {
    let mut bytes = [0_u8; 24];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn reverse_hmac_signature(
    secret: &[u8],
    method: &str,
    path: &str,
    timestamp: &str,
    nonce: &str,
    key_id: &str,
    body_sha256: &str,
) -> Result<String, MarketplaceCheckError> {
    if method != "POST"
        || !path.starts_with('/')
        || path.contains('?')
        || timestamp.len() != 13
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || nonce.len() < 16
        || nonce.len() > 256
        || URL_SAFE_NO_PAD.decode(nonce).is_err()
        || body_sha256.len() != 64
        || !body_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(MarketplaceCheckError::InvalidRequest);
    }
    let canonical = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{key_id}\n{body_sha256}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|_| MarketplaceCheckError::AuthorizationUnavailable)?;
    mac.update(canonical.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}
