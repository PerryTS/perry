//! Reclaiming common-registry ids without letting a stale id alias (#11453).
//!
//! Common handles reach JavaScript as bare `POINTER_TAG | id` numbers, which
//! the collector does not own. Two kinds of id are *parked* here:
//!
//! * **Reclaimable** payloads (`register_reclaimable_handle`): kinds whose only
//!   owners are JS values — a `crypto.createHash()` result, a `StringDecoder`.
//!   Nothing ever called `drop_handle` on them, so every one leaked its payload
//!   and its id: a server hashing once per request exhausted the shared 262k-id
//!   band after ~200k requests and panicked.
//! * **Retired** ids (`drop_handle` / `take_handle`): the payload is gone, but
//!   JS may still hold the number. These used to be tombstoned forever.
//!
//! A parked id is released — payload dropped, id handed back to the shared
//! pool's freelist — only when a *full heap trace* completes without any traced
//! word naming it. That is the stale-id guarantee: an id is reissued only after
//! the collector proved no JS value (heap slot, stack root, handle scope,
//! registered native root) still holds it, so a stale id can never resolve to a
//! new object. The runtime offers every traced `[1, 0x40000)` word to
//! [`observe`] through `perry_ffi_gc_register_pool_handle_trace`.
//!
//! **Young ids.** An id parked since the previous trace began is kept by the
//! next one (a native frame may hold a fresh id unpublished across an
//! allocation), and ids parked while a trace runs are never decided by it. So
//! an id always survives at least one full trace after it was parked. Each
//! parked id carries the trace epoch it was parked in; no side sets.
//!
//! **Pacing.** The collector's own full traces decide parked ids for free. A
//! trace is *requested* only when both hold: the parked count reached
//! `trigger_at` (twice the survivors, never below [`MIN_TRIGGER`]), and the
//! mutator has run at least [`AMORTIZE`] times as long as the last requested
//! trace took since it ended. The second condition caps what reclamation can
//! add to a handle-churning program at about 1/[`AMORTIZE`] of its run time,
//! however small its heap. Two things override that budget so memory stays
//! bounded: [`MAX_PARKED`] parked ids, and a shared band running low.
//!
//! **Threads.** Parking is per mutator: each thread's trace decides only the
//! ids that thread parked. A thread that exits hands its parked ids to the
//! next mutator to begin a trace, which decides them against its own heap.
use super::handle::{Handle, HANDLES, REGISTRATIONS};
use perry_ffi::NativeRegistrationIdentity;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

type Mark = extern "C" fn(u64, *mut c_void);
extern "C" {
    fn perry_ffi_gc_request_handle_collection();
    fn perry_ffi_gc_register_pool_handle_trace(
        phase: extern "C" fn(u32) -> bool,
        observe: extern "C" fn(u64, Mark, *mut c_void) -> bool,
    );
}

/// Parked-id count below which parking alone never requests a full trace.
pub(super) const MIN_TRIGGER: usize = 4096;
/// A requested trace waits until the mutator has run this many times as long
/// as the previous requested trace took (see the module doc).
pub(super) const AMORTIZE: u32 = 32;
/// Parked ids at which a trace is requested regardless of the time budget, so
/// parked payloads (a digest is ~600 bytes) stay a bounded share of RSS.
pub(super) const MAX_PARKED: usize = 64 * 1024;
/// Shared-band ids still obtainable below which registration keeps asking
/// for traces (checked every [`BAND_CHECK_STRIDE`] registrations).
pub(super) const BAND_RESERVE: usize = 32 * 1024;
const BAND_CHECK_STRIDE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Payload still registered; dropped when proven unreachable.
    Reclaimable,
    /// Payload already removed; the id sits in `Retiring` until proven.
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parked {
    kind: Kind,
    identity: NativeRegistrationIdentity,
    /// Trace epoch the id was parked in.
    epoch: u64,
}

/// Ids are small distinct integers: a multiplicative hash is enough, and
/// SipHash on every park showed up in the churn profile.
#[derive(Default)]
struct IdHasher(u64);
impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_i64(&mut self, n: i64) {
        self.0 = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}
type IdBuild = BuildHasherDefault<IdHasher>;

struct Epoch {
    /// Incremented when a trace begins; parked ids record it.
    current: u64,
    live: Option<HashSet<Handle, IdBuild>>,
    trigger_at: usize,
    hooked: bool,
    registrations: u32,
    /// When the in-flight requested trace began, until the mutator next parks.
    requested_started: Option<Instant>,
    requested_pending: bool,
    /// Wall time of the previous requested trace, including its sweep.
    last_cost: Duration,
    last_end: Option<Instant>,
}

#[derive(Default)]
struct ParkedIds(HashMap<Handle, Parked, IdBuild>);

// A dying mutator's heap is gone, but a value may have crossed to another
// heap, so its parked ids are adopted and decided by a live mutator instead of
// being released unproven.
impl Drop for ParkedIds {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        if let Ok(mut orphans) = ORPHANS.lock() {
            orphans.extend(self.0.drain());
        }
    }
}

static ORPHANS: Mutex<Vec<(Handle, Parked)>> = Mutex::new(Vec::new());

thread_local! {
    static PARKED: RefCell<ParkedIds> = RefCell::new(ParkedIds::default());
    static EPOCH: RefCell<Epoch> = RefCell::new(Epoch {
        current: 0,
        live: None,
        trigger_at: MIN_TRIGGER,
        hooked: false,
        registrations: 0,
        requested_started: None,
        requested_pending: false,
        last_cost: Duration::ZERO,
        last_end: None,
    });
    #[cfg(test)]
    pub(super) static FULL_TRACES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn park(id: Handle, kind: Kind, identity: NativeRegistrationIdentity) {
    let Ok((hooked, epoch)) = EPOCH.try_with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        let hooked = std::mem::replace(&mut epoch.hooked, true);
        // First mutator activity after a requested trace: charge its whole
        // cost, sweep included, to the pacing budget.
        if let Some(started) = epoch.requested_started.take() {
            let now = Instant::now();
            epoch.last_cost = now - started;
            epoch.last_end = Some(now);
        }
        (hooked, epoch.current)
    }) else {
        // Thread teardown: nothing can trace this thread any more.
        orphan(
            id,
            Parked {
                kind,
                identity,
                epoch: 0,
            },
        );
        return;
    };
    if !hooked {
        unsafe { perry_ffi_gc_register_pool_handle_trace(phase, observe) };
    }
    let parked = Parked {
        kind,
        identity,
        epoch,
    };
    let Ok(count) = PARKED.try_with(|map| {
        let mut map = map.borrow_mut();
        map.0.insert(id, parked);
        map.0.len()
    }) else {
        orphan(id, parked);
        return;
    };
    let request = EPOCH
        .try_with(|epoch| {
            let mut epoch = epoch.borrow_mut();
            if count < epoch.trigger_at || epoch.requested_pending {
                return false;
            }
            if count < MAX_PARKED {
                if let Some(end) = epoch.last_end {
                    if end.elapsed() < epoch.last_cost * AMORTIZE {
                        return false;
                    }
                }
            }
            epoch.requested_pending = true;
            true
        })
        .unwrap_or(false);
    if request {
        request_full_trace();
    }
}

fn orphan(id: Handle, parked: Parked) {
    if let Ok(mut orphans) = ORPHANS.lock() {
        orphans.push((id, parked));
    }
}

/// A payload whose only owners are JS values: drop it and recycle its id once
/// a full trace proves nothing names it.
pub(super) fn park_reclaimable(identity: NativeRegistrationIdentity) {
    park(identity.numeric_id(), Kind::Reclaimable, identity);
}

/// A removed payload's id, still `Retiring` in the pool: reusable once a full
/// trace proves no JS value still holds the number.
pub(super) fn park_retired(identity: NativeRegistrationIdentity) {
    park(identity.numeric_id(), Kind::Retired, identity);
}

/// Stop treating `id` as reclaimable: native state now refers to it (a
/// crypto digest used as a stream has listeners and queued events keyed by
/// id), so it lives until explicitly dropped. Returns whether it was parked.
pub fn retain_strongly(id: Handle) -> bool {
    PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            match map.0.get(&id) {
                Some(parked) if parked.kind == Kind::Reclaimable => map.0.remove(&id).is_some(),
                _ => false,
            }
        })
        .unwrap_or(false)
}

/// Called on every common registration: keeps traces coming while the shared
/// band is running low, whoever is consuming it.
pub(super) fn note_registration() {
    let check = EPOCH
        .try_with(|epoch| {
            let mut epoch = epoch.borrow_mut();
            epoch.registrations = epoch.registrations.wrapping_add(1);
            epoch.registrations % BAND_CHECK_STRIDE == 0
        })
        .unwrap_or(false);
    if check && REGISTRATIONS.available_ids() < BAND_RESERVE {
        request_full_trace();
    }
}

/// Ask for a full trace at the next safe poll. Never collects synchronously.
pub(super) fn request_full_trace() {
    unsafe { perry_ffi_gc_request_handle_collection() };
}

/// Ids this mutator has parked (tests and diagnostics).
pub fn parked_handle_count() -> usize {
    PARKED.try_with(|map| map.borrow().0.len()).unwrap_or(0)
}

extern "C" fn phase(phase: u32) -> bool {
    match phase {
        0 => {
            let orphans = ORPHANS
                .lock()
                .map(|mut orphans| std::mem::take(&mut *orphans))
                .unwrap_or_default();
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.current += 1;
                if std::mem::take(&mut epoch.requested_pending) {
                    epoch.requested_started = Some(Instant::now());
                }
            });
            let current = EPOCH.try_with(|epoch| epoch.borrow().current).unwrap_or(0);
            let parked = PARKED
                .try_with(|map| {
                    let mut map = map.borrow_mut();
                    for (id, mut parked) in orphans {
                        // Adopted ids are decided no earlier than the next trace.
                        parked.epoch = current;
                        map.0.insert(id, parked);
                    }
                    !map.0.is_empty()
                })
                .unwrap_or(false);
            let _ = EPOCH.try_with(|epoch| {
                epoch.borrow_mut().live = parked.then(HashSet::default);
            });
            parked
        }
        1 => {
            finish_full_trace();
            true
        }
        _ => {
            // Aborted: nothing was proven dead. Parked epochs are untouched,
            // so the next trace still keeps anything this one would have.
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.live = None;
                epoch.current = epoch.current.saturating_sub(1);
            });
            true
        }
    }
}

fn finish_full_trace() {
    #[cfg(test)]
    FULL_TRACES.with(|count| count.set(count.get() + 1));
    let (live, current) = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        (epoch.live.take().unwrap_or_default(), epoch.current)
    });
    // Parked in the previous epoch or later: born since the previous trace
    // began, or during this one.
    let young_from = current.saturating_sub(1);
    let mut dead = Vec::new();
    let survivors = PARKED.with(|map| {
        let mut map = map.borrow_mut();
        map.0.retain(|id, parked| {
            let keep = parked.epoch >= young_from || live.contains(id);
            if !keep {
                dead.push(*parked);
            }
            keep
        });
        map.0.len()
    });
    EPOCH.with(|epoch| {
        epoch.borrow_mut().trigger_at = MIN_TRIGGER.max(survivors.saturating_mul(2));
    });
    release(&dead);
}

/// Drop proven-dead payloads and hand their ids back to the shared pool.
/// Allocates nothing on the GC heap; payload destructors run outside every
/// registry lock.
fn release(dead: &[Parked]) {
    for parked in dead {
        let identity = parked.identity;
        match parked.kind {
            Kind::Reclaimable => {
                // A concurrent `drop_handle` may have retired it already; that
                // path parked the id itself and owns it now.
                if !REGISTRATIONS.begin_retirement_of(identity) {
                    continue;
                }
                let payload = HANDLES.remove(&identity.numeric_id());
                assert!(REGISTRATIONS.finish_retirement_reusable(identity));
                drop(payload);
            }
            Kind::Retired => {
                REGISTRATIONS.finish_retirement_reusable(identity);
            }
        }
    }
}

extern "C" fn observe(bits: u64, _mark: Mark, _ctx: *mut c_void) -> bool {
    let id = if bits >> 48 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as Handle
    } else if bits >> 48 == 0 {
        // Typed native-pointer slots can hold the unboxed id.
        bits as Handle
    } else {
        return false;
    };
    let parked = PARKED
        .try_with(|map| map.borrow().0.contains_key(&id))
        .unwrap_or(false);
    if !parked {
        return false;
    }
    let _ = EPOCH.try_with(|epoch| {
        if let Some(live) = epoch.borrow_mut().live.as_mut() {
            live.insert(id);
        }
    });
    true
}

#[cfg(test)]
#[path = "handle_lifecycle_tests.rs"]
mod tests;
