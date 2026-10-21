//! `js_create_namespace` (Issue #100): build a module-namespace object from
//! parallel key/value arrays. Split out of `object_ops.rs` to keep that
//! module under the file-size gate.

use super::*;

pub(crate) const MODULE_NAMESPACE_CLASS_ID: u32 = 0xFFFF_4E53;

/// #9889: Implement the module namespace exotic object's [[GetOwnProperty]]
/// result while retaining accessors as the internal representation of live
/// bindings. The getter can allocate and evacuate both the namespace and key,
/// so keep them rooted until it returns. `build_data_descriptor` roots the
/// resulting value before allocating the descriptor object.
pub(crate) unsafe fn module_namespace_own_property_descriptor(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value = obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
        key_handle.with_const_ptr::<crate::StringHeader, _>(|current_key| {
            js_object_get_field_by_name(current_obj, current_key)
        })
    });
    super::descriptors::build_data_descriptor(f64::from_bits(value.bits()), true, true, false)
}

/// Apply the host-defined invariants shared by static and dynamic module
/// namespace objects.
#[no_mangle]
pub extern "C" fn js_finalize_namespace(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return value;
    }
    let obj = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null()
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !crate::object::is_valid_obj_ptr(obj as *const u8)
    {
        return value;
    }
    unsafe {
        (*obj).class_id = MODULE_NAMESPACE_CLASS_ID;
        // A module namespace exotic object has a [[Prototype]] of `null`
        // (ECMA-262 10.4.6), and `OBJ_FLAG_NULL_PROTO` is the proof
        // `Object.getPrototypeOf` already reads to answer `null`. Without it a
        // namespace reached the self-prototype fallback in
        // `object_ops/prototype.rs`, which answers with a further namespace
        // object, so a prototype-chain walk over an `import * as ns` binding
        // never terminated. That is the same non-termination the fallback's
        // `NATIVE_MODULE_CLASS_ID` arm already guards against for
        // `require("path")` namespaces; a static ESM namespace reaches this
        // function instead, and had no such guard.
        let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
        crate::object::shapes::restamp_object_proto_id(obj);
    }
    crate::object::js_object_prevent_extensions(value)
}

/// Issue #100: build a module-namespace object (the value an `await
/// import("./foo.ts")` resolves to) from parallel arrays of keys and
/// values. Entries whose parallel `live_flags` byte is non-zero carry a
/// zero-argument getter closure instead of a snapshot value.
///
/// Keys are length-prefixed UTF-8 (Perry strings are not guaranteed
/// null-terminated), passed as parallel `*const *const u8` (data
/// pointers) and `*const i32` (byte lengths). Values are the already
/// NaN-boxed `f64` representations passed as a flat `f64` array.
///
/// The returned f64 is a NaN-boxed POINTER_TAG `ObjectHeader` with its
/// `keys_array` populated so `Object.keys(ns)`/iteration and property
/// dispatch work the same as on any other JS object. Caller is
/// responsible for pinning the object as a GC root if it stores the
/// result in a long-lived slot — codegen does this by writing the
/// result into the module-scoped `__perry_ns_<prefix>` global which is
/// already registered with `js_gc_register_global_root`.
///
/// Empty namespace (`n == 0`) returns a fresh empty object.
///
/// Returns an `f64` directly (not `JSValue`) so the LLVM ABI signature
/// `double js_create_namespace(...)` declared in `runtime_decls.rs`
/// matches: NaN-boxed values use float-register-return on AArch64 /
/// SysV-x86_64. A `JSValue` return would route through integer
/// registers (`#[repr(transparent)]` over `u64`) and the call site's
/// `%xmm0` read would observe stale bits.
#[no_mangle]
pub extern "C" fn js_create_namespace(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    js_finalize_namespace(build_namespace_object(
        n, keys, key_lens, values, live_flags,
    ))
}

/// The namespace of a CommonJS module under Bun's interop: the static
/// entries (the lexer-detected names, `default`, `module.exports`), plus
/// every other own enumerable string key of the evaluated `module.exports`
/// object. Bun exposes all of them on `import * as ns`; the static lexer
/// cannot see names that only exist at run time (a UMD factory's return
/// value, `module.exports = make()`), so they are read here, once, after
/// the module body ran. Values are a snapshot, like Node's detected names.
/// `default` already holds the exports object and `__esModule` is interop
/// metadata, so neither is copied.
#[no_mangle]
pub extern "C" fn js_create_cjs_namespace(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    let ns = build_namespace_object(n, keys, key_lens, values, live_flags);
    js_finalize_namespace(js_namespace_adopt_cjs_exports(ns))
}

/// The consumer-side twin of [`js_create_cjs_namespace`]: an `import * as ns`
/// of a CommonJS module read as a whole value is materialized from the
/// importer's member list, then takes the run-time `module.exports` keys here
/// before it is finalized. Returns the (possibly relocated) namespace.
#[no_mangle]
pub extern "C" fn js_namespace_adopt_cjs_exports(ns: f64) -> f64 {
    unsafe { adopt_cjs_exports(ns) }
}

/// Copy the own enumerable string keys of `ns.default` (the evaluated
/// `module.exports`) that `ns` does not already own. `ns` is not finalized
/// yet, so it is still extensible. Everything that can allocate keeps the
/// namespace, the copy, and the current key rooted.
unsafe fn adopt_cjs_exports(ns: f64) -> f64 {
    let ns_v = JSValue::from_bits(ns.to_bits());
    if !ns_v.is_pointer() {
        return ns;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ns_h = scope.root_raw_mut_ptr(ns_v.as_pointer::<ObjectHeader>() as *mut ObjectHeader);
    let default_key = crate::string::js_string_from_bytes(b"default".as_ptr(), 7);
    let exports = ns_h
        .with_mut_ptr::<ObjectHeader, _>(|obj| js_object_get_field_by_name_f64(obj, default_key));
    let current = |h: &crate::gc::RuntimeHandle<'_>| {
        h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64))
    };
    if !JSValue::from_bits(exports.to_bits()).is_reference() {
        return current(&ns_h);
    }
    let exports_h = scope.root_nanbox_f64(exports);
    // CopyDataProperties of the exports value (objects, functions and
    // proxies alike) into a plain object, then move the missing keys over.
    let staging_obj = js_object_alloc(0, 0);
    let staging_h = scope.root_raw_mut_ptr(staging_obj);
    let staging_v =
        staging_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
    super::assign::js_object_literal_spread(staging_v, exports_h.get_nanbox_f64());
    let staging_v =
        staging_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
    let names = js_object_keys_value(staging_v);
    let names_h = scope.root_raw_mut_ptr(names);
    let len = crate::array::js_array_length(names);
    for i in 0..len {
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key_v = names_h
            .with_mut_ptr::<crate::array::ArrayHeader, _>(|a| crate::array::js_array_get_f64(a, i));
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_v) as *const crate::StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        match super::has_own_helpers::str_from_string_header(key_ptr) {
            Some("default") | Some("__esModule") | None => continue,
            Some(_) => {}
        }
        let key_h = iter_scope.root_string_ptr(key_ptr);
        let ns_value =
            ns_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
        let key_value = key_h
            .with_const_ptr::<crate::StringHeader, _>(|k| crate::value::js_nanbox_string(k as i64));
        if crate::value::js_is_truthy(js_object_has_own(ns_value, key_value)) != 0 {
            continue;
        }
        let value = staging_h.with_mut_ptr::<ObjectHeader, _>(|o| {
            key_h
                .with_const_ptr::<crate::StringHeader, _>(|k| js_object_get_field_by_name_f64(o, k))
        });
        let value_h = iter_scope.root_nanbox_f64(value);
        ns_h.with_mut_ptr::<ObjectHeader, _>(|o| {
            key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                js_object_set_field_by_name(o, k, value_h.get_nanbox_f64())
            })
        });
    }
    current(&ns_h)
}

fn build_namespace_object(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    let count = if n < 0 { 0 } else { n as usize };
    unsafe {
        // Export values arrive in a caller stack buffer, which is not part of
        // the runtime root set. Namespace construction allocates the object,
        // keys array, and key strings; retain every value across those moving
        // collections so CJS default/shared object identity is preserved.
        let raw_values = if count == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(values, count).to_vec()
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let value_handles = scope.root_nanbox_f64_slice(&raw_values);
        // Allocate a plain object with `count` inline slots. class_id 0
        // is the generic-object class used by Object.create / {} / URL.
        let obj = js_object_alloc(0, count as u32);
        if obj.is_null() {
            // Fallback to undefined — should never happen but defensive.
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let obj_handle = scope.root_raw_mut_ptr(obj);

        // Initialize an empty keys array so `js_object_set_field_by_name`
        // can append to it. Pre-populating the keys array AND calling
        // set_field_by_name would double every key — the property
        // setter's "add key to keys_array" step runs unconditionally.
        let keys_arr = crate::array::js_array_alloc(0);
        let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        js_object_set_keys(obj, keys_arr);

        // Set each (key, value) pair on the object. We route through
        // `js_object_set_field_by_name` so the standard property-write
        // path (inline-slot allocation, shape transitions, accessor
        // dispatch) handles everything. This matches how user-written
        // `obj.k = v` and `js_object_assign_one` populate objects, so
        // downstream reads (PropertyGet PIC, Object.keys, JSON.stringify)
        // all work without special-casing the namespace shape.
        for i in 0..count {
            let key_data = *keys.add(i);
            let key_len = *key_lens.add(i);
            let key_len_u = if key_len < 0 { 0u32 } else { key_len as u32 };
            // Use the heap StringHeader path so the property machinery
            // (which expects a real `StringHeader*`) gets a valid
            // pointer. Pre-SSO-only would crash on >7-byte export names.
            let key_hdr = crate::string::js_string_from_bytes(key_data, key_len_u);
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            let val = value_handles[i].get_nanbox_f64();
            if !live_flags.is_null() && *live_flags.add(i) != 0 {
                let key_handle = scope.root_string_ptr(key_hdr);
                obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
                    key_handle.with_const_ptr::<crate::StringHeader, _>(|current_key| {
                        js_object_define_accessor(
                            crate::value::js_nanbox_pointer(current_obj as i64),
                            crate::value::js_nanbox_string(current_key as i64),
                            val,
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                    });
                });
                let key = String::from_utf8_lossy(std::slice::from_raw_parts(
                    key_data,
                    key_len_u as usize,
                ))
                .into_owned();
                obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
                    set_property_attrs(
                        current_obj as usize,
                        key,
                        PropertyAttrs::new(false, true, false),
                    );
                });
            } else {
                js_object_set_field_by_name(obj, key_hdr, val);
            }
        }

        // NaN-box POINTER_TAG and return.
        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

/// A module namespace born in its compiler-owned final layout: `desc` is the
/// module's namespace descriptor (`perry-codegen` `layout_desc::
/// namespace_layout`): its export names in sorted order (ECMA-262 10.4.6,
/// [[OwnPropertyKeys]]), slot = position, each key's attributes (a live
/// binding is an accessor whose getter reads the binding; every other export
/// a data property), then `@@toStringTag`, a `null` [[Prototype]], and born
/// non-extensible. So the
/// object carries its final shape from its first instruction: no key-add,
/// attribute, prototype or integrity transition, and no shape is minted.
///
/// `values` holds one NaN-boxed value per key, in key order: the getter
/// closure of an accessor key, the value of a data key, and last the
/// `@@toStringTag` value "Module".
///
/// # Safety
/// `desc` is a registered namespace descriptor of this program and `values`
/// points at `desc.key_count` values.
#[no_mangle]
pub unsafe extern "C" fn js_create_namespace_layout(
    desc: *const super::layout_desc::PerryLayoutDesc,
    values: *const f64,
) -> f64 {
    use super::key_attrs::ENTRY_ACCESSOR;
    let d = &*desc;
    let count = d.key_count as usize;
    let keys = if count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(d.keys, count)
    };
    if super::layout_desc::register_namespace_layout(desc).is_none() {
        // This runtime could not register the layout: the same namespace,
        // built key by key from the descriptor's own names and attributes.
        return namespace_from_unregistered_layout(keys, values);
    }
    // The values arrive in a caller stack buffer the collector does not
    // scan, and each accessor's pair allocation can collect: root them all,
    // then each pair, before the object exists.
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw: Vec<f64> = if count == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(values, count).to_vec()
    };
    let value_handles = scope.root_nanbox_f64_slice(&raw);
    let mut words = Vec::with_capacity(count);
    for (key, value) in keys.iter().zip(value_handles.iter()) {
        if key.attrs & ENTRY_ACCESSOR != 0 {
            let pair = super::accessor_pair::pair_new(super::accessor_pair::Accessor {
                get: value.get_nanbox_u64(),
                ..Default::default()
            });
            words.push(Some(scope.root_nanbox_u64(
                crate::value::js_nanbox_pointer(pair as i64).to_bits(),
            )));
        } else {
            words.push(None);
        }
    }
    // Nothing below collects: the birth and its slot stores are one step.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let obj = super::layout_desc::perry_alloc_layout(desc);
    (*obj).class_id = MODULE_NAMESPACE_CLASS_ID;
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    let old = crate::arena::pointer_in_old_gen(obj as usize);
    for (i, key) in keys.iter().enumerate() {
        let bits = match &words[i] {
            Some(pair) => pair.get_nanbox_u64(),
            None => value_handles[i].get_nanbox_u64(),
        };
        let slot = fields.add(key.slot as usize);
        // GC_STORE_AUDIT(INIT): a declared slot of the unpublished birth.
        *slot = bits;
        if old {
            crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, bits);
        }
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

/// [`js_create_namespace_layout`] for a descriptor this runtime did not
/// register: the generic per-key construction, from the descriptor's export
/// names (its string atoms) and accessor bits.
#[cold]
unsafe fn namespace_from_unregistered_layout(
    keys: &[super::layout_desc::PerryLayoutKey],
    values: *const f64,
) -> f64 {
    let mut names = Vec::with_capacity(keys.len());
    let mut lens = Vec::with_capacity(keys.len());
    let mut live = Vec::with_capacity(keys.len());
    let mut ordered = Vec::with_capacity(keys.len());
    debug_assert!(keys.iter().enumerate().all(|(i, k)| k.slot as usize == i));
    for (i, key) in keys.iter().enumerate() {
        // `@@toStringTag` is the generic construction's own business.
        if (*key.atom).kind != crate::codegen_abi::ATOM_KIND_STRING {
            continue;
        }
        let s = (*key.atom).string;
        names.push(crate::string::string_data(s));
        lens.push((*s).byte_len as i32);
        live.push(u8::from(key.attrs & super::key_attrs::ENTRY_ACCESSOR != 0));
        ordered.push(*values.add(i));
    }
    js_create_namespace(
        names.len() as i32,
        names.as_ptr(),
        lens.as_ptr(),
        ordered.as_ptr(),
        live.as_ptr(),
    )
}

// The namespace descriptor's integrity byte is the runtime's flag.
const _: () = assert!(
    crate::codegen_abi::LAYOUT_INTEGRITY_NON_EXTENSIBLE as u16 == crate::gc::OBJ_FLAG_NO_EXTEND
);

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    #[test]
    fn get_own_property_names_array_and_string_no_crash() {
        // Regression: getOwnPropertyNames on an array/string read a bogus
        // keys_array off the wrong header and segfaulted. Now returns the
        // index names + "length".
        let arr = crate::array::js_array_alloc(4);
        for v in [10.0, 20.0, 30.0] {
            crate::array::js_array_push_f64(arr, v);
        }
        let arr_val = crate::value::js_nanbox_pointer(arr as i64);
        let names = js_object_get_own_property_names(arr_val);
        let names_ptr =
            (names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        // 3 indices + "length".
        assert_eq!(crate::array::js_array_length(names_ptr), 4);

        let s = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
        let s_val = crate::value::js_nanbox_string(s as i64);
        let s_names = js_object_get_own_property_names(s_val);
        let s_ptr = (s_names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(s_ptr), 3); // "0","1","length"
    }

    /// #1781: `Object.is` content-compares strings only when both sides pass
    /// `is_string()` (STRING_TAG-only). Two SSO operands match via the
    /// bit-pattern fallback, but a mixed SSO/heap pair with equal content
    /// (e.g. a JSON-parsed value vs a heap literal) did not — `Object.is`
    /// wrongly returned false. Now representation-independent.
    #[test]
    fn object_is_compares_sso_and_mixed_strings() {
        let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
        let a = JSValue::try_short_string(b"abc").unwrap();
        let b = JSValue::try_short_string(b"abc").unwrap();
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(b.bits())
            )),
            "two equal SSO strings"
        );

        let heap = JSValue::string_ptr(crate::string::js_string_from_bytes(b"abc".as_ptr(), 3));
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(heap.bits())
            )),
            "mixed SSO/heap, equal content"
        );

        let c = JSValue::try_short_string(b"xyz").unwrap();
        assert!(
            !truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(c.bits())
            )),
            "different content"
        );
    }

    /// #1781: an object with an inline-SSO key must answer
    /// `hasOwnProperty("id")` truthfully. Pre-fix the
    /// `is_string()`-gated keys-array iteration in `own_key_present`
    /// skipped the SSO key silently and the call returned false.
    #[test]
    fn own_key_present_finds_sso_stored_key() {
        unsafe {
            let obj = super::super::alloc::js_object_alloc(0, 4);
            // Build a keys array with a single SSO-tagged key directly
            // (skipping `js_object_set_field_by_name`, which would
            // intern the key to heap and bypass the SSO blind spot
            // we're regression-testing).
            let keys = crate::array::js_array_alloc(4);
            let sso = JSValue::try_short_string(b"id").expect("SSO");
            crate::array::js_array_push_f64(keys, f64::from_bits(sso.bits()));
            super::super::set_object_keys(obj, super::super::ObjectKeys::owned(keys));

            let incoming = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            assert!(
                own_key_present(obj, incoming),
                "SSO key 'id' should be visible to own_key_present"
            );

            let incoming_other = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
            assert!(
                !own_key_present(obj, incoming_other),
                "absent key 'tag' must not match"
            );
        }
    }

    /// #3527: a non-object value reaching `own_key_present` must return
    /// `false`, never SIGBUS. Two shapes seen compiling Express: (a) a
    /// misaligned receiver pointer, and (b) a real-looking object whose
    /// `keys_array` field holds misaligned garbage (the value read from a
    /// native-module namespace sentinel mis-treated as an object). Both are
    /// rejected by the low-3-bits alignment guard — every genuine GC pointer
    /// is `align.max(8)`-aligned.
    #[test]
    fn own_key_present_rejects_misaligned_pointers() {
        unsafe {
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);

            // (a) misaligned receiver — would deref `[obj-8]`/`crate::object::object_keys_array(obj)`
            // on garbage without the guard.
            let misaligned_obj = 0x2800_0203usize as *mut ObjectHeader;
            assert!(
                !own_key_present(misaligned_obj, key),
                "misaligned receiver must return false, not crash"
            );
        }
    }
}
