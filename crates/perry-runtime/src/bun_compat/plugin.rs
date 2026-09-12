//! Inert Bun runtime plugin registration (#10100). Setup runs now, but native
//! builds cannot install JavaScript module loaders after compilation.

use super::{key_ptr, object_field, undefined};
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::value::{js_nanbox_pointer, JSValue};

extern "C" fn ignore(_closure: *const ClosureHeader, _value: f64) -> f64 {
    undefined()
}

fn noop() -> f64 {
    js_register_closure_arity(ignore as *const u8, 1);
    js_nanbox_pointer(js_closure_alloc(ignore as *const u8, 0) as i64)
}

fn set(scope: &RuntimeHandleScope, object: &RuntimeHandle, name: &[u8], value: f64) {
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(key_ptr(name));
    key.with_mut_ptr(|key| {
        js_object_set_field_by_name(
            JSValue::from_bits(object.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>()
                .cast_mut(),
            key,
            value.get_nanbox_f64(),
        );
    });
}

pub fn decorate_bun_plugin(value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let plugin = scope.root_nanbox_f64(value);
    let clear = scope.root_nanbox_f64(noop());
    let raw = JSValue::from_bits(plugin.get_nanbox_f64().to_bits()).as_pointer::<u8>();
    crate::closure::closure_set_dynamic_prop(raw as usize, "clearAll", clear.get_nanbox_f64());
    plugin.get_nanbox_f64()
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_bun_plugin(plugin: f64) -> f64 {
    register(plugin)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_bun_plugin(plugin: f64) -> f64 {
    register(plugin)
}

fn register(plugin: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let plugin = scope.root_nanbox_f64(plugin);
    let is_function = !crate::fs::extract_closure_ptr(plugin.get_nanbox_f64()).is_null();
    let setup = if is_function {
        plugin.get_nanbox_f64()
    } else {
        object_field(plugin.get_nanbox_f64(), b"setup").unwrap_or_else(undefined)
    };
    if crate::fs::extract_closure_ptr(setup).is_null() {
        let message =
            key_ptr(b"Bun.plugin expects a setup function or an object with a callable setup");
        let error = crate::error::js_typeerror_new(message);
        crate::exception::js_throw(js_nanbox_pointer(error as i64));
    }
    let setup = scope.root_nanbox_f64(setup);
    crate::stub_diag::perry_runtime_stub(
        "Bun.plugin",
        "setup runs synchronously; runtime loader hooks are ignored in native builds",
        Some("#10100"),
    );
    if crate::stub_diag::strict_stubs_enabled() {
        let message =
            key_ptr(b"Bun.plugin runtime transforms are not available in the native build");
        crate::node_submodules::register_error_code_pub(message, "ERR_PERRY_UNIMPLEMENTED");
        let error = crate::error::js_error_new_with_message(message);
        crate::exception::js_throw(js_nanbox_pointer(error as i64));
    }
    let build = scope.root_nanbox_f64(js_nanbox_pointer(js_object_alloc(0, 6) as i64));
    let hook = scope.root_nanbox_f64(noop());
    for name in [
        b"onLoad".as_slice(),
        b"onResolve",
        b"onStart",
        b"onEnd",
        b"module",
    ] {
        set(&scope, &build, name, hook.get_nanbox_f64());
    }
    set(
        &scope,
        &build,
        b"config",
        js_nanbox_pointer(js_object_alloc(0, 0) as i64),
    );
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(if is_function {
        undefined()
    } else {
        plugin.get_nanbox_f64()
    }));
    let result = crate::exception::catch_js_throw(|| unsafe {
        let args = [build.get_nanbox_f64()];
        crate::closure::js_native_call_value(setup.get_nanbox_f64(), args.as_ptr(), 1)
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    let result = match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    };
    if crate::promise::js_value_is_promise(result) != 0 {
        // Bun forwards async setup's promise, including its fulfillment value
        // and rejection. Hooks stay inert on either side of await.
        result
    } else {
        undefined()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SETUP_CALLS: AtomicU32 = AtomicU32::new(0);
    static LOADER_CALLS: AtomicU32 = AtomicU32::new(0);

    fn closure(func: extern "C" fn(*const ClosureHeader, f64) -> f64) -> f64 {
        js_register_closure_arity(func as *const u8, 1);
        js_nanbox_pointer(js_closure_alloc(func as *const u8, 0) as i64)
    }

    extern "C" fn loader(_closure: *const ClosureHeader, _args: f64) -> f64 {
        LOADER_CALLS.fetch_add(1, Ordering::SeqCst);
        undefined()
    }

    extern "C" fn setup(_closure: *const ClosureHeader, build: f64) -> f64 {
        SETUP_CALLS.fetch_add(1, Ordering::SeqCst);
        let scope = RuntimeHandleScope::new();
        let build = scope.root_nanbox_f64(build);
        let callback = scope.root_nanbox_f64(closure(loader));
        // The builder must remain usable across collection in user setup.
        crate::gc::js_gc_collect();
        for name in [
            b"onLoad".as_slice(),
            b"onResolve",
            b"onStart",
            b"onEnd",
            b"module",
        ] {
            let hook = object_field(build.get_nanbox_f64(), name).expect("builder hook");
            let args = [undefined(), callback.get_nanbox_f64()];
            let result = unsafe { crate::closure::js_native_call_value(hook, args.as_ptr(), 2) };
            assert_eq!(result.to_bits(), undefined().to_bits());
        }
        assert!(object_field(build.get_nanbox_f64(), b"config").is_some());
        42.0
    }

    #[test]
    fn calls_setup_for_objects_and_functions_without_running_hooks() {
        SETUP_CALLS.store(0, Ordering::SeqCst);
        LOADER_CALLS.store(0, Ordering::SeqCst);
        let scope = RuntimeHandleScope::new();
        let setup = scope.root_nanbox_f64(closure(setup));
        let object = scope.root_nanbox_f64(js_nanbox_pointer(js_object_alloc(0, 1) as i64));
        set(&scope, &object, b"setup", setup.get_nanbox_f64());
        assert_eq!(
            js_bun_plugin(object.get_nanbox_f64()).to_bits(),
            undefined().to_bits()
        );
        assert_eq!(
            js_bun_plugin(setup.get_nanbox_f64()).to_bits(),
            undefined().to_bits()
        );
        assert_eq!(SETUP_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(LOADER_CALLS.load(Ordering::SeqCst), 0);
    }

    extern "C" fn async_setup(_closure: *const ClosureHeader, _build: f64) -> f64 {
        js_nanbox_pointer(crate::promise::js_promise_resolved(42.0) as i64)
    }

    #[test]
    fn async_setup_preserves_its_result() {
        let scope = RuntimeHandleScope::new();
        let result = scope.root_nanbox_f64(js_bun_plugin(closure(async_setup)));
        assert_eq!(
            crate::promise::js_value_is_promise(result.get_nanbox_f64()),
            1
        );
        let promise = JSValue::from_bits(result.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        assert_eq!(crate::promise::js_promise_state(promise), 1);
        assert_eq!(crate::promise::js_promise_value(promise), 42.0);
    }

    #[test]
    fn clear_all_is_a_callable_noop() {
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(decorate_bun_plugin(closure(setup)));
        let raw = JSValue::from_bits(value.get_nanbox_f64().to_bits()).as_pointer::<u8>();
        let clear = crate::closure::closure_get_dynamic_prop(raw as usize, "clearAll");
        assert!(!crate::fs::extract_closure_ptr(clear).is_null());
        let result = unsafe { crate::closure::js_native_call_value(clear, std::ptr::null(), 0) };
        assert_eq!(result.to_bits(), undefined().to_bits());
    }
}
