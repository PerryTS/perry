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

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::Mutex;
use std::time::Instant;

use crate::{
    NativeLeaseKind, NativeQuarantine, NativeRegistrationIdentity, NativeRegistrationKind,
    NativeRegistrationLease, NativeRegistrationRegistry, NativeRegistryDomain,
};
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

const FREE_HANDLES_CAP: usize = 64 * 1024;
static REGISTRATIONS: Lazy<NativeRegistrationRegistry> = Lazy::new(|| {
    NativeRegistrationRegistry::new(FFI_HANDLE_ID_START, FFI_HANDLE_ID_END, FREE_HANDLES_CAP)
});

/// The authoritative domain for payloads stored in this FFI map.
pub fn handle_registry_domain() -> NativeRegistryDomain {
    REGISTRATIONS.domain()
}

/// Lookup alone retains no lease; acquire rechecks the full identity.
pub fn handle_registration(handle: Handle) -> Option<NativeRegistrationIdentity> {
    REGISTRATIONS.identity(handle)
}

/// Acquire a counted reference only if this exact registration is still Live.
/// The lease orders id reuse; it does not retain the payload or a payload borrow.
pub fn acquire_handle_registration(
    identity: NativeRegistrationIdentity,
    kind: NativeLeaseKind,
) -> Option<NativeRegistrationLease> {
    REGISTRATIONS.acquire(identity, kind)
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

/// Register `value` under a fresh handle and return the handle.
///
/// `T` must be `Send + Sync + 'static` — the registry is shared
/// across threads (tokio workers may resolve promises that touch
/// handle data while the main thread is also touching it).
pub fn register_handle<T: 'static + Send + Sync>(value: T) -> Handle {
    crate::event_pump::ensure_handle_tick_hook_registered();
    ensure_handle_exists_probe_registered();
    let identity = REGISTRATIONS
        .begin_registration(NativeRegistrationKind::Payload)
        .expect("perry-ffi native handle registration exhausted");
    let handle = identity.numeric_id();
    // Pending blocks acquisition/reuse while the payload-map lock is held.
    let previous = HANDLES.insert(handle, Box::new(value));
    assert!(
        previous.is_none(),
        "pending native id must have an empty payload slot"
    );
    assert!(REGISTRATIONS.publish(identity));
    handle
}

/// Reserve from the shared numeric pool without inserting an FFI payload.
/// Exhaustion preserves the legacy zero sentinel.
pub fn reserve_handle_id() -> Handle {
    crate::event_pump::ensure_handle_tick_hook_registered();
    reserve_handle_id_in_domain(handle_registry_domain())
}

/// Reserve an id for a private payload registry using its authoritative domain.
/// The caller publishes no JavaScript value here and must populate its own map
/// before handing the numeric id to its clients.
pub fn reserve_handle_id_in_domain(domain: NativeRegistryDomain) -> Handle {
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
    free_reserved_id(id, NativeQuarantine::NextDrain);
}

/// Retire a reserved id after its owner removes the payload, delaying reuse
/// until a later drain at or after `deadline` with both lease counts at zero.
/// Zero, duplicate retirement, and ordinary payload ids leave queues unchanged.
pub fn free_handle_id_until(id: Handle, deadline: Instant) {
    free_reserved_id(id, NativeQuarantine::Until(deadline));
}

fn free_reserved_id(id: Handle, quarantine: NativeQuarantine) {
    if let Some(identity) = REGISTRATIONS.begin_retirement(id, NativeRegistrationKind::Reserved) {
        assert!(REGISTRATIONS.finish_retirement(identity, quarantine));
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
    remove_payload(handle, NativeQuarantine::NextDrain)
        .and_then(|boxed| boxed.downcast::<T>().ok())
        .map(|boxed| *boxed)
}

/// Remove the current payload; its native identity outlives retained leases.
pub fn drop_handle(handle: Handle) -> bool {
    remove_payload(handle, NativeQuarantine::NextDrain).is_some()
}

/// Remove the current payload and return whether a payload was removed.
/// Its id can be reused only after a later drain at or after `deadline` with
/// both lease counts at zero; retained leases do not retain the removed payload.
pub fn drop_handle_until(handle: Handle, deadline: Instant) -> bool {
    remove_payload(handle, NativeQuarantine::Until(deadline)).is_some()
}

fn remove_payload(
    handle: Handle,
    quarantine: NativeQuarantine,
) -> Option<Box<dyn Any + Send + Sync>> {
    let identity = REGISTRATIONS.begin_retirement(handle, NativeRegistrationKind::Payload)?;
    // Retiring blocks acquisition/reuse. Neither payload removal nor its later
    // destructor runs under the registration-state mutex.
    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    assert!(REGISTRATIONS.finish_retirement(identity, quarantine));
    removed
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
/// perry-ext-http's main-thread pump walks every registered
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
mod tests {
    use super::*;
    use std::time::Duration;

    extern "C" fn rewrite_const_pointer_slot(
        kind: u32,
        slot: *mut c_void,
        ctx: *mut c_void,
    ) -> bool {
        assert_eq!(kind, FFI_ROOT_SLOT_RAW_MUT_PTR);
        unsafe {
            *(slot as *mut *const u8) = ctx as *const u8;
        }
        true
    }

    #[test]
    fn const_pointer_root_slot_is_rewritten() {
        let original = 1_u8;
        let replacement = 2_u8;
        let mut slot = &original as *const u8;
        let mut visitor = GcRootVisitor::new(
            rewrite_const_pointer_slot,
            &replacement as *const u8 as *mut c_void,
        );

        assert!(visitor.visit_raw_const_ptr_slot(&mut slot));
        assert_eq!(slot, &replacement as *const u8);
    }

    #[test]
    fn round_trip_simple_value() {
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let h = register_handle(42_i64);
        // Same handle, wrong type — no value comes back.
        let r = with_handle::<String, _, _>(h, |s| s.clone());
        assert!(r.is_none());
        drop_handle(h);
    }

    #[test]
    fn handles_are_unique() {
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
    // value, while excluding other registry fixtures from the interval. It
    // still fails against a no-reclaim `drop_handle` (the freed id never
    // lands on the freelist, so it is never reused and id consumption is
    // unbounded). The bounding invariant is tested in isolation against a
    // local native registry with a small queue capacity.
    // ----------------------------------------------------------------

    // Every fixture that drains or observes post-removal ids shares this lock,
    // including the sibling registration_tests module and its worker lifetime.
    pub(super) static RECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Register `value`, reporting whether `register_handle` REUSED a parked
    /// id rather than minting a fresh one (the recycling contract). A pop
    /// leaves the fresh-id counter untouched; a fresh mint advances it, which a
    /// no-reclaim build would do on every register. Returns `(handle, reused)`.
    fn register_observing_reuse<T: 'static + Send + Sync>(value: T) -> (Handle, bool) {
        let before = REGISTRATIONS.next_fresh_id_for_tests();
        let handle = register_handle(value);
        let reused = REGISTRATIONS.next_fresh_id_for_tests() == before;
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
    /// meaningful on the process-wide freelist. All registry fixtures in this
    /// binary now share RECYCLE_TEST_LOCK, so no other fixture may drain or
    /// consume a row during this interval. The existing bounded retry still
    /// distinguishes reuse from a removal path that never queues any ids.
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
            // This attempt minted fresh; retire it before checking reuse again.
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
        // independent of perry-ext-http's reaper plumbing): a response R1
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
    fn ordinary_quarantine_is_bounded() {
        let registry = NativeRegistrationRegistry::new(1, 20, 4);
        let ids: Vec<_> = (0..12)
            .map(|_| {
                let identity = registry
                    .begin_registration(NativeRegistrationKind::Reserved)
                    .unwrap();
                assert!(registry.publish(identity));
                identity
            })
            .collect();
        for identity in ids {
            assert!(registry.begin_retirement_of(identity));
            assert!(registry.finish_retirement(identity, NativeQuarantine::NextDrain));
        }
        assert_eq!(
            registry.drain(Instant::now()),
            4,
            "ordinary quarantine retains at most four of twelve retirements"
        );
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
        let before = REGISTRATIONS.next_fresh_id_for_tests();
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
        let after = REGISTRATIONS.next_fresh_id_for_tests();
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
        let registry = NativeRegistrationRegistry::new(FFI_HANDLE_ID_END - 1, FFI_HANDLE_ID_END, 4);
        assert_eq!(
            registry
                .begin_registration(NativeRegistrationKind::Reserved)
                .unwrap()
                .numeric_id(),
            FFI_HANDLE_ID_END - 1
        );
        assert_eq!(
            registry.begin_registration(NativeRegistrationKind::Reserved),
            Err(crate::NativeRegistrationError::IdExhausted)
        );
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
        let before = REGISTRATIONS.next_fresh_id_for_tests();
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
        let fresh_minted = (REGISTRATIONS.next_fresh_id_for_tests() - before) as usize;
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
        let _serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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

#[cfg(test)]
#[path = "handle_registration_tests.rs"]
mod registration_tests;
