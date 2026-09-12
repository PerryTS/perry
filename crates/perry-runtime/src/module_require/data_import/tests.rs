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
    let file_url = crate::url::node_compat::path_to_file_url_string(path, cfg!(windows));
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
