//! Minimal Proxy runtime support.
//!
//! A `Proxy` wraps a `target` (any JSValue, NaN-boxed f64) and a `handler`
//! (an object whose own fields include optional trap functions: `get`, `set`,
//! `has`, `deleteProperty`, `apply`, `construct`). Traps are closures created
//! in user code.
//!
//! Implementation: a thread-local registry maps a small integer handle to a
//! `ProxyEntry`. The handle is returned NaN-boxed with POINTER_TAG by codegen.
//! A handle ID below 0x1000 is used so callers can distinguish a "real proxy"
//! from a raw heap pointer if needed. A revoked proxy has its `revoked` flag
//! flipped AND its target/handler slots detached (nulled) so the wrapped
//! object graphs can be collected; subsequent operations throw the
//! revoked-proxy TypeError.
//!
//! We deliberately do NOT patch generic object.rs/field dispatch — Perry
//! codegen rewrites known Proxy locals to ProxyGet/ProxySet/etc. variants at
//! HIR lowering time, which route through the entry points here.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

mod apply_construct;
pub use apply_construct::{call_proxy_value_with_this, js_proxy_apply, js_proxy_construct};
pub(crate) use apply_construct::{is_callable_function, is_constructor_function};
mod has_delete;
pub(crate) use has_delete::reflect_ordinary_delete_property_key;
pub use has_delete::{js_proxy_delete, js_proxy_has};
mod invariants;
mod put_value;
pub use put_value::{js_proxy_set, js_put_value_set};
pub(crate) use put_value::{js_put_value_set_ic_miss, proxy_set_with_receiver};
mod json;
mod metadata;
mod own_keys;
mod prototype;
mod reflect;
mod reflect_misc;
pub(crate) use reflect_misc::js_proxy_get_prototype_of;
pub use reflect_misc::{
    js_reflect_apply, js_reflect_construct, js_reflect_define_property,
    js_reflect_get_prototype_of, js_reflect_is_extensible, js_reflect_own_keys,
    js_reflect_prevent_extensions,
};

pub use own_keys::js_proxy_own_keys;
pub(crate) use own_keys::{
    proxy_enum_own_keys, proxy_own_property_names, proxy_own_property_symbols,
};
pub use prototype::js_reflect_set_prototype_of;

pub(crate) use json::{
    js_proxy_checked_target, js_proxy_checked_target_for_is_array, js_proxy_own_keys_for_json,
};
pub use reflect::{
    js_reflect_delete, js_reflect_get, js_reflect_get_own_property_descriptor, js_reflect_has,
    js_reflect_set,
};

/// A single Proxy registry entry.
///
/// Revocation detaches: `js_proxy_revoke` stores 0 bits into `target` and
/// `handler` (spec: [[ProxyTarget]]/[[ProxyHandler]] become null) so the
/// wrapped graphs can die. Minor collections strongly scan live entries;
/// full collections instead discover proxy handles through traced slots and
/// prune entries whose handles were not observed. No valid proxy
/// target/handler is ever the all-zero-bits number `0.0` (both must be
/// objects), so 0 bits is an unambiguous detached sentinel. Every trap path
/// checks `revoked` before touching `target`/`handler`.
#[repr(C)]
pub struct ProxyEntry {
    pub target: f64,  // NaN-boxed target value; 0 bits once revoked
    pub handler: f64, // NaN-boxed handler object (raw f64 bits preserved); 0 bits once revoked
    pub revoked: bool,
    /// Whether the proxy's (possibly nested) [[ProxyTarget]] was callable at
    /// creation. Per spec a proxy has a [[Call]] internal method iff its
    /// target did AT CREATION, and `typeof` of a revoked proxy is unchanged —
    /// so callability must be snapshotted, not recomputed from `target`
    /// (which revocation nulls).
    pub callable: bool,
}

thread_local! {
    /// id -> entry. Index 0 is reserved so we never return a null handle.
    static PROXIES: RefCell<Vec<Option<Box<ProxyEntry>>>> = RefCell::new(vec![None]);
    /// Backing store for `Reflect.{define,get,has,delete}Metadata` and friends.
    ///
    /// IMPORTANT: keys are raw NaN-box bits of the target value. For the
    /// canary scope (Nest-style DI) targets are always `ClassRef`s
    /// (INT32_TAG | class_id) and method-descriptor `.value` closures, both of
    /// which have stable bit patterns across the program lifetime. Regular
    /// heap-pointer targets are NOT GC-tracked here, so under the generational
    /// evacuating GC their entries become stale if the underlying object
    /// moves. If/when general object metadata becomes load-bearing, register
    /// a scanner that rewrites `target_bits` during GC fixup (similar to the
    /// 9 existing scanners in gc.rs).
    static REFLECT_METADATA: RefCell<HashMap<MetadataKey, f64>> = RefCell::new(HashMap::new());
    /// Live proxy ids observed by the current full GC trace. `None` outside a
    /// full trace; minors continue to root every registry entry strongly.
    static PROXY_FULL_TRACE_LIVE: RefCell<Option<Vec<bool>>> = const { RefCell::new(None) };
    /// Hot reject-path gate: collector funnels test this before decoding a
    /// proxy-band payload, so ordinary marking pays one TLS boolean branch.
    static PROXY_FULL_TRACE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// Monotone liveness counter for diagnostics and non-vacuous tests.
    static PROXY_GC_RECLAIMED_TOTAL: Cell<u64> = const { Cell::new(0) };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MetadataKey {
    target_bits: u64,
    key: String,
    property_key: Option<String>,
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Tag bits high enough to live inside a 48-bit pointer slot but low enough
/// that real heap pointers never collide. Keep proxies near the top of the
/// runtime's small-handle band so Web Fetch handles can occupy a broad
/// disjoint range below this without sharing visible `POINTER_TAG | id` bits
/// with a proxy. Any operation on a proxy MUST go through the Proxy* dispatch
/// helpers in this module. The band boundary is owned by
/// `value::addr_class` (`PROXY_ID_BAND_START`).
const PROXY_TAG_BASE: u64 = crate::value::addr_class::PROXY_ID_BAND_START as u64;

/// Number of ids the revocable-Proxy band can encode: `HANDLE_BAND_MAX -
/// PROXY_ID_BAND_START`, i.e. **65,536**, of which id 0 is reserved so a
/// handle is never the bare band base.
///
/// This is a hard ceiling, not a soft one. `encode_proxy_id` maps id `n` to
/// `PROXY_TAG_BASE + n`, so the first id at or above this length encodes to
/// `HANDLE_BAND_MAX` — a payload that `addr_class::is_proxy_id_band` rejects
/// and `addr_class::is_above_handle_band` **accepts as a dereferenceable heap
/// address**. Minting one is therefore a memory-safety bug rather than a lost
/// proxy: the 65,536th `new Proxy(...)` in a thread used to hand back a value
/// that the very next property read dereferenced, SIGSEGV (#8213).
///
/// Every other handle band already refuses to allocate past its end
/// (`common/handle.rs`, `fetch/mod.rs` both panic). The Proxy band was the
/// one without a guard — and the only one whose ids are minted straight from
/// user code with no matching free/close call, so it is also the only one a
/// long-running program reaches by simply staying up (#8213 measured ~4
/// proxies per HTTP request on a warm Next.js App Route, i.e. this ceiling
/// lands after roughly 16k requests).
const PROXY_ID_BAND_LEN: u64 = (crate::value::addr_class::HANDLE_BAND_MAX
    - crate::value::addr_class::PROXY_ID_BAND_START) as u64;

#[cfg(test)]
thread_local! {
    /// Test-only shrink of [`PROXY_ID_BAND_LEN`] so the exhaustion boundary
    /// can be walked without allocating 65k objects. Thread-local, and the
    /// test harness gives every test its own thread, so it cannot leak into
    /// another test. Never set outside `cfg(test)`.
    static PROXY_ID_BAND_LEN_OVERRIDE: std::cell::Cell<Option<u64>> =
        const { std::cell::Cell::new(None) };
}

#[inline]
fn proxy_id_band_len() -> u64 {
    #[cfg(test)]
    {
        if let Some(len) = PROXY_ID_BAND_LEN_OVERRIDE.with(|c| c.get()) {
            return len;
        }
    }
    PROXY_ID_BAND_LEN
}

/// Reserve the id a registry of `len` entries would hand out next, or `None`
/// when that id would fall outside the band.
///
/// Split out from [`js_proxy_new`] because the refusal is the testable half:
/// the throw itself ends the process when no `try` is open, so the boundary
/// is asserted here instead.
fn reserve_proxy_id(len: usize) -> Option<u64> {
    let id = len as u64;
    (id < proxy_id_band_len()).then_some(id)
}

/// The band is full. Throw a catchable `RangeError` rather than mint an
/// out-of-band id — same trade as `error::throw_allocation_failed` (#5067):
/// a program that can catch it keeps running, and one that cannot gets a
/// named error instead of a segfault.
#[cold]
fn throw_proxy_band_exhausted() -> ! {
    let msg = format!(
        "Too many proxies: this thread's Proxy registry is full ({} entries); \
         Perry never reclaims a Proxy registry slot",
        PROXY_ID_BAND_LEN - 1
    );
    let handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_rangeerror_new(handle);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn encode_proxy_id(id: u64) -> i64 {
    (PROXY_TAG_BASE + id) as i64
}

fn decode_proxy_id(raw: i64) -> Option<u64> {
    let raw = raw as u64;
    if raw < PROXY_TAG_BASE {
        return None;
    }
    // Reject payloads at or past the band end so this decoder and
    // `addr_class::is_proxy_id_band` agree about what a proxy id is. They used
    // to disagree above the band: `lookup` accepted anything below 4 GiB while
    // every addr-class consumer read the same payload as a heap address
    // (#8213). `reserve_proxy_id` makes such an id unmintable; this keeps the
    // two classifications from drifting apart again.
    if raw >= crate::value::addr_class::HANDLE_BAND_MAX as u64 {
        return None;
    }
    let id = raw - PROXY_TAG_BASE;
    if id == 0 {
        return None;
    }
    Some(id)
}

/// Look up a proxy by NaN-boxed value. Validates that the value is
/// pointer-tagged with a low-48 payload inside the proxy-id range AND that
/// the id corresponds to a registered entry, so a regular heap pointer
/// whose lower bits happen to fall in the encoding range doesn't get
/// misclassified as a proxy.
fn lookup(proxy_boxed: f64) -> Option<u64> {
    let bits = proxy_boxed.to_bits();
    // Proxies are always POINTER_TAG.
    if (bits >> 48) != (POINTER_TAG >> 48) {
        return None;
    }
    let lower48 = bits & POINTER_MASK;
    // Real heap pointers live >= 0x1_0000_0000 on macOS/iOS arenas.
    if lower48 >= 0x1_0000_0000 {
        return None;
    }
    let id = decode_proxy_id(lower48 as i64)?;
    // A collected slot remains a tombstone. Treating it as a non-proxy would
    // hand the small id-band payload to generic object code, which may
    // dereference it; fail loudly if the collector ever under-approximates.
    let status = PROXIES.with(|p| {
        let v = p.borrow();
        match v.get(id as usize) {
            Some(Some(_)) => 1,
            Some(None) => 2,
            None => 0,
        }
    });
    match status {
        1 => Some(id),
        2 => collected_return(),
        _ => None,
    }
}

/// Keep a proxy id visible to any full collection that starts while a native
/// proxy operation is running. The returned scope owns the slot; the handle
/// itself need not be retained because slots live until the scope is dropped.
fn pin_proxy_for_native_call(proxy_boxed: f64) -> crate::gc::RuntimeHandleScope {
    let scope = crate::gc::RuntimeHandleScope::new();
    let _ = scope.root_nanbox_f64(proxy_boxed);
    scope
}

/// Arm proxy-id observation for a full trace. The registry becomes weak for
/// the mark phase until [`gc_finish_full_trace`] prunes unobserved entries.
pub(crate) fn gc_begin_full_trace() {
    let (len, has_live_entries) = PROXIES.with(|p| {
        let proxies = p.borrow();
        (proxies.len(), proxies.iter().any(Option::is_some))
    });
    PROXY_FULL_TRACE_LIVE.with(|live| {
        assert!(live.borrow().is_none(), "proxy full trace already active");
        *live.borrow_mut() = Some(vec![false; len]);
    });
    PROXY_FULL_TRACE_ACTIVE.with(|active| active.set(has_live_entries));
}

#[inline(always)]
pub(crate) fn gc_full_trace_active() -> bool {
    PROXY_FULL_TRACE_ACTIVE.with(Cell::get)
}

/// Observe one bits value from a collector-owned tracing funnel. Returns true
/// when it names an existing proxy slot (live or a collected tombstone).
/// First observation marks the live entry's target/handler immediately; this
/// closes the cycle without making the whole registry a strong root.
pub(crate) fn gc_observe_traced_value(bits: u64, valid_ptrs: &crate::gc::ValidPointerSet) -> bool {
    if !gc_full_trace_active() || (bits & !POINTER_MASK) != POINTER_TAG {
        return false;
    }
    let payload = (bits & POINTER_MASK) as usize;
    if !crate::value::addr_class::is_proxy_id_band(payload) {
        return false;
    }
    let Some(id) = decode_proxy_id(payload as i64) else {
        return false;
    };
    let entry = PROXIES.with(|proxies| {
        let proxies = proxies.borrow();
        proxies.get(id as usize).map(|slot| {
            slot.as_ref()
                .map(|entry| (entry.target.to_bits(), entry.handler.to_bits()))
        })
    });
    let Some(entry) = entry else {
        return false;
    };
    let first_observation = PROXY_FULL_TRACE_LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let live = live.as_mut().expect("proxy observation outside full trace");
        if id as usize >= live.len() {
            live.resize(id as usize + 1, false);
        }
        let first = !live[id as usize];
        live[id as usize] = true;
        first
    });
    if first_observation {
        if let Some((target, handler)) = entry {
            crate::gc::try_mark_value_or_raw(target, valid_ptrs);
            crate::gc::try_mark_value_or_raw(handler, valid_ptrs);
        }
    }
    true
}

/// End a full proxy trace and tombstone every registry entry whose handle was
/// not observed. Returns the number reclaimed in this pass.
pub(crate) fn gc_finish_full_trace() -> usize {
    PROXY_FULL_TRACE_ACTIVE.with(|active| active.set(false));
    let live = PROXY_FULL_TRACE_LIVE.with(|state| {
        state
            .borrow_mut()
            .take()
            .expect("proxy full trace was not active")
    });
    let (reclaimed, remaining, slots) = PROXIES.with(|proxies| {
        let mut proxies = proxies.borrow_mut();
        let mut reclaimed = 0usize;
        for (id, slot) in proxies.iter_mut().enumerate().skip(1) {
            if slot.is_some() && !live.get(id).copied().unwrap_or(false) {
                slot.take();
                reclaimed += 1;
            }
        }
        let remaining = proxies.iter().flatten().count();
        (reclaimed, remaining, proxies.len().saturating_sub(1))
    });
    let total = PROXY_GC_RECLAIMED_TOTAL.with(|counter| {
        let total = counter.get().saturating_add(reclaimed as u64);
        counter.set(total);
        total
    });
    if crate::gc::gc_diag_enabled() {
        eprintln!(
            "[gc-proxy-registry] live={remaining} tombstones={} slots={slots} reclaimed={reclaimed} reclaimed_total={total}",
            slots.saturating_sub(remaining),
        );
    }
    reclaimed
}

/// Cancel observation when an in-progress GC cycle is dropped.
pub(crate) fn gc_abort_full_trace() {
    // A parked budgeted cycle can be dropped by the GC cycle TLS destructor.
    // Darwin does not guarantee an order between independent TLS destructors,
    // so the proxy trace cells may already be unavailable during thread exit.
    // There is no trace left to observe once that thread is gone; ordinary
    // cycle cancellation still takes the same cleanup path.
    let _ = PROXY_FULL_TRACE_ACTIVE.try_with(|active| active.set(false));
    let _ = PROXY_FULL_TRACE_LIVE.try_with(|live| {
        live.borrow_mut().take();
    });
}

/// Allocate a new proxy. Returns the NaN-boxed POINTER_TAG value holding the
/// encoded proxy id in the low bits.
/// `new Proxy(target, handler)` requires both arguments to be objects
/// (functions count as objects). Node throws
/// `TypeError: Cannot create proxy with a non-object as target or handler`
/// when either is a primitive or nullish. (#2846)
fn proxy_arg_is_object(value: f64) -> bool {
    let bits = value.to_bits();
    let top = bits >> 48;
    // POINTER_TAG heap value (object / function / array).
    if top == 0x7FFD {
        let ptr = (bits & POINTER_MASK) as usize;
        if ptr < 0x1000 {
            return false;
        }
        // A Symbol is a POINTER_TAG value too (registered side-table), but it
        // is a primitive, not an object — `new Proxy(Symbol(), {})` and
        // `new Proxy({}, Symbol())` must throw TypeError.
        if crate::symbol::is_registered_symbol(ptr) {
            return false;
        }
        return true;
    }
    // Module-level raw-I64 object/array pointers (top16 == 0).
    if top == 0 && bits > 0x10000 {
        return true;
    }
    // Class refs (INT32-tagged constructors, top16 == 0x7FFE) are callable
    // objects and are valid proxy targets/handlers.
    if top == 0x7FFE {
        return true;
    }
    false
}

fn throw_proxy_non_object() -> ! {
    let msg = "Cannot create proxy with a non-object as target or handler";
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed)
}

/// Creation-time callability snapshot for a proxy's target: a nested proxy
/// contributes ITS creation-time snapshot (per spec, each proxy's [[Call]]
/// presence is fixed when that proxy is created), anything else is asked
/// directly.
fn target_callable_at_creation(target: f64) -> bool {
    match lookup(target) {
        Some(id) => PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.callable)
                .unwrap_or(false)
        }),
        None => crate::object::value_is_callable(target),
    }
}

#[no_mangle]
pub extern "C" fn js_proxy_new(target: f64, handler: f64) -> f64 {
    // #2846: validate both arguments are objects before allocating.
    if !proxy_arg_is_object(target) || !proxy_arg_is_object(handler) {
        throw_proxy_non_object();
    }
    let callable = target_callable_at_creation(target);
    // Reserve BEFORE taking the mutable borrow: `throw_proxy_band_exhausted`
    // allocates a JS error, which can collect, and `scan_proxy_roots_mut`
    // borrows `PROXIES` mutably — throwing under an open borrow would panic
    // the collector (and a caught throw would leave the registry borrowed for
    // the life of the thread).
    let Some(reserved) = reserve_proxy_id(PROXIES.with(|p| p.borrow().len())) else {
        throw_proxy_band_exhausted();
    };
    PROXIES.with(|p| {
        let mut v = p.borrow_mut();
        let id = v.len() as u64;
        debug_assert_eq!(id, reserved, "nothing may allocate a proxy id in between");
        v.push(Some(Box::new(ProxyEntry {
            target,
            handler,
            revoked: false,
            callable,
        })));
        // A proxy born during a sliced full trace was absent from the begin
        // snapshot. Arm observation before its handle can be published into a
        // root/heap slot; the incremental write barrier will record it.
        if PROXY_FULL_TRACE_LIVE.with(|live| live.borrow().is_some()) {
            PROXY_FULL_TRACE_ACTIVE.with(|active| active.set(true));
        }
        let encoded = encode_proxy_id(id) as u64;
        f64::from_bits(POINTER_TAG | (encoded & POINTER_MASK))
    })
}

/// Revoke a proxy. Subsequent operations will return TAG_UNDEFINED or fire an
/// exception where the compiler inserts one. Detaches the entry's
/// target/handler (stores 0 bits — spec: [[ProxyTarget]]/[[ProxyHandler]]
/// become null) so the graphs they root can be collected; every trap path
/// throws the revoked-proxy TypeError off the `revoked` flag before reading
/// either slot. Idempotent.
#[no_mangle]
pub extern "C" fn js_proxy_revoke(proxy_boxed: f64) {
    if let Some(id) = lookup(proxy_boxed) {
        PROXIES.with(|p| {
            if let Some(Some(entry)) = p.borrow_mut().get_mut(id as usize) {
                entry.revoked = true;
                entry.target = f64::from_bits(0);
                entry.handler = f64::from_bits(0);
            }
        });
    }
}

/// Query whether `proxy_boxed` is a currently-revoked proxy. Returns 1 if so.
#[no_mangle]
pub extern "C" fn js_proxy_is_revoked(proxy_boxed: f64) -> i32 {
    if let Some(id) = lookup(proxy_boxed) {
        return PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| if e.revoked { 1i32 } else { 0 })
                .unwrap_or(0)
        });
    }
    0
}

/// Query whether the given NaN-boxed value is a Proxy instance. Returns 1/0.
#[no_mangle]
pub extern "C" fn js_proxy_is_proxy(value: f64) -> i32 {
    if lookup(value).is_some() {
        1
    } else {
        0
    }
}

/// Resolve the backing object used by Perry's private-element storage without
/// invoking any Proxy trap.  Private names use the object's internal
/// [[PrivateElements]] list in ECMAScript; they are deliberately not ordinary
/// `[[Get]]`/`[[Set]]` operations.  Perry's Proxy is a stable registry handle,
/// so its private storage lives on the backing target and all private-element
/// entry points consistently resolve through this helper.
pub(crate) fn private_element_receiver(mut value: f64) -> f64 {
    for _ in 0..32 {
        let Some(id) = lookup(value) else {
            return value;
        };
        let (target, revoked) = PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|entry| entry.as_ref())
                .map(|entry| (entry.target, entry.revoked))
                .unwrap_or((f64::from_bits(TAG_UNDEFINED), false))
        });
        if revoked {
            revoked_return_with_message(
                "Cannot access a private element on a proxy that has been revoked",
            );
        }
        value = target;
    }
    value
}

/// `IsArray`'s Proxy branch (ECMA-262 §7.2.2). If `value` is a live Proxy,
/// returns `Some(target)` so the caller can recurse on the target; if the Proxy
/// has been revoked, throws a `TypeError` (does not return). Returns `None` for
/// any non-Proxy value, so the caller falls back to its ordinary array check.
pub(crate) fn is_array_proxy_step(value: f64) -> Option<f64> {
    let id = lookup(value)?;
    let (target, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.revoked))
            .unwrap_or((f64::from_bits(TAG_UNDEFINED), false))
    });
    if revoked {
        revoked_return_with_message("Cannot perform 'IsArray' on a proxy that has been revoked");
    }
    Some(target)
}

/// Whether a Proxy value's (possibly nested) [[ProxyTarget]] is callable —
/// the predicate behind `typeof proxyOfFn === "function"` and
/// `Function.prototype.toString` accepting a proxy receiver. Reads the
/// creation-time `callable` snapshot (which already resolved through nested
/// proxies), so callability survives revocation (per spec, `typeof` of a
/// revoked proxy is unchanged) even though revoke nulls the recorded target.
pub(crate) fn proxy_wraps_callable(value: f64) -> bool {
    match lookup(value) {
        Some(id) => PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.callable)
                .unwrap_or(false)
        }),
        None => crate::object::value_is_callable(value),
    }
}

/// Return the proxy's target (for Proxy.revocable.proxy revocation checks).
/// A revoked proxy's target slot is detached (0 bits) — report it as
/// `undefined`, the same "no target" convention non-proxies get.
#[no_mangle]
pub extern "C" fn js_proxy_target(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.target)
                .filter(|t| t.to_bits() != 0)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Return the proxy's handler for `util.inspect(..., { showProxy: true })`.
/// A revoked proxy's handler slot is detached (0 bits) — report `undefined`.
#[no_mangle]
pub extern "C" fn js_proxy_handler(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.handler)
                .filter(|h| h.to_bits() != 0)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Helper: fetch the trap closure from the handler object by name. Returns
/// TAG_UNDEFINED if the handler has no such trap.
fn handler_trap(handler: f64, trap_name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(trap_name.as_ptr(), trap_name.len() as u32);
    let obj_ptr = extract_pointer(handler.to_bits()) as *const crate::ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_field_by_name_f64(obj_ptr, key)
}

/// Raise a "proxy revoked" TypeError via `js_throw`. Does not return.
fn revoked_return() -> f64 {
    revoked_return_with_message("Cannot perform operation on a proxy that has been revoked")
}

fn collected_return() -> ! {
    let _ = revoked_return_with_message(
        "Cannot perform operation on a proxy that has been garbage collected",
    );
    unreachable!("js_throw returned from collected proxy TypeError")
}

fn revoked_return_with_message(msg: &str) -> f64 {
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed);
}

fn is_callable(value: f64) -> bool {
    // Treat any pointer-tagged value as potentially callable. Inside the
    // closure call the runtime will no-op if the pointer isn't a closure.
    let bits = value.to_bits();
    let tag = bits & !POINTER_MASK;
    tag == POINTER_TAG && (bits & POINTER_MASK) != 0
}

fn closure_from(value: f64) -> *const crate::ClosureHeader {
    let bits = value.to_bits();
    ((bits & POINTER_MASK) as usize) as *const crate::ClosureHeader
}

/// Coerce a trap return value to a NaN-boxed boolean (`ToBoolean`), as the
/// `Reflect.{set,deleteProperty,defineProperty,preventExtensions}` paths must
/// return the trap's boolean result rather than discarding it.
fn nanbox_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

fn coerce_trap_bool(value: f64) -> f64 {
    nanbox_bool(crate::value::js_is_truthy(value) != 0)
}

/// Invoke a present (already-confirmed-callable) handler trap with the handler
/// bound as the trap's `this` (ECMA-262: traps are called as
/// `Call(trap, handler, args)`). Object-literal/method traps read `this` from a
/// reserved closure slot, while free-function traps fall back to
/// `IMPLICIT_THIS`; we set both so either style observes the handler. Mirrors
/// the apply/construct/getOwnPropertyDescriptor trap-call dance, which the
/// per-trap paths (get/set/has/deleteProperty/defineProperty/…) previously
/// skipped — they called the trap with the wrong `this` and, for get/set,
/// dropped the trailing `receiver` argument.
fn call_trap(handler: f64, trap: f64, args: &[f64]) -> f64 {
    let rebound = crate::closure::clone_closure_rebind_this(trap.to_bits(), handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy trap is not a function");
    }
    let undef = f64::from_bits(TAG_UNDEFINED);
    let a = |i: usize| -> f64 { args.get(i).copied().unwrap_or(undef) };
    let prev = crate::object::js_implicit_this_set(handler);
    let result = match args.len() {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, a(0)),
        2 => js_closure_call2(closure, a(0), a(1)),
        3 => js_closure_call3(closure, a(0), a(1), a(2)),
        _ => crate::closure::js_closure_call4(closure, a(0), a(1), a(2), a(3)),
    };
    crate::object::js_implicit_this_set(prev);
    result
}

/// Throw `TypeError: Reflect.<op> called on non-object`. Does not return.
fn reflect_non_object_typeerror(op: &str) -> f64 {
    let msg = format!("Reflect.{op} called on non-object");
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed);
}

/// Throw a `TypeError` with an arbitrary message. Does not return.
fn throw_type_error(msg: &str) -> f64 {
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed)
}

/// `String(value)` rendering of a JS value, for diagnostic messages that
/// embed the offending value (e.g. Node's `"1 is not a constructor"` and
/// the proxy construct-trap `"… non-object ('1')"`). Returns an empty
/// string on a null/unrenderable value. (#2768)
pub(crate) fn value_display_string(value: f64) -> String {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let str_ptr = crate::value::js_jsvalue_to_string(value);
    if str_ptr.is_null() {
        return String::new();
    }
    let nb = f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(nb, &mut scratch) {
        if !ptr.is_null() && len > 0 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            return String::from_utf8_lossy(bytes).into_owned();
        }
    }
    String::new()
}

fn reflect_value_is_symbol(value: f64) -> bool {
    let bits = value.to_bits();
    (bits >> 48) == (POINTER_TAG >> 48)
        && (bits & POINTER_MASK) >= 0x1_0000_0000
        && unsafe { crate::symbol::js_is_symbol(value) != 0 }
}

/// Is `value` a Reflect-acceptable object? Heap objects, class refs (callable
/// constructors), and proxies all count. Primitives / null / undefined do not.
fn reflect_value_is_object(value: f64) -> bool {
    if lookup(value).is_some() {
        return true;
    }
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if top16 == (POINTER_TAG >> 48) {
        let lower48 = bits & POINTER_MASK;
        // A handle-backed native object — Request / Response / Headers, sockets,
        // streams — is a POINTER_TAG'd small id, not a heap `ObjectHeader`. It is
        // still an OBJECT to JS, so `Reflect.get(request, k)` must not be refused:
        // the sub-4GB cutoff below classified every one of them as a non-object and
        // threw `TypeError: Reflect.get called on non-object`. Next's app-route
        // runtime wraps the request in a Proxy whose `get` trap forwards through
        // `Reflect.get(target, …)`, so every authenticated route 500'd.
        if crate::value::addr_class::is_handle_band(lower48 as usize)
            || crate::value::addr_class::is_stream_id_band(lower48 as usize)
        {
            return lower48 != 0;
        }
        if lower48 < 0x1_0000_0000 {
            return false;
        }
        if reflect_value_is_symbol(value) {
            return false;
        }
    }
    if crate::object::js_value_is_heap_object(value) {
        return true;
    }
    // Class refs (INT32-tagged constructors) are callable objects.
    top16 == 0x7FFE
}

/// `CreateListFromArrayLike(value)` — collect indexed `0..length` properties of
/// an array-like object into an owned `Vec<f64>`. Throws `TypeError` for a
/// non-object `value`, matching Node's `CreateListFromArrayLike called on
/// non-object`. Plain Arrays use the fast array accessors; any other object
/// reads `.length` then `[0]..[length-1]` via the field getter.
fn create_list_from_array_like(value: f64) -> Vec<f64> {
    // Fast path: a real Array.
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let is_pointer = top16 == 0x7FFD || (top16 == 0 && bits > 0x10000);
    if is_pointer {
        let ptr = (bits & POINTER_MASK) as usize;
        // `arguments` is an ordinary ObjectHeader backed by the arguments
        // registry, not a GC_TYPE_ARRAY. Reading it through the generic object
        // field path below loses its indexed bindings, so
        // `Reflect.apply(target, thisArg, arguments)` constructed an empty
        // argument list. Next 16's ProxyTracer forwards startActiveSpan with
        // exactly that shape; its callback was consequently never invoked and
        // the production App Route request remained pending (#8036).
        if let Some(values) = unsafe {
            crate::object::arguments_object_to_vec(ptr as *const crate::object::ObjectHeader)
        } {
            return values;
        }
        // #7531: `value` is `argumentsList` from `Reflect.apply(target,
        // thisArg, argumentsList)` / `Reflect.construct` -- caller-supplied,
        // so it can be a fetch/zlib/proxy/common-registry handle id under
        // the same POINTER_TAG as a real Array. The old floor
        // (`GC_HEADER_SIZE + 0x1000`) sits below every handle band and had
        // no other guard before the deref below -- a handle reached
        // `addr - GC_HEADER_SIZE` unconditionally.
        if crate::value::addr_class::is_plausible_heap_addr(ptr) {
            unsafe {
                let gc =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let len = crate::array::js_array_length(arr) as usize;
                    let mut out = Vec::with_capacity(len);
                    for i in 0..len {
                        let v = crate::array::js_array_get(arr, i as u32);
                        out.push(f64::from_bits(v.bits()));
                    }
                    return out;
                }
            }
        }
    }
    if !reflect_value_is_object(value) {
        throw_type_error("CreateListFromArrayLike called on non-object");
    }
    // General array-like object: read `.length`, then `[0]..[length-1]`.
    let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let len_val = {
        let obj_ptr = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
        if obj_ptr.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            crate::object::js_object_get_field_by_name_f64(obj_ptr, len_key)
        }
    };
    let len_f = f64::from_bits(len_val.to_bits());
    let len = if len_f.is_finite() && len_f > 0.0 {
        len_f as usize
    } else {
        0
    };
    let mut out = Vec::with_capacity(len);
    let obj_ptr = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
    for i in 0..len {
        let idx_str = i.to_string();
        let key = crate::string::js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key);
        out.push(v);
    }
    out
}

/// Invoke a callable `f64` value with the supplied positional args and an
/// explicit `thisArg` binding, throwing `TypeError` if `f` is not callable.
/// Used by `Reflect.apply`. `thisArg` flows through `IMPLICIT_THIS` so free
/// functions reading `this` observe it.
fn call_with_this_and_args(f: f64, this_arg: f64, args: &[f64]) -> f64 {
    // A concise/object-literal method reads `this` from a baked capture slot,
    // not IMPLICIT_THIS; rebind to the explicit `Reflect.apply` receiver so it
    // is honored (no-op for arrows / plain fns / bound fns).
    let f = crate::closure::rebind_explicit_this(f, this_arg);
    let closure = closure_from(f);
    if closure.is_null() {
        return throw_type_error("Reflect.apply target is not a function");
    }
    let prev = crate::object::js_implicit_this_set(this_arg);
    let a = |i: usize| -> f64 {
        args.get(i)
            .copied()
            .unwrap_or(f64::from_bits(TAG_UNDEFINED))
    };
    let result = match args.len() {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, a(0)),
        2 => js_closure_call2(closure, a(0), a(1)),
        3 => js_closure_call3(closure, a(0), a(1), a(2)),
        _ => crate::closure::js_closure_call4(closure, a(0), a(1), a(2), a(3)),
    };
    crate::object::js_implicit_this_set(prev);
    result
}

/// Detect the runtime's "null object" sentinel returned by
/// `js_native_call_method` when a method lookup falls off the end.
/// `proxy[key]` — if handler.get exists, call it with (target, key);
/// otherwise fetch the field from the target directly via the generic path.
#[no_mangle]
pub extern "C" fn js_proxy_get(proxy_boxed: f64, key: f64) -> f64 {
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    // `[[Get]] ( P, Receiver )` receives an already-computed property key P, but
    // codegen calls this helper with the raw index value for a computed read on
    // a statically-known proxy (`proxy[10]` lowers to
    // `js_proxy_get(proxy, 10.0)`). Apply `ToPropertyKey` so a numeric index is
    // seen by the trap as the canonical string key (`10` -> `"10"`) and the
    // forward-to-target path below stringifies consistently. Symbols and
    // strings pass through unchanged. Without this the get trap received a raw
    // number and key-equality checks (`key === "10"`) silently failed (test262
    // Proxy/get/trap-is-{null,undefined}-target-is-proxy `proxy[10]`). A key
    // that is already a string (the overwhelmingly common `proxy.foo` case) or
    // a symbol is left untouched, so this only pays `ToPropertyKey` for the
    // numeric / object-index forms.
    let key = {
        let tag = key.to_bits() & 0xFFFF_0000_0000_0000;
        let is_string_key =
            tag == crate::value::STRING_TAG || tag == crate::value::SHORT_STRING_TAG;
        if is_string_key || unsafe { crate::symbol::js_is_symbol(key) } != 0 {
            key
        } else {
            unsafe { crate::object::js_to_property_key(key) }
        }
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    let trap = handler_trap(handler, "get");
    if is_callable(trap) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target_h = scope.root_nanbox_f64(target);
        let key_h = scope.root_nanbox_f64(key);
        let result = call_trap(
            handler,
            trap,
            &[
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
                proxy_boxed,
            ],
        );
        let result_h = scope.root_nanbox_f64(result);
        invariants::enforce_get_invariant(
            target_h.get_nanbox_f64(),
            key_h.get_nanbox_f64(),
            result_h.get_nanbox_f64(),
        );
        return result_h.get_nanbox_f64();
    }
    // No get trap — forward to the target's `[[Get]]`. A proxy target must
    // recurse through proxy dispatch rather than `target_get`, which would deref
    // the fake pointer.
    if lookup(target).is_some() {
        return js_proxy_get(target, key);
    }
    // `p.apply` / `p.call` / `p.bind` VALUE reads on a callable-wrapping
    // proxy resolve to Function.prototype's methods with the PROXY as the
    // receiver — reify a bound method so a later invocation dispatches
    // `js_native_call_method(proxy, "call", …)` and routes through the
    // proxy's [[Call]] (apply trap). Reading off the target instead would
    // bypass the trap. (Test262 proxy-toString reads `.apply` as a value;
    // Function.prototype.toString on the reified method is the
    // NativeFunction form.)
    if crate::object::value_is_callable(target) {
        if let Some(name) = key_to_rust_string(key) {
            let method: Option<&'static [u8]> = match name.as_str() {
                "apply" => Some(b"apply"),
                "call" => Some(b"call"),
                "bind" => Some(b"bind"),
                _ => None,
            };
            if let Some(m) = method {
                // Only when the target has no OWN override of the slot.
                let t_ptr = extract_pointer(target.to_bits()) as usize;
                if !crate::closure::closure_has_own_dynamic_prop(t_ptr, &name) {
                    return unsafe { crate::closure::reify_function_method_value(proxy_boxed, m) };
                }
            }
        }
    }
    target_get(target, key)
}

/// Resolve the ultimate target when a Proxy wraps a class constructor. Used
/// by method-call dispatch to bind a static method's visible `this` to the
/// Proxy receiver while retaining the target class as its lexical owner.
pub(crate) fn proxy_target_class_id(mut value: f64) -> Option<u32> {
    let mut depth = 0usize;
    while let Some(id) = lookup(value) {
        value = PROXIES.with(|proxies| {
            proxies
                .borrow()
                .get(id as usize)
                .and_then(|entry| entry.as_ref())
                .map(|entry| entry.target)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        });
        depth += 1;
        if depth >= 32 {
            return None;
        }
    }
    if let Some(class_id) = crate::object::class_ref_id(value) {
        return Some(class_id);
    }
    if crate::object::is_class_object_value(value) {
        let raw = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
        let class_id = crate::object::js_object_get_class_id(raw);
        return (class_id != 0).then_some(class_id);
    }
    None
}

/// Extract a raw heap pointer (48-bit) from either a NaN-boxed value
/// (POINTER_TAG / STRING_TAG) or a raw i64/f64-reinterpreted pointer
/// (module-level globals store Arrays/Objects as raw I64s, not NaN-boxed).
fn extract_pointer(bits: u64) -> u64 {
    let top = bits >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        bits & POINTER_MASK
    } else if top == 0 {
        // Raw untagged pointer (module-level I64 global).
        bits
    } else {
        0
    }
}

fn small_handle_from_value(value: f64) -> Option<i64> {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == (POINTER_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as i64;
        if raw > 0 && (raw as u64) < PROXY_TAG_BASE {
            return Some(raw);
        }
    } else if top == 0 && crate::value::addr_class::is_small_handle(bits as usize) {
        return Some(bits as i64);
    }
    None
}

fn set_handle_property(target: f64, key: f64, value: f64) -> Option<bool> {
    let handle = small_handle_from_value(target)?;
    let Some(name) = key_to_rust_string(key) else {
        // A SYMBOL-keyed write on a small native handle (e.g. the
        // @hono/node-server `incoming[wrapBodyStream] = true` on the HTTP
        // IncomingMessage handle). The handle is not a heap ObjectHeader, so
        // it has no field storage; route the write to the per-object symbol
        // side table (keyed by the handle pointer, exactly like a plain
        // object) and report success. Returning `Some(false)` here made
        // strict-mode assignment throw `TypeError: Cannot assign to read only
        // property` and 500 every POST/PUT served by Hono's node adapter.
        if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
            unsafe { crate::symbol::js_object_set_symbol_property(target, key, value) };
            return Some(true);
        }
        return Some(false);
    };
    if let Some(dispatch) = crate::object::handle_property_set_dispatch() {
        unsafe { dispatch(handle, name.as_ptr(), name.len(), value) };
    }
    Some(true)
}

fn target_get_property_key(target: f64, property_key: f64) -> f64 {
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        return unsafe { crate::symbol::js_object_get_symbol_property(target, property_key) };
    }
    let key_ptr =
        crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
    if key_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Class constructors use Perry's INT32-tagged ClassRef representation,
    // not a heap pointer. Preserve those bits exactly as the ordinary dynamic
    // class-property path does; pointer extraction would turn the target into
    // null and make `new Proxy(C, {}).staticMethod` read as `undefined`.
    if crate::object::class_ref_id(target).is_some() {
        return crate::object::js_object_get_field_by_name_f64(
            target.to_bits() as *const crate::ObjectHeader,
            key_ptr,
        );
    }
    let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
}

fn target_get(target: f64, key: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    target_get_property_key(
        target_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
    )
}

/// `Reflect.set` with an explicit receiver: OrdinarySet(target, P, V,
/// receiver), boolean result NaN-boxed.
pub(crate) fn reflect_ordinary_set_with_receiver(
    target: f64,
    property_key: f64,
    value: f64,
    receiver: f64,
) -> f64 {
    nanbox_bool(ordinary_set_with_receiver(
        target,
        property_key,
        value,
        receiver,
    ))
}

fn target_set(target: f64, key: f64, value: f64) {
    // #6935: `js_to_property_key` runs a user `Symbol.toPrimitive` / `toString`
    // / `valueOf` for an object key (and allocates for every primitive one), so
    // it can trigger a GC that **evacuates** live objects. Both the `target`
    // receiver and the `value` being written into it were raw NaN-boxed Rust
    // locals across the call — a stale target dropped the write onto a
    // forwarding stub, a stale value stored a dangling pointer in a live
    // object. `target_get` already roots; this write sibling did not.
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_heap_word_u64(target.to_bits());
    let value_handle = scope.root_nanbox_f64(value);
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    let property_key = scope.root_nanbox_f64(property_key).get_nanbox_f64();
    let target = f64::from_bits(target_handle.get_heap_word_u64());
    let value = value_handle.get_nanbox_f64();
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(target, property_key, value);
        }
        return;
    }
    // #6943 audit: this `js_string_coerce` is provably INERT and needs no
    // rooting. `js_to_property_key` returns either a Symbol — taken by the
    // early return above — or `js_nanbox_string(heap_ptr)`, i.e. an
    // already-heap `STRING_TAG` value, which `js_string_coerce` hands straight
    // back without touching the allocator.
    let key_ptr = crate::builtins::js_string_coerce(property_key) as *const crate::StringHeader;
    if crate::object::class_ref_id(target).is_some() {
        // Preserve the INT32-tagged class-ref bits so class dynamic props
        // land in CLASS_DYNAMIC_PROPS instead of being pointer-extracted to 0.
        if !key_ptr.is_null() {
            crate::object::js_object_set_field_by_name(
                target.to_bits() as *mut crate::ObjectHeader,
                key_ptr,
                value,
            );
        }
        return;
    }
    let obj_addr = extract_pointer(target.to_bits()) as usize;
    if crate::closure::is_closure_ptr(obj_addr) {
        if let Some(name) = key_to_rust_string(property_key) {
            crate::closure::closure_set_dynamic_prop(obj_addr, &name, value);
        }
        return;
    }
    let obj_ptr = obj_addr as *mut crate::ObjectHeader;
    if obj_ptr.is_null() || key_ptr.is_null() {
        return;
    }
    crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, value);
}

fn raw_ptr_from_value(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if top16 == (POINTER_TAG >> 48) {
        bits & POINTER_MASK
    } else if top16 == 0 && bits > 0x10000 {
        bits
    } else {
        return None;
    } as usize;
    // #7531: `value` is a Proxy `target`/`key` candidate -- caller-supplied,
    // so it can be a fetch/zlib/proxy/common-registry handle id. The old
    // floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle band; the
    // local name `raw` also made this invisible to
    // `scripts/addr_class_inventory.py`'s `handle-floor` regex (it only
    // matches `ptr`/`addr`/`bits`-shaped identifiers), so this was debt the
    // ratchet could not see. `array_ptr_from_value` derefs
    // `raw - GC_HEADER_SIZE` right after this with only a magnitude-only
    // `is_valid_obj_ptr` guard in between, so a handle id reached that
    // deref.
    if !crate::value::addr_class::is_above_handle_band(raw) {
        return None;
    }
    Some(raw)
}

fn array_ptr_from_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let raw = raw_ptr_from_value(value)?;
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return None;
    }
    unsafe {
        let gc = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
            || (*gc).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
        {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

fn key_equals(key: f64, name: &[u8]) -> bool {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return false;
    };
    if ptr.is_null() || len as usize != name.len() {
        return false;
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) == name }
}

fn key_is_length(key: f64) -> bool {
    key_equals(key, b"length")
}

/// Does deleting `key` off `target` hit a non-configurable exotic own property
/// that lives outside the ordinary descriptor table? Covers an Array's `length`
/// and a plain (non-arrow, non-bound) function's `prototype` — both are
/// non-configurable, so `Reflect.deleteProperty` / `delete` must report failure.
fn is_non_configurable_exotic_own(target: f64, key: f64) -> bool {
    if array_ptr_from_value(target).is_some() && key_is_length(key) {
        return true;
    }
    if key_equals(key, b"prototype") {
        let raw = extract_pointer(target.to_bits()) as usize;
        if raw != 0 && crate::closure::is_closure_ptr(raw) {
            let closure = raw as *const crate::closure::ClosureHeader;
            // Arrow and bound functions have no own `prototype` slot at all, so
            // deleting it succeeds vacuously; only a plain function's `prototype`
            // is a non-configurable own property.
            if !crate::closure::closure_is_arrow(closure)
                && !crate::closure::closure_is_bound_method(closure)
            {
                return true;
            }
        }
    }
    false
}

fn parse_canonical_nonnegative_i32(bytes: &[u8]) -> Option<i32> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut value = 0u32;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((byte - b'0') as u32)?;
        if value > i32::MAX as u32 {
            return None;
        }
    }
    Some(value as i32)
}

fn integer_index_key(key: f64) -> Option<i32> {
    let jsval = crate::value::JSValue::from_bits(key.to_bits());
    if jsval.is_int32() {
        let index = jsval.as_int32();
        return (index >= 0).then_some(index);
    }
    if !key.is_nan() {
        return (key.is_finite() && key >= 0.0 && key.fract() == 0.0 && key <= i32::MAX as f64)
            .then_some(key as i32);
    }

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    parse_canonical_nonnegative_i32(bytes)
}

fn set_integer_indexed_exotic(target: f64, key: f64, value: f64) -> bool {
    let Some(index) = integer_index_key(key) else {
        return false;
    };
    let Some(raw) = raw_ptr_from_value(target) else {
        return false;
    };
    // #8149: `ArrayBuffer` / `SharedArrayBuffer` / `DataView` are registered
    // buffers, but they are NOT integer-indexed exotic objects — `dv[0] = 7`
    // creates an ORDINARY own property in node and leaves the byte at 0.
    // Answering `false` here hands the write back to `js_put_value_set`'s
    // ordinary `[[Set]]` walk, which would bit-cast the `BufferHeader`, so
    // store the expando directly and claim the write.
    if crate::buffer::is_non_indexed_buffer_view(raw) {
        crate::buffer::buffer_set_own_prop(raw, &index.to_string(), value);
        return true;
    }
    if crate::buffer::is_registered_buffer(raw) {
        crate::buffer::js_buffer_set(raw as *mut crate::buffer::BufferHeader, index, value as i32);
        return true;
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        crate::typedarray::js_typed_array_set(
            raw as *mut crate::typedarray::TypedArrayHeader,
            index,
            value,
        );
        return true;
    }
    false
}

#[derive(Clone, Copy)]
enum OwnSetDescriptor {
    Data { writable: bool },
    Accessor { setter_bits: u64 },
}

fn key_to_rust_string(value: f64) -> Option<String> {
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    unsafe {
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

fn property_key_to_rust_string(value: f64) -> Option<String> {
    let property_key = unsafe { crate::object::js_to_property_key(value) };
    key_to_rust_string(property_key)
}

fn own_set_descriptor(target: f64, key: f64) -> Option<OwnSetDescriptor> {
    if small_handle_from_value(target).is_some() {
        return None;
    }

    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        let value = unsafe { crate::symbol::js_object_get_symbol_property(target, key) };
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        // An existing symbol-keyed own data property is non-writable when the
        // receiver is frozen or its per-symbol attrs say so — so a strict
        // `obj[sym] = v` is rejected (throws) rather than silently no-op'd
        // (test262 Object/freeze/frozen-object-contains-symbol-properties-strict).
        // Mirrors the string-keyed / `set_symbol_property` guards.
        let writable = !crate::symbol::symbol_property_is_non_writable(target, key);
        return Some(OwnSetDescriptor::Data { writable });
    }

    // The null-receiver guard stays BEFORE the coercion: `key_to_rust_string`
    // can run a user `toString`, and moving it earlier would make that side
    // effect observable on a path that previously short-circuited.
    if extract_pointer(target.to_bits()) as usize == 0 {
        return None;
    }
    // #6943: `key_to_rust_string` runs the GC-capable `js_string_coerce`, and
    // `obj_ptr` is BOTH a dereferenced heap address (the typed-array probe) and
    // the raw ADDRESS KEY of the `ACCESSOR_DESCRIPTORS` /
    // `PROPERTY_DESCRIPTORS` side tables. A stale one does not crash: it
    // silently misses, so a non-writable own property or a setter-less accessor
    // reads back as "no descriptor" and the [[Set]] that should have been
    // rejected goes through. Root the receiver across the coercion and derive
    // the address afterwards. (Found in the same review pass as the `key` gap
    // below.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_heap_word_u64(target.to_bits());
    let key_name = key_to_rust_string(key)?;
    let target = f64::from_bits(target_handle.get_heap_word_u64());
    let obj_ptr = extract_pointer(target.to_bits()) as usize;
    if obj_ptr == 0 {
        return None;
    }
    // A typed array keeps its ordinary (non-index) own expando properties and
    // their descriptors in the typed-array side tables, which the generic
    // address-keyed lookups below skip (`object_has_descriptors` is
    // deliberately gated off for typed arrays). Consult that state directly so
    // a non-writable own data property / setter-less accessor rejects the write
    // (test262 TypedArray internals/Set key-is-not-numeric-index).
    if crate::typedarray::lookup_typed_array_kind(obj_ptr).is_some() {
        return match crate::typedarray_props::typed_array_own_set_descriptor(obj_ptr, &key_name) {
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Data { writable }) => {
                Some(OwnSetDescriptor::Data { writable })
            }
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Accessor { setter_bits }) => {
                Some(OwnSetDescriptor::Accessor { setter_bits })
            }
            None => None,
        };
    }
    // `ACCESSOR_DESCRIPTORS` / `PROPERTY_DESCRIPTORS` are keyed by raw address,
    // so a fresh object reusing a freed address would otherwise read back the
    // previous tenant's stale getter-only accessor / non-writable descriptor and
    // report this `obj.k = v` as read-only — falsely throwing "Cannot assign to
    // read only property" on a plain `{}` (Next.js app-page-turbo runtime's
    // `exports.Fragment = …`, reached here once a descriptor on Object.prototype
    // disables the plain-object [[Set]] fast path process-wide, #5054). Gate on
    // the per-object `OBJ_FLAG_HAS_DESCRIPTORS` flag — set reliably for every
    // descriptor installed on a `GC_TYPE_OBJECT`, and clear on a fresh
    // allocation. Closures don't carry the flag, so keep consulting the side
    // tables for them (their `name`/`length` + user `defineProperty` descriptors
    // live there).
    if crate::object::object_has_descriptors(obj_ptr) || crate::closure::is_closure_ptr(obj_ptr) {
        if let Some(acc) = crate::object::get_accessor_descriptor(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Accessor {
                setter_bits: acc.set,
            });
        }
        if let Some(attrs) = crate::object::get_property_attrs(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Data {
                writable: attrs.writable(),
            });
        }
    }
    if crate::closure::is_closure_ptr(obj_ptr) {
        if crate::object::has_own_helpers::closure_own_key_present(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Data {
                writable: !matches!(key_name.as_str(), "name" | "length"),
            });
        }
        return None;
    }
    if crate::object::obj_value_has_own_key(target, key) {
        return Some(OwnSetDescriptor::Data { writable: true });
    }
    None
}

fn prototype_of_for_set(value: f64) -> Option<f64> {
    if !reflect_value_is_object(value) {
        return None;
    }
    // A Proxy is a small registered id (`POINTER_TAG | (PROXY_TAG_BASE + id)`),
    // NOT a heap object. The POINTER_TAG block below would treat that id as a
    // raw pointer; on Linux (`is_valid_obj_ptr` HEAP_MIN = 0x1000) the ~1MB id
    // passes the range check and dereferences unmapped low memory → SIGSEGV.
    // drizzle nests proxies (a proxy whose target is itself a proxy), so this is
    // reachable when `is(value, type)` walks `getPrototypeOf` over a
    // proxy-wrapped table/column. Route it through the Proxy `[[GetPrototypeOf]]`
    // (no-trap → the target's prototype) instead. Returns `None` for a null /
    // self prototype, matching the heap-object handling below.
    if lookup(value).is_some() {
        let proto = reflect_misc::proxy_get_prototype_of_impl(value);
        let proto_bits = proto.to_bits();
        return if proto_bits == TAG_NULL
            || proto_bits == TAG_UNDEFINED
            || proto_bits == value.to_bits()
        {
            None
        } else {
            Some(proto)
        };
    }
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as usize;
        // #7531: `lookup(value)` above only recognizes an already-registered
        // revocable-Proxy id; a fetch/zlib/common-registry handle id (or a
        // Proxy id from a DIFFERENT realm/registry) reaches here instead.
        // The old floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle
        // band, and the local name `raw` made it invisible to
        // `scripts/addr_class_inventory.py`'s `handle-floor` regex. Below,
        // a rejected `object_static_prototype` lookup falls through to
        // `is_valid_obj_ptr(obj)` -- a magnitude-only check whose own floor
        // is 0x1000 -- followed by an unconditional `(*obj).class_id` read,
        // so an admitted handle id reached that deref.
        if crate::value::addr_class::is_above_handle_band(raw) {
            if let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(raw) {
                if proto_bits == TAG_NULL || proto_bits == TAG_UNDEFINED || proto_bits == bits {
                    return None;
                }
                return Some(f64::from_bits(proto_bits));
            }
            let obj = raw as *const crate::ObjectHeader;
            if crate::object::is_valid_obj_ptr(obj as *const u8) {
                unsafe {
                    let class_id = (*obj).class_id;
                    if class_id != 0 {
                        let proto = crate::object::class_prototype_object(class_id);
                        if !proto.is_null() && proto as usize != raw {
                            return Some(crate::value::js_nanbox_pointer(proto as i64));
                        }
                    }
                }
            }
        }
    }
    let proto = crate::object::js_object_get_prototype_of(value);
    let bits = proto.to_bits();
    if bits == TAG_NULL || bits == TAG_UNDEFINED || bits == value.to_bits() {
        None
    } else {
        Some(proto)
    }
}

fn reflect_target_get_prototype_of(value: f64) -> f64 {
    prototype_of_for_set(value).unwrap_or_else(|| crate::object::js_object_get_prototype_of(value))
}

fn call_setter_with_receiver(setter_bits: u64, receiver: f64, value: f64) -> bool {
    if setter_bits == 0 {
        return false;
    }
    let rebound = crate::closure::clone_closure_rebind_this(setter_bits, receiver);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return false;
    }
    let prev = crate::object::js_implicit_this_set(receiver);
    let _ = js_closure_call1(closure, value);
    crate::object::js_implicit_this_set(prev);
    true
}

/// #5129: build a fresh data property descriptor
/// `{ value, writable: true, enumerable: true, configurable: true }`
/// (the CreateDataProperty shape) for defining a property on a Proxy receiver
/// via its `[[DefineOwnProperty]]`.
unsafe fn build_create_data_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 4);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    for (name, field) in [
        (b"value".as_slice(), value_root.get_nanbox_f64()),
        (b"writable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"enumerable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"configurable".as_slice(), f64::from_bits(TAG_TRUE)),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key,
            field,
        );
    }
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

/// Define a writable, enumerable, configurable own data property using the
/// receiver's `[[DefineOwnProperty]]`. This is the operation used by public
/// class fields: it bypasses inherited setters on ordinary objects and still
/// drives a Proxy's `defineProperty` trap.
pub(crate) fn create_data_property(receiver: f64, key: f64, value: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);

    // Compiled class instances are born with their declared public-field keys
    // and default data-property attributes already represented by the shape.
    // In that common case, CreateDataProperty is exactly an existing own-data
    // overwrite: no inherited setter can intercept it, and there is no custom
    // descriptor to reset. Reuse the ordinary write path's fully validated,
    // barriered helper before allocating a four-property descriptor object.
    //
    // This is deliberately fail-closed. Proxies/handles, computed or SSO
    // keys, missing own keys, exotic receivers, prototype overrides, and any
    // receiver with custom descriptors fall through to the complete
    // [[DefineOwnProperty]] path below. The rooted field-name string need not
    // be interned: the transient-key entry point scans by content and never
    // caches that pointer.
    let receiver_bits = receiver.get_nanbox_f64().to_bits();
    let key_bits = key.get_nanbox_f64().to_bits();
    if (receiver_bits & !POINTER_MASK) == POINTER_TAG
        && (key_bits & !POINTER_MASK) == crate::value::STRING_TAG
    {
        let object = (receiver_bits & POINTER_MASK) as *mut crate::ObjectHeader;
        let key_ptr = (key_bits & POINTER_MASK) as *const crate::StringHeader;
        if unsafe {
            crate::object::try_existing_own_data_overwrite_transient_key(
                object,
                key_ptr,
                value.get_nanbox_f64(),
            )
        } {
            return true;
        }
    }

    let descriptor = unsafe { build_create_data_descriptor(value.get_nanbox_f64()) };
    let descriptor = scope.root_nanbox_f64(descriptor);
    crate::value::js_is_truthy(js_reflect_define_property(
        receiver.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor.get_nanbox_f64(),
    )) != 0
}

/// #5129: build a `{ value }`-only property descriptor — the `valueDesc` of
/// OrdinarySetWithOwnDescriptor step 2.d.iii, used to update an existing
/// writable data property on a Proxy receiver without disturbing its other
/// attributes (`writable`/`enumerable`/`configurable`).
unsafe fn build_value_only_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 1);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key,
        value_root.get_nanbox_f64(),
    );
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

fn create_or_update_receiver_property(receiver: f64, key: f64, value: f64) -> bool {
    if !reflect_value_is_object(receiver) {
        return false;
    }
    // #5129: a Proxy receiver — e.g. a `set` trap forwarding
    // `Reflect.set(target, key, value, proxy)` (the 4-arg form) — must route
    // through the proxy's `[[DefineOwnProperty]]` (its `defineProperty` trap,
    // or, absent a trap, a define on the proxy's target), NOT an ordinary data
    // store. This is OrdinarySetWithOwnDescriptor's tail
    // `CreateDataProperty(Receiver, P, V)`. Treating the proxy id as a heap
    // object (the `target_set` fall-through below) segfaulted; re-invoking the
    // `set` trap would have recursed infinitely.
    if lookup(receiver).is_some() {
        // OrdinarySetWithOwnDescriptor steps 2.c–2.e for a Proxy receiver. We
        // must mirror the ordinary algorithm's receiver-own-descriptor checks,
        // not jump straight to CreateDataProperty:
        //
        //   2.c existingDescriptor = Receiver.[[GetOwnProperty]](P)
        //   2.d if it exists:
        //       i.   accessor descriptor      → return false
        //       ii.  non-writable data        → return false
        //       iii. else redefine `{ value }` only (preserve other attrs)
        //   2.e else CreateDataProperty(Receiver, P, V)
        //
        // `[[GetOwnProperty]]` fires the proxy's getOwnPropertyDescriptor trap
        // (trap-less: reads its target) and returns a completed plain
        // descriptor object, or `undefined` when absent. A throwing/invariant-
        // violating trap unwinds via `js_throw` and never returns here.
        let existing = js_reflect_get_own_property_descriptor(receiver, key);
        let desc = if reflect_value_is_object(existing) {
            let is_accessor = unsafe {
                reflect::descriptor_field_present(existing, b"get")
                    || reflect::descriptor_field_present(existing, b"set")
            };
            // A completed data descriptor always carries `writable`; treat a
            // missing flag as non-writable (reject) to stay on the safe side.
            let writable = unsafe { reflect::descriptor_bool_field(existing, b"writable") };
            if is_accessor || writable != Some(true) {
                return false;
            }
            unsafe { build_value_only_descriptor(value) }
        } else {
            unsafe { build_create_data_descriptor(value) }
        };
        return crate::value::js_is_truthy(js_reflect_define_property(receiver, key, desc)) != 0;
    }
    if let Some(desc) = own_set_descriptor(receiver, key) {
        match desc {
            OwnSetDescriptor::Data { writable } => {
                if !writable {
                    return false;
                }
            }
            OwnSetDescriptor::Accessor { .. } => {
                // OrdinarySetWithOwnDescriptor step 2.d.i: reaching here means
                // the source (target own) descriptor was a data property (or
                // absent -> CreateDataProperty). A RECEIVER own accessor makes
                // the algorithm return false WITHOUT invoking the setter -- the
                // setter only fires when it is the descriptor found by the
                // OrdinarySet walk itself (the Accessor arm in
                // ordinary_set_with_receiver), never through this
                // CreateDataProperty-on-receiver tail.
                return false;
            }
        }
    } else if crate::closure::is_closure_ptr(extract_pointer(receiver.to_bits()) as usize) {
        target_set(receiver, key, value);
        return true;
    } else if crate::object::obj_value_no_extend(receiver) {
        return false;
    }
    target_set(receiver, key, value);
    true
}

fn ordinary_set_with_receiver(target: f64, key: f64, value: f64, receiver: f64) -> bool {
    if let Some(ok) = set_handle_property(target, key, value) {
        return ok;
    }

    // #5054 fast path: the spec walk below probes own_set_descriptor on the
    // target, which ends in a LINEAR keys_array scan — so every dynamic
    // `obj[key] = v` was O(own-key-count) and building a wide dynamic object
    // quadratic (10k props ~ 12s). When nothing the walk models can apply,
    // the write reduces to the ordinary data-property store:
    //   - target written as itself (receiver bits identical),
    //   - plain GC_TYPE_OBJECT with class_id 0 (no class setter machinery),
    //   - no descriptor ever installed on THIS object
    //     (OBJ_FLAG_HAS_DESCRIPTORS) and not frozen/sealed/non-extensible,
    //   - no recorded setPrototypeOf target (prototype chain is exactly
    //     Object.prototype) and no descriptor on Object.prototype,
    //   - string key.
    let target_top16 = target.to_bits() >> 48;
    if target.to_bits() == receiver.to_bits()
        // POINTER_TAG'd heap object, or a module-level slot's raw I64 pointer
        // (top 16 bits zero).
        && (target_top16 == 0x7FFD || target_top16 == 0)
        && unsafe { crate::symbol::js_is_symbol(key) } == 0
    {
        let addr = extract_pointer(target.to_bits()) as usize;
        // Typed arrays must be excluded before the header probe: small TAs
        // are plain-alloc'd without a GcHeader.
        if crate::typedarray::lookup_typed_array_kind(addr).is_none()
            && crate::object::exotic_expando::exotic_expando_kind_of_value(target).is_none()
            && !crate::closure::is_closure_ptr(addr)
        {
            unsafe {
                if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
                    const SLOW_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
                        | crate::gc::OBJ_FLAG_SEALED
                        | crate::gc::OBJ_FLAG_NO_EXTEND
                        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
                    if header.obj_type == crate::gc::GC_TYPE_OBJECT
                        && header._reserved & SLOW_FLAGS == 0
                    {
                        let class_id = (*(addr as *const crate::ObjectHeader)).class_id;
                        // #6943: BOTH arms below reach a GC-capable
                        // `js_string_coerce` on `key` — the class arm calls it
                        // directly for the store-plan key, and the plain-object
                        // arm reaches it through
                        // `object_proto_may_intercept_key` ->
                        // `obj_value_has_own_key`. The coercion is inert only
                        // for an already-heap `STRING_TAG` key; an SSO short
                        // key materializes onto the heap, a numeric key builds
                        // its stringification, and an object key runs a user
                        // `toString` / `valueOf`. Any of those can trigger a GC
                        // that **evacuates**.
                        //
                        // The rule is: when the coercion is NOT inert, every
                        // operand that outlives it must be rooted and re-read
                        // afterwards. That is four of them, not three —
                        // `addr` (the receiver, dereferenced for `plan_eligible`
                        // and passed to `class_instance_set_may_intercept`),
                        // `target` and the `value` about to be written INTO it,
                        // and **`key` itself**, which is re-used at the
                        // interception check and again at `target_set`. An
                        // object key is a `POINTER_TAG` heap value and is
                        // exactly the shape that runs the user JS which can
                        // evacuate it. (`key` was missed in the first pass of
                        // this fix; caught in review.)
                        //
                        // Only an already-heap `STRING_TAG` key is inert — it
                        // is handed straight back with no allocation — so that
                        // one shape takes no scope. Every other shape does: an
                        // SSO short key materializes onto the heap, a numeric
                        // key builds its stringification, and an object key
                        // runs a user `toString` / `valueOf`.
                        let scope = (!crate::builtins::string_coerce_is_inert(key))
                            .then(crate::gc::RuntimeHandleScope::new);
                        let roots = scope.as_ref().map(|s| {
                            (
                                s.root_heap_word_u64(target.to_bits()),
                                s.root_nanbox_f64(value),
                                s.root_raw_mut_ptr(addr as *mut u8),
                                s.root_nanbox_f64(key),
                            )
                        });
                        // Re-read an operand through its handle (or pass the
                        // original through untouched on the inert path).
                        let cur_target = || match &roots {
                            Some((t, ..)) => f64::from_bits(t.get_heap_word_u64()),
                            None => target,
                        };
                        let cur_value = || match &roots {
                            Some((_, v, ..)) => v.get_nanbox_f64(),
                            None => value,
                        };
                        let cur_addr = || match &roots {
                            Some((_, _, a, _)) => a.get_raw_mut_ptr::<u8>() as usize,
                            None => addr,
                        };
                        let cur_key = || match &roots {
                            Some((_, _, _, k)) => k.get_nanbox_f64(),
                            None => key,
                        };
                        let fast_safe = if class_id == 0 {
                            // Plain object: prototype is exactly Object.prototype, and
                            // Object.prototype doesn't intercept this key (per-key, not
                            // the coarse process-wide descriptor flag — that made wide
                            // builds O(n²)).
                            crate::object::prototype_chain::object_static_prototype(addr).is_none()
                                && !crate::object::object_proto_may_intercept_key(key)
                        } else {
                            // `DisposableStack#disposed` is a getter-only
                            // builtin accessor on a reserved native prototype.
                            // Those class ids are intentionally absent from the
                            // JS class-vtable registry, so the shared
                            // class-interception plan cannot discover it.
                            // Keep an own descriptor on the instance eligible
                            // for the normal walk; otherwise report the
                            // inherited accessor's rejected [[Set]] here
                            // (silent in sloppy PutValue, TypeError in strict).
                            let inherited_disposed_readonly = matches!(
                                class_id,
                                crate::disposable::CLASS_ID_DISPOSABLE_STACK
                                    | crate::disposable::CLASS_ID_ASYNC_DISPOSABLE_STACK
                            ) && property_key_to_rust_string(key)
                                .as_deref()
                                == Some("disposed")
                                // #6943: `property_key_to_rust_string` runs
                                // `ToPropertyKey` + `js_string_coerce`, so both
                                // operands must be re-read before this probe.
                                && own_set_descriptor(cur_target(), cur_key()).is_none();
                            if inherited_disposed_readonly {
                                return false;
                            }
                            // Class instance: the `class_id == 0` guard previously sent
                            // EVERY wide class-instance build down the O(own-key) slow
                            // walk (O(n²)). Safe to fast-path when no inherited accessor /
                            // non-writable anywhere in the prototype chain could intercept
                            // this key.
                            //
                            // This receiver-based [[Set]] is the dominant real-code store
                            // path (codegen emits js_put_value_set for many `obj.f = v`),
                            // and it re-ran the full O(chain) interception walk on every
                            // call. Consult the SHARED store-plan cache first: the verdict
                            // is the identical `!class_instance_set_may_intercept(class_id,
                            // key)` that `js_object_set_field_by_name` already memoizes, so
                            // a hit on either path serves the other. Only trust a cached
                            // class-chain verdict for instances whose chain matches the
                            // class chain — SLOW_FLAGS above already excluded
                            // frozen/sealed/descriptor bits; add the per-instance
                            // divergence flags (setPrototypeOf override / null proto).
                            let key_ptr = crate::builtins::js_string_coerce(cur_key())
                                as *const crate::StringHeader;
                            // #6943: re-read the receiver AND the key through
                            // their handles — everything below uses both.
                            let addr = cur_addr();
                            let key = cur_key();
                            let interned = crate::object::interned_key_ptr(key_ptr);
                            // #6595: a per-evaluation CLASS OBJECT (what a
                            // capture-carrying class materializes as,
                            // `ShapeObjectKind::Class`) shares its
                            // template cid with its instances, and its own-data
                            // writes must reach the #6530
                            // `mirror_class_object_static_write` hook in
                            // `js_object_set_field_by_name`. Recording a plan
                            // here armed the mirror-free fast lane for the very
                            // store being vetted, so from the second same-shaped
                            // class on (shape-transition cache hit) post-class
                            // statics like bundled zod's `ZodX.create` vanished
                            // from ClassRef static dispatch. Class objects
                            // neither record nor honor store plans.
                            let plan_eligible = header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO
                                == 0
                                && !crate::object::prototype_chain::object_has_prototype_override(
                                    addr,
                                )
                                && class_id != crate::object::NATIVE_MODULE_CLASS_ID
                                // #8113: this asks for ORDINARY specifically —
                                // it must stay FALSE for a class object or
                                // #6595 reopens. `object_is_regular` is exactly
                                // `descriptor.object_kind == Ordinary` since
                                // #8086, so it is the same predicate the
                                // deleted `object_type == OBJECT_TYPE_REGULAR`
                                // word expressed, not the weaker
                                // "is an ObjectHeader" test.
                                && crate::object::object_is_regular(
                                    addr as *const crate::ObjectHeader,
                                )
                                && interned != 0;
                            let verdict = if plan_eligible
                                && crate::object::prop_plan::store_plan_check(class_id, interned)
                            {
                                true
                            } else {
                                let clear = !crate::object::class_instance_set_may_intercept(
                                    addr, class_id, key,
                                );
                                if clear && plan_eligible {
                                    crate::object::prop_plan::store_plan_record(class_id, interned);
                                }
                                clear
                            };
                            verdict
                        };
                        // #6943: the store itself takes the refreshed receiver,
                        // KEY and payload — all three were rooted across
                        // whichever coercion the arm above performed.
                        if fast_safe {
                            target_set(cur_target(), cur_key(), cur_value());
                            return true;
                        }
                    }
                }
            }
        }
    }

    // CommonJS native-module namespaces are MUTABLE in Node — monkey-patching
    // like Next.js's `require('node:timers').setImmediate = patched` must
    // store the override (read back through the namespace vtable's
    // `get_own_field`) rather than reporting the built-in member
    // non-writable and throwing under strict mode.
    {
        let jv = crate::value::JSValue::from_bits(target.to_bits());
        if jv.is_pointer() {
            let obj = extract_pointer(target.to_bits()) as *const crate::object::ObjectHeader;
            if !obj.is_null() && unsafe { (*obj).class_id } == crate::object::NATIVE_MODULE_CLASS_ID
            {
                let module_name = unsafe { crate::object::get_module_name_from_namespace(target) };
                if let (false, Some(prop)) =
                    (module_name.is_empty(), property_key_to_rust_string(key))
                {
                    if prop != "__module__" {
                        if module_name == "buffer.Buffer" && prop == "poolSize" {
                            crate::object::set_buffer_pool_size(value);
                        } else {
                            crate::object::native_namespace_prop_override_store(
                                &module_name,
                                &prop,
                                value,
                            );
                        }
                        return true;
                    }
                }
            }
        }
    }

    let mut current = target;
    for _ in 0..64 {
        // A Proxy hop in the prototype chain: `OrdinarySetWithOwnDescriptor`
        // step 2.a-b dispatches the full `[[Set]]` on the parent with the
        // ORIGINAL `receiver`, not the raw own-descriptor walk below (which
        // would misread the small proxy id as a heap pointer).
        if lookup(current).is_some() {
            return crate::value::js_is_truthy(proxy_set_with_receiver(
                current, key, value, receiver,
            )) != 0;
        }
        // Integer-Indexed exotic [[Set]] (§10.4.5.5): a typed array in the
        // chain intercepts a canonical numeric index key — the prototype
        // chain is NEVER consulted for it. `SameValue(O, Receiver)` writes
        // the element; a different receiver with a valid index falls to the
        // ordinary data-descriptor flow (create on receiver); an invalid
        // canonical index is a silent no-op `true`.
        let cur_addr = extract_pointer(current.to_bits()) as usize;
        if crate::typedarray::lookup_typed_array_kind(cur_addr).is_some() {
            if let Some(name) = property_key_to_rust_string(key) {
                match crate::typedarray_props::typed_array_canonical_index_validity(cur_addr, &name)
                {
                    Some(valid) => {
                        let recv_addr = extract_pointer(receiver.to_bits()) as usize;
                        if recv_addr == cur_addr {
                            return unsafe {
                                crate::typedarray_props::typed_array_set_property_by_name(
                                    cur_addr, &name, value,
                                )
                            };
                        }
                        if !valid {
                            return true;
                        }
                        // The receiver may itself be a typed array: the
                        // CreateDataProperty lands in ITS [[DefineOwnProperty]],
                        // which rejects an index that is invalid FOR THE
                        // RECEIVER (`Reflect.set(ta, "0", v, emptyTa)` → false).
                        if crate::typedarray::lookup_typed_array_kind(recv_addr).is_some() {
                            return match crate::typedarray_props::
                                typed_array_canonical_index_validity(recv_addr, &name)
                            {
                                Some(true) => unsafe {
                                    crate::typedarray_props::typed_array_set_property_by_name(
                                        recv_addr, &name, value,
                                    )
                                },
                                Some(false) => false,
                                None => create_or_update_receiver_property(receiver, key, value),
                            };
                        }
                        return create_or_update_receiver_property(receiver, key, value);
                    }
                    // Ordinary key on a TA in the chain: stop the walk (Perry's
                    // TA prototype methods are served natively, not as data
                    // descriptors visible to `own_set_descriptor`) and define
                    // on the receiver.
                    None => {
                        return create_or_update_receiver_property(receiver, key, value);
                    }
                }
            }
        }
        if let Some(desc) = own_set_descriptor(current, key) {
            return match desc {
                OwnSetDescriptor::Data { writable } => {
                    if !writable {
                        false
                    } else {
                        create_or_update_receiver_property(receiver, key, value)
                    }
                }
                OwnSetDescriptor::Accessor { setter_bits } => {
                    call_setter_with_receiver(setter_bits, receiver, value)
                }
            };
        }
        // #6828: `%Object.prototype%.__proto__` is a legacy accessor whose
        // setter performs `SetPrototypeOf(Receiver, value)`. Perry exposes the
        // getter intrinsically but does not materialize the built-in accessor
        // in the ordinary descriptor table, so model it at the exact point in
        // the [[Set]] walk where that descriptor would be found.
        //
        // Keep this AFTER `own_set_descriptor`: a user-installed own
        // `__proto__` data/accessor property on an object earlier in the chain
        // must win. A null-prototype receiver never reaches the canonical
        // Object.prototype and therefore still creates an ordinary own data
        // property. Per Annex B, a primitive RHS is ignored rather than
        // throwing (unlike `Object.setPrototypeOf`).
        let current_addr = extract_pointer(current.to_bits()) as usize;
        if current_addr != 0
            && current_addr == crate::array::object_prototype_addr()
            && key_to_rust_string(key).as_deref() == Some("__proto__")
        {
            let value_bits = value.to_bits();
            let valid_proto = value_bits == TAG_NULL
                || lookup(value).is_some()
                || crate::object::class_ref_id(value).is_some()
                || unsafe { crate::object::value_is_object_like(value) };
            if valid_proto && reflect_value_is_object(receiver) {
                crate::object::js_object_set_prototype_of(receiver, value);
            }
            return true;
        }
        if crate::closure::is_closure_ptr(extract_pointer(current.to_bits()) as usize) {
            // ECMAScript poison pill: `fn.caller = v` / `fn.arguments = v` on
            // a strict-mode function (all Perry-compiled code) throws via the
            // %ThrowTypeError% accessor's absent setter. A genuine own data
            // prop (defineProperty round-trip) still wins via the descriptor
            // arm above.
            let cur_ptr = extract_pointer(current.to_bits()) as usize;
            if let Some(name) = key_to_rust_string(key) {
                if matches!(name.as_str(), "caller" | "arguments")
                    && !crate::closure::closure_has_own_dynamic_prop(cur_ptr, &name)
                {
                    throw_type_error("Restricted function property assignment");
                }
                // Every function's [[Prototype]] is %Function.prototype% — a
                // descriptor installed there via `Object.defineProperty(
                // Function.prototype, k, {...})` must intercept a plain
                // `boundFn.k = v` write (invoke an accessor's setter, throw
                // for a getter-only accessor, or block a non-writable data
                // property) instead of silently shadowing it with a new own
                // data property on the receiver.
                if crate::closure::closure_set_via_function_prototype_descriptor(
                    cur_ptr, &name, value, receiver,
                ) {
                    return true;
                }
                // `Object.preventExtensions(fn)` / `Object.seal(fn)` set
                // NO_EXTEND / SEALED in the closure's GcHeader (functions are
                // GcHeader-backed closures). A [[Set]] that would ADD a new own
                // property must fail (OrdinaryDefineOwnProperty returns false,
                // silent in non-strict, TypeError in strict); an existing own
                // key can still be updated. (test262
                // Object/preventExtensions/15.2.3.10-3-{3,13}.)
                if !crate::closure::closure_has_own_dynamic_prop(cur_ptr, &name) {
                    let non_extensible = unsafe {
                        let gc = (cur_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                            as *const crate::gc::GcHeader;
                        (*gc)._reserved
                            & (crate::gc::OBJ_FLAG_NO_EXTEND | crate::gc::OBJ_FLAG_SEALED)
                            != 0
                    };
                    if non_extensible {
                        return false;
                    }
                }
            }
            return create_or_update_receiver_property(receiver, key, value);
        }
        let Some(proto) = prototype_of_for_set(current) else {
            return create_or_update_receiver_property(receiver, key, value);
        };
        current = proto;
    }
    false
}

fn class_super_accessor_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
) -> Option<bool> {
    let key_name = property_key_to_rust_string(key)?;
    let registry = crate::object::CLASS_VTABLE_REGISTRY.read().ok()?;
    let reg = registry.as_ref()?;
    let mut cid = parent_class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(vtable) = reg.get(&cid) {
            let setter_alias = format!("__set_{}", key_name);
            if let Some(&setter_ptr) = vtable
                .setters
                .get(&key_name)
                .or_else(|| vtable.setters.get(&setter_alias))
            {
                let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(setter_ptr) };
                let prev_this = crate::object::js_implicit_this_set(receiver);
                let _ = f(receiver, value);
                crate::object::js_implicit_this_set(prev_this);
                return Some(true);
            }
            let getter_alias = format!("__get_{}", key_name);
            if vtable.getters.contains_key(&key_name) || vtable.getters.contains_key(&getter_alias)
            {
                return Some(false);
            }
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => {
                cid = parent;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

fn receiver_super_parent_class_id(receiver: f64) -> Option<u32> {
    let obj = extract_pointer(receiver.to_bits()) as *const crate::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    let class_id = unsafe { (*obj).class_id };
    if class_id == 0 {
        return None;
    }
    crate::object::get_parent_class_id(class_id)
}

fn normalize_accessor_receiver(receiver: f64) -> f64 {
    let bits = receiver.to_bits();
    if bits != 0 && (bits >> 48) == 0 {
        crate::value::js_nanbox_pointer(bits as i64)
    } else if receiver.is_finite() && receiver > 65_536.0 && receiver.fract() == 0.0 {
        crate::value::js_nanbox_pointer(receiver as i64)
    } else {
        receiver
    }
}

/// `super[key] = value` for class methods. The property lookup starts at the
/// parent prototype, but writes use the current `this` as Receiver.
#[no_mangle]
pub extern "C" fn js_super_put_value_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    let receiver = normalize_accessor_receiver(receiver);
    // Static-context super uses the parent CONSTRUCTOR as the lookup target,
    // while retaining the child constructor as Receiver. A statically-known
    // class parent is represented by a ClassRef; a function-valued parent is
    // the value captured at class-definition time. The previous instance-only
    // path looked at `Parent.prototype` and made valid static writes fail.
    if let Some(child_id) = crate::object::class_ref_id(receiver) {
        let target = if parent_class_id != 0 {
            f64::from_bits(crate::value::INT32_TAG | parent_class_id as u64)
        } else {
            crate::object::js_get_dynamic_parent_value(child_id)
        };
        let tv = crate::value::JSValue::from_bits(target.to_bits());
        if !tv.is_undefined() && !tv.is_null() {
            return js_put_value_set(target, key, value, receiver, strict);
        }
        // A base class's constructor inherits from Function.prototype. Perry
        // does not materialize that object for this path; when lookup misses,
        // OrdinarySet creates the own property on Receiver.
        target_set(receiver, key, value);
        return value;
    }
    if parent_class_id == 0 && crate::object::is_class_object_value(receiver) {
        let obj = crate::value::JSValue::from_bits(receiver.to_bits())
            .as_pointer::<crate::ObjectHeader>();
        let child_id = if obj.is_null() {
            0
        } else {
            crate::object::js_object_get_class_id(obj)
        };
        let dynamic_parent = crate::object::js_get_dynamic_parent_value(child_id);
        if crate::value::JSValue::from_bits(dynamic_parent.to_bits()).is_undefined() {
            target_set(receiver, key, value);
            return value;
        }
        return js_put_value_set(dynamic_parent, key, value, receiver, strict);
    }
    let receiver_parent_class_id = receiver_super_parent_class_id(receiver);
    if let Some(ok) =
        class_super_accessor_set(parent_class_id, key, value, receiver).or_else(|| {
            receiver_parent_class_id
                .filter(|cid| *cid != parent_class_id)
                .and_then(|cid| class_super_accessor_set(cid, key, value, receiver))
        })
    {
        if !ok && strict != 0 {
            let key_name = key_to_rust_string(key).unwrap_or_else(|| "property".to_string());
            crate::error::throw_immutable_write(0, &key_name);
        }
        return value;
    }

    let effective_parent_class_id = if parent_class_id != 0 {
        parent_class_id
    } else {
        receiver_parent_class_id.unwrap_or(0)
    };
    let proto = crate::object::class_prototype_object(effective_parent_class_id);
    if !proto.is_null() {
        let target = crate::value::js_nanbox_pointer(proto as i64);
        return js_put_value_set(target, key, value, receiver, strict);
    }

    // No resolvable parent-class prototype — `super` is `Object.prototype`
    // (e.g. `class A {}` with no `extends`). Per spec `super.x = v` performs
    // the home object's prototype `[[Set]]` with `this` as the receiver, which
    // for a missing key + no inherited setter creates an own data property on
    // the receiver. Do that ordinary set instead of throwing. (Test262
    // syntax/class-body-method-definition-super-property.)
    js_put_value_set(receiver, key, value, receiver, strict)
}

/// `Proxy.revocable(target, handler)` — returns an ordinary object
/// `{ proxy, revoke }` where `proxy` is a fresh revocable Proxy and `revoke`
/// is a callable, idempotent function that revokes only that proxy. (#2846)
///
/// Unlike the destructuring fast-path in `stmt.rs`, this builds a real heap
/// object so `typeof rec.revoke === "function"`, `rec.proxy.a` forwards, and
/// the revoke function can be stored/aliased and still work.
#[no_mangle]
pub extern "C" fn js_proxy_revocable(target: f64, handler: f64) -> f64 {
    // Reuse `js_proxy_new` so the same object-argument validation applies.
    let proxy = js_proxy_new(target, handler);

    // Build the revoke closure capturing the proxy value.
    let revoke_closure =
        crate::closure::js_closure_alloc(reflect_misc::proxy_revoke_trampoline as *const u8, 1);
    crate::closure::js_register_closure_arity(
        reflect_misc::proxy_revoke_trampoline as *const u8,
        0,
    );
    crate::closure::js_closure_set_capture_f64(revoke_closure, 0, proxy);
    let revoke_boxed = f64::from_bits(POINTER_TAG | ((revoke_closure as u64) & POINTER_MASK));

    // Build the `{ proxy, revoke }` record. Root everything across the
    // intermediate allocations so a GC during key/string allocation can't
    // strand the proxy/revoke values.
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy_root = scope.root_nanbox_f64(proxy);
    let revoke_root = scope.root_nanbox_f64(revoke_boxed);

    let obj = crate::object::js_object_alloc(0, 2);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let keys = crate::array::js_array_alloc(0);
    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    crate::object::js_object_set_keys(obj, keys);

    let proxy_key = crate::string::js_string_from_bytes(b"proxy".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(obj, proxy_key, proxy_root.get_nanbox_f64());
    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    let revoke_key = crate::string::js_string_from_bytes(b"revoke".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(obj, revoke_key, revoke_root.get_nanbox_f64());

    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    f64::from_bits(POINTER_TAG | ((obj as u64) & POINTER_MASK))
}

// #2846: retention anchor for `Proxy.revocable` (codegen-only callsite).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_PROXY_REVOCABLE: extern "C" fn(f64, f64) -> f64 = js_proxy_revocable;

// #2762: retention anchors for the Reflect-specific extensibility entry points.
// These `#[no_mangle]` fns are emitted only by codegen (no Rust caller in the
// crate graph), so the auto-optimize whole-program LLVM bitcode rebuild would
// otherwise internalize and dead-strip them. See node_stream_keepalive.rs.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_IS_EXTENSIBLE: extern "C" fn(f64) -> f64 = js_reflect_is_extensible;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_PREVENT_EXTENSIONS: extern "C" fn(f64) -> f64 = js_reflect_prevent_extensions;

// #2761: retention anchor for `Reflect.setPrototypeOf` (codegen-only callsite).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_SET_PROTOTYPE_OF: extern "C" fn(f64, f64) -> f64 = js_reflect_set_prototype_of;

// #2763/#2764/#2766/#2767: retention anchors for the Reflect entry points
// whose only callsites are codegen-emitted. `js_reflect_get` gained a third
// `receiver` arg (#2766) and must keep its new signature retained.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_GET: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_GET_OWN_PROPERTY_DESCRIPTOR: extern "C" fn(f64, f64) -> f64 =
    js_reflect_get_own_property_descriptor;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_HAS: extern "C" fn(f64, f64) -> f64 = js_reflect_has;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_OWN_KEYS: extern "C" fn(f64) -> f64 = js_reflect_own_keys;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REFLECT_APPLY: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_apply;

/// #8194: death pruning for `REFLECT_METADATA`.
///
/// The key carries the decorator target's NaN-boxed bits, and
/// `rewrite_metadata_target_bits` **rekeys** them when the target moves.
/// Entries were removed only by `Reflect.deleteMetadata`, so a target that
/// simply became unreachable left its address in the key — the #8040 shape,
/// because the next rewrite pass reads whatever the arena put there.
///
/// Non-`POINTER_TAG` keys (class refs, primitives) and handle-band ids are
/// left alone: they are not heap addresses, and the `gc::dead_owner` probes
/// decline to attribute them anyway.
pub(crate) fn prune_dead_reflect_metadata_targets(is_dead_owner: &dyn Fn(usize) -> bool) {
    REFLECT_METADATA.with(|store| {
        store.borrow_mut().retain(|key, _| {
            if (key.target_bits & !POINTER_MASK) != POINTER_TAG {
                return true;
            }
            let addr = (key.target_bits & POINTER_MASK) as usize;
            addr == 0 || !is_dead_owner(addr)
        });
    });
}

#[cfg(test)]
pub(crate) fn test_reflect_metadata_len() -> usize {
    REFLECT_METADATA.with(|store| store.borrow().len())
}

#[cfg(test)]
pub(crate) fn test_seed_reflect_metadata(target_bits: u64, key: &str) {
    REFLECT_METADATA.with(|store| {
        store.borrow_mut().insert(
            MetadataKey {
                target_bits,
                key: key.to_string(),
                property_key: None,
            },
            f64::from_bits(TAG_UNDEFINED),
        );
    });
}

/// Rewrite a `REFLECT_METADATA` key's POINTER-tagged target bits during the
/// GC metadata-rewrite phase; non-pointer targets (class refs, primitives)
/// pass through untouched.
fn rewrite_metadata_target_bits(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    target_bits: u64,
) -> u64 {
    if !visitor.is_metadata_rewrite_phase() {
        return target_bits;
    }
    if (target_bits & !POINTER_MASK) != POINTER_TAG {
        return target_bits;
    }
    let mut addr = (target_bits & POINTER_MASK) as usize;
    if visitor.visit_metadata_usize_slot(&mut addr) {
        POINTER_TAG | (addr as u64 & POINTER_MASK)
    } else {
        target_bits
    }
}

/// GC scanner for the proxy registry + reflect-metadata store. A minor trace
/// must visit every live entry because it does not scan the whole heap. A full
/// mark trace skips those strong edges and observes proxy handles from roots
/// and heap slots instead; rewrite/verify phases still visit surviving entry
/// slots. `REFLECT_METADATA`'s keys are rekeyed during metadata rewrite.
pub(crate) fn scan_proxy_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // A full mark trace discovers liveness from proxy-band handles instead of
    // making the registry itself a root. Minors cannot make that inference
    // because they do not scan the whole heap, and rewrite/verify phases must
    // still update the live entries after evacuation.
    if !(gc_full_trace_active() && visitor.is_mark_phase()) {
        PROXIES.with(|proxies| {
            for entry in proxies.borrow_mut().iter_mut().flatten() {
                visitor.visit_nanbox_f64_slot(&mut entry.target);
                visitor.visit_nanbox_f64_slot(&mut entry.handler);
            }
        });
    }

    REFLECT_METADATA.with(|store| {
        let mut store = store.borrow_mut();
        let needs_rebuild = store
            .keys()
            .any(|key| rewrite_metadata_target_bits(visitor, key.target_bits) != key.target_bits);
        if needs_rebuild {
            let old = std::mem::take(&mut *store);
            for (mut key, mut value) in old {
                visitor.visit_nanbox_f64_slot(&mut value);
                key.target_bits = rewrite_metadata_target_bits(visitor, key.target_bits);
                store.insert(key, value);
            }
        } else {
            for value in store.values_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_proxy_slot_is_live(proxy_boxed: f64) -> bool {
    let bits = proxy_boxed.to_bits();
    let Some(id) = decode_proxy_id((bits & POINTER_MASK) as i64) else {
        return false;
    };
    PROXIES.with(|proxies| {
        proxies
            .borrow()
            .get(id as usize)
            .is_some_and(Option::is_some)
    })
}

#[cfg(test)]
pub(crate) fn test_proxy_gc_reclaimed_total() -> u64 {
    PROXY_GC_RECLAIMED_TOTAL.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj_value() -> f64 {
        let obj = crate::object::js_object_alloc(0, 0);
        f64::from_bits(POINTER_TAG | ((obj as u64) & POINTER_MASK))
    }

    #[test]
    fn class_field_create_data_property_keeps_a_pristine_declared_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C1A_5501;
        let packed = b"declared_field";
        let keys =
            crate::object::js_build_class_keys_array(CID, 1, packed.as_ptr(), packed.len() as u32);
        let shape_id = crate::object::shapes::js_object_shape_id_for_keys(keys as usize as u64, 1);
        let object =
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 1, keys, shape_id);

        let raw_key = crate::string::js_string_from_bytes(packed.as_ptr(), packed.len() as u32);
        let receiver = f64::from_bits(POINTER_TAG | (object as u64 & POINTER_MASK));
        let key_value = f64::from_bits(crate::value::STRING_TAG | (raw_key as u64 & POINTER_MASK));

        let key_header = unsafe {
            &*((raw_key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
        };
        assert_eq!(
            key_header.gc_flags & crate::gc::GC_FLAG_INTERNED,
            0,
            "the regression path uses the same transient field-name representation as codegen"
        );

        assert!(create_data_property(receiver, key_value, 42.0));
        assert_eq!(unsafe { (*object).parent_class_id }, shape_id);
        let header = unsafe {
            &*((object as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
        };
        assert_eq!(
            header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
            0,
            "all-default class-field attributes need no descriptor side table"
        );
        assert!(crate::object::get_property_attrs(object as usize, "declared_field").is_none());
        assert_eq!(
            f64::from_bits(crate::object::js_object_get_field_by_name(object, raw_key).bits()),
            42.0
        );
    }

    /// #8213: an id past the end of the revocable-Proxy band does not merely
    /// fail to round-trip — it encodes to a payload every addr-class consumer
    /// reads as a **dereferenceable heap address**, so the value the 65,536th
    /// `new Proxy(...)` handed back was segfaulted by the next property read.
    ///
    /// The vacuity guard matters here: if the band is ever moved or resized,
    /// `PROXY_ID_BAND_LEN` moves with it and the loop below would still pass
    /// while testing a different range, so pin the width too.
    #[test]
    fn every_reservable_proxy_id_encodes_inside_the_band() {
        use crate::value::addr_class;

        assert_eq!(
            PROXY_ID_BAND_LEN, 0x10000,
            "the revocable-Proxy band is [0xF0000, 0x100000): 65,536 ids"
        );

        for len in 0..PROXY_ID_BAND_LEN as usize {
            let id = reserve_proxy_id(len).expect("inside the band");
            assert_eq!(id, len as u64);
            assert!(
                addr_class::is_proxy_id_band(encode_proxy_id(id) as usize),
                "id {id} must encode inside the proxy band"
            );
        }

        // The first refused id, and why refusing it is a memory-safety fix
        // rather than a tidiness one.
        assert_eq!(reserve_proxy_id(PROXY_ID_BAND_LEN as usize), None);
        let out_of_band = encode_proxy_id(PROXY_ID_BAND_LEN) as usize;
        assert!(!addr_class::is_proxy_id_band(out_of_band));
        assert!(
            addr_class::is_above_handle_band(out_of_band),
            "an out-of-band proxy id is classified as a heap address to be \
             dereferenced — that is the SIGSEGV this guard prevents"
        );
    }

    /// The decoder and `addr_class::is_proxy_id_band` must agree about what a
    /// proxy id is. Before #8213 they did not: `lookup` accepted any payload
    /// below 4 GiB, so an out-of-band id was simultaneously "a live proxy"
    /// (here) and "a heap pointer" (everywhere else).
    #[test]
    fn decode_proxy_id_rejects_payloads_outside_the_band() {
        use crate::value::addr_class;

        assert_eq!(
            decode_proxy_id(PROXY_TAG_BASE as i64),
            None,
            "id 0 reserved"
        );
        assert_eq!(
            decode_proxy_id((PROXY_TAG_BASE - 1) as i64),
            None,
            "below band"
        );
        assert_eq!(
            decode_proxy_id((addr_class::HANDLE_BAND_MAX - 1) as i64),
            Some(PROXY_ID_BAND_LEN - 1),
            "the last in-band payload still decodes"
        );
        assert_eq!(
            decode_proxy_id(addr_class::HANDLE_BAND_MAX as i64),
            None,
            "the first payload past the band is not a proxy id"
        );
        assert_eq!(decode_proxy_id(0x1_0000_0000_i64), None);
    }

    /// End of the live path: `js_proxy_new` stops handing out ids at the band
    /// edge. The refusal itself is asserted through `reserve_proxy_id` because
    /// the throw `js_proxy_new` performs exits the process when no `try` is
    /// open (`exception::js_throw`), which a unit test cannot survive.
    #[test]
    fn js_proxy_new_never_mints_past_the_band_edge() {
        use crate::value::addr_class;

        // Shrink the band so the edge is reachable without 65k allocations.
        // Index 0 is reserved, so a length of 6 leaves 5 usable ids.
        PROXY_ID_BAND_LEN_OVERRIDE.with(|c| c.set(Some(6)));

        let mut minted = Vec::new();
        for _ in 0..5 {
            let proxy = js_proxy_new(obj_value(), obj_value());
            let payload = (proxy.to_bits() & POINTER_MASK) as usize;
            assert!(
                addr_class::is_proxy_id_band(payload),
                "{payload:#x} escaped the proxy band"
            );
            assert_eq!(js_proxy_is_proxy(proxy), 1);
            minted.push(proxy);
        }

        let full = PROXIES.with(|p| p.borrow().len());
        assert_eq!(full, 6, "registry is at the (shrunk) band edge");
        assert_eq!(
            reserve_proxy_id(full),
            None,
            "the next new Proxy(...) must be refused, not minted out of band"
        );

        // Everything minted before the edge still works.
        for proxy in minted {
            assert_eq!(js_proxy_is_proxy(proxy), 1);
            assert_eq!(js_proxy_is_revoked(proxy), 0);
        }
    }

    /// The live witness for #8213: what a program that runs off the end of the
    /// band actually gets. It cannot be observed in-process — `js_throw` with
    /// no open `try` prints the uncaught error and `process::exit(1)`s — so the
    /// child re-runs this test against a shrunk band and the parent asserts on
    /// its exit status and output. `Some(1)` versus a signal death is exactly
    /// the difference this fix makes: before, the 65,536th `new Proxy(...)`
    /// returned a payload the next property read dereferenced (SIGSEGV, no
    /// status code at all).
    ///
    /// It also covers the two things a unit test on `reserve_proxy_id` cannot:
    /// that the registry borrow is released before the throw (an open borrow
    /// would panic the moment the error allocation reaches the GC's proxy
    /// scanner), and that the error is allocatable at all with a full registry.
    #[test]
    fn exhausting_the_band_reports_a_range_error_instead_of_segfaulting() {
        // Harness plumbing, deliberately not a `PERRY_GC_*` name (that family
        // is audited by `scripts/check_gc_env_knobs.py`).
        const CHILD_ENV: &str = "PERRY_TEST_PROXY_BAND_EXHAUSTION_CHILD";

        if std::env::var_os(CHILD_ENV).is_some() {
            // Index 0 is reserved, so a length of 4 leaves 3 usable ids and the
            // 4th call is the one past the edge.
            PROXY_ID_BAND_LEN_OVERRIDE.with(|c| c.set(Some(4)));
            for _ in 0..8 {
                js_proxy_new(obj_value(), obj_value());
            }
            unreachable!("js_proxy_new must not mint an id past the band edge");
        }

        let output = std::process::Command::new(
            std::env::current_exe().expect("current test binary"),
        )
        .arg("proxy::tests::exhausting_the_band_reports_a_range_error_instead_of_segfaulting")
        .arg("--exact")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .output()
        .expect("launch the band-exhaustion witness");

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "exhaustion must exit(1) after an uncaught RangeError, not die on a \
             signal and not run past the edge; output was:\n{combined}"
        );
        assert!(
            combined.contains("RangeError"),
            "the uncaught error must be a RangeError; output was:\n{combined}"
        );
        assert!(
            combined.contains("Too many proxies"),
            "the message must name the exhausted registry; output was:\n{combined}"
        );
    }

    /// 2026-07-09 GC audit (wave 2 batch A): revocation must DETACH — null the
    /// registry entry's target/handler so `scan_proxy_roots_mut` stops rooting
    /// the wrapped graphs — while `revoked` keeps gating every trap path.
    #[test]
    fn revoke_detaches_target_and_handler_slots() {
        let target = obj_value();
        let handler = obj_value();
        let proxy = js_proxy_new(target, handler);

        assert_eq!(js_proxy_is_revoked(proxy), 0);
        assert_eq!(js_proxy_target(proxy).to_bits(), target.to_bits());
        assert_eq!(js_proxy_handler(proxy).to_bits(), handler.to_bits());

        js_proxy_revoke(proxy);
        assert_eq!(js_proxy_is_revoked(proxy), 1);

        let id = lookup(proxy).expect("revoked proxy stays a registered proxy");
        let (target_bits, handler_bits) = PROXIES.with(|p| {
            let v = p.borrow();
            let entry = v[id as usize].as_ref().expect("entry present");
            (entry.target.to_bits(), entry.handler.to_bits())
        });
        assert_eq!(target_bits, 0, "revoke must null [[ProxyTarget]]");
        assert_eq!(handler_bits, 0, "revoke must null [[ProxyHandler]]");

        // Detached slots are reported as `undefined`, never as raw 0.0.
        assert_eq!(js_proxy_target(proxy).to_bits(), TAG_UNDEFINED);
        assert_eq!(js_proxy_handler(proxy).to_bits(), TAG_UNDEFINED);

        // Idempotent.
        js_proxy_revoke(proxy);
        assert_eq!(js_proxy_is_revoked(proxy), 1);
    }

    /// `typeof` of a revoked proxy is unchanged (spec: [[Call]] presence is
    /// fixed at creation), so callability must survive target detachment —
    /// including through a nested proxy whose inner proxy gets revoked.
    #[test]
    fn revoked_proxy_keeps_creation_callability() {
        extern "C" fn dummy_fn(_closure: *const crate::closure::ClosureHeader) -> f64 {
            f64::from_bits(TAG_UNDEFINED)
        }
        let f = crate::closure::js_closure_alloc(dummy_fn as *const u8, 0);
        let f_val = f64::from_bits(POINTER_TAG | ((f as u64) & POINTER_MASK));
        let handler = obj_value();

        let callable_proxy = js_proxy_new(f_val, handler);
        assert!(proxy_wraps_callable(callable_proxy));
        let nested = js_proxy_new(callable_proxy, handler);
        assert!(
            proxy_wraps_callable(nested),
            "nested proxy inherits the inner proxy's creation-time snapshot"
        );

        js_proxy_revoke(callable_proxy);
        assert!(
            proxy_wraps_callable(callable_proxy),
            "typeof of a revoked proxy must be unchanged despite the nulled target"
        );
        assert!(proxy_wraps_callable(nested));

        // And a plain-object proxy stays non-callable across revocation.
        let plain_proxy = js_proxy_new(obj_value(), obj_value());
        assert!(!proxy_wraps_callable(plain_proxy));
        js_proxy_revoke(plain_proxy);
        assert!(!proxy_wraps_callable(plain_proxy));
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    fn boxed_object(obj: *mut crate::ObjectHeader) -> f64 {
        f64::from_bits(POINTER_TAG | (obj as u64 & POINTER_MASK))
    }

    fn boxed_interned_key(keys: *mut crate::ArrayHeader, slot: u32, name: &[u8]) -> f64 {
        let key = crate::array::js_array_get(keys, slot).as_string_ptr();
        let key = crate::string::js_string_intern(key, fnv1a(name));
        f64::from_bits(crate::value::STRING_TAG | (key as u64 & POINTER_MASK))
    }

    fn object_array_numeric_write_guard(array: f64, keys: &[f64], count: u32) -> u64 {
        assert!((1..=4).contains(&keys.len()));
        let mut padded = [0.0; 4];
        padded[..keys.len()].copy_from_slice(keys);
        put_value::js_object_array_numeric_write_guard(
            array,
            padded[0],
            padded[1],
            padded[2],
            padded[3],
            keys.len() as u32,
            count,
        )
    }

    fn object_array_numeric_write_range_guard(
        array: f64,
        keys: &[f64],
        start: u32,
        end: u32,
    ) -> u64 {
        assert!((1..=4).contains(&keys.len()));
        let mut padded = [0.0; 4];
        padded[..keys.len()].copy_from_slice(keys);
        put_value::js_object_array_numeric_write_range_guard(
            array,
            padded[0],
            padded[1],
            padded[2],
            padded[3],
            keys.len() as u32,
            start,
            end,
        )
    }

    /// #6809/#6812: the whole-loop preflight may only publish raw slot indexes
    /// when every array element has the same writable data layout. The
    /// generated clone performs no checks after this result, so heterogeneous
    /// shapes, holes, descriptor flags, class-id-zero objects, and forged or
    /// stale typed-layout state must all reject.
    #[test]
    fn object_array_numeric_write_guard_requires_complete_uniform_proof() {
        let packed = b"a\0b\0c\0d\0";
        let keys = crate::object::js_build_class_keys_array(
            0x6809_01,
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let first = crate::object::js_object_alloc_class_inline_keys(0x6809_01, 0, 4, keys);
        let second = crate::object::js_object_alloc_class_inline_keys(0x6809_01, 0, 4, keys);
        let values = [boxed_object(first), boxed_object(second)];
        let array = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
        let array_box = boxed_object(array.cast());
        let a = boxed_interned_key(keys, 0, b"a");
        let b = boxed_interned_key(keys, 1, b"b");
        let c = boxed_interned_key(keys, 2, b"c");
        let d = boxed_interned_key(keys, 3, b"d");

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[c], 2),
            3,
            "one-field loops should publish one non-zero 16-bit lane"
        );
        let plain_a_ptr = crate::string::js_string_from_bytes(b"a".as_ptr(), 1);
        let plain_a_gc =
            unsafe { crate::value::addr_class::try_read_gc_header(plain_a_ptr as usize) }
                .expect("fresh string header");
        assert_eq!(
            plain_a_gc.gc_flags & crate::gc::GC_FLAG_INTERNED,
            0,
            "the coverage key must exercise a non-interned string-pool handle"
        );
        let plain_a =
            f64::from_bits(crate::value::STRING_TAG | (plain_a_ptr as u64 & POINTER_MASK));
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[plain_a], 2),
            1,
            "the once-only content lookup must not depend on unrelated interning"
        );
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
            (4u64 << 48) | (3u64 << 32) | (2u64 << 16) | 1,
            "four inline slots should be published in source order"
        );
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[c, c, d], 2),
            (4u64 << 32) | (3u64 << 16) | 3,
            "duplicate target keys must preserve ordered duplicate stores"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write2_guard(array_box, c, d, 2),
            (4u64 << 32) | 3,
            "the cached-object compatibility ABI must keep its 32-bit lanes"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write_guard(array_box, a, b, c, d, 0, 2),
            0,
            "zero active fields must reject"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write_guard(array_box, a, b, c, d, 5, 2),
            0,
            "field counts beyond the fixed descriptor must reject"
        );

        unsafe {
            let header =
                (second as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let original = (*header)._reserved;

            for (flag, reason) in [
                (
                    crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
                    "descriptor-bearing receivers",
                ),
                (crate::gc::OBJ_FLAG_FROZEN, "frozen receivers"),
                (crate::gc::OBJ_FLAG_SEALED, "sealed receivers"),
                (crate::gc::OBJ_FLAG_NO_EXTEND, "non-extensible receivers"),
                (
                    crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO,
                    "typed-array-prototype receivers",
                ),
            ] {
                (*header)._reserved = original | flag;
                assert_eq!(
                    object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
                    0,
                    "{reason} must use ordinary [[Set]]"
                );
            }

            (*header)._reserved = original | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
                0,
                "an intact typed-layout bit without its descriptor must reject"
            );
            (*header)._reserved = original;
        }

        for object in [first, second] {
            crate::gc::js_gc_init_typed_shape_layout(
                object as u64,
                4,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
        }
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
            (4u64 << 48) | (3u64 << 32) | (2u64 << 16) | 1,
            "finite numbers are valid in verified ordinary JSValue typed slots"
        );

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[f64::NAN], 2),
            0,
            "non-interned/non-string keys must reject"
        );

        let hole_values = [boxed_object(first), f64::from_bits(crate::value::TAG_HOLE)];
        let hole_array =
            crate::array::js_array_from_f64(hole_values.as_ptr(), hole_values.len() as u32);
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(hole_array.cast()), &[c, d], 2),
            0,
            "a hole cannot be treated as an object receiver"
        );

        let other_keys = crate::object::js_build_class_keys_array(
            0x6809_02,
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let other = crate::object::js_object_alloc_class_inline_keys(0x6809_02, 0, 4, other_keys);
        let mixed_values = [boxed_object(first), boxed_object(other)];
        let mixed =
            crate::array::js_array_from_f64(mixed_values.as_ptr(), mixed_values.len() as u32);
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(mixed.cast()), &[c, d], 2),
            0,
            "content-equal but distinct shape keys arrays must not share raw slots"
        );

        let ranged_values = [
            boxed_object(other),
            boxed_object(first),
            boxed_object(second),
        ];
        let ranged =
            crate::array::js_array_from_f64(ranged_values.as_ptr(), ranged_values.len() as u32);
        let ranged_box = boxed_object(ranged.cast());
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 1, 3),
            (4u64 << 16) | 3,
            "a non-zero range must ignore an ineligible receiver before its source start"
        );
        assert_eq!(
            object_array_numeric_write_guard(ranged_box, &[c, d], 3),
            0,
            "the legacy prefix ABI must continue proving from element zero"
        );
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 3, 3),
            0,
            "an empty receiver range must reject"
        );
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 1, 4),
            0,
            "a receiver range may not outrun the array"
        );

        unsafe {
            let original = (*first).class_id;
            let first_header =
                (first as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let original_flags = (*first_header)._reserved;
            (*first).class_id = 0;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                0,
                "an UNMARKED class-id-zero object has no established ordinary-receiver \
                 identity and must use ordinary [[Set]]"
            );
            // #8098: the SAME receiver, birth-marked ORDINARY by the runtime —
            // which is what `JSON.parse` output carries — is eligible. The mark
            // is the only thing that differs between these two assertions, so
            // the pair discriminates "the guard reads the mark" from "the guard
            // stopped caring about class-id zero".
            (*first_header)._reserved = original_flags | crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                (4u64 << 16) | 3,
                "a marked ordinary plain object publishes the same raw slots as a \
                 class instance of the same shape"
            );
            // A native-module receiver stays out no matter what it is marked.
            (*first).class_id = crate::object::NATIVE_MODULE_CLASS_ID;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                0,
                "a native-module receiver must reject even when marked ordinary"
            );
            (*first_header)._reserved = original_flags;
            (*first).class_id = original;
        }

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 3),
            0,
            "the preflight must reject a receiver prefix longer than the array"
        );

        let wide_packed = b"a\0b\0c\0d\0e\0";
        let wide_keys = crate::object::js_build_class_keys_array(
            0x6812_03,
            5,
            wide_packed.as_ptr(),
            wide_packed.len() as u32,
        );
        let narrow = crate::object::js_object_alloc_class_inline_keys(0x6812_03, 0, 4, wide_keys);
        let narrow_values = [boxed_object(narrow)];
        let narrow_array =
            crate::array::js_array_from_f64(narrow_values.as_ptr(), narrow_values.len() as u32);
        let e = boxed_interned_key(wide_keys, 4, b"e");
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(narrow_array.cast()), &[e], 1),
            0,
            "a key beyond the four-slot physical allocation must reject"
        );
    }

    /// #8098: the write PIC's generated hit path re-tests the ordinary-plain
    /// mark as a raw `_reserved` bit literal, so the runtime constant and the
    /// literal `perry-codegen/src/expr/proxy_reflect.rs` emits
    /// (`PLAIN_ORDINARY_OBJ_FLAG`) are one ABI. A silent divergence would make
    /// every generated guard test the wrong bit — either admitting a receiver
    /// the runtime never cleared, or never hitting at all. Pin the value here.
    #[test]
    fn plain_ordinary_object_flag_matches_the_emitted_write_pic_literal() {
        assert_eq!(
            crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
            0x200,
            "perry-codegen emits 0x200 for this bit"
        );
        // It ADMITS a receiver, so it must not appear in the mask that REJECTS
        // one (`WRITE_PIC_BLOCKING_FLAGS = 0x1907`) — a collision would make
        // every marked object permanently ineligible.
        assert_eq!(crate::gc::OBJ_FLAG_PLAIN_ORDINARY & 0x1907, 0);
        // Bit 9 is shared with the array-only arguments-object flag, disjoint
        // by `obj_type`; and it must not collide with any object-meaningful
        // flag or with the survival-age / layout-state fields the GC owns.
        for other in [
            crate::gc::OBJ_FLAG_FROZEN,
            crate::gc::OBJ_FLAG_SEALED,
            crate::gc::OBJ_FLAG_NO_EXTEND,
            crate::gc::OBJ_FLAG_NULL_PROTO,
            crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO,
            crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
            crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT,
            0x0038, // GC_COPY_SURVIVAL_AGE_MASK
            0xC000, // GC_LAYOUT_STATE_MASK
        ] {
            assert_eq!(
                crate::gc::OBJ_FLAG_PLAIN_ORDINARY & other,
                0,
                "the ordinary-plain mark must own its own bit"
            );
        }
    }

    /// #8098 end-to-end: real `JSON.parse` output must reach the whole-loop
    /// numeric write clone.
    ///
    /// This drives the shipped parser rather than hand-building a class-less
    /// object, because the property under test is that the PARSER marks what it
    /// allocates — a guard relaxation with no marking site would leave the
    /// matrix's `receiver_class_id_zero` cell exactly where it was.
    ///
    /// The 13-byte object payload is deliberately below the tape's 1 KB floor
    /// and has an object root, so `js_json_parse` takes the eager
    /// `DirectParser` (`json/parse_api.rs`) — no lazy tape stands between this
    /// probe and the objects it inspects (#7635).
    #[test]
    fn json_parse_receivers_are_admitted_to_the_whole_loop_write_clone() {
        let src = br#"{"x":0,"y":0}"#;
        let mut receivers = Vec::new();
        for _ in 0..4 {
            let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
            let value = unsafe { crate::json::js_json_parse(text) };
            assert!(value.is_pointer(), "JSON.parse must yield an object");
            receivers.push(f64::from_bits(value.bits()));
        }
        let array = crate::array::js_array_from_f64(receivers.as_ptr(), receivers.len() as u32);
        let array_box = boxed_object(array.cast());

        let objects: Vec<*mut crate::ObjectHeader> = receivers
            .iter()
            .map(|v| (v.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader)
            .collect();
        unsafe {
            assert_eq!(
                (*objects[0]).class_id,
                0,
                "the premise: parsed receivers carry no class id"
            );
            assert_ne!(
                crate::object::shapes::object_shape_stamp(objects[0]),
                0,
                "the premise: #8067/#8086 birth-stamps them with a real ShapeId"
            );
            for object in &objects[1..] {
                assert_eq!(
                    crate::object::shapes::object_shape_stamp(*object),
                    crate::object::shapes::object_shape_stamp(objects[0]),
                    "repeated parses share one keys array, hence one ShapeId"
                );
            }
        }

        let key_ptr = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
        let key_y = f64::from_bits(crate::value::STRING_TAG | (key_ptr as u64 & POINTER_MASK));
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[key_y], 4),
            2,
            "the whole-loop clone must publish slot 1 for a parsed receiver prefix"
        );

        // The discriminating quantity: clear the ordinary mark on ONE receiver
        // and nothing else. Same objects, same ShapeId, same keys, same slots —
        // if the guard still accepted, it would not be reading the mark.
        unsafe {
            let header =
                (objects[2] as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let saved = (*header)._reserved;
            assert_ne!(
                saved & crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
                0,
                "the parser must mark what it allocates"
            );
            (*header)._reserved = saved & !crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[key_y], 4),
                0,
                "one unmarked receiver in the prefix must send the whole nest to \
                 ordinary [[Set]]"
            );
            (*header)._reserved = saved;
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[key_y], 4),
                2,
                "restoring the mark restores eligibility"
            );
        }
    }

    /// #8098: the same admission on the per-site static write PIC, which is the
    /// path scattered `record.field = …` writes take (the whole-loop clone only
    /// covers a constant-counted nest). A miss that refuses to prime leaves the
    /// site on the runtime path forever.
    #[test]
    fn json_parse_receivers_prime_the_static_write_pic() {
        let src = br#"{"n":1}"#;
        let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
        let value = unsafe { crate::json::js_json_parse(text) };
        assert!(value.is_pointer());
        let target = f64::from_bits(value.bits());
        let object = (value.bits() & POINTER_MASK) as *mut crate::ObjectHeader;

        let key_ptr = crate::string::js_string_from_bytes(b"n".as_ptr(), 1);
        let key_ptr = crate::string::js_string_intern(key_ptr, fnv1a(b"n"));

        let mut cache = [0i64; 2];
        let stored = put_value::js_put_value_set_ic_miss(target, key_ptr, 7.0, 0, &mut cache);
        assert_eq!(stored, 7.0);
        let expected_token = unsafe {
            crate::object::shapes::PIC_ID_TOKEN_BIT
                | crate::object::shapes::object_shape_id(object) as u64
        };
        assert_eq!(
            cache[0] as u64, expected_token,
            "a parsed receiver must prime the way with its own ShapeId token"
        );
        assert_eq!(cache[1], 0, "`n` is the receiver's first own slot");

        // Discriminating half: an otherwise identical parsed receiver with the
        // ordinary mark cleared must NOT prime.
        let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
        let value = unsafe { crate::json::js_json_parse(text) };
        let unmarked = (value.bits() & POINTER_MASK) as *mut crate::ObjectHeader;
        unsafe {
            let header =
                (unmarked as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*header)._reserved &= !crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
        }
        let mut cache2 = [0i64; 2];
        let stored = put_value::js_put_value_set_ic_miss(
            f64::from_bits(value.bits()),
            key_ptr,
            9.0,
            0,
            &mut cache2,
        );
        assert_eq!(stored, 9.0, "the write itself still succeeds");
        assert_eq!(
            cache2[0], 0,
            "an unmarked class-less receiver must stay on the miss path"
        );
    }

    /// #7531: `create_list_from_array_like` backs `Reflect.apply(target,
    /// thisArg, argumentsList)` / `Reflect.construct` -- `argumentsList` is
    /// caller-supplied and can be a fetch/zlib/proxy/common-registry handle
    /// id under the same POINTER_TAG as a real Array. The OLD "is this an
    /// Array?" fast path was a bare magnitude floor
    /// (`GC_HEADER_SIZE + 0x1000` = 0x1008) with NO other guard before
    /// dereferencing `addr - GC_HEADER_SIZE` -- every probe below is at/above
    /// that floor, so the old code would have derefed unconditionally.
    /// Calling the real function end-to-end (not just the predicate) proves
    /// the fast path is skipped without crashing; Node's semantics for a
    /// lengthless array-like is an empty list.
    #[test]
    fn create_list_from_array_like_rejects_handle_band_fast_path_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::ZLIB_HANDLE_BAND_START,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(POINTER_TAG | (addr as u64));
            assert!(
                create_list_from_array_like(boxed).is_empty(),
                "{addr:#x} must not be misread as a live Array"
            );
        }
    }

    #[test]
    fn create_list_from_array_like_unpacks_arguments_objects() {
        let raw = crate::array::js_array_alloc(3);
        let raw = crate::array::js_array_push_f64(raw, 11.0);
        let raw = crate::array::js_array_push_f64(raw, 22.0);
        let raw = crate::array::js_array_push_f64(raw, 33.0);
        let raw_args = crate::value::js_nanbox_pointer(raw as i64);
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let arguments = crate::object::js_arguments_object_alloc(raw_args, undefined, 0);
        let boxed_arguments = crate::value::js_nanbox_pointer(arguments as i64);

        assert_eq!(
            create_list_from_array_like(boxed_arguments),
            vec![11.0, 22.0, 33.0],
            "Reflect.apply must preserve every entry from a real arguments object"
        );
    }

    /// #7531: `raw_ptr_from_value` feeds `array_ptr_from_value`, which derefs
    /// `raw - GC_HEADER_SIZE` right after a magnitude-only `is_valid_obj_ptr`
    /// guard. The OLD floor here (`GC_HEADER_SIZE + 0x1000`) sat below every
    /// handle band, and the local variable name `raw` made it invisible to
    /// `scripts/addr_class_inventory.py`'s `handle-floor` regex.
    #[test]
    fn array_ptr_from_value_rejects_every_handle_band_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::ZLIB_HANDLE_BAND_START,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(POINTER_TAG | (addr as u64));
            assert!(
                array_ptr_from_value(boxed).is_none(),
                "{addr:#x} must not resolve to an ArrayHeader"
            );
        }
    }

    /// #7531: `prototype_of_for_set` backs `Reflect.getPrototypeOf` /
    /// `is(value, type)` prototype walks. `lookup(value)` only recognizes an
    /// ALREADY-REGISTERED revocable Proxy id; any other handle-band value
    /// (fetch/zlib/common-registry, or a foreign Proxy id) falls through to
    /// the POINTER_TAG block, whose OLD floor
    /// (`GC_HEADER_SIZE + 0x1000`) admitted it into
    /// `object_static_prototype` (safe) and then a magnitude-only
    /// `is_valid_obj_ptr(obj)` guard followed by an unconditional
    /// `(*obj).class_id` read.
    #[test]
    fn prototype_of_for_set_rejects_every_handle_band_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::ZLIB_HANDLE_BAND_START,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(POINTER_TAG | (addr as u64));
            // Reaching this assertion at all (no SIGSEGV) is half the test.
            let _ = prototype_of_for_set(boxed);
        }
    }
}
