//! Compatible B4 contracts while B4c still owns the emitted layout switch.
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
fn detached_bit_and_nested_pin_count_do_not_overlap() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let value = bytes::from_slice(Brand::ArrayBuffer, &[0x25; 1024]);
    let owner = JSValue::from_bits(value.to_bits()).as_pointer::<u8>() as usize;
    let pins: Vec<_> = (0..31).map(|_| bytes::pin(value).unwrap()).collect();
    assert!(!buffer::is_detached_buffer(owner));
    assert!(matches!(bytes::pin(value), Err(bytes::NotBytes::PinLimit)));
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
    }
}

#[test]
fn each_compatible_b4_sabotage_turns_its_witness_red() {
    assert!(!fault("unused"));
    for (fault, witness) in [
        (
            "symbol_header",
            "gc::tests::buffer_b4::persistent_symbols_have_a_leaf_header_at_p_minus_eight",
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
