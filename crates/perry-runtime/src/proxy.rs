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

use std::cell::RefCell;
use std::collections::HashMap;

use crate::closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

mod apply_construct;
pub(crate) use apply_construct::{is_callable_function, is_constructor_function};
pub use apply_construct::{js_proxy_apply, js_proxy_construct};
mod has_delete;
pub(crate) use has_delete::reflect_ordinary_delete_property_key;
pub use has_delete::{js_proxy_delete, js_proxy_has};
mod invariants;
mod put_value;
pub(crate) use put_value::proxy_set_with_receiver;
pub use put_value::{js_proxy_set, js_put_value_set};
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
/// wrapped graphs can die — `scan_proxy_roots_mut` otherwise strongly roots
/// them for the life of the registry slot (2026-07-09 GC audit). No valid
/// proxy target/handler is ever the all-zero-bits number `0.0` (both must be
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

fn encode_proxy_id(id: u64) -> i64 {
    (PROXY_TAG_BASE + id) as i64
}

fn decode_proxy_id(raw: i64) -> Option<u64> {
    let raw = raw as u64;
    if raw < PROXY_TAG_BASE {
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
    // Only a real entry in the registry counts as a proxy.
    PROXIES.with(|p| {
        let v = p.borrow();
        if (id as usize) < v.len() && v[id as usize].is_some() {
            Some(id)
        } else {
            None
        }
    })
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
    PROXIES.with(|p| {
        let mut v = p.borrow_mut();
        let id = v.len() as u64;
        v.push(Some(Box::new(ProxyEntry {
            target,
            handler,
            revoked: false,
            callable,
        })));
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
        if ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
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
    let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
    let key_ptr =
        crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
    if obj_ptr.is_null() || key_ptr.is_null() {
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
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(target, property_key, value);
        }
        return;
    }
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
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
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

    let obj_ptr = extract_pointer(target.to_bits()) as usize;
    if obj_ptr == 0 {
        return None;
    }
    let key_name = key_to_rust_string(key)?;
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
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
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
                        let fast_safe = if class_id == 0 {
                            // Plain object: prototype is exactly Object.prototype, and
                            // Object.prototype doesn't intercept this key (per-key, not
                            // the coarse process-wide descriptor flag — that made wide
                            // builds O(n²)).
                            crate::object::prototype_chain::object_static_prototype(addr).is_none()
                                && !crate::object::object_proto_may_intercept_key(key)
                        } else {
                            // Class instance: the `class_id == 0` guard previously sent
                            // EVERY wide class-instance build down the O(own-key) slow
                            // walk (O(n²)). Safe to fast-path when no inherited accessor /
                            // non-writable anywhere in the prototype chain could intercept
                            // this key.
                            !crate::object::class_instance_set_may_intercept(addr, class_id, key)
                        };
                        if fast_safe {
                            target_set(target, key, value);
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
                                module_name,
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
#[used]
static KEEP_PROXY_REVOCABLE: extern "C" fn(f64, f64) -> f64 = js_proxy_revocable;

// #2762: retention anchors for the Reflect-specific extensibility entry points.
// These `#[no_mangle]` fns are emitted only by codegen (no Rust caller in the
// crate graph), so the auto-optimize whole-program LLVM bitcode rebuild would
// otherwise internalize and dead-strip them. See node_stream_keepalive.rs.
#[used]
static KEEP_REFLECT_IS_EXTENSIBLE: extern "C" fn(f64) -> f64 = js_reflect_is_extensible;
#[used]
static KEEP_REFLECT_PREVENT_EXTENSIONS: extern "C" fn(f64) -> f64 = js_reflect_prevent_extensions;

// #2761: retention anchor for `Reflect.setPrototypeOf` (codegen-only callsite).
#[used]
static KEEP_REFLECT_SET_PROTOTYPE_OF: extern "C" fn(f64, f64) -> f64 = js_reflect_set_prototype_of;

// #2763/#2764/#2766/#2767: retention anchors for the Reflect entry points
// whose only callsites are codegen-emitted. `js_reflect_get` gained a third
// `receiver` arg (#2766) and must keep its new signature retained.
#[used]
static KEEP_REFLECT_GET: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_get;
#[used]
static KEEP_REFLECT_GET_OWN_PROPERTY_DESCRIPTOR: extern "C" fn(f64, f64) -> f64 =
    js_reflect_get_own_property_descriptor;
#[used]
static KEEP_REFLECT_HAS: extern "C" fn(f64, f64) -> f64 = js_reflect_has;
#[used]
static KEEP_REFLECT_OWN_KEYS: extern "C" fn(f64) -> f64 = js_reflect_own_keys;
#[used]
static KEEP_REFLECT_APPLY: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_apply;

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

/// GC scanner for the proxy registry + reflect-metadata store (2026-07-02
/// audit P0; ported from the stranded be73b4f8d). A proxy's target/handler
/// are commonly reachable ONLY through `PROXIES` — without visiting them a
/// minor GC collects (or moves) them and every subsequent trap derefs freed
/// or stale memory. `REFLECT_METADATA`'s own doc admits its keys go stale on
/// a target move; rekey them during the metadata-rewrite phase.
pub(crate) fn scan_proxy_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROXIES.with(|proxies| {
        for entry in proxies.borrow_mut().iter_mut().flatten() {
            visitor.visit_nanbox_f64_slot(&mut entry.target);
            visitor.visit_nanbox_f64_slot(&mut entry.handler);
        }
    });

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
mod tests {
    use super::*;

    fn obj_value() -> f64 {
        let obj = crate::object::js_object_alloc(0, 0);
        f64::from_bits(POINTER_TAG | ((obj as u64) & POINTER_MASK))
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
}
