//! B4 cell admission and lifetime witnesses; no address cache exists.
use super::super::*;
use super::support::*;
use crate::buffer::{self, bytes};

fn full_gc() {
    let before = gc_total_collection_count();
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert!(gc_total_collection_count() > before);
}

#[test]
fn all_byte_brands_have_the_common_cell_and_one_header() {
    let _guard = GcTestIsolationGuard::new();
    for kind in 0..12 {
        let p = crate::typedarray::typed_array_alloc(kind, 3);
        let h = unsafe { buffer::store::header(p as usize) };
        assert_eq!(
            unsafe { (*h).obj_type },
            crate::typedarray::type_for_kind(kind)
        );
        assert_eq!(unsafe { (*p).length }, 3);
        assert_eq!(unsafe { (*p).link }, 0);
        assert!(!gc_type_is_movable(unsafe { (*h).obj_type }));
        assert_eq!(
            bytes::no_gc(
                |_| bytes::span(crate::value::js_nanbox_pointer(p as i64), false)
                    .unwrap()
                    .ptr as usize
            ),
            p as usize + crate::codegen_abi::BYTES_STORE
        );
    }
    assert_eq!(crate::codegen_abi::BYTES_STORE, 16);
}

#[test]
fn foreign_and_view_headers_resolve_the_real_window() {
    let _guard = GcTestIsolationGuard::new();
    let mut data = [3; 16];
    let owner = buffer::header::buffer_alloc_foreign(data.as_mut_ptr(), 16);
    buffer::mark_as_uint8array(owner as usize);
    let view = buffer::js_buffer_slice(owner, 3, 9);
    assert_eq!(unsafe { (*view).link }, owner as usize);
    assert_eq!(unsafe { (*view).capacity }, 3);
    bytes::no_gc(|_| {
        assert_eq!(
            bytes::span(crate::value::js_nanbox_pointer(view as i64), false)
                .unwrap()
                .ptr as *const u8,
            unsafe { data.as_ptr().add(3) }
        )
    });
    buffer::js_buffer_set(view, 0, 91);
    assert_eq!(data[3], 91);
    buffer::detach_array_buffer(owner as usize);
    assert_eq!(buffer::js_buffer_length(view), 0);
}

#[test]
fn rebranding_has_no_stale_address_admission() {
    let _guard = GcTestIsolationGuard::new();
    let owner = buffer::js_buffer_alloc(8, 29);
    assert_eq!(buffer::admitted_u8_read(owner as usize, 0), Some(29));
    buffer::mark_as_array_buffer(owner as usize);
    assert_eq!(buffer::admitted_u8_read(owner as usize, 0), None);
    buffer::mark_as_uint8array(owner as usize);
    assert_eq!(buffer::admitted_u8_read(owner as usize, 0), Some(29));
}

#[test]
fn bagged_view_keeps_owner_and_properties_across_full_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let owner = buffer::js_buffer_alloc(64, 29);
    let view = buffer::js_buffer_slice(owner, 7, 19);
    buffer::buffer_set_own_prop(view as usize, "tag", 7.0);
    let bag = unsafe { buffer::store::bag(view as usize) };
    assert!(!bag.is_null());
    assert_eq!(
        unsafe { buffer::store::owner(view as usize) },
        owner as usize
    );
    assert_eq!(unsafe { (*view).link }, bag as usize);
    js_shadow_slot_set(0, ptr_bits(view as usize));
    full_gc();
    assert!(
        unsafe { crate::value::addr_class::try_read_tracked_gc_header(owner as usize) }.is_some()
    );
    assert_eq!(buffer::js_buffer_get(view, 0), 29);
    assert_eq!(buffer::buffer_get_own_prop(view as usize, "tag"), Some(7.0));
}

#[test]
fn thirty_one_pins_expando_and_buffer_keep_inline_bytes_in_place() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_named_mutable_root_scanner(
        "b4 pinned",
        crate::gc::pin::scan_pinned_object_roots_mut,
    );
    let owner = buffer::js_buffer_alloc(64, 29);
    let value = crate::value::js_nanbox_pointer(owner as i64);
    let pins: Vec<_> = (0..31).map(|_| bytes::pin(value).unwrap()).collect();
    let before = pins[0].as_ptr();
    buffer::buffer_set_own_prop(owner as usize, "tag", 7.0);
    let ab = buffer::buffer_backing_array_buffer(owner as usize);
    bytes::no_gc(|_| {
        assert_eq!(bytes::span(value, false).unwrap().ptr as *const u8, before);
        assert_eq!(
            bytes::span(crate::value::js_nanbox_pointer(ab as i64), false)
                .unwrap()
                .ptr as *const u8,
            before
        );
    });
    assert_eq!(buffer::js_buffer_get(owner, 0), 29);
    full_gc();
    for pin in &pins {
        assert_eq!(pin.as_ptr(), before);
        assert_eq!(unsafe { *pin.as_ptr() }, 29);
    }
    drop(pins);
    assert_eq!(
        unsafe { (*buffer::store::header(owner as usize))._reserved & 0x3e00 },
        0
    );
}

#[test]
fn thirty_second_pin_uses_a_hidden_property_and_unpins_cleanly() {
    let _guard = CopyingNurseryTestGuard::new(0);
    gc_register_named_mutable_root_scanner(
        "b4 pinned",
        crate::gc::pin::scan_pinned_object_roots_mut,
    );
    let owner = buffer::js_buffer_alloc(64, 29);
    let value = crate::value::js_nanbox_pointer(owner as i64);
    let mut pins: Vec<_> = (0..32).map(|_| bytes::pin(value).unwrap()).collect();
    assert_eq!(
        unsafe { buffer::store::bag_get(owner as usize, buffer::store::PIN_OVERFLOW_KEY) },
        Some(1.0)
    );
    assert!(buffer::buffer_own_prop_names(owner as usize).is_empty());
    drop(pins.pop());
    assert_eq!(
        unsafe { buffer::store::bag_get(owner as usize, buffer::store::PIN_OVERFLOW_KEY) },
        Some(0.0)
    );
    drop(pins);
    assert_eq!(
        unsafe { (*buffer::store::header(owner as usize))._reserved & 0x3e00 },
        0
    );
}

#[test]
fn agent_sab_metadata_never_writes_the_process_store_header() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_named_mutable_root_scanner(
        "b4 pinned",
        crate::gc::pin::scan_pinned_object_roots_mut,
    );
    let owner = crate::shared_sab::alloc_shared_sab(16);
    let block = crate::shared_sab::shared_store_owner(owner as usize).unwrap();
    let header = unsafe { buffer::store::header(block) };
    let before = unsafe { *(header as *const u64) };
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let pin = bytes::pin(crate::value::js_nanbox_pointer(owner as i64)).unwrap();
    buffer::buffer_set_own_prop(owner as usize, "tag", 7.0);
    full_gc();
    assert_eq!(unsafe { *(header as *const u64) }, before);
    assert_eq!(
        crate::shared_sab::shared_store_owner(owner as usize),
        Some(block)
    );
    drop(pin);
}
