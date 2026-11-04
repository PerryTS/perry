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
//! **Young ids.** Ids parked while a trace runs are never decided by it. An
//! id parked since the previous trace began is kept by the next one only if
//! that trace began in the event-loop turn the id was parked in: a native
//! frame may hold a fresh id unpublished across an allocation that begins a
//! trace, but no native frame of an earlier turn is still running (the runtime
//! counts a turn only at the generated main loop). A thread whose turn never
//! advances keeps every such id through one more trace. Each parked record
//! carries the trace epoch and the turn it was parked in and the epoch of the
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
//! **Band reserve.** A release may need two traces to begin after the park
//! (the first keeps an id parked in its own turn as young). So when fewer than `max(BAND_RESERVE, 2 x
//! span + 2 x stride)` shared ids are obtainable, `span` being the ids this
//! mutator registered across its last trace (request to finish), registration
//! requests a trace without the time budget. The band then runs out only if
//! the program itself names nearly all of it.
//!
//! **Parked records.** A payload of perry-ffi's registry parks *with its
//! record*: [`crate::park_reclaimable_ffi`] moves the `NativePayloadRecord`
//! out of the registry into the parked entry, so the registry and every type
//! scanner walking it hold only payloads native code owns. Lookups on the
//! parking mutator fall back to the entry; the payload box does not move. One
//! root scanner visits the parked records through their owner
//! ([`ParkedRecordOwner::visit`]): minors and relocation scans visit all of
//! them, a full mark only the young ones and those borrowed mutably during the
//! trace, and a rescan of the same trace (the final remark) only those borrowed
//! mutably since or parked after the first scan reached them. An old record's
//! edges are marked through a traced word naming its id.
//!
//! **Threads.** Parking is per mutator: each thread's trace decides only the
//! ids that thread parked. A thread that exits hands its parked ids to the
//! next mutator to begin a trace, which decides them against its own heap.
use crate::handle::{GcRootVisitor, Handle, HandleIdBuildHasher, REGISTRATIONS};
use crate::{NativePayloadRecord, NativeRegistrationIdentity};
use std::any::Any;
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
    fn perry_ffi_gc_event_loop_turn() -> u64;
    fn perry_ffi_gc_note_parked_bytes(bytes: usize, parked: bool);
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

#[cfg(all(test, not(feature = "runtime-link")))]
thread_local! {
    static TEST_EVENT_LOOP_TURN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, not(feature = "runtime-link")))]
thread_local! {
    static TEST_PARKED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// The unit tests read the bytes the collector would count.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_note_parked_bytes(bytes: usize, parked: bool) {
    TEST_PARKED_BYTES.with(|held| {
        let now = if parked {
            held.get() + bytes
        } else {
            held.get()
                .checked_sub(bytes)
                .expect("released more parked bytes than were parked")
        };
        held.set(now);
    });
}

/// Parked payload bytes this thread's collector counts (unit tests).
#[cfg(all(test, not(feature = "runtime-link")))]
pub(crate) fn parked_bytes_for_tests() -> usize {
    TEST_PARKED_BYTES.with(std::cell::Cell::get)
}

// The unit tests begin event-loop turns themselves.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_event_loop_turn() -> u64 {
    TEST_EVENT_LOOP_TURN.with(std::cell::Cell::get)
}

/// Begin an event-loop turn on this test thread.
#[cfg(all(test, not(feature = "runtime-link")))]
pub(crate) fn begin_event_loop_turn_for_tests() {
    TEST_EVENT_LOOP_TURN.with(|turn| turn.set(turn.get() + 1));
}

fn event_loop_turn() -> u64 {
    unsafe { perry_ffi_gc_event_loop_turn() }
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
    /// Push every GC edge the payload of `id` holds for the current mutator,
    /// as JSValue words: its property bag, its callbacks and the handle words
    /// of payloads it names. A full trace marks them the first time a word
    /// naming `id` is reached; until then the owner's root scanner skips them
    /// (see [`payload_edges_are_rooted`]).
    fn edges(&self, id: Handle, out: &mut Vec<u64>);
}

/// The owner of a parked perry-ffi payload record (a type family of
/// [`crate::register_handle`] payloads whose owners are JS values once native
/// code is done with them). perry-ffi holds the record and drops it when a
/// trace proves the id unnamed; the owner only knows the payload's edges.
pub trait ParkedRecordOwner: Sync {
    /// Visit every GC edge `payload` holds for the current mutator, exactly as
    /// the owner's root scanner visits a native-owned payload of the same type
    /// (its property bag is visited by perry-ffi). A full mark reaches this
    /// through the parked-record scanner while the record is young or written,
    /// and otherwise through `observe` the first time a word naming the id is
    /// traced (with a visitor that collects the edges as words). Runs with no
    /// registry lock held; it may visit handle words of other payloads.
    fn visit(&self, payload: &mut (dyn Any + Send + Sync), visitor: &mut GcRootVisitor<'_>);
    /// The native bytes `payload` holds: what dropping it gives back to the
    /// allocator. While the record is parked only a full trace can free them,
    /// so the mutator's collector counts them as heap bytes.
    fn bytes(&self, payload: &(dyn Any + Send + Sync)) -> usize;
}

enum Kind {
    /// Payload still registered with its owner's own map; dropped through the
    /// owner when proven unreachable.
    Reclaimable(&'static dyn ParkedPayloadOwner),
    /// A perry-ffi payload record, moved out of the registry at park; perry-ffi
    /// drops it when proven unreachable.
    Record {
        owner: &'static dyn ParkedRecordOwner,
        record: Box<NativePayloadRecord>,
    },
    /// Payload already removed; the id sits in `Retiring` until proven.
    Retired,
}

impl Kind {
    fn reclaimable(&self) -> bool {
        !matches!(self, Kind::Retired)
    }
}

struct Parked {
    kind: Kind,
    identity: NativeRegistrationIdentity,
    /// Trace epoch the id was parked in.
    epoch: u64,
    /// Event-loop turn the id was parked in (see [`is_young`]).
    turn: u64,
    /// Epoch of the last trace that observed a word naming the id.
    reached: u64,
    /// Epoch of the last trace during which native code borrowed the payload
    /// mutably (a possible edge store after `observe` took its edges).
    written: u64,
    /// Serial of the last full trace whose root scan visited the record's
    /// edges (`Epoch::serial`, never reused, unlike an aborted epoch).
    visited: u64,
    /// A retirement with a grace deadline (`drop_handle_until`) is not
    /// released by a trace that began before it.
    not_before: Option<Instant>,
    /// The record is named by the young log (`Epoch::young_records`) and not
    /// yet taken by a minor-scoped walk: a note while set adds nothing, so
    /// the log names each record at most once between walks.
    young_logged: bool,
    /// Payload bytes this mutator's collector counts as heap bytes while the
    /// record is parked: memory the GC keeps alive, which the full that proves
    /// the id unnamed frees. Added when the record is parked here and
    /// subtracted when it leaves (proven dead, taken back, retained).
    counted: usize,
}

struct Epoch {
    /// Incremented when a trace begins; parked ids record it.
    current: u64,
    /// Incremented when a trace begins and never decremented (an aborted
    /// trace gives its epoch back, not its serial).
    serial: u64,
    /// A trace armed by `phase(0)` is running.
    tracing: bool,
    /// Event-loop turn the current (or last) trace began in.
    began_turn: u64,
    /// When the armed trace began: the one clock its deadline checks read,
    /// so its weak-key verdicts and its releases agree.
    trace_began: Option<Instant>,
    /// Unproven parked ids (`parked - live`) at which parking requests a trace.
    trigger_at: usize,
    /// Parked ids the last finished trace could decide and found named: what
    /// the program holds. Young ids it only kept are not counted.
    live: usize,
    hooked: bool,
    /// The parked-record scanner is registered for this mutator.
    records_hooked: bool,
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
    /// Parked records that may hold a word a minor can act on (`gc/young_log.rs`
    /// in perry-runtime, applied to the parked records): noted before a record
    /// becomes findable and on every mutable borrow of a parked record; a
    /// minor-scoped walk visits only these and keeps the ones still relevant.
    /// A log, not an index: stale ids are allowed; a record's
    /// `young_logged` mark keeps it from being named twice.
    young_records: Vec<Handle>,
    /// The last minor-scoped walk of the parked records (tests, diagnostics).
    young_walk: YoungRecordWalk,
}

/// One minor-scoped walk of the parked records: ids the log named (each record
/// at most once, see `Parked::young_logged`),
/// records visited, records still relevant afterwards, records parked.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct YoungRecordWalk {
    pub logged: usize,
    pub visited: usize,
    pub kept: usize,
    pub parked: usize,
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
        // The dying heap's counters go with it; an adopter counts nothing.
        if let Ok(mut orphans) = ORPHANS.lock() {
            orphans.extend(self.0.drain().map(|(id, mut parked)| {
                parked.counted = 0;
                (id, parked)
            }));
        }
    }
}

static ORPHANS: Mutex<Vec<(Handle, Parked)>> = Mutex::new(Vec::new());

thread_local! {
    static PARKED: RefCell<ParkedIds> = RefCell::new(ParkedIds::default());
    static EPOCH: RefCell<Epoch> = const { RefCell::new(Epoch {
        current: 0,
        serial: 0,
        tracing: false,
        began_turn: 0,
        trace_began: None,
        trigger_at: MIN_TRIGGER,
        live: 0,
        hooked: false,
        records_hooked: false,
        registrations: 0,
        span_from: None,
        span: 0,
        requested_started: None,
        requested_pending: false,
        last_cost: Duration::ZERO,
        last_end: None,
        full_traces: 0,
        young_records: Vec::new(),
        young_walk: YoungRecordWalk { logged: 0, visited: 0, kept: 0, parked: 0 },
    }) };
    /// Payloads a finished trace proved unnamed, released by this mutator
    /// outside the collection pause (see [`release_proven`]).
    static PROVEN_DEAD: RefCell<ProvenDead> = const { RefCell::new(ProvenDead(Vec::new())) };
}

/// Proven-dead parked entries awaiting release. A dying thread releases them.
struct ProvenDead(Vec<Parked>);

impl Drop for ProvenDead {
    fn drop(&mut self) {
        release(std::mem::take(&mut self.0));
    }
}

fn park(id: Handle, kind: Kind, identity: NativeRegistrationIdentity, not_before: Option<Instant>) {
    let unstamped = |kind| Parked {
        kind,
        identity,
        epoch: 0,
        turn: 0,
        reached: 0,
        written: 0,
        visited: 0,
        not_before,
        young_logged: false,
        counted: 0,
    };
    // Only a mutator's full traces see the values that can name the id. A
    // retirement elsewhere (a worker pool thread) has no heap to decide it.
    if !unsafe { perry_ffi_gc_thread_is_mutator() } {
        note_off_mutator_retirement(id);
        orphan(id, unstamped(kind));
        return;
    }
    let record = matches!(kind, Kind::Record { .. });
    let Ok((hooked, records_hooked, epoch)) = EPOCH.try_with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        let hooked = std::mem::replace(&mut epoch.hooked, true);
        let records_hooked = epoch.records_hooked;
        epoch.records_hooked |= record;
        if record {
            // Before the record is findable in `PARKED`: a minor may run as
            // soon as it is.
            epoch.young_records.push(id);
        }
        // First mutator activity after a requested trace: charge its whole
        // cost, sweep included, to the pacing budget.
        if let Some(started) = epoch.requested_started.take() {
            let now = Instant::now();
            epoch.last_cost = now - started;
            epoch.last_end = Some(now);
        }
        (hooked, records_hooked, epoch.current)
    }) else {
        // Thread teardown: nothing can trace this thread any more.
        orphan(id, unstamped(kind));
        return;
    };
    if !hooked {
        unsafe {
            perry_ffi_gc_register_pool_handle_trace(phase, observe);
            perry_ffi_gc_register_pool_handle_liveness(is_live);
        }
    }
    // perry-ffi's own unit-test binary has no collector to register with; its
    // tests drive the scanner through `full_scan_edges_for_tests`.
    #[cfg(all(test, not(feature = "runtime-link")))]
    let _ = records_hooked;
    #[cfg(not(all(test, not(feature = "runtime-link"))))]
    if record && !records_hooked {
        crate::handle::gc_register_mutable_root_scanner_named(
            "ffi:parked-payloads",
            scan_parked_record_roots,
        );
    }
    let counted = match &kind {
        Kind::Record { owner, record } => owner.bytes(record.payload.as_ref()),
        _ => 0,
    };
    let mut parked = unstamped(kind);
    parked.epoch = epoch;
    parked.turn = event_loop_turn();
    parked.young_logged = record;
    parked.counted = counted;
    let mut slot = Some(parked);
    let Ok((count, replaced)) = PARKED.try_with(|map| {
        let mut map = map.borrow_mut();
        let replaced = slot.take().and_then(|parked| map.0.insert(id, parked));
        (map.0.len(), replaced.map_or(0, |old| old.counted))
    }) else {
        if let Some(mut parked) = slot.take() {
            parked.counted = 0;
            orphan(id, parked);
        }
        return;
    };
    uncount(replaced);
    count_parked(counted);
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
    release_proven(RELEASE_BATCH);
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

/// A parked record's payload bytes join this mutator's heap bytes.
fn count_parked(bytes: usize) {
    if bytes != 0 {
        unsafe { perry_ffi_gc_note_parked_bytes(bytes, true) };
    }
}

/// Parked payload bytes leave this mutator's heap bytes.
fn uncount(bytes: usize) {
    if bytes != 0 {
        unsafe { perry_ffi_gc_note_parked_bytes(bytes, false) };
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

/// A perry-ffi payload record whose owners are JS values: perry-ffi holds it
/// in the parked entry and drops it, recycling the id, once a full trace
/// proves nothing names it (see [`crate::park_reclaimable_ffi`]).
pub(crate) fn park_record(
    identity: NativeRegistrationIdentity,
    owner: &'static dyn ParkedRecordOwner,
    record: NativePayloadRecord,
) {
    park(
        identity.numeric_id(),
        Kind::Record {
            owner,
            record: Box::new(record),
        },
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
    let Some(kind) = PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            match map.0.get(&id) {
                Some(parked) if parked.kind.reclaimable() => map.0.remove(&id),
                _ => None,
            }
        })
        .ok()
        .flatten()
        .map(|parked| {
            uncount(parked.counted);
            parked.kind
        })
    else {
        return false;
    };
    // A record goes back to the registry its native owner reads.
    if let Kind::Record { record, .. } = kind {
        crate::handle::restore_parked_record(id, *record);
    }
    true
}

/// The boxed record of `id` while this mutator holds it parked. The box does
/// not move until the entry is released or retired, so the pointer stays valid
/// for as long as a registry borrow would; no `PARKED` borrow is held on
/// return (visiting and observing re-enter it).
pub(crate) fn parked_record(id: Handle) -> Option<*mut NativePayloadRecord> {
    PARKED
        .try_with(|map| match map.borrow_mut().0.get_mut(&id) {
            Some(Parked {
                kind: Kind::Record { record, .. },
                ..
            }) => Some(&mut **record as *mut NativePayloadRecord),
            _ => None,
        })
        .ok()
        .flatten()
}

/// [`parked_record`] for a mutable borrow: the borrower may store a word a
/// minor can act on, so the record is noted in the young log first.
pub(crate) fn parked_record_for_write(id: Handle) -> Option<*mut NativePayloadRecord> {
    let record = parked_record(id)?;
    note_young_write(id);
    Some(record)
}

/// Note a parked record in the young log after native code wrote it (a minor
/// that ran inside the borrow may have taken it from the log before the
/// store). A record the log already names is not named twice.
pub(crate) fn note_young_write(id: Handle) {
    let newly = PARKED
        .try_with(|map| match map.borrow_mut().0.get_mut(&id) {
            Some(
                parked @ Parked {
                    kind: Kind::Record { .. },
                    ..
                },
            ) => !std::mem::replace(&mut parked.young_logged, true),
            _ => false,
        })
        .unwrap_or(false);
    if newly {
        let _ = EPOCH.try_with(|epoch| epoch.borrow_mut().young_records.push(id));
    }
}

/// Take the record of a parked `id` back from this mutator (its payload is
/// being dropped or taken by native code; the caller retires the id).
pub(crate) fn take_parked_record(id: Handle) -> Option<NativePayloadRecord> {
    PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            match map.0.get(&id) {
                Some(Parked {
                    kind: Kind::Record { .. },
                    ..
                }) => {
                    let parked = map.0.remove(&id)?;
                    uncount(parked.counted);
                    match parked.kind {
                        Kind::Record { record, .. } => Some(*record),
                        _ => None,
                    }
                }
                _ => None,
            }
        })
        .ok()
        .flatten()
}

/// Run `f` on every parked record of type `T` this mutator holds (agent
/// retirement clears a dying heap's edges from finished payloads too). No
/// `PARKED` borrow is held while `f` runs.
pub fn iter_parked_payloads_of_mut<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&mut T),
{
    for (_, record) in parked_records(|_| true) {
        // SAFETY: see `parked_record`; nothing releases records while `f` runs.
        if let Some(payload) = unsafe { &mut *record }.payload.downcast_mut::<T>() {
            f(payload);
        }
    }
}

/// The owner and record of each parked record `select` accepts.
fn parked_records(
    mut select: impl FnMut(&mut Parked) -> bool,
) -> Vec<(&'static dyn ParkedRecordOwner, *mut NativePayloadRecord)> {
    PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            let mut records = Vec::new();
            for parked in map.0.values_mut() {
                if !matches!(parked.kind, Kind::Record { .. }) || !select(parked) {
                    continue;
                }
                if let Kind::Record { owner, record } = &mut parked.kind {
                    records.push((*owner, &mut **record as *mut NativePayloadRecord));
                }
            }
            records
        })
        .unwrap_or_default()
}

/// Clear the property bags whose allocations a retiring mutator frees, on
/// this mutator's parked records and on orphaned ones.
pub(crate) fn clear_parked_props_in_ranges(contains: &dyn Fn(usize) -> bool) {
    for (_, record) in parked_records(|_| true) {
        unsafe { &mut *record }.clear_props_if(contains);
    }
    if let Ok(mut orphans) = ORPHANS.lock() {
        for (_, parked) in orphans.iter_mut() {
            if let Kind::Record { record, .. } = &mut parked.kind {
                record.clear_props_if(contains);
            }
        }
    }
}

/// Whether a full trace's root scan of serial `serial` and epoch `current`
/// visits a parked record. Young records (they may still be unpublished) and
/// records borrowed mutably during this trace are roots; an old record's edges
/// are marked only through a word naming its id. A rescan of the same trace
/// (the final remark) skips a record the earlier scan visited unless it was
/// borrowed mutably since: its edges cannot have changed.
fn full_scan_visits(parked: &Parked, current: u64, began_turn: u64, serial: u64) -> bool {
    let written = parked.written == current;
    if parked.visited == serial && !written {
        return false;
    }
    written || is_young(parked, current, began_turn)
}

/// Whether the trace of epoch `current`, begun in event-loop turn
/// `began_turn`, treats a parked id as young: parked during this trace, or
/// parked since the previous trace began and in the turn this trace began in.
/// A native frame may hold a fresh id unpublished across an allocation that
/// begins a trace; no native frame of an earlier turn is still running, so an
/// id parked in an earlier turn is decided by this trace. The one young rule
/// for root visits, verdicts and the live count.
fn is_young(parked: &Parked, current: u64, began_turn: u64) -> bool {
    parked.epoch >= current
        || (parked.epoch.saturating_add(1) == current && parked.turn == began_turn)
}

/// The mutable root scanner of this mutator's parked records: each record's
/// property bag and its owner's edges. Minors and relocation scans visit all
/// of them (they rewrite moved referents); a full mark only those
/// [`full_scan_visits`] selects.
fn scan_parked_record_roots(visitor: &mut GcRootVisitor<'_>) {
    if visitor.is_young_scope() {
        scan_young_parked_records(visitor);
        return;
    }
    let full_mark = visitor.is_full_mark();
    let (current, began_turn, serial) = EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            (epoch.current, epoch.began_turn, epoch.serial)
        })
        .unwrap_or((0, 0, 0));
    let records = parked_records(|parked| {
        if !full_mark {
            return true;
        }
        let visits = full_scan_visits(parked, current, began_turn, serial);
        if visits {
            parked.visited = serial;
        }
        visits
    });
    for (owner, record) in records {
        // SAFETY: see `parked_record`; a scan releases nothing.
        let record = unsafe { &mut *record };
        if let Some(slot) = record.props_slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        owner.visit(record.as_mut(), visitor);
    }
}

/// A minor-scoped pass acts only on minor-relevant words, so a parked record
/// whose bag and owner edges are all old is a no-op for it. Visit the records
/// the young log names, exactly as the full walk does, and keep logged the
/// ones still relevant after the visit (a to-space survivor stays young, a
/// promoted referent is old and drops out). Full-scope walks leave the log
/// alone: they never make an old word young, and only this walk drops ids.
fn scan_young_parked_records(visitor: &mut GcRootVisitor<'_>) {
    let Ok(batch) = EPOCH.try_with(|epoch| std::mem::take(&mut epoch.borrow_mut().young_records))
    else {
        return;
    };
    #[cfg(debug_assertions)]
    debug_assert_young_records_logged();
    let mut walk = YoungRecordWalk {
        logged: batch.len(),
        parked: parked_handle_count(),
        ..YoungRecordWalk::default()
    };
    let mut kept = Vec::new();
    let mut edges = Vec::new();
    for id in batch {
        // Taking the mark visits a record once per walk, whatever the log
        // holds for it (a stale entry of a reissued id included).
        let Some((owner, record)) = parked_record_of(id, true) else {
            continue;
        };
        // SAFETY: see `parked_record`; a scan releases nothing.
        let record = unsafe { &mut *record };
        walk.visited += 1;
        if let Some(slot) = record.props_slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        owner.visit(record.as_mut(), visitor);
        edges.clear();
        record_edges(owner, record, &mut edges);
        if edges
            .iter()
            .any(|&bits| crate::handle::word_is_minor_relevant(bits))
        {
            kept.push(id);
        }
    }
    walk.kept = kept.len();
    let _ = PARKED.try_with(|map| {
        let mut map = map.borrow_mut();
        for id in &kept {
            if let Some(parked) = map.0.get_mut(id) {
                parked.young_logged = true;
            }
        }
    });
    let _ = EPOCH.try_with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        // Notes made during the walk stay; the kept ids join them.
        epoch.young_records.extend(kept);
        epoch.young_walk = walk;
    });
}

/// The owner and record of the parked record `id`, if this mutator holds one.
/// `take_mark`: only if the young log names it, and the mark is taken.
fn parked_record_of(
    id: Handle,
    take_mark: bool,
) -> Option<(&'static dyn ParkedRecordOwner, *mut NativePayloadRecord)> {
    PARKED
        .try_with(|map| match map.borrow_mut().0.get_mut(&id) {
            Some(Parked {
                kind: Kind::Record { owner, record },
                young_logged,
                ..
            }) => {
                if take_mark && !std::mem::take(young_logged) {
                    return None;
                }
                Some((*owner, &mut **record as *mut NativePayloadRecord))
            }
            _ => None,
        })
        .ok()
        .flatten()
}

/// The young-log rule 2 for the parked records: every parked record that holds
/// a minor-relevant word right now is named by the log a minor-scoped walk
/// took. A miss is a writer that stores into a parked record without noting it.
#[cfg(debug_assertions)]
fn debug_assert_young_records_logged() {
    let ids: Vec<(Handle, bool)> = PARKED
        .try_with(|map| {
            map.borrow()
                .0
                .iter()
                .filter(|(_, parked)| matches!(parked.kind, Kind::Record { .. }))
                .map(|(&id, parked)| (id, parked.young_logged))
                .collect()
        })
        .unwrap_or_default();
    let mut edges = Vec::new();
    for (id, logged) in ids {
        let Some((owner, record)) = parked_record_of(id, false) else {
            continue;
        };
        edges.clear();
        // SAFETY: see `parked_record`.
        record_edges(owner, unsafe { &mut *record }, &mut edges);
        if edges
            .iter()
            .any(|&bits| crate::handle::word_is_minor_relevant(bits))
        {
            assert!(
                logged,
                "perry-ffi: parked record {id} holds a minor-relevant word but the \
                 young log does not name it: a writer stored into it without noting it"
            );
        }
    }
}

/// The last minor-scoped walk of the parked records (tests, diagnostics).
#[doc(hidden)]
pub fn young_record_walk_for_tests() -> YoungRecordWalk {
    EPOCH.with(|epoch| epoch.borrow().young_walk)
}

/// The words one minor-scoped scan of the parked records visits (tests).
#[doc(hidden)]
pub fn young_scan_edges_for_tests() -> Vec<u64> {
    let mut edges = Vec::new();
    scan_parked_record_roots(&mut GcRootVisitor::young_edge_collector_for_tests(
        &mut edges,
    ));
    edges.retain(|&bits| bits != 0);
    edges
}

/// The words one full-mark scan of the parked records visits right now (a
/// rescan within the same trace sees only what it would revisit) (tests).
#[doc(hidden)]
pub fn full_scan_edges_for_tests() -> Vec<u64> {
    let mut edges = Vec::new();
    scan_parked_record_roots(&mut GcRootVisitor::edge_collector(&mut edges));
    edges.retain(|&bits| bits != 0);
    edges
}

/// The edges `observe` marks when a word first names the parked record `id`
/// (tests).
#[doc(hidden)]
pub fn parked_record_edges_for_tests(id: Handle) -> Vec<u64> {
    let mut edges = Vec::new();
    let owner = PARKED.with(|map| match map.borrow().0.get(&id).map(|p| &p.kind) {
        Some(Kind::Record { owner, .. }) => Some(*owner),
        _ => None,
    });
    if let (Some(owner), Some(record)) = (owner, parked_record(id)) {
        // SAFETY: see `parked_record`.
        record_edges(owner, unsafe { &mut *record }, &mut edges);
    }
    edges.retain(|&bits| bits != 0);
    edges
}

/// A parked record's edges as words: its property bag and what its owner's
/// visit reaches, collected rather than marked.
fn record_edges(
    owner: &'static dyn ParkedRecordOwner,
    record: &mut NativePayloadRecord,
    edges: &mut Vec<u64>,
) {
    edges.extend(record.props());
    let mut collect = GcRootVisitor::edge_collector(edges);
    owner.visit(record.as_mut(), &mut collect);
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
    release_proven(RELEASE_BATCH);
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

/// Whether a full mark's root scanner must visit the edges of the payload of
/// `id` itself. True for a payload no parked record owns (native code owns it),
/// for a young parked record (it may still be unpublished), and for one native
/// code borrowed mutably during this trace: an edge stored after `observe` took
/// the payload's edges is reached by the final root remark instead. An old
/// parked payload's edges are marked only through a traced word naming its id.
pub fn payload_edges_are_rooted(id: Handle) -> bool {
    let (current, began_turn) = EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            (epoch.current, epoch.began_turn)
        })
        .unwrap_or((0, 0));
    PARKED
        .try_with(|parked| {
            parked.borrow().0.get(&id).is_none_or(|parked| {
                is_young(parked, current, began_turn) || parked.written == current
            })
        })
        .unwrap_or(true)
}

/// Native code is about to borrow the payload of `id` mutably. During an armed
/// trace this may store an edge into a parked payload whose edges `observe`
/// already marked, so the payload is rooted for the rest of this trace.
/// Outside a trace it costs one thread-local read.
#[inline]
pub fn note_mutable_access(id: Handle) {
    let current = EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            epoch.tracing.then_some(epoch.current)
        })
        .ok()
        .flatten();
    let Some(current) = current else {
        return;
    };
    let _ = PARKED.try_with(|map| {
        if let Some(parked) = map.borrow_mut().0.get_mut(&id) {
            if parked.kind.reclaimable() {
                parked.written = current;
            }
        }
    });
}

/// Whether this mutator has `id` parked: its native owner is done with it
/// and a full trace decides it.
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
    let done = phase(phase_id);
    // The test is the mutator: it releases the yield right after the finish.
    if phase_id == 1 {
        release_all_proven();
    }
    done
}

/// Finish the armed trace and leave its yield for the mutator to release
/// (tests of the release outside the pause).
#[doc(hidden)]
pub fn trace_finish_unreleased_for_tests() -> bool {
    phase(1)
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
            // Nothing ran since the last finish released its yield: do it now,
            // before this trace stamps a new epoch.
            release_all_proven();
            let orphans = ORPHANS
                .lock()
                .map(|mut orphans| std::mem::take(&mut *orphans))
                .unwrap_or_default();
            let turn = event_loop_turn();
            let current = EPOCH
                .try_with(|epoch| {
                    let mut epoch = epoch.borrow_mut();
                    epoch.current += 1;
                    epoch.serial += 1;
                    epoch.began_turn = turn;
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
                    let mut adopted = Vec::new();
                    for (id, mut parked) in orphans {
                        // Adopted ids are decided no earlier than the next trace.
                        parked.epoch = current;
                        parked.turn = turn;
                        if matches!(parked.kind, Kind::Record { .. }) {
                            parked.young_logged = true;
                            adopted.push(id);
                        }
                        map.0.insert(id, parked);
                    }
                    // A minor may follow before the next full trace.
                    if !adopted.is_empty() {
                        let _ = EPOCH.try_with(|epoch| {
                            epoch.borrow_mut().young_records.extend(adopted);
                        });
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

/// Whether the armed trace of epoch `current`, begun in turn `began_turn` at
/// `began`, keeps a parked id: young ([`is_young`]), reached by this trace, or
/// still inside its grace deadline. One predicate for weak-key verdicts and
/// releases.
fn trace_keeps(parked: &Parked, current: u64, began_turn: u64, began: Option<Instant>) -> bool {
    is_young(parked, current, began_turn)
        || parked.reached == current
        || parked
            .not_before
            .is_some_and(|deadline| began.is_none_or(|began| began < deadline))
}

fn finish_full_trace() {
    let (current, began_turn, began) = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.tracing = false;
        epoch.full_traces += 1;
        if let Some(from) = epoch.span_from.take() {
            epoch.span = epoch.registrations.wrapping_sub(from) as usize;
        }
        (epoch.current, epoch.began_turn, epoch.trace_began.take())
    });
    let mut live = 0usize;
    let dead: Vec<Parked> = PARKED.with(|map| {
        let mut map = map.borrow_mut();
        let mut dead_ids = Vec::new();
        for (&id, parked) in map.0.iter() {
            if !trace_keeps(parked, current, began_turn, began) {
                dead_ids.push(id);
            } else if !is_young(parked, current, began_turn) && parked.reached == current {
                // Decidable, and a word named it: the program holds it.
                live += 1;
            }
        }
        dead_ids
            .into_iter()
            .filter_map(|id| map.0.remove(&id))
            .collect()
    });
    // This full frees the dead records' bytes: they leave the heap bytes it
    // measures as live, though the allocator gets them back after the pause.
    uncount(
        dead.iter()
            .map(|parked| parked.counted)
            .fold(0usize, usize::saturating_add),
    );
    EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.live = live;
        epoch.trigger_at = MIN_TRIGGER.max(live.saturating_mul(2));
    });
    // The verdict is final, but dropping the payloads (allocator frees, id
    // retirement) needs no stopped world: the mutator releases them after the
    // pause, at the next tick begin and alongside its next parks and
    // registrations, the way native code frees a payload it owns when its
    // work finishes.
    let _ = PROVEN_DEAD.try_with(|proven| proven.borrow_mut().0.extend(dead));
}

/// Proven-dead entries one park or registration releases. Each finished
/// exchange parks and registers several ids, so the mutator releases faster
/// than it parks and the backlog stays within one trace's yield.
const RELEASE_BATCH: usize = 8;

/// Release up to `max` of this mutator's proven-dead parked entries. Called
/// outside the collection pause, with no registry lock or `PARKED` borrow
/// held; a release may re-enter perry-ffi (one entry is taken at a time).
pub(crate) fn release_proven(max: usize) {
    for _ in 0..max {
        let Some(parked) = PROVEN_DEAD
            .try_with(|proven| proven.borrow_mut().0.pop())
            .ok()
            .flatten()
        else {
            return;
        };
        release(vec![parked]);
    }
}

/// Release every proven-dead parked entry this thread holds (the tick-begin
/// hook; a trace that begins with some left releases them first).
pub fn release_all_proven() {
    release_proven(usize::MAX);
}

/// Proven-dead parked entries not yet released on this thread (tests).
#[doc(hidden)]
pub fn proven_dead_pending_for_tests() -> usize {
    PROVEN_DEAD
        .try_with(|proven| proven.borrow().0.len())
        .unwrap_or(0)
}

/// Drop proven-dead payloads and hand their ids back to the shared pool.
/// Allocates nothing on the GC heap; payload destructors run outside every
/// registry lock.
fn release(dead: Vec<Parked>) {
    for parked in dead {
        match parked.kind {
            Kind::Reclaimable(owner) => owner.release(parked.identity),
            Kind::Record { record, .. } => {
                crate::handle::release_parked_record(parked.identity, *record)
            }
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
    let Some((current, began_turn, began)) = EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            epoch
                .tracing
                .then_some((epoch.current, epoch.began_turn, epoch.trace_began))
        })
        .ok()
        .flatten()
    else {
        return true;
    };
    PARKED
        .try_with(|map| {
            map.borrow()
                .0
                .get(&id)
                .is_none_or(|parked| trace_keeps(parked, current, began_turn, began))
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
    let tracing = EPOCH
        .try_with(|epoch| {
            let epoch = epoch.borrow();
            epoch.tracing.then_some(epoch.current)
        })
        .ok()
        .flatten();
    let Some(edges_of) = PARKED
        .try_with(|map| {
            let mut map = map.borrow_mut();
            let parked = map.0.get_mut(&id)?;
            let first = tracing
                .is_some_and(|current| std::mem::replace(&mut parked.reached, current) != current);
            Some(first.then_some(match &mut parked.kind {
                Kind::Reclaimable(owner) => EdgesOf::Owner(*owner),
                Kind::Record { owner, record } => {
                    EdgesOf::Record(*owner, &mut **record as *mut NativePayloadRecord)
                }
                Kind::Retired => EdgesOf::None,
            }))
        })
        .ok()
        .flatten()
    else {
        return false;
    };
    // Marked outside every borrow and registry lock: marking may offer more
    // words (a paired payload's id) and re-enter `observe`.
    let mut edges = Vec::new();
    match edges_of {
        None | Some(EdgesOf::None) => {}
        Some(EdgesOf::Owner(owner)) => owner.edges(id, &mut edges),
        Some(EdgesOf::Record(owner, record)) => {
            // SAFETY: see `parked_record`; marking releases nothing.
            record_edges(owner, unsafe { &mut *record }, &mut edges);
        }
    }
    for bits in edges.into_iter().filter(|&bits| bits != 0) {
        mark(bits, ctx);
    }
    true
}

/// Where `observe` takes a newly reached parked id's edges from.
enum EdgesOf {
    Owner(&'static dyn ParkedPayloadOwner),
    Record(&'static dyn ParkedRecordOwner, *mut NativePayloadRecord),
    None,
}
