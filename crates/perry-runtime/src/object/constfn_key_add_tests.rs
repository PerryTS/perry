//! Positive cache reuse and refusals for ordered ConstFn key-add publication.
use super::*;
use crate::{closure, gc, object, value};

extern "C" fn capture_body(c: *const closure::ClosureHeader, _this: closure::JsThis) -> f64 {
    f64::from_bits(closure::js_closure_get_capture_bits(c, 0))
}
extern "C" fn other_body(_c: *const closure::ClosureHeader, _this: closure::JsThis) -> f64 {
    99.0
}
fn info() -> *const closure::JsFunctionInfo {
    crate::fn_info!(capture_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
}
unsafe fn bits(c: *mut closure::ClosureHeader) -> u64 {
    value::js_nanbox_pointer(c as i64).to_bits()
}
unsafe fn slot(obj: *mut ObjectHeader) -> *mut closure::ClosureHeader {
    object::js_object_get_field(obj, 0).as_pointer::<closure::ClosureHeader>() as *mut _
}
unsafe fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

#[test]
fn cached_constfn_key_add_uses_current_factory_captures() {
    let _lock = gc::global_side_table_test_lock();
    let _no_move = gc::GcSuppressScope::new();
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let key = scope.root_raw_mut_ptr(key("cached_cf_factory_method"));
        let a = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        let b = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        closure::js_closure_set_capture_f64(a.get_raw_mut_ptr(), 0, 31.0);
        closure::js_closure_set_capture_f64(b.get_raw_mut_ptr(), 0, 47.0);
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        object::js_object_set_field_by_name(
            first.get_raw_mut_ptr(),
            key.get_raw_mut_ptr(),
            f64::from_bits(bits(a.get_raw_mut_ptr())),
        );
        let target = shapes::object_shape_stamp(first.get_raw_mut_ptr());
        assert_eq!(
            shapes::shape_descriptor_by_id(target)
                .unwrap()
                .special_constfn_mask,
            1
        );
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        shapes::test_watch_cached_transition_stamps(
            second.get_raw_mut_ptr::<ObjectHeader>() as usize
        );
        object::js_object_set_field_by_name(
            second.get_raw_mut_ptr(),
            key.get_raw_mut_ptr(),
            f64::from_bits(bits(b.get_raw_mut_ptr())),
        );
        assert_eq!(
            shapes::test_cached_transition_stamps(),
            1,
            "must install cached Any intermediate"
        );
        shapes::test_reset_cached_transition_stamps();
        assert_eq!(shapes::object_shape_stamp(second.get_raw_mut_ptr()), target);
        assert_eq!(slot(second.get_raw_mut_ptr()), b.get_raw_mut_ptr());
        assert_eq!(
            capture_body(slot(first.get_raw_mut_ptr()), closure::JsThis::UNDEFINED),
            31.0
        );
        assert_eq!(
            capture_body(slot(second.get_raw_mut_ptr()), closure::JsThis::UNDEFINED),
            47.0
        );
    }
}

#[test]
fn cached_constfn_key_add_refuses_wrong_body_unsafe_this_and_deprecation() {
    let _lock = gc::global_side_table_test_lock();
    let _no_move = gc::GcSuppressScope::new();
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let key = scope.root_raw_mut_ptr(key("cached_cf_refused_method"));
        let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        let pred = shapes::object_shape_stamp(first.get_raw_mut_ptr());
        object::js_object_set_field_by_name(
            first.get_raw_mut_ptr(),
            key.get_raw_mut_ptr(),
            f64::from_bits(bits(closure.get_raw_mut_ptr())),
        );
        let hit =
            object::transition_cache_lookup(pred, key.get_raw_mut_ptr()).expect("cached body edge");
        let receiver = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        assert_eq!(shapes::object_shape_stamp(receiver.get_raw_mut_ptr()), pred);
        let wrong = scope.root_raw_mut_ptr(closure::js_closure_alloc(
            crate::fn_info!(other_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
            0,
        ));
        let unsafe_this = scope.root_raw_mut_ptr(closure::js_closure_alloc(
            info(),
            closure::CAPTURES_THIS_FLAG,
        ));
        let unloadable = scope.root_raw_mut_ptr(closure::js_closure_alloc(
            crate::fn_info!(capture_body, 0),
            1,
        ));
        for value in [
            bits(wrong.get_raw_mut_ptr()),
            bits(unsafe_this.get_raw_mut_ptr()),
            bits(unloadable.get_raw_mut_ptr()),
            9.0f64.to_bits(),
            value::TAG_UNDEFINED,
        ] {
            assert!(admit_or_store(receiver.get_raw_mut_ptr(), pred, hit, value).is_none());
            assert_eq!(shapes::object_shape_stamp(receiver.get_raw_mut_ptr()), pred);
            assert_eq!(
                object::js_object_get_field(receiver.get_raw_mut_ptr(), 0).bits(),
                value::TAG_UNDEFINED
            );
        }
        assert!(shapes::shape_record_by_id(hit.2)
            .unwrap()
            .deprecate_special_to_any(hit.1));
        assert!(admit_or_store(
            receiver.get_raw_mut_ptr(),
            pred,
            hit,
            bits(closure.get_raw_mut_ptr())
        )
        .is_none());
        assert_eq!(shapes::object_shape_stamp(receiver.get_raw_mut_ptr()), pred);
        assert_eq!(
            object::js_object_get_field(receiver.get_raw_mut_ptr(), 0).bits(),
            value::TAG_UNDEFINED
        );
    }
}

#[test]
fn cached_constfn_key_add_moves_receiver_and_current_closure() {
    moving_roundtrip("cfmove1", 1);
}

#[test]
fn cached_constfn_key_add_long_key_safely_falls_back_after_relocation() {
    moving_roundtrip("cached_cf_long_moving_method", 0);
}

fn moving_roundtrip(method_key: &str, expected_cached_stamps: u64) {
    let _guard = gc::CopyingNurseryTestGuard::new(0);
    let _triggers = gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _forced = gc::knob_overrides::ForcedEvacuationTestGuard::on();
    gc::register_runtime_handle_root_scanner_for_tests();
    gc::gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    gc::gc_register_mutable_root_scanner(object::scan_object_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(object::scan_shape_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(object::scan_transition_cache_roots_mut);
    gc::gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    let previous =
        gc::set_conservative_stack_scan_override(Some(gc::ConservativeStackScanMode::Disabled));
    struct Restore(Option<gc::ConservativeStackScanMode>);
    impl Drop for Restore {
        fn drop(&mut self) {
            gc::set_conservative_stack_scan_override(self.0);
        }
    }
    let _restore = Restore(previous);
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let key = scope.root_raw_mut_ptr(key(method_key));
        let a = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        let b = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        closure::js_closure_set_capture_f64(a.get_raw_mut_ptr(), 0, 17.0);
        closure::js_closure_set_capture_f64(b.get_raw_mut_ptr(), 0, 29.0);
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        object::js_object_set_field_by_name(
            first.get_raw_mut_ptr(),
            key.get_raw_mut_ptr(),
            f64::from_bits(bits(a.get_raw_mut_ptr())),
        );
        let target = shapes::object_shape_stamp(first.get_raw_mut_ptr());
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        let original_receiver = second.get_raw_mut_ptr::<ObjectHeader>() as usize;
        let original_closure = b.get_raw_mut_ptr::<closure::ClosureHeader>() as usize;
        assert!(crate::arena::pointer_in_nursery(original_receiver));
        assert!(crate::arena::pointer_in_nursery(original_closure));
        gc::gc_collect_minor();
        assert_ne!(
            second.get_raw_mut_ptr::<ObjectHeader>() as usize,
            original_receiver,
            "pre-store receiver must move"
        );
        assert_ne!(
            b.get_raw_mut_ptr::<closure::ClosureHeader>() as usize,
            original_closure,
            "incoming closure root must refresh"
        );
        assert_eq!(
            field_rep_store::object_slot_rep(second.get_raw_mut_ptr(), 0),
            field_rep::REP_ANY
        );
        assert_eq!(
            object::js_object_get_field(second.get_raw_mut_ptr(), 0).bits(),
            value::TAG_UNDEFINED
        );
        shapes::test_watch_cached_transition_stamps(
            second.get_raw_mut_ptr::<ObjectHeader>() as usize
        );
        object::js_object_set_field_by_name(
            second.get_raw_mut_ptr(),
            key.get_raw_mut_ptr(),
            f64::from_bits(bits(b.get_raw_mut_ptr())),
        );
        assert_eq!(
            shapes::test_cached_transition_stamps(),
            expected_cached_stamps,
            "content keys reuse the cache; relocated pointer keys safely miss"
        );
        shapes::test_reset_cached_transition_stamps();
        let before_receiver = second.get_raw_mut_ptr::<ObjectHeader>() as usize;
        let before_closure = slot(second.get_raw_mut_ptr()) as usize;
        field_rep_store::assert_field_rep_lanes(
            second.get_raw_mut_ptr(),
            shapes::object_shape_record(second.get_raw_mut_ptr()),
            1,
        );
        gc::gc_collect_minor();
        field_rep_store::assert_field_rep_lanes(
            second.get_raw_mut_ptr(),
            shapes::object_shape_record(second.get_raw_mut_ptr()),
            1,
        );
        assert_ne!(
            second.get_raw_mut_ptr::<ObjectHeader>() as usize,
            before_receiver,
            "post-store receiver must move"
        );
        assert_ne!(
            slot(second.get_raw_mut_ptr()) as usize,
            before_closure,
            "SPECIAL current closure slot must rewrite"
        );
        assert_eq!(shapes::object_shape_stamp(second.get_raw_mut_ptr()), target);
        assert_eq!(
            capture_body(slot(first.get_raw_mut_ptr()), closure::JsThis::UNDEFINED),
            17.0
        );
        assert_eq!(
            capture_body(slot(second.get_raw_mut_ptr()), closure::JsThis::UNDEFINED),
            29.0
        );
    }
}

#[test]
fn cached_constfn_key_add_preserves_preceding_f64_lane() {
    let _lock = gc::global_side_table_test_lock();
    let _no_move = gc::GcSuppressScope::new();
    let scope = gc::RuntimeHandleScope::new();
    unsafe {
        let number_key = scope.root_raw_mut_ptr(key("cfnum001"));
        let method_key = scope.root_raw_mut_ptr(key("cfmethod"));
        let a = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        let b = scope.root_raw_mut_ptr(closure::js_closure_alloc(info(), 1));
        closure::js_closure_set_capture_f64(a.get_raw_mut_ptr(), 0, 41.0);
        closure::js_closure_set_capture_f64(b.get_raw_mut_ptr(), 0, 43.0);
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        object::js_object_set_field_by_name(
            first.get_raw_mut_ptr(),
            number_key.get_raw_mut_ptr(),
            3.0,
        );
        object::js_object_set_field_by_name(
            first.get_raw_mut_ptr(),
            method_key.get_raw_mut_ptr(),
            f64::from_bits(bits(a.get_raw_mut_ptr())),
        );
        let target = shapes::object_shape_stamp(first.get_raw_mut_ptr());
        assert_eq!(
            shapes::shape_descriptor_by_id(target)
                .unwrap()
                .special_constfn_mask,
            2
        );
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        object::js_object_set_field_by_name(
            second.get_raw_mut_ptr(),
            number_key.get_raw_mut_ptr(),
            7.0,
        );
        shapes::test_watch_cached_transition_stamps(
            second.get_raw_mut_ptr::<ObjectHeader>() as usize
        );
        object::js_object_set_field_by_name(
            second.get_raw_mut_ptr(),
            method_key.get_raw_mut_ptr(),
            f64::from_bits(bits(b.get_raw_mut_ptr())),
        );
        assert_eq!(shapes::test_cached_transition_stamps(), 1);
        shapes::test_reset_cached_transition_stamps();
        assert_eq!(shapes::object_shape_stamp(second.get_raw_mut_ptr()), target);
        assert_eq!(
            field_rep_store::object_slot_rep(second.get_raw_mut_ptr(), 0),
            field_rep::REP_F64
        );
        assert_eq!(
            object::js_object_get_field(second.get_raw_mut_ptr(), 0).bits(),
            7.0f64.to_bits()
        );
        let current = object::js_object_get_field(second.get_raw_mut_ptr(), 1)
            .as_pointer::<closure::ClosureHeader>();
        assert_eq!(current, b.get_raw_mut_ptr());
        assert_eq!(capture_body(current, closure::JsThis::UNDEFINED), 43.0);
    }
}
