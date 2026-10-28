//! Acceptance of fixed record births, including undefined-valued properties and GC edges.
use super::runtime_layouts::*;
use super::*;

#[test]
fn static_layout_records_are_present_without_an_inline_floor() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let descriptors = [
        std::ptr::addr_of!(RECORD_V026),
        std::ptr::addr_of!(RECORD_F0_0),
        std::ptr::addr_of!(RECORD_F3_0),
        std::ptr::addr_of!(RECORD_F6_0),
        std::ptr::addr_of!(RECORD_F7_0),
        std::ptr::addr_of!(RECORD_F13_0),
        std::ptr::addr_of!(RECORD_F17_0),
        std::ptr::addr_of!(RECORD_F20_0),
        std::ptr::addr_of!(RECORD_F21_0),
        std::ptr::addr_of!(RECORD_F28_0),
        std::ptr::addr_of!(RECORD_F29_0),
        std::ptr::addr_of!(RECORD_F30_0),
        std::ptr::addr_of!(RECORD_F31_0),
        std::ptr::addr_of!(RECORD_F37_0),
        std::ptr::addr_of!(RECORD_F327_0),
        std::ptr::addr_of!(RECORD_F328_0),
        std::ptr::addr_of!(RECORD_F411_0),
        std::ptr::addr_of!(RECORD_F412_0),
        std::ptr::addr_of!(RECORD_F413_0),
        std::ptr::addr_of!(RECORD_F414_0),
        std::ptr::addr_of!(RECORD_F418_0),
        std::ptr::addr_of!(RECORD_F426_0),
        std::ptr::addr_of!(RECORD_F427_0),
        std::ptr::addr_of!(RECORD_F429_0),
        std::ptr::addr_of!(RECORD_F430_0),
        std::ptr::addr_of!(RECORD_F434_0),
        std::ptr::addr_of!(RECORD_F470_0),
        std::ptr::addr_of!(RECORD_F471_0),
        std::ptr::addr_of!(RECORD_F473_0),
        std::ptr::addr_of!(RECORD_F483_0),
        std::ptr::addr_of!(RECORD_F487_0),
        std::ptr::addr_of!(RECORD_F568_0),
        std::ptr::addr_of!(RECORD_PCHECKED031),
        std::ptr::addr_of!(RECORD_U610),
        std::ptr::addr_of!(RECORD_U613),
        std::ptr::addr_of!(RECORD_Q435_0),
        std::ptr::addr_of!(RECORD_Q436_0),
        std::ptr::addr_of!(RECORD_Q439_0),
        std::ptr::addr_of!(RECORD_Q440_0),
        std::ptr::addr_of!(RECORD_Q441_0),
        std::ptr::addr_of!(RECORD_Q443_0),
        std::ptr::addr_of!(RECORD_Q448_0),
        std::ptr::addr_of!(RECORD_Q449_0),
        std::ptr::addr_of!(RECORD_Q450_0),
        std::ptr::addr_of!(RECORD_Q451_0),
        std::ptr::addr_of!(RECORD_Q452_0),
        std::ptr::addr_of!(RECORD_Q453_0),
        std::ptr::addr_of!(RECORD_Q454_0),
        std::ptr::addr_of!(RECORD_Q455_0),
        std::ptr::addr_of!(RECORD_Q456_0),
        std::ptr::addr_of!(RECORD_Q457_0),
        std::ptr::addr_of!(RECORD_Q458_0),
        std::ptr::addr_of!(RECORD_Q459_0),
        std::ptr::addr_of!(RECORD_Q460_0),
        std::ptr::addr_of!(RECORD_Q461_0),
        std::ptr::addr_of!(RECORD_Q462_0),
        std::ptr::addr_of!(RECORD_Q463_0),
        std::ptr::addr_of!(RECORD_Q464_0),
        std::ptr::addr_of!(RECORD_Q465_0),
        std::ptr::addr_of!(RECORD_Q466_0),
        std::ptr::addr_of!(RECORD_Q467_0),
        std::ptr::addr_of!(RECORD_Q468_0),
        std::ptr::addr_of!(RECORD_Q468_1),
        std::ptr::addr_of!(RECORD_Q468_2),
        std::ptr::addr_of!(RECORD_Q468_3),
    ];
    for desc in descriptors {
        let obj = scope.root_raw_mut_ptr(unsafe { perry_alloc_runtime_record(desc) });
        let pointer = obj.get_raw_mut_ptr::<ObjectHeader>();
        unsafe {
            assert_eq!((*pointer).parent_class_id, (*desc).layout_id);
            let gc = crate::gc::header_from_trusted_user_ptr(pointer.cast());
            assert_eq!(
                (*gc).size as usize,
                crate::gc::GC_HEADER_SIZE
                    + std::mem::size_of::<ObjectHeader>()
                    + (*desc).slot_count as usize * 8
            );
            let fields = pointer
                .cast::<u8>()
                .add(std::mem::size_of::<ObjectHeader>())
                .cast::<u64>();
            for slot in 0..(*desc).slot_count as usize {
                assert_eq!(*fields.add(slot), crate::value::TAG_UNDEFINED,
                    "every declared record key must be present even when no caller value is assigned");
            }
        }
    }
}

#[test]
fn static_layout_path_records_keep_data_and_expand_after_gc() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let input =
        scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(b"/a/b.txt".as_ptr(), 8));
    let obj = scope.root_raw_mut_ptr(crate::path::js_path_posix_parse(input.get_raw_const_ptr()));
    let ptr = obj.get_raw_mut_ptr::<ObjectHeader>();
    unsafe {
        let desc = std::ptr::addr_of!(RECORD_F418_0);
        assert_eq!((*ptr).parent_class_id, (*desc).layout_id);
        let gc = crate::gc::header_from_trusted_user_ptr(ptr.cast());
        assert_eq!(
            (*gc).size as usize,
            crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ObjectHeader>() + 40,
            "path records must allocate exactly their declared five slots"
        );
    }
    let key = scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(b"extra".as_ptr(), 5));
    js_object_set_field_by_name(obj.get_raw_mut_ptr(), key.get_raw_const_ptr(), 19.0);
    crate::gc::js_gc_collect();
    assert_eq!(
        js_object_get_field_by_name_f64(obj.get_raw_const_ptr(), key.get_raw_const_ptr()),
        19.0
    );
    let base_key = scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(b"base".as_ptr(), 4));
    let base =
        js_object_get_field_by_name_f64(obj.get_raw_const_ptr(), base_key.get_raw_const_ptr());
    assert_eq!(crate::fs::validate::read_js_string_pub(base), "b.txt");
}

#[test]
fn static_layout_image_results_keep_dimensions_after_gc() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise =
        scope.root_raw_mut_ptr(
            perry_runtime_image_result(7, 10.0, 20.0) as *mut crate::promise::Promise
        );
    let result =
        scope.root_nanbox_f64(crate::promise::js_promise_result(promise.get_raw_mut_ptr()));
    crate::gc::js_gc_collect();
    let pointer =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
    assert_eq!(js_object_get_field(pointer, 1).bits(), 10.0f64.to_bits());
    assert_eq!(js_object_get_field(pointer, 2).bits(), 20.0f64.to_bits());
}

#[test]
fn static_layout_node_timing_snapshot_keeps_managed_values_after_gc() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let timing = scope.root_nanbox_f64(crate::perf_hook_slots::js_perf_node_timing());
    let name = scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(
        b"snapshot-name".as_ptr(),
        13,
    ));
    let source = crate::value::js_nanbox_get_pointer(timing.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field(
        source,
        0,
        crate::JSValue::string_ptr(name.get_raw_mut_ptr()),
    );
    let result = scope.root_nanbox_f64(unsafe {
        crate::perf_hook_slots::node_timing_to_json(timing.get_nanbox_f64())
    });
    crate::gc::js_gc_collect();
    let pointer =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
    unsafe {
        assert_eq!(
            (*pointer).parent_class_id,
            (*std::ptr::addr_of!(RECORD_F429_0)).layout_id
        );
    }
    let name = js_object_get_field(pointer, 0);
    assert_eq!(
        crate::fs::validate::read_js_string_pub(f64::from_bits(name.bits())),
        "snapshot-name"
    );
}

#[test]
#[ignore = "descriptor audit: run explicitly with --nocapture"]
fn static_layout_misc_dump_registered_contracts() {
    crate::gc::initialize_runtime_fixture();
    let descriptors = [
        std::ptr::addr_of!(RECORD_V026),
        std::ptr::addr_of!(RECORD_F0_0),
        std::ptr::addr_of!(RECORD_F3_0),
        std::ptr::addr_of!(RECORD_F6_0),
        std::ptr::addr_of!(RECORD_F7_0),
        std::ptr::addr_of!(RECORD_F13_0),
        std::ptr::addr_of!(RECORD_F17_0),
        std::ptr::addr_of!(RECORD_F20_0),
        std::ptr::addr_of!(RECORD_F21_0),
        std::ptr::addr_of!(RECORD_F28_0),
        std::ptr::addr_of!(RECORD_F29_0),
        std::ptr::addr_of!(RECORD_F30_0),
        std::ptr::addr_of!(RECORD_F31_0),
        std::ptr::addr_of!(RECORD_F37_0),
        std::ptr::addr_of!(RECORD_F327_0),
        std::ptr::addr_of!(RECORD_F328_0),
        std::ptr::addr_of!(RECORD_F411_0),
        std::ptr::addr_of!(RECORD_F412_0),
        std::ptr::addr_of!(RECORD_F413_0),
        std::ptr::addr_of!(RECORD_F414_0),
        std::ptr::addr_of!(RECORD_F418_0),
        std::ptr::addr_of!(RECORD_F426_0),
        std::ptr::addr_of!(RECORD_F427_0),
        std::ptr::addr_of!(RECORD_F429_0),
        std::ptr::addr_of!(RECORD_F430_0),
        std::ptr::addr_of!(RECORD_F434_0),
        std::ptr::addr_of!(RECORD_F470_0),
        std::ptr::addr_of!(RECORD_F471_0),
        std::ptr::addr_of!(RECORD_F473_0),
        std::ptr::addr_of!(RECORD_F483_0),
        std::ptr::addr_of!(RECORD_F487_0),
        std::ptr::addr_of!(RECORD_F568_0),
        std::ptr::addr_of!(RECORD_PCHECKED031),
        std::ptr::addr_of!(RECORD_U610),
        std::ptr::addr_of!(RECORD_U613),
        std::ptr::addr_of!(RECORD_Q435_0),
        std::ptr::addr_of!(RECORD_Q436_0),
        std::ptr::addr_of!(RECORD_Q439_0),
        std::ptr::addr_of!(RECORD_Q440_0),
        std::ptr::addr_of!(RECORD_Q441_0),
        std::ptr::addr_of!(RECORD_Q443_0),
        std::ptr::addr_of!(RECORD_Q448_0),
        std::ptr::addr_of!(RECORD_Q449_0),
        std::ptr::addr_of!(RECORD_Q450_0),
        std::ptr::addr_of!(RECORD_Q451_0),
        std::ptr::addr_of!(RECORD_Q452_0),
        std::ptr::addr_of!(RECORD_Q453_0),
        std::ptr::addr_of!(RECORD_Q454_0),
        std::ptr::addr_of!(RECORD_Q455_0),
        std::ptr::addr_of!(RECORD_Q456_0),
        std::ptr::addr_of!(RECORD_Q457_0),
        std::ptr::addr_of!(RECORD_Q458_0),
        std::ptr::addr_of!(RECORD_Q459_0),
        std::ptr::addr_of!(RECORD_Q460_0),
        std::ptr::addr_of!(RECORD_Q461_0),
        std::ptr::addr_of!(RECORD_Q462_0),
        std::ptr::addr_of!(RECORD_Q463_0),
        std::ptr::addr_of!(RECORD_Q464_0),
        std::ptr::addr_of!(RECORD_Q465_0),
        std::ptr::addr_of!(RECORD_Q466_0),
        std::ptr::addr_of!(RECORD_Q467_0),
        std::ptr::addr_of!(RECORD_Q468_0),
        std::ptr::addr_of!(RECORD_Q468_1),
        std::ptr::addr_of!(RECORD_Q468_2),
        std::ptr::addr_of!(RECORD_Q468_3),
    ];

    for desc in descriptors {
        unsafe {
            let d = &*desc;
            let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
            let index = std::slice::from_raw_parts(d.index, d.index_mask as usize + 1);
            eprint!(
                "LAYOUT {} {} {} {} {} {} {} {} {} {} {:?}",
                d.family,
                d.variant,
                d.kind,
                d.integrity,
                d.slot_count,
                d.layout_id,
                d.rep,
                d.proto.addr(),
                d.key_count,
                d.index_mask,
                index
            );
            for key in keys {
                let atom = &*key.atom;
                let string = &*atom.string;
                let bytes = std::slice::from_raw_parts(
                    crate::string::string_data(atom.string),
                    string.byte_len as usize,
                );
                eprint!(
                    " | {:?}:{}:{}:{}:{:?}",
                    std::str::from_utf8(bytes).unwrap(),
                    key.slot,
                    key.attrs,
                    atom.hash,
                    key._pad
                );
            }
            eprintln!();
        }
    }
}

#[test]
fn static_layout_record_buffer_keeps_values_across_collection() {
    crate::gc::initialize_runtime_fixture();
    super::runtime_layouts::runtime_record!(record, RECORD_F418_0);
    let bytes = vec![b'x'; 8192];
    record.set(
        0,
        crate::JSValue::string_ptr(crate::string::js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        )),
    );
    // The value lives only in a rooted stack cell while the record is unborn.
    crate::gc::js_gc_collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(record.finish());
    crate::gc::js_gc_collect();
    let value = js_object_get_field(object.get_raw_mut_ptr(), 0);
    let string = crate::value::js_get_string_pointer_unified(f64::from_bits(value.bits()))
        as *const crate::StringHeader;
    unsafe {
        assert_eq!((*string).byte_len, bytes.len() as u32);
        assert_eq!(
            std::slice::from_raw_parts(crate::string::string_data(string), bytes.len()),
            bytes
        );
        assert_eq!(
            (*object.get_raw_mut_ptr::<ObjectHeader>()).parent_class_id,
            (*std::ptr::addr_of!(RECORD_F418_0)).layout_id
        );
    }
    for slot in 1..5 {
        assert_eq!(
            js_object_get_field(object.get_raw_mut_ptr(), slot).bits(),
            crate::value::TAG_UNDEFINED
        );
    }
}
