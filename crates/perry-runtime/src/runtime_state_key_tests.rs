use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_pointer, TAG_NULL, TAG_UNDEFINED};

const NAME: &[u8] = b"rtmemoField";

fn boxed(obj: *mut ObjectHeader) -> f64 {
    js_nanbox_pointer(obj as i64)
}

fn object<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    set(&obj, b"pad", 0.0);
    obj
}

fn set(obj: &RuntimeHandle<'_>, name: &[u8], value: f64) {
    let scope = RuntimeHandleScope::new();
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ));
    obj.with_mut_ptr(|p| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(p, k, value))
    });
}

fn inherit<'s>(scope: &'s RuntimeHandleScope, proto: &RuntimeHandle<'_>) -> RuntimeHandle<'s> {
    let value = crate::object::js_object_create(proto.with_mut_ptr(boxed));
    let obj =
        scope.root_raw_mut_ptr((value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader);
    set(&obj, b"pad", 0.0);
    obj
}

fn read(key: NamedStateKey, obj: &RuntimeHandle<'_>) -> f64 {
    obj.with_mut_ptr(|p| unsafe { key.read_object(p) })
}

fn leaf(key: NamedStateKey, obj: &RuntimeHandle<'_>) -> Option<f64> {
    obj.with_mut_ptr(|p| key.memo.with(|site| unsafe { site.read_leaf(p) }))
}

macro_rules! fresh_gate {
    ($name:literal) => {
        if !crate::object::method_site::run_with_fresh_worker_gate($name) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
    };
}

#[test]
fn runtime_node_modules_use_the_memo_entry() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = std::process::Command::new("python3")
        .arg(root.join("scripts/check_runtime_named_sites.py"))
        .output()
        .expect("run whole-module named-read invariant");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
}

#[test]
fn own_spill_read_and_write_share_the_state_memo_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let obj = object(&scope);
    for i in 0..20 {
        set(&obj, format!("padding{i}").as_bytes(), i as f64);
    }
    set(&obj, NAME, 41.0);
    let key = crate::runtime_state_key!(NAME);
    assert_eq!(read(key, &obj), 41.0);
    assert_eq!(
        leaf(key, &obj),
        Some(41.0),
        "read must prime the shared site"
    );
    let slot = obj.with_mut_ptr(|p| key.memo.with(|site| unsafe { site.own_slot(p).unwrap() }));
    obj.with_mut_ptr(|p| assert!(slot >= unsafe { crate::object::object_live_slot_count(p) }));
    key.write_value(obj.with_mut_ptr(boxed), 42.0);
    assert_eq!(leaf(key, &obj), Some(42.0));
    crate::object::js_object_freeze(obj.with_mut_ptr(boxed));
    assert!(crate::exception::catch_js_throw(|| {
        key.write_value(obj.with_mut_ptr(boxed), 99.0);
    })
    .is_err());
    assert_eq!(
        read(key, &obj),
        42.0,
        "a warmed read must not bypass frozen Set"
    );
}

#[test]
fn deep_absence_and_holder_reads_validate_every_hop() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "deep_absence_and_holder_reads_validate_every_hop",
    ) {
        return;
    }
    // The nursery guard already holds the global side-table isolation lock.
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    for scanner in [
        crate::object::method_site::read_holder::scan_read_holder_roots_mut,
        crate::object::scan_object_cache_roots_mut,
        crate::object::scan_shape_cache_roots_mut,
        crate::object::shapes::scan_shape_table_rekey_mut,
        crate::object::shapes::scan_shape_prototype_words_mut,
        crate::string::scan_intern_table_roots_mut,
    ] {
        crate::gc::gc_register_mutable_root_scanner(scanner);
    }
    let scope = RuntimeHandleScope::new();
    let terminal = object(&scope);
    crate::object::js_object_set_prototype_of(
        terminal.with_mut_ptr(boxed),
        f64::from_bits(TAG_NULL),
    );
    let mut chain = vec![terminal];
    for _ in 0..270 {
        chain.push(inherit(&scope, chain.last().unwrap()));
    }
    let obj = chain.last().unwrap();
    let key = crate::runtime_state_key!(NAME);
    assert_eq!(read(key, obj).to_bits(), TAG_UNDEFINED);
    assert_eq!(
        leaf(key, obj).map(f64::to_bits),
        Some(TAG_UNDEFINED),
        "absence must be memoized beyond the short holder entry"
    );
    set(&chain[6], NAME, 7.0);
    assert_ne!(
        leaf(key, obj).map(f64::to_bits),
        Some(TAG_UNDEFINED),
        "an intermediate key add invalidates absence"
    );
    assert_eq!(read(key, obj), 7.0);
    assert_eq!(leaf(key, obj), Some(7.0));
    set(&chain[6], NAME, 8.0);
    assert_eq!(
        leaf(key, obj),
        Some(8.0),
        "holder values are loaded on every use"
    );
    let old_shape =
        chain[6].with_mut_ptr(|p| unsafe { crate::object::shapes::object_shape_stamp(p) });
    set(&chain[6], NAME, f64::from_bits(TAG_NULL));
    if leaf(key, obj).is_none() {
        assert_ne!(
            chain[6].with_mut_ptr(|p| unsafe { crate::object::shapes::object_shape_stamp(p) }),
            old_shape,
            "a value-store miss must be a holder shape change"
        );
    }
    assert_eq!(read(key, obj).to_bits(), TAG_NULL);
    assert_eq!(leaf(key, obj).map(f64::to_bits), Some(TAG_NULL));
    set(&chain[6], NAME, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(read(key, obj).to_bits(), TAG_UNDEFINED);
    assert_eq!(leaf(key, obj).map(f64::to_bits), Some(TAG_UNDEFINED));
    let name = scope.root_string_ptr(crate::string::js_string_from_bytes(
        NAME.as_ptr(),
        NAME.len() as u32,
    ));
    chain[6].with_mut_ptr(|p| name.with_const_ptr(|k| crate::object::js_object_delete_field(p, k)));
    assert_eq!(read(key, obj).to_bits(), TAG_UNDEFINED);
    let replacement = object(&scope);
    set(&replacement, NAME, 9.0);
    crate::object::js_object_set_prototype_of(
        chain[9].with_mut_ptr(boxed),
        replacement.with_mut_ptr(boxed),
    );
    assert_eq!(read(key, obj), 9.0);
    assert_eq!(leaf(key, obj), Some(9.0));
    let before = obj.with_mut_ptr(boxed).to_bits();
    let holder_before = replacement.with_mut_ptr(boxed).to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(
        obj.with_mut_ptr(boxed).to_bits(),
        before,
        "the control must actually move the receiver"
    );
    assert_ne!(replacement.with_mut_ptr(boxed).to_bits(), holder_before);
    assert_eq!(
        leaf(key, obj),
        Some(9.0),
        "moving GC must preserve the warmed hit"
    );
    assert_eq!(read(key, obj), 9.0, "holder roots survive collection");
    set(obj, NAME, 10.0);
    assert_eq!(
        read(key, obj),
        10.0,
        "an own property shadows the cached holder"
    );
}

#[test]
fn null_prototype_absence_is_a_receiver_shape_fact() {
    fresh_gate!("null_prototype_absence_is_a_receiver_shape_fact");
    let scope = RuntimeHandleScope::new();
    let obj = object(&scope);
    crate::object::js_object_set_prototype_of(obj.with_mut_ptr(boxed), f64::from_bits(TAG_NULL));
    let key = crate::runtime_state_key!(NAME);
    assert_eq!(read(key, &obj).to_bits(), TAG_UNDEFINED);
    assert_eq!(leaf(key, &obj).map(f64::to_bits), Some(TAG_UNDEFINED));
    let same_shape = object(&scope);
    crate::object::js_object_set_prototype_of(
        same_shape.with_mut_ptr(boxed),
        f64::from_bits(TAG_NULL),
    );
    assert_eq!(
        leaf(key, &same_shape).map(f64::to_bits),
        Some(TAG_UNDEFINED)
    );
    set(&obj, NAME, 23.0);
    assert_eq!(read(key, &obj), 23.0);
    assert_eq!(read(key, &same_shape).to_bits(), TAG_UNDEFINED);
}

extern "C" fn getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    73.0
}

#[test]
fn warmed_own_read_does_not_turn_an_accessor_into_a_data_write() {
    fresh_gate!("warmed_own_read_does_not_turn_an_accessor_into_a_data_write");
    let scope = RuntimeHandleScope::new();
    let obj = object(&scope);
    set(&obj, NAME, 1.0);
    let key = crate::runtime_state_key!(NAME);
    assert_eq!(read(key, &obj), 1.0);
    assert_eq!(leaf(key, &obj), Some(1.0));
    let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    obj.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::set_accessor_descriptor(
            p as usize,
            String::from_utf8(NAME.to_vec()).unwrap(),
            crate::object::AccessorDescriptor {
                get: js_nanbox_pointer(get as i64).to_bits(),
                ..Default::default()
            },
        )
    });
    assert_eq!(read(key, &obj), 73.0);
    assert!(crate::exception::catch_js_throw(|| {
        key.write_value(obj.with_mut_ptr(boxed), 99.0);
    })
    .is_err());
    assert_eq!(
        read(key, &obj),
        73.0,
        "getter-only properties retain ordinary Set semantics"
    );
}

extern "C" fn echo_method(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    arg: f64,
) -> f64 {
    assert!(crate::value::JSValue::from_bits(this.as_f64().to_bits()).is_pointer());
    arg + 1.0
}

extern "C" fn method_getter(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let count = crate::closure::js_closure_get_capture_f64(closure, 1);
    crate::closure::js_closure_set_capture_f64(closure as *mut _, 1, count + 1.0);
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

#[test]
fn extension_method_site_preserves_this_and_reads_the_getter_once() {
    fresh_gate!("extension_method_site_preserves_this_and_reads_the_getter_once");
    let scope = RuntimeHandleScope::new();
    let obj = object(&scope);
    let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(echo_method, 1),
        0,
    ));
    let get = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(method_getter, 0),
        2,
    ));
    get.with_mut_ptr(|p| {
        crate::closure::js_closure_set_capture_f64(
            p,
            0,
            method
                .with_mut_ptr(|m: *mut crate::closure::ClosureHeader| js_nanbox_pointer(m as i64)),
        );
        crate::closure::js_closure_set_capture_f64(p, 1, 0.0);
    });
    obj.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::set_accessor_descriptor(
            p as usize,
            String::from_utf8(NAME.to_vec()).unwrap(),
            crate::object::AccessorDescriptor {
                get: get.with_mut_ptr(|g: *mut crate::closure::ClosureHeader| {
                    js_nanbox_pointer(g as i64).to_bits()
                }),
                ..Default::default()
            },
        )
    });
    let site = crate::native_payload::StateKeySite::new();
    assert_eq!(std::mem::size_of_val(&site), 16);
    for arg in [5.0, 6.0] {
        let args = [arg];
        assert_eq!(
            unsafe {
                js_runtime_state_key_call(
                    obj.with_mut_ptr(boxed),
                    NAME.as_ptr(),
                    NAME.len(),
                    &site,
                    args.as_ptr(),
                    args.len(),
                )
            },
            arg + 1.0
        );
    }
    assert_eq!(
        get.with_const_ptr(|p| crate::closure::js_closure_get_capture_f64(p, 1)),
        2.0
    );
}
