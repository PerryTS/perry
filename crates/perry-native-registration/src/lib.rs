//! Native registration identities and lease-ordered identifier reuse.
//!
//! One mutex orders slot allocation, publication, retirement, lease acquisition,
//! and quarantine draining. Payload insertion/removal happens outside that
//! mutex: Pending and Retiring slots cannot be acquired or reused. No callback,
//! payload destructor, JavaScript value, or collector slot lives in this module.

#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

/// Identity of one authoritative native registry instance, not an id band.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeRegistryDomain(u64);

static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(1);
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

fn issue_serial(counter: &AtomicU64) -> Result<u64, NativeRegistrationError> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .map_err(|_| NativeRegistrationError::SerialExhausted)
}

impl NativeRegistryDomain {
    /// Allocate a domain without ever wrapping or reissuing an earlier token.
    pub fn new() -> Result<Self, NativeRegistrationError> {
        issue_serial(&NEXT_DOMAIN).map(Self)
    }
}

/// Immutable identity. Provider calls continue to receive `numeric_id()` only.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeRegistrationIdentity {
    domain: NativeRegistryDomain,
    serial: u64,
    numeric_id: i64,
}

impl NativeRegistrationIdentity {
    /// Return the authoritative registry domain recorded for this registration.
    pub fn domain(self) -> NativeRegistryDomain {
        self.domain
    }
    /// Return the process-wide issuance serial distinguishing this registration.
    pub fn serial(self) -> u64 {
        self.serial
    }
    /// Return the numeric id passed to providers; reuse creates a new identity.
    pub fn numeric_id(self) -> i64 {
        self.numeric_id
    }
}

/// Reason a native domain or registration could not be allocated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRegistrationError {
    /// An explicit id lies outside the allocator's half-open numeric range.
    InvalidId,
    /// The explicit id names a slot that is not reusable with zero leases.
    Occupied,
    /// No reusable id remains and the fresh-id counter reached the range end.
    IdExhausted,
    /// The domain or registration counter cannot advance without wrapping.
    SerialExhausted,
}

/// Reserved ids identify payloads stored outside the allocator's payload map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRegistrationKind {
    /// The id names a value in the allocator adapter's payload map.
    Payload,
    /// The id is reserved for a payload stored by a separate owner.
    Reserved,
}

/// Which independently counted reference keeps a registration from reuse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLeaseKind {
    /// A reference retained for the lifetime of a wrapper.
    Wrapper,
    /// A reference retained for the duration of an operation.
    Operation,
}

/// Both ordinary and deadline retirement require a subsequent explicit drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeQuarantine {
    /// Eligible at a later drain once both lease counts reach zero.
    NextDrain,
    /// Eligible at a later drain only at or after this instant and with zero leases.
    Until(Instant),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Pending,
    Live,
    Retiring,
    Quarantined(NativeQuarantine),
    Reusable,
    Abandoned,
}

struct Slot {
    identity: NativeRegistrationIdentity,
    kind: NativeRegistrationKind,
    phase: Phase,
    wrappers: usize,
    operations: usize,
}

impl Slot {
    fn unleased(&self) -> bool {
        self.wrappers == 0 && self.operations == 0
    }
}

struct State {
    next_id: i64,
    slots: HashMap<i64, Slot>,
    // Queue rows are non-owning numeric keys; dropping a row drops no lease.
    ordinary: Vec<i64>,
    deadlines: Vec<i64>,
    free: Vec<i64>,
}

struct Inner {
    domain: NativeRegistryDomain,
    start: i64,
    end: i64,
    queue_cap: usize,
    state: Mutex<State>,
}

/// Shared native state for an id allocator and its authoritative registries.
/// Clones name the same allocator; `new` creates an independent domain.
#[derive(Clone)]
pub struct NativeRegistrationRegistry(Arc<Inner>);

/// One counted reference to a particular registration, with exactly one release.
/// This retains native identity only, not a removed payload or a payload borrow.
/// It is deliberately not `Clone`; additional acquisition must consult live state.
pub struct NativeRegistrationLease {
    registry: NativeRegistrationRegistry,
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
}

impl NativeRegistrationLease {
    /// Return the exact registration whose reference count this lease retains.
    pub fn identity(&self) -> NativeRegistrationIdentity {
        self.identity
    }
    /// Return which kind of reference is released when this lease is dropped.
    pub fn kind(&self) -> NativeLeaseKind {
        self.kind
    }
}

impl Drop for NativeRegistrationLease {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        let slot = state
            .slots
            .get_mut(&self.identity.numeric_id)
            .expect("leased registration must retain its slot");
        assert_eq!(
            slot.identity, self.identity,
            "leased registration must not be replaced"
        );
        let count = match self.kind {
            NativeLeaseKind::Wrapper => &mut slot.wrappers,
            NativeLeaseKind::Operation => &mut slot.operations,
        };
        *count = count
            .checked_sub(1)
            .expect("native lease released exactly once");
    }
}

impl NativeRegistrationRegistry {
    /// Create an independent allocator for the half-open id range `start..end`.
    /// Each quarantine queue and the freelist has a separate `queue_cap` bound;
    /// overflow leaves the affected slot unavailable for reuse.
    ///
    /// # Panics
    /// Panics if the range is empty or includes nonpositive ids, or if domain
    /// issuance is exhausted.
    pub fn new(start: i64, end: i64, queue_cap: usize) -> Self {
        assert!(
            start > 0 && start < end,
            "native id range must be positive and nonempty"
        );
        Self(Arc::new(Inner {
            domain: NativeRegistryDomain::new().expect("native registry domains exhausted"),
            start,
            end,
            queue_cap,
            state: Mutex::new(State {
                next_id: start,
                slots: HashMap::new(),
                ordinary: Vec::new(),
                deadlines: Vec::new(),
                free: Vec::new(),
            }),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Return the default domain assigned to registrations from this allocator.
    pub fn domain(&self) -> NativeRegistryDomain {
        self.0.domain
    }

    /// Claim a pending slot before inserting a payload outside the state lock.
    pub fn begin_registration(
        &self,
        kind: NativeRegistrationKind,
    ) -> Result<NativeRegistrationIdentity, NativeRegistrationError> {
        self.begin_registration_in_domain(self.domain(), kind)
    }

    /// Allocate from this id pool for a separately owned payload registry.
    pub fn begin_registration_in_domain(
        &self,
        domain: NativeRegistryDomain,
        kind: NativeRegistrationKind,
    ) -> Result<NativeRegistrationIdentity, NativeRegistrationError> {
        let mut state = self.lock();
        let serial = issue_serial(&NEXT_SERIAL)?;
        let id = if let Some(&id) = state.free.last() {
            let slot = &state.slots[&id];
            assert!(slot.phase == Phase::Reusable && slot.unleased());
            state.free.pop();
            id
        } else {
            while state.next_id < self.0.end && state.slots.contains_key(&state.next_id) {
                state.next_id += 1;
            }
            if state.next_id >= self.0.end {
                return Err(NativeRegistrationError::IdExhausted);
            }
            let id = state.next_id;
            state.next_id += 1;
            id
        };
        Ok(Self::insert_pending(&mut state, domain, kind, id, serial))
    }

    /// Explicit ids cannot overwrite live, pending, retiring, leased, quarantined,
    /// or abandoned slots. An eligible explicit reuse consumes its freelist row.
    pub fn begin_registration_with_id(
        &self,
        id: i64,
        kind: NativeRegistrationKind,
    ) -> Result<NativeRegistrationIdentity, NativeRegistrationError> {
        if !(self.0.start..self.0.end).contains(&id) {
            return Err(NativeRegistrationError::InvalidId);
        }
        let mut state = self.lock();
        if let Some(slot) = state.slots.get(&id) {
            if slot.phase != Phase::Reusable || !slot.unleased() {
                return Err(NativeRegistrationError::Occupied);
            }
        }
        let serial = issue_serial(&NEXT_SERIAL)?;
        state.free.retain(|entry| *entry != id);
        Ok(Self::insert_pending(
            &mut state,
            self.domain(),
            kind,
            id,
            serial,
        ))
    }

    fn insert_pending(
        state: &mut State,
        domain: NativeRegistryDomain,
        kind: NativeRegistrationKind,
        id: i64,
        serial: u64,
    ) -> NativeRegistrationIdentity {
        let identity = NativeRegistrationIdentity {
            domain,
            serial,
            numeric_id: id,
        };
        state.slots.insert(
            id,
            Slot {
                identity,
                kind,
                phase: Phase::Pending,
                wrappers: 0,
                operations: 0,
            },
        );
        identity
    }

    /// Publish native availability only after payload insertion is complete.
    pub fn publish(&self, identity: NativeRegistrationIdentity) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.slots.get_mut(&identity.numeric_id) else {
            return false;
        };
        if slot.identity != identity || slot.phase != Phase::Pending {
            return false;
        }
        slot.phase = Phase::Live;
        true
    }

    /// Lookup is not a lease. Acquisition below rechecks this exact identity.
    pub fn identity(&self, id: i64) -> Option<NativeRegistrationIdentity> {
        self.lock()
            .slots
            .get(&id)
            .filter(|slot| slot.phase == Phase::Live)
            .map(|slot| slot.identity)
    }

    /// Publication/operation acquisition is ordered against retirement and reuse.
    pub fn acquire(
        &self,
        identity: NativeRegistrationIdentity,
        kind: NativeLeaseKind,
    ) -> Option<NativeRegistrationLease> {
        let mut state = self.lock();
        let slot = state.slots.get_mut(&identity.numeric_id)?;
        if slot.identity != identity || slot.phase != Phase::Live {
            return None;
        }
        let count = match kind {
            NativeLeaseKind::Wrapper => &mut slot.wrappers,
            NativeLeaseKind::Operation => &mut slot.operations,
        };
        *count = count.checked_add(1)?;
        Some(NativeRegistrationLease {
            registry: self.clone(),
            identity,
            kind,
        })
    }

    /// Close native availability before removing a payload outside this lock.
    /// The kind check prevents reserved-id free from retiring a payload slot.
    pub fn begin_retirement(
        &self,
        id: i64,
        kind: NativeRegistrationKind,
    ) -> Option<NativeRegistrationIdentity> {
        let mut state = self.lock();
        let slot = state.slots.get_mut(&id)?;
        if slot.phase != Phase::Live || slot.kind != kind {
            return None;
        }
        slot.phase = Phase::Retiring;
        Some(slot.identity)
    }

    /// Exact-identity counterpart for callers already carrying a registration.
    pub fn begin_retirement_of(&self, identity: NativeRegistrationIdentity) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.slots.get_mut(&identity.numeric_id) else {
            return false;
        };
        if slot.identity != identity || slot.phase != Phase::Live {
            return false;
        }
        slot.phase = Phase::Retiring;
        true
    }

    /// Complete removal before making a slot eligible for a later drain.
    /// Overflow leaves an unavailable tombstone, including for explicit ids.
    pub fn finish_retirement(
        &self,
        identity: NativeRegistrationIdentity,
        quarantine: NativeQuarantine,
    ) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.slots.get(&identity.numeric_id) else {
            return false;
        };
        if slot.identity != identity || slot.phase != Phase::Retiring {
            return false;
        }
        let queue = match quarantine {
            NativeQuarantine::NextDrain => &mut state.ordinary,
            NativeQuarantine::Until(_) => &mut state.deadlines,
        };
        let phase = if queue.len() < self.0.queue_cap {
            queue.push(identity.numeric_id);
            Phase::Quarantined(quarantine)
        } else {
            Phase::Abandoned
        };
        state.slots.get_mut(&identity.numeric_id).unwrap().phase = phase;
        true
    }

    /// One drain boundary. `now` is explicit so deadline tests need no sleeping.
    /// Lease inspection and transition to the freelist use the acquisition mutex.
    pub fn drain(&self, now: Instant) -> usize {
        let mut state = self.lock();
        let ordinary = std::mem::take(&mut state.ordinary);
        let deadlines = std::mem::take(&mut state.deadlines);
        let mut promoted = 0;
        for id in ordinary.into_iter().chain(deadlines) {
            let slot = state
                .slots
                .get_mut(&id)
                .expect("quarantine slot must exist");
            let Phase::Quarantined(quarantine) = slot.phase else {
                panic!("quarantine row must match slot");
            };
            let elapsed = match quarantine {
                NativeQuarantine::NextDrain => true,
                NativeQuarantine::Until(deadline) => now >= deadline,
            };
            if !elapsed || !slot.unleased() {
                match quarantine {
                    NativeQuarantine::NextDrain => state.ordinary.push(id),
                    NativeQuarantine::Until(_) => state.deadlines.push(id),
                }
                continue;
            }
            if state.free.len() < self.0.queue_cap {
                state.slots.get_mut(&id).unwrap().phase = Phase::Reusable;
                state.free.push(id);
                promoted += 1;
            } else {
                state.slots.get_mut(&id).unwrap().phase = Phase::Abandoned;
            }
        }
        promoted
    }

    /// Read the fresh-id counter for cross-crate adapter fixtures.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn next_fresh_id_for_tests(&self) -> i64 {
        self.lock().next_id
    }
}

#[cfg(test)]
mod tests;
