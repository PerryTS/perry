//! Annex B accessor methods (`__defineGetter__`/`__lookupSetter__` etc.) and
//! the raw own-field read `js_object_get_own_field_or_undef`.
use super::super::*;
use super::*;
// Disambiguate the two in-scope `value_is_callable` globs (object_ops'
// `descriptor_helpers` via `super::*` and `object`'s `instanceof` via
// `super::super::*`): this module wants the `descriptor_helpers` (unsafe)
// version, matching the pre-split object_ops-local resolution.
use super::descriptor_helpers::value_is_callable;

/// `Object.prototype.__defineGetter__(key, getter)` (Annex B §B.2.2.2).
/// Installs an accessor with the given getter and `enumerable: true,
/// configurable: true`. A non-callable getter throws a TypeError. Returns
/// `undefined`.
#[no_mangle]
pub extern "C" fn js_object_define_getter(this: f64, key: f64, getter: f64) -> f64 {
    unsafe { define_accessor_annexb(this, key, getter, true) }
}

/// `Object.prototype.__defineSetter__(key, setter)` (Annex B §B.2.2.3).
#[no_mangle]
pub extern "C" fn js_object_define_setter(this: f64, key: f64, setter: f64) -> f64 {
    unsafe { define_accessor_annexb(this, key, setter, false) }
}

/// Shared `__defineGetter__`/`__defineSetter__` body. Builds an accessor
/// descriptor `{ [get|set]: func, enumerable: true, configurable: true }` and
/// delegates to `js_object_define_property`, so the function's `this`-binding
/// and the closure/class-ref/symbol-key paths all behave like a normal
/// accessor define.
unsafe fn define_accessor_annexb(this: f64, key: f64, func: f64, is_getter: bool) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if !value_is_callable(func) {
        let which = if is_getter {
            "__defineGetter__"
        } else {
            "__defineSetter__"
        };
        throw_object_type_error(format!("Object.prototype.{which}: Expecting function").as_bytes());
    }
    let desc = js_object_alloc(0, 3);
    if desc.is_null() {
        return undef;
    }
    let field = if is_getter { "get" } else { "set" };
    let fkey = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    js_object_set_field_by_name(desc, fkey, func);
    let true_v = f64::from_bits(crate::value::JSValue::bool(true).bits());
    let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
    js_object_set_field_by_name(desc, enum_key, true_v);
    let cfg_key = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
    js_object_set_field_by_name(desc, cfg_key, true_v);
    let desc_val = f64::from_bits(crate::value::JSValue::pointer(desc as *const u8).bits());
    js_object_define_property(this, key, desc_val);
    undef
}

/// `Object.prototype.__lookupGetter__(key)` (Annex B §B.2.2.4). Walks the
/// receiver's own + prototype chain; returns the getter of the first own
/// accessor property found (or `undefined`).
#[no_mangle]
pub extern "C" fn js_object_lookup_getter(this: f64, key: f64) -> f64 {
    unsafe { lookup_accessor_annexb(this, key, true) }
}

/// `Object.prototype.__lookupSetter__(key)` (Annex B §B.2.2.5).
#[no_mangle]
pub extern "C" fn js_object_lookup_setter(this: f64, key: f64) -> f64 {
    unsafe { lookup_accessor_annexb(this, key, false) }
}

/// Shared `__lookupGetter__`/`__lookupSetter__` body. Walks own + proto chain
/// via `getOwnPropertyDescriptor`/`getPrototypeOf`; the first own property
/// found stops the walk — its `get`/`set` field is returned (`undefined` for a
/// data property or the opposite-only accessor case).
unsafe fn lookup_accessor_annexb(this: f64, key: f64, want_getter: bool) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let field = if want_getter { "get" } else { "set" };
    let fkey = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let mut cur = this;
    // Cap the walk so a pathological/cyclic prototype can't spin forever.
    for _ in 0..100_000 {
        let jv = crate::value::JSValue::from_bits(cur.to_bits());
        if jv.is_null() || jv.is_undefined() {
            return undef;
        }
        let desc = js_object_get_own_property_descriptor(cur, key);
        if !crate::value::JSValue::from_bits(desc.to_bits()).is_undefined() {
            let desc_ptr = extract_obj_ptr(desc);
            if desc_ptr.is_null() {
                return undef;
            }
            let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, fkey);
            return f64::from_bits(v.bits());
        }
        cur = js_object_get_prototype_of(cur);
    }
    undef
}

/// Issue #620: returns the OWN-property value at `name` if one exists in the
/// receiver's own keys_array (a string-keyed data property), otherwise
/// returns TAG_UNDEFINED. Used by class-method dispatch to detect override
/// patterns like `this.method = X` (hono's SmartRouter.match rebinds itself
/// on first call). Distinct from `js_object_get_field_by_name` because it
/// does NOT walk the class vtable's getter chain — we only want a raw own
/// data-property read, not a side-effecting getter invocation.
#[no_mangle]
pub extern "C" fn js_object_get_own_field_or_undef(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    const TAG_UNDEF: u64 = 0x7FFC_0000_0000_0001;
    if name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        // Reject anything in the native / Web-Fetch small-handle band (see
        // `value::addr_class`). Headers/Request/Response/Blob and node:http
        // handles are NaN-boxed POINTER_TAG values holding a small registry
        // id, not heap object pointers. The old `< 0x10000` floor let a
        // Headers handle (first id = 0x40000) through; this fn then
        // dereferenced `[handle - GC_HEADER_SIZE]` as a GcHeader and
        // segfaulted. macOS's `is_valid_obj_ptr` floor (0x200_0000_0000)
        // masked this, but on Linux/Android/iOS the floor is 0x1000, so the
        // bad deref reached.
        if !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
            return f64::from_bits(TAG_UNDEF);
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return f64::from_bits(TAG_UNDEF);
        }
        // Skip closures sharing the GC_TYPE_OBJECT slot (CLOSURE_MAGIC at +12).
        let type_tag_at_12 =
            *((obj as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
            return f64::from_bits(TAG_UNDEF);
        }
        let key_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_count = crate::array::js_array_length(keys) as usize;
        if key_count > 65536 {
            return f64::from_bits(TAG_UNDEF);
        }
        let alloc_limit = std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: SSO-aware match by byte slice — the
            // own-property-or-undef path was the route through which
            // hono's `c.req.X` dispatch decided to invoke the vtable
            // getter, and pre-fix a SSO-stored `X` was invisible here.
            if crate::string::js_string_key_matches_bytes(key_val, key_bytes) {
                let val = if i < alloc_limit {
                    js_object_get_field(obj, i as u32)
                } else {
                    match overflow_get(obj as usize, i) {
                        Some(bits) => crate::JSValue::from_bits(bits),
                        None => return f64::from_bits(TAG_UNDEF),
                    }
                };
                return f64::from_bits(val.bits());
            }
        }
        f64::from_bits(TAG_UNDEF)
    }
}
