//! #11953: stream property publication must happen outside the registry borrow.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

per_test_global! {
    static PROPERTY_HOOK_MOVED: AtomicBool = AtomicBool::new(false);
}
per_test_global! {
    static PROBE_CALLS: AtomicUsize = AtomicUsize::new(0);
}

fn property_publication_gc_hook(receiver: &RuntimeHandle<'_>) {
    crate::fs::assert_stream_registries_available_for_tests();
    let before = crate::value::JSValue::from_bits(receiver.get_nanbox_f64().to_bits())
        .as_pointer::<crate::ObjectHeader>() as usize;
    let count = gc_collection_count();
    let _ = gc_collect_minor();
    assert!(
        gc_collection_count() > count,
        "property publication must run a minor collection"
    );
    let after = crate::value::JSValue::from_bits(receiver.get_nanbox_f64().to_bits())
        .as_pointer::<crate::ObjectHeader>() as usize;
    PROPERTY_HOOK_MOVED.store(after != before, Ordering::SeqCst);
}

extern "C" fn stream_probe_callback(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn callback_value() -> f64 {
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(stream_probe_callback, 0), 0);
    crate::value::js_nanbox_pointer(callback as i64)
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

fn object_value(ptr: *mut crate::ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn call_method(receiver: f64, method: &[u8], args: &[f64]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            receiver,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

fn field(receiver: f64, name: &[u8]) -> u64 {
    let key = crate::string::canonical_key(name);
    let ptr = crate::value::js_nanbox_get_pointer(receiver) as *const crate::ObjectHeader;
    crate::object::js_object_get_field_by_name(ptr, key).bits()
}

fn assert_gc_moved_receiver() {
    assert!(
        PROPERTY_HOOK_MOVED.load(Ordering::SeqCst),
        "the forced collection must relocate the rooted stream receiver"
    );
}

fn setup_moving_gc() -> (
    CopyingNurseryTestGuard,
    ConservativeScanDisabledGuard,
    GcTriggerThresholdTestGuard,
    ForcedEvacuationTestGuard,
) {
    let guard = CopyingNurseryTestGuard::new(0);
    let conservative = ConservativeScanDisabledGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::fs::scan_fs_stream_roots_mut);
    (guard, conservative, triggers, evacuation)
}

#[test]
fn classic_stream_publishes_props_after_registry_borrow_and_roots_listener() {
    let (_guard, _conservative, _triggers, _evacuation) = setup_moving_gc();
    PROPERTY_HOOK_MOVED.store(false, Ordering::SeqCst);
    PROBE_CALLS.store(0, Ordering::SeqCst);
    let scope = RuntimeHandleScope::new();
    let path = scope.root_nanbox_f64(string_value(b"/tmp/perry-stream-gc"));
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let stream = scope.root_nanbox_f64(crate::fs::js_fs_create_write_stream(
        path.get_nanbox_f64(),
        options.get_nanbox_f64(),
    ));
    let event = scope.root_nanbox_f64(string_value(b"probe"));
    {
        let callback_scope = RuntimeHandleScope::new();
        let callback = callback_scope.root_nanbox_f64(callback_value());
        let _ = call_method(
            stream.get_nanbox_f64(),
            b"on",
            &[event.get_nanbox_f64(), callback.get_nanbox_f64()],
        );
    }
    crate::fs::set_stream_property_hook_for_tests(Some(property_publication_gc_hook));
    let chunk = scope.root_nanbox_f64(string_value(b"x"));
    let _ = call_method(stream.get_nanbox_f64(), b"write", &[chunk.get_nanbox_f64()]);
    crate::fs::set_stream_property_hook_for_tests(None);
    assert_gc_moved_receiver();
    assert_eq!(
        field(stream.get_nanbox_f64(), b"writableLength"),
        1.0f64.to_bits()
    );
    let _ = call_method(stream.get_nanbox_f64(), b"emit", &[event.get_nanbox_f64()]);
    assert_eq!(PROBE_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn utf8_stream_publishes_props_after_registry_borrow_and_roots_listener() {
    let (_guard, _conservative, _triggers, _evacuation) = setup_moving_gc();
    PROPERTY_HOOK_MOVED.store(false, Ordering::SeqCst);
    PROBE_CALLS.store(0, Ordering::SeqCst);
    let scope = RuntimeHandleScope::new();
    let options = crate::object::js_object_alloc(0, 3);
    let fd_key = crate::string::canonical_key(b"fd");
    let min_key = crate::string::canonical_key(b"minLength");
    let mode_key = crate::string::canonical_key(b"mode");
    crate::object::js_object_set_field_by_name(options, fd_key, 13.0);
    crate::object::js_object_set_field_by_name(options, min_key, 128.0);
    crate::object::js_object_set_field_by_name(options, mode_key, 416.0);
    let options = scope.root_nanbox_f64(object_value(options));
    let stream = scope.root_nanbox_f64(crate::fs::js_fs_utf8_stream_new(options.get_nanbox_f64()));
    let event = scope.root_nanbox_f64(string_value(b"probe"));
    {
        let callback_scope = RuntimeHandleScope::new();
        let callback = callback_scope.root_nanbox_f64(callback_value());
        let _ = call_method(
            stream.get_nanbox_f64(),
            b"on",
            &[event.get_nanbox_f64(), callback.get_nanbox_f64()],
        );
    }
    crate::fs::set_stream_property_hook_for_tests(Some(property_publication_gc_hook));
    let chunk = scope.root_nanbox_f64(string_value(b"x"));
    let _ = call_method(stream.get_nanbox_f64(), b"write", &[chunk.get_nanbox_f64()]);
    crate::fs::set_stream_property_hook_for_tests(None);
    assert_gc_moved_receiver();
    assert_eq!(
        field(stream.get_nanbox_f64(), b"minLength"),
        128.0f64.to_bits()
    );
    assert_eq!(field(stream.get_nanbox_f64(), b"mode"), 416.0f64.to_bits());
    let _ = call_method(stream.get_nanbox_f64(), b"emit", &[event.get_nanbox_f64()]);
    assert_eq!(PROBE_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn utf8_stream_sync_write_end_and_close_preserve_normal_flow() {
    let scope = RuntimeHandleScope::new();
    let path = std::env::temp_dir().join(format!(
        "perry-stream-gc-normal-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let path_text = path.to_string_lossy().into_owned();
    let path_value = scope.root_nanbox_f64(string_value(path_text.as_bytes()));
    let flags = scope.root_nanbox_f64(string_value(b"w"));
    let fd = crate::fs::js_fs_open_sync(path_value.get_nanbox_f64(), flags.get_nanbox_f64());

    let options = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field_by_name(options, crate::string::canonical_key(b"fd"), fd);
    crate::object::js_object_set_field_by_name(
        options,
        crate::string::canonical_key(b"minLength"),
        1.0,
    );
    crate::object::js_object_set_field_by_name(
        options,
        crate::string::canonical_key(b"sync"),
        f64::from_bits(crate::value::TAG_TRUE),
    );
    let options = scope.root_nanbox_f64(object_value(options));
    let stream = scope.root_nanbox_f64(crate::fs::js_fs_utf8_stream_new(options.get_nanbox_f64()));
    let chunk = scope.root_nanbox_f64(string_value(b"stream payload"));
    let _ = call_method(stream.get_nanbox_f64(), b"write", &[chunk.get_nanbox_f64()]);
    let _ = call_method(stream.get_nanbox_f64(), b"end", &[]);

    assert_eq!(std::fs::read(&path).unwrap(), b"stream payload");
    assert_eq!(
        field(stream.get_nanbox_f64(), b"destroyed"),
        crate::value::TAG_TRUE
    );
    assert!(
        !crate::fs::fd_is_registered(fd as i32),
        "end must close its fd"
    );
    std::fs::remove_file(path).unwrap();
}
