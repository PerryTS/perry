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
//! **Young ids.** Ids parked since the previous trace began are kept by the
//! next one (a native frame may hold a fresh id unpublished across an
//! allocation), and ids parked while a trace runs are never decided by it. So
//! an id always survives at least one full trace after it was parked.
//!
//! **Pacing.** Ids carry no GC payload, so parking alone never reaches the
//! heap's own triggers. A full trace is requested when this mutator's parked
//! count reaches `trigger_at` (reset to twice the survivors, never below
//! [`MIN_TRIGGER`]) and whenever the shared band runs low.
//!
//! **Threads.** Parking is per mutator: each thread's trace decides only the
//! ids that thread parked. A thread that exits hands its parked ids to the
//! next mutator to begin a trace, which decides them against its own heap.
use super::handle::{Handle, HANDLES, REGISTRATIONS};
use perry_ffi::NativeRegistrationIdentity;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::Mutex;

type Mark = extern "C" fn(u64, *mut c_void);
extern "C" {
    fn perry_ffi_gc_request_handle_collection();
    fn perry_ffi_gc_register_pool_handle_trace(
        phase: extern "C" fn(u32) -> bool,
        observe: extern "C" fn(u64, Mark, *mut c_void) -> bool,
    );
}

/// Parked-id count below which parking alone never requests a full trace.
/// A requested full trace costs tens of millions of instructions even on a
/// small heap, so the floor is set high: 32k parked ids is an eighth of the
/// band, and a digest payload is a few hundred bytes. Collections the heap
/// runs on its own schedule decide parked ids too, at no extra cost.
pub(super) const MIN_TRIGGER: usize = 32 * 1024;
/// Shared-band ids still obtainable below which registration keeps asking
/// for traces (checked every [`BAND_CHECK_STRIDE`] registrations).
pub(super) const BAND_RESERVE: usize = 32 * 1024;
const BAND_CHECK_STRIDE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parked {
    /// Payload still registered; dropped when proven unreachable.
    Reclaimable(NativeRegistrationIdentity),
    /// Payload already removed; the id sits in `Retiring` until proven.
    Retired(NativeRegistrationIdentity),
}

struct Epoch {
    born: HashSet<Handle>,
    young: HashSet<Handle>,
    live: Option<HashSet<Handle>>,
    trigger_at: usize,
    hooked: bool,
    registrations: u32,
}

#[derive(Default)]
struct ParkedIds(HashMap<Handle, Parked>);

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
        born: HashSet::new(),
        young: HashSet::new(),
        live: None,
        trigger_at: MIN_TRIGGER,
        hooked: false,
        registrations: 0,
    });
    #[cfg(test)]
    pub(super) static FULL_TRACES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn park(id: Handle, parked: Parked) {
    let hooked = EPOCH
        .try_with(|epoch| std::mem::replace(&mut epoch.borrow_mut().hooked, true))
        .unwrap_or(true);
    if !hooked {
        unsafe { perry_ffi_gc_register_pool_handle_trace(phase, observe) };
    }
    let Ok(count) = PARKED.try_with(|map| {
        let mut map = map.borrow_mut();
        map.0.insert(id, parked);
        map.0.len()
    }) else {
        // Thread teardown: nothing can trace this thread any more.
        if let Ok(mut orphans) = ORPHANS.lock() {
            orphans.push((id, parked));
        }
        return;
    };
    let request = EPOCH
        .try_with(|epoch| {
            let mut epoch = epoch.borrow_mut();
            epoch.born.insert(id);
            if count < epoch.trigger_at {
                return false;
            }
            epoch.trigger_at = count + MIN_TRIGGER;
            true
        })
        .unwrap_or(false);
    if request {
        request_full_trace();
    }
}

/// A payload whose only owners are JS values: drop it and recycle its id once
/// a full trace proves nothing names it.
pub(super) fn park_reclaimable(identity: NativeRegistrationIdentity) {
    park(identity.numeric_id(), Parked::Reclaimable(identity));
}

/// A removed payload's id, still `Retiring` in the pool: reusable once a full
/// trace proves no JS value still holds the number.
pub(super) fn park_retired(identity: NativeRegistrationIdentity) {
    park(identity.numeric_id(), Parked::Retired(identity));
}

/// Stop treating `id` as reclaimable: native state now refers to it (a
/// crypto digest used as a stream has listeners and queued events keyed by
/// id), so it lives until explicitly dropped. Returns whether it was parked.
pub fn retain_strongly(id: Handle) -> bool {
    PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            match map.0.get(&id) {
                Some(Parked::Reclaimable(_)) => map.0.remove(&id).is_some(),
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
            let parked = PARKED
                .try_with(|map| {
                    let mut map = map.borrow_mut();
                    for (id, parked) in &orphans {
                        map.0.insert(*id, *parked);
                    }
                    !map.0.is_empty()
                })
                .unwrap_or(false);
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.young = std::mem::take(&mut epoch.born);
                // Adopted ids are decided no earlier than the next trace.
                epoch.born.extend(orphans.iter().map(|(id, _)| *id));
                epoch.live = parked.then(HashSet::new);
            });
            parked
        }
        1 => {
            finish_full_trace();
            true
        }
        _ => {
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.live = None;
                let young = std::mem::take(&mut epoch.young);
                epoch.born.extend(young);
            });
            true
        }
    }
}

fn finish_full_trace() {
    #[cfg(test)]
    FULL_TRACES.with(|count| count.set(count.get() + 1));
    let (live, young, born) = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        let live = epoch.live.take().unwrap_or_default();
        let young = std::mem::take(&mut epoch.young);
        (live, young, epoch.born.clone())
    });
    let (dead, survivors) = PARKED.with(|map| {
        let mut map = map.borrow_mut();
        let keep = |id: &Handle| live.contains(id) || young.contains(id) || born.contains(id);
        let dead: Vec<Parked> = map
            .0
            .iter()
            .filter(|(id, _)| !keep(id))
            .map(|(_, parked)| *parked)
            .collect();
        map.0.retain(|id, _| keep(id));
        (dead, map.0.len())
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
        match *parked {
            Parked::Reclaimable(identity) => {
                // A concurrent `drop_handle` may have retired it already; that
                // path parked the id itself and owns it now.
                if !REGISTRATIONS.begin_retirement_of(identity) {
                    continue;
                }
                let payload = HANDLES.remove(&identity.numeric_id());
                assert!(REGISTRATIONS.finish_retirement_reusable(identity));
                drop(payload);
            }
            Parked::Retired(identity) => {
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
