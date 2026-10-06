//! Unified byte-cell ownership, lifetime and sabotage contracts.
use super::super::*;
use super::support::*;
use crate::buffer::{
    self,
    bytes::{self, Brand, Init},
};
use crate::value::JSValue;

fn bits<T>(p: *const T) -> f64 {
    crate::value::js_nanbox_pointer(p as i64)
}
fn fault(name: &str) -> bool {
    std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some(name)
}

fn collect_between_allocation_and_copy(source: f64) {
    // Inspect liveness before sweeping so the planted missing root fails
    // deterministically without dereferencing reclaimed memory.
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid);
    let owner = JSValue::from_bits(source.to_bits()).as_pointer::<u8>() as usize;
    assert_marked_user_ptr(owner, "copy source before destination collection");
    clear_marks();
    clear_mark_seeds();
    let before = gc_total_collection_count();
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(gc_total_collection_count() > before);
}

#[test]
fn typed_array_copy_roots_source_across_destination_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    let buffer = buffer::buffer_alloc(8);
    assert!(crate::typedarray::typed_array_to_array_buffer(buffer.cast()).is_null());
    let ta = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT32, 4);
    for (index, value) in [37.0, 91.0, -1.0, 1024.0].into_iter().enumerate() {
        crate::typedarray::js_typed_array_set(ta, index as i32, value);
    }
    let copied = bytes::copy_with_collection(
        Brand::ArrayBuffer,
        bits(ta),
        collect_between_allocation_and_copy,
    )
    .unwrap();
    let copied = JSValue::from_bits(copied.to_bits()).as_pointer::<buffer::BufferHeader>();
    assert!(!copied.is_null());
    assert!(buffer::is_array_buffer(copied as usize));
    bytes::no_gc(|scope| {
        let bytes = bytes::bytes(bits(copied), scope).unwrap();
        let expected: Vec<u8> = [37_i32, 91, -1, 1024]
            .into_iter()
            .flat_map(i32::to_ne_bytes)
            .collect();
        assert_eq!(bytes, expected);
    });
}

#[test]
fn ranged_byte_copy_retains_source_and_counts_typed_elements() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    for kind in crate::typedarray::KIND_INT8..=crate::typedarray::KIND_FLOAT16 {
        let scope = RuntimeHandleScope::new();
        let size = crate::typedarray::elem_size_for_kind(kind);
        let source = scope.root_raw_mut_ptr(crate::typedarray::typed_array_alloc(kind, 4));
        let input = bits(source.get_raw_mut_ptr::<crate::typedarray::TypedArrayHeader>());
        bytes::no_gc(|scope| unsafe {
            for (index, byte) in bytes::bytes_mut(input, scope)
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                *byte = (index + 37) as u8;
            }
        });
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        for (offset, length, expected_start, expected_len) in [
            (1.0, 2.0, size, 2 * size),
            (2.0, undefined, 2 * size, 2 * size),
            (3.0, 99.0, 3 * size, size),
            (99.0, undefined, 4 * size, 0),
        ] {
            let out = buffer::js_buffer_copy_bytes_from(input, offset, length);
            let out = scope.root_raw_mut_ptr(out);
            bytes::no_gc(|scope| {
                let actual =
                    bytes::bytes(bits(out.get_raw_mut_ptr::<buffer::BufferHeader>()), scope)
                        .unwrap();
                let expected: Vec<u8> = (expected_start..expected_start + expected_len)
                    .map(|i| (i + 37) as u8)
                    .collect();
                assert_eq!(actual, expected, "raw-byte range for kind {kind}");
            });
        }
    }
    // Leave no caller root for this source: the shared implementation must
    // retain it from allocation through the collecting callback and copy.
    let source = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_UINT32, 4);
    for i in 0..4 {
        crate::typedarray::js_typed_array_set(source, i, (37 + i) as f64);
    }
    let out = bytes::copy_range_with_collection(
        Brand::Buffer,
        bits(source),
        4,
        8,
        collect_between_allocation_and_copy,
    )
    .unwrap();
    bytes::no_gc(|scope| {
        let expected: Vec<u8> = [38_u32, 39]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        assert_eq!(bytes::bytes(out, scope).unwrap(), expected);
    });
}

#[test]
fn typed_lane_copies_preserve_bits_and_retain_their_source() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    for kind in crate::typedarray::KIND_INT8..=crate::typedarray::KIND_FLOAT16 {
        let size = crate::typedarray::elem_size_for_kind(kind);
        let source = crate::typedarray::typed_array_alloc(kind, 4);
        let expected: Vec<u8> = (0..4 * size).map(|i| (i + 37) as u8).collect();
        bytes::no_gc(|scope| unsafe {
            bytes::bytes_mut(bits(source), scope)
                .unwrap()
                .copy_from_slice(&expected);
        });
        let copied = bytes::copy_typed_range_with_collection(
            bits(source),
            1,
            2,
            true,
            collect_between_allocation_and_copy,
        )
        .unwrap();
        let addr = JSValue::from_bits(copied.to_bits()).as_pointer::<u8>() as usize;
        assert_eq!(crate::typedarray::lookup_typed_array_kind(addr), Some(kind));
        bytes::no_gc(|scope| {
            let actual = bytes::bytes(copied, scope).unwrap();
            let expected: Vec<u8> = expected[size..3 * size]
                .chunks_exact(size)
                .rev()
                .flatten()
                .copied()
                .collect();
            assert_eq!(actual, expected, "reverse raw lanes of kind {kind}");
        });
    }
    // Reversing Float64 lanes must preserve a NaN's payload and both zeros.
    let source = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_FLOAT64, 3);
    let expected = [0x7ff8_1234_5678_9abc_u64, 0, 1_u64 << 63];
    bytes::no_gc(|scope| unsafe {
        let data = bytes::bytes_mut(bits(source), scope).unwrap();
        for (slot, value) in data.chunks_exact_mut(8).zip(expected) {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    });
    let reversed = crate::typedarray::js_typed_array_to_reversed(source);
    bytes::no_gc(|scope| {
        let expected: Vec<u8> = expected
            .into_iter()
            .rev()
            .flat_map(u64::to_ne_bytes)
            .collect();
        assert_eq!(bytes::bytes(bits(reversed), scope).unwrap(), expected);
    });
}

extern "C" fn collect_in_typed_find(
    callback: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    candidate: f64,
    index: f64,
    receiver: f64,
) -> f64 {
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid);
    let owner = JSValue::from_bits(receiver.to_bits()).as_pointer::<u8>() as usize;
    assert_marked_user_ptr(owner, "findLast receiver across its callback");
    assert_marked_user_ptr(callback as usize, "findLast callback across its body");
    let candidate = (candidate.to_bits() & crate::value::POINTER_MASK) as usize;
    assert_marked_user_ptr(candidate, "BigInt findLast candidate across its callback");
    clear_marks();
    clear_mark_seeds();
    let before = gc_total_collection_count();
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(gc_total_collection_count() > before);
    f64::from_bits(if index == 1.0 {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

#[test]
fn shared_typed_copies_keep_atomic_lane_width_and_raw_bits() {
    let _guard = CopyingNurseryTestGuard::new(0);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    for kind in crate::typedarray::KIND_INT8..=crate::typedarray::KIND_FLOAT16 {
        let size = crate::typedarray::elem_size_for_kind(kind);
        let backing = buffer::js_shared_array_buffer_new((4 * size) as i32);
        let source =
            crate::typedarray_view::js_typed_array_view(kind as i32, bits(backing), 0.0, 4.0);
        assert!(crate::typedarray::typed_array_has_shared_backing(source));
        let expected: Vec<u8> = (0..4 * size).map(|i| (i + 37) as u8).collect();
        // No worker accesses this fixture while its bytes are initialized.
        bytes::no_gc(|scope| unsafe {
            bytes::bytes_mut(bits(source), scope)
                .unwrap()
                .copy_from_slice(&expected);
        });
        let copied = bytes::copy_typed_range_with_collection(
            bits(source),
            1,
            2,
            true,
            collect_between_allocation_and_copy,
        )
        .unwrap();
        bytes::no_gc(|scope| {
            let expected: Vec<u8> = expected[size..3 * size]
                .chunks_exact(size)
                .rev()
                .flatten()
                .copied()
                .collect();
            assert_eq!(
                bytes::bytes(copied, scope).unwrap(),
                expected,
                "shared kind {kind}"
            );
        });
    }
}

#[test]
fn typed_find_last_retains_receiver_callback_and_bigint_candidate() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    for find_index in [false, true] {
        let (source, callback) = {
            let scope = RuntimeHandleScope::new();
            let source = scope.root_raw_mut_ptr(crate::typedarray::typed_array_alloc(
                crate::typedarray::KIND_BIGUINT64,
                4,
            ));
            for i in 0..4 {
                crate::typedarray::set_bigint_lane_bits(source.get_raw_mut_ptr(), i, 37 + i as u64);
            }
            let callback =
                crate::closure::js_closure_alloc(crate::fn_info!(collect_in_typed_find, 3), 0);
            (source.get_raw_mut_ptr(), callback)
        };
        let before = gc_total_collection_count();
        if find_index {
            assert_eq!(
                crate::typedarray::js_typed_array_find_last_index(source, callback),
                1.0
            );
        } else {
            let value = crate::typedarray::js_typed_array_find_last(source, callback);
            assert!(JSValue::from_bits(value.to_bits()).is_bigint());
            let value = (value.to_bits() & crate::value::POINTER_MASK)
                as *const crate::bigint::BigIntHeader;
            assert_eq!(unsafe { (*value).limbs[0] }, 38);
        }
        assert!(
            gc_total_collection_count() >= before + 3,
            "three collecting predicate calls"
        );
    }
}

#[test]
fn views_observe_owner_resize_and_detach_after_a_live_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _force = ForcedEvacuationTestGuard::on();
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    let options = crate::object::js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"maxByteLength".as_ptr(), 13);
    crate::object::js_object_set_field_by_name(options, key, 16.0);
    let ab = buffer::js_array_buffer_new_with_options(8.0, bits(options));
    let u8 = buffer::js_buffer_slice(ab, 0, 8);
    let i32view = crate::typedarray_view::js_typed_array_view(
        crate::typedarray::KIND_INT32 as i32,
        bits(ab),
        0.0,
        2.0,
    );
    let dv = buffer::js_data_view_new(bits(ab), 0.0, 8.0);
    let values = [bits(u8), bits(i32view), dv];
    for (i, value) in values.into_iter().enumerate() {
        bytes::no_gc(|scope| unsafe {
            bytes::bytes_mut(value, scope).unwrap()[i] = (41 + i) as u8;
        });
    }
    let pin = bytes::pin(bits(ab)).unwrap();
    let young = young_leaf();
    js_shadow_slot_set(0, string_bits(young));
    let before = gc_total_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        gc_total_collection_count() > before,
        "collection must run while bytes are held"
    );
    for value in values {
        bytes::no_gc(|scope| assert_eq!(&bytes::bytes(value, scope).unwrap()[..3], &[41, 42, 43]));
    }
    buffer::array_buffer_resize(ab as usize, &[4.0]);
    for value in values {
        assert!(
            bytes::no_gc(|scope| bytes::bytes(value, scope).is_err()),
            "fixed view must check its resized owner"
        );
    }
    buffer::array_buffer_resize(ab as usize, &[8.0]);
    for value in values {
        bytes::no_gc(|scope| {
            let data = bytes::bytes(value, scope).unwrap();
            assert_eq!(&data[..3], &[41, 42, 43]);
            assert_eq!(&data[4..], &[0; 4]);
        });
    }
    buffer::detach_array_buffer(ab as usize);
    assert!(buffer::is_detached_buffer(ab as usize));
    for value in values {
        assert!(bytes::no_gc(|scope| bytes::bytes(value, scope).is_err()));
    }
    drop(pin);
}

#[test]
fn pinned_inline_detach_retains_pages_until_the_last_unpin() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    let handles = RuntimeHandleScope::new();
    let owner = handles.root_raw_mut_ptr(buffer::buffer_alloc(64 * 1024));
    let owner = owner.get_raw_mut_ptr::<buffer::BufferHeader>();
    unsafe {
        (*owner).length = 64 * 1024;
    }
    buffer::mark_as_array_buffer(owner as usize);
    assert!(!buffer::is_foreign_backed_buffer(owner as usize));
    let first = bytes::pin(bits(owner)).unwrap();
    let second = bytes::pin(bits(owner)).unwrap();
    unsafe {
        std::ptr::write_bytes(first.as_mut_ptr(), 37, first.len());
    }
    buffer::detach_array_buffer(owner as usize);
    assert!(buffer::is_detached_buffer(owner as usize));
    let check = |pin: &bytes::Pinned| unsafe {
        assert!(
            std::slice::from_raw_parts(pin.as_ptr(), pin.len())
                .iter()
                .all(|byte| *byte == 37),
            "detach decommitted pages while native code still holds a pin"
        );
    };
    check(&first);
    drop(first);
    check(&second);
    let data = second.as_ptr();
    drop(second);
    #[cfg(target_os = "linux")]
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let middle = (data as usize + page * 2) & !(page - 1);
        assert_eq!(
            *(middle as *const u8),
            0,
            "last unpin must release detached inline pages"
        );
    }
}

#[test]
fn detached_bit_and_nested_pin_count_do_not_overlap() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let value = bytes::from_slice(Brand::ArrayBuffer, &[0x25; 1024]);
    let owner = JSValue::from_bits(value.to_bits()).as_pointer::<u8>() as usize;
    let pins: Vec<_> = (0..31).map(|_| bytes::pin(value).unwrap()).collect();
    assert!(!buffer::is_detached_buffer(owner));
    let overflow_pin = bytes::pin(value).expect("pin overflow uses the hidden bag");
    drop(overflow_pin);
    buffer::detach_array_buffer(owner);
    assert!(buffer::is_detached_buffer(owner));
    for pin in &pins {
        unsafe {
            assert_eq!(*pin.as_ptr(), 0x25);
        }
    }
    drop(pins);
    assert!(buffer::is_detached_buffer(owner));
}

#[test]
fn large_concat_and_nested_views_preserve_one_visible_window() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _force = ForcedEvacuationTestGuard::on();
    let (nested, owner) = {
        let handles = RuntimeHandleScope::new();
        let array = handles.root_raw_mut_ptr(crate::array::js_array_alloc(300));
        let (part, pin) = bytes::new_bytes(Brand::Buffer, 18_000, Init::Uninit);
        unsafe {
            std::ptr::write_bytes(pin.as_mut_ptr(), 0x25, pin.len());
        }
        for _ in 0..300 {
            crate::array::js_array_push_f64(array.get_raw_mut_ptr(), part);
        }
        let concat = buffer::js_buffer_concat(array.get_raw_mut_ptr());
        let concat = handles.root_raw_mut_ptr(concat);
        let view = buffer::js_buffer_slice(concat.get_raw_mut_ptr(), 4, 5_400_000);
        let view = handles.root_raw_mut_ptr(view);
        let nested =
            handles.root_raw_mut_ptr(buffer::js_buffer_slice(view.get_raw_mut_ptr(), 4, 12));
        assert_eq!(
            buffer::buffer_backing_array_buffer(
                concat.get_raw_mut_ptr::<buffer::BufferHeader>() as usize
            ),
            buffer::buffer_backing_array_buffer(
                nested.get_raw_mut_ptr::<buffer::BufferHeader>() as usize
            )
        );
        let holder = crate::array::js_array_alloc(1);
        crate::array::js_array_push_f64(
            holder,
            bits(nested.get_raw_mut_ptr::<buffer::BufferHeader>()),
        );
        js_shadow_slot_set(0, ptr_bits(holder as usize));
        (
            nested.get_raw_mut_ptr::<buffer::BufferHeader>(),
            concat.get_raw_mut_ptr::<buffer::BufferHeader>() as usize,
        )
    };
    let before = gc_total_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(gc_total_collection_count() > before);
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(
        unsafe { crate::value::addr_class::try_read_tracked_gc_header(owner) }.is_some(),
        "the nested view must retain its concat owner without a separate owner root"
    );
    assert_eq!(
        buffer::js_buffer_read_uint32_be(bits(nested), 0),
        0x25252525_u32 as f64
    );
    bytes::no_gc(|scope| assert_eq!(bytes::bytes(bits(nested), scope).unwrap(), &[0x25; 8]));
}

#[test]
fn persistent_symbols_have_a_leaf_header_at_p_minus_eight() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let key = crate::string::js_string_from_bytes(b"b4-header".as_ptr(), 9);
    let registered =
        unsafe { crate::symbol::js_symbol_for(crate::value::js_nanbox_string(key as i64)) };
    let values = [
        bits(crate::symbol::well_known_symbol("iterator")),
        registered,
        crate::symbol::intl_legacy_constructed_symbol(),
    ];
    for value in values {
        let p = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
        let h = unsafe { &*header_from_trusted_user_ptr(p) };
        assert_eq!(h.obj_type, GC_TYPE_SYMBOL);
        assert_eq!(
            h.size as usize,
            GC_HEADER_SIZE + std::mem::size_of::<crate::symbol::SymbolHeader>()
        );
        assert_ne!(h.gc_flags & GC_FLAG_PINNED, 0);
        assert_eq!(unsafe { crate::symbol::js_is_symbol(value) }, 1);
        assert!(!buffer::is_registered_buffer(p as usize));
        assert!(crate::typedarray::lookup_typed_array_kind(p as usize).is_none());
        assert!(!crate::typedarray::is_offheap_sidetable_alloc(p as usize));
    }
}

#[test]
fn each_compatible_b4_sabotage_turns_its_witness_red() {
    assert!(!fault("unused"));
    for (fault, witness) in [
        ("attach_moves_bytes", "gc::tests::u8_inline_cache::thirty_one_pins_expando_and_buffer_keep_inline_bytes_in_place"),
        ("pin_overflow", "gc::tests::u8_inline_cache::thirty_second_pin_uses_a_hidden_property_and_unpins_cleanly"),
        ("inline_detach_decommit", "gc::tests::buffer_b4::pinned_inline_detach_retains_pages_until_the_last_unpin"),
        (
            "pool_root",
            "gc::tests::buffer_b4::pool_identity_alignment_rollover_and_root_are_real_owner_edges",
        ),
        (
            "pool_identity",
            "gc::tests::buffer_b4::pool_identity_alignment_rollover_and_root_are_real_owner_edges",
        ),
        (
            "shared_lane_copy",
            "gc::tests::buffer_b4::shared_typed_copies_keep_atomic_lane_width_and_raw_bits",
        ),
        (
            "find_receiver_root",
            "gc::tests::buffer_b4::typed_find_last_retains_receiver_callback_and_bigint_candidate",
        ),
        (
            "typed_copy_root",
            "gc::tests::buffer_b4::typed_lane_copies_preserve_bits_and_retain_their_source",
        ),
        (
            "copy_root",
            "gc::tests::buffer_b4::ranged_byte_copy_retains_source_and_counts_typed_elements",
        ),
        (
            "copy_root",
            "gc::tests::buffer_b4::typed_array_copy_roots_source_across_destination_collection",
        ),
        (
            "copy_kind",
            "gc::tests::buffer_b4::typed_array_copy_roots_source_across_destination_collection",
        ),
        (
            "native_resolution",
            "typedarray::resolved_read_tests::native_resolution_uses_the_view_owner_and_offset",
        ),
        (
            "symbol_header",
            "gc::tests::buffer_b4::persistent_symbols_have_a_leaf_header_at_p_minus_eight",
        ),
        (
            "symbol_header",
            "buffer::header_brand_tests::a_persistent_symbol_is_rejected_by_its_header_brand",
        ),
        (
            "detach_mark",
            "gc::tests::buffer_bytes::detach_defers_native_free_until_the_last_pin",
        ),
        (
            "owner_check",
            "gc::tests::buffer_b4::views_observe_owner_resize_and_detach_after_a_live_collection",
        ),
        (
            "transfer_copy",
            "buffer::backing::tests::transfer_receiver_uses_original_pointer_after_sender_gc",
        ),
        (
            "view_edge",
            "gc::tests::buffer_b4::large_concat_and_nested_views_preserve_one_visible_window",
        ),
        (
            "u32_admission",
            "typedarray::tests::owning_u32_admission_reads_current_header",
        ),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", witness, "--nocapture"])
            .env("PERRY_B4_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&child.stdout).contains("running 1 test"),
            "witness must actually run: {witness}"
        );
        assert!(
            !child.status.success(),
            "sabotage {fault} left {witness} green"
        );
        eprintln!("B4 sabotage {fault}: RED");
    }
}

#[test]
fn pool_identity_alignment_rollover_and_root_are_real_owner_edges() {
    let _guard = CopyingNurseryTestGuard::new(0);
    buffer::pool::reset_for_test();
    crate::object::native_module::set_buffer_pool_size(8192.0);
    if !fault("pool_root") {
        gc_register_named_mutable_root_scanner("b4 pool", buffer::pool::scan_pool_roots_mut);
    }
    let first = buffer::pool::copy(3);
    let second = buffer::pool::place(GC_TYPE_BUFFER, buffer::pool::Init::Unsafe, 5);
    let owner = unsafe { buffer::store::owner(first as usize) };
    assert_ne!(owner, first as usize);
    assert_eq!(unsafe { buffer::store::owner(second as usize) }, owner);
    assert_eq!(unsafe { (*first).capacity }, 0);
    assert_eq!(unsafe { (*second).capacity }, 8);
    assert_eq!(buffer::buffer_backing_array_buffer(first as usize), owner);
    assert_eq!(buffer::buffer_backing_array_buffer(second as usize), owner);
    let unpooled = buffer::pool::place(GC_TYPE_BUFFER, buffer::pool::Init::Copy, 4096);
    assert_eq!(
        unsafe { buffer::store::owner(unpooled as usize) },
        unpooled as usize
    );
    let zeroed = buffer::js_buffer_alloc(3, 0);
    assert_eq!(
        unsafe { buffer::store::owner(zeroed as usize) },
        zeroed as usize
    );
    let u8 = buffer::js_uint8array_alloc(3);
    assert_eq!(unsafe { buffer::store::owner(u8 as usize) }, u8 as usize);
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid);
    assert_marked_user_ptr(owner, "pool retained by its per-agent root");
    clear_marks();
    clear_mark_seeds();
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    // No JS view or pin roots the pool at this collection; its own scanner must.
    let next = buffer::pool::copy(3);
    assert_eq!(unsafe { buffer::store::owner(next as usize) }, owner);
    for _ in 0..3 {
        let _ = buffer::pool::copy(3000);
    }
    let last = buffer::pool::copy(3000);
    assert_ne!(unsafe { buffer::store::owner(last as usize) }, owner);
    buffer::pool::reset_for_test();
}
