use super::super::*;
use super::support::*;
use crate::gc::policy::{GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING, GC_TRIGGER_ARMED};

struct ParseStateGuard {
    pending: bool,
    flags: u8,
    armed: bool,
    before_suppression: usize,
}

#[test]
fn json_inline_object_parse_allocates_only_fresh_output_without_suppression() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let text = r#"{"a":1,"b":true,"c":"é"}"#.as_bytes();
    let input = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    unsafe { crate::json::test_json_parse_direct(input); }
    let _state = ParseStateGuard::new();
    let before = crate::arena::arena_in_use_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    let collections = gc_collection_count();
    let mut previous = 0usize;
    let mut allocated = 0usize;
    for i in 0..1000 {
        let value = unsafe {
            if i % 2 == 0 {
                crate::json::js_json_parse(input)
            } else {
                crate::json::js_json_parse_result(input).unwrap()
            }
        };
        let object = value.as_pointer::<crate::ObjectHeader>();
        assert_ne!(object as usize, previous);
        previous = object as usize;
        unsafe {
            let header = (object as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            allocated += (*header).size as usize;
            assert_eq!((*header).obj_type, GC_TYPE_OBJECT);
            assert_ne!((*header)._reserved & OBJ_FLAG_PLAIN_ORDINARY, 0);
            assert_eq!(crate::object::object_live_slot_count(object), 3);
            let keys = crate::object::object_keys_array(object);
            assert_eq!((*keys).length, 3);
            let fields = (object as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const crate::JSValue;
            assert_eq!((*fields).as_number(), 1.0);
            assert!((*fields.add(1)).as_bool());
            assert!((*fields.add(2)).is_short_string());
        }
    }
    assert_eq!(crate::arena::arena_in_use_bytes() - before, allocated);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(gc_collection_count(), collections);
    assert_eq!(GC_PRE_SUPPRESS_BYTES.with(|c| c.get()), 123);
}

#[test]
fn json_inline_object_parse_roots_keys_and_returns_movable_output() {
    for fallible in [false, true] {
        let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _state = ParseStateGuard::new();
        let _evacuation = ForcedEvacuationTestGuard::on();
        let _protection = crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
        let scope = RuntimeHandleScope::new();
        let text = br#"{"a":1,"b":true}"#;
        let input = scope.root_string_ptr(crate::js_string_from_bytes(text.as_ptr(), text.len() as u32));
        unsafe { crate::json::test_json_parse_direct(input.get_raw_const_ptr()); }
        let input_address = input.get_raw_const_ptr::<crate::StringHeader>() as usize;
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
        let value = unsafe {
            if fallible {
                crate::json::js_json_parse_result(input.get_raw_const_ptr()).unwrap()
            } else {
                crate::json::js_json_parse(input.get_raw_const_ptr())
            }
        };
        assert_ne!(input_address, input.get_raw_const_ptr::<crate::StringHeader>() as usize);
        assert!(!GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
        let output_address = value.as_pointer::<crate::ObjectHeader>() as usize;
        let output = scope.root_nanbox_u64(value.bits());
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        let moved = crate::JSValue::from_bits(output.get_nanbox_f64().to_bits()).as_pointer::<crate::ObjectHeader>();
        assert_ne!(output_address, moved as usize);
        unsafe {
            let keys = crate::object::object_keys_array(moved);
            assert_eq!((*keys).length, 2);
            assert!(crate::string::js_string_key_matches_bytes(crate::array::js_array_get(keys, 0), b"a"));
            assert!(crate::string::js_string_key_matches_bytes(crate::array::js_array_get(keys, 1), b"b"));
            let fields = (moved as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const crate::JSValue;
            assert_eq!((*fields).as_number(), 1.0);
            assert!((*fields.add(1)).as_bool());
        }
    }
}

impl ParseStateGuard {
    fn new() -> Self {
        Self {
            pending: GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.replace(false)),
            flags: GC_FLAGS.with(|c| c.get()),
            armed: GC_TRIGGER_ARMED.with(|c| c.replace(false)),
            before_suppression: GC_PRE_SUPPRESS_BYTES.with(|c| c.replace(123)),
        }
    }
}

impl Drop for ParseStateGuard {
    fn drop(&mut self) {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(self.pending));
        GC_FLAGS.with(|c| c.set(self.flags));
        GC_TRIGGER_ARMED.with(|c| c.set(self.armed));
        GC_PRE_SUPPRESS_BYTES.with(|c| c.set(self.before_suppression));
    }
}

#[test]
fn json_scalar_parse_preserves_blocked_debt_and_services_it_when_safe() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    for (text, fallible) in [
        (b"null".as_slice(), false),
        (b"\"a\"".as_slice(), true),
        (b"{}".as_slice(), false),
        (b"{ \t}".as_slice(), true),
    ] {
        let input = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let root = scope.root_string_ptr(input);
        let parse = || unsafe {
            if fallible {
                crate::json::js_json_parse_result(root.get_raw_const_ptr()).unwrap()
            } else {
                crate::json::js_json_parse(root.get_raw_const_ptr())
            }
        };
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
        let old_flags = GC_FLAGS.with(|c| c.replace(c.get() | GC_FLAG_SUPPRESSED));
        let before = gc_collection_count();
        let value = parse();
        assert!(value.is_null() || value.is_short_string() || value.is_pointer());
        assert_eq!(gc_collection_count(), before);
        assert!(GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
        GC_FLAGS.with(|c| c.set(old_flags));

        // Model the raise-only post-parse threshold: the debt hook must lower
        // and arm it. A just-due threshold with the arm manually cleared is
        // inconsistent with the state that schedules this debt.
        GC_NEXT_TRIGGER_BYTES.with(|c| c.set(usize::MAX));
        GC_TRIGGER_ARMED.with(|c| c.set(false));
        let value = parse();
        assert!(value.is_null() || value.is_short_string() || value.is_pointer());
        assert!(!GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
        assert!(
            gc_collection_count() > before
                || gc_budgeted_cycle_active()
                || GC_TRIGGER_ARMED.with(|c| c.get()),
            "pending work must reach the collector"
        );
    }
}

#[test]
fn json_empty_parse_allocates_only_fresh_output_without_suppression() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    let input = crate::js_string_from_bytes(b"{}".as_ptr(), 2);
    let before = crate::arena::arena_in_use_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    let collections = gc_collection_count();
    let mut previous = 0;
    let mut allocated = 0;
    for i in 0..1000 {
        let value = unsafe {
            if i % 2 == 0 {
                crate::json::js_json_parse(input)
            } else {
                crate::json::js_json_parse_result(input).unwrap()
            }
        };
        let object =
            value.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
        assert_ne!(object as usize, previous);
        previous = object as usize;
        unsafe {
            let header = (object as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            allocated += (*header).size as usize;
            assert_eq!((*header).obj_type, GC_TYPE_OBJECT);
            assert_ne!((*header)._reserved & OBJ_FLAG_PLAIN_ORDINARY, 0);
            assert_eq!((*object).class_id, 0);
            assert!((*object).meta.is_null());
            assert!(crate::object::object_keys_array(object).is_null());
            assert_eq!(crate::object::object_live_slot_count(object), 0);
        }
    }
    assert_eq!(crate::arena::arena_in_use_bytes() - before, allocated);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(gc_collection_count(), collections);
    assert_eq!(GC_PRE_SUPPRESS_BYTES.with(|c| c.get()), 123);
}

#[test]
fn json_empty_parse_finishes_reading_before_pending_evacuation() {
    assert_empty_parse_moves_input_and_output(false);
}

#[test]
fn json_empty_result_parse_finishes_reading_before_pending_evacuation() {
    assert_empty_parse_moves_input_and_output(true);
}

fn assert_empty_parse_moves_input_and_output(fallible: bool) {
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let text = b" \t{\n}\r ";
    let input = scope.root_string_ptr(crate::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ));
    let address = input.get_raw_const_ptr::<crate::StringHeader>() as usize;
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
    let value = unsafe {
        if fallible {
            crate::json::js_json_parse_result(input.get_raw_const_ptr()).unwrap()
        } else {
            crate::json::js_json_parse(input.get_raw_const_ptr())
        }
    };
    assert_ne!(
        input.get_raw_const_ptr::<crate::StringHeader>() as usize,
        address,
        "input must actually move"
    );
    assert!(!GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
    let output = scope.root_nanbox_u64(value.bits());
    let output_address = value.as_pointer::<crate::object::ObjectHeader>() as usize;
    let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let moved = crate::JSValue::from_bits(output.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    assert_ne!(moved as usize, output_address, "output must actually move");
    unsafe {
        assert!(crate::object::object_keys_array(moved).is_null());
        assert_eq!(crate::object::object_live_slot_count(moved), 0);
        let header = (moved as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        assert_ne!((*header)._reserved & OBJ_FLAG_PLAIN_ORDINARY, 0);
    }
}

#[test]
fn json_scalar_parse_allocates_no_managed_scratch_and_does_not_rebaseline() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    let input = crate::js_string_from_bytes(b"true".as_ptr(), 4);
    let before = crate::arena::arena_total_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    let collections = gc_collection_count();
    for _ in 0..1000 {
        assert!(unsafe { crate::json::js_json_parse(input) }.as_bool());
        assert!(unsafe { crate::json::js_json_parse_result(input) }
            .unwrap()
            .as_bool());
    }
    assert_eq!(crate::arena::arena_total_bytes(), before);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(gc_collection_count(), collections);
    assert_eq!(GC_PRE_SUPPRESS_BYTES.with(|c| c.get()), 123);
}

#[test]
fn json_direct_parse_input_survives_pending_collection() {
    assert_container_input_survives_collection(false, false, true);
}

#[test]
fn json_result_parse_input_survives_pending_collection() {
    assert_container_input_survives_collection(true, false, true);
}

#[test]
fn json_lazy_parse_input_survives_pending_collection() {
    assert_container_input_survives_collection(false, true, true);
}

#[test]
fn json_lazy_parse_input_survives_the_tape_entry_trigger() {
    assert_container_input_survives_collection(false, true, false);
}

fn assert_container_input_survives_collection(fallible: bool, array: bool, pending: bool) {
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    let text = if array {
        format!("[{}]", vec!["123"; 512].join(","))
    } else {
        r#"{"answer":42,"text":"kept input"}"#.to_string()
    };
    let scope = RuntimeHandleScope::new();
    let input = scope.root_string_ptr(crate::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ));
    let before_address = input.get_raw_const_ptr::<crate::StringHeader>() as usize;
    let before = gc_collection_count();
    // Model debt left at the preceding parse boundary. The pending hook must
    // lower and arm the threshold itself; the later gc_check_trigger is not
    // the first collection point in the ordinary entry.
    if pending {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
    } else {
        triggers.make_arena_trigger_due();
        GC_TRIGGER_ARMED.with(|c| c.set(true));
    }
    let value = unsafe {
        if fallible {
            crate::json::js_json_parse_result(input.get_raw_const_ptr()).unwrap()
        } else {
            crate::json::js_json_parse(input.get_raw_const_ptr())
        }
    };
    let output = scope.root_nanbox_u64(value.bits());
    assert!(gc_collection_count() > before, "entry hook must collect");
    assert_ne!(
        input.get_raw_const_ptr::<crate::StringHeader>() as usize,
        before_address,
        "the input must actually move"
    );
    assert!(!GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
    // Keep this assertion about the parse's input lifetime, independently of
    // any later stringify allocation/prototype bootstrap collection.
    gc_suppress();
    let rendered = unsafe { crate::json::js_json_stringify(output.get_nanbox_f64(), 0) };
    let actual = unsafe {
        std::slice::from_raw_parts(
            crate::string::string_data(rendered),
            (*rendered).byte_len as usize,
        )
    };
    assert_eq!(actual, text.as_bytes());
    gc_unsuppress();
}
