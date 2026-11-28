//! The for-in key visit (`js_for_in_key_live`) and for-in over a Proxy
//! subject (PXFORIN1), split out of `enumeration.rs` for the 2000-line cap.
//! A Proxy subject runs node's traps in node's order (V8's for-in): `ownKeys`,
//! then `getPrototypeOf` when the keys are taken, and at each key's visit
//! `getOwnPropertyDescriptor` (14.7.5.9: property attributes come from
//! [[GetOwnProperty]]; a key deleted before its visit is not visited). No
//! `has` trap runs.

use super::*;

/// The for-in loop's deletion check `keys[idx] in obj` under the loop fact
/// `fact` (DYNP E3c). `keys` is the loop's snapshot (an array only the loop
/// holds) and `idx` the loop counter below its length, so the key is its
/// element `idx`, read directly. The own key still at its place answers
/// `true` from a few loads (`ordinary_keys::key_live_by_fact`); a Proxy
/// subject's key is visited by [`for_in_key_enumerable`]; everything else is
/// the complete `in` operator.
#[no_mangle]
pub extern "C" fn js_for_in_key_live(obj: f64, keys: f64, idx: f64, fact: i64) -> f64 {
    let pos = idx as u32;
    let arr = (keys.to_bits() & crate::value::POINTER_MASK) as *const ArrayHeader;
    let in_range = pos as f64 == idx && !arr.is_null() && unsafe { pos < (*arr).length };
    let key = if in_range {
        unsafe { f64::from_bits(*crate::array::array_elements_ptr(arr).add(pos as usize)) }
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    if fact != 0
        && in_range
        && unsafe {
            super::ordinary_keys::key_live_by_fact(obj.to_bits(), key.to_bits(), pos, fact as u64)
        }
    {
        return f64::from_bits(JSValue::bool(true).bits());
    }
    // A Proxy subject's keys are its `ownKeys` string keys, unfiltered
    // (`for_in_keys_with`, `proxy_subject`): the key's visit asks for its
    // descriptor, as node does (V8 HasEnumerableProperty). No `has` trap runs.
    if crate::proxy_hooks::proxy_is_proxy(obj) != 0 {
        let live = for_in_key_enumerable(obj, key);
        return f64::from_bits(JSValue::bool(live).bits());
    }
    crate::object::field_get_set::has_property::js_in_operator(obj, key)
}

/// The Proxy level of the keys: ONE `ownKeys` trap; every string key goes out
/// unfiltered (its visit reads its descriptor, [`for_in_key_enumerable`]) and
/// names the shadow set `seen` for the later levels.
pub(super) fn push_proxy_own_keys(
    proxy: f64,
    out: &crate::gc::RuntimeHandle<'_>,
    seen: &mut std::collections::HashSet<String>,
    scratch: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let own = crate::proxy_hooks::proxy_own_keys(proxy);
    let own_ptr = (own.to_bits() & crate::value::POINTER_MASK) as *const ArrayHeader;
    if own_ptr.is_null() {
        return;
    }
    let own_arr = scope.root_raw_const_ptr(own_ptr);
    let n = own_arr.with_const_ptr(|keys| crate::array::js_array_length(keys));
    for i in 0..n {
        let kv = own_arr.with_const_ptr(|keys| crate::array::js_array_get(keys, i));
        if !kv.is_any_string() {
            continue;
        }
        let name = unsafe { crate::string::js_string_key_bytes(kv, scratch) }
            .and_then(|b| std::str::from_utf8(b).ok().map(|s| s.to_string()));
        if let Some(name) = name {
            seen.insert(name);
        }
        let updated =
            out.with_mut_ptr(|out| crate::array::js_array_push_f64(out, f64::from_bits(kv.bits())));
        out.set_raw_mut_ptr(updated);
    }
}

/// The visit of a for-in key of a Proxy subject: the first holder on the
/// chain from `obj` whose [[GetOwnProperty]] answers decides, live iff its
/// descriptor is enumerable. An absent own key moves on through
/// [[GetPrototypeOf]] (on a Proxy its `getPrototypeOf` trap); a null
/// prototype ends it, not live. (V8 HasEnumerableProperty.)
pub(super) fn for_in_key_enumerable(obj: f64, key: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder = scope.root_nanbox_f64(obj);
    let key_h = scope.root_nanbox_f64(key);
    // Depth cap as in the key walk.
    for _ in 0..1000 {
        let desc = crate::proxy::js_reflect_get_own_property_descriptor(
            holder.get_nanbox_f64(),
            key_h.get_nanbox_f64(),
        );
        if desc.to_bits() != crate::value::TAG_UNDEFINED {
            let desc_h = scope.root_nanbox_f64(desc);
            // Intern "enumerable" first; the descriptor is read after it.
            let ek = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
            let ptr =
                (desc_h.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ObjectHeader;
            if ptr.is_null() {
                return false;
            }
            return crate::value::js_is_truthy(crate::object::js_object_get_field_by_name_f64(
                ptr, ek,
            )) != 0;
        }
        let proto =
            super::super::object_ops::js_object_get_prototype_of(holder.get_nanbox_f64());
        if !JSValue::from_bits(proto.to_bits()).is_reference() {
            return false;
        }
        holder.set_nanbox_f64(proto);
    }
    false
}
