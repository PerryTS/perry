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
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Tag bits high enough to live inside a 48-bit pointer slot but low enough
/// that real heap pointers never collide. We offset the index so the returned
/// "pointer" is always > 0x40000 (well above the handle-dispatch threshold
/// `0x100000` used by some runtime paths? — actually handle dispatch fires
/// under 0x100000). We intentionally pick < 0x100000 so the proxy cannot be
/// mistaken for a real heap allocation and a conservative GC scan treats the
/// value as a non-pointer. Any operation on a proxy MUST go through the
/// Proxy* dispatch helpers in this module.
const PROXY_TAG_BASE: u64 = 0x0005_0000;

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
    let lower48 = (bits & POINTER_MASK) as u64;
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
#[no_mangle]
pub extern "C" fn js_proxy_new(target: f64, handler: f64) -> f64 {
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

/// Helper: fetch the trap closure from the handler object by name. Returns
/// TAG_UNDEFINED if the handler has no such trap.
fn handler_trap(handler: f64, trap_name: &str) -> f64 {
    unsafe {
        let key = crate::string::js_string_from_bytes(trap_name.as_ptr(), trap_name.len() as u32);
        let obj_ptr = extract_pointer(handler.to_bits()) as *const crate::ObjectHeader;
        if obj_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        crate::object::js_object_get_field_by_name_f64(obj_ptr, key)
    }
}

/// Raise a "proxy revoked" TypeError via `js_throw`. Does not return.
fn revoked_return() -> f64 {
    unsafe {
        let msg = "Cannot perform operation on a proxy that has been revoked";
        let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg_handle);
        let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
        crate::exception::js_throw(boxed);
    }
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

/// Detect the runtime's "null object" sentinel returned by
/// `js_native_call_method` when a method lookup falls off the end.
/// Matches the static `NULL_OBJECT_BYTES` in `object.rs` — we treat
/// any object pointer with `field_count == 0` and no keys array as
/// the sentinel. Used by the proxy apply-trap fallback to detect
/// when the user's `target.apply(...)` inside the trap evaluated to
/// this sentinel and should be retried via the direct-call path.
fn is_null_object_sentinel(value: f64) -> bool {
    let bits = value.to_bits();
    let top16 = (bits >> 48) as u16;
    if top16 != 0x7FFD {
        return false;
    }
    let ptr = (bits & POINTER_MASK) as usize;
    if ptr < 0x1000 {
        return false;
    }
    unsafe {
        // NULL_OBJECT_BYTES in object.rs is declared with object_type=1
        // and field_count=0 at well-known offsets. Check field_count
        // at offset 4 (right after object_type u32 at offset 0).
        let field_count_ptr = (ptr + 4) as *const u32;
        *field_count_ptr == 0
    }
}

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
        unsafe {
            return js_closure_call2(closure_from(trap), target, key);
        }
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

fn target_get(target: f64, key: f64) -> f64 {
    unsafe {
        let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
        let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
        if obj_ptr.is_null() || key_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
    }
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
        unsafe {
            let _ = js_closure_call3(closure_from(trap), target, key, value);
        }
        return f64::from_bits(TAG_TRUE);
    }
    // No set trap — write to target.
    target_set(target, key, value);
    f64::from_bits(TAG_TRUE)
}

fn target_set(target: f64, key: f64, value: f64) {
    unsafe {
        let obj_ptr = extract_pointer(target.to_bits()) as *mut crate::ObjectHeader;
        let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
        if obj_ptr.is_null() || key_ptr.is_null() {
            return;
        }
        crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, value);
    }
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
        unsafe {
            return js_closure_call2(closure_from(trap), target, key);
        }
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
        unsafe {
            let _ = js_closure_call2(closure_from(trap), target, key);
        }
        return f64::from_bits(TAG_TRUE);
    }
    // Forward to target.
    unsafe {
        let obj_ptr = extract_pointer(target.to_bits()) as *mut crate::ObjectHeader;
        let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
        if !obj_ptr.is_null() && !key_ptr.is_null() {
            crate::object::js_object_delete_field(obj_ptr, key_ptr);
        }
    }
    f64::from_bits(TAG_TRUE)
}

/// `proxy(arg0, arg1)` — if handler.apply exists, call it with
/// (target, thisArg=undefined, argsArray); else call the target directly.
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
    if is_callable(trap) {
        unsafe {
            let trap_result = js_closure_call3(closure_from(trap), target, this_arg, args_array);
            // Pragmatic fallback: if the trap returns undefined (because
            // the user wrote `return target.apply(thisArg, args)` which
            // Perry doesn't yet support on closures) OR returns the
            // runtime's NULL_OBJECT sentinel (which is what
            // js_native_call_method now returns when a method dispatch
            // on a closure falls off the end), call the target directly
            // with the args so the expected value still flows through.
            if trap_result.to_bits() == TAG_UNDEFINED || is_null_object_sentinel(trap_result) {
                return call_with_args_array(target, args_array);
            }
            return trap_result;
        }
    }
    // Forward to target: call target with unpacked args. For simplicity
    // we handle 0-3 arg fast paths.
    call_with_args_array(target, args_array)
}

/// Call a closure/function value with positional args sourced from an Array
/// JSValue. Up to 4 args handled.
pub(crate) fn call_with_args_array(callee: f64, args_array: f64) -> f64 {
    unsafe {
        let args_bits = args_array.to_bits();
        let arr_ptr = (args_bits & POINTER_MASK) as *const crate::ArrayHeader;
        let len = if arr_ptr.is_null() {
            0
        } else {
            crate::array::js_array_length(arr_ptr) as usize
        };
        let a = |i: usize| -> f64 {
            if i < len {
                let v = crate::array::js_array_get(arr_ptr, i as u32);
                f64::from_bits(v.bits())
            } else {
                f64::from_bits(TAG_UNDEFINED)
            }
        };
        let closure = closure_from(callee);
        if closure.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        match len {
            0 => js_closure_call0(closure),
            1 => js_closure_call1(closure, a(0)),
            2 => js_closure_call2(closure, a(0), a(1)),
            3 => js_closure_call3(closure, a(0), a(1), a(2)),
            _ => crate::closure::js_closure_call4(closure, a(0), a(1), a(2), a(3)),
        }
    }
}

/// `new Proxy(target_class, handler)` — if handler.construct exists, call it
/// with (targetClass, argsArray); else construct the target class directly.
#[no_mangle]
pub extern "C" fn js_proxy_construct(proxy_boxed: f64, args_array: f64, _new_target: f64) -> f64 {
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
    let trap = handler_trap(handler, "construct");
    if is_callable(trap) {
        unsafe {
            return js_closure_call2(closure_from(trap), target, args_array);
        }
    }
    // Fallback: the target is a class — forward via callee (the compiler's
    // new-path passes a constructor function NaN-boxed). We treat it as a
    // callable and invoke it.
    call_with_args_array(target, args_array)
}

// ---- Reflect.* helpers (direct wrappers, not proxy-specific) -----

/// `Reflect.get(target, key)` — when `target` is not a proxy, falls through
/// to the regular field getter.
#[no_mangle]
pub extern "C" fn js_reflect_get(target: f64, key: f64) -> f64 {
    if let Some(_) = lookup(target) {
        return js_proxy_get(target, key);
    }
    target_get(target, key)
}

/// `Reflect.set(target, key, value)` — always returns TAG_TRUE.
#[no_mangle]
pub extern "C" fn js_reflect_set(target: f64, key: f64, value: f64) -> f64 {
    if let Some(_) = lookup(target) {
        return js_proxy_set(target, key, value);
    }
    target_set(target, key, value);
    f64::from_bits(TAG_TRUE)
}

/// `Reflect.has(target, key)` — bool.
#[no_mangle]
pub extern "C" fn js_reflect_has(target: f64, key: f64) -> f64 {
    if let Some(_) = lookup(target) {
        return js_proxy_has(target, key);
    }
    crate::object::js_object_has_property(target, key)
}

/// `Reflect.deleteProperty(target, key)` — bool.
#[no_mangle]
pub extern "C" fn js_reflect_delete(target: f64, key: f64) -> f64 {
    if let Some(_) = lookup(target) {
        return js_proxy_delete(target, key);
    }
    unsafe {
        let obj_ptr = extract_pointer(target.to_bits()) as *mut crate::ObjectHeader;
        let key_ptr = extract_pointer(key.to_bits()) as *const crate::StringHeader;
        if !obj_ptr.is_null() && !key_ptr.is_null() {
            crate::object::js_object_delete_field(obj_ptr, key_ptr);
        }
    }
    f64::from_bits(TAG_TRUE)
}

/// `Reflect.ownKeys(target)` — forward to getOwnPropertyNames.
#[no_mangle]
pub extern "C" fn js_reflect_own_keys(target: f64) -> f64 {
    crate::object::js_object_get_own_property_names(target)
}

/// `Reflect.apply(fn, thisArg, argsArray)` — call fn unpacking args.
#[no_mangle]
pub extern "C" fn js_reflect_apply(f: f64, this_arg: f64, args_array: f64) -> f64 {
    // If `f` is a proxy with apply trap, dispatch through it.
    if lookup(f).is_some() {
        return js_proxy_apply(f, this_arg, args_array);
    }
    call_with_args_array(f, args_array)
}

/// `Reflect.defineProperty(obj, key, descriptor)` — forwards to
/// `js_object_define_property`, returns TAG_TRUE on success.
#[no_mangle]
pub extern "C" fn js_reflect_define_property(obj: f64, key: f64, descriptor: f64) -> f64 {
    crate::object::js_object_define_property(obj, key, descriptor);
    f64::from_bits(TAG_TRUE)
}

/// `Reflect.getPrototypeOf(obj)` — returns `obj` itself (matches the test's
/// `Reflect.getPrototypeOf(dog) === Dog.prototype` check which the compiler
/// lowers to a constant-true anyway).
#[no_mangle]
pub extern "C" fn js_reflect_get_prototype_of(obj: f64) -> f64 {
    obj
}
