//! Marketplace capacity-reservation boundary.
//!
//! Phase 4G intentionally defines the durable record and authority contract
//! without supplying a local implementation.  The currently available durable
//! stores are local snapshots plus asynchronously replicated, local-first
//! GuardianDB state.  Neither offers a fleet-wide conditional mutation, so an
//! implementation using them could acknowledge two final-unit reservations
//! during a partition.  See `docs/marketplace-capacity-reservation-authority.md`.
//!
//! This module is deliberately not a process-local "reservation store":
//! correctness tests of an `RwLock` would prove only one process's behavior and
//! would be actively misleading for a multi-node authority.

use serde::{Deserialize, Serialize};

/// The capacity dimensions that DevHub currently receives authoritatively in a
/// Marketplace v2 workload intent.  `slots` is one because a workload consumes
/// one execution slot in addition to its resource requirements.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityVector {
    pub vcpu: u32,
    pub memory_mib: u64,
    pub storage_gib: u64,
    pub slots: u32,
}

/// Terminal states are never reactivated.  A future authority must enforce
/// these transitions in the same conditional write as capacity accounting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationState {
    Pending,
    Reserved,
    Committed,
    Released,
    Expired,
    Revoked,
    Failed,
}

impl ReservationState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Released | Self::Expired | Self::Revoked | Self::Failed
        )
    }
}

/// The persisted shape required of a future linearizable authority.  Provider
/// and node fields are internal-only and must never be projected into a
/// Marketplace receipt or lifecycle callback.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapacityReservation {
    pub reservation_id: String,
    pub allocation_id: String,
    pub marketplace_order_id: String,
    pub buyer_tenant_id: String,
    pub provider_id: String,
    pub node_id: String,
    pub workload_class: String,
    pub capacity: CapacityVector,
    pub placement_scope: String,
    pub preferred_network_id: Option<String>,
    pub commercial_authorization_revision: String,
    pub provider_eligibility_revision: String,
    pub release_id: String,
    pub state: ReservationState,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub committed_at_ms: Option<u64>,
    pub released_at_ms: Option<u64>,
    pub cancellation_reason: Option<String>,
    /// A durable, consensus-issued fencing term, not the current
    /// observer-local control-plane epoch.
    pub authority_term: u64,
    pub idempotency_key: String,
    pub request_digest: String,
    pub state_revision: u64,
}

/// The minimum operation a backing authority must expose.  `try_reserve`
/// encompasses eligibility reads, idempotency lookup, capacity check, capacity
/// consumption, and reservation creation in one linearizable operation.
///
/// No implementation exists until a quorum-confirmed CAS/transaction primitive
/// is deployed.  Keeping the trait explicit prevents a future caller from
/// silently substituting a local mutex or an eventually consistent replica.
pub trait ReservationAuthority: Send + Sync {
    type Error;

    fn try_reserve(&self, request: ReservationRequest) -> Result<CapacityReservation, Self::Error>;
    fn commit(
        &self,
        reservation_id: &str,
        authority_term: u64,
    ) -> Result<CapacityReservation, Self::Error>;
    fn release(
        &self,
        reservation_id: &str,
        authority_term: u64,
        reason: &str,
    ) -> Result<CapacityReservation, Self::Error>;
    fn expire_due(&self, now_ms: u64, authority_term: u64) -> Result<(), Self::Error>;
}

/// Inputs which must be digest-bound by the authority's durable idempotency
/// index.  This is internal-only; it deliberately contains node/provider
/// candidates so no public API needs to disclose placement topology.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReservationRequest {
    pub allocation_id: String,
    pub marketplace_order_id: String,
    pub buyer_tenant_id: String,
    pub provider_id: String,
    pub node_id: String,
    pub workload_class: String,
    pub capacity: CapacityVector,
    pub placement_scope: String,
    pub preferred_network_id: Option<String>,
    pub commercial_authorization_revision: String,
    pub provider_eligibility_revision: String,
    pub release_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub expires_at_ms: u64,
}

/// Stable fail-closed outcome for the v2 boundary.  This is intentionally
/// separate from authorization availability: configured credentials cannot
/// compensate for an absent reservation authority.
pub const AUTHORITY_UNAVAILABLE_REASON: &str = "capacity_reservation_authority_unavailable";
