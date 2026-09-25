//! Own-name / own-descriptor builders shared by the descriptor entry points:
//! native-handle own names, symbol-keyed descriptors, the data/accessor
//! descriptor object builders, and string-primitive index descriptors. Split out
//! of `descriptors.rs` to keep it under the 2,000-line cap (#10750); pure
//! relocation.

use super::*;

/// Build a `{ value, writable, enumerable, configurable }` data descriptor
/// object. Shared by the string-primitive descriptor path (#2818).
/// #6363: the own STRING keys of a native HANDLE, as a NaN-boxed JS array.
///
/// A handle (zlib stream, fetch Headers/Request/Response/Blob, crypto hash, …)
/// is a registry id, not a heap object; its typed surface (`blob.size`) is
/// prototype accessors in Node and is therefore not an own key. What IS an own
/// key is anything the user attached — a plain `handle.foo = v` write or an
/// `Object.defineProperty(handle, …)` — all of which live in the
/// `handle_expando` table. `enumerable_only` selects the `Object.keys` /
/// for-in / spread surface over the `getOwnPropertyNames` one.
pub(crate) unsafe fn handle_own_names_array(hid: i64, enumerable_only: bool) -> f64 {
    let arr = handle_own_names_raw_array(hid, enumerable_only);
    f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000)
}

/// Raw-`ArrayHeader` sibling of [`handle_own_names_array`], for the enumeration
/// paths that build on `*mut ArrayHeader` rather than NaN-boxed values.
pub(crate) unsafe fn handle_own_names_raw_array(
    hid: i64,
    enumerable_only: bool,
) -> *mut crate::array::ArrayHeader {
    let names = crate::object::handle_expando::handle_expando_own_keys(hid, enumerable_only);
    // Exact capacity, so `js_array_push` cannot reallocate under us (the same
    // contract `js_object_get_own_property_names`' own name-array builder relies
    // on a few lines up).
    let arr = crate::array::js_array_alloc(names.len() as u32);
    for name in &names {
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::array::js_array_push(arr, JSValue::string_ptr(s));
    }
    arr
}

/// `[[GetOwnProperty]]` for a SYMBOL key, from the symbol side tables.
///
/// Both tables are keyed by the receiver's NaN-box PAYLOAD
/// (`symbol::obj_key_from_f64`), never by a dereferenced address, so this works
/// unchanged for a heap object and for a native HANDLE id (#6363) — which is why
/// the handle branch in `js_object_get_own_property_descriptor` delegates here
/// instead of re-deriving the lookup.
pub(crate) unsafe fn symbol_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    let owner = crate::symbol::obj_key_from_f64(obj_value);
    let sym_key = crate::symbol::sym_key_from_f64(key_value);
    if sym_key == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Computed Symbol class members live in the class registries rather than
    // the generic per-object Symbol table. They are nevertheless own
    // properties of the constructor/prototype and must reflect as method or
    // accessor descriptors.
    let class_owner = if let Some(cid) = super::class_ref_id(obj_value) {
        Some((cid, super::class_prototype_ref_id(obj_value).is_none()))
    } else if owner != 0 {
        super::class_registry::class_id_for_decl_prototype_object(owner).map(|cid| (cid, false))
    } else {
        None
    };
    if let Some((cid, is_static)) = class_owner {
        let display_name = crate::symbol::symbol_function_name(sym_key);
        if let Some((get, set)) =
            super::class_registry::class_own_symbol_accessor_ptrs(cid, sym_key, is_static)
        {
            return build_accessor_descriptor(
                super::class_registry::class_accessor_function_value(get, false, &display_name),
                super::class_registry::class_accessor_function_value(set, true, &display_name),
                false,
                true,
            );
        }
        if let Some((func_ptr, param_count, has_rest)) =
            super::class_registry::class_own_symbol_method(cid, sym_key, is_static)
        {
            let value = super::build_symbol_bound_method_closure(
                obj_value,
                func_ptr,
                param_count,
                has_rest,
                is_static,
                &display_name,
            );
            return build_data_descriptor(value, true, false, true);
        }
    }
    if owner == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let attrs = crate::symbol::get_symbol_property_attrs(owner, sym_key)
        .unwrap_or(PropertyAttrs::new(true, true, true));
    if let Some((get, set)) = crate::symbol::symbol_accessor_descriptor_bits(owner, sym_key) {
        // A `0` get/set means "absent half" — surface it as `undefined`
        // (not the number `0`) so a get-only accessor reflects
        // `{ get, set: undefined }`.
        let undef = crate::value::TAG_UNDEFINED;
        return build_accessor_descriptor(
            f64::from_bits(if get == 0 { undef } else { get }),
            f64::from_bits(if set == 0 { undef } else { set }),
            attrs.enumerable(),
            attrs.configurable(),
        );
    }
    if let Some(value_bits) = crate::symbol::symbol_property_root_bits(owner, sym_key) {
        return build_data_descriptor(
            f64::from_bits(value_bits),
            attrs.writable(),
            attrs.enumerable(),
            attrs.configurable(),
        );
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) unsafe fn build_data_descriptor(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bf = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let packed = b"value\0writable\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_E5_C0, 4, packed.as_ptr(), packed.len() as u32);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let fields = (desc as *mut u8).add(header_size) as *mut f64;
    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
    *fields = value.get_nanbox_f64();
    *fields.add(1) = bf(writable);
    *fields.add(2) = bf(enumerable);
    *fields.add(3) = bf(configurable);
    super::rebuild_object_field_layout(desc, 4);
    f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
}

pub(crate) unsafe fn build_accessor_descriptor(
    get: f64,
    set: f64,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bf = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    let scope = crate::gc::RuntimeHandleScope::new();
    let get = scope.root_nanbox_f64(get);
    let set = scope.root_nanbox_f64(set);
    let packed = b"get\0set\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let fields = (desc as *mut u8).add(header_size) as *mut f64;
    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
    *fields = get.get_nanbox_f64();
    *fields.add(1) = set.get_nanbox_f64();
    *fields.add(2) = bf(enumerable);
    *fields.add(3) = bf(configurable);
    super::rebuild_object_field_layout(desc, 4);
    f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
}

/// #2818: own-property descriptor for a string primitive receiver. Index keys
/// in range yield the single-char value descriptor (writable:false,
/// enumerable:true, configurable:false); "length" yields the length value
/// descriptor (writable:false, enumerable:false, configurable:false). Any
/// other key is absent → undefined.
pub(super) unsafe fn string_primitive_descriptor(str_value: f64, key_value: f64) -> f64 {
    // #6943: the receiver here is itself a heap value — `str_value` is the
    // boxed/primitive string whose bytes are read below via
    // `str_bytes_from_jsvalue`. It was a raw Rust local across the GC-capable
    // key coercion, so an evacuating collection left it pointing at a
    // forwarding stub and the index/`length` descriptor was computed from
    // moved-out bytes.
    let scope = crate::gc::RuntimeHandleScope::new();
    let str_handle = scope.root_heap_word_u64(str_value.to_bits());
    let key_str = crate::builtins::js_string_coerce(key_value);
    let str_value = f64::from_bits(str_handle.get_heap_word_u64());
    if key_str.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (sptr, sblen) = match crate::string::str_bytes_from_jsvalue(str_value, &mut scratch) {
        Some((p, b)) if !p.is_null() => (p, b),
        _ => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let utf16_len = crate::string::compute_utf16_len(sptr, sblen);

    if name == "length" {
        return build_data_descriptor(utf16_len as f64, false, false, false);
    }

    if let Some(index) = super::canonical_array_index(name) {
        if index < utf16_len {
            // String exotic indices are UTF-16 code units, including lone
            // surrogate halves. Use the same read path as s[index]; `.chars()`
            // counts Unicode scalars and returned the wrong descriptors.
            let string = crate::value::js_get_string_pointer_unified(f64::from_bits(
                str_handle.get_heap_word_u64(),
            )) as *const crate::StringHeader;
            let cstr = crate::string::js_string_char_at(string, index as i32);
            let char_val = f64::from_bits(JSValue::string_ptr(cstr).bits());
            return build_data_descriptor(char_val, false, true, false);
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
