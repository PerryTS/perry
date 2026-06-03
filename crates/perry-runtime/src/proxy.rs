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
//! flipped; subsequent operations return an error NaN-boxed value.
//!
//! We deliberately do NOT patch generic object.rs/field dispatch — Perry
//! codegen rewrites known Proxy locals to ProxyGet/ProxySet/etc. variants at
//! HIR lowering time, which route through the entry points here.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::closure::{js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3};

/// A single Proxy registry entry.
#[repr(C)]
pub struct ProxyEntry {
    pub target: f64,  // NaN-boxed target value
    pub handler: f64, // NaN-boxed handler object (raw f64 bits preserved)
    pub revoked: bool,
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
/// runtime's `< 0x100000` small-handle band so Web Fetch handles can occupy a
/// broad disjoint range below this without sharing visible `POINTER_TAG | id`
/// bits with a proxy. Any operation on a proxy MUST go through the Proxy*
/// dispatch helpers in this module.
const PROXY_TAG_BASE: u64 = 0x000F_0000;

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
        return ptr >= 0x1000;
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

#[no_mangle]
pub extern "C" fn js_proxy_new(target: f64, handler: f64) -> f64 {
    // #2846: validate both arguments are objects before allocating.
    if !proxy_arg_is_object(target) || !proxy_arg_is_object(handler) {
        throw_proxy_non_object();
    }
    PROXIES.with(|p| {
        let mut v = p.borrow_mut();
        let id = v.len() as u64;
        v.push(Some(Box::new(ProxyEntry {
            target,
            handler,
            revoked: false,
        })));
        let encoded = encode_proxy_id(id) as u64;
        f64::from_bits(POINTER_TAG | (encoded & POINTER_MASK))
    })
}

/// Revoke a proxy. Subsequent operations will return TAG_UNDEFINED or fire an
/// exception where the compiler inserts one.
#[no_mangle]
pub extern "C" fn js_proxy_revoke(proxy_boxed: f64) {
    if let Some(id) = lookup(proxy_boxed) {
        PROXIES.with(|p| {
            if let Some(Some(entry)) = p.borrow_mut().get_mut(id as usize) {
                entry.revoked = true;
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

/// Return the proxy's target (for Proxy.revocable.proxy revocation checks).
#[no_mangle]
pub extern "C" fn js_proxy_target(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.target)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Return the proxy's handler for `util.inspect(..., { showProxy: true })`.
#[no_mangle]
pub extern "C" fn js_proxy_handler(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.handler)
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
    let msg = "Cannot perform operation on a proxy that has been revoked";
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

/// Is `value` a Reflect-acceptable object? Heap objects, class refs (callable
/// constructors), and proxies all count. Primitives / null / undefined do not.
fn reflect_value_is_object(value: f64) -> bool {
    if lookup(value).is_some() {
        return true;
    }
    if crate::object::js_value_is_heap_object(value) {
        return true;
    }
    // Class refs (INT32-tagged constructors) are callable objects.
    (value.to_bits() >> 48) == 0x7FFE
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
        return js_closure_call2(closure_from(trap), target, key);
    }
    // No get trap — forward to target.
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
        if raw > 0 && raw < 0x10000 {
            return Some(raw);
        }
    } else if top == 0 && bits > 0 && bits < 0x10000 {
        return Some(bits as i64);
    }
    None
}

fn set_handle_property(target: f64, key: f64, value: f64) -> Option<bool> {
    let handle = small_handle_from_value(target)?;
    let Some(name) = key_to_rust_string(key) else {
        return Some(false);
    };
    if let Some(dispatch) = crate::object::handle_property_set_dispatch() {
        unsafe { dispatch(handle, name.as_ptr(), name.len(), value) };
    }
    Some(true)
}

fn target_get(target: f64, key: f64) -> f64 {
    let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
    let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
    if obj_ptr.is_null() || key_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
}

/// `proxy[key] = value` — if handler.set exists, call it with
/// (target, key, value) and return TAG_TRUE (the trap's return value is
/// ignored by the default test semantics since we echo `value`). Otherwise
/// forward to the target directly.
#[no_mangle]
pub extern "C" fn js_proxy_set(proxy_boxed: f64, key: f64, value: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let trap = handler_trap(handler, "set");
    if is_callable(trap) {
        // #2756: the `set` trap's boolean result is observable through
        // `Reflect.set(proxy, …)` (and strict-mode assignment). Coerce and
        // return it rather than discarding it.
        let trap_result = js_closure_call3(closure_from(trap), target, key, value);
        return coerce_trap_bool(trap_result);
    }
    // No set trap — write to target and report the ordinary [[Set]] result.
    reflect_ordinary_set(target, key, value)
}

/// Perform an ordinary (non-proxy) `[[Set]]` and report success as a NaN-boxed
/// boolean, without throwing on a non-writable / non-extensible target the way
/// strict-mode assignment does (#2756 / #615). Returns `false` when the write
/// cannot be applied.
fn reflect_ordinary_set(target: f64, key: f64, value: f64) -> f64 {
    nanbox_bool(ordinary_set_with_receiver(target, key, value, target))
}

fn target_set(target: f64, key: f64, value: f64) {
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(target, property_key, value);
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
    let key_ptr = extract_pointer(property_key.to_bits()) as *const crate::StringHeader;
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

fn key_is_length(key: f64) -> bool {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return false;
    };
    if ptr.is_null() || len != 6 {
        return false;
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) == b"length" }
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

fn own_set_descriptor(target: f64, key: f64) -> Option<OwnSetDescriptor> {
    if small_handle_from_value(target).is_some() {
        return None;
    }

    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        let value = unsafe { crate::symbol::js_object_get_symbol_property(target, key) };
        return (value.to_bits() != TAG_UNDEFINED)
            .then_some(OwnSetDescriptor::Data { writable: true });
    }

    let obj_ptr = extract_pointer(target.to_bits()) as usize;
    if obj_ptr == 0 {
        return None;
    }
    let key_name = key_to_rust_string(key)?;
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
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as usize;
        if raw >= (crate::gc::GC_HEADER_SIZE as usize) + 0x1000 {
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

fn create_or_update_receiver_property(receiver: f64, key: f64, value: f64) -> bool {
    if !reflect_value_is_object(receiver) {
        return false;
    }
    if let Some(desc) = own_set_descriptor(receiver, key) {
        match desc {
            OwnSetDescriptor::Data { writable } => {
                if !writable {
                    return false;
                }
            }
            OwnSetDescriptor::Accessor { setter_bits } => {
                return call_setter_with_receiver(setter_bits, receiver, value);
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

    let mut current = target;
    for _ in 0..64 {
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
            return create_or_update_receiver_property(receiver, key, value);
        }
        let Some(proto) = prototype_of_for_set(current) else {
            return create_or_update_receiver_property(receiver, key, value);
        };
        current = proto;
    }
    false
}

/// Assignment PutValue for a property reference. Returns the assigned RHS value
/// on success or sloppy failure, and throws TypeError when strict code attempts
/// a failed [[Set]].
#[no_mangle]
pub extern "C" fn js_put_value_set(
    target: f64,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let target = target_handle.get_nanbox_f64();
    let key = key_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let receiver = receiver_handle.get_nanbox_f64();
    let property_key_handle =
        scope.root_nanbox_f64(unsafe { crate::object::js_to_property_key(key) });
    let property_key = property_key_handle.get_nanbox_f64();

    if lookup(target).is_none() {
        if set_integer_indexed_exotic(target, property_key, value) {
            return value;
        }
        if target.to_bits() == receiver.to_bits() && key_is_length(property_key) {
            if let Some(arr) = array_ptr_from_value(target) {
                crate::array::js_array_set_length(arr, value);
                return value;
            }
        }
    }

    let target_bits = target.to_bits();
    if target_bits == TAG_NULL || target_bits == TAG_UNDEFINED {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        let msg = format!("Cannot set properties of null or undefined (setting '{key_name}')");
        return throw_type_error(&msg);
    }
    let ok = if lookup(target).is_some() {
        js_proxy_set(target, property_key, value).to_bits() == TAG_TRUE
    } else {
        ordinary_set_with_receiver(target, property_key, value, receiver)
    };
    if !ok && strict != 0 {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        crate::error::throw_immutable_write(0, &key_name);
    }
    value_handle.get_nanbox_f64()
}

/// `key in proxy` — if handler.has exists, call it; otherwise delegate to
/// `js_object_has_property` on the target.
#[no_mangle]
pub extern "C" fn js_proxy_has(proxy_boxed: f64, key: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let trap = handler_trap(handler, "has");
    if is_callable(trap) {
        return js_closure_call2(closure_from(trap), target, key);
    }
    crate::object::js_object_has_property(target, key)
}

/// `delete proxy[key]` — if handler.deleteProperty exists, call it; else
/// delegate to `js_object_delete_field` on the target.
#[no_mangle]
pub extern "C" fn js_proxy_delete(proxy_boxed: f64, key: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let trap = handler_trap(handler, "deleteProperty");
    if is_callable(trap) {
        // #2760: the `deleteProperty` trap's boolean result is observable
        // through `Reflect.deleteProperty(proxy, …)`.
        let trap_result = js_closure_call2(closure_from(trap), target, key);
        return coerce_trap_bool(trap_result);
    }
    // Forward to target with ordinary `[[Delete]]` semantics.
    reflect_ordinary_delete(target, key)
}

/// Perform an ordinary (non-proxy) `[[Delete]]` and report the result as a
/// NaN-boxed boolean. Returns `false` for a non-configurable property (#2760),
/// matching `Reflect.deleteProperty` rather than the silent-success behavior of
/// the `delete` operator.
fn reflect_ordinary_delete(target: f64, key: f64) -> f64 {
    if let Some((_writable, configurable)) = crate::object::obj_value_attrs(target, key) {
        if !configurable {
            return nanbox_bool(false);
        }
    }
    let obj_ptr = extract_pointer(target.to_bits()) as *mut crate::ObjectHeader;
    let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
    if !obj_ptr.is_null() && !key_ptr.is_null() {
        let deleted = crate::object::js_object_delete_field(obj_ptr, key_ptr);
        return nanbox_bool(deleted != 0);
    }
    nanbox_bool(true)
}

/// Is `value` a callable function value: a closure, a class-ref constructor, or
/// a (possibly callable) proxy? Distinct from `is_callable`, which treats *any*
/// pointer-tagged value as callable — that's too loose for trap validation,
/// where a present-but-non-callable trap (e.g. `apply: {}`) must throw a
/// `TypeError` rather than be silently invoked as a no-op.
fn is_callable_function(value: f64) -> bool {
    let bits = value.to_bits();
    // Class-ref constructors (INT32-tagged, top16 == 0x7FFE) are callable.
    if (bits >> 48) == 0x7FFE {
        return true;
    }
    // A proxy whose target is callable is itself callable.
    if lookup(value).is_some() {
        return true;
    }
    // A POINTER_TAG value is callable only if it points at a closure.
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let raw = (bits & POINTER_MASK) as usize;
        return crate::closure::is_closure_ptr(raw);
    }
    false
}

/// Forward a `[[Call]]` to `target` (the default behavior when a proxy has no
/// `apply` trap). If `target` is itself a proxy, recurse so its own trap chain
/// runs; otherwise invoke the target through the canonical value-call path with
/// `this_arg` bound via `IMPLICIT_THIS`. Routing through `js_native_call_value`
/// (rather than calling the closure directly) also recovers built-in prototype
/// methods invoked as values — e.g. forwarding to `Object.prototype.hasOwnProperty`
/// re-dispatches by name with the receiver taken from `IMPLICIT_THIS`.
fn forward_apply(target: f64, this_arg: f64, args_array: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_apply(target, this_arg, args_array);
    }
    let args_bits = args_array.to_bits();
    let arr_ptr = (args_bits & POINTER_MASK) as *const crate::ArrayHeader;
    let len = if arr_ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr_ptr) as usize
    };
    let mut buf: Vec<f64> = Vec::with_capacity(len);
    for i in 0..len {
        let v = crate::array::js_array_get(arr_ptr, i as u32);
        buf.push(f64::from_bits(v.bits()));
    }
    let (ptr, n) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    let prev = crate::object::js_implicit_this_set(this_arg);
    let result = unsafe { crate::closure::js_native_call_value(target, ptr, n) };
    crate::object::js_implicit_this_set(prev);
    result
}

/// `proxy(arg0, arg1)` / `p.call(thisArg, …)` / `Reflect.apply(p, thisArg, …)`.
///
/// Implements the Proxy `[[Call]]` exotic behavior (#3656):
///   * trap absent / `undefined` / `null` → forward `[[Call]]` to the target,
///     binding `thisArg`;
///   * trap present but not callable → `TypeError`;
///   * trap present → `Call(trap, handler, «target, thisArg, argArray»)` — the
///     handler is the trap's `this`, and the trap's return value is returned
///     verbatim (no fallback to the target).
///
/// `args_array` is an already-constructed Array JSValue (NaN-boxed).
#[no_mangle]
pub extern "C" fn js_proxy_apply(proxy_boxed: f64, this_arg: f64, args_array: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
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
    let trap = handler_trap(handler, "apply");
    let trap_bits = trap.to_bits();
    // GetMethod: a missing / undefined / null trap means "use the default" —
    // forward the call to the target's [[Call]].
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return forward_apply(target, this_arg, args_array);
    }
    // Present-but-not-callable trap → TypeError.
    if !is_callable_function(trap) {
        return throw_type_error("proxy apply trap is not a function");
    }
    // Invoke the trap with the handler bound as `this` and the spec argument
    // list (target, thisArgument, argArray). Object-literal/free-function traps
    // read `this` from a closure slot and/or the IMPLICIT_THIS fallback, so we
    // set both — mirroring the `Reflect.get` accessor path.
    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy apply trap is not a function");
    }
    let prev = crate::object::js_implicit_this_set(handler);
    let result = js_closure_call3(closure, target, this_arg, args_array);
    crate::object::js_implicit_this_set(prev);
    result
}

/// Forward a `[[Construct]]` to `target` (the default behavior when a proxy has
/// no `construct` trap). Recurses through proxy targets; otherwise constructs a
/// fresh instance from the target function value.
fn forward_construct(target: f64, args_array: f64, new_target: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_construct(target, args_array, new_target);
    }
    if !is_callable_function(target) {
        return throw_type_error("target is not a constructor");
    }
    let buf = create_list_from_array_like(args_array);
    let (ptr, n) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    unsafe { crate::object::js_new_function_construct_with_new_target(target, ptr, n, new_target) }
}

/// `new Proxy(...)` / `Reflect.construct(p, args, newTarget)`.
///
/// Implements the Proxy `[[Construct]]` exotic behavior (#3656):
///   * trap absent / `undefined` / `null` → forward `[[Construct]]` to the
///     target (recursing through proxy targets), threading `newTarget`;
///   * trap present but not callable → `TypeError`;
///   * trap present → `Call(trap, handler, «target, argArray, newTarget»)` with
///     the handler bound as `this`. The trap's result must be an Object, else
///     `TypeError`.
///
/// `new_target` defaults to the proxy itself when the caller passes
/// `undefined` (the `new Proxy(...)` path).
#[no_mangle]
pub extern "C" fn js_proxy_construct(proxy_boxed: f64, args_array: f64, new_target: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
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
    // Default newTarget to the proxy itself (spec: `new P(...)` passes the
    // constructor being invoked, which is the proxy).
    let nt = if new_target.to_bits() == TAG_UNDEFINED {
        proxy_boxed
    } else {
        new_target
    };
    let trap = handler_trap(handler, "construct");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return forward_construct(target, args_array, nt);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy construct trap is not a function");
    }
    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy construct trap is not a function");
    }
    let prev = crate::object::js_implicit_this_set(handler);
    let result = js_closure_call3(closure, target, args_array, nt);
    crate::object::js_implicit_this_set(prev);
    // [[Construct]] must return an Object (spec step 9 of the construct trap).
    // Symbols are primitives despite being pointer-backed, so exclude them.
    let is_symbol = unsafe { crate::symbol::js_is_symbol(result) != 0 };
    if is_symbol || !reflect_value_is_object(result) {
        return throw_type_error("proxy [[Construct]] trap returned a non-object value");
    }
    result
}

fn array_from_args(args: &[f64]) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    let mut a = arr;
    for &arg in args {
        a = crate::array::js_array_push_f64(a, arg);
    }
    f64::from_bits(POINTER_TAG | ((a as u64) & POINTER_MASK))
}

#[no_mangle]
pub extern "C" fn js_reflect_construct(target: f64, args_like: f64, new_target: f64) -> f64 {
    if !is_callable_function(target) {
        return throw_type_error("target is not a constructor");
    }
    let nt = if new_target.to_bits() == TAG_UNDEFINED {
        target
    } else {
        new_target
    };
    if !is_callable_function(nt) {
        return throw_type_error("newTarget is not a constructor");
    }
    let args = create_list_from_array_like(args_like);
    if lookup(target).is_some() {
        let args_array = array_from_args(&args);
        return js_proxy_construct(target, args_array, nt);
    }
    let (ptr, n) = if args.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (args.as_ptr(), args.len())
    };
    unsafe { crate::object::js_new_function_construct_with_new_target(target, ptr, n, nt) }
}

// ---- Reflect.* helpers (direct wrappers, not proxy-specific) -----

/// `Reflect.get(target, key, receiver)` (#2766).
///
/// - throws `TypeError` for a non-object target,
/// - uses `receiver` as the `this` binding for accessor getters,
/// - dispatches proxy `get` traps (forwarding `(target, key)` to the existing
///   proxy path; the three-argument trap receiver is out of scope — Perry's
///   proxy traps are two-argument).
///
/// `receiver` is the optional third argument; codegen passes `target` when the
/// call site omits it (matching the spec default), and `undefined` is treated
/// as "use target".
#[no_mangle]
pub extern "C" fn js_reflect_get(target: f64, key: f64, receiver: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_get(target, key);
    }
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("get");
    }
    // Default receiver to target when undefined.
    let recv = if receiver.to_bits() == TAG_UNDEFINED {
        target
    } else {
        receiver
    };
    // #2766: if `key` resolves to an accessor *getter* on `target`, rebind its
    // `this` to the receiver and invoke it — object-literal getters capture
    // `this` in a reserved closure slot (not `IMPLICIT_THIS`), so plain
    // forwarding would read the target's fields, not the receiver's. When the
    // receiver equals the target we can skip the clone and use the ordinary
    // read.
    if recv.to_bits() != target.to_bits() {
        if let Some(getter_bits) = crate::object::reflect_getter_closure_bits(target, key) {
            if getter_bits == 0 {
                // Accessor with no getter → undefined.
                return f64::from_bits(TAG_UNDEFINED);
            }
            let rebound = crate::closure::clone_closure_rebind_this(getter_bits, recv);
            let closure = closure_from(f64::from_bits(rebound));
            if !closure.is_null() {
                // Also set IMPLICIT_THIS for free-function getters that read
                // `this` from the implicit-this fallback rather than a slot.
                let prev = crate::object::js_implicit_this_set(recv);
                let result = js_closure_call0(closure);
                crate::object::js_implicit_this_set(prev);
                return result;
            }
        }
    }
    let prev = crate::object::js_implicit_this_set(recv);
    let result = target_get(target, key);
    crate::object::js_implicit_this_set(prev);
    result
}

/// `Reflect.set(target, key, value)` — returns the boolean result of the
/// `[[Set]]` operation (#2756): `false` for a non-writable property or a new
/// key on a non-extensible object, and the coerced trap result for a proxy.
#[no_mangle]
pub extern "C" fn js_reflect_set(target: f64, key: f64, value: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_set(target, key, value);
    }
    reflect_ordinary_set(target, key, value)
}

/// `Reflect.has(target, key)` (#2764) — `[[HasProperty]]` semantics:
///
/// - throws `TypeError` for a non-object target,
/// - walks the recorded ordinary prototype chain (e.g. `Object.create(proto)`),
/// - dispatches to a proxy `has` trap (with `ToBoolean` coercion).
#[no_mangle]
pub extern "C" fn js_reflect_has(target: f64, key: f64) -> f64 {
    if lookup(target).is_some() {
        let trap_result = js_proxy_has(target, key);
        // #2764: normalize the trap result with ToBoolean.
        return coerce_trap_bool(trap_result);
    }
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("has");
    }
    // Own + (for class refs / closures) internal lookup.
    let own = crate::object::js_object_has_property(target, key);
    if own.to_bits() == TAG_TRUE {
        return own;
    }
    // #2764: `[[HasProperty]]` must also see inherited properties. Perry's
    // `js_object_has_property` only checks own keys, but the ordinary field
    // getter DOES walk the (Object.create / setPrototypeOf-recorded) prototype
    // chain. So probe via a field read: a non-`undefined` result means the
    // property resolves somewhere on the chain. (A genuinely
    // present-but-`undefined` inherited value is indistinguishable here, which
    // matches the own-undefined behavior of `js_object_has_property` and is
    // acceptable for the inherited case.)
    let inherited = target_get(target, key);
    if inherited.to_bits() != TAG_UNDEFINED {
        return nanbox_bool(true);
    }
    nanbox_bool(false)
}

/// `Reflect.deleteProperty(target, key)` — returns the boolean delete result
/// (#2760): `false` for a non-configurable property, and the coerced trap
/// result for a proxy.
#[no_mangle]
pub extern "C" fn js_reflect_delete(target: f64, key: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_delete(target, key);
    }
    reflect_ordinary_delete(target, key)
}

/// `Reflect.ownKeys(target)` (#2763) — returns string own-property names
/// followed by own symbol keys (Node order: integer-index then insertion-order
/// string keys, then symbols). Throws `TypeError` for a non-object target.
///
/// Proxy `ownKeys` traps are out of scope (Perry has no `ownKeys` trap
/// dispatch); a proxy target falls through to its registered target's keys.
#[no_mangle]
pub extern "C" fn js_reflect_own_keys(target: f64) -> f64 {
    // Resolve a proxy to its target so we enumerate real keys.
    let real = if let Some(id) = lookup(target) {
        PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.target)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        })
    } else {
        target
    };
    if !reflect_value_is_object(real) {
        return reflect_non_object_typeerror("ownKeys");
    }
    // String own names (this fn already throws for null/undefined; we've
    // validated above for the other primitives).
    let names = crate::object::js_object_get_own_property_names(real);
    let names_ptr = (names.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
    if names_ptr.is_null() {
        return names;
    }
    // Append own symbol keys (#2763).
    let syms_raw = unsafe { crate::symbol::js_object_get_own_property_symbols(real) };
    let syms_ptr = syms_raw as *const crate::array::ArrayHeader;
    if !syms_ptr.is_null() {
        let sym_count = crate::array::js_array_length(syms_ptr) as usize;
        let mut out = names_ptr;
        for i in 0..sym_count {
            let sym = crate::array::js_array_get(syms_ptr, i as u32);
            out = crate::array::js_array_push_f64(out, f64::from_bits(sym.bits()));
        }
        return f64::from_bits(POINTER_TAG | ((out as u64) & POINTER_MASK));
    }
    names
}

/// `Reflect.apply(fn, thisArg, argumentsList)` (#2767).
///
/// - throws `TypeError` for a non-callable target,
/// - implements `CreateListFromArrayLike(argumentsList)` (throws for a
///   non-object `argumentsList`, reads `0..length` from any array-like),
/// - binds `thisArg` for the call.
///
/// Proxy targets still dispatch to `js_proxy_apply` (which forwards the
/// already-constructed `args_array`). Proxy `apply` trap result fidelity for
/// an `undefined` trap return is out of scope here — Perry's proxy-apply path
/// keeps a pragmatic fallback (see `js_proxy_apply`).
#[no_mangle]
pub extern "C" fn js_reflect_apply(f: f64, this_arg: f64, args_array: f64) -> f64 {
    // If `f` is a proxy with apply trap, dispatch through it.
    if lookup(f).is_some() {
        return js_proxy_apply(f, this_arg, args_array);
    }
    // Non-callable target → TypeError (before evaluating argumentsList,
    // matching Node which reports the function check first).
    if !is_callable(f) {
        return throw_type_error("Reflect.apply target is not a function");
    }
    let args = create_list_from_array_like(args_array);
    call_with_this_and_args(f, this_arg, &args)
}

/// `Reflect.defineProperty(obj, key, descriptor)` — returns `false` when the
/// definition cannot be applied (#2758): defining a *new* property on a
/// non-extensible object, or redefining an existing *non-configurable*
/// property. Successful definitions return `true`. For a proxy target, the
/// coerced `defineProperty` trap result is returned.
#[no_mangle]
pub extern "C" fn js_reflect_define_property(obj: f64, key: f64, descriptor: f64) -> f64 {
    if lookup(obj).is_some() {
        let id = lookup(obj).unwrap();
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
        let trap = handler_trap(handler, "defineProperty");
        if is_callable(trap) {
            let trap_result = js_closure_call3(closure_from(trap), target, key, descriptor);
            return coerce_trap_bool(trap_result);
        }
        // No trap — define on the underlying target with ordinary semantics.
        return reflect_ordinary_define(target, key, descriptor);
    }
    reflect_ordinary_define(obj, key, descriptor)
}

/// Ordinary (non-proxy) `[[DefineOwnProperty]]` reporting success as a boolean.
fn reflect_ordinary_define(obj: f64, key: f64, descriptor: f64) -> f64 {
    let has_own = crate::object::obj_value_has_own_key(obj, key);
    // Redefining a non-configurable existing property fails.
    if has_own {
        if let Some((_writable, configurable)) = crate::object::obj_value_attrs(obj, key) {
            if !configurable {
                return nanbox_bool(false);
            }
        }
    } else if crate::object::obj_value_no_extend(obj) {
        // Defining a brand-new property on a non-extensible object fails.
        return nanbox_bool(false);
    }
    crate::object::js_object_define_property(obj, key, descriptor);
    nanbox_bool(true)
}

/// `Reflect.getPrototypeOf(obj)` — shares the actual prototype lookup with
/// `Object.getPrototypeOf` (#2757): returns the object's `[[Prototype]]`,
/// including `null` for null-prototype objects, not the object itself.
#[no_mangle]
pub extern "C" fn js_reflect_get_prototype_of(obj: f64) -> f64 {
    crate::object::js_object_get_prototype_of(obj)
}

/// `Reflect.isExtensible(target)` — throws a `TypeError` for non-object targets
/// (#2762), otherwise returns the boolean extensibility of the target. For a
/// proxy, dispatches to the `isExtensible` trap when present.
#[no_mangle]
pub extern "C" fn js_reflect_is_extensible(target: f64) -> f64 {
    if let Some(id) = lookup(target) {
        let (inner, handler, revoked) = PROXIES.with(|p| {
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
        let trap = handler_trap(handler, "isExtensible");
        if is_callable(trap) {
            let trap_result = js_closure_call1(closure_from(trap), inner);
            return coerce_trap_bool(trap_result);
        }
        return crate::object::js_object_is_extensible(inner);
    }
    if !crate::object::js_value_is_heap_object(target) {
        return reflect_non_object_typeerror("isExtensible");
    }
    crate::object::js_object_is_extensible(target)
}

/// `Reflect.preventExtensions(target)` — throws a `TypeError` for non-object
/// targets (#2762) and returns a boolean (`true` on success), unlike
/// `Object.preventExtensions` which returns the object. For a proxy, dispatches
/// to the `preventExtensions` trap when present and returns its coerced result.
#[no_mangle]
pub extern "C" fn js_reflect_prevent_extensions(target: f64) -> f64 {
    if let Some(id) = lookup(target) {
        let (inner, handler, revoked) = PROXIES.with(|p| {
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
        let trap = handler_trap(handler, "preventExtensions");
        if is_callable(trap) {
            let trap_result = js_closure_call1(closure_from(trap), inner);
            return coerce_trap_bool(trap_result);
        }
        crate::object::js_object_prevent_extensions(inner);
        return nanbox_bool(true);
    }
    if !crate::object::js_value_is_heap_object(target) {
        return reflect_non_object_typeerror("preventExtensions");
    }
    crate::object::js_object_prevent_extensions(target);
    nanbox_bool(true)
}

/// `Reflect.setPrototypeOf(target, proto)` (#2761).
///
/// Returns a boolean: `true` when the prototype change is applied, `false`
/// when it is rejected (target is non-extensible and the proto actually
/// changes). Throws `TypeError` for a non-object target or a proto that is
/// neither an object nor `null`.
///
/// Proxy `setPrototypeOf` traps are out of scope (no trap dispatch); a proxy
/// target reports `true` after recording the change on its underlying target.
#[no_mangle]
pub extern "C" fn js_reflect_set_prototype_of(target: f64, proto: f64) -> f64 {
    // Resolve a proxy to its underlying target.
    let real = if let Some(id) = lookup(target) {
        PROXIES.with(|p| {
            p.borrow()
                .get(id as usize)
                .and_then(|o| o.as_ref())
                .map(|e| e.target)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED))
        })
    } else {
        target
    };

    // Target must be an object.
    if !reflect_value_is_object(real) {
        return reflect_non_object_typeerror("setPrototypeOf");
    }

    // Proto must be an object or null.
    let proto_bits = proto.to_bits();
    let proto_ok = proto_bits == TAG_NULL || reflect_value_is_object(proto);
    if !proto_ok {
        return throw_type_error("Object prototype may only be an Object or null");
    }

    // #2761: a non-extensible target rejects a *changing* prototype. If the
    // current prototype already equals `proto`, the no-op set still succeeds.
    if crate::object::obj_value_no_extend(real) {
        let current = crate::object::js_object_get_prototype_of(real);
        if current.to_bits() != proto_bits {
            return nanbox_bool(false);
        }
        return nanbox_bool(true);
    }

    // Apply via the shared Object-side helper (records in the side-table).
    crate::object::js_object_set_prototype_of(real, proto);
    nanbox_bool(true)
}

#[no_mangle]
pub extern "C" fn js_reflect_define_metadata(
    key: f64,
    value: f64,
    target: f64,
    property_key: f64,
) -> f64 {
    if let Some(metadata_key) = make_metadata_key(key, target, property_key) {
        REFLECT_METADATA.with(|store| {
            store.borrow_mut().insert(metadata_key, value);
        });
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_reflect_get_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(key_part) = metadata_key_part(key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let Some(property_key_part) = metadata_property_key_part(property_key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    get_metadata_in_prototype_chain(&key_part, target, property_key_part.as_ref())
}

fn get_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    REFLECT_METADATA.with(|store| {
        store
            .borrow()
            .get(&metadata_key)
            .copied()
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
    })
}

#[no_mangle]
pub extern "C" fn js_reflect_get_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    get_own_metadata(key, target, property_key)
}

#[no_mangle]
pub extern "C" fn js_reflect_has_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(key_part) = metadata_key_part(key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let Some(property_key_part) = metadata_property_key_part(property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let found = get_metadata_in_prototype_chain(&key_part, target, property_key_part.as_ref())
        .to_bits()
        != TAG_UNDEFINED;
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub extern "C" fn js_reflect_has_own_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let found = REFLECT_METADATA.with(|store| store.borrow().contains_key(&metadata_key));
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub extern "C" fn js_reflect_get_metadata_keys(target: f64, property_key: f64) -> f64 {
    metadata_keys_for(target, property_key, true)
}

#[no_mangle]
pub extern "C" fn js_reflect_get_own_metadata_keys(target: f64, property_key: f64) -> f64 {
    metadata_keys_for(target, property_key, false)
}

#[no_mangle]
pub extern "C" fn js_reflect_delete_metadata(key: f64, target: f64, property_key: f64) -> f64 {
    let Some(metadata_key) = make_metadata_key(key, target, property_key) else {
        return f64::from_bits(TAG_FALSE);
    };
    let deleted = REFLECT_METADATA.with(|store| store.borrow_mut().remove(&metadata_key).is_some());
    f64::from_bits(if deleted { TAG_TRUE } else { TAG_FALSE })
}

fn make_metadata_key(key: f64, target: f64, property_key: f64) -> Option<MetadataKey> {
    Some(MetadataKey {
        target_bits: target.to_bits(),
        key: metadata_key_part(key)?,
        property_key: metadata_property_key_part(property_key)?,
    })
}

/// Resolve the `propertyKey` argument of a `Reflect.*Metadata(…)` call.
///
/// Returns:
/// - `Some(None)` when the argument is `undefined` — class-level metadata.
/// - `Some(Some(s))` for any value that coerces to a string.
/// - `None` for values we explicitly refuse to key on (e.g. Symbols). The
///   caller treats this as "skip the operation" so we never silently store
///   metadata under an unstable bit-pattern key (#754 review).
fn metadata_property_key_part(property_key: f64) -> Option<Option<String>> {
    if property_key.to_bits() == TAG_UNDEFINED {
        return Some(None);
    }
    metadata_key_part(property_key).map(Some)
}

/// Coerce a metadata key to a stable owned String, or return None if the
/// value cannot be represented as a string key. Returning None makes the
/// caller treat the op as a no-op rather than fabricating a fake key.
///
/// Symbol-keyed metadata is explicitly unsupported (see
/// docs/src/language/decorators.md) — Symbols flow through here and return
/// None rather than colliding on `toString()`'s `"Symbol()"` rendering.
fn metadata_key_part(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) {
        if ptr.is_null() {
            return None;
        }
        if len == 0 {
            return Some(String::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if crate::value::is_js_handle(value) {
        let str_ptr = crate::value::js_jsvalue_to_string(value);
        if !str_ptr.is_null() {
            let nb =
                f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & 0x0000_FFFF_FFFF_FFFF));
            if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(nb, &mut scratch) {
                if !ptr.is_null() {
                    if len == 0 {
                        return Some(String::new());
                    }
                    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
                    return Some(String::from_utf8_lossy(bytes).into_owned());
                }
            }
        }
    }
    // Numbers, booleans, null — coerce through the standard JS path so
    // e.g. `0`, `true`, etc. produce deterministic string keys.
    let coerced = crate::builtins::js_string_coerce(value);
    if !coerced.is_null() {
        let name_ptr =
            unsafe { (coerced as *const u8).add(std::mem::size_of::<crate::StringHeader>()) };
        let name_len = unsafe { (*coerced).byte_len as usize };
        if let Ok(s) =
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(name_ptr, name_len) })
        {
            return Some(s.to_string());
        }
    }
    None
}

fn get_metadata_in_prototype_chain(key: &str, target: f64, property_key: Option<&String>) -> f64 {
    let mut current = target;
    loop {
        let current_bits = current.to_bits();
        let found = REFLECT_METADATA.with(|store| {
            store
                .borrow()
                .get(&MetadataKey {
                    target_bits: current_bits,
                    key: key.to_string(),
                    property_key: property_key.cloned(),
                })
                .copied()
        });
        if let Some(value) = found {
            return value;
        }

        let next = crate::object::js_object_get_prototype_of(current);
        let next_bits = next.to_bits();
        if next_bits == TAG_NULL || next_bits == TAG_UNDEFINED || next_bits == current_bits {
            return f64::from_bits(TAG_UNDEFINED);
        }
        current = next;
    }
}

fn metadata_keys_for(target: f64, property_key: f64, include_prototypes: bool) -> f64 {
    let Some(wanted_property_key) = metadata_property_key_part(property_key) else {
        let empty = crate::array::js_array_alloc(0);
        return f64::from_bits(POINTER_TAG | ((empty as u64) & POINTER_MASK));
    };

    let keys = REFLECT_METADATA.with(|store| {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        let store = store.borrow();
        let mut current = target;

        loop {
            let current_bits = current.to_bits();
            for metadata_key in store.keys() {
                if metadata_key.target_bits == current_bits
                    && metadata_key.property_key == wanted_property_key
                    && seen.insert(metadata_key.key.clone())
                {
                    keys.push(metadata_key.key.clone());
                }
            }

            if !include_prototypes {
                break;
            }

            let next = crate::object::js_object_get_prototype_of(current);
            let next_bits = next.to_bits();
            if next_bits == TAG_NULL || next_bits == TAG_UNDEFINED || next_bits == current_bits {
                break;
            }
            current = next;
        }

        keys
    });

    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        values.push(crate::string::js_string_new_sso(
            key.as_ptr(),
            key.len() as u32,
        ));
    }

    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    f64::from_bits(POINTER_TAG | ((arr as u64) & POINTER_MASK))
}

/// Native trampoline backing the `revoke` function returned by
/// `Proxy.revocable`. The closure captures the proxy value in capture slot 0;
/// invoking it revokes that specific proxy. Idempotent — revoking an
/// already-revoked proxy is a no-op (Node's `revoke()` is idempotent). (#2846)
extern "C" fn proxy_revoke_trampoline(closure: *const crate::closure::ClosureHeader) -> f64 {
    let proxy = crate::closure::js_closure_get_capture_f64(closure, 0);
    js_proxy_revoke(proxy);
    f64::from_bits(TAG_UNDEFINED)
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
    let revoke_closure = crate::closure::js_closure_alloc(proxy_revoke_trampoline as *const u8, 1);
    crate::closure::js_register_closure_arity(proxy_revoke_trampoline as *const u8, 0);
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
static KEEP_REFLECT_HAS: extern "C" fn(f64, f64) -> f64 = js_reflect_has;
#[used]
static KEEP_REFLECT_OWN_KEYS: extern "C" fn(f64) -> f64 = js_reflect_own_keys;
#[used]
static KEEP_REFLECT_APPLY: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_apply;
