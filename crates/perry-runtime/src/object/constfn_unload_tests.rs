//! Linux executable gate: info and code really live in a dlopen/dlclose image.
use super::*;

#[test]
fn constfn_refuses_real_unloadable_image_before_dlclose() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let dir = std::env::temp_dir().join(format!("perry-constfn-unload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("image.c");
    let image = dir.join("image.so");
    std::fs::write(&source, format!(
        "#include <stdint.h>\nunsigned char image_info[{}] __attribute__((aligned(8)));\ndouble image_method(void *closure, uint64_t receiver) {{ return 73.0; }}\n",
        std::mem::size_of::<crate::closure::JsFunctionInfo>(),
    )).unwrap();
    let result = std::process::Command::new("cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&image)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture image build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = std::ffi::CString::new(image.to_str().unwrap()).unwrap();
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        assert!(!handle.is_null(), "fixture image must actually load");
        let code = libc::dlsym(handle, c"image_method".as_ptr());
        let info =
            libc::dlsym(handle, c"image_info".as_ptr()) as *mut crate::closure::JsFunctionInfo;
        assert!(!code.is_null() && !info.is_null());
        // The info itself lives in the image's writable BSS, then stays
        // immutable while any live closure can use it. It lacks permanence.
        // GC_STORE_AUDIT(POINTER_FREE): native image metadata contains code
        // addresses and scalar flags, with no managed-heap edge.
        info.write(crate::closure::JsFunctionInfo::from_code(
            code as *const u8,
            0,
        ));
        let c = crate::closure::js_closure_alloc(info, 0);
        assert_eq!(
            crate::closure::js_closure_call0(
                c as *const crate::closure::ClosureHeader,
                crate::closure::JsThis::UNDEFINED
            ),
            73.0,
            "loaded-image body control"
        );
        let obj = crate::object::js_object_alloc(0, 4);
        let name = crate::string::js_string_from_bytes(b"unload_cf".as_ptr(), 9);
        crate::object::js_object_set_field_by_name(
            obj,
            name,
            f64::from_bits(crate::JSValue::object_ptr(c.cast()).bits()),
        );
        let old = shapes::object_shape_stamp(obj);
        assert_eq!(
            shapes::shape_descriptor_by_id(old)
                .unwrap()
                .special_constfn_mask,
            0,
            "generic key-add must exclude unloadable info"
        );
        let entries = [ConstFnStaticEntry { slot: 0, info }];
        assert!(parse_constfn_static_entries(entries.as_ptr(), 1).is_none());
        let final_obj = js_object_finalize_constfn_static(
            obj as usize as u64,
            0,
            b"unload_cf\0".as_ptr(),
            10,
            1,
            1,
            0,
            3,
            entries.as_ptr(),
            1,
        );
        assert_eq!(
            shapes::object_shape_stamp(final_obj as usize as *mut _),
            old
        );
        assert!(shapes::shape_descriptor_by_id(old)
            .unwrap()
            .constfn_infos()
            .is_empty());
        // Revoke the only caller and its borrowed info before unmapping it.
        crate::object::js_object_set_field_by_name(
            obj,
            name,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        (*(c as *mut crate::closure::ClosureHeader)).info = std::ptr::null();
        assert_eq!(
            libc::dlclose(handle),
            0,
            "fixture image must actually unload"
        );
        assert!(
            shapes::shape_descriptor_by_id(shapes::object_shape_stamp(obj))
                .unwrap()
                .constfn_infos()
                .is_empty(),
            "no shape-owned image address may survive unload"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}
