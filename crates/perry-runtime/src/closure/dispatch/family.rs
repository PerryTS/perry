//! K3 C3, the receiver-family check (owner premise 11-23): a method that
//! code outside the program's view gets hold of runs only with `this` = an
//! object of its own class family, or nullish. The compiler proves sites in
//! such a method from that premise (`perry-wpfacts` `recv_family`); the
//! runtime checks it where the runtime or the host invokes a handed
//! function: a callback a builtin calls with a `thisArg`, an event
//! listener, a Proxy trap, a builtin's `Invoke(this, key)`, an implicit
//! protocol call. A program call (`f.call(h)`, `Reflect.apply`, a bound
//! function: `explicit_this`) is the program's own binding and is never
//! checked here.
//!
//! A body whose family is live at the compiler's fixpoint carries a nonzero
//! [`JsFunctionInfo::family`]: the signed displacement from its info to its
//! home-key record (codegen `fn_info`). The record is `u16` key count, then
//! per key a tag byte (`FAMILY_KEY_STRING`: UTF-8 bytes, or
//! `FAMILY_KEY_WELL_KNOWN`: a well-known symbol's short name), a `u16`
//! length and the bytes, all little-endian and unaligned.
//!
//! `this` is accepted, exactly as the model's family is built:
//! - nullish;
//! - a Proxy whose target (unwrapped, no trap runs) is accepted;
//! - an object that is no declared class's prototype object (the model's
//!   `fam_this` excludes exactly those: CWN ruling K3BUILD13 2) and whose
//!   chain (itself and its prototypes, Proxies unwrapped) holds the
//!   function under one of its home keys (a home holder or copy target), or
//!   holds it in an object strictly above `this` under any key (a prototype
//!   it was copied onto: "classes it was copied onto").
//!
//! "Holds the function" compares the body (`JsFunctionInfo`), as the model's
//! family is per body. The check reads own properties only (data values and
//! accessor functions), runs no program code and no trap.
//! A violation is a Perry contract error.

use crate::closure::{ClosureHeader, JsFunctionInfo};

/// Home-key record tag: a string key.
pub(crate) const FAMILY_KEY_STRING: u8 = 0;
/// Home-key record tag: a well-known symbol by its short name (`iterator`).
pub(crate) const FAMILY_KEY_WELL_KNOWN: u8 = 1;

/// The home keys of `info`'s record, as (tag, bytes).
fn home_keys(info: &'static JsFunctionInfo) -> Vec<(u8, &'static [u8])> {
    let at = (info as *const JsFunctionInfo as *const u8).wrapping_offset(info.family as isize);
    // SAFETY: a nonzero `family` is codegen's displacement to the body's
    // static, immutable record (`fn_info::render_definition`).
    unsafe {
        let rd = |p: *const u8| u16::from_le_bytes([*p, *p.add(1)]);
        let n = rd(at) as usize;
        let mut p = at.add(2);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let tag = *p;
            let len = rd(p.add(1)) as usize;
            out.push((tag, std::slice::from_raw_parts(p.add(3), len)));
            p = p.add(3 + len);
        }
        out
    }
}

/// The info of `value` when it is a function object.
fn body_of(value: f64) -> Option<*const JsFunctionInfo> {
    let v = crate::JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let p = v.as_pointer::<u8>() as usize;
    if !crate::closure::is_closure_ptr(p) {
        return None;
    }
    crate::closure::closure_info(p as *const ClosureHeader).map(|i| i as *const JsFunctionInfo)
}

/// Own property `key` of `obj` holds the body `info` (a data value, or an
/// accessor's getter or setter).
fn holds(obj: f64, key: f64, info: *const JsFunctionInfo) -> bool {
    use crate::object::OwnProperty;
    // SAFETY: `obj` is an ordinary (non-Proxy) object value.
    match unsafe { crate::object::own_property(obj, key) } {
        Some(OwnProperty::Data(v, _)) => body_of(v) == Some(info),
        Some(OwnProperty::Accessor(g, s, _)) => {
            body_of(g) == Some(info) || body_of(s) == Some(info)
        }
        _ => false,
    }
}

/// `v` with every Proxy layer replaced by its target (no trap runs);
/// `undefined` for a revoked Proxy.
fn unwrap_proxies(mut v: f64) -> f64 {
    while crate::proxy_hooks::proxy_is_proxy(v) != 0 {
        v = crate::proxy::js_proxy_target(v);
    }
    v
}

fn is_object(v: f64) -> bool {
    let j = crate::JSValue::from_bits(v.to_bits());
    // SAFETY: `js_is_symbol` reads only the tag and header of a pointer value.
    j.is_pointer() && unsafe { crate::symbol::js_is_symbol(v) } == 0 && {
        let p = j.as_pointer::<u8>() as usize;
        // SAFETY: a pointer-tagged value names a live allocation or none.
        unsafe { crate::value::addr_class::try_read_gc_header(p) }.is_some_and(|h| {
            crate::gc_type_is_object!(h.obj_type) || crate::closure::is_closure_ptr(p)
        })
    }
}

fn declared_class_prototype(v: f64) -> bool {
    let p = crate::JSValue::from_bits(v.to_bits()).as_pointer::<u8>() as usize;
    crate::object::class_id_for_decl_prototype_object(p).is_some()
}

/// The receiver-family test (module docs). Allocates (key strings, the
/// own-key lists), so every value it walks is held in a handle.
pub(crate) fn family_this_ok(info: &'static JsFunctionInfo, this: f64) -> bool {
    let this = unwrap_proxies(this);
    let j = crate::JSValue::from_bits(this.to_bits());
    if j.is_null() || j.is_undefined() {
        return true;
    }
    if !is_object(this) || declared_class_prototype(this) {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let start = scope.root_nanbox_f64(this);
    let body = info as *const JsFunctionInfo;
    // The home keys, as key values (symbols are process-lifetime).
    let keys: Vec<_> = home_keys(info)
        .into_iter()
        .map(|(tag, bytes)| {
            let v = if tag == FAMILY_KEY_WELL_KNOWN {
                let name = std::str::from_utf8(bytes).unwrap_or("");
                let s = crate::symbol::well_known_symbol(name);
                f64::from_bits(crate::value::POINTER_TAG | (s as u64 & crate::value::POINTER_MASK))
            } else {
                let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                f64::from_bits(crate::value::STRING_TAG | (s as u64 & crate::value::POINTER_MASK))
            };
            scope.root_nanbox_f64(v)
        })
        .collect();
    // Pass 1: the chain holds the body under a home key.
    let cur = scope.root_nanbox_f64(start.get_nanbox_f64());
    for _ in 0..CHAIN_LIMIT {
        let o = cur.get_nanbox_f64();
        if keys
            .iter()
            .any(|k| holds(cur.get_nanbox_f64(), k.get_nanbox_f64(), body))
        {
            return true;
        }
        let Some(next) = proto_of(o) else { break };
        cur.set_nanbox_f64(next);
    }
    // Pass 2: an object strictly above `this` holds it under any key.
    cur.set_nanbox_f64(start.get_nanbox_f64());
    for _ in 0..CHAIN_LIMIT {
        let Some(next) = proto_of(cur.get_nanbox_f64()) else {
            break;
        };
        cur.set_nanbox_f64(next);
        let names = scope
            .root_nanbox_f64(crate::object::property_ops::reflect_misc::js_reflect_own_keys(next));
        let arr = crate::JSValue::from_bits(names.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let n = crate::array::js_array_length(arr);
        for i in 0..n {
            let arr = crate::JSValue::from_bits(names.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>();
            let k = crate::array::js_array_get_f64(arr, i);
            if holds(cur.get_nanbox_f64(), k, body) {
                return true;
            }
        }
    }
    false
}

/// A chain longer than this is a cycle the runtime refused to build; the
/// walk stops (rejecting) rather than spin.
const CHAIN_LIMIT: usize = 1 << 16;

/// The next object on `o`'s chain (Proxies unwrapped, no trap), if any.
fn proto_of(o: f64) -> Option<f64> {
    let p = unwrap_proxies(crate::object::js_object_get_prototype_of(o));
    is_object(p).then_some(p)
}

/// Whether `closure`'s body carries a live family (a caller resolving a
/// callback once per loop records this and checks only then).
#[inline]
pub fn has_family(closure: *const ClosureHeader) -> bool {
    crate::closure::closure_info(closure).is_some_and(|i| i.family != 0)
}

/// The check at a runtime/host invoke point: `closure` is about to run with
/// receiver `this`. Raises the Perry contract error on a violation.
#[inline]
pub fn check_family_this(closure: *const ClosureHeader, this: f64) {
    let Some(info) = crate::closure::closure_info(closure) else {
        return;
    };
    if info.family == 0 {
        return;
    }
    check_slow(info, this);
}

/// `check_family_this` on a function value (an event listener, a trap).
#[inline]
pub fn check_family_this_value(callee: f64, this: f64) {
    let v = crate::JSValue::from_bits(callee.to_bits());
    if !v.is_pointer() {
        return;
    }
    let p = v.as_pointer::<u8>() as usize;
    if crate::closure::is_closure_ptr(p) {
        check_family_this(p as *const ClosureHeader, this);
    }
}

#[cold]
#[inline(never)]
fn check_slow(info: &'static JsFunctionInfo, this: f64) {
    if !family_this_ok(info, this) {
        crate::object::property_ops::common::throw_type_error(
            "Perry contract error: a method was invoked by the runtime with a `this` outside its \
             class family (receiver-family premise)",
        );
    }
}

#[cfg(test)]
#[path = "family_tests.rs"]
mod tests;
