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
//! Each `i64` is allocated atomically from a counter starting at 1
//! — `0` is reserved as `INVALID_HANDLE` so `register_handle` can
//! never produce a falsy value (matches JS truthiness semantics
//! for type checks like `if (handle)`).
//!
//! perry-stdlib has its own copy of this same registry (in
//! `crates/perry-stdlib/src/common/handle.rs`). They are separate
//! integer spaces — perry-ffi-allocated handles cannot be looked
//! up via perry-stdlib's `get_handle`, and vice versa. Programs
//! that link both registries (e.g. via the well-known flip) just
//! end up with two `DashMap` statics; each wrapper consults the
//! registry it was compiled against, so handles never collide.
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
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
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
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    HANDLES.insert(handle, Box::new(value));
    handle
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
    HANDLES
        .remove(&handle)
        .and_then(|(_, boxed)| boxed.downcast::<T>().ok())
        .map(|b| *b)
}

/// Remove a handle and drop its value. Returns `true` if the
/// handle existed.
pub fn drop_handle(handle: Handle) -> bool {
    HANDLES.remove(&handle).is_some()
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
/// Added by issue #604 — perry-ext-http-server's main-thread pump
/// needs to walk every registered HttpServer / HttpsServer /
/// Http2SecureServer handle each tick to drain pending requests.
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
/// exposes copied values only. During copied-minor evacuation the runtime
/// cannot rewrite wrapper-owned storage discovered through this API, so live
/// young roots reported here are treated as copy-only fallback roots. Prefer
/// [`gc_register_mutable_root_scanner`] for new scanners.
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

    #[test]
    fn round_trip_simple_value() {
        let h = register_handle(42_i64);
        assert_ne!(h, INVALID_HANDLE);
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
}
