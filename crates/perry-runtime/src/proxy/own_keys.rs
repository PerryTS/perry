//! Proxy `[[OwnPropertyKeys]]` (ECMA-262 §10.5.11) — the `ownKeys` trap.
//!
//! `Object.keys` / `Object.getOwnPropertyNames` / `Object.getOwnPropertySymbols`
//! / `Reflect.ownKeys` on a Proxy all funnel through here. The trap result is
//! validated as a duplicate-free list of String/Symbol keys, then checked
//! against the target's non-configurable / non-extensible invariants.

use super::{
    call_trap, create_list_from_array_like, handler_trap, is_callable_function, lookup,
    revoked_return, throw_type_error, POINTER_MASK, POINTER_TAG, PROXIES, TAG_NULL, TAG_UNDEFINED,
};

fn alloc_key_array(keys: &[f64]) -> f64 {
    let mut arr = crate::array::js_array_alloc(keys.len() as u32);
    for &k in keys {
        arr = crate::array::js_array_push_f64(arr, k);
    }
    f64::from_bits(POINTER_TAG | ((arr as u64) & POINTER_MASK))
}

fn is_string_key(v: f64) -> bool {
    crate::value::JSValue::from_bits(v.to_bits()).is_any_string()
}

fn is_symbol_key(v: f64) -> bool {
    unsafe { crate::symbol::js_is_symbol(v) != 0 }
}

/// A stable identity for duplicate detection: string content for string keys,
/// raw bits for symbol keys.
fn key_identity(v: f64) -> Option<String> {
    if is_symbol_key(v) {
        return Some(format!("sym:{:016x}", v.to_bits()));
    }
    let s = crate::builtins::js_string_coerce(v);
    if s.is_null() {
        return None;
    }
    unsafe {
        let name_ptr = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*s).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|x| format!("str:{x}"))
    }
}

/// Collect a (possibly proxy) value's own keys (string names then symbols),
/// recursing through proxy targets so the no-trap path forwards correctly.
fn target_own_keys(target: f64) -> Vec<f64> {
    if lookup(target).is_some() {
        let arr = js_proxy_own_keys(target);
        return read_array(arr);
    }
    let mut out = Vec::new();
    let names = crate::object::js_object_get_own_property_names(target);
    out.extend(read_array(names));
    let syms = unsafe { crate::symbol::js_object_get_own_property_symbols(target) };
    let syms_ptr = syms as *const crate::array::ArrayHeader;
    if !syms_ptr.is_null() {
        let n = crate::array::js_array_length(syms_ptr) as usize;
        for i in 0..n {
            let s = crate::array::js_array_get(syms_ptr, i as u32);
            out.push(f64::from_bits(s.bits()));
        }
    }
    out
}

fn read_array(arr_value: f64) -> Vec<f64> {
    let ptr = (arr_value.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    if ptr.is_null() {
        return Vec::new();
    }
    let n = crate::array::js_array_length(ptr) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(f64::from_bits(
            crate::array::js_array_get(ptr, i as u32).bits(),
        ));
    }
    out
}

fn target_key_is_configurable(target: f64, key: f64) -> bool {
    let desc = crate::object::js_object_get_own_property_descriptor(target, key);
    if desc.to_bits() == TAG_UNDEFINED {
        // Not an own property of the target; treat as configurable (no
        // invariant to enforce for it).
        return true;
    }
    let ptr = (desc.to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    if ptr.is_null() {
        return true;
    }
    let k = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
    crate::value::js_is_truthy(crate::object::js_object_get_field_by_name_f64(ptr, k)) != 0
}

/// Proxy `[[OwnPropertyKeys]]`: returns a fresh Array JSValue holding the own
/// keys (strings + symbols). Dispatches the `ownKeys` trap with the handler
/// bound as `this`, applies the element-type / duplicate / invariant checks, or
/// forwards to the target when no trap is present.
#[no_mangle]
pub extern "C" fn js_proxy_own_keys(proxy_boxed: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return alloc_key_array(&[]),
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
    let trap = handler_trap(handler, "ownKeys");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return alloc_key_array(&target_own_keys(target));
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy ownKeys trap is not a function");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(target);
    let trap_result = call_trap(handler, trap, &[target_h.get_nanbox_f64()]);

    // CreateListFromArrayLike(trapResult, « String, Symbol »): throws for a
    // non-object result and for any element that is neither a string nor a
    // symbol.
    let trap_keys = create_list_from_array_like(trap_result);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &k in &trap_keys {
        if !is_string_key(k) && !is_symbol_key(k) {
            return throw_type_error("proxy ownKeys trap returned a non-string, non-symbol key");
        }
        let Some(idy) = key_identity(k) else {
            return throw_type_error("proxy ownKeys trap returned an invalid key");
        };
        if !seen.insert(idy) {
            return throw_type_error("proxy ownKeys trap returned duplicate keys");
        }
    }

    let target = target_h.get_nanbox_f64();
    let extensible = !crate::object::obj_value_no_extend(target);
    let target_keys = target_own_keys(target);

    // Partition the target's own keys by configurability.
    let mut nonconfig: Vec<f64> = Vec::new();
    let mut config: Vec<f64> = Vec::new();
    for &tk in &target_keys {
        if target_key_is_configurable(target, tk) {
            config.push(tk);
        } else {
            nonconfig.push(tk);
        }
    }

    // Fast path: an extensible target with only configurable keys imposes no
    // invariant — return the trap result verbatim.
    if extensible && nonconfig.is_empty() {
        return alloc_key_array(&trap_keys);
    }

    let mut unchecked: Vec<String> = trap_keys.iter().filter_map(|&k| key_identity(k)).collect();
    let remove = |unchecked: &mut Vec<String>, key: f64| -> bool {
        if let Some(idy) = key_identity(key) {
            if let Some(pos) = unchecked.iter().position(|x| *x == idy) {
                unchecked.remove(pos);
                return true;
            }
        }
        false
    };

    // Every non-configurable target key must appear in the trap result.
    for &k in &nonconfig {
        if !remove(&mut unchecked, k) {
            return throw_type_error(
                "proxy ownKeys trap omitted a non-configurable target property",
            );
        }
    }
    if extensible {
        return alloc_key_array(&trap_keys);
    }
    // A non-extensible target additionally requires every configurable key to
    // be present and forbids extra keys.
    for &k in &config {
        if !remove(&mut unchecked, k) {
            return throw_type_error(
                "proxy ownKeys trap omitted a property of a non-extensible target",
            );
        }
    }
    if !unchecked.is_empty() {
        return throw_type_error(
            "proxy ownKeys trap returned an extra key for a non-extensible target",
        );
    }
    alloc_key_array(&trap_keys)
}

/// `Object.getOwnPropertyNames(proxy)` — the string subset of the proxy's own
/// keys.
pub(crate) fn proxy_own_property_names(proxy_boxed: f64) -> f64 {
    let keys = read_array(js_proxy_own_keys(proxy_boxed));
    let names: Vec<f64> = keys.into_iter().filter(|&k| is_string_key(k)).collect();
    alloc_key_array(&names)
}

/// `Object.getOwnPropertySymbols(proxy)` — the symbol subset.
pub(crate) fn proxy_own_property_symbols(proxy_boxed: f64) -> f64 {
    let keys = read_array(js_proxy_own_keys(proxy_boxed));
    let syms: Vec<f64> = keys.into_iter().filter(|&k| is_symbol_key(k)).collect();
    alloc_key_array(&syms)
}

/// `Object.keys(proxy)` — EnumerableOwnPropertyNames: string keys whose proxy
/// `[[GetOwnProperty]]` reports `enumerable: true`.
pub(crate) fn proxy_enum_own_keys(proxy_boxed: f64) -> f64 {
    let keys = read_array(js_proxy_own_keys(proxy_boxed));
    let mut out: Vec<f64> = Vec::new();
    for k in keys {
        if !is_string_key(k) {
            continue;
        }
        let desc = super::js_reflect_get_own_property_descriptor(proxy_boxed, k);
        if desc.to_bits() == TAG_UNDEFINED {
            continue;
        }
        let ptr = (desc.to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
        if ptr.is_null() {
            continue;
        }
        let ek = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        if crate::value::js_is_truthy(crate::object::js_object_get_field_by_name_f64(ptr, ek)) != 0
        {
            out.push(k);
        }
    }
    let _ = TAG_NULL;
    alloc_key_array(&out)
}
