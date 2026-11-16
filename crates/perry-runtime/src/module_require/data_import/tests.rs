use super::*;
use crate::module_require::{js_module_dynamic_import_deferred, js_module_dynamic_import_fallback};

fn json(source: &str) -> f64 {
    let source = js_string_from_bytes(source.as_ptr(), source.len() as u32);
    unsafe { crate::json::js_json_parse_result(source) }
        .map(|value| f64::from_bits(value.bits()))
        .expect("valid test JSON")
}

fn settled(promise: f64, state: i32) -> f64 {
    let promise =
        JSValue::from_bits(promise.to_bits()).as_pointer::<crate::promise::Promise>() as *mut _;
    assert_eq!(crate::promise::js_promise_state(promise), state);
    crate::promise::js_promise_result(promise)
}

fn error_code(value: f64) -> Option<&'static str> {
    crate::node_submodules::error_code_for_error(
        JSValue::from_bits(value.to_bits()).as_pointer::<crate::error::ErrorHeader>(),
    )
}

#[test]
fn runtime_data_import_loaders_and_rejections() {
    let directory = std::env::temp_dir().join(format!(
        "perry-data-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let scope = RuntimeHandleScope::new();
    let path = directory.join("config space # %.data");
    let path = path.to_str().unwrap();
    let file_url =
        crate::module_require::file_url_text::path_to_file_url_string(path, cfg!(windows));
    let note = scope.root_nanbox_f64(string_value("deferred test site"));

    for (loader, contents) in [
        ("json", "{\"answer\":42}"),
        ("text", "hello π\n"),
        ("file", "asset"),
    ] {
        std::fs::write(path, contents).unwrap();
        let options =
            scope.root_nanbox_f64(json(&format!("{{\"with\":{{\"type\":\"{loader}\"}}}}")));
        for specifier in [path, file_url.as_str()] {
            let specifier = scope.root_nanbox_f64(string_value(specifier));
            for deferred in [false, true] {
                let promise = if deferred {
                    js_module_dynamic_import_deferred(
                        specifier.get_nanbox_f64(),
                        options.get_nanbox_f64(),
                        note.get_nanbox_f64(),
                    )
                } else {
                    js_module_dynamic_import_fallback(
                        specifier.get_nanbox_f64(),
                        options.get_nanbox_f64(),
                    )
                };
                let namespace = scope.root_nanbox_f64(settled(promise, 1));
                let value = property(namespace.get_nanbox_f64(), b"default").unwrap();
                match loader {
                    "json" => assert_eq!(property(value, b"answer").unwrap(), 42.0),
                    "text" => assert_eq!(string_bytes(value).as_deref(), Some(contents)),
                    "file" => assert_eq!(string_bytes(value).as_deref(), Some(path)),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[cfg(feature = "bun-cli-utils")]
    {
        std::fs::write(path, "provider = \"anthropic\"\n[settings]\nretry = 3\n").unwrap();
        let specifier = scope.root_nanbox_f64(string_value(&file_url));
        let options = scope.root_nanbox_f64(json(r#"{"with":{"type":"toml"}}"#));
        let promise = js_module_dynamic_import_deferred(
            specifier.get_nanbox_f64(),
            options.get_nanbox_f64(),
            note.get_nanbox_f64(),
        );
        let namespace = scope.root_nanbox_f64(settled(promise, 1));
        let table =
            scope.root_nanbox_f64(property(namespace.get_nanbox_f64(), b"default").unwrap());
        assert_eq!(
            string_bytes(property(table.get_nanbox_f64(), b"provider").unwrap()).as_deref(),
            Some("anthropic")
        );
        let settings = property(table.get_nanbox_f64(), b"settings").unwrap();
        assert_eq!(property(settings, b"retry").unwrap(), 3.0);
    }

    for loader in ["json", "toml"] {
        if loader == "toml" && !cfg!(feature = "bun-cli-utils") {
            continue;
        }
        std::fs::write(path, "invalid = [").unwrap();
        let options =
            scope.root_nanbox_f64(json(&format!("{{\"with\":{{\"type\":\"{loader}\"}}}}")));
        let specifier = scope.root_nanbox_f64(string_value(&file_url));
        let promise =
            js_module_dynamic_import_fallback(specifier.get_nanbox_f64(), options.get_nanbox_f64());
        let error = scope.root_nanbox_f64(settled(promise, 2));
        assert_eq!(
            string_bytes(property(error.get_nanbox_f64(), b"name").unwrap()).as_deref(),
            Some("SyntaxError")
        );
    }

    let options = scope.root_nanbox_f64(json(r#"{"with":{"type":"text"}}"#));
    for specifier in [
        "./relative.data",
        "https://example.invalid/data",
        "unknown-package",
    ] {
        let specifier = scope.root_nanbox_f64(string_value(specifier));
        let promise =
            js_module_dynamic_import_fallback(specifier.get_nanbox_f64(), options.get_nanbox_f64());
        let error = scope.root_nanbox_f64(settled(promise, 2));
        assert_eq!(
            error_code(error.get_nanbox_f64()),
            Some("ERR_MODULE_NOT_FOUND")
        );
    }
    // Existing runtime files without a supported data attribute remain deferred.
    let specifier = scope.root_nanbox_f64(string_value(path));
    let promise = js_module_dynamic_import_deferred(
        specifier.get_nanbox_f64(),
        undefined(),
        note.get_nanbox_f64(),
    );
    let error = scope.root_nanbox_f64(settled(promise, 2));
    // No supported data attribute, so this falls through to the ordinary
    // JavaScript-module deferral — which #10131 now prefixes with the
    // actionable native-build explanation. What this test is actually
    // asserting is that the deferral keeps its call-site note, so check the
    // note is carried rather than pinning the whole message text.
    let deferred_message =
        string_bytes(property(error.get_nanbox_f64(), b"message").unwrap()).unwrap();
    assert!(
        deferred_message.ends_with("deferred test site"),
        "the deferred site note must survive: {deferred_message}"
    );

    std::fs::remove_file(path).unwrap();
    // URL conversion failures reject the promise instead of throwing here.
    let invalid_url = scope.root_nanbox_f64(string_value(&file_url.replace("%20", "%2F")));
    let promise =
        js_module_dynamic_import_fallback(invalid_url.get_nanbox_f64(), options.get_nanbox_f64());
    let error = scope.root_nanbox_f64(settled(promise, 2));
    assert_eq!(
        string_bytes(property(error.get_nanbox_f64(), b"name").unwrap()).as_deref(),
        Some("TypeError")
    );
    let promise =
        js_module_dynamic_import_fallback(specifier.get_nanbox_f64(), options.get_nanbox_f64());
    let error = scope.root_nanbox_f64(settled(promise, 2));
    assert_eq!(error_code(error.get_nanbox_f64()), Some("ENOENT"));
    std::fs::remove_dir(directory).unwrap();
}

/// #13488: a statically resolved `import()` site validates its options before
/// loading: a non-object options bag, a non-object `with` and a non-string
/// attribute value each reject with a TypeError; absent or object-valued
/// options with string attributes pass.
#[test]
fn import_options_are_validated_before_loading() {
    let scope = RuntimeHandleScope::new();
    for source in [
        "1",
        "null",
        "\"json\"",
        "{\"with\":1}",
        "{\"with\":null}",
        "{\"with\":\"json\"}",
        "{\"with\":{\"type\":1}}",
        "{\"with\":{\"type\":\"json\",\"other\":null}}",
    ] {
        let options = scope.root_nanbox_f64(json(source));
        let promise = js_module_dynamic_import_check_options(options.get_nanbox_f64());
        assert_ne!(promise, 0, "{source} must reject");
        let promise = scope.root_nanbox_f64(js_nanbox_pointer(promise));
        let error = scope.root_nanbox_f64(settled(promise.get_nanbox_f64(), 2));
        assert_eq!(
            string_bytes(property(error.get_nanbox_f64(), b"name").unwrap()).as_deref(),
            Some("TypeError"),
            "{source}"
        );
    }
    for source in ["{}", "{\"with\":{}}", "{\"with\":{\"type\":\"json\"}}"] {
        let options = scope.root_nanbox_f64(json(source));
        assert_eq!(
            js_module_dynamic_import_check_options(options.get_nanbox_f64()),
            0,
            "{source} must pass"
        );
    }
    assert_eq!(js_module_dynamic_import_check_options(undefined()), 0);
}

fn key(name: &str) -> f64 {
    crate::value::nanbox_string_key(crate::string::canonical_key(name.as_bytes()))
}

per_test_global! {
    static WITH_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
}

/// A `with` getter that is valid on its first read and throws on any later one.
extern "C" fn with_getter_valid_once(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if WITH_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
        return json(r#"{"type":"json"}"#);
    }
    crate::exception::throw_core(crate::error::CoreError::type_error(
        crate::string::canonical_key(b"options.with read twice"),
    ))
}

/// #13488: EvaluateImportCall reads `options.with` once; the loader takes
/// `type` from the validated attributes rather than reading the getter again.
#[test]
fn import_options_with_is_read_once() {
    let path = std::env::temp_dir().join(format!("perry-with-once-{}.json", std::process::id()));
    std::fs::write(&path, "{\"answer\":42}").unwrap();
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(json("{}"));
    let getter = crate::closure::js_closure_alloc(crate::fn_info!(with_getter_valid_once, 0), 0);
    crate::object::js_object_define_getter(
        options.get_nanbox_f64(),
        key("with"),
        js_nanbox_pointer(getter as i64),
    );
    WITH_READS.store(0, std::sync::atomic::Ordering::SeqCst);
    let specifier = scope.root_nanbox_f64(string_value(path.to_str().unwrap()));
    let promise =
        js_module_dynamic_import_fallback(specifier.get_nanbox_f64(), options.get_nanbox_f64());
    let promise = scope.root_nanbox_f64(promise);
    assert_eq!(WITH_READS.load(std::sync::atomic::Ordering::SeqCst), 1);
    let namespace = scope.root_nanbox_f64(settled(promise.get_nanbox_f64(), 1));
    let value = property(namespace.get_nanbox_f64(), b"default").unwrap();
    assert_eq!(property(value, b"answer").unwrap(), 42.0);
    std::fs::remove_file(path).unwrap();
}

extern "C" fn throwing_own_keys(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
    _target: f64,
) -> f64 {
    crate::exception::throw_core(crate::error::CoreError::type_error(
        crate::string::canonical_key(b"ownKeys trap"),
    ))
}

/// #13488: the attributes are enumerated through [[OwnPropertyKeys]], so a
/// Proxy `with` runs its `ownKeys` trap and a throw there rejects the import.
#[test]
fn import_options_with_proxy_runs_own_keys() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::feature_hooks::js_runtime_install_proxy();
    let scope = RuntimeHandleScope::new();
    let handler = scope.root_nanbox_f64(json("{}"));
    let trap = crate::closure::js_closure_alloc(crate::fn_info!(throwing_own_keys, 1), 0);
    crate::object::property_ops::reflect::js_reflect_set(
        handler.get_nanbox_f64(),
        key("ownKeys"),
        js_nanbox_pointer(trap as i64),
        handler.get_nanbox_f64(),
    );
    let target = scope.root_nanbox_f64(json(r#"{"type":"json"}"#));
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        target.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    let options = scope.root_nanbox_f64(json("{}"));
    crate::object::property_ops::reflect::js_reflect_set(
        options.get_nanbox_f64(),
        key("with"),
        proxy.get_nanbox_f64(),
        options.get_nanbox_f64(),
    );
    let promise = js_module_dynamic_import_check_options(options.get_nanbox_f64());
    assert_ne!(promise, 0, "a throwing ownKeys trap must reject");
    let promise = scope.root_nanbox_f64(js_nanbox_pointer(promise));
    let error = scope.root_nanbox_f64(settled(promise.get_nanbox_f64(), 2));
    assert_eq!(
        string_bytes(property(error.get_nanbox_f64(), b"message").unwrap()).as_deref(),
        Some("ownKeys trap")
    );
}
