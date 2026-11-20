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
//! trace is *requested* only when both hold: the parked ids the last trace did
//! not prove live reached `trigger_at` (twice the ids it proved live, never
//! below [`MIN_TRIGGER`]; young ids it merely kept are not live), and the
//! mutator has run at least [`AMORTIZE`] times as long as the last requested
//! trace took since it ended. The second condition caps what reclamation can
//! add to a handle-churning program at about 1/[`AMORTIZE`] of its run time,
//! however small its heap. Two things override that budget so memory stays
//! bounded: [`MAX_PARKED`] unproven ids, and the band reserve.
//!
//! **Band reserve.** A release needs two traces to begin after the park (the
//! first keeps the id as young). So when fewer than `max(BAND_RESERVE, 2 x
//! span + 2 x stride)` shared ids are obtainable, `span` being the ids this
//! mutator registered across its last trace (request to finish), registration
//! requests a trace without the time budget. The band then runs out only if
//! the program itself names nearly all of it.
//!
//! **Threads.** Parking is per mutator: each thread's trace decides only the
//! ids that thread parked. A thread that exits hands its parked ids to the
//! next mutator to begin a trace, which decides them against its own heap.
use crate::handle::{Handle, HandleIdBuildHasher, REGISTRATIONS};
use crate::NativeRegistrationIdentity;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    fn perry_ffi_gc_register_pool_handle_liveness(is_live: extern "C" fn(u64) -> bool);
    fn perry_ffi_gc_thread_is_mutator() -> bool;
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

#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_register_pool_handle_liveness(
    _is_live: extern "C" fn(u64) -> bool,
) {
}

// The unit tests drive the provider on their own thread as its mutator.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_thread_is_mutator() -> bool {
    true
}

/// Unproven parked ids below which parking alone never requests a full trace.
pub const MIN_TRIGGER: usize = 4096;
/// A requested trace waits until the mutator has run this many times as long
/// as the previous requested trace took (see the module doc).
pub const AMORTIZE: u32 = 32;
/// Unproven parked ids at which a trace is requested regardless of the time budget, so
/// parked payloads (a digest is ~600 bytes) stay a bounded share of RSS.
pub const MAX_PARKED: usize = 64 * 1024;
/// Floor of the band reserve: shared-band ids still obtainable below which
/// registration requests traces (checked every [`BAND_CHECK_STRIDE`]
/// registrations; the reserve grows with the observed per-trace consumption).
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
    /// A retirement with a grace deadline (`drop_handle_until`) is not
    /// released by a trace that began before it.
    not_before: Option<Instant>,
}

struct Epoch {
    /// Incremented when a trace begins; parked ids record it.
    current: u64,
    /// A trace armed by `phase(0)` is running.
    tracing: bool,
    /// When the armed trace began: the one clock its deadline checks read,
    /// so its weak-key verdicts and its releases agree.
    trace_began: Option<Instant>,
    /// Unproven parked ids (`parked - live`) at which parking requests a trace.
    trigger_at: usize,
    /// Parked ids the last finished trace could decide and found named: what
    /// the program holds. Young ids it only kept are not counted.
    live: usize,
    hooked: bool,
    registrations: u32,
    /// Registration count when the next trace was requested (or, when the
    /// collector began it unasked, when it began): the start of `span`.
    span_from: Option<u32>,
    /// Ids this mutator registered from the request to the finish of its last
    /// trace: the consumption the band reserve must cover, twice.
    span: usize,
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
        trace_began: None,
        trigger_at: MIN_TRIGGER,
        live: 0,
        hooked: false,
        registrations: 0,
        span_from: None,
        span: 0,
        requested_started: None,
        requested_pending: false,
        last_cost: Duration::ZERO,
        last_end: None,
        full_traces: 0,
    }) };
}

fn park(id: Handle, kind: Kind, identity: NativeRegistrationIdentity, not_before: Option<Instant>) {
    // Only a mutator's full traces see the values that can name the id. A
    // retirement elsewhere (a worker pool thread) has no heap to decide it.
    if !unsafe { perry_ffi_gc_thread_is_mutator() } {
        note_off_mutator_retirement(id);
        orphan(
            id,
            Parked {
                kind,
                identity,
                epoch: 0,
                reached: 0,
                not_before,
            },
        );
        return;
    }
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
                not_before,
            },
        );
        return;
    };
    if !hooked {
        unsafe {
            perry_ffi_gc_register_pool_handle_trace(phase, observe);
            perry_ffi_gc_register_pool_handle_liveness(is_live);
        }
    }
    let parked = Parked {
        kind,
        identity,
        epoch,
        reached: 0,
        not_before,
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
            let unproven = count.saturating_sub(epoch.live);
            if unproven < epoch.trigger_at || epoch.requested_pending {
                return false;
            }
            if unproven < MAX_PARKED {
                if let Some(end) = epoch.last_end {
                    if end.elapsed() < epoch.last_cost * AMORTIZE {
                        return false;
                    }
                }
            }
            epoch.note_request();
            true
        })
        .unwrap_or(false);
    if request {
        request_full_trace();
    }
}

static OFF_MUTATOR_RETIREMENTS: AtomicUsize = AtomicUsize::new(0);

/// An id retired on a thread with no heap of its own is adopted by the next
/// mutator to trace, which is sound only while the value cannot live in
/// another agent's heap. Debug builds report the first such site with its
/// backtrace (unit-test threads that never start a collector retire ids too,
/// so this reports instead of aborting); every build counts them.
fn note_off_mutator_retirement(id: Handle) {
    // Tests make the case fatal; the knob is read only on this path.
    if std::env::var_os("PERRY_HANDLE_OFF_MUTATOR_FATAL").is_some() {
        eprintln!("perry-ffi: handle id {id} retired off a mutator thread (PERRY_HANDLE_OFF_MUTATOR_FATAL)");
        std::process::abort();
    }
    let first = OFF_MUTATOR_RETIREMENTS.fetch_add(1, Ordering::Relaxed) == 0;
    if cfg!(debug_assertions) && first {
        eprintln!(
            "perry-ffi: handle id {id} retired off a mutator thread; retire ids on the owning agent\n{}",
            std::backtrace::Backtrace::force_capture()
        );
    }
}

/// Ids retired on a thread that runs no collector (diagnostics, tests).
#[doc(hidden)]
pub fn off_mutator_retirements() -> usize {
    OFF_MUTATOR_RETIREMENTS.load(Ordering::Relaxed)
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
    park(
        identity.numeric_id(),
        Kind::Reclaimable(owner),
        identity,
        None,
    );
}

/// A removed payload's id, still `Retiring` in the pool: reusable once a full
/// trace proves no JS value still holds the number.
pub fn park_retired(identity: NativeRegistrationIdentity) {
    park(identity.numeric_id(), Kind::Retired, identity, None);
}

/// [`park_retired`] that also holds the id until `deadline`: no trace that
/// began before it releases the id, whatever it proves.
pub fn park_retired_until(identity: NativeRegistrationIdentity, deadline: Instant) {
    park(
        identity.numeric_id(),
        Kind::Retired,
        identity,
        Some(deadline),
    );
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

impl Epoch {
    fn note_request(&mut self) {
        self.requested_pending = true;
        self.span_from = Some(self.registrations);
    }

    /// Obtainable shared ids below which registration requests a trace: room
    /// for what this mutator consumes across the two traces a release needs,
    /// plus the stride before the check sees the band low (design 2.5).
    fn band_reserve(&self) -> usize {
        BAND_RESERVE.max(
            self.span
                .saturating_mul(2)
                .saturating_add(2 * BAND_CHECK_STRIDE as usize),
        )
    }
}

/// Called on every registration in the shared band: keeps traces coming while
/// the band is running low, whoever is consuming it.
pub fn note_registration() {
    let reserve = EPOCH
        .try_with(|epoch| {
            let mut epoch = epoch.borrow_mut();
            epoch.registrations = epoch.registrations.wrapping_add(1);
            (epoch.registrations % BAND_CHECK_STRIDE == 0 && !epoch.requested_pending)
                .then(|| epoch.band_reserve())
        })
        .ok()
        .flatten();
    let Some(reserve) = reserve else {
        return;
    };
    if REGISTRATIONS.available_ids() >= reserve {
        return;
    }
    let unproven = PARKED
        .try_with(|map| map.borrow().0.len())
        .unwrap_or(0)
        .saturating_sub(EPOCH.with(|epoch| epoch.borrow().live));
    // Nothing parked here is unproven: a trace could free nothing for this
    // mutator, and the program holds what it registered.
    if unproven == 0 && ORPHANS.lock().map_or(true, |orphans| orphans.is_empty()) {
        return;
    }
    EPOCH.with(|epoch| epoch.borrow_mut().note_request());
    request_full_trace();
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

/// The paced trigger and the live count it derives from (tests).
#[doc(hidden)]
pub fn trace_trigger_for_tests() -> (usize, usize) {
    EPOCH.with(|epoch| {
        let epoch = epoch.borrow();
        (epoch.trigger_at, epoch.live)
    })
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
                    epoch.trace_began = Some(Instant::now());
                    if std::mem::take(&mut epoch.requested_pending) {
                        epoch.requested_started = Some(Instant::now());
                    }
                    if epoch.span_from.is_none() {
                        epoch.span_from = Some(epoch.registrations);
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
                epoch.trace_began = None;
                epoch.current = epoch.current.saturating_sub(1);
            });
            true
        }
    }
}

/// Whether the armed trace of epoch `current`, begun at `began`, keeps a
/// parked id: parked in the previous epoch or later (born since the previous
/// trace began, or during this one), reached by this trace, or still inside
/// its grace deadline. One predicate for weak-key verdicts and releases.
fn trace_keeps(parked: &Parked, current: u64, began: Option<Instant>) -> bool {
    parked.epoch >= current.saturating_sub(1)
        || parked.reached == current
        || parked
            .not_before
            .is_some_and(|deadline| began.is_none_or(|began| began < deadline))
}

fn finish_full_trace() {
    let (current, began) = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.tracing = false;
        epoch.full_traces += 1;
        if let Some(from) = epoch.span_from.take() {
            epoch.span = epoch.registrations.wrapping_sub(from) as usize;
        }
        (epoch.current, epoch.trace_began.take())
    });
    let mut dead = Vec::new();
    let mut live = 0usize;
    PARKED.with(|map| {
        let mut map = map.borrow_mut();
        map.0.retain(|_, parked| {
            let keep = trace_keeps(parked, current, began);
            if !keep {
                dead.push(*parked);
            } else if parked.epoch < current.saturating_sub(1) && parked.reached == current {
                // Decidable, and a word named it: the program holds it.
                live += 1;
            }
            keep
        });
    });
    EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.live = live;
        epoch.trigger_at = MIN_TRIGGER.max(live.saturating_mul(2));
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

/// Liveness of a handle-band key word during an armed trace (the runtime asks
/// for weak and ephemeron keys). False exactly for the ids this trace's finish
/// will release, so their weak entries are deleted in the same trace.
extern "C" fn is_live(bits: u64) -> bool {
    let id = (bits & 0x0000_FFFF_FFFF_FFFF) as Handle;
    let Some(parked) = PARKED
        .try_with(|map| map.borrow().0.get(&id).copied())
        .ok()
        .flatten()
    else {
        return true;
    };
    EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            !epoch.tracing || trace_keeps(&parked, epoch.current, epoch.trace_began)
        })
        .unwrap_or(true)
}

/// Ask the liveness predicate directly (tests).
#[doc(hidden)]
pub fn is_live_for_tests(bits: u64) -> bool {
    is_live(bits)
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
