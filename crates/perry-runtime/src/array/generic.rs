//! Generic `Array.prototype` methods over *array-like* receivers (#4597).
//!
//! ECMA-262 §23.1.3 specifies each `Array.prototype` method as operating on
//! `O = ToObject(this)` with `len = LengthOfArrayLike(O)` and indexed
//! `Get(O, k)` / `HasProperty(O, k)` — i.e. the algorithms are *generic* over
//! any object that exposes a `length` and indexed properties (plain objects,
//! `arguments`, functions, strings, typed arrays …), not just genuine
//! `Array` exotic objects.
//!
//! Perry's primary (hot) array methods in the sibling modules are specialised
//! to a real `ArrayHeader` receiver for speed. Those paths are untouched. The
//! functions here are reached *only* from the explicit
//! `Array.prototype.<m>.call(receiver, …)` / `.apply(…)` (and bound-local)
//! forms — lowered to `Expr::ArrayLikeMethod` in the HIR — where `receiver`
//! may be any value. They operate on the **original** receiver value so that:
//!   * the callback observes the original object as its 3rd argument
//!     (`(value, index, O)`), per spec, rather than a materialised clone, and
//!   * element reads are live `Get(O, k)` (data props, getters, function
//!     expandos), and holes are honoured via `HasProperty(O, k)`.
//!
//! Receiver coercion mirrors `ToObject`:
//!   * `undefined` / `null` → `TypeError`,
//!   * a real array → fast direct element access,
//!   * a string → length is the code-unit count, indices are 1-char strings,
//!   * any other heap object/closure → `Get`/`HasProperty` via the polymorphic
//!     object helpers,
//!   * a bare number/boolean → boxed into its `Number`/`Boolean` wrapper object
//!     (so inherited prototype `length`/indices are read and the callback's
//!     3rd argument is `instanceof Number`/`Boolean`),
//!   * a symbol/bigint → no indexed properties → an empty array-like (length 0).

use super::*;
use crate::closure::{js_closure_call3, ClosureHeader};
use crate::value::{JSValue, TAG_HOLE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};
use std::ptr;

#[inline(always)]
fn undef() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline(always)]
fn boxed_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { crate::value::TAG_FALSE })
}

#[inline(always)]
pub(super) fn nanbox_arr(arr: *mut ArrayHeader) -> f64 {
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

#[inline(always)]
fn top16(bits: u64) -> u64 {
    bits >> 48
}

/// `ToObject(recv)` (ECMA-262 §7.1.18). `undefined` / `null` throw a
/// `TypeError`; a bare number / boolean primitive is boxed into its wrapper
/// object so the generic algorithms below (and the callback's 3rd argument)
/// observe an object with the right prototype chain — e.g.
/// `Array.prototype.map.call(false, fn)` must read length/indices inherited
/// from `Boolean.prototype` and pass a `Boolean` wrapper to `fn`. Strings keep
/// their dedicated code-unit path; symbols / bigints (no indexed properties)
/// are returned as-is and read as an empty array-like.
pub(super) fn to_object(recv: f64) -> f64 {
    let b = recv.to_bits();
    if b == TAG_UNDEFINED || b == TAG_NULL {
        let msg = b"Cannot convert undefined or null to object";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    // A symbol is `POINTER_TAG`-shaped (heap-allocated), so it must be boxed
    // BEFORE the generic pointer early-return below — otherwise `ToObject` would
    // pass it through unchanged and a method returning `ToObject(this)` (e.g.
    // `[].sort.call(Symbol())`) yields the raw symbol, not an `instanceof
    // Symbol` wrapper (test262 sort/call-with-primitive).
    if unsafe { crate::symbol::js_is_symbol(recv) } != 0 {
        return crate::builtins::js_boxed_symbol_new(recv);
    }
    // Already a heap object / array / closure.
    if top16(b) == 0x7FFD {
        return recv;
    }
    // A primitive string boxes to a `String` wrapper object (ECMA-262
    // `ToObject`). The wrapper carries own indexed (`0`,`1`,…) and `length`
    // data properties (installed by `js_boxed_string_new`), so the array-like
    // length/index reads below resolve through the normal object path — and the
    // wrapper (not the primitive) is what flows to the callback as the `this`
    // object, so `obj instanceof String` is true. (test262
    // Array.prototype.{every,some,reduce,reduceRight,...}/15.4.4.*-1-7.)
    if is_string_value(b) {
        return crate::builtins::js_boxed_string_new(recv, 1);
    }
    if b == TAG_TRUE || b == crate::value::TAG_FALSE {
        return crate::builtins::js_boxed_boolean_new(recv);
    }
    // Numbers: real f64 values (`is_number`) and INT32-tagged small integers
    // (0x7FFE), which `is_number` excludes because the tag sits in the
    // string/special band.
    if top16(b) == 0x7FFE || JSValue::from_bits(b).is_number() {
        return crate::builtins::js_boxed_number_new(recv);
    }
    // BigInt boxes into a `BigInt` wrapper object (`ToObject`): no indexed
    // properties (still an empty array-like for the read/iterate methods), but a
    // method that *returns* `ToObject(this)` — `[].sort.call(0n)` — must yield an
    // object that is `instanceof BigInt` (test262 sort/call-with-primitive).
    // (Symbols are handled above, before the pointer early-return.)
    if JSValue::from_bits(b).is_bigint() {
        return crate::builtins::js_boxed_bigint_new(recv);
    }
    recv
}

/// Validate `cb` is callable, returning its `ClosureHeader*` (throws a
/// `TypeError` otherwise, reusing the array-method renderer for parity with the
/// specialised paths).
#[inline]
fn callable(cb: f64) -> *const ClosureHeader {
    crate::array::js_validate_array_map_callback(0, cb) as *const ClosureHeader
}

/// `ToIntegerOrInfinity` clamped to a non-negative `i64` length
/// (`LengthOfArrayLike`'s `ToLength`).
#[inline]
fn to_length(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    let n = v.trunc();
    if n <= 0.0 {
        0
    } else if n > 9_007_199_254_740_991.0 {
        9_007_199_254_740_991
    } else {
        n as i64
    }
}

/// Genuine `ArrayHeader*` if `recv` is a real (or lazy) array, else null.
///
/// Objects and arrays share `POINTER_TAG`, so the GC-header `obj_type` byte —
/// not `clean_arr_ptr` alone — must gate the array fast path: `clean_arr_ptr`
/// accepts an object pointer whose leading `ObjectHeader` words happen to pass
/// its `length <= capacity` bound, then `(*arr).length` / the element buffer
/// read `field_count` / inline slots as garbage (see `normalize_array_receiver`).
#[inline]
pub(super) fn as_real_array(recv: f64) -> *mut ArrayHeader {
    let b = recv.to_bits();
    if top16(b) != 0x7FFD {
        return ptr::null_mut();
    }
    let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return ptr::null_mut();
    }
    // Pointer-shaped non-heap handles (Proxy ids live at 0xF0000+, stream ids
    // and friends in nearby bands) would be dereferenced as a GcHeader below —
    // `Array.prototype.indexOf.call(proxy, …)` SIGSEGV'd. The Linux heap
    // range check alone admits the id bands (they start at 0x1000), so gate
    // on the handle-band classifier too.
    if !crate::value::addr_class::is_above_handle_band(raw)
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return ptr::null_mut();
    }
    let obj_type = unsafe {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        return clean_arr_ptr_mut(raw as *mut ArrayHeader);
    }
    ptr::null_mut()
}

#[inline]
pub(super) fn is_string_value(bits: u64) -> bool {
    let t = top16(bits);
    // Heap string (0x7FFF) or small-string-optimised inline string (0x7FF9).
    t == 0x7FFF || t == 0x7FF9
}

/// `LengthOfArrayLike(ToObject(recv))`.
/// Classification of a (non-array, non-string) `POINTER_TAG` receiver, used to
/// pick a *safe* property-access path. Exotic GC cells (Date, Map, Set, BigInt,
/// Error, …) must NOT be dereferenced as an `ObjectHeader` (that SIGBUSes) nor
/// passed to `js_object_get_index_polymorphic` (whose final fallback reads them
/// as an `ArrayHeader`). Typed arrays / buffers carry NO GC header, so they are
/// detected via their registries *before* the GC-header byte is read.
enum PtrKind {
    /// Plain object or function/closure — `length`/index reads via the object
    /// helpers (prototype-chain aware) are safe.
    Object,
    /// Typed array or buffer — `js_value_length_f64` /
    /// `js_object_get_index_polymorphic` handle these by registry.
    IndexedNative,
    /// Date / RegExp / Error / … exotic cell that carries expando own
    /// properties in the `exotic_expando` side table (`d = new Date();
    /// d.length = 2; d[0] = 11`). Array-like reads resolve through that
    /// table (test262 Array.prototype.*-1-11/-1-14 "applied to Date").
    ExpandoExotic(crate::object::exotic_expando::ExoticKind),
    /// A Proxy id — every array-like op routes through its traps.
    Proxy,
    /// Map / Set / Symbol / BigInt / … — no safe array-like access;
    /// treated as an empty array-like.
    Exotic,
}

fn proxy_string_key(k: i64) -> f64 {
    let s = k.to_string();
    let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(key).bits())
}

fn proxy_named_key(name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(JSValue::string_ptr(key).bits())
}

fn classify_pointer(recv: f64) -> Option<PtrKind> {
    let b = recv.to_bits();
    if top16(b) != 0x7FFD {
        return None;
    }
    // Proxies are small registered ids (pointer-shaped, below the heap floor)
    // — classify BEFORE any address-based probe. All array-like ops route
    // through the proxy traps (so a revoked proxy's Get(length) throws —
    // test262 {map,filter,splice,concat}/create-revoked-proxy).
    if crate::proxy::js_proxy_is_proxy(recv) != 0 {
        return Some(PtrKind::Proxy);
    }
    let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
    // Typed arrays / buffers are `std::alloc`-backed (no GC header) — probe
    // their registries before any GC-header read.
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
    {
        return Some(PtrKind::IndexedNative);
    }
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return Some(PtrKind::Exotic);
    }
    // Non-heap registry ids (stream/fetch handles) must not be dereferenced
    // as a GcHeader. (The Linux heap-range check alone admits the id bands.)
    if !crate::value::addr_class::is_above_handle_band(raw)
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return Some(PtrKind::Exotic);
    }
    let obj_type = unsafe {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
        return Some(PtrKind::Object);
    }
    if let Some(kind) = crate::object::exotic_expando::exotic_expando_kind(raw) {
        return Some(PtrKind::ExpandoExotic(kind));
    }
    Some(PtrKind::Exotic)
}

/// `LengthOfArrayLike(ToObject(recv))`.
pub(super) fn al_length(recv: f64) -> i64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { (*arr).length as i64 };
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        let sh = crate::value::js_jsvalue_to_string(recv);
        if sh.is_null() {
            return 0;
        }
        return crate::string::js_string_length(sh) as i64;
    }
    match classify_pointer(recv) {
        Some(PtrKind::Object) => {
            // Plain object / function: `Get(O, "length")`. An OWN accessor —
            // even a setter-only one — shadows anything inherited (test262
            // some/15.4.4.17-2-12): fire its getter or read undefined, and
            // never fall through to the prototype probes.
            let raw_addr = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
            let mut len_val;
            if let Some(acc) = crate::object::get_accessor_descriptor(raw_addr, "length") {
                len_val = if acc.get != 0 {
                    f64::from_bits(
                        unsafe { crate::object::invoke_accessor_getter(acc.get, recv) }.bits(),
                    )
                } else {
                    undef()
                };
            } else {
                let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                len_val = crate::object::js_object_get_field_by_name_f64(
                    raw_addr as *const crate::object::ObjectHeader,
                    key,
                );
                let key_v = f64::from_bits(JSValue::string_ptr(key).bits());
                let own_present =
                    crate::object::js_object_has_own(recv, key_v).to_bits() == TAG_TRUE;
                // `Get(O, "length")` walks the prototype chain — an inherited
                // `Object.prototype.length = 2` (test262 sort/S15.4.4.11_A6_T2,
                // splice/S15.4.4.12_A4_T1) resolves only when there is no own
                // property at all.
                if len_val.to_bits() == TAG_UNDEFINED && !own_present {
                    len_val = object_get_named_property_chain(raw_addr, "length");
                    // The recorded/default proto tables may resolve a DIFFERENT
                    // cell than the user-visible `Object.prototype` (read off
                    // the `Object` constructor) — probe it as a last resort.
                    if len_val.to_bits() == TAG_UNDEFINED {
                        len_val = canonical_object_prototype_named_get("length");
                    }
                }
            }
            // `LengthOfArrayLike` is `ToLength(ToNumber(Get(O, "length")))`.
            // A non-numeric `length` (e.g. `length: true` → 1, `length: "2"` →
            // 2) must be ToNumber-coerced first — the raw NaN-boxed bool/string
            // bits would otherwise read as NaN → 0.
            to_length(crate::builtins::js_number_coerce(len_val))
        }
        // Typed arrays / buffers expose a real length via the safe dispatcher.
        Some(PtrKind::IndexedNative) => to_length(crate::value::js_value_length_f64(recv)),
        // Proxy: Get("length") through the `get` trap (throws on revoked).
        Some(PtrKind::Proxy) => to_length(crate::builtins::js_number_coerce(
            crate::proxy::js_proxy_get(recv, proxy_named_key("length")),
        )),
        // Date/RegExp/Error expando receiver: `length` lives in the exotic
        // side table.
        Some(PtrKind::ExpandoExotic(kind)) => {
            let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
            match crate::object::exotic_expando::value_lookup(kind, raw, "length") {
                Some(bits) => to_length(crate::builtins::js_number_coerce(f64::from_bits(bits))),
                None => 0,
            }
        }
        // Exotic cells / bare primitives → empty array-like.
        Some(PtrKind::Exotic) | None => 0,
    }
}

/// Read a named property off the user-visible `Object.prototype` (resolved
/// through the `Object` constructor, where user writes like
/// `Object.prototype.length = 2` actually land).
fn canonical_object_prototype_named_get(name: &str) -> f64 {
    let ctor = crate::object::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_v = JSValue::from_bits(ctor.to_bits());
    if !ctor_v.is_pointer() {
        return undef();
    }
    let proto =
        crate::closure::closure_get_dynamic_prop(ctor_v.as_pointer::<u8>() as usize, "prototype");
    let proto_v = JSValue::from_bits(proto.to_bits());
    if !proto_v.is_pointer() {
        return undef();
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(
        proto_v.as_pointer::<crate::object::ObjectHeader>(),
        key,
    )
}

/// `Get(O, name)` over the recorded/default prototype chain for an ordinary
/// heap object, for a *named* (non-index) key. Companion to
/// `object_get_property_chain`; used when the direct own read misses.
fn object_get_named_property_chain(obj_ptr: usize, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_val = f64::from_bits(JSValue::string_ptr(key).bits());
    let mut cur = obj_ptr;
    for _ in 0..1000 {
        if cur == 0 {
            return undef();
        }
        let cur_val = f64::from_bits(crate::value::js_nanbox_pointer(cur as i64).to_bits());
        if crate::object::js_object_has_own(cur_val, key_val).to_bits() == TAG_TRUE {
            return crate::object::js_object_get_field_by_name_f64(
                cur as *const crate::object::ObjectHeader,
                key,
            );
        }
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return undef(),
            },
        };
        if proto_bits == TAG_NULL {
            return undef();
        }
        let t16 = proto_bits >> 48;
        let next = if t16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if t16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return undef();
        };
        if next == cur {
            return undef();
        }
        cur = next;
    }
    undef()
}

/// `Get(ToObject(recv), k)` (returns `undefined` for absent/out-of-range).
pub(super) fn al_get(recv: f64, k: i64) -> f64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        if k < 0 {
            return undef();
        }
        return js_array_get_f64(arr, k as u32);
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        return crate::object::js_object_get_index_polymorphic(b as i64, k as f64);
    }
    match classify_pointer(recv) {
        // `js_object_get_index_polymorphic` is safe for objects/closures and
        // for typed arrays / buffers (handled at its top).
        Some(PtrKind::IndexedNative) => {
            crate::object::js_object_get_index_polymorphic(b as i64, k as f64)
        }
        Some(PtrKind::Object) => {
            let v = crate::object::js_object_get_index_polymorphic(b as i64, k as f64);
            // `js_object_get_index_polymorphic` walks own + explicit-`setPrototypeOf`
            // chains, but not the *default* `Object.prototype` for a plain `{}`
            // object. The generic Array algorithms `Get(O, k)` per spec, so an
            // index living on `Object.prototype[k]` must resolve. Fall back to a
            // chain read only when the direct read missed.
            if v.to_bits() == TAG_UNDEFINED {
                // An OWN property — even a setter-only accessor — shadows
                // anything inherited (test262 some/15.4.4.17-7-c-i-19):
                // `Get` resolves to undefined, never the prototype value.
                let raw_addr = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
                let key_s = k.to_string();
                if crate::object::get_accessor_descriptor(raw_addr, &key_s).is_some() {
                    return undef();
                }
                let key = crate::string::js_string_from_bytes(key_s.as_ptr(), key_s.len() as u32);
                let key_v = f64::from_bits(JSValue::string_ptr(key).bits());
                if crate::object::js_object_has_own(recv, key_v).to_bits() == TAG_TRUE {
                    return undef();
                }
                let chained = object_get_property_chain(raw_addr, k);
                if chained.to_bits() == TAG_UNDEFINED && k >= 0 && k <= u32::MAX as i64 {
                    // Canonical Object.prototype probe (data or accessor) —
                    // the recorded/default proto tables may miss it.
                    if crate::array::object_prototype_has_index_prop(k as u32) {
                        return super::sort::object_prototype_index_get(k as u32);
                    }
                }
                chained
            } else {
                v
            }
        }
        // Date/RegExp/Error expando receiver: indexed expandos live in the
        // exotic side table.
        Some(PtrKind::ExpandoExotic(kind)) => {
            let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
            match crate::object::exotic_expando::value_lookup(kind, raw, &k.to_string()) {
                Some(bits) => f64::from_bits(bits),
                None => undef(),
            }
        }
        Some(PtrKind::Proxy) => crate::proxy::js_proxy_get(recv, proxy_string_key(k)),
        Some(PtrKind::Exotic) | None => undef(),
    }
}

/// `Get(O, ToString(k))` over the prototype chain, reading the first own
/// indexed data property found. Companion to `object_has_property_chain`; used
/// only as a fallback when the polymorphic index read misses (so the default
/// `Object.prototype` is consulted for an inherited indexed element).
fn object_get_property_chain(obj_ptr: usize, k: i64) -> f64 {
    let s = k.to_string();
    let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let mut cur = obj_ptr;
    for _ in 0..1000 {
        if cur == 0 {
            return undef();
        }
        let cur_val = f64::from_bits(crate::value::js_nanbox_pointer(cur as i64).to_bits());
        let key_val = f64::from_bits(JSValue::string_ptr(key).bits());
        if crate::object::js_object_has_own(cur_val, key_val).to_bits() == TAG_TRUE {
            return crate::object::js_object_get_index_polymorphic(cur as i64, k as f64);
        }
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return undef(),
            },
        };
        if proto_bits == TAG_NULL {
            return undef();
        }
        let top16 = proto_bits >> 48;
        let next = if top16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return undef();
        };
        if next == cur {
            return undef();
        }
        cur = next;
    }
    undef()
}

/// `HasProperty(ToObject(recv), k)`.
fn al_has(recv: f64, k: i64) -> bool {
    if k < 0 {
        return false;
    }
    let arr = as_real_array(recv);
    if !arr.is_null() {
        unsafe {
            if k >= (*arr).length as i64 {
                return false;
            }
            let el = *((arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64)
                .add(k as usize);
            return el.to_bits() != TAG_HOLE;
        }
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        return k < al_length(recv);
    }
    match classify_pointer(recv) {
        Some(PtrKind::Object) => {
            let s = k.to_string();
            // An own accessor (even setter-only) counts as present.
            if crate::object::get_accessor_descriptor((b & 0x0000_FFFF_FFFF_FFFF) as usize, &s)
                .is_some()
            {
                return true;
            }
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            let key_val = f64::from_bits(JSValue::string_ptr(key).bits());
            if object_has_property_chain((b & 0x0000_FFFF_FFFF_FFFF) as usize, key_val) {
                return true;
            }
            // Canonical Object.prototype probe (data or accessor).
            k >= 0
                && k <= u32::MAX as i64
                && crate::array::object_prototype_has_index_prop(k as u32)
        }
        // Typed arrays / buffers are dense over their length.
        Some(PtrKind::IndexedNative) => k < al_length(recv),
        Some(PtrKind::ExpandoExotic(kind)) => {
            let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
            crate::object::exotic_expando::exotic_has_own_property(kind, raw, &k.to_string())
        }
        Some(PtrKind::Proxy) => {
            crate::value::js_is_truthy(crate::proxy::js_proxy_has(recv, proxy_string_key(k))) != 0
        }
        Some(PtrKind::Exotic) | None => false,
    }
}

/// `[[HasProperty]]` (ECMA-262 §10.1.7) over the recorded prototype chain for an
/// ordinary heap object. `js_object_has_property` (the `in` operator backend)
/// only scans the receiver's *own* keys for the plain-object case, so the
/// generic Array algorithms (which spec on `HasProperty`) missed inherited
/// indexed properties — e.g. an element living on `Object.prototype[k]` or a
/// `proto` from `Object.create(proto)`. Walk own-then-prototype here so
/// `Array.prototype.forEach.call(obj, …)` visits inherited indices.
fn object_has_property_chain(obj_ptr: usize, key_val: f64) -> bool {
    let mut cur = obj_ptr;
    // Bound the walk to guard against user-induced prototype cycles.
    for _ in 0..1000 {
        if cur == 0 {
            return false;
        }
        let cur_val = f64::from_bits(crate::value::js_nanbox_pointer(cur as i64).to_bits());
        if crate::object::js_object_has_own(cur_val, key_val).to_bits() == TAG_TRUE {
            return true;
        }
        // Advance to the recorded [[Prototype]] (explicit `setPrototypeOf`) or
        // the default `Object.prototype` for a plain `{}` object.
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return false,
            },
        };
        if proto_bits == TAG_NULL {
            return false;
        }
        let top16 = proto_bits >> 48;
        let next = if top16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return false;
        };
        if next == cur {
            return false;
        }
        cur = next;
    }
    false
}

/// RAII-ish guard binding the callback `this` (the optional `thisArg`) for the
/// duration of a generic iteration, restoring the previous binding on drop.
struct ThisGuard(f64);
impl ThisGuard {
    fn new(this_arg: f64) -> Self {
        ThisGuard(crate::object::js_implicit_this_set(this_arg))
    }
}
impl Drop for ThisGuard {
    fn drop(&mut self) {
        crate::object::js_implicit_this_set(self.0);
    }
}

// ---------------------------------------------------------------------------
// Callback iteration methods. The callback receives `(value, index, O)` with
// `O` the *original* receiver value; `this_arg` binds the callback's `this`.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_forEach(recv: f64, cb: f64, this_arg: f64) -> f64 {
    // #5989: a Set/Map reaching this array-like fallback (a member-access
    // `<expr>.forEach(cb, thisArg)` receiver codegen couldn't prove a
    // collection) has no `.length`; delegate to its real `forEach` — see
    // `super::generic_mutators::arraylike_collection_foreach`.
    if super::generic_mutators::arraylike_collection_foreach(recv, cb, this_arg) {
        return undef();
    }
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        js_closure_call3(cb, v, k as f64, recv);
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_map(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    // ArraySpeciesCreate → ArrayCreate throws RangeError for len ≥ 2^32
    // (test262 map/create-non-array-invalid-len) — BEFORE any callback runs.
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let result = js_array_alloc_with_length(len.max(0) as u32);
    let elems = unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue; // preserve holes
        }
        let v = al_get(recv, k);
        let mapped = js_closure_call3(cb, v, k as f64, recv);
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
            ptr::write(elems.add(k as usize), mapped);
            note_array_slot(result, k as usize, mapped.to_bits());
        }
    }
    nanbox_arr(result)
}

#[no_mangle]
pub extern "C" fn js_arraylike_filter(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut result = js_array_alloc(0);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        let keep = js_closure_call3(cb, v, k as f64, recv);
        if crate::value::js_is_truthy(keep) != 0 {
            result = js_array_push_f64(result, v);
        }
    }
    nanbox_arr(result)
}

#[no_mangle]
pub extern "C" fn js_arraylike_some(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return boxed_bool(true);
        }
    }
    boxed_bool(false)
}

#[no_mangle]
pub extern "C" fn js_arraylike_every(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) == 0 {
            return boxed_bool(false);
        }
    }
    boxed_bool(true)
}

// find / findIndex / findLast / findLastIndex do NOT skip holes (spec uses
// Get, treating absent as undefined).

#[no_mangle]
pub extern "C" fn js_arraylike_find(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return v;
        }
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_findIndex(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return k as f64;
        }
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_findLast(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    let mut k = len - 1;
    while k >= 0 {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return v;
        }
        k -= 1;
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_findLastIndex(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    let mut k = len - 1;
    while k >= 0 {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return k as f64;
        }
        k -= 1;
    }
    -1.0
}

// ---------------------------------------------------------------------------
// reduce / reduceRight — accumulator, optional initial value.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_reduce(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut acc = init;
    let mut k = 0i64;
    if has_init == 0 {
        // Seed from the first present element.
        loop {
            if k >= len {
                super::generic_mutators::throw_reduce_empty();
            }
            if al_has(recv, k) {
                acc = al_get(recv, k);
                k += 1;
                break;
            }
            k += 1;
        }
    }
    while k < len {
        if al_has(recv, k) {
            let v = al_get(recv, k);
            acc = crate::closure::js_closure_call4(cb, acc, v, k as f64, recv);
        }
        k += 1;
    }
    acc
}

#[no_mangle]
pub extern "C" fn js_arraylike_reduceRight(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut acc = init;
    let mut k = len - 1;
    if has_init == 0 {
        loop {
            if k < 0 {
                super::generic_mutators::throw_reduce_empty();
            }
            if al_has(recv, k) {
                acc = al_get(recv, k);
                k -= 1;
                break;
            }
            k -= 1;
        }
    }
    while k >= 0 {
        if al_has(recv, k) {
            let v = al_get(recv, k);
            acc = crate::closure::js_closure_call4(cb, acc, v, k as f64, recv);
        }
        k -= 1;
    }
    acc
}

// ---------------------------------------------------------------------------
// Search methods.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_indexOf(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return -1.0;
    }
    // ToIntegerOrInfinity(fromIndex), clamped.
    let mut start = if has_from == 0 {
        0
    } else {
        let n = crate::array::search::from_index_to_integer(from);
        if n >= len as f64 {
            return -1.0;
        } else if n >= 0.0 {
            n as i64
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            0
        }
    };
    if start < 0 {
        start = 0;
    }
    for k in start..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_jsvalue_equals(v, value) == 1 {
            return k as f64;
        }
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_lastIndexOf(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return -1.0;
    }
    let mut start = if has_from == 0 {
        len - 1
    } else {
        let n = crate::array::search::from_index_to_integer(from);
        if n >= 0.0 {
            (n as i64).min(len - 1)
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            return -1.0;
        }
    };
    while start >= 0 {
        if al_has(recv, start) {
            let v = al_get(recv, start);
            if crate::value::js_jsvalue_equals(v, value) == 1 {
                return start as f64;
            }
        }
        start -= 1;
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_includes(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return boxed_bool(false);
    }
    let mut start = if has_from == 0 {
        0
    } else {
        let n = crate::array::search::from_index_to_integer(from);
        if n >= len as f64 {
            return boxed_bool(false);
        } else if n >= 0.0 {
            n as i64
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            0
        }
    };
    if start < 0 {
        start = 0;
    }
    // includes does NOT skip holes — absent indices read as undefined.
    for k in start..len {
        let v = al_get(recv, k);
        if crate::value::js_jsvalue_same_value_zero(v, value) == 1 {
            return boxed_bool(true);
        }
    }
    boxed_bool(false)
}

// ---------------------------------------------------------------------------
// at / join / slice — no callback identity concerns; materialise where it
// keeps the implementation simple (slice/join build fresh results anyway).
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_at(recv: f64, index: f64) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    let n = if index.is_nan() { 0.0 } else { index.trunc() };
    let mut k = n as i64;
    if k < 0 {
        k += len;
    }
    if k < 0 || k >= len {
        return undef();
    }
    al_get(recv, k)
}

/// Materialise `recv` into a fresh real array (holes preserved as `TAG_HOLE`),
/// for the delegating `join` / `slice` paths.
fn materialize(recv: f64) -> *mut ArrayHeader {
    let len = al_length(recv);
    let arr = js_array_alloc_with_length(len.max(0) as u32);
    let elems = unsafe { (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..len {
        if !al_has(recv, k) {
            continue; // leave the hole
        }
        let v = al_get(recv, k);
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
            ptr::write(elems.add(k as usize), v);
            note_array_slot(arr, k as usize, v.to_bits());
        }
    }
    arr
}

#[no_mangle]
pub extern "C" fn js_arraylike_join(recv: f64, sep: f64) -> f64 {
    let recv = to_object(recv);
    let arr = materialize(recv);
    let sep_ptr = if sep.to_bits() == TAG_UNDEFINED {
        ptr::null()
    } else {
        crate::value::js_jsvalue_to_string(sep) as *const crate::string::StringHeader
    };
    let s = crate::array::js_array_join(arr, sep_ptr);
    f64::from_bits(JSValue::string_ptr(s).bits())
}

#[no_mangle]
pub extern "C" fn js_arraylike_slice(
    recv: f64,
    start: f64,
    has_start: i32,
    end: f64,
    has_end: i32,
) -> f64 {
    let recv = to_object(recv);
    let arr = materialize(recv);
    let len = unsafe { (*arr).length as i64 };
    let s = if has_start == 0 {
        0
    } else {
        clamp_index(start, len)
    };
    // ECMA-262 §23.1.3.25 step 4: an `end` of `undefined` (whether omitted OR
    // passed explicitly, e.g. `Array.prototype.slice.call(arr, 1, undefined)`)
    // means "to the end" (relativeEnd = len). Only a present, non-undefined
    // `end` is run through ToIntegerOrInfinity. `clamp_index` maps the
    // TAG_UNDEFINED bit pattern (a NaN) to 0, which would wrongly empty the
    // slice — so special-case it here.
    let e = if has_end == 0 || end.to_bits() == crate::value::TAG_UNDEFINED {
        len
    } else {
        clamp_index(end, len)
    };
    let result = js_array_slice(arr, s as i32, e as i32);
    nanbox_arr(result)
}

/// ECMA-262 relative-index clamp used by `slice` (negative counts from the end,
/// `NaN`/`-Infinity` → 0, `+Infinity` → len).
fn clamp_index(v: f64, len: i64) -> i64 {
    let n = if v.is_nan() { 0.0 } else { v.trunc() };
    if n < 0.0 {
        let r = len + n as i64;
        r.max(0)
    } else if n > len as f64 {
        len
    } else {
        n as i64
    }
}

// Keep the generic entry points anchored against dead-strip in the default
// (codegen-only reference) compile path.
// Keep the generic entry points anchored against dead-strip in the default
// (codegen-only reference) compile path (see #3320 — `#[no_mangle]` alone is
// not enough once the bitcode is re-linked).
#[used]
static KEEP_ARRAYLIKE_CB: [extern "C" fn(f64, f64, f64) -> f64; 9] = [
    js_arraylike_forEach,
    js_arraylike_map,
    js_arraylike_filter,
    js_arraylike_some,
    js_arraylike_every,
    js_arraylike_find,
    js_arraylike_findIndex,
    js_arraylike_findLast,
    js_arraylike_findLastIndex,
];
#[used]
static KEEP_ARRAYLIKE_REDUCE: [extern "C" fn(f64, f64, i32, f64) -> f64; 2] =
    [js_arraylike_reduce, js_arraylike_reduceRight];
#[used]
static KEEP_ARRAYLIKE_SEARCH: [extern "C" fn(f64, f64, f64, i32) -> f64; 3] = [
    js_arraylike_indexOf,
    js_arraylike_lastIndexOf,
    js_arraylike_includes,
];
#[used]
static KEEP_ARRAYLIKE_AT: extern "C" fn(f64, f64) -> f64 = js_arraylike_at;
#[used]
static KEEP_ARRAYLIKE_JOIN: extern "C" fn(f64, f64) -> f64 = js_arraylike_join;
#[used]
static KEEP_ARRAYLIKE_SLICE: extern "C" fn(f64, f64, i32, f64, i32) -> f64 = js_arraylike_slice;

// ---------------------------------------------------------------------------
// Generic array-like MUTATORS over a plain-object receiver (#4742 follow-up).
//
// `Array.prototype.{pop,shift,push,unshift,reverse,splice}` are intentionally
// generic (ECMA-262 §23.1.3) — they operate on `O = ToObject(this)` with live
// `Get`/`Set`/`Delete`/`HasProperty` and a writable `length`. Perry's dense
// fast paths assume a real `ArrayHeader`; when the receiver is a plain object
// (a stored `obj.pop = Array.prototype.pop; obj.pop()` borrow, or
// `Array.prototype.pop.call(obj, …)`), the dense path read the object's
// `ObjectHeader` words as an `ArrayHeader` and corrupted/crashed
// (`TypeError: Cannot convert object to primitive value`).
//
// These helpers run the spec algorithm by mutating the *original* receiver
// object in place via the polymorphic index get/set/delete and a `length`
// property write. They are dispatched from `js_native_call_method` only when
// the receiver classifies as a plain `Object` (never a real array / typed
// array / buffer / primitive), so the hot real-array paths are untouched.
// ---------------------------------------------------------------------------

/// `Set(O, ToString(k), v, true)` for an array-like object receiver.
fn al_set(recv: f64, k: i64, v: f64) {
    if crate::proxy::js_proxy_is_proxy(recv) != 0 {
        crate::proxy::js_proxy_set(recv, proxy_string_key(k), v);
        return;
    }
    crate::object::js_object_set_index_polymorphic(recv.to_bits() as i64, k as f64, v);
}

/// `DeletePropertyOrThrow(O, ToString(k))` for an array-like object receiver.
fn al_delete(recv: f64, k: i64) {
    if crate::proxy::js_proxy_is_proxy(recv) != 0 {
        crate::proxy::js_proxy_delete(recv, proxy_string_key(k));
        return;
    }
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;
    crate::object::js_object_delete_dynamic(raw, k as f64);
}

/// `Set(O, "length", len, true)` for an array-like object receiver. An own
/// `length` ACCESSOR fires its setter — and throws TypeError when there is
/// none (getter-only `length`, test262 splice/S15.4.4.12_A6.1_T3), matching
/// `Set(..., true)` on a non-writable slot.
fn al_set_length(recv: f64, len: i64) {
    let raw_addr = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if let Some(acc) = crate::object::get_accessor_descriptor(raw_addr, "length") {
        if acc.set != 0 {
            unsafe { crate::object::invoke_accessor_setter(acc.set, recv, len as f64) };
            return;
        }
        crate::collection_iter::throw_type_error(
            "Cannot set property length of object which has only a getter",
        );
    }
    // An object-LITERAL `get length()` lives in the anon-shape class vtable,
    // not the defineProperty descriptor table — a getter with no setter makes
    // `Set(O, "length", ..., true)` throw (test262 splice/S15.4.4.12_A6.1_T3).
    {
        let raw = raw_addr as *const crate::object::ObjectHeader;
        let class_id = crate::object::js_object_get_class_id(raw);
        if class_id != 0 {
            if let Some((getter, setter)) =
                crate::object::class_own_accessor_ptrs(class_id, "length")
            {
                if setter == 0 && getter != 0 {
                    crate::collection_iter::throw_type_error(
                        "Cannot set property length of object which has only a getter",
                    );
                }
            }
        }
    }
    // `Set(O, "length", …, true)` (Throw=true) must throw a TypeError when the
    // set fails: a frozen array, an array/object whose `length` is a
    // non-writable data property (`defineProperty(o,"length",{writable:false})`
    // or a function's intrinsic `length`), or a frozen plain object. The
    // by-name setter below silently no-ops in those cases (PutValue's
    // non-strict fall-through), so detect the failure up front and throw.
    if al_length_write_would_fail(recv, raw_addr) {
        crate::collection_iter::throw_type_error(
            "Cannot assign to read only property 'length' of object",
        );
    }
    let raw = raw_addr as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(raw, key, len as f64);
}

/// True when `Set(recv, "length", …)` would fail (and so must throw under
/// `Throw=true`): a frozen array, a non-writable `length` data property, or a
/// callable receiver (a function's `length` is non-writable by spec).
fn al_length_write_would_fail(recv: f64, raw_addr: usize) -> bool {
    // Frozen array: `length` is non-writable after `Object.freeze`.
    let arr = as_real_array(recv);
    if !arr.is_null() {
        if crate::array::array_is_frozen(arr) {
            return true;
        }
        return crate::object::get_property_attrs(raw_addr, "length")
            .map(|a| !a.writable())
            .unwrap_or(false);
    }
    // Non-array receivers: a recorded non-writable `length` descriptor
    // (`defineProperty(o,"length",{writable:false})`) or a callable receiver
    // whose intrinsic `length` is non-writable.
    if crate::object::get_property_attrs(raw_addr, "length")
        .map(|a| !a.writable())
        .unwrap_or(false)
    {
        return true;
    }
    crate::closure::is_closure_ptr(raw_addr)
}

/// `ToIntegerOrInfinity(v)` as an `f64` (NaN → 0; ±Infinity preserved).
fn to_integer_or_infinity(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else if v.is_infinite() {
        v
    } else {
        v.trunc()
    }
}

/// Resolve a relative index argument (`splice` start) to an absolute,
/// clamped `[0, len]` index.
fn relative_index(v: f64, len: i64) -> i64 {
    let n = to_integer_or_infinity(v);
    if n < 0.0 {
        let r = len as f64 + n;
        if r < 0.0 {
            0
        } else {
            r as i64
        }
    } else if n > len as f64 {
        len
    } else {
        n as i64
    }
}

#[inline]
fn arg_at(args_ptr: *const f64, args_len: usize, i: usize) -> f64 {
    if i < args_len && !args_ptr.is_null() {
        unsafe { *args_ptr.add(i) }
    } else {
        undef()
    }
}

/// A receiver that reached a dense `ArrayHeader` entry point but is actually
/// a plain object/closure at runtime (a variable whose static type was
/// inferred `Array` and later reassigned — `var x = [1, 0]; … x = {0:1,1:0};
/// x.sort()`, test262 sort/S15.4.4.11_A6_T2 #5, splice/S15.4.4.12_A4_T1 #7).
/// Returns it NaN-boxed for the generic engine, or `None` for real arrays.
pub(crate) fn non_array_object_receiver(arr: *const ArrayHeader) -> Option<f64> {
    let raw = arr as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    if !crate::value::addr_class::is_above_handle_band(raw)
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return None;
    }
    let obj_type = unsafe {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
        Some(f64::from_bits(JSValue::pointer(raw as *const u8).bits()))
    } else {
        None
    }
}

/// If `arr` points to a *plain* object (an object literal — `GC_TYPE_OBJECT`
/// with `class_id == 0` or an anonymous shape id), return it NaN-boxed as a
/// receiver value, else `None`. Used by the dense `Array.prototype` mutator
/// entry points to detect a borrowed array-like receiver (`obj.pop =
/// Array.prototype.pop; obj.pop()`, whose thunk calls the dense helper with the
/// object pointer) and route it to the spec-generic engine. Real arrays / typed
/// arrays / buffers / class instances return `None` and keep the dense path.
pub fn plain_object_value(arr: *const ArrayHeader) -> Option<f64> {
    let recv = f64::from_bits(JSValue::pointer(arr as *const u8).bits());
    if !matches!(classify_pointer(recv), Some(PtrKind::Object)) {
        return None;
    }
    let class_id = crate::object::js_object_get_class_id(arr as *const crate::object::ObjectHeader);
    if class_id != 0
        && !crate::object::is_anon_shape_class_id(class_id)
        && !super::subclass::is_array_subclass_class_id(class_id)
    {
        return None;
    }
    Some(recv)
}

/// `Array.prototype.pop` over an array-like object receiver.
pub(crate) fn object_pop(recv: f64) -> f64 {
    let len = al_length(recv);
    if len <= 0 {
        al_set_length(recv, 0);
        return undef();
    }
    let new_len = len - 1;
    let element = al_get(recv, new_len);
    al_delete(recv, new_len);
    al_set_length(recv, new_len);
    element
}

/// `Array.prototype.shift` over an array-like object receiver.
pub(crate) fn object_shift(recv: f64) -> f64 {
    let len = al_length(recv);
    if len <= 0 {
        al_set_length(recv, 0);
        return undef();
    }
    let first = al_get(recv, 0);
    for k in 1..len {
        if al_has(recv, k) {
            al_set(recv, k - 1, al_get(recv, k));
        } else {
            al_delete(recv, k - 1);
        }
    }
    al_delete(recv, len - 1);
    al_set_length(recv, len - 1);
    first
}

/// `Array.prototype.push` over an array-like object receiver. Returns the new
/// length.
fn object_push(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    for i in 0..args_len {
        al_set(recv, len + i as i64, arg_at(args_ptr, args_len, i));
    }
    let new_len = len + args_len as i64;
    al_set_length(recv, new_len);
    new_len as f64
}

/// `Array.prototype.unshift` over an array-like object receiver. Returns the
/// new length.
fn object_unshift(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    let count = args_len as i64;
    if count > 0 {
        // Move existing elements up by `count`, high index first so we don't
        // clobber not-yet-moved slots.
        let mut k = len;
        while k > 0 {
            let from = k - 1;
            let to = from + count;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k -= 1;
        }
        for j in 0..count {
            al_set(recv, j, arg_at(args_ptr, args_len, j as usize));
        }
    }
    let new_len = len + count;
    al_set_length(recv, new_len);
    new_len as f64
}

/// `Array.prototype.reverse` over an array-like object receiver. Returns the
/// receiver.
fn object_reverse(recv: f64) -> f64 {
    let len = al_length(recv);
    let middle = len / 2;
    let mut lower = 0;
    while lower < middle {
        let upper = len - 1 - lower;
        let lower_exists = al_has(recv, lower);
        let upper_exists = al_has(recv, upper);
        let lower_val = al_get(recv, lower);
        let upper_val = al_get(recv, upper);
        match (lower_exists, upper_exists) {
            (true, true) => {
                al_set(recv, lower, upper_val);
                al_set(recv, upper, lower_val);
            }
            (false, true) => {
                al_set(recv, lower, upper_val);
                al_delete(recv, upper);
            }
            (true, false) => {
                al_delete(recv, lower);
                al_set(recv, upper, lower_val);
            }
            (false, false) => {}
        }
        lower += 1;
    }
    recv
}

/// `Array.prototype.splice` over an array-like object receiver. Returns a fresh
/// plain array of the removed elements (holes preserved).
pub(crate) fn object_splice(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    let actual_start = relative_index(arg_at(args_ptr, args_len, 0), len);
    let delete_count = if args_len == 0 {
        0
    } else if args_len == 1 {
        len - actual_start
    } else {
        let dc = to_integer_or_infinity(arg_at(args_ptr, args_len, 1));
        dc.max(0.0).min((len - actual_start) as f64) as i64
    };
    // Removed elements -> fresh plain array (holes preserved). ArrayCreate
    // throws RangeError for a count ≥ 2^32 (splice/create-non-array-invalid-len).
    if delete_count > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let removed = js_array_alloc_with_length(delete_count.max(0) as u32);
    let removed_elems =
        unsafe { (removed as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..delete_count {
        let from = actual_start + k;
        if al_has(recv, from) {
            let v = al_get(recv, from);
            unsafe {
                // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
                ptr::write(removed_elems.add(k as usize), v);
                note_array_slot(removed, k as usize, v.to_bits());
            }
        }
    }
    let item_count = args_len.saturating_sub(2) as i64;
    if item_count < delete_count {
        // Shift the tail down to close the gap.
        let mut k = actual_start;
        while k < len - delete_count {
            let from = k + delete_count;
            let to = k + item_count;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k += 1;
        }
        // Delete the now-vacated trailing slots.
        let mut k = len;
        while k > len - delete_count + item_count {
            al_delete(recv, k - 1);
            k -= 1;
        }
    } else if item_count > delete_count {
        // Open a gap by shifting the tail up.
        let mut k = len - delete_count;
        while k > actual_start {
            let from = k + delete_count - 1;
            let to = k + item_count - 1;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k -= 1;
        }
    }
    // Write the inserted items.
    for j in 0..item_count {
        al_set(
            recv,
            actual_start + j,
            arg_at(args_ptr, args_len, 2 + j as usize),
        );
    }
    al_set_length(recv, len - delete_count + item_count);
    nanbox_arr(removed)
}

/// `Array.prototype.sort` over an array-like (non-real-array) receiver:
/// ECMA-262 SortIndexedProperties with holes skipped — collect via
/// `HasProperty`/`Get`, sort (undefined trailing, never compared), write back
/// via `Set` and `Delete` the trailing range. Returns the receiver.
/// `cmp_validated` is the already-validated comparator (null = default sort).
pub(crate) fn object_sort(recv: f64, cmp_validated: *const ClosureHeader) -> f64 {
    let cmp = if cmp_validated.is_null() {
        None
    } else {
        Some(super::sort::ComparatorCall::new(cmp_validated))
    };
    let len = al_length(recv);
    unsafe {
        // Root BOTH the receiver value and the collection temp for the whole
        // protocol: `al_has`/`al_get`/`al_set` fire user accessors (and the
        // comparator runs inside `sort_rooted_values`) — any of them can
        // allocate and sweep or move either object, so every raw pointer is
        // re-derived from its rooted handle after each such call.
        let scope = crate::gc::RuntimeHandleScope::new();
        let recv_handle = scope.root_nanbox_f64(recv);
        let temp = super::sort::RootedArrayElems::new(
            &scope,
            js_array_alloc_with_length(len.clamp(0, u32::MAX as i64) as u32),
        );
        let mut count = 0usize;
        let mut undef_count = 0usize;
        for j in 0..len {
            if al_has(recv_handle.get_nanbox_f64(), j) {
                let v = al_get(recv_handle.get_nanbox_f64(), j);
                if v.to_bits() == TAG_UNDEFINED {
                    undef_count += 1;
                } else {
                    temp.set(count, v);
                    count += 1;
                }
            }
        }
        (*temp.arr()).length = count as u32;
        rebuild_array_layout(temp.arr());
        let _ = super::sort::sort_rooted_values(temp.arr(), count, cmp);
        for j in 0..count {
            al_set(recv_handle.get_nanbox_f64(), j as i64, temp.get(j));
        }
        for j in count..count + undef_count {
            al_set(recv_handle.get_nanbox_f64(), j as i64, undef());
        }
        for j in (count + undef_count) as i64..len {
            al_delete(recv_handle.get_nanbox_f64(), j);
        }
        recv_handle.get_nanbox_f64()
    }
}

/// `Array.prototype.concat` over a non-real-array receiver: the receiver is
/// the first concat element (spread only when `@@isConcatSpreadable` says so —
/// a plain object/wrapper lands as a single element), then each argument is
/// appended with the usual spreadability rules.
fn object_concat(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let mut result = super::from_concat::append_concat_arg(js_array_alloc(0), recv);
    for i in 0..args_len {
        result = super::from_concat::append_concat_arg(result, arg_at(args_ptr, args_len, i));
    }
    nanbox_arr(result)
}

/// Generic `Array.prototype.sort.call(receiver, comparator?)` entry
/// (#4597 extension): ToObject + route real arrays to the dense/spec sort,
/// everything else through the array-like engine. Returns the receiver.
#[no_mangle]
pub extern "C" fn js_arraylike_sort(recv: f64, comparator: f64) -> f64 {
    // Spec step 1: comparator must be undefined or callable — BEFORE ToObject.
    let cmp = crate::array::js_validate_array_comparator(comparator) as *const ClosureHeader;
    let o = to_object(recv);
    let arr = as_real_array(o);
    if !arr.is_null() {
        let r = crate::array::js_array_sort_with_comparator(arr, cmp);
        return nanbox_arr(r);
    }
    object_sort(o, cmp)
}

/// Generic `Array.prototype.concat.call(receiver, ...items)` entry.
#[no_mangle]
pub extern "C" fn js_arraylike_concat(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    let o = to_object(recv);
    let arr = as_real_array(o);
    if !arr.is_null() {
        let r = crate::array::js_array_concat_variadic(arr, args_ptr, count.max(0));
        return nanbox_arr(r);
    }
    object_concat(o, args_ptr, count.max(0) as usize)
}

/// Generic `Array.prototype.splice.call(receiver, start?, deleteCount?, ...items)`.
#[no_mangle]
pub extern "C" fn js_arraylike_splice(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    let o = to_object(recv);
    let count = count.max(0) as usize;
    let arr = as_real_array(o);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, "splice", args_ptr, count) };
    }
    object_splice(o, args_ptr, count)
}

#[used]
static KEEP_ARRAYLIKE_SORT: extern "C" fn(f64, f64) -> f64 = js_arraylike_sort;
#[used]
static KEEP_ARRAYLIKE_VARIADIC: [extern "C" fn(f64, *const f64, i32) -> f64; 2] =
    [js_arraylike_concat, js_arraylike_splice];

/// Walk the receiver's [[Prototype]] chain looking for a *real array* link —
/// the `function foo() {}; foo.prototype = new Array(1, 2, 3); new foo()`
/// shape (test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*). Such a receiver
/// inherits `Array.prototype` methods through the array, so the generic
/// engine must serve them; a plain `{}` (no array on the chain) keeps the
/// normal "not a function" behavior.
fn proto_chain_contains_real_array(obj_ptr: usize) -> bool {
    let mut cur = obj_ptr;
    for _ in 0..64 {
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return false,
            },
        };
        if proto_bits == TAG_NULL {
            return false;
        }
        let t16 = proto_bits >> 48;
        let next = if t16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if t16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return false;
        };
        if next == cur {
            return false;
        }
        let next_val = f64::from_bits(crate::value::js_nanbox_pointer(next as i64).to_bits());
        if !as_real_array(next_val).is_null() {
            return true;
        }
        cur = next;
    }
    false
}

/// Dynamic-dispatch hook: a plain-object receiver whose prototype chain
/// contains a real array inherits the `Array.prototype` methods through it.
/// Routes the generic-engine methods; returns `None` for receivers with an
/// own user method of this name, no array on the chain, or unsupported names.
pub fn try_array_proto_chain_method(
    object: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(classify_pointer(object), Some(PtrKind::Object)) {
        return None;
    }
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
    let own = crate::object::js_object_get_field_by_name_f64(raw, key);
    if matches!(classify_own_slot(own), OwnSlot::UserMethod) {
        return None;
    }
    if !proto_chain_contains_real_array(raw as usize) {
        return None;
    }
    dispatch_arraylike_read_method(object, method, args_ptr, args_len)
}

/// Dispatch a non-mutating generic `Array.prototype` method (the spec-generic
/// engine over `[[Get]]`/`length`) on an arbitrary array-like receiver.
/// Returns `None` for an unsupported method name. The receiver may be a plain
/// object, a Proxy (#5196), etc. — `al_get`/`al_length` route element reads
/// through the receiver's `[[Get]]` (so proxy `get` traps fire). Callers are
/// responsible for any receiver/own-property gating they need.
pub fn dispatch_arraylike_read_method(
    object: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let a = |i: usize| arg_at(args_ptr, args_len, i);
    let has = |i: usize| (args_len > i) as i32;
    Some(match method {
        "forEach" => js_arraylike_forEach(object, a(0), a(1)),
        "map" => js_arraylike_map(object, a(0), a(1)),
        "filter" => js_arraylike_filter(object, a(0), a(1)),
        "some" => js_arraylike_some(object, a(0), a(1)),
        "every" => js_arraylike_every(object, a(0), a(1)),
        "find" => js_arraylike_find(object, a(0), a(1)),
        "findIndex" => js_arraylike_findIndex(object, a(0), a(1)),
        "findLast" => js_arraylike_findLast(object, a(0), a(1)),
        "findLastIndex" => js_arraylike_findLastIndex(object, a(0), a(1)),
        "reduce" => js_arraylike_reduce(object, a(0), has(1), a(1)),
        "reduceRight" => js_arraylike_reduceRight(object, a(0), has(1), a(1)),
        "indexOf" => js_arraylike_indexOf(object, a(0), a(1), has(1)),
        "lastIndexOf" => js_arraylike_lastIndexOf(object, a(0), a(1), has(1)),
        "includes" => js_arraylike_includes(object, a(0), a(1), has(1)),
        "at" => js_arraylike_at(object, a(0)),
        "join" => js_arraylike_join(object, a(0)),
        "slice" => js_arraylike_slice(object, a(0), has(0), a(1), has(1)),
        "sort" => js_arraylike_sort(object, a(0)),
        "concat" => js_arraylike_concat(object, args_ptr, args_len as i32),
        _ => return None,
    })
}

/// Run a generic `Array.prototype` mutator on `recv` for the reified prototype
/// method thunks (`Array.prototype.pop`, etc.). A real-array receiver routes to
/// the dense helpers; a plain array-like object routes to the spec-generic
/// engine; any other receiver yields `undefined`. `recv` is the call-site
/// `this` (IMPLICIT_THIS) the thunk read.
pub fn array_proto_mutator(recv: f64, method: &str, args_ptr: *const f64, args_len: usize) -> f64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, method, args_ptr, args_len) };
    }
    run_object_mutator(recv, method, args_ptr, args_len).unwrap_or_else(undef)
}

#[inline]
fn arg_or_undef(args_ptr: *const f64, args_len: usize, i: usize) -> f64 {
    if i < args_len && !args_ptr.is_null() {
        unsafe { *args_ptr.add(i) }
    } else {
        undef()
    }
}

/// Dense-array branch of [`array_proto_mutator`]. Reuses the existing dense
/// runtime helpers (matching the `js_native_call_method` array arms).
pub(super) unsafe fn real_array_mutator(
    arr: *mut ArrayHeader,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    match method {
        "pop" => crate::array::js_array_pop_f64(arr),
        "shift" => crate::array::js_array_shift_f64(arr),
        "reverse" => {
            crate::array::js_array_reverse(arr);
            nanbox_arr(arr)
        }
        "push" => {
            // Spec §23.1.3.21 step 6 calls Set(O,"length",…) even with 0 args,
            // so frozen / non-writable-length throws regardless of arg count.
            if crate::array::array_is_frozen(arr) {
                crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
            }
            crate::array::guard_writable_length(arr);
            let mut a = arr;
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, arg_or_undef(args_ptr, args_len, i));
            }
            crate::array::js_array_length(a) as f64
        }
        "unshift" => {
            // Route everything through js_array_unshift_variadic so the
            // frozen / non-writable-length guards fire even for 0-arg calls.
            let count = if args_ptr.is_null() { 0 } else { args_len };
            let r = crate::array::js_array_unshift_variadic(arr, args_ptr, count as u32);
            crate::array::js_array_length(r) as f64
        }
        "sort" => {
            let cmp =
                crate::array::js_validate_array_comparator(arg_or_undef(args_ptr, args_len, 0))
                    as *const ClosureHeader;
            nanbox_arr(crate::array::js_array_sort_with_comparator(arr, cmp))
        }
        "concat" => {
            let count = if args_ptr.is_null() {
                0
            } else {
                args_len as i32
            };
            nanbox_arr(crate::array::js_array_concat_variadic(arr, args_ptr, count))
        }
        "splice" => {
            // ToIntegerOrInfinity with i32 clamping: NaN → 0, +Infinity →
            // i32::MAX (clamps to len downstream), -Infinity → i32::MIN
            // (relative-from-end clamps to 0). The old `is_infinite() → 0`
            // made `splice(Infinity, 3)` delete from the front (test262
            // splice/S15.4.4.12_A2.1_T3).
            let arg_i32 = |i: usize| -> i32 {
                crate::array::js_array_splice_delete_count(arg_or_undef(args_ptr, args_len, i))
            };
            let start = if args_len >= 1 { arg_i32(0) } else { 0 };
            let delete_count = if args_len == 0 {
                0
            } else if args_len == 1 {
                i32::MAX
            } else {
                arg_i32(1)
            };
            let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
            } else {
                Vec::new()
            };
            let items_ptr = if items.is_empty() {
                ptr::null()
            } else {
                items.as_ptr()
            };
            let mut out_arr: *mut ArrayHeader = ptr::null_mut();
            let deleted = crate::array::js_array_splice(
                arr,
                start,
                delete_count,
                items_ptr,
                items.len() as u32,
                &mut out_arr,
            );
            nanbox_arr(deleted)
        }
        _ => undef(),
    }
}

/// Classify the own `method_name` slot of an array-like receiver: `Absent`
/// (no callable), `UserMethod` (a genuine user closure/function), or
/// `BorrowedBuiltin` (a `BOUND_METHOD` closure — `obj.pop = Array.prototype.pop`).
/// Only `Absent` and `BorrowedBuiltin` route to the generic engine; a borrowed
/// builtin, dispatched normally, binds the wrong receiver (its captured
/// `Array.prototype`) and loops, so the engine must run on the real receiver.
enum OwnSlot {
    Absent,
    UserMethod,
    BorrowedBuiltin,
}

fn classify_own_slot(v: f64) -> OwnSlot {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return OwnSlot::Absent;
    }
    let c = jv.as_pointer::<ClosureHeader>();
    if c.is_null() {
        return OwnSlot::Absent;
    }
    let fp = crate::closure::get_valid_func_ptr(c);
    if fp.is_null() {
        OwnSlot::Absent
    } else if fp == crate::closure::BOUND_METHOD_FUNC_PTR
        // A raw built-in prototype-method closure (`{ splice:
        // Array.prototype.splice }` stores the thunk itself, not a bound
        // reification) must also run the generic engine on THIS receiver —
        // dispatching it as a user method loses the receiver entirely
        // (test262 splice/S15.4.4.12_A6.1_T3).
        || crate::object::builtin_closure_is_non_constructable_value(v)
    {
        OwnSlot::BorrowedBuiltin
    } else {
        OwnSlot::UserMethod
    }
}

/// True when `object` owns a user method (an own callable field, not a borrowed
/// builtin) named `method`, so an array-like read/iterate fast path must defer
/// to it rather than hijack the name. Mirrors the own-slot gate that
/// [`try_object_arraylike_mutator`] applies to the mutating family, for callers
/// (the Array-subclass read arm) that reach `dispatch_arraylike_read_method`
/// directly.
pub(crate) fn object_owns_user_method(object: f64, method: &str) -> bool {
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
    let own = crate::object::js_object_get_field_by_name_f64(raw, key);
    matches!(classify_own_slot(own), OwnSlot::UserMethod)
}

/// Dispatch a generic `Array.prototype` mutator over an array-like receiver.
///
/// Returns `Some(result)` only when `object` is a plain heap object / closure
/// (classified `Object`) — i.e. NOT a real array, typed array, buffer, exotic
/// cell, or primitive — so the dense real-array fast paths in
/// `js_native_call_method` keep their existing behavior. The caller routes
/// `pop` / `shift` / `push` / `unshift` / `reverse` / `splice` here before the
/// dense array arms that would otherwise read the object as an `ArrayHeader`.
///
/// A `class X extends Array` instance (see [`super::subclass`]) is also admitted
/// by the relaxed guard below, so its inherited mutators run on the object.
pub fn try_object_arraylike_mutator(
    object: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(classify_pointer(object), Some(PtrKind::Object)) {
        return None;
    }
    // Restrict to *plain* objects — object literals (`class_id == 0` or an
    // anonymous shape id). A real user class instance (`class Stack { push(){…} }`)
    // owns a registered class id; hijacking its same-named method would be a
    // regression, so leave those to the normal vtable dispatch.
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let class_id = crate::object::js_object_get_class_id(raw);
    if class_id != 0
        && !crate::object::is_anon_shape_class_id(class_id)
        && !super::subclass::is_array_subclass_class_id(class_id)
    {
        return None;
    }
    // Fire the generic engine when the own `method_name` slot is absent (the
    // `Array.prototype.<m>.call(obj, …)` borrow, dispatched by name) or holds a
    // borrowed builtin method (`obj.pop = Array.prototype.pop`, whose normal
    // dispatch binds its captured `Array.prototype` and loops). A genuine user
    // method (`{ push(x) {…} }`) is left to the normal dispatch.
    let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
    let own = crate::object::js_object_get_field_by_name_f64(raw, key);
    if matches!(classify_own_slot(own), OwnSlot::UserMethod) {
        return None;
    }
    run_object_mutator(object, method, args_ptr, args_len)
}

/// Run a generic `Array.prototype` mutator over a *plain-object* receiver
/// (object literal / anonymous shape — never a real array / typed array /
/// buffer / class instance). Returns `None` for any other receiver so the
/// caller keeps its existing behavior. Unlike [`try_object_arraylike_mutator`]
/// this applies NO own-property gate — callers (e.g. the bound-method dispatch
/// for a borrowed builtin) have already established that the array algorithm
/// must run on `recv`.
pub fn run_object_mutator(
    recv: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(classify_pointer(recv), Some(PtrKind::Object)) {
        return None;
    }
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let class_id = crate::object::js_object_get_class_id(raw);
    if class_id != 0
        && !crate::object::is_anon_shape_class_id(class_id)
        && !super::subclass::is_array_subclass_class_id(class_id)
    {
        return None;
    }
    let result = match method {
        "pop" => object_pop(recv),
        "shift" => object_shift(recv),
        "push" => object_push(recv, args_ptr, args_len),
        "unshift" => object_unshift(recv, args_ptr, args_len),
        "reverse" => object_reverse(recv),
        "splice" => object_splice(recv, args_ptr, args_len),
        "sort" => {
            let cmp = crate::array::js_validate_array_comparator(arg_at(args_ptr, args_len, 0))
                as *const ClosureHeader;
            object_sort(recv, cmp)
        }
        "concat" => object_concat(recv, args_ptr, args_len),
        _ => return None,
    };
    Some(result)
}
