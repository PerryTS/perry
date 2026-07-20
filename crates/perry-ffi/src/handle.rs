//! Handle registry — opaque integer IDs for Rust objects that
//! survive across the FFI boundary (added in v0.5.x of the
//! perry-ffi v0.5 surface — non-breaking; pure additions).
//!
//! Most non-trivial wrappers (mysql2 connection pools, ws clients,
//! ioredis pipelines, even simple ones like lru-cache) need to
//! hand a long-lived Rust object to TypeScript and get it back
//! later. We can't pass Rust ownership directly across `extern "C"`
//! — the runtime can't drop a `Box<MyType>` because it doesn't know
//! `MyType`'s vtable. Instead we register the object in a global
//! [`DashMap`], return a small integer handle to TypeScript, and
//! every method call comes back through the FFI with the handle
//! plus a type-aware downcast.
//!
//! # Layout
//!
//! Single process-wide [`DashMap`] keyed by [`Handle`] (a `i64`).
//! A fresh `i64` is allocated atomically from a counter starting at
//! 1 — `0` is reserved as `INVALID_HANDLE` so `register_handle` can
//! never produce a falsy value (matches JS truthiness semantics
//! for type checks like `if (handle)`). Visible ids stop before
//! `0x40000`; the pointer-tagged small-handle band above that is
//! reserved for Web Fetch and proxy handles.
//!
//! Ids freed by [`drop_handle`] / [`take_handle`] are parked on a
//! bounded freelist and handed back out by [`register_handle`]
//! before the counter advances, so a handle-per-request workload
//! consumes ids in proportion to its *concurrent* live count rather
//! than its *cumulative* allocation count — while reclaimed ids fit
//! within the bounded freelist. Frees beyond [`FREE_HANDLES_CAP`]
//! are intentionally discarded, so a burst larger than the cap can
//! still advance [`NEXT_HANDLE`] and consume fresh ids. Ids are
//! therefore reused over time but a given id is unique among the
//! handles live at any instant — a recycled id is only parked after
//! its prior entry was removed from the map.
//!
//! A freed id is NOT reusable the instant it is freed: it first sits
//! in a quarantine and is promoted to the freelist only by
//! [`drain_quarantined_handles`], which the host event loop calls
//! once per tick. This deferral closes an ABA / use-after-recycle
//! hazard — a consumer holding a stale bare id (e.g. an HTTP handler's
//! `res` after the response was finalized) would otherwise see its id
//! re-occupied by the next registration within the same tick and
//! silently mutate a different object. See [`QUARANTINED_HANDLES`].
//!
//! perry-stdlib has its own copy of this same registry (in
//! `crates/perry-stdlib/src/common/handle.rs`). They are separate
//! integer spaces — perry-ffi-allocated handles cannot be looked
//! up via perry-stdlib's `get_handle`, and vice versa. Programs
//! that link both registries (e.g. via the well-known flip) just
//! end up with two `DashMap` statics; each wrapper consults the
//! registry it was compiled against. Values returned to JS can still collide
//! at the runtime dispatch layer if two subsystems expose the same
//! `POINTER_TAG | id` bits, so handle families that participate in generic
//! property/method dispatch reserve disjoint visible id ranges.
//!
//! # Safety
//!
//! [`get_handle`] / [`get_handle_mut`] return `'static` references
//! by exploiting the fact that DashMap entries are stable while
//! they exist. The caller must not drop the handle (via
//! [`take_handle`] / [`drop_handle`]) while a borrow is live.
//! Single-threaded FFI usage — the typical pattern — has no
//! aliasing problem; multi-threaded wrappers should use
//! [`with_handle`] which scopes the borrow under a closure.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use dashmap::DashMap;
use once_cell::sync::Lazy;

/// Opaque integer handle to a Rust object. `0` is reserved as
/// [`INVALID_HANDLE`]; valid handles start at `1`.
pub type Handle = i64;

/// Sentinel value for "no handle" / null. Never returned by
/// [`register_handle`]; may be passed in by FFI callers when the
/// JS side has `null` / `undefined`.
pub const INVALID_HANDLE: Handle = 0;

static HANDLES: Lazy<DashMap<Handle, Box<dyn Any + Send + Sync>>> = Lazy::new(DashMap::new);
const FFI_HANDLE_ID_START: Handle = 1;
const FFI_HANDLE_ID_END: Handle = 0x40000;

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(FFI_HANDLE_ID_START);

/// Freelist of ids reclaimed by [`drop_handle`] / [`take_handle`].
///
/// Without this, [`register_handle`] only ever bumps [`NEXT_HANDLE`], so a
/// long-lived process that allocates a handle per unit of work — e.g.
/// `perry-ext-http-server`, which registers a request + response handle per
/// request and `drop_handle`s both once the response flushes — burns through
/// the visible id band (`1 .. 0x40000`) and eventually panics in
/// [`next_fresh_handle_id`], even though only a handful of handles are live at
/// any instant. Recycling freed ids bounds id consumption by the *concurrent*
/// live-handle count rather than the *cumulative* allocation count.
///
/// Bounded at [`FREE_HANDLES_CAP`] idle ids: a brief spike that frees a huge
/// batch parks at most that many for reuse, and any excess is simply not
/// recycled (the fresh-id path still serves it) so the freelist's own memory
/// can't grow without limit. An id is only ever pushed here *after* it has
/// been removed from [`HANDLES`], so a recycled id is never live in two
/// registrations at once.
static FREE_HANDLES: Lazy<Mutex<Vec<Handle>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Upper bound on parked idle ids. The visible band is `0x40000` (262 144)
/// ids; capping the freelist well under that keeps its backing `Vec` small
/// while still covering realistic concurrent in-flight counts (tens of
/// thousands of simultaneous requests). Past the cap, a freed id is dropped on
/// the floor — `register_handle` falls back to a fresh id exactly as it did
/// before recycling existed.
const FREE_HANDLES_CAP: usize = 64 * 1024;

/// Quarantine for ids that have just been removed from [`HANDLES`] but are NOT
/// yet eligible for reuse.
///
/// # Why a quarantine, not direct recycling (ABA / use-after-recycle)
///
/// The visible handle is a bare integer with no generation/epoch (the i64 ABI
/// is fixed and published — a generation cannot be packed into the id). A
/// consumer that resolves an object purely by id therefore cannot distinguish
/// "the object I was given" from "a *different* object that happens to occupy
/// the same recycled id now." `perry-ext-http-server` hits this: a request
/// handler can return before `res.end()`, leaving a stale JS-side `res` value
/// (a bare tagged id) outstanding; once that request is finalized its id is
/// freed. If the id were recycled *immediately*, the very next
/// [`register_handle`] (e.g. the next incoming request's response) would
/// re-occupy it, and a late `res.write`/`res.end` from the retired handler
/// would resolve the id to — and mutate — the *new* request's response,
/// bleeding one request's body into another's. Before the freelist existed a
/// freed id stayed dead, so such a stale write was a safe no-op; the freelist
/// removed that safety. The quarantine restores it.
///
/// A freed id is parked here first and only promoted to [`FREE_HANDLES`] by
/// [`drain_quarantined_handles`], which the host event loop calls once per
/// pump tick. One full tick covers the dominant case: a stale `res.*` that the
/// retired handler defers via a microtask or a same-turn continuation runs
/// before the next tick's drain, and while the id sits in quarantine it maps
/// to nothing in [`HANDLES`], so that stale call re-fetches an empty slot and
/// no-ops (exactly the pre-freelist behavior) instead of corrupting a live
/// object.
///
/// # The two quarantine tiers
///
/// The one-tick window only covers ids whose owner is *provably done writing*
/// at free time — an HTTP response that reached `res.end()` (its
/// `writable_ended` is set, so any further `res.write`/`res.end` is a no-op the
/// caller can't ride into a recycled object). For those, one tick is enough:
/// the stale call spends itself against an empty slot on the same turn.
///
/// But a response can be finalized *without* ever ending — the HTTP reaper
/// frees a parked request's handles when its peer disconnects, or when the
/// owning server is force-closed, neither of which sets `writable_ended`. The
/// handler is still suspended on a slow `await`/`fetch` and may resume many
/// ticks later and call `res.write`. A one-tick quarantine would have promoted
/// (and possibly re-minted) that id long before, so the late write would land
/// on a *live* response — silent cross-request body corruption that is NOT a
/// write-after-end (the handler never called `end()`, so nothing rejects it).
///
/// For that case the id goes into [`QUARANTINED_UNTIL`] with a *deadline*
/// instead — the request's grace deadline, which the reaper already tracks
/// (Node's `requestTimeout`, default ~300s). The id is held until that deadline
/// passes, by which point the request is definitively dead: a handler that
/// resumes within grace finds its id still parked (the write no-ops against an
/// empty slot); once the deadline elapses no legitimate resume can write, so
/// the id is safe to recycle. This closes the window for arbitrarily-long-async
/// handlers without a per-handle generation (which the fixed i64 ABI forbids).
///
/// Bound: the deadline-gated quarantine holds at most one id per response per
/// in-flight grace window — the same population the reaper's `IN_FLIGHT` list
/// already bounds — and is capped at [`FREE_HANDLES_CAP`] like every other
/// tier, so it cannot grow without limit.
///
/// Embedders that never call [`drain_quarantined_handles`] simply never
/// recycle ids — they fall back to fresh-id minting, which is the pre-freelist
/// behavior and is safe (it only forgoes the id-reuse optimization).
static QUARANTINED_HANDLES: Lazy<Mutex<Vec<Handle>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Deadline-gated quarantine: ids freed before their owner finished writing
/// (the HTTP reaper's peer-disconnect / force-close paths, where
/// `writable_ended` was never set). Each id is held until `Instant::now()`
/// passes its paired deadline, then promoted to [`FREE_HANDLES`] by
/// [`drain_quarantined_handles`]. See [`QUARANTINED_HANDLES`] for the full
/// rationale (the "two quarantine tiers" section).
static QUARANTINED_UNTIL: Lazy<Mutex<Vec<(Handle, Instant)>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Pop a recycled id, or `None` when the freelist is empty.
fn pop_free_handle() -> Option<Handle> {
    FREE_HANDLES.lock().unwrap_or_else(|p| p.into_inner()).pop()
}

/// Park a no-longer-live id in the quarantine (NOT the freelist — see
/// [`QUARANTINED_HANDLES`]). Caller MUST have already removed `handle` from
/// [`HANDLES`] (see the safety note above). Drops the id when the quarantine
/// is at [`FREE_HANDLES_CAP`], in which case the id is simply never reused
/// (the fresh-id path still serves it), matching the freelist's overflow
/// behavior.
fn recycle_handle(handle: Handle) {
    let mut q = QUARANTINED_HANDLES
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    push_bounded(&mut q, handle, FREE_HANDLES_CAP);
}

/// Park a no-longer-live id in the DEADLINE-GATED quarantine — held until
/// `Instant::now()` passes `deadline`, not merely until the next tick. For ids
/// freed before their owner finished writing (the HTTP reaper's
/// peer-disconnect / force-close paths); see [`QUARANTINED_UNTIL`]. Caller MUST
/// have already removed `handle` from [`HANDLES`]. Bounded exactly like
/// [`recycle_handle`] — past the cap the id is dropped and the fresh-id path
/// serves future registrations.
fn recycle_handle_until(handle: Handle, deadline: Instant) {
    let mut q = QUARANTINED_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    if q.len() < FREE_HANDLES_CAP {
        q.push((handle, deadline));
    }
}

/// Promote quarantined ids to the freelist, making them eligible for reuse by
/// [`register_handle`]. The host event loop calls this once per pump tick, AT
/// THE TOP of the tick — before any of this tick's finalizations quarantine new
/// ids.
///
/// Two tiers are drained (see [`QUARANTINED_HANDLES`]):
///
/// * The one-tick tier ([`QUARANTINED_HANDLES`]) is drained whole. An id freed
///   during tick N is released no earlier than the start of tick N+1, by which
///   point tick N's handler microtasks have drained and any stale handle
///   reference has been spent against an empty slot.
/// * The deadline-gated tier ([`QUARANTINED_UNTIL`]) is drained SELECTIVELY:
///   only entries whose deadline has elapsed are promoted; the rest are
///   retained for a future tick. This holds an id freed before its owner
///   finished writing until the request's grace window closes, so a
///   long-suspended handler that resumes within grace still no-ops against an
///   empty slot.
///
/// Returns the number of ids promoted (for diagnostics/tests).
pub fn drain_quarantined_handles() -> usize {
    let now = Instant::now();
    let one_tick: Vec<Handle> = {
        let mut q = QUARANTINED_HANDLES
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut *q)
    };
    let elapsed: Vec<Handle> = {
        let mut q = QUARANTINED_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
        // Retain entries still within their grace window; harvest the elapsed
        // ones for promotion.
        let mut ready = Vec::new();
        q.retain(|(handle, deadline)| {
            if now >= *deadline {
                ready.push(*handle);
                false
            } else {
                true
            }
        });
        ready
    };
    if one_tick.is_empty() && elapsed.is_empty() {
        return 0;
    }
    let mut free = FREE_HANDLES.lock().unwrap_or_else(|p| p.into_inner());
    let mut promoted = 0;
    for handle in one_tick.into_iter().chain(elapsed) {
        let before = free.len();
        push_bounded(&mut free, handle, FREE_HANDLES_CAP);
        if free.len() != before {
            promoted += 1;
        }
    }
    promoted
}

/// Push `handle` onto `free` unless it is already at `cap`. Factored out so
/// the bounding invariant is unit-testable without touching the process-wide
/// freelist (which concurrent tests churn).
fn push_bounded(free: &mut Vec<Handle>, handle: Handle, cap: usize) {
    if free.len() < cap {
        free.push(handle);
    }
}

static ROOT_SCANNERS: Lazy<Mutex<Vec<fn(&mut dyn FnMut(f64))>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
static MUTABLE_ROOT_SCANNERS: Lazy<Mutex<Vec<NamedGcMutableRootScanner>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

thread_local! {
    static ROOT_SCANNER_TRAMPOLINE_REGISTERED: Cell<bool> = const { Cell::new(false) };
    static MUTABLE_ROOT_SCANNER_TRAMPOLINES_REGISTERED: RefCell<Vec<usize>> = const {
        RefCell::new(Vec::new())
    };
}

type PerryFfiRootMarker = extern "C" fn(value: f64, ctx: *mut c_void);
type PerryFfiRootScanner = extern "C" fn(mark: PerryFfiRootMarker, ctx: *mut c_void);
type PerryFfiMutableRootVisitor =
    extern "C" fn(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool;
type PerryFfiNamedMutableRootScanner =
    extern "C" fn(scanner_id: usize, visit: PerryFfiMutableRootVisitor, ctx: *mut c_void);

#[derive(Clone, Copy)]
struct NamedGcMutableRootScanner {
    scanner: GcMutableRootScanner,
}

const FFI_ROOT_SLOT_I64: u32 = 1;
const FFI_ROOT_SLOT_USIZE: u32 = 2;
const FFI_ROOT_SLOT_RAW_MUT_PTR: u32 = 3;
const FFI_ROOT_SLOT_NANBOX_F64: u32 = 4;
const FFI_ROOT_SLOT_NANBOX_U64: u32 = 5;

extern "C" {
    fn perry_ffi_gc_register_root_scanner(scanner: PerryFfiRootScanner);
    fn perry_ffi_gc_register_mutable_root_scanner_named(
        source_ptr: *const u8,
        source_len: usize,
        scanner_id: usize,
        scanner: PerryFfiNamedMutableRootScanner,
    );
}

// perry-runtime hook: register a probe the runtime's generic method dispatcher
// consults to tell a `register_handle` id apart from a Node timer id (both
// occupy the pointer-tagged small-integer band). Defined in perry-runtime and
// resolved at the final link of any real Perry binary.
//
// The declaration is gated OUT of perry-ffi's own unit-test binary when
// `runtime-link` is off, where a no-op stub stands in instead (see below) —
// otherwise the always-present `extern` item and the stub would clash (E0428).
#[cfg(not(all(test, not(feature = "runtime-link"))))]
extern "C" {
    fn js_register_ffi_handle_exists_probe(probe: extern "C" fn(handle: i64) -> bool);
}

// perry-ffi's own unit-test binary does not link perry-runtime: `runtime-link`
// is off by default and CI runs `cargo test -p perry-ffi` per-package in
// isolation (no `--workspace` feature unification, see `.github/workflows/
// test.yml`). The handle-registry tests below exercise `register_handle`,
// which calls `js_register_ffi_handle_exists_probe` to wire up the runtime's
// handle-vs-timer disambiguation probe. Give that test binary a no-op
// definition so it links and the registry tests keep running. Gated on
// `not(feature = "runtime-link")` so it never collides with perry-runtime's
// real definition — which is present whenever runtime-link is on, or at a
// wrapper's final link against libperry_runtime.a, neither of which is a
// perry-ffi `test` build.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn js_register_ffi_handle_exists_probe(
    _probe: extern "C" fn(handle: i64) -> bool,
) {
}

/// Probe handed to perry-runtime: is `handle` a live entry in this registry?
/// Used to disambiguate a `POINTER_TAG | id` value that names both a live
/// handle and a live timer (e.g. HTTP/2 server handle 1 vs `setTimeout` id 1),
/// so the runtime routes `server.close()` to the handle rather than swallowing
/// it as `clearTimeout`. See `class_handles::ffi_handle_exists`.
extern "C" fn ffi_handle_exists_probe(handle: Handle) -> bool {
    HANDLES.contains_key(&handle)
}

/// Register [`ffi_handle_exists_probe`] with perry-runtime exactly once, the
/// first time any handle is created. Done lazily (rather than at an init entry
/// point perry-ffi doesn't own) so it is wired up before any handle value can
/// reach the runtime's generic dispatcher.
fn ensure_handle_exists_probe_registered() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_ffi_handle_exists_probe(ffi_handle_exists_probe);
    });
}

/// Function pointer type for native wrappers that expose mutable GC root slots.
///
/// Register one with [`gc_register_mutable_root_scanner`]. The scanner should
/// walk wrapper-owned storage and call the relevant [`GcRootVisitor`] method for
/// each slot that may hold a Perry heap pointer.
pub type GcMutableRootScanner = for<'a> fn(&mut GcRootVisitor<'a>);

/// Visitor passed to mutable GC root scanners.
///
/// The visitor does not expose runtime internals. Each method forwards the
/// address of a wrapper-owned slot to Perry's runtime so the GC can mark the
/// current referent and, during copied-minor evacuation, rewrite the slot to a
/// forwarded address.
pub struct GcRootVisitor<'a> {
    visit: PerryFfiMutableRootVisitor,
    ctx: *mut c_void,
    _marker: PhantomData<&'a mut ()>,
}

impl<'a> GcRootVisitor<'a> {
    fn new(visit: PerryFfiMutableRootVisitor, ctx: *mut c_void) -> Self {
        Self {
            visit,
            ctx,
            _marker: PhantomData,
        }
    }

    /// Visit a raw heap pointer stored in an `i64` slot.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_i64_slot(&mut self, slot: &mut i64) -> bool {
        (self.visit)(FFI_ROOT_SLOT_I64, slot as *mut i64 as *mut c_void, self.ctx)
    }

    /// Visit a raw heap pointer stored in a `usize` slot.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_usize_slot(&mut self, slot: &mut usize) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_USIZE,
            slot as *mut usize as *mut c_void,
            self.ctx,
        )
    }

    /// Visit a raw mutable heap pointer slot.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_RAW_MUT_PTR,
            slot as *mut *mut T as *mut c_void,
            self.ctx,
        )
    }

    /// Visit a NaN-boxed JS value stored as an `f64`.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_NANBOX_F64,
            slot as *mut f64 as *mut c_void,
            self.ctx,
        )
    }

    /// Visit a NaN-boxed JS value stored as raw `u64` bits.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_NANBOX_U64,
            slot as *mut u64 as *mut c_void,
            self.ctx,
        )
    }
}

/// Register `value` under a fresh handle and return the handle.
///
/// `T` must be `Send + Sync + 'static` — the registry is shared
/// across threads (tokio workers may resolve promises that touch
/// handle data while the main thread is also touching it).
pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    ensure_handle_exists_probe_registered();
    // Reuse a reclaimed id when one is parked, else mint a fresh one. A
    // recycled id was removed from `HANDLES` before being parked, so inserting
    // under it here cannot collide with a live registration. Unlike
    // `reserve_handle_id`, exhaustion still aborts here: `register_handle` must
    // return a live key to insert under, and there is no valid id left to hand
    // out. A leaking `register_handle` workload is a bug (its ids are recycled
    // by `drop_handle`), so exhaustion here means the concurrent live-handle
    // count genuinely exceeded the band.
    let handle = pop_free_handle()
        .or_else(next_fresh_handle_id)
        .unwrap_or_else(|| {
            panic!("perry-ffi handle id range exhausted before reserved Web handle bands")
        });
    HANDLES.insert(handle, Box::new(value));
    handle
}

/// Reserve a globally-unique handle id WITHOUT storing a value in the FFI
/// registry. For a subsystem that keeps its own object map (perry-ext-net's
/// socket registry) but must not alias another library's ids: every ext lib
/// that mints ids privately from 1 collides with the others in the shared
/// `[1, 0x40000)` band, and the composite handle-method dispatch then routes a
/// call to whichever extension *thinks* it owns that number. That is how
/// `socket.on('data', …)` on ext-net socket #1 got claimed by ext-http-server
/// (whose server was also #1) and the mysql2 handshake hung: the listener
/// registered on the HTTP server and the socket's bytes reached nobody.
///
/// Return [`INVALID_HANDLE`] when the visible id band is exhausted rather than
/// aborting the process (#6441). A reserved id is not recycled until the owning
/// subsystem calls [`free_handle_id`]; a subsystem that never frees (or frees
/// more slowly than it reserves) will eventually drain the band, and a
/// long-running server must degrade that to a recoverable, JS-visible error
/// (e.g. an `EMFILE`-style throw at the socket-alloc site) instead of a crash.
/// The `0` sentinel is safe to route on: callers must NOT register an object
/// under it — `0` is the "no handle" value — so the guard turns exhaustion into
/// a caught error at the boundary, never a phantom id-0 entry.
pub fn reserve_handle_id() -> Handle {
    pop_free_handle()
        .or_else(next_fresh_handle_id)
        .unwrap_or(INVALID_HANDLE)
}

/// Free a handle id previously minted by [`reserve_handle_id`], returning it to
/// circulation through the same quarantine [`drop_handle`] uses.
///
/// [`reserve_handle_id`] hands a subsystem that keeps its OWN object map (e.g.
/// perry-ext-net's socket registry) a globally-unique id without storing
/// anything in [`HANDLES`] — so there is nothing to remove here; this recycles
/// only the *id*. The caller MUST have already dropped the id from its own map
/// and must guarantee no further dispatch will resolve it, exactly the contract
/// [`drop_handle`] places on [`register_handle`] ids.
///
/// Like every freed id it is parked in the one-tick quarantine
/// ([`QUARANTINED_HANDLES`]) and only promoted to the freelist by
/// [`drain_quarantined_handles`], so a stale bare reference dispatched before
/// the next tick spends against an empty slot instead of aliasing a freshly
/// reserved id — the ABA / use-after-recycle class #6407 fixes. See
/// [`QUARANTINED_HANDLES`]. Passing [`INVALID_HANDLE`] is a no-op, so a caller
/// can free the result of a possibly-exhausted [`reserve_handle_id`]
/// unconditionally.
///
/// This is the primitive both candidate free-when-unreachable fixes for the
/// reserved-id leak build on (a GC-finalized socket object, or a handle-band
/// liveness sweep — #6441). It performs no reachability analysis itself: a
/// stale JS reference to a `net.Socket` can outlive its `'close'`, so freeing
/// on `'close'` alone is unsafe and left to that follow-up.
pub fn free_handle_id(id: Handle) {
    if id == INVALID_HANDLE {
        return;
    }
    recycle_handle(id);
}

/// Deadline-gated twin of [`free_handle_id`]: holds the reserved id in the
/// [`QUARANTINED_UNTIL`] tier until `Instant::now()` passes `deadline`, rather
/// than merely until the next tick. For a subsystem that frees an id while a
/// stale holder may still resume and dispatch on it within a known grace window
/// (mirrors [`drop_handle_until`]). Passing [`INVALID_HANDLE`] is a no-op.
pub fn free_handle_id_until(id: Handle, deadline: Instant) {
    if id == INVALID_HANDLE {
        return;
    }
    recycle_handle_until(id, deadline);
}

/// Mint a never-before-used id, or `None` once the visible band is exhausted.
///
/// Returns `None` rather than panicking so callers choose their own exhaustion
/// policy: [`reserve_handle_id`] degrades to a recoverable [`INVALID_HANDLE`]
/// (#6441), while [`register_handle`] — which has no valid key to insert under
/// — still aborts. The atomic keeps advancing past [`FFI_HANDLE_ID_END`] on
/// each post-exhaustion call; that is harmless (every such call maps to `None`)
/// and the `i64` counter cannot realistically wrap.
fn next_fresh_handle_id() -> Option<Handle> {
    fresh_id_or_exhausted(NEXT_HANDLE.fetch_add(1, Ordering::SeqCst))
}

/// Classify a raw counter value as a usable fresh id or band-exhausted.
/// Factored out so the exhaustion boundary is unit-testable without advancing
/// the process-wide [`NEXT_HANDLE`] past [`FFI_HANDLE_ID_END`] (which would
/// break every other test in this binary).
fn fresh_id_or_exhausted(raw: Handle) -> Option<Handle> {
    if raw >= FFI_HANDLE_ID_END {
        None
    } else {
        Some(raw)
    }
}

/// Look up a handle and run `f` against the borrowed value.
/// Recommended over [`get_handle`] — the borrow is scoped, so
/// concurrent [`take_handle`] / [`drop_handle`] can't dangle it.
pub fn with_handle<T: 'static + Send + Sync, R, F: FnOnce(&T) -> R>(
    handle: Handle,
    f: F,
) -> Option<R> {
    HANDLES
        .get(&handle)
        .and_then(|entry| entry.value().downcast_ref::<T>().map(f))
}

/// Look up a handle and run `f` against a mutable borrow. Same
/// caveats as [`with_handle`].
pub fn with_handle_mut<T: 'static + Send + Sync, R, F: FnOnce(&mut T) -> R>(
    handle: Handle,
    f: F,
) -> Option<R> {
    HANDLES
        .get_mut(&handle)
        .and_then(|mut entry| entry.value_mut().downcast_mut::<T>().map(f))
}

/// Borrow the handle's value as `&'static T`. The reference is
/// only stable as long as the handle is in the registry — drop
/// or take it while a borrow is outstanding and you've got a
/// dangle. Prefer [`with_handle`] when possible.
pub fn get_handle<T: 'static + Send + Sync>(handle: Handle) -> Option<&'static T> {
    // SAFETY: DashMap entries are heap-allocated `Box<dyn Any>`s
    // whose contents don't move while in the map. The returned
    // reference points into that Box; it stays valid until the
    // entry is removed (which is the caller's responsibility to
    // sequence correctly).
    HANDLES.get(&handle).and_then(|entry| {
        let ptr = entry.value().downcast_ref::<T>()? as *const T;
        Some(unsafe { &*ptr })
    })
}

/// Mutable counterpart to [`get_handle`].
pub fn get_handle_mut<T: 'static + Send + Sync>(handle: Handle) -> Option<&'static mut T> {
    HANDLES.get_mut(&handle).and_then(|mut entry| {
        let ptr = entry.value_mut().downcast_mut::<T>()? as *mut T;
        Some(unsafe { &mut *ptr })
    })
}

/// Remove the handle from the registry and return its value if
/// the type matches. After this, the handle is no longer valid.
pub fn take_handle<T: 'static + Send + Sync>(handle: Handle) -> Option<T> {
    let removed = HANDLES.remove(&handle);
    if removed.is_some() {
        // Removed from the registry — the id is dead and safe to recycle.
        recycle_handle(handle);
    }
    removed
        .and_then(|(_, boxed)| boxed.downcast::<T>().ok())
        .map(|b| *b)
}

/// Remove a handle and drop its value. Returns `true` if the
/// handle existed.
pub fn drop_handle(handle: Handle) -> bool {
    if HANDLES.remove(&handle).is_some() {
        // Removed from the registry — the id is dead and safe to recycle.
        recycle_handle(handle);
        true
    } else {
        false
    }
}

/// Remove a handle and drop its value, but defer recycling its id until
/// `deadline` rather than the next tick. Returns `true` if the handle existed.
///
/// For ids freed before their owner finished writing — the HTTP reaper frees a
/// parked response on peer-disconnect / server-force-close without ever setting
/// `writable_ended`, so a handler suspended on a slow `await` can resume many
/// ticks later and write through the bare id. Holding the id until the
/// request's grace deadline keeps it parked (a no-op slot) across that whole
/// window. See [`QUARANTINED_UNTIL`].
pub fn drop_handle_until(handle: Handle, deadline: Instant) -> bool {
    if HANDLES.remove(&handle).is_some() {
        recycle_handle_until(handle, deadline);
        true
    } else {
        false
    }
}

/// True if the handle currently maps to a registered object.
pub fn handle_exists(handle: Handle) -> bool {
    HANDLES.contains_key(&handle)
}

/// Visit every registered handle whose stored type matches `T`,
/// invoking `f(&value)` for each.
///
/// Used by GC root scanners that need to keep user closures alive
/// — e.g. `EventEmitter` listeners stored inside an
/// `EventEmitterHandle`. Without this, a malloc-triggered GC
/// between `.on(...)` and `.emit(...)` would sweep the closure
/// (issue #35 pattern in perry-stdlib).
///
/// Pair with [`gc_register_root_scanner`] to wire the scanner into
/// perry's GC.
pub fn iter_handles_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&T),
{
    for entry in HANDLES.iter() {
        if let Some(v) = entry.value().downcast_ref::<T>() {
            f(v);
        }
    }
}

/// Visit every registered handle whose stored type matches `T`,
/// invoking `f(&mut value)` for each.
///
/// This is the mutable counterpart to [`iter_handles_of`]. It is intended for
/// mutable GC scanners that need to hand owned fields to
/// [`GcRootVisitor`], allowing copied-minor GC to rewrite those fields after
/// evacuation.
///
/// The callback runs while the registry entry is borrowed. Do not remove or
/// re-register handles from inside `f`.
pub fn iter_handles_of_mut<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&mut T),
{
    for mut entry in HANDLES.iter_mut() {
        if let Some(v) = entry.value_mut().downcast_mut::<T>() {
            f(v);
        }
    }
}

/// Visit every registered handle id whose stored type matches `T`,
/// invoking `f(handle_id)` for each.
///
/// Unlike [`iter_handles_of`], this hands the caller the integer
/// handle id rather than a borrow. Useful when the callback needs
/// to perform operations that can't be expressed against `&T`
/// (e.g. methods on `T` that need `&mut T`, or sites that must
/// drop / re-register the handle).
///
/// Caller is responsible for not removing the handle while the
/// iteration is in progress — the underlying `DashMap` iterator
/// holds shards but doesn't pin entire entries. The recommended
/// pattern is to snapshot ids into a `Vec` first, then act on each
/// id outside the iteration.
///
/// perry-ext-http-server's main-thread pump walks every registered
/// HttpServer / HttpsServer / Http2SecureServer handle each tick to
/// drain pending requests.
pub fn iter_handle_ids_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(Handle),
{
    for entry in HANDLES.iter() {
        if entry.value().downcast_ref::<T>().is_some() {
            f(*entry.key());
        }
    }
}

/// Register a legacy copy-only GC root scanner with Perry's runtime.
///
/// The scanner is called during every GC mark phase; it should call its `mark`
/// callback with each NaN-boxed JsValue that should be kept alive. This API
/// exposes copied values only. The runtime cannot rewrite wrapper-owned storage
/// discovered through this API, so registering any scanner here makes
/// low-pause copied-minor GC ineligible. It remains supported for legacy
/// fallback/full collection only. Prefer [`gc_register_mutable_root_scanner`]
/// for new scanners and for low-pause compatibility.
///
/// This registers through `perry_ffi_gc_register_root_scanner`, the stable
/// C ABI bridge exported by the runtime.
/// Wrapper authors typically combine this with [`iter_handles_of`]:
///
/// ```ignore
/// use perry_ffi::{gc_register_root_scanner, iter_handles_of, nanbox_string_bits};
///
/// fn scan_my_roots(mark: &mut dyn FnMut(f64)) {
///     iter_handles_of::<MyHandle, _>(|h| {
///         for closure_ptr in &h.callbacks {
///             // POINTER_TAG over the closure pointer.
///             let nanboxed = f64::from_bits(0x7FFD_0000_0000_0000 | (*closure_ptr as u64 & 0x0000_FFFF_FFFF_FFFF));
///             mark(nanboxed);
///         }
///     });
/// }
///
/// // Register once on first wrapper-method invocation.
/// gc_register_root_scanner(scan_my_roots);
/// ```
#[deprecated(
    note = "copy-only GC root scanners force fallback/full collection; use gc_register_mutable_root_scanner for low-pause GC"
)]
pub fn gc_register_root_scanner(scanner: fn(&mut dyn FnMut(f64))) {
    {
        let mut scanners = ROOT_SCANNERS
            .lock()
            .expect("perry-ffi root scanner registry poisoned");
        if !scanners
            .iter()
            .any(|registered| *registered as usize == scanner as usize)
        {
            scanners.push(scanner);
        }
    }
    ROOT_SCANNER_TRAMPOLINE_REGISTERED.with(|registered| {
        if !registered.get() {
            unsafe {
                perry_ffi_gc_register_root_scanner(scan_registered_roots);
            }
            registered.set(true);
        }
    });
}

/// Register an anonymous mutable GC root scanner with Perry's runtime.
///
/// This mutable scanner family is preferred for native wrappers that keep Perry
/// heap pointers in handle-owned Rust fields. Unlike
/// [`gc_register_root_scanner`], it exposes the actual slots, so copied-minor GC
/// can rewrite them after moving young objects. Prefer
/// [`gc_register_mutable_root_scanner_named`] for in-tree or package-owned
/// scanners so GC diagnostics can attribute roots to the wrapper that owns them.
///
/// Wrapper authors typically combine this with [`iter_handles_of_mut`]:
///
/// ```ignore
/// use perry_ffi::{gc_register_mutable_root_scanner_named, iter_handles_of_mut, GcRootVisitor};
///
/// fn scan_my_roots(visitor: &mut GcRootVisitor<'_>) {
///     iter_handles_of_mut::<MyHandle, _>(|h| {
///         visitor.visit_i64_slot(&mut h.callback);
///     });
/// }
///
/// gc_register_mutable_root_scanner_named("my-wrapper", scan_my_roots);
/// ```
pub fn gc_register_mutable_root_scanner(scanner: GcMutableRootScanner) {
    gc_register_mutable_root_scanner_named("ffi:anonymous", scanner);
}

/// Register a source-attributed mutable GC root scanner with Perry's runtime.
///
/// `source` should be a short, stable package or subsystem name such as
/// `perry-ext-http-server`. It is copied into runtime GC diagnostics and
/// verifier errors so native roots do not collapse behind `perry-ffi`'s shared
/// dispatcher.
pub fn gc_register_mutable_root_scanner_named(source: &'static str, scanner: GcMutableRootScanner) {
    assert_valid_root_source(source);
    let scanner_id = {
        let mut scanners = MUTABLE_ROOT_SCANNERS
            .lock()
            .expect("perry-ffi mutable root scanner registry poisoned");
        if let Some((scanner_id, _)) = scanners
            .iter()
            .enumerate()
            .find(|(_, registered)| registered.scanner as usize == scanner as usize)
        {
            scanner_id
        } else {
            let scanner_id = scanners.len();
            scanners.push(NamedGcMutableRootScanner { scanner });
            scanner_id
        }
    };
    MUTABLE_ROOT_SCANNER_TRAMPOLINES_REGISTERED.with(|registered| {
        let mut registered = registered.borrow_mut();
        if registered.contains(&scanner_id) {
            return;
        }
        unsafe {
            perry_ffi_gc_register_mutable_root_scanner_named(
                source.as_ptr(),
                source.len(),
                scanner_id,
                scan_registered_mutable_root_by_id,
            );
        }
        registered.push(scanner_id);
    });
}

fn assert_valid_root_source(source: &'static str) {
    assert!(
        !source.is_empty() && source.len() <= 128 && source.chars().all(|c| !c.is_control()),
        "perry-ffi GC root scanner source must be non-empty, <= 128 bytes, and printable"
    );
}

extern "C" fn scan_registered_roots(mark: PerryFfiRootMarker, ctx: *mut c_void) {
    let scanners = ROOT_SCANNERS
        .lock()
        .expect("perry-ffi root scanner registry poisoned")
        .clone();
    for scanner in scanners {
        scanner(&mut |value| mark(value, ctx));
    }
}

extern "C" fn scan_registered_mutable_root_by_id(
    scanner_id: usize,
    visit: PerryFfiMutableRootVisitor,
    ctx: *mut c_void,
) {
    let scanner = MUTABLE_ROOT_SCANNERS
        .lock()
        .expect("perry-ffi mutable root scanner registry poisoned")
        .get(scanner_id)
        .copied();
    let Some(scanner) = scanner else {
        return;
    };
    let mut visitor = GcRootVisitor::new(visit, ctx);
    (scanner.scanner)(&mut visitor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn round_trip_simple_value() {
        let h = register_handle(42_i64);
        assert_ne!(h, INVALID_HANDLE);
        assert!(h < FFI_HANDLE_ID_END);
        let v = with_handle::<i64, _, _>(h, |v| *v).expect("present");
        assert_eq!(v, 42);
        assert!(drop_handle(h));
        assert!(!handle_exists(h));
    }

    #[test]
    fn mutable_access_persists() {
        struct Counter(u32);
        let h = register_handle(Counter(0));
        with_handle_mut::<Counter, _, _>(h, |c| c.0 += 1).expect("present");
        with_handle_mut::<Counter, _, _>(h, |c| c.0 += 1).expect("present");
        let n = with_handle::<Counter, _, _>(h, |c| c.0).expect("present");
        assert_eq!(n, 2);
        drop_handle(h);
    }

    #[test]
    fn iter_handles_of_mut_updates_matching_values() {
        struct Counter(u32);
        let a = register_handle(Counter(1));
        let b = register_handle(Counter(10));
        let other = register_handle("not a counter".to_string());

        iter_handles_of_mut::<Counter, _>(|c| c.0 += 1);

        let mut values = Vec::new();
        iter_handles_of::<Counter, _>(|c| values.push(c.0));
        values.sort_unstable();
        assert_eq!(values, vec![2, 11]);

        drop_handle(a);
        drop_handle(b);
        drop_handle(other);
    }

    #[test]
    fn type_mismatch_returns_none() {
        let h = register_handle(42_i64);
        // Same handle, wrong type — no value comes back.
        let r = with_handle::<String, _, _>(h, |s| s.clone());
        assert!(r.is_none());
        drop_handle(h);
    }

    #[test]
    fn handles_are_unique() {
        let a = register_handle(1_i32);
        let b = register_handle(2_i32);
        assert_ne!(a, b);
        drop_handle(a);
        drop_handle(b);
    }

    // ----------------------------------------------------------------
    // Id-recycling freelist.
    //
    // The registry is process-wide and the default test harness runs
    // these in parallel, so the reuse-sensitive tests below serialize on
    // `RECYCLE_TEST_LOCK` and assert the *recycling contract* (a freed id
    // is reused, fresh-id consumption stays bounded) rather than a fixed id
    // value — robust to other tests churning the shared registry, but still
    // failing hard against a no-reclaim `drop_handle` (the freed id never
    // lands on the freelist, so it is never reused and id consumption is
    // unbounded). The bounding invariant is tested in isolation against a
    // local freelist via `push_bounded`.
    // ----------------------------------------------------------------

    static RECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Register `value`, reporting whether `register_handle` REUSED a parked
    /// id rather than minting a fresh one (the recycling contract). A pop
    /// leaves [`NEXT_HANDLE`] untouched; a fresh mint advances it, which a
    /// no-reclaim build would do on every register. Returns `(handle, reused)`.
    fn register_observing_reuse<T: 'static + Send + Sync>(value: T) -> (Handle, bool) {
        let before = NEXT_HANDLE.load(Ordering::SeqCst);
        let handle = register_handle(value);
        let reused = NEXT_HANDLE.load(Ordering::SeqCst) == before;
        (handle, reused)
    }

    /// Free `id`, then register `value` and keep retrying until we observe a
    /// REUSE (the new registration drew a parked id instead of minting fresh),
    /// returning the reused handle. Each non-reusing attempt is dropped so it
    /// re-parks an id for the next try.
    ///
    /// A freed id now sits in QUARANTINE until [`drain_quarantined_handles`]
    /// promotes it to the freelist (the ABA fix), so each attempt drains first
    /// — exactly what the host event loop does once per tick. A no-reclaim
    /// `drop_handle` would quarantine NOTHING, so the drain promotes nothing
    /// and reuse never happens: the contract still fails hard against a build
    /// that doesn't reclaim.
    ///
    /// The bounded retry is what makes the reuse assertion both robust and
    /// meaningful on the *process-wide* freelist. The non-serialized registry
    /// tests (`round_trip_simple_value` etc.) run in parallel and can pop the
    /// very id we just freed in the window before our register — so a single
    /// observation can legitimately miss reuse. But recycling guarantees reuse
    /// happens *eventually* (we keep re-parking + re-draining ids), whereas a
    /// no-reclaim `drop_handle` parks NOTHING, so every attempt mints fresh and
    /// the loop exhausts — turning "reuse never happens" into a hard failure.
    fn drop_then_register_reusing<T: 'static + Send + Sync>(id: Handle, value: T) -> Handle
    where
        T: Clone,
    {
        assert!(drop_handle(id), "the id to recycle must have been live");
        for _ in 0..10_000 {
            // Promote prior-tick quarantined ids to the freelist (the host
            // pump's per-tick drain), then observe whether register reuses one.
            drain_quarantined_handles();
            let (handle, reused) = register_observing_reuse(value.clone());
            if reused {
                return handle;
            }
            // A parallel test popped our parked id first and we minted fresh;
            // drop it (re-quarantining an id) and try again.
            assert!(drop_handle(handle));
        }
        panic!(
            "register_handle never reused a freed id across 10000 attempts — \
             ids are not being recycled (a no-reclaim drop_handle would do this)"
        );
    }

    #[test]
    fn register_drop_register_reuses_a_freed_id() {
        // End-to-end: a register/drop/register cycle reuses the freed id rather
        // than minting a second fresh one. A no-reclaim `drop_handle` parks
        // nothing, so `register_handle` would always mint fresh and
        // `drop_then_register_reusing` would never observe reuse — a hard fail.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let h1 = register_handle(7_i64);
        let h2 = drop_then_register_reusing(h1, 9_i64);
        assert_eq!(with_handle::<i64, _, _>(h2, |v| *v), Some(9));
        assert!(drop_handle(h2));
    }

    #[test]
    fn reused_id_carries_no_stale_state() {
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        // Register a String, drop it, then register a different type under the
        // RECYCLED id. The recycled id must resolve to the NEW value with the
        // NEW type — never the prior String (cross-request bleed). The String
        // is no longer reachable because its entry was removed at drop.
        //
        // `drop_then_register_reusing` guarantees the second register actually
        // reused the freed id, so this test exercises the recycle path — it
        // would never silently pass on a no-reclaim registry where the
        // stale-state question is moot.
        let first = register_handle("stale".to_string());
        let second = drop_then_register_reusing(first, 1234_i64);
        assert!(
            with_handle::<String, _, _>(second, |s| s.clone()).is_none(),
            "recycled id must not expose the prior handle's value or type"
        );
        assert_eq!(with_handle::<i64, _, _>(second, |v| *v), Some(1234));
        drop_handle(second);
    }

    #[test]
    fn freed_id_is_not_reusable_until_drained_no_cross_request_bleed() {
        // The ABA / use-after-recycle regression. Models the HTTP cross-request
        // body bleed in handle-registry terms (the layer where the hazard lives,
        // independent of perry-ext-http-server's reaper plumbing): a response R1
        // is registered under id `h`; the handler returns before `res.end()` and
        // the request is later finalized, freeing `h`; within the SAME tick a new
        // request registers its response R2; then a stale `res.write`/`res.end`
        // from the retired handler fires, carrying only the bare id `h`, and
        // mutates whatever `h` resolves to.
        //
        // The hazard: if the free made `h` immediately reusable, the new
        // registration would re-occupy `h` with R2 and the stale write would
        // mutate R2 — one request's body bleeding into another's. The quarantine
        // makes a freed id reusable only after `drain_quarantined_handles` (the
        // host pump's per-tick promotion, which runs only AFTER the finalizing
        // handler's microtasks — and thus any stale writes — have drained). So
        // within the tick `h` resolves to NOTHING, and the stale write no-ops
        // against the empty slot exactly as it did before the freelist existed.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        // A `ServerResponse`-shaped stand-in: the field a bleed would corrupt.
        #[derive(Clone)]
        struct Response {
            buffered_body: Vec<u8>,
        }

        // Register R1, capturing its id (the stale `res` holds this).
        let h = register_handle(Response {
            buffered_body: b"request-1-body".to_vec(),
        });

        // Finalize R1 — it is removed from the registry and its id freed
        // (quarantined, NOT yet on the freelist).
        assert!(drop_handle(h));

        // A new request registers R2 in the SAME tick (no drain yet). Because
        // `h` is quarantined, register_handle CANNOT hand `h` back — R2 gets a
        // different id. This is the load-bearing assertion: revert the
        // quarantine (recycle straight to the freelist) and `h2 == h`, so the
        // stale write below lands on R2 and the final assertion fails.
        let h2 = register_handle(Response {
            buffered_body: b"request-2-body".to_vec(),
        });
        assert_ne!(
            h2, h,
            "a just-freed id must not be reusable within the same tick — \
             reusing it lets a stale handle holder corrupt the new request"
        );

        // The stale write fires against `h`. With the quarantine, `h` resolves
        // to nothing — a safe no-op (mirrors how the HTTP FFI's
        // `get_handle::<ServerResponse>(h)` returns None → early-return).
        let appended = with_handle_mut::<Response, _, _>(h, |r| {
            r.buffered_body.extend_from_slice(b"-STALE-WRITE");
        });
        assert!(
            appended.is_none(),
            "a stale write to a finalized (quarantined) id must hit an empty \
             slot, not a live object"
        );

        // R2's body is pristine — the stale write did NOT bleed into it.
        let r2_body = with_handle::<Response, _, _>(h2, |r| r.buffered_body.clone())
            .expect("R2 is still live");
        assert_eq!(
            r2_body, b"request-2-body",
            "the second request's response body must be untouched by the \
             stale write to the finalized first request's id"
        );

        // After a tick boundary (drain), `h` is safely reusable again — and a
        // fresh registration under it carries no stale R1 state.
        drain_quarantined_handles();
        let h3 = drop_then_register_reusing(
            h2,
            Response {
                buffered_body: b"request-3-body".to_vec(),
            },
        );
        let r3_body =
            with_handle::<Response, _, _>(h3, |r| r.buffered_body.clone()).expect("R3 is live");
        assert_eq!(r3_body, b"request-3-body");
        drop_handle(h3);
    }

    #[test]
    fn deadline_gated_id_survives_ticks_until_grace_then_recycles() {
        // The LONG-ASYNC use-after-recycle regression — the residual window the
        // one-tick quarantine does NOT cover. Models the HTTP reaper finalizing a
        // response that never ended (`writable_ended` unset — peer disconnect or
        // server force-close) while its handler is still suspended on a slow
        // upstream `await`: response R1 is registered under id `h`; the reaper
        // finalizes the request WITHOUT the handler ending the response
        // (`drop_handle_until(h, grace_deadline)`) because the peer is gone, so
        // `h` enters the deadline-gated quarantine; many pump ticks pass (each
        // draining); then the handler resumes — still within the grace window —
        // and its stale `res.write` fires carrying only the bare id `h`.
        //
        // With a one-tick quarantine, the first drain would have promoted `h`
        // (and a new request could have re-minted it), so the resumed write would
        // mutate a LIVE response — cross-request body corruption that is NOT a
        // write-after-end (the handler never called `end()`). The deadline-gated
        // quarantine keeps `h` parked across EVERY tick until the grace deadline
        // passes, so the stale write no-ops against an empty slot for the whole
        // window — then `h` recycles cleanly once the request is definitively
        // dead.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        #[derive(Clone)]
        struct Response {
            buffered_body: Vec<u8>,
        }

        // Register R1 (the suspended handler holds this bare id).
        let h = register_handle(Response {
            buffered_body: b"request-1-body".to_vec(),
        });

        // The reaper finalizes WITHOUT end — defer recycling until a grace
        // deadline well in the future (stand-in for `requestTimeout`).
        let grace = Instant::now() + Duration::from_secs(3600);
        assert!(drop_handle_until(h, grace));

        // Several pump ticks pass. Each drain must leave `h` parked because its
        // deadline has not elapsed — so a new request can never be handed `h`.
        // (Fail-before: a one-tick quarantine promotes `h` on the first drain,
        // making the assertion below fail.)
        for _ in 0..5 {
            drain_quarantined_handles();
            let probe = register_handle(0_i64);
            assert_ne!(
                probe, h,
                "a deadline-gated id must not be recycled before its grace \
                 deadline — reusing it lets a long-suspended handler corrupt a \
                 new request's response"
            );
            drop_handle(probe);
        }

        // The stale write fires against `h`. While `h` is parked it maps to
        // nothing, so the write no-ops against an empty slot — no bleed.
        let appended = with_handle_mut::<Response, _, _>(h, |r| {
            r.buffered_body.extend_from_slice(b"-STALE-WRITE");
        });
        assert!(
            appended.is_none(),
            "a stale write to a deadline-gated id must hit an empty slot, not a \
             live object"
        );

        // Drain the far-future entry away so it can't leak into later tests
        // (its deadline hasn't elapsed, so this is a no-op for `h` — but it
        // clears the one-tick tier). Then prove the elapsed-deadline path DOES
        // recycle: a separate id parked with an already-past deadline is
        // promoted by the very next drain and reused cleanly, carrying no stale
        // state. This is the post-grace tick, modeled deterministically (no
        // sleep) with an id we fully control.
        drain_quarantined_handles();
        let dead = register_handle(Response {
            buffered_body: b"to-be-finalized".to_vec(),
        });
        assert!(drop_handle_until(
            dead,
            Instant::now() - Duration::from_secs(1)
        ));
        // The elapsed-deadline entry is eligible immediately; retry to absorb
        // parallel tests racing the shared freelist (same pattern as
        // `drop_then_register_reusing`).
        let mut recycled = None;
        for _ in 0..10_000 {
            drain_quarantined_handles();
            let candidate = register_handle(Response {
                buffered_body: b"request-2-body".to_vec(),
            });
            if candidate == dead {
                recycled = Some(candidate);
                break;
            }
            drop_handle(candidate);
        }
        let h2 = recycled.expect("an elapsed-deadline id must recycle once its grace passes");
        let body = with_handle::<Response, _, _>(h2, |r| r.buffered_body.clone())
            .expect("the recycled id resolves to the new response");
        assert_eq!(
            body, b"request-2-body",
            "the recycled id carries the NEW response, never the finalized one"
        );
        drop_handle(h2);

        // `h`'s far-future deadline entry stays parked — that is the point of
        // the test. The freelist is bounded and the process-wide registry
        // tolerates a held id, so this leaks nothing that matters across tests.
    }

    #[test]
    fn live_handles_never_share_an_id() {
        // Serialized: this test drains the quarantine, which mutates the shared
        // recycle state the other recycle-sensitive tests depend on.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        // Recycling must never hand the same id to two live handles. Hold a
        // batch live (none dropped) and assert every id is distinct, then
        // free them and re-allocate the same count, again all-distinct.
        fn batch_all_distinct() -> Vec<Handle> {
            let live: Vec<Handle> = (0..256).map(|i| register_handle(i as i64)).collect();
            let mut sorted = live.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                live.len(),
                "no two concurrently-live handles may share an id"
            );
            live
        }

        let first = batch_all_distinct();
        for h in &first {
            drop_handle(*h);
        }
        // Promote the just-quarantined ids so the next batch reuses them (the
        // host pump's per-tick drain); they must still all be mutually distinct.
        drain_quarantined_handles();
        let second = batch_all_distinct();
        for h in &second {
            drop_handle(*h);
        }
    }

    #[test]
    fn freelist_is_bounded() {
        // The bounding invariant, tested against a local freelist so it is
        // deterministic and can't race the process-wide one. Past `cap`,
        // `push_bounded` drops the id on the floor — `register_handle` then
        // falls back to a fresh id, exactly as before recycling existed.
        let cap = 4;
        let mut free: Vec<Handle> = Vec::new();
        for id in 0..(cap as Handle + 8) {
            push_bounded(&mut free, id, cap);
        }
        assert_eq!(free.len(), cap, "freelist must not grow past the cap");
        // Below the cap it parks every id in order.
        assert_eq!(free, vec![0, 1, 2, 3]);
    }

    #[test]
    fn churn_does_not_exhaust_the_id_band() {
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        // Register/drop churn past the id band size while never holding more
        // than one handle live. With recycling the fresh counter barely
        // moves — each drop refills the freelist the next register drains —
        // so cumulative allocations are decoupled from fresh-id consumption.
        // Without recycling this loop would advance the counter by
        // `iterations` and `next_fresh_handle_id` would PANIC at the
        // `FFI_HANDLE_ID_END` exhaustion check (a fail-before of a different
        // shape: the no-reclaim build can't even complete the loop).
        //
        // Measure the fresh-counter delta directly. Concurrent tests mint a
        // bounded handful of fresh ids; recycling keeps OUR contribution near
        // zero, so the total delta stays tiny in absolute terms.
        let iterations = FFI_HANDLE_ID_END as usize + 8192;
        let before = NEXT_HANDLE.load(Ordering::SeqCst);
        for n in 0..iterations {
            let h = register_handle(n as i64);
            assert!(drop_handle(h));
            // The dropped id is quarantined, not yet reusable — drain it back
            // to the freelist (the host pump's per-tick promotion) so the next
            // register reuses it rather than minting fresh and exhausting the
            // band. A no-reclaim build quarantines nothing, so this drain is a
            // no-op and the counter still runs away.
            drain_quarantined_handles();
        }
        let after = NEXT_HANDLE.load(Ordering::SeqCst);
        let fresh_minted = (after - before) as usize;
        assert!(
            fresh_minted < 4096,
            "fresh-id consumption ({fresh_minted}) over {iterations} \
             register/drop cycles should stay tiny once ids recycle; a \
             no-reclaim registry would mint one per allocation and exhaust \
             the band"
        );
    }

    // ----------------------------------------------------------------
    // Reserved-id recycling (#6441).
    //
    // `reserve_handle_id` mints a globally-unique id WITHOUT storing a
    // value in `HANDLES` — for a subsystem (perry-ext-net's socket map)
    // that keeps its own object map. Before `free_handle_id` existed
    // nothing ever returned a reserved id, so a long-running server
    // leaked the whole `[1, 0x40000)` band and then crashed. These tests
    // pin the two halves of the fix: reserved ids now recycle through the
    // same quarantine as `drop_handle` ids, and exhaustion degrades to
    // `INVALID_HANDLE` instead of a panic.
    // ----------------------------------------------------------------

    #[test]
    fn fresh_id_or_exhausted_flags_the_band_boundary() {
        // Pure boundary logic, tested without advancing the process-wide
        // `NEXT_HANDLE` past `FFI_HANDLE_ID_END` (which would break every
        // other test in this binary). Ids strictly below the end are usable;
        // the end value and anything past it are exhausted (`None`).
        assert_eq!(fresh_id_or_exhausted(1), Some(1));
        assert_eq!(
            fresh_id_or_exhausted(FFI_HANDLE_ID_END - 1),
            Some(FFI_HANDLE_ID_END - 1)
        );
        assert_eq!(fresh_id_or_exhausted(FFI_HANDLE_ID_END), None);
        assert_eq!(fresh_id_or_exhausted(FFI_HANDLE_ID_END + 4096), None);
    }

    #[test]
    fn reserve_free_reserve_recycles_the_id() {
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        // Reserve/free churn far past the band size while holding at most one
        // id live. With `free_handle_id` recycling each id through the
        // quarantine, the fresh counter barely moves — cumulative reservations
        // decouple from fresh-id consumption, exactly as `register`/`drop` do.
        //
        // Before this fix `reserve_handle_id` had no `free_*` twin, so this
        // loop would advance the counter by `iterations`, exhaust the band, and
        // (post-#6441) start returning `INVALID_HANDLE` — tripping the
        // `assert_ne!` below. (Pre-#6441 it panicked outright.) Either way a
        // no-recycle build cannot complete the loop.
        let iterations = FFI_HANDLE_ID_END as usize + 8192;
        let before = NEXT_HANDLE.load(Ordering::SeqCst);
        for _ in 0..iterations {
            let id = reserve_handle_id();
            assert_ne!(
                id, INVALID_HANDLE,
                "reserve_handle_id must not run out of ids once freed ids recycle"
            );
            free_handle_id(id);
            // Promote the just-quarantined id back to the freelist (the host
            // pump's per-tick drain) so the next reserve reuses it.
            drain_quarantined_handles();
        }
        let fresh_minted = (NEXT_HANDLE.load(Ordering::SeqCst) - before) as usize;
        assert!(
            fresh_minted < 4096,
            "fresh-id consumption ({fresh_minted}) over {iterations} \
             reserve/free cycles should stay tiny once reserved ids recycle"
        );
    }

    #[test]
    fn freed_reserved_id_not_reusable_until_drained() {
        // The ABA guarantee for reserved ids, mirroring
        // `freed_id_is_not_reusable_until_drained_no_cross_request_bleed`: a
        // socket id freed on `'close'` must not be handed to a *new* reservation
        // within the same tick, or a stale JS `socket` reference dispatched
        // before the next drain would alias the new socket (the exact aliasing
        // class #6407 fixes). While quarantined the id maps to nothing, so the
        // stale dispatch spends against an empty slot.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let h = reserve_handle_id();
        assert_ne!(h, INVALID_HANDLE);
        free_handle_id(h);

        // No drain yet: `h` sits in the quarantine, NOT the freelist, so neither
        // a fresh reservation nor a registration can re-mint it this tick. Only
        // the serialized recycle tests call `drain`, and this test holds the
        // lock, so `h` provably stays quarantined here.
        let other_reserved = reserve_handle_id();
        assert_ne!(
            other_reserved, h,
            "a just-freed reserved id must not be reusable within the same tick"
        );
        let other_registered = register_handle(0_i64);
        assert_ne!(
            other_registered, h,
            "a just-freed reserved id must not leak into register_handle this tick"
        );

        // After a drain boundary the id is promoted and eventually reused.
        free_handle_id(other_reserved);
        let reused = drop_then_register_reusing(other_registered, 55_i64);
        assert_eq!(with_handle::<i64, _, _>(reused, |v| *v), Some(55));
        drop_handle(reused);
        drain_quarantined_handles();
    }

    #[test]
    fn free_handle_id_until_holds_reserved_id_past_ticks() {
        // The deadline-gated twin for reserved ids (the reaper's peer-disconnect
        // path in handle-registry terms): a reserved id freed with a future
        // grace deadline stays parked across EVERY drain until the deadline
        // elapses, then recycles cleanly.
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let h = reserve_handle_id();
        assert_ne!(h, INVALID_HANDLE);
        free_handle_id_until(h, Instant::now() + Duration::from_secs(3600));

        for _ in 0..5 {
            drain_quarantined_handles();
            let probe = reserve_handle_id();
            assert_ne!(
                probe, h,
                "a deadline-gated reserved id must not recycle before its grace \
                 deadline elapses"
            );
            free_handle_id(probe);
        }

        // An id parked with an already-elapsed deadline is promoted by the very
        // next drain and reused (retry to absorb parallel tests racing the
        // shared freelist, same pattern as `drop_then_register_reusing`).
        let dead = reserve_handle_id();
        free_handle_id_until(dead, Instant::now() - Duration::from_secs(1));
        let mut recycled = false;
        for _ in 0..10_000 {
            drain_quarantined_handles();
            let candidate = reserve_handle_id();
            if candidate == dead {
                recycled = true;
                free_handle_id(candidate);
                break;
            }
            free_handle_id(candidate);
        }
        assert!(
            recycled,
            "an elapsed-deadline reserved id must recycle once its grace passes"
        );
        // `h`'s far-future entry stays parked — the point of the test. The
        // freelist is bounded, so a single held id leaks nothing that matters.
        drain_quarantined_handles();
    }

    #[test]
    fn free_handle_id_ignores_invalid_handle() {
        // `reserve_handle_id` returns `INVALID_HANDLE` on exhaustion, so callers
        // free its result unconditionally; freeing the sentinel must be a no-op
        // (never park `0` for reuse — it is the "no handle" value).
        free_handle_id(INVALID_HANDLE);
        free_handle_id_until(INVALID_HANDLE, Instant::now());
        drain_quarantined_handles();
        // `register_handle` never returns `INVALID_HANDLE`, so if `0` had been
        // parked and handed back this would fail its own non-null invariant.
        let h = register_handle(1_i64);
        assert_ne!(h, INVALID_HANDLE);
        drop_handle(h);
    }
}
