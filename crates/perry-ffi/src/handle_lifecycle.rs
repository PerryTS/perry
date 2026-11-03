//! Reclaiming ids of the shared handle band without letting a stale id alias
//! (#11453).
//!
//! Handles reach JavaScript as bare `HANDLE_TAG | id` / `POINTER_TAG | id`
//! words, which the collector does not own. Every registry that allocates from
//! the shared `[1, 0x40000)` pool ([`crate::shared_handle_id_pool`]) parks two
//! kinds of id here:
//!
//! * **Reclaimable** payloads: kinds whose only owners are JS values (a
//!   `crypto.createSign()` result, a `StringDecoder`). Nothing ever calls
//!   `drop_handle` on them; their registry hands the payload's release to a
//!   [`ParkedPayloadOwner`].
//! * **Retired** ids (`drop_handle` / `take_handle`): the payload is gone, but
//!   JS may still hold the number.
//!
//! A parked id is released (payload dropped, id handed back to the shared
//! pool's freelist) only when a *full heap trace* completes without any traced
//! word naming it. That is the stale-id guarantee: an id is reissued only after
//! the collector proved no JS value (heap slot, stack root, handle scope,
//! registered native root) still holds it, so a stale id can never resolve to a
//! new object. The runtime offers every traced `[1, 0x40000)` word to
//! `observe` through `perry_ffi_gc_register_pool_handle_trace`.
//!
//! **Young ids.** An id parked since the previous trace began is kept by the
//! next one (a native frame may hold a fresh id unpublished across an
//! allocation), and ids parked while a trace runs are never decided by it. So
//! an id always survives at least one full trace after it was parked. Each
//! parked record carries the trace epoch it was parked in and the epoch of the
//! last trace that reached it; no side sets.
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
use crate::handle::{Handle, HandleIdBuildHasher, REGISTRATIONS};
use crate::NativeRegistrationIdentity;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type Mark = extern "C" fn(u64, *mut c_void);

// perry-runtime's collector hooks, resolved at the final link. perry-ffi's
// own unit-test binary does not link the runtime (`runtime-link` off), so it
// gets inert definitions below instead; see `handle.rs` for the same split.
#[cfg(not(all(test, not(feature = "runtime-link"))))]
extern "C" {
    fn perry_ffi_gc_request_handle_collection();
    fn perry_ffi_gc_register_pool_handle_trace(
        phase: extern "C" fn(u32) -> bool,
        observe: extern "C" fn(u64, Mark, *mut c_void) -> bool,
    );
}

#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_request_handle_collection() {}

#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_register_pool_handle_trace(
    _phase: extern "C" fn(u32) -> bool,
    _observe: extern "C" fn(u64, Mark, *mut c_void) -> bool,
) {
}

/// Parked-id count below which parking alone never requests a full trace.
pub const MIN_TRIGGER: usize = 4096;
/// A requested trace waits until the mutator has run this many times as long
/// as the previous requested trace took (see the module doc).
pub const AMORTIZE: u32 = 32;
/// Parked ids at which a trace is requested regardless of the time budget, so
/// parked payloads (a digest is ~600 bytes) stay a bounded share of RSS.
pub const MAX_PARKED: usize = 64 * 1024;
/// Shared-band ids still obtainable below which registration keeps asking
/// for traces (checked every [`BAND_CHECK_STRIDE`] registrations).
pub const BAND_RESERVE: usize = 32 * 1024;
const BAND_CHECK_STRIDE: u32 = 256;

/// The payload map behind a reclaimable id. One `static` per map; the parked
/// record names it, so release needs no table of owners.
pub trait ParkedPayloadOwner: Sync {
    /// The id was proven unreachable: retire the registration of `identity`
    /// (unless a concurrent `drop_handle` already did), drop its payload, and
    /// hand the id back with `finish_retirement_reusable`. Runs outside every
    /// registry lock and allocates nothing on the GC heap.
    fn release(&self, identity: NativeRegistrationIdentity);
    /// The current mutator's property bag on the payload of `id`, if any.
    fn property_bag(&self, id: Handle) -> Option<u64>;
}

#[derive(Clone, Copy)]
enum Kind {
    /// Payload still registered; dropped through its owner when proven
    /// unreachable.
    Reclaimable(&'static dyn ParkedPayloadOwner),
    /// Payload already removed; the id sits in `Retiring` until proven.
    Retired,
}

#[derive(Clone, Copy)]
struct Parked {
    kind: Kind,
    identity: NativeRegistrationIdentity,
    /// Trace epoch the id was parked in.
    epoch: u64,
    /// Epoch of the last trace that observed a word naming the id.
    reached: u64,
}

struct Epoch {
    /// Incremented when a trace begins; parked ids record it.
    current: u64,
    /// A trace armed by `phase(0)` is running.
    tracing: bool,
    trigger_at: usize,
    hooked: bool,
    registrations: u32,
    /// When the in-flight requested trace began, until the mutator next parks.
    requested_started: Option<Instant>,
    requested_pending: bool,
    /// Wall time of the previous requested trace, including its sweep.
    last_cost: Duration,
    last_end: Option<Instant>,
    /// Full traces this mutator finished with parked ids (diagnostics, tests).
    full_traces: usize,
}

#[derive(Default)]
struct ParkedIds(HashMap<Handle, Parked, HandleIdBuildHasher>);

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
    static EPOCH: RefCell<Epoch> = const { RefCell::new(Epoch {
        current: 0,
        tracing: false,
        trigger_at: MIN_TRIGGER,
        hooked: false,
        registrations: 0,
        requested_started: None,
        requested_pending: false,
        last_cost: Duration::ZERO,
        last_end: None,
        full_traces: 0,
    }) };
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
                reached: 0,
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
        reached: 0,
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

/// A payload whose only owners are JS values: `owner` drops it, and its id is
/// recycled, once a full trace proves nothing names it.
pub fn park_reclaimable(
    identity: NativeRegistrationIdentity,
    owner: &'static dyn ParkedPayloadOwner,
) {
    park(identity.numeric_id(), Kind::Reclaimable(owner), identity);
}

/// A removed payload's id, still `Retiring` in the pool: reusable once a full
/// trace proves no JS value still holds the number.
pub fn park_retired(identity: NativeRegistrationIdentity) {
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
                Some(parked) if matches!(parked.kind, Kind::Reclaimable(_)) => {
                    map.0.remove(&id).is_some()
                }
                _ => false,
            }
        })
        .unwrap_or(false)
}

/// Called on every registration in the shared band: keeps traces coming while
/// the band is running low, whoever is consuming it.
pub fn note_registration() {
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
pub fn request_full_trace() {
    unsafe { perry_ffi_gc_request_handle_collection() };
}

/// Ids this mutator has parked (tests and diagnostics).
pub fn parked_handle_count() -> usize {
    PARKED.try_with(|map| map.borrow().0.len()).unwrap_or(0)
}

/// Strong native owners and newborn parked records protect unpublished bags;
/// old parked records are traced only when a reachable JS value names them.
pub fn payload_bag_is_rooted(id: Handle) -> bool {
    let parked = PARKED.with(|parked| parked.borrow().0.get(&id).copied());
    parked.is_none_or(|parked| {
        let current = EPOCH.with(|epoch| epoch.borrow().current);
        parked.epoch >= current.saturating_sub(1)
    })
}

/// Whether this mutator has `id` parked (tests and diagnostics).
#[doc(hidden)]
pub fn is_parked(id: Handle) -> bool {
    PARKED
        .try_with(|map| map.borrow().0.contains_key(&id))
        .unwrap_or(false)
}

/// Full traces this mutator finished while it had parked ids (tests).
#[doc(hidden)]
pub fn finished_full_traces() -> usize {
    EPOCH
        .try_with(|epoch| epoch.borrow().full_traces)
        .unwrap_or(0)
}

/// Drive the provider's trace phase directly (tests).
#[doc(hidden)]
pub fn trace_phase_for_tests(phase_id: u32) -> bool {
    phase(phase_id)
}

/// Offer a traced word to the provider directly (tests).
#[doc(hidden)]
pub fn observe_for_tests(bits: u64) -> bool {
    extern "C" fn no_mark(_: u64, _: *mut c_void) {}
    observe(bits, no_mark, std::ptr::null_mut())
}

extern "C" fn phase(phase: u32) -> bool {
    match phase {
        0 => {
            let orphans = ORPHANS
                .lock()
                .map(|mut orphans| std::mem::take(&mut *orphans))
                .unwrap_or_default();
            let current = EPOCH
                .try_with(|epoch| {
                    let mut epoch = epoch.borrow_mut();
                    epoch.current += 1;
                    if std::mem::take(&mut epoch.requested_pending) {
                        epoch.requested_started = Some(Instant::now());
                    }
                    epoch.current
                })
                .unwrap_or(0);
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
                epoch.borrow_mut().tracing = parked;
            });
            parked
        }
        1 => {
            finish_full_trace();
            true
        }
        _ => {
            // Aborted: nothing was proven dead. Parked epochs are untouched,
            // so the next trace still keeps anything this one would have. A
            // `reached` stamp of the aborted epoch only makes the next trace
            // keep that id once more.
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.tracing = false;
                epoch.current = epoch.current.saturating_sub(1);
            });
            true
        }
    }
}

fn finish_full_trace() {
    let current = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.tracing = false;
        epoch.full_traces += 1;
        epoch.current
    });
    // Parked in the previous epoch or later: born since the previous trace
    // began, or during this one.
    let young_from = current.saturating_sub(1);
    let mut dead = Vec::new();
    let survivors = PARKED.with(|map| {
        let mut map = map.borrow_mut();
        map.0.retain(|_, parked| {
            let keep = parked.epoch >= young_from || parked.reached == current;
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
        match parked.kind {
            Kind::Reclaimable(owner) => owner.release(parked.identity),
            Kind::Retired => {
                REGISTRATIONS.finish_retirement_reusable(parked.identity);
            }
        }
    }
}

extern "C" fn observe(bits: u64, mark: Mark, ctx: *mut c_void) -> bool {
    let id = if matches!(bits >> 48, 0xfff9 | 0x7ffd) {
        (bits & 0x0000_FFFF_FFFF_FFFF) as Handle
    } else if bits >> 48 == 0 {
        // Typed native-pointer slots can hold the unboxed id.
        bits as Handle
    } else {
        return false;
    };
    let Some((parked, first)) = PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            let parked = map.0.get_mut(&id)?;
            let first = EPOCH
                .try_with(|epoch| {
                    let epoch = epoch.borrow();
                    epoch.tracing
                        && std::mem::replace(&mut parked.reached, epoch.current) != epoch.current
                })
                .unwrap_or(false);
            Some((*parked, first))
        })
        .ok()
        .flatten()
    else {
        return false;
    };
    if first {
        // Marked outside every borrow: marking may offer more words.
        if let Kind::Reclaimable(owner) = parked.kind {
            if let Some(bits) = owner.property_bag(id).filter(|&bits| bits != 0) {
                mark(bits, ctx);
            }
        }
    }
    true
}
