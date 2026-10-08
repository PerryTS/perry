//! Routing must refuse internal GC cells without creating native handle roots.
use super::*;

fn assert_refused(owner: usize) {
    // Release builds return absence; debug builds also diagnose the bad caller.
    let edit = std::panic::catch_unwind(|| HolderEdit::new(owner).is_none());
    assert!(
        edit.unwrap_or(cfg!(debug_assertions)),
        "install admitted an internal cell"
    );
    let read = std::panic::catch_unwind(|| unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(owner);
        bag.is_null()
    });
    assert!(
        read.unwrap_or(cfg!(debug_assertions)),
        "query admitted an internal cell"
    );
    assert!(
        super::super::handle_expando::handle_property_bag(owner as i64).is_null(),
        "refusal must not create an address-keyed native root"
    );
}

#[test]
fn descriptor_holder_refusal_never_roots_internal_gc_cells() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let capture = crate::r#box::js_box_alloc(1.0) as usize;
    let native =
        crate::native_handle::js_native_handle_new_borrowed(0, 1, 1, 0, std::ptr::null(), 0);
    let map = crate::weakref::js_weakmap_new();
    let key = crate::object::js_object_alloc(0, 0);
    crate::weakref::js_weakmap_set(
        crate::value::js_nanbox_pointer(map as i64),
        crate::value::js_nanbox_pointer(key as i64),
        2.0,
    );
    let weak = unsafe { crate::weakref::storage::owned_storage(map) } as usize;
    assert_ne!(weak, 0, "the internal storage must exist");
    for owner in [
        capture,
        weak,
        crate::value::js_nanbox_get_pointer(native) as usize,
    ] {
        assert_refused(owner);
    }
}

#[test]
fn descriptor_holder_refusal_reads_ignore_a_native_registry_collision() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let capture = crate::r#box::js_box_alloc(1.0) as usize;
    // Plant exactly the obsolete address-keyed state. A query must refuse
    // even when the native registry has a bag at this GC cell's address.
    unsafe { super::super::handle_expando::handle_property_bag_ensure(capture as i64) };
    let result = std::panic::catch_unwind(|| unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(capture);
        bag.is_null()
    });
    super::super::handle_expando::handle_expando_clear(capture as i64);
    assert!(
        result.unwrap_or(cfg!(debug_assertions)),
        "query used an address-keyed native bag"
    );
}

#[test]
fn forwarded_object_descriptor_holder_resolves_reads_and_writes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let alias = crate::object::js_object_alloc(0, 0);
        let middle = crate::object::js_object_alloc(0, 0);
        let live = crate::object::js_object_alloc(0, 0);
        let alias_header = crate::gc::header_from_trusted_user_ptr(alias.cast()).cast_mut();
        let middle_header = crate::gc::header_from_trusted_user_ptr(middle.cast()).cast_mut();
        let alias_flags = (*alias_header).gc_flags;
        let middle_flags = (*middle_header).gc_flags;
        let alias_payload = *alias.cast::<u64>();
        let middle_payload = *middle.cast::<u64>();
        crate::gc::set_forwarding_address(alias_header, middle.cast());
        crate::gc::set_forwarding_address(middle_header, live.cast());
        set_property_attrs(
            alias as usize,
            "x".into(),
            PropertyAttrs::new(false, true, true),
        );
        for owner in [alias, middle, live] {
            let DescriptorRoute::Keys(bag) = descriptor_route(owner as usize);
            assert_eq!(bag, live as *const ObjectHeader);
            assert!(!get_property_attrs(owner as usize, "x").unwrap().writable());
            assert!(super::super::handle_expando::handle_property_bag(owner as i64).is_null());
        }
        let edit = HolderEdit::new(alias as usize).expect("alias normalizes to live holder");
        assert_eq!(edit.bag, live);
        drop(edit);
        *alias.cast::<u64>() = alias_payload;
        *middle.cast::<u64>() = middle_payload;
        (*alias_header).gc_flags = alias_flags;
        (*middle_header).gc_flags = middle_flags;
    }
}

#[test]
fn native_fetch_band_descriptor_holder_uses_the_stable_registry_owner() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let handle = 0x4_2428i64;
    unsafe {
        let descriptor = crate::object::js_object_alloc(0, 3);
        for (name, value) in [
            ("value", 42.0),
            ("enumerable", f64::from_bits(crate::value::TAG_TRUE)),
            ("configurable", f64::from_bits(crate::value::TAG_TRUE)),
        ] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(descriptor, key, value);
        }
        let key = crate::string::js_string_from_bytes(b"lane12201".as_ptr(), 9);
        crate::object::js_object_define_property(
            crate::value::js_nanbox_pointer(handle),
            crate::value::js_nanbox_string(key as i64),
            crate::value::js_nanbox_pointer(descriptor as i64),
        );
        assert_eq!(
            super::super::handle_expando::handle_expando_data_get(handle, "lane12201"),
            Some(42.0)
        );
        assert!(!get_property_attrs(handle as usize, "lane12201")
            .unwrap()
            .writable());
        super::super::handle_expando::handle_expando_clear(handle);
        assert!(
            descriptor_holder(handle as usize).is_null(),
            "release drops the holder edge"
        );
    }
}

#[test]
fn lazy_array_descriptor_holder_is_the_materialized_arrays_bag() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let input = b"[1,2,3]";
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(tape, 0, 3, text)
    })
    .expect("valid array tape");
    unsafe {
        let DescriptorRoute::Keys(before) = descriptor_route(lazy as usize);
        assert!(before.is_null());
        assert!((*lazy).materialized.is_null(), "reads must not materialize");
        set_property_attrs(
            lazy as usize,
            "tag".into(),
            PropertyAttrs::new(false, true, true),
        );
        assert!(
            !(*lazy).materialized.is_null(),
            "installs normalize to an ordinary array"
        );
        let DescriptorRoute::Keys(bag) = descriptor_route(lazy as usize);
        assert_eq!(
            bag,
            crate::array::array_property_bag((*lazy).materialized) as *const ObjectHeader
        );
        assert!(!get_property_attrs(lazy as usize, "tag").unwrap().writable());
        assert!(super::super::handle_expando::handle_property_bag(lazy as i64).is_null());
    }
}
