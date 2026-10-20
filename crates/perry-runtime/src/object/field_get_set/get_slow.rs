//! Object model step 5 (OBJECT_MODEL_2026-10-13.md §3.7): the read entries
//! behind every named read site.
//!
//! A site reads a coloured key through its 2c arm, and everything else
//! through the unknown-receiver probe (`layout_probe.rs`). What the probe
//! declines reaches one of these entries, which hold no site state: an
//! accessor the probe's walk handed back is called; every exotic receiver
//! (a closure, an array, a typed array, a buffer, a native handle, a Proxy,
//! a namespace, a string wrapper, ...) takes the complete by-name operation,
//! which is its spec path.
use crate::object::ObjectHeader;

/// `[[Get]]` through the accessor `acc` that the layout walk found for a key
/// of `obj` (on `obj` or on its chain), with `obj` as `this`. `None` for a
/// static class accessor, which the complete read calls.
#[inline]
unsafe fn call_getter(
    obj: *const ObjectHeader,
    acc: crate::object::accessor_pair::Accessor,
) -> Option<f64> {
    let this = crate::value::js_nanbox_pointer(obj as i64);
    if acc.raw_get != 0 {
        let f = crate::closure::body_call::js_method_body_fn!(acc.raw_get as *const u8;);
        return Some(f(this));
    }
    if acc.static_get != 0 {
        return None;
    }
    if acc.get == 0 {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    // A getter that takes `this` from the call (not a capture) is called with
    // the receiver directly, as a site accessor entry calls it
    // (`accessor_pair::holder_closure_getter`).
    let closure = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if crate::closure::is_closure_bits(acc.get)
        && !crate::closure::closure_reads_this_from_capture(closure)
    {
        return Some(crate::closure::js_closure_call0(
            closure,
            crate::closure::JsThis::from_f64(this),
        ));
    }
    let v = crate::object::field_get_set::invoke_accessor_getter(acc.get, this);
    Some(f64::from_bits(v.bits()))
}

/// The read of `key` on the POINTER-tagged receiver whose payload is
/// `obj_handle`, after the probe declined it.
///
/// A site key is a property atom, which carries its hash: when the
/// receiver's layout, or the layout of the first prototype that holds the
/// key (every object before it proving the key absent, its overflow
/// included), says the key is an accessor, the getter is called at once. The
/// walk reads what the probe reads (layout facts, the holder's accessor
/// slot: one load where the key index names it, `ACCESSOR_ENTRY`). Every
/// other key takes the complete read.
#[no_mangle]
pub extern "C-unwind" fn js_object_get_field_slow(
    obj_handle: i64,
    key: *const crate::StringHeader,
) -> f64 {
    let obj = obj_handle as usize as *const ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(obj as usize) || key.is_null() {
        return super::ic_miss::get_field_miss(obj, key);
    }
    unsafe {
        let key_bits = crate::value::STRING_TAG | key as u64;
        let verify = crate::object::layout_probe::verify::on();
        if verify {
            crate::object::layout_probe::verify::check(obj_handle as usize, key_bits);
        }
        let atom = crate::string::is_property_atom(key);
        let acc = if atom {
            crate::object::layout_probe::probe_accessor_word(
                obj,
                key_bits,
                crate::string::property_atom_hash(key),
            )
        } else {
            None
        };
        if let Some(acc) = acc {
            if verify {
                crate::object::layout_probe::verify::accessor(obj as usize, key_bits, &acc);
            }
            if crate::object::layout_probe::diag_enabled() {
                crate::object::layout_probe::diag_accessor(key);
            }
            if let Some(value) = call_getter(obj, acc) {
                return value;
            }
        } else if crate::object::layout_probe::diag_enabled() {
            crate::object::layout_probe::diag_explain(obj_handle, key);
        }
        if let Some(value) = crate::object::overflow::site_miss_own_get(obj, key) {
            return f64::from_bits(value.bits());
        }
        if !atom {
            // A key that is no property atom: the walk asks its atom.
            if let Some(acc) = crate::object::layout_probe::probe_accessor(obj, key) {
                if let Some(value) = call_getter(obj, acc) {
                    return value;
                }
            }
        }
    }
    super::ic_miss::get_field_miss(obj, key)
}

/// The complete read of `key` on any receiver value `obj_bits`: the
/// non-pointer tag ladder, else [`js_object_get_field_slow`]. Outlined sites
/// call it after their probe declined.
#[no_mangle]
pub extern "C-unwind" fn js_object_get_field_value(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
) -> f64 {
    let bits = obj_bits as u64;
    if bits >> 48 == crate::value::POINTER_TAG >> 48
        && crate::value::addr_class::is_above_handle_band(
            (bits & crate::value::POINTER_MASK) as usize,
        )
    {
        crate::object::field_rep_store::migrate_on_miss_value(bits);
        return js_object_get_field_slow((bits & crate::value::POINTER_MASK) as i64, key);
    }
    js_object_get_field_nonptr(obj_bits, key, site_id)
}

/// The read of `key` on a receiver that is not a heap object pointer: a heap
/// or short string, an INT32 class reference, `null`/`undefined` (which
/// throw), a small native handle, any other primitive.
#[no_mangle]
pub extern "C-unwind" fn js_object_get_field_nonptr(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
) -> f64 {
    let bits = obj_bits as u64;
    let tag = bits >> 48;
    let obj_unmasked = bits as usize as *const ObjectHeader;
    if tag == crate::value::STRING_TAG >> 48 {
        let masked = (bits & crate::value::POINTER_MASK) as usize as *const ObjectHeader;
        return super::js_object_get_field_by_name_f64(masked, key);
    }
    if tag == crate::value::POINTER_TAG >> 48 {
        // A small native handle (or a Proxy cell) below the heap band.
        let masked = (bits & crate::value::POINTER_MASK) as usize as *const ObjectHeader;
        return super::ic_miss::get_field_miss(masked, key);
    }
    if tag == 0x7FF9 {
        return super::js_object_get_field_by_name_f64(obj_unmasked, key);
    }
    if tag == 0x7FFE {
        return crate::typed_feedback::js_typed_feedback_object_get_field_by_name_f64(
            site_id,
            obj_unmasked,
            key,
        );
    }
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        let is_null = u32::from(bits == crate::value::TAG_NULL);
        let (ptr, len) = unsafe {
            match super::super::has_own_helpers::str_from_string_header(key) {
                Some(s) => (s.as_ptr(), s.len()),
                None => (std::ptr::null(), 0),
            }
        };
        crate::error::js_throw_type_error_property_access(is_null, ptr, len);
    }
    super::js_object_get_field_by_name_f64(obj_unmasked, key)
}

/// The complete keyed read `obj[key]` (a string or symbol key the site does
/// not name) after the probe declined it. `dynamic` is a site whose key may
/// be any value (the by-value operation); otherwise a symbol key reads the
/// symbol property and a string key the by-key operation.
///
/// # Safety
/// `obj_box` and `key` are NaN-boxed values of the running agent.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_object_get_keyed_slow(
    obj_box: f64,
    key: f64,
    site_id: u64,
    dynamic: u32,
) -> f64 {
    crate::object::layout_probe::verify::check(
        (obj_box.to_bits() & crate::value::POINTER_MASK) as usize,
        key.to_bits(),
    );
    let recv = super::runtime_read_site::object_receiver(obj_box);
    if let Some(recv) = recv {
        if let Some(value) = crate::object::overflow::own_get_key(recv, key.to_bits()) {
            return f64::from_bits(value.bits());
        }
    }
    match recv {
        Some(recv) if dynamic == 0 => {
            if crate::symbol::js_is_symbol(key) != 0 {
                crate::symbol::js_object_get_symbol_property(obj_box, key)
            } else {
                crate::object::dynamic_key_read::js_typed_feedback_object_get_field_by_key_f64(
                    site_id, recv, key, obj_box,
                )
            }
        }
        _ => crate::value::js_dyn_index_get(obj_box, key),
    }
}
