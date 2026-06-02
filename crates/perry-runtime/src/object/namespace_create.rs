//! `js_create_namespace` (Issue #100): build a module-namespace object from
//! parallel key/value arrays. Split out of `object_ops.rs` to keep that
//! module under the file-size gate.

use super::*;

/// Issue #100: build a module-namespace object (the value an `await
/// import("./foo.ts")` resolves to) from parallel arrays of keys and
/// values.
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
) -> f64 {
    let count = if n < 0 { 0 } else { n as usize };
    unsafe {
        // Allocate a plain object with `count` inline slots. class_id 0
        // is the generic-object class used by Object.create / {} / URL.
        let obj = js_object_alloc(0, count as u32);
        if obj.is_null() {
            // Fallback to undefined — should never happen but defensive.
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
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
            let val = *values.add(i);
            js_object_set_field_by_name(obj, key_hdr, val);
        }

        // NaN-box POINTER_TAG and return.
        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

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
            super::super::set_object_keys_array(obj, keys);

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

            // (a) misaligned receiver — would deref `[obj-8]`/`(*obj).keys_array`
            // on garbage without the guard.
            let misaligned_obj = 0x2800_0203usize as *mut ObjectHeader;
            assert!(
                !own_key_present(misaligned_obj, key),
                "misaligned receiver must return false, not crash"
            );

            // (b) aligned real object, but its keys_array points at misaligned
            // garbage — the exact Express crash shape.
            let obj = super::super::alloc::js_object_alloc(0, 4);
            (*obj).keys_array = 0x2800_0203usize as *mut _;
            assert!(
                !own_key_present(obj, key),
                "misaligned keys_array must return false, not crash"
            );
        }
    }
}
