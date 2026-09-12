//! #10105: callers can report the native module-loading boundary and recover.
use super::*;
use crate::promise::{js_promise_reason, js_promise_state, Promise};

fn rejection_message(value: f64) -> String {
    assert_ne!(crate::promise::js_value_is_promise(value), 0);
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
    assert_eq!(
        js_promise_state(promise),
        2,
        "import must reject asynchronously"
    );
    crate::promise::js_promise_mark_internally_handled(promise);
    let error = crate::value::js_nanbox_get_pointer(js_promise_reason(promise))
        as *mut crate::error::ErrorHeader;
    assert_eq!(
        crate::node_submodules::error_code_for_error(error).as_deref(),
        Some("ERR_MODULE_NOT_FOUND"),
        "keep the code used by optional dependency loaders"
    );
    unsafe {
        assert_eq!(
            crate::exception::string_header_to_string(crate::error::js_error_get_name(error)),
            "Error"
        );
        crate::exception::string_header_to_string(crate::error::js_error_get_message(error))
    }
}

fn assert_native_guidance(message: &str, specifier: &str) {
    for expected in [
        specifier,
        "not available in this native build",
        "plugins",
        "custom tools",
        "providers",
        "Bun/Node distribution",
        "statically resolvable import()",
    ] {
        assert!(
            message.contains(expected),
            "missing {expected:?}: {message}"
        );
    }
}

#[test]
fn unresolved_imports_name_the_module_and_native_build_remedy() {
    for specifier in [
        "some-npm-plugin",
        "file:///config/tools/custom.ts",
        "file:///config/plugins/tui.tsx",
        "@example/custom-provider",
    ] {
        let message = rejection_message(js_module_dynamic_import_fallback(
            string_value(specifier),
            undefined(),
        ));
        assert_native_guidance(&message, specifier);
    }
}

#[test]
fn deferred_import_keeps_the_site_and_adds_the_runtime_specifier() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let specifier = scope.root_nanbox_f64(string_value("some-npm-plugin"));
    let note = string_value(
        "dynamic import() of a runtime-computed path cannot run in an ahead-of-time compiled binary (src/plugin/loader.ts:139)",
    );
    let message = rejection_message(js_module_dynamic_import_deferred(
        specifier.get_nanbox_f64(),
        undefined(),
        note,
    ));
    assert_native_guidance(&message, "some-npm-plugin");
    assert_eq!(message.matches("src/plugin/loader.ts:139").count(), 1);
}

#[test]
fn deferred_builtin_imports_still_resolve() {
    for specifier in ["os", "node:os", "node:fs/promises"] {
        let scope = crate::gc::RuntimeHandleScope::new();
        let specifier = scope.root_nanbox_f64(string_value(specifier));
        let note = string_value("deferred import at src/plugin/loader.ts:139");
        let value =
            js_module_dynamic_import_deferred(specifier.get_nanbox_f64(), undefined(), note);
        assert_ne!(crate::promise::js_value_is_promise(value), 0);
        let promise = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        assert_eq!(js_promise_state(promise), 1, "builtin import must resolve");
        assert_ne!(
            crate::promise::js_promise_value(promise).to_bits(),
            TAG_UNDEFINED,
            "the resolved namespace must exist"
        );
    }
}
