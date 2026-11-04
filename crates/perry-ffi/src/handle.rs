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
//! A fresh `i64` is allocated under the native state mutex from a counter starting at
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
//! still advance the fresh-id counter and consume fresh ids. Ids are
//! therefore reused over time but a given id is unique among the
//! handles live at any instant — a recycled id is only parked after
//! its prior entry was removed from the map.
//!
//! A removed payload first enters native quarantine. Reuse requires a later
//! drain, any deadline to have elapsed, and zero wrapper/operation leases.
//! Slot identity is `(registry domain, registration serial, numeric id)`; the
//! numeric provider ABI stays unchanged. Pending insertion and retiring removal
//! run outside the state mutex while their slots remain unavailable for reuse.
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

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::{
    NativeLeaseKind, NativeRegistrationIdentity, NativeRegistrationKind, NativeRegistrationLease,
    NativeRegistrationRegistry, NativeRegistryDomain,
};
use dashmap::DashMap;
use std::sync::LazyLock as Lazy;

/// Opaque integer handle to a Rust object. `0` is reserved as
/// [`INVALID_HANDLE`]; valid handles start at `1`.
pub type Handle = i64;

/// Sentinel value for "no handle" / null. Never returned by
/// [`register_handle`]; may be passed in by FFI callers when the
/// JS side has `null` / `undefined`.
pub const INVALID_HANDLE: Handle = 0;

/// Hashes a handle id for the payload maps. Ids are small sequential integers
/// chosen by the registry, never by a client, so a multiplicative mix spreads
/// them over shards and buckets without SipHash's per-probe cost.
#[derive(Default, Clone, Copy)]
pub struct HandleIdHasher(u64);

impl std::hash::Hasher for HandleIdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        let mixed = self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        mixed ^ (mixed >> 29)
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(8)) ^ u64::from(byte);
        }
    }
    #[inline]
    fn write_i64(&mut self, id: i64) {
        self.0 = id as u64;
    }
    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }
}

/// Build hasher for maps keyed by a [`Handle`].
pub type HandleIdBuildHasher = std::hash::BuildHasherDefault<HandleIdHasher>;

static HANDLES: Lazy<DashMap<Handle, crate::NativePayloadRecord, HandleIdBuildHasher>> =
    Lazy::new(|| DashMap::with_hasher(HandleIdBuildHasher::default()));
const FFI_HANDLE_ID_START: Handle = 1;

// ---------------------------------------------------------------------------
// Per-type id index for rarely-instanced, frequently-enumerated types.
//
// `iter_handles_of` / `iter_handle_ids_of` used to walk EVERY entry of
// `HANDLES` and downcast each one. perry-ext-http's per-tick pump does that
// four times a tick (keepalive probe + one walk per server kind) just to find
// the one or two server handles, so its cost grew with every other live handle
// in the process — and since `req.socket` became a per-connection handle, a
// server holding N keep-alive connections paid O(N) per tick: -9% req/s at
// c=256 on the bench mini. A type declared through `index_handle_type::<T>()`
// keeps its live ids in a side list maintained by register/remove, and the
// three iterators visit only those ids.
//
// Cost for everything else: `register_handle` / removal compare the payload's
// `TypeId` against at most `MAX_INDEXED_TYPES` declared ids (none when nothing
// was declared). Only declare types with few live instances — the side list is
// a `Vec` updated by linear search on removal.
// ---------------------------------------------------------------------------

const MAX_INDEXED_TYPES: usize = 8;
static INDEXED_LEN: AtomicUsize = AtomicUsize::new(0);
static INDEXED_TYPES: [OnceLock<TypeId>; MAX_INDEXED_TYPES] =
    [const { OnceLock::new() }; MAX_INDEXED_TYPES];
static INDEXED_IDS: [Mutex<Vec<Handle>>; MAX_INDEXED_TYPES] =
    [const { Mutex::new(Vec::new()) }; MAX_INDEXED_TYPES];
static INDEX_DECLARE: Mutex<()> = Mutex::new(());

#[inline]
fn indexed_slot(type_id: TypeId) -> Option<usize> {
    let len = INDEXED_LEN.load(Ordering::Acquire);
    (0..len).find(|&slot| INDEXED_TYPES[slot].get() == Some(&type_id))
}

fn lock_index(slot: usize) -> std::sync::MutexGuard<'static, Vec<Handle>> {
    INDEXED_IDS[slot]
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The live ids of an indexed type, snapshotted so callers may register or
/// drop handles while visiting them. `None` when `T` is not indexed.
fn indexed_ids_of<T: 'static>() -> Option<Vec<Handle>> {
    let slot = indexed_slot(TypeId::of::<T>())?;
    Some(lock_index(slot).clone())
}

/// Keep a side list of the live handle ids whose payload is a `T`, so
/// [`iter_handles_of`], [`iter_handles_of_mut`] and [`iter_handle_ids_of`] for
/// `T` visit only those instead of walking the whole registry.
///
/// Idempotent, and correct at any time: handles of `T` registered before the
/// call are backfilled. Meant for types with few live instances that are
/// enumerated often (servers polled every event-loop tick). Declaring more
/// than `MAX_INDEXED_TYPES` types leaves the extras unindexed (they keep the
/// full walk), which is slower but never wrong.
pub fn index_handle_type<T: 'static + Send + Sync>() {
    let type_id = TypeId::of::<T>();
    if indexed_slot(type_id).is_some() {
        return;
    }
    let _declaring = INDEX_DECLARE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if indexed_slot(type_id).is_some() {
        return;
    }
    let slot = INDEXED_LEN.load(Ordering::Acquire);
    if slot >= MAX_INDEXED_TYPES {
        return;
    }
    // Hold the slot's list across publication and backfill: a concurrent
    // register/remove of `T` that sees the new length waits here until the
    // backfill is in, and one that raced ahead of publication is either found
    // by the walk (its insert happened first) or already gone from the map.
    let mut ids = lock_index(slot);
    let _ = INDEXED_TYPES[slot].set(type_id);
    INDEXED_LEN.store(slot + 1, Ordering::Release);
    for entry in HANDLES.iter() {
        if entry.value().is::<T>() {
            ids.push(*entry.key());
        }
    }
}

fn index_note_registered(type_id: TypeId, handle: Handle) {
    if let Some(slot) = indexed_slot(type_id) {
        let mut ids = lock_index(slot);
        // A registration racing `index_handle_type`'s backfill can already
        // be listed.
        if !ids.contains(&handle) {
            ids.push(handle);
        }
    }
}

fn index_note_removed(payload: &(dyn Any + Send + Sync), handle: Handle) {
    // `Any::type_id` of the payload itself, not of the box around it.
    if let Some(slot) = indexed_slot(Any::type_id(payload)) {
        let mut ids = lock_index(slot);
        if let Some(position) = ids.iter().position(|&id| id == handle) {
            ids.swap_remove(position);
        }
    }
}
const FFI_HANDLE_ID_END: Handle = 0x40000;

const FREE_HANDLES_CAP: usize = 64 * 1024;
pub(crate) static REGISTRATIONS: Lazy<NativeRegistrationRegistry> = Lazy::new(|| {
    NativeRegistrationRegistry::new(FFI_HANDLE_ID_START, FFI_HANDLE_ID_END, FREE_HANDLES_CAP)
});

/// The process-wide numeric id pool behind every handle in the shared
/// `[1, 0x40000)` band. A private payload registry that mints ids in that band
/// must allocate them from this pool (under its own domain), or its ids alias
/// this map's and every "is this handle mine?" probe answers for the wrong
/// object (#11196: perry-stdlib's common registry minted its own `1` beside an
/// ext-net socket `1`). Clones name the same allocator.
pub fn shared_handle_id_pool() -> NativeRegistrationRegistry {
    REGISTRATIONS.clone()
}

/// The authoritative domain for payloads stored in this FFI map.
pub fn handle_registry_domain() -> NativeRegistryDomain {
    REGISTRATIONS.domain()
}

/// Lookup alone retains no lease; acquire rechecks the full identity.
/// A record this mutator holds parked is still registered: its identity
/// lives in the parked entry.
pub fn handle_registration(handle: Handle) -> Option<NativeRegistrationIdentity> {
    REGISTRATIONS
        .identity(handle)
        .or_else(|| crate::handle_lifecycle::parked_identity(handle))
}

/// Acquire a counted reference only if this exact registration is still Live.
/// The lease orders id reuse; it does not retain the payload or a payload borrow.
pub fn acquire_handle_registration(
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
) -> Option<NativeRegistrationLease> {
    REGISTRATIONS
        .acquire(identity, kind)
        .or_else(|| crate::handle_lifecycle::acquire_parked(identity, kind))
}

/// Advance both quarantine tiers. A retired id is reusable only after its
/// deadline/drain boundary and the release of every wrapper/operation lease.
pub fn drain_quarantined_handles() -> usize {
    REGISTRATIONS.drain(Instant::now())
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
// Same reason: `register_handle`'s exhaustion path references the runtime's
// throw entry, which this unit-test binary does not link. No test here
// exhausts the process-wide band; if one ever does, it aborts loudly.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn js_throw_error_with_code(
    _msg_ptr: *const u8,
    _msg_len: usize,
    _code_ptr: *const u8,
    _code_len: usize,
    _kind: i32,
) -> ! {
    std::process::abort()
}

#[cfg(not(all(test, not(feature = "runtime-link"))))]
extern "C" {
    fn perry_ffi_gc_root_visitor_is_full_mark(ctx: *mut c_void) -> bool;
    fn perry_ffi_gc_root_visitor_is_young_scope(ctx: *mut c_void) -> bool;
    fn perry_ffi_gc_word_is_minor_relevant(bits: u64) -> bool;
}

// The unit-test binary drives scanners only through perry-ffi's own
// edge-collecting visitor, which never asks the runtime.
#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_root_visitor_is_full_mark(_ctx: *mut c_void) -> bool {
    false
}

#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_root_visitor_is_young_scope(_ctx: *mut c_void) -> bool {
    false
}

// No heap here: the tests name the words a minor could act on.
#[cfg(all(test, not(feature = "runtime-link")))]
thread_local! {
    pub(crate) static MINOR_RELEVANT_WORDS_FOR_TESTS: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(all(test, not(feature = "runtime-link")))]
#[no_mangle]
unsafe extern "C" fn perry_ffi_gc_word_is_minor_relevant(bits: u64) -> bool {
    MINOR_RELEVANT_WORDS_FOR_TESTS.with(|words| words.borrow().contains(&bits))
}

/// Whether a minor-scoped pass can act on the referent of the JSValue word
/// `bits`: a nursery, longlived or malloc-GC object (perry-runtime
/// `gc/young_log.rs`). An old object is neither moved nor swept by a minor and
/// never becomes young again; a non-heap word answers false.
pub(crate) fn word_is_minor_relevant(bits: u64) -> bool {
    bits != 0 && unsafe { perry_ffi_gc_word_is_minor_relevant(bits) }
}

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
    handle_exists(handle)
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
    /// Collects the visited slots' referents as JSValue words instead of
    /// handing them to the runtime (a parked record's edges for `observe`).
    collects_edges: bool,
    /// An edge collector standing in for a minor-scoped pass (tests).
    collects_young: bool,
    _marker: PhantomData<&'a mut ()>,
}

impl<'a> GcRootVisitor<'a> {
    /// Whether this visitor is performing a full reachability mark.
    pub fn is_full_mark(&self) -> bool {
        if self.collects_edges {
            return !self.collects_young;
        }
        unsafe { perry_ffi_gc_root_visitor_is_full_mark(self.ctx) }
    }

    /// Whether this visit belongs to a minor-scoped pass, which can act only
    /// on words [`word_is_minor_relevant`] accepts: a table that logs the
    /// entries that may hold such a word may visit only those.
    pub fn is_young_scope(&self) -> bool {
        if self.collects_edges {
            return self.collects_young;
        }
        unsafe { perry_ffi_gc_root_visitor_is_young_scope(self.ctx) }
    }

    fn new(visit: PerryFfiMutableRootVisitor, ctx: *mut c_void) -> Self {
        Self {
            visit,
            ctx,
            collects_edges: false,
            collects_young: false,
            _marker: PhantomData,
        }
    }

    /// A full-mark visitor that pushes each visited slot's referent onto
    /// `edges` as a JSValue word (raw addresses become pointer words) and
    /// rewrites nothing: the same scanner code then names a payload's edges.
    pub(crate) fn edge_collector(edges: &'a mut Vec<u64>) -> Self {
        Self {
            visit: collect_edge_slot,
            ctx: edges as *mut Vec<u64> as *mut c_void,
            collects_edges: true,
            collects_young: false,
            _marker: PhantomData,
        }
    }

    /// [`edge_collector`](Self::edge_collector) that answers as a
    /// minor-scoped pass (tests drive the young-scope walks with it).
    #[doc(hidden)]
    pub fn young_edge_collector_for_tests(edges: &'a mut Vec<u64>) -> Self {
        let mut visitor = Self::edge_collector(edges);
        visitor.collects_young = true;
        visitor
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

    /// Visit a raw const heap pointer slot.
    ///
    /// Native UI backends commonly unbox a closure once and retain its code
    /// pointer as `*const u8`. The collector treats const and mutable pointee
    /// types identically; it only needs the address of the mutable pointer
    /// *slot* so an evacuating collection can rewrite it.
    ///
    /// Returns `true` when the runtime rewrote the slot to a forwarded address.
    pub fn visit_raw_const_ptr_slot<T>(&mut self, slot: &mut *const T) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_RAW_MUT_PTR,
            slot as *mut *const T as *mut c_void,
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

extern "C" fn collect_edge_slot(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const PAYLOAD: u64 = 0x0000_FFFF_FFFF_FFFF;
    let address_word = |address: u64| {
        if address == 0 {
            0
        } else {
            POINTER_TAG | (address & PAYLOAD)
        }
    };
    // SAFETY: `slot` is the live slot of the kind the visitor method named.
    let word = unsafe {
        match kind {
            FFI_ROOT_SLOT_I64 => {
                let address = *(slot as *const i64);
                if address > 0 {
                    address_word(address as u64)
                } else {
                    0
                }
            }
            FFI_ROOT_SLOT_USIZE | FFI_ROOT_SLOT_RAW_MUT_PTR => {
                address_word(*(slot as *const usize) as u64)
            }
            FFI_ROOT_SLOT_NANBOX_F64 => (*(slot as *const f64)).to_bits(),
            FFI_ROOT_SLOT_NANBOX_U64 => *(slot as *const u64),
            _ => 0,
        }
    };
    if word != 0 {
        // SAFETY: `edge_collector` set `ctx` to its `&mut Vec<u64>`.
        unsafe { &mut *(ctx as *mut Vec<u64>) }.push(word);
    }
    false
}

/// Register `value` under a fresh handle and return the handle.
///
/// `T` must be `Send + Sync + 'static` — the registry is shared
/// across threads (tokio workers may resolve promises that touch
/// handle data while the main thread is also touching it).
pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    crate::event_pump::ensure_handle_tick_hook_registered();
    ensure_handle_exists_probe_registered();
    crate::handle_lifecycle::note_registration();
    let identity = match REGISTRATIONS.begin_registration(NativeRegistrationKind::Payload) {
        Ok(identity) => identity,
        Err(error) => {
            drop(value);
            throw_handle_ids_exhausted(error)
        }
    };
    let handle = identity.numeric_id();
    // Pending blocks acquisition/reuse while the payload-map lock is held.
    let previous = HANDLES.insert(handle, crate::NativePayloadRecord::new(value));
    assert!(
        previous.is_none(),
        "pending native id must have an empty payload slot"
    );
    // Still pending, so the id cannot be reused before the index has it.
    index_note_registered(TypeId::of::<T>(), handle);
    assert!(REGISTRATIONS.publish(identity));
    handle
}

/// True exhaustion of the shared band — every id live, quarantined, or still
/// named by a reachable JS value — is a catchable `Error` with code
/// `ERR_PERRY_HANDLE_IDS_EXHAUSTED` rather than a process-aborting panic
/// (#11453). Steady-state churn cannot reach it: retired ids recycle through
/// the collector, and reclamation is requested first so a later registration
/// can succeed.
fn throw_handle_ids_exhausted(error: crate::NativeRegistrationError) -> ! {
    crate::handle_lifecycle::request_full_trace();
    let message = format!(
        "Perry native handle ids exhausted ({error:?}): all {} ids of the shared handle \
         band are live or still referenced",
        FFI_HANDLE_ID_END - FFI_HANDLE_ID_START
    );
    crate::throw_with_code(
        &message,
        "ERR_PERRY_HANDLE_IDS_EXHAUSTED",
        crate::ErrorKind::Error,
    )
}

/// Reserve from the shared numeric pool without inserting an FFI payload.
/// Exhaustion preserves the legacy zero sentinel.
pub fn reserve_handle_id() -> Handle {
    reserve_handle_id_in_domain(handle_registry_domain())
}

/// Reserve an id for a private payload registry using its authoritative domain.
/// The caller publishes no JavaScript value here and must populate its own map
/// before handing the numeric id to its clients.
pub fn reserve_handle_id_in_domain(domain: NativeRegistryDomain) -> Handle {
    crate::event_pump::ensure_handle_tick_hook_registered();
    crate::handle_lifecycle::note_registration();
    let Ok(identity) =
        REGISTRATIONS.begin_registration_in_domain(domain, NativeRegistrationKind::Reserved)
    else {
        return INVALID_HANDLE;
    };
    assert!(REGISTRATIONS.publish(identity));
    identity.numeric_id()
}

/// Retire a reserved id after its owner removed the payload. Duplicate frees,
/// zero, and attempts to free ordinary payload ids leave the queues unchanged.
pub fn free_handle_id(id: Handle) {
    free_reserved_id(id, None);
}

/// Retire a reserved id after its owner removes the payload, delaying reuse
/// until a later drain at or after `deadline` with both lease counts at zero.
/// Zero, duplicate retirement, and ordinary payload ids leave queues unchanged.
pub fn free_handle_id_until(id: Handle, deadline: Instant) {
    free_reserved_id(id, Some(deadline));
}

/// A reserved id reached JS through its owner's map, so it parks like a
/// payload id: reusable once a full trace proves no value names it.
fn free_reserved_id(id: Handle, deadline: Option<Instant>) {
    if let Some(identity) = REGISTRATIONS.begin_retirement(id, NativeRegistrationKind::Reserved) {
        park(identity, deadline);
    }
}

/// Every retirement in the shared band waits for the collector's proof
/// (`handle_lifecycle`): a JS value copies an id with no hook, so no tick
/// quarantine or lease can tell when the last copy died.
fn park(identity: crate::NativeRegistrationIdentity, deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => crate::handle_lifecycle::park_retired_until(identity, deadline),
        None => crate::handle_lifecycle::park_retired(identity),
    }
}

/// Look up a handle and run `f` against the borrowed value.
/// Recommended over [`get_handle`] — the borrow is scoped, so
/// concurrent [`take_handle`] / [`drop_handle`] can't dangle it.
pub fn with_handle<T: 'static + Send + Sync, R, F: FnOnce(&T) -> R>(
    handle: Handle,
    f: F,
) -> Option<R> {
    if let Some(entry) = HANDLES.get(&handle) {
        return entry.value().downcast_ref::<T>().map(f);
    }
    // SAFETY: see `handle_lifecycle::parked_record`.
    let record = unsafe { &*crate::handle_lifecycle::parked_record(handle)? };
    record.payload.downcast_ref::<T>().map(f)
}

/// Look up a handle and run `f` against a mutable borrow. Same
/// caveats as [`with_handle`].
pub fn with_handle_mut<T: 'static + Send + Sync, R, F: FnOnce(&mut T) -> R>(
    handle: Handle,
    f: F,
) -> Option<R> {
    // A mutable borrow may store an edge into a parked payload.
    crate::handle_lifecycle::note_mutable_access(handle);
    if let Some(mut entry) = HANDLES.get_mut(&handle) {
        return entry.value_mut().downcast_mut::<T>().map(f);
    }
    // SAFETY: see `handle_lifecycle::parked_record`.
    let record = unsafe { &mut *crate::handle_lifecycle::parked_record_for_write(handle)? };
    let result = record.payload.downcast_mut::<T>().map(f);
    // `f` may allocate, so a minor may have run inside it and dropped the
    // record from the young log before `f` stored a young word.
    crate::handle_lifecycle::note_young_write(handle);
    result
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
    if let Some(entry) = HANDLES.get(&handle) {
        let ptr = entry.value().downcast_ref::<T>()? as *const T;
        return Some(unsafe { &*ptr });
    }
    // A parked record's box stays put until its release, like a registry
    // entry's until its removal.
    let record = unsafe { &*crate::handle_lifecycle::parked_record(handle)? };
    let ptr = record.payload.downcast_ref::<T>()? as *const T;
    Some(unsafe { &*ptr })
}

/// Mutable counterpart to [`get_handle`].
pub fn get_handle_mut<T: 'static + Send + Sync>(handle: Handle) -> Option<&'static mut T> {
    crate::handle_lifecycle::note_mutable_access(handle);
    if let Some(mut entry) = HANDLES.get_mut(&handle) {
        let ptr = entry.value_mut().downcast_mut::<T>()? as *mut T;
        return Some(unsafe { &mut *ptr });
    }
    // The borrow, like a registry one, must not live across a collection: the
    // young log names the record from here to the next minor only.
    let record = unsafe { &mut *crate::handle_lifecycle::parked_record_for_write(handle)? };
    let ptr = record.payload.downcast_mut::<T>()? as *mut T;
    Some(unsafe { &mut *ptr })
}

/// Remove the handle from the registry and return its value if
/// the type matches. After this, the handle is no longer valid.
pub fn take_handle<T: 'static + Send + Sync>(handle: Handle) -> Option<T> {
    remove_payload(handle, None)
        .and_then(|boxed| boxed.downcast::<T>().ok())
        .map(|boxed| *boxed)
}

/// Remove the current payload; its native identity outlives retained leases.
pub fn drop_handle(handle: Handle) -> bool {
    remove_payload(handle, None).is_some()
}

/// Remove the current payload and return whether a payload was removed.
/// Its id is reused only after a full trace that began at or after `deadline`
/// proves no value names it; retained leases do not retain the removed payload.
pub fn drop_handle_until(handle: Handle, deadline: Instant) -> bool {
    remove_payload(handle, Some(deadline)).is_some()
}

fn remove_payload(handle: Handle, deadline: Option<Instant>) -> Option<Box<dyn Any + Send + Sync>> {
    let Some(identity) = REGISTRATIONS.begin_retirement(handle, NativeRegistrationKind::Payload)
    else {
        // A parked record's registration lives in its parked entry: it
        // re-enters the slot map as Retiring, and the retired id replaces
        // the entry.
        let (registration, record) = crate::handle_lifecycle::take_parked_record(handle)?;
        park(REGISTRATIONS.retire_parked(registration), deadline);
        return Some(record.payload);
    };
    // Retiring blocks acquisition/reuse. Neither payload removal nor its later
    // destructor runs under the registration-state mutex.
    let removed = match HANDLES.remove(&handle) {
        Some((_, record)) => {
            // Still retiring, so the id cannot be reused before the index
            // drops it.
            index_note_removed(record.payload.as_ref(), handle);
            Some(record.payload)
        }
        // Live in the slot map, so never parked.
        None => None,
    };
    park(identity, deadline);
    removed
}

/// Park a registered payload whose owners are JS values once native code is
/// finished with it. Its record leaves the registry (so registry walks and
/// type scanners no longer see it) for the parked entry, from which perry-ffi
/// drops it and reissues the id once a full trace proves no traced word names
/// it; `owner` names its edges meanwhile. Until then it stays readable through
/// the lookups on this mutator. Returns false when `handle` is not a live
/// registration.
pub fn park_reclaimable_ffi(
    handle: Handle,
    owner: &'static dyn crate::handle_lifecycle::ParkedRecordOwner,
) -> bool {
    let Some(identity) = REGISTRATIONS.identity(handle) else {
        return false;
    };
    // The registration leaves the slot map for the parked entry.
    let Some(registration) = REGISTRATIONS.park(identity) else {
        return false;
    };
    let Some((_, record)) = HANDLES.remove(&handle) else {
        REGISTRATIONS.unpark(registration);
        return false;
    };
    index_note_removed(record.payload.as_ref(), handle);
    crate::handle_lifecycle::park_record(registration, owner, record);
    true
}

/// A parked record a trace proved unnamed: hand its id back at once (an
/// outstanding lease still defers reuse), then drop the record outside every
/// registry lock.
pub(crate) fn release_parked_record(
    registration: crate::NativeParkedRegistration,
    record: crate::NativePayloadRecord,
) {
    REGISTRATIONS.release_parked(registration);
    drop(record);
}

/// A parked record native code owns again (`retain_strongly`): payload first,
/// then the registration is Live in the slot map again.
pub(crate) fn restore_parked_record(
    handle: Handle,
    registration: crate::NativeParkedRegistration,
    record: crate::NativePayloadRecord,
) {
    let type_id = Any::type_id(record.payload.as_ref());
    HANDLES.insert(handle, record);
    index_note_registered(type_id, handle);
    REGISTRATIONS.unpark(registration);
}

/// True if the handle currently maps to a registered object.
pub fn handle_exists(handle: Handle) -> bool {
    HANDLES.contains_key(&handle) || crate::handle_lifecycle::parked_record(handle).is_some()
}

/// Visit every registered handle whose stored type matches `T`,
/// invoking `f(&value)` for each.
///
/// Used by GC root scanners that need to keep user closures alive
/// — e.g. listener closures stored inside a registered handle's
/// payload. Without this, a malloc-triggered GC between
/// `.on(...)` and the dispatch would sweep the closure
/// (issue #35 pattern in perry-stdlib).
///
/// Pair with [`gc_register_root_scanner`] to wire the scanner into
/// perry's GC.
pub fn iter_handles_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(&T),
{
    if let Some(ids) = indexed_ids_of::<T>() {
        for id in ids {
            if let Some(entry) = HANDLES.get(&id) {
                if let Some(v) = entry.value().downcast_ref::<T>() {
                    f(v);
                }
            }
        }
        return;
    }
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
    if let Some(ids) = indexed_ids_of::<T>() {
        for id in ids {
            if let Some(mut entry) = HANDLES.get_mut(&id) {
                if let Some(v) = entry.value_mut().downcast_mut::<T>() {
                    f(v);
                }
            }
        }
        return;
    }
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
/// perry-ext-http's main-thread pump walks every registered
/// HttpServer / HttpsServer / Http2SecureServer handle each tick to
/// drain pending requests.
pub fn iter_handle_ids_of<T, F>(mut f: F)
where
    T: 'static + Send + Sync,
    F: FnMut(Handle),
{
    if let Some(ids) = indexed_ids_of::<T>() {
        for id in ids {
            let is_t = HANDLES
                .get(&id)
                .is_some_and(|entry| entry.value().is::<T>());
            if is_t {
                f(id);
            }
        }
        return;
    }
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
/// `perry-ext-http`. It is copied into runtime GC diagnostics and
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
#[path = "handle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "handle_registration_tests.rs"]
mod registration_tests;

#[cfg(test)]
#[path = "handle_reclaim_tests.rs"]
mod reclaim_tests;

/// Access the current mutator's bag on an existing native payload record.
/// No caller may retain a reference into the registry across a GC operation.
pub fn native_handle_props(handle: Handle) -> Option<u64> {
    if let Some(entry) = HANDLES.get(&handle) {
        return entry.props();
    }
    // SAFETY: see `handle_lifecycle::parked_record`.
    unsafe { &*crate::handle_lifecycle::parked_record(handle)? }.props()
}
/// Replace the property bag on the current mutator's native owner.
pub fn set_native_handle_props(handle: Handle, bits: u64) -> bool {
    crate::handle_lifecycle::note_mutable_access(handle);
    let stored = match HANDLES.get_mut(&handle) {
        Some(mut entry) => entry.set_props(bits),
        // The parked-record scanner visits a parked record's bag.
        None => {
            return crate::handle_lifecycle::parked_record_for_write(handle)
                .is_some_and(|record| unsafe { &mut *record }.set_props(bits));
        }
    };
    if stored && bits != 0 {
        gc_register_mutable_root_scanner_named("ffi:native-payloads", scan_native_payload_roots);
    }
    stored
}
fn scan_native_payload_roots(visitor: &mut GcRootVisitor<'_>) {
    // Native code owns every registered payload; parked records' bags are
    // visited by the parked-record scanner (`handle_lifecycle`).
    for mut entry in HANDLES.iter_mut() {
        if let Some(slot) = entry.props_slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
    }
}
/// Called by the host's existing allocation-range teardown hook.
pub fn clear_native_handle_props_in_ranges(contains: impl Fn(usize) -> bool) {
    for mut entry in HANDLES.iter_mut() {
        entry.clear_props_if(&contains);
    }
    crate::handle_lifecycle::clear_parked_props_in_ranges(&contains);
}
