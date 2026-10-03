//! Positive cache reuse and refusals for ordered ConstFn key-add publication.
//! Scoped reads/capture stores are leaf operations. The property setter roots
//! receiver, key, and value before its allocating tail; no borrowed pointer is
//! used after that call. Movement observations retain only old integer addresses.
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
        a.with_mut_ptr(|a_ptr| closure::js_closure_set_capture_f64(a_ptr, 0, 31.0));
        b.with_mut_ptr(|b_ptr| closure::js_closure_set_capture_f64(b_ptr, 0, 47.0));
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        first.with_mut_ptr(|first_ptr| {
            key.with_mut_ptr(|key_ptr| {
                object::js_object_set_field_by_name(
                    first_ptr,
                    key_ptr,
                    f64::from_bits(a.with_mut_ptr(|a_ptr| bits(a_ptr))),
                )
            })
        });
        let target = first.with_mut_ptr(|first_ptr| shapes::object_shape_stamp(first_ptr));
        assert_eq!(
            shapes::shape_descriptor_by_id(target)
                .unwrap()
                .special_constfn_mask,
            1
        );
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        second.with_mut_ptr::<ObjectHeader, _>(|second_ptr| {
            shapes::test_watch_cached_transition_stamps(second_ptr as usize)
        });
        second.with_mut_ptr(|second_ptr| {
            key.with_mut_ptr(|key_ptr| {
                object::js_object_set_field_by_name(
                    second_ptr,
                    key_ptr,
                    f64::from_bits(b.with_mut_ptr(|b_ptr| bits(b_ptr))),
                )
            })
        });
        assert_eq!(
            shapes::test_cached_transition_stamps(),
            1,
            "must install cached Any intermediate"
        );
        shapes::test_reset_cached_transition_stamps();
        assert_eq!(
            second.with_mut_ptr(|second_ptr| shapes::object_shape_stamp(second_ptr)),
            target
        );
        b.with_mut_ptr(|b_ptr| {
            assert_eq!(second.with_mut_ptr(|second_ptr| slot(second_ptr)), b_ptr)
        });
        assert_eq!(
            capture_body(
                first.with_mut_ptr(|first_ptr| slot(first_ptr)),
                closure::JsThis::UNDEFINED
            ),
            31.0
        );
        assert_eq!(
            capture_body(
                second.with_mut_ptr(|second_ptr| slot(second_ptr)),
                closure::JsThis::UNDEFINED
            ),
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
        let pred = first.with_mut_ptr(|first_ptr| shapes::object_shape_stamp(first_ptr));
        first.with_mut_ptr(|first_ptr| {
            key.with_mut_ptr(|key_ptr| {
                object::js_object_set_field_by_name(
                    first_ptr,
                    key_ptr,
                    f64::from_bits(closure.with_mut_ptr(|closure_ptr| bits(closure_ptr))),
                )
            })
        });
        let hit = key
            .with_mut_ptr(|key_ptr| object::transition_cache_lookup(pred, key_ptr))
            .expect("cached body edge");
        let receiver = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        assert_eq!(
            receiver.with_mut_ptr(|receiver_ptr| shapes::object_shape_stamp(receiver_ptr)),
            pred
        );
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
            wrong.with_mut_ptr(|wrong_ptr| bits(wrong_ptr)),
            unsafe_this.with_mut_ptr(|unsafe_this_ptr| bits(unsafe_this_ptr)),
            unloadable.with_mut_ptr(|unloadable_ptr| bits(unloadable_ptr)),
            9.0f64.to_bits(),
            value::TAG_UNDEFINED,
        ] {
            assert!(receiver
                .with_mut_ptr(|receiver_ptr| admit_or_store(receiver_ptr, pred, hit, value))
                .is_none());
            assert_eq!(
                receiver.with_mut_ptr(|receiver_ptr| shapes::object_shape_stamp(receiver_ptr)),
                pred
            );
            assert_eq!(
                receiver
                    .with_mut_ptr(|receiver_ptr| object::js_object_get_field(receiver_ptr, 0))
                    .bits(),
                value::TAG_UNDEFINED
            );
        }
        assert!(shapes::shape_record_by_id(hit.2)
            .unwrap()
            .deprecate_special_to_any(hit.1));
        assert!(receiver
            .with_mut_ptr(|receiver_ptr| admit_or_store(
                receiver_ptr,
                pred,
                hit,
                closure.with_mut_ptr(|closure_ptr| bits(closure_ptr))
            ))
            .is_none());
        assert_eq!(
            receiver.with_mut_ptr(|receiver_ptr| shapes::object_shape_stamp(receiver_ptr)),
            pred
        );
        assert_eq!(
            receiver
                .with_mut_ptr(|receiver_ptr| object::js_object_get_field(receiver_ptr, 0))
                .bits(),
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
        a.with_mut_ptr(|a_ptr| closure::js_closure_set_capture_f64(a_ptr, 0, 17.0));
        b.with_mut_ptr(|b_ptr| closure::js_closure_set_capture_f64(b_ptr, 0, 29.0));
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        first.with_mut_ptr(|first_ptr| {
            key.with_mut_ptr(|key_ptr| {
                object::js_object_set_field_by_name(
                    first_ptr,
                    key_ptr,
                    f64::from_bits(a.with_mut_ptr(|a_ptr| bits(a_ptr))),
                )
            })
        });
        let target = first.with_mut_ptr(|first_ptr| shapes::object_shape_stamp(first_ptr));
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        let original_receiver = second.with_mut_ptr::<ObjectHeader, _>(|obj| obj as usize);
        let original_closure = b.with_mut_ptr::<closure::ClosureHeader, _>(|c| c as usize);
        assert!(crate::arena::pointer_in_nursery(original_receiver));
        assert!(crate::arena::pointer_in_nursery(original_closure));
        // Only scalar old-address observations cross collection. Both handles
        // reload after the same real copying minor, before any dereference.
        let ((_, moved_closure), moved_receiver) = second.across_mut::<ObjectHeader, _>(|| {
            b.across_mut::<closure::ClosureHeader, _>(|| gc::gc_collect_minor())
        });
        assert_ne!(
            moved_receiver as usize, original_receiver,
            "pre-store receiver must move"
        );
        assert_ne!(
            moved_closure as usize, original_closure,
            "incoming closure root must refresh"
        );
        assert_eq!(
            second.with_mut_ptr(|second_ptr| field_rep_store::object_slot_rep(second_ptr, 0)),
            field_rep::REP_ANY
        );
        assert_eq!(
            second
                .with_mut_ptr(|second_ptr| object::js_object_get_field(second_ptr, 0))
                .bits(),
            value::TAG_UNDEFINED
        );
        second.with_mut_ptr::<ObjectHeader, _>(|second_ptr| {
            shapes::test_watch_cached_transition_stamps(second_ptr as usize)
        });
        second.with_mut_ptr(|second_ptr| {
            key.with_mut_ptr(|key_ptr| {
                object::js_object_set_field_by_name(
                    second_ptr,
                    key_ptr,
                    f64::from_bits(b.with_mut_ptr(|b_ptr| bits(b_ptr))),
                )
            })
        });
        assert_eq!(
            shapes::test_cached_transition_stamps(),
            expected_cached_stamps,
            "content keys reuse the cache; relocated pointer keys safely miss"
        );
        shapes::test_reset_cached_transition_stamps();
        let before_receiver = second.with_mut_ptr::<ObjectHeader, _>(|obj| obj as usize);
        let before_closure = second.with_mut_ptr(|second_ptr| slot(second_ptr)) as usize;
        second.with_mut_ptr(|second_ptr| {
            field_rep_store::assert_field_rep_lanes(
                second_ptr,
                shapes::object_shape_record(second_ptr),
                1,
            )
        });
        let (_, moved_receiver) = second.across_mut::<ObjectHeader, _>(|| gc::gc_collect_minor());
        field_rep_store::assert_field_rep_lanes(
            moved_receiver,
            shapes::object_shape_record(moved_receiver),
            1,
        );
        assert_ne!(
            moved_receiver as usize, before_receiver,
            "post-store receiver must move"
        );
        assert_ne!(
            slot(moved_receiver) as usize,
            before_closure,
            "SPECIAL current closure slot must rewrite"
        );
        assert_eq!(
            second.with_mut_ptr(|second_ptr| shapes::object_shape_stamp(second_ptr)),
            target
        );
        assert_eq!(
            capture_body(
                first.with_mut_ptr(|first_ptr| slot(first_ptr)),
                closure::JsThis::UNDEFINED
            ),
            17.0
        );
        assert_eq!(
            capture_body(
                second.with_mut_ptr(|second_ptr| slot(second_ptr)),
                closure::JsThis::UNDEFINED
            ),
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
        a.with_mut_ptr(|a_ptr| closure::js_closure_set_capture_f64(a_ptr, 0, 41.0));
        b.with_mut_ptr(|b_ptr| closure::js_closure_set_capture_f64(b_ptr, 0, 43.0));
        let first = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        first.with_mut_ptr(|first_ptr| {
            number_key.with_mut_ptr(|number_key_ptr| {
                object::js_object_set_field_by_name(first_ptr, number_key_ptr, 3.0)
            })
        });
        first.with_mut_ptr(|first_ptr| {
            method_key.with_mut_ptr(|method_key_ptr| {
                object::js_object_set_field_by_name(
                    first_ptr,
                    method_key_ptr,
                    f64::from_bits(a.with_mut_ptr(|a_ptr| bits(a_ptr))),
                )
            })
        });
        let target = first.with_mut_ptr(|first_ptr| shapes::object_shape_stamp(first_ptr));
        assert_eq!(
            shapes::shape_descriptor_by_id(target)
                .unwrap()
                .special_constfn_mask,
            2
        );
        let second = scope.root_raw_mut_ptr(object::js_object_alloc(0, 4));
        second.with_mut_ptr(|second_ptr| {
            number_key.with_mut_ptr(|number_key_ptr| {
                object::js_object_set_field_by_name(second_ptr, number_key_ptr, 7.0)
            })
        });
        second.with_mut_ptr::<ObjectHeader, _>(|second_ptr| {
            shapes::test_watch_cached_transition_stamps(second_ptr as usize)
        });
        second.with_mut_ptr(|second_ptr| {
            method_key.with_mut_ptr(|method_key_ptr| {
                object::js_object_set_field_by_name(
                    second_ptr,
                    method_key_ptr,
                    f64::from_bits(b.with_mut_ptr(|b_ptr| bits(b_ptr))),
                )
            })
        });
        assert_eq!(shapes::test_cached_transition_stamps(), 1);
        shapes::test_reset_cached_transition_stamps();
        assert_eq!(
            second.with_mut_ptr(|second_ptr| shapes::object_shape_stamp(second_ptr)),
            target
        );
        assert_eq!(
            second.with_mut_ptr(|second_ptr| field_rep_store::object_slot_rep(second_ptr, 0)),
            field_rep::REP_F64
        );
        assert_eq!(
            second
                .with_mut_ptr(|second_ptr| object::js_object_get_field(second_ptr, 0))
                .bits(),
            7.0f64.to_bits()
        );
        let current = second
            .with_mut_ptr(|second_ptr| object::js_object_get_field(second_ptr, 1))
            .as_pointer::<closure::ClosureHeader>();
        b.with_mut_ptr(|b_ptr| assert_eq!(current, b_ptr));
        assert_eq!(capture_body(current, closure::JsThis::UNDEFINED), 43.0);
    }
}
