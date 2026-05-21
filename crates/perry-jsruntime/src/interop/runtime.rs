//! Runtime lifecycle (`js_runtime_init` / `js_runtime_shutdown`) plus the
//! Reflect-metadata bridge JS that wires npm `reflect-metadata` into
//! Perry's native metadata store (#1021 NestJS decorator routing).

use super::*;

use deno_core::v8;

/// Initialize the JavaScript runtime
/// Must be called once before any other jsruntime functions
#[no_mangle]
pub extern "C" fn js_runtime_init() {
    jsruntime_profile_register();
    bump_v8_entry(V8EntryKind::RuntimeInit);
    // Force initialization of the Tokio runtime
    let _ = get_tokio_runtime();
    // Force initialization of the JS runtime on this thread
    ensure_runtime_initialized();

    // Register JS handle functions with perry-runtime so the unified functions can use them
    perry_runtime::js_set_handle_array_get(js_handle_array_get);
    perry_runtime::js_set_handle_array_length(js_handle_array_length);
    perry_runtime::js_set_handle_object_get_property(js_handle_object_get_property);
    perry_runtime::js_set_handle_to_string(js_handle_to_string);
    perry_runtime::js_set_handle_call_method(js_call_method);
    perry_runtime::js_set_native_module_js_loader(native_module_js_property_loader);
    perry_runtime::js_set_new_from_handle_v8(js_new_from_handle_v8_impl);
    perry_runtime::js_set_handle_typeof(js_handle_typeof);
    perry_runtime::promise::js_register_foreign_promise_adapter(js_await_any_promise);
    unsafe {
        js_register_jsruntime_pump(jsruntime_process_pending);
        js_register_jsruntime_has_active(jsruntime_has_active_handles);
    }

    with_runtime(install_reflect_metadata_bridge);
    with_runtime(capture_intrinsics_for_export_snapshots);
}

fn capture_intrinsics_for_export_snapshots(state: &mut JsRuntimeState) {
    deno_core::scope!(scope, &mut state.runtime);
    capture_export_snapshot_intrinsics(scope);
}

pub(crate) fn install_reflect_metadata_bridge(state: &mut JsRuntimeState) {
    deno_core::scope!(scope, &mut state.runtime);
    let global = scope.get_current_context().global(scope);

    macro_rules! define_global_function {
        ($name:literal, $callback:ident) => {
            if let (Some(key), Some(function)) = (
                v8::String::new(scope, $name),
                v8::Function::builder($callback).build(scope),
            ) {
                global.set(scope, key.into(), function.into());
            }
        };
    }

    define_global_function!(
        "__perryReflectDefineMetadata",
        reflect_define_metadata_bridge
    );
    define_global_function!("__perryReflectGetMetadata", reflect_get_metadata_bridge);
    define_global_function!(
        "__perryReflectGetOwnMetadata",
        reflect_get_own_metadata_bridge
    );
    define_global_function!("__perryReflectHasMetadata", reflect_has_metadata_bridge);
    define_global_function!(
        "__perryReflectHasOwnMetadata",
        reflect_has_own_metadata_bridge
    );
    define_global_function!(
        "__perryReflectGetMetadataKeys",
        reflect_get_metadata_keys_bridge
    );
    define_global_function!(
        "__perryReflectGetOwnMetadataKeys",
        reflect_get_own_metadata_keys_bridge
    );
    define_global_function!(
        "__perryReflectDeleteMetadata",
        reflect_delete_metadata_bridge
    );
    define_global_function!("__perryAsyncTick", perry_async_tick_bridge);

    let Some(source) = v8::String::new(
        scope,
        r#"
(function () {
  if (typeof Reflect !== "object" || Reflect === null) return;
  if (
    Reflect.__perryMetadataBridgeInstalled === true &&
    Reflect.defineMetadata &&
    Reflect.defineMetadata.__perryMetadataBridgeWrapper === true
  ) {
    return;
  }
  const markBridgeWrapper = fn => {
    try {
      Object.defineProperty(fn, "__perryMetadataBridgeWrapper", { value: true });
    } catch (_) {}
    return fn;
  };
  const originalDefine = Reflect.defineMetadata;
  const originalGet = Reflect.getMetadata;
  const originalGetOwn = Reflect.getOwnMetadata;
  const originalHas = Reflect.hasMetadata;
  const originalHasOwn = Reflect.hasOwnMetadata;
  if (typeof originalDefine !== "function" || typeof originalGet !== "function") {
    Reflect.defineMetadata = markBridgeWrapper(function (key, value, target, propertyKey) {
      return globalThis.__perryReflectDefineMetadata(key, value, target, propertyKey);
    });
    Reflect.getMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectGetMetadata(key, target, propertyKey);
    });
    Reflect.getOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectGetOwnMetadata(key, target, propertyKey);
    });
    Reflect.hasMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectHasMetadata(key, target, propertyKey);
    });
    Reflect.hasOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectHasOwnMetadata(key, target, propertyKey);
    });
    Reflect.getMetadataKeys = markBridgeWrapper(function (target, propertyKey) {
      return globalThis.__perryReflectGetMetadataKeys(target, propertyKey);
    });
    Reflect.getOwnMetadataKeys = markBridgeWrapper(function (target, propertyKey) {
      return globalThis.__perryReflectGetOwnMetadataKeys(target, propertyKey);
    });
    Reflect.deleteMetadata = markBridgeWrapper(function (key, target, propertyKey) {
      return globalThis.__perryReflectDeleteMetadata(key, target, propertyKey);
    });
    Reflect.metadata = markBridgeWrapper(function (key, value) {
      return function (target, propertyKey) {
        Reflect.defineMetadata(key, value, target, propertyKey);
      };
    });
    Reflect.__perryMetadataBridgeInstalled = true;
    return;
  }

  Reflect.defineMetadata = markBridgeWrapper(function (key, value, target, propertyKey) {
    const result = originalDefine.apply(this, arguments);
    globalThis.__perryReflectDefineMetadata(key, value, target, propertyKey);
    return result;
  });

  Reflect.getMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    const original = originalGet.apply(this, arguments);
    return original === undefined
      ? globalThis.__perryReflectGetMetadata(key, target, propertyKey)
      : original;
  });

  Reflect.getOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    const original = typeof originalGetOwn === "function"
      ? originalGetOwn.apply(this, arguments)
      : undefined;
    return original === undefined
      ? globalThis.__perryReflectGetOwnMetadata(key, target, propertyKey)
      : original;
  });

  Reflect.hasMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    if (typeof originalHas === "function" && originalHas.apply(this, arguments)) return true;
    return globalThis.__perryReflectHasMetadata(key, target, propertyKey);
  });

  Reflect.hasOwnMetadata = markBridgeWrapper(function (key, target, propertyKey) {
    if (typeof originalHasOwn === "function" && originalHasOwn.apply(this, arguments)) return true;
    return globalThis.__perryReflectHasOwnMetadata(key, target, propertyKey);
  });

  Reflect.__perryMetadataBridgeInstalled = true;
})();
"#,
    ) else {
        return;
    };
    let _ = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
}

fn perry_async_tick_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let promise = resolver.get_promise(scope);
    PENDING_JSRUNTIME_TICKS.with(|ticks| {
        ticks.borrow_mut().push(v8::Global::new(scope, resolver));
    });
    perry_runtime::event_pump::js_notify_main_thread();
    retval.set(promise.into());
}

fn reflect_define_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let value = v8_to_native_metadata_value(scope, args.get(1));
    let target = v8_to_native_metadata_target(scope, args.get(2));
    let property_key = v8_to_native(scope, args.get(3));
    let result = perry_runtime::proxy::js_reflect_define_metadata(key, value, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_get_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_own_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_get_own_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_has_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_has_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_has_own_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_has_own_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_metadata_keys_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let target = v8_to_native_metadata_target(scope, args.get(0));
    let property_key = v8_to_native(scope, args.get(1));
    let result = perry_runtime::proxy::js_reflect_get_metadata_keys(target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_get_own_metadata_keys_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let target = v8_to_native_metadata_target(scope, args.get(0));
    let property_key = v8_to_native(scope, args.get(1));
    let result = perry_runtime::proxy::js_reflect_get_own_metadata_keys(target, property_key);
    retval.set(native_to_v8(scope, result));
}

fn reflect_delete_metadata_bridge(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let key = v8_to_native(scope, args.get(0));
    let target = v8_to_native_metadata_target(scope, args.get(1));
    let property_key = v8_to_native(scope, args.get(2));
    let result = perry_runtime::proxy::js_reflect_delete_metadata(key, target, property_key);
    retval.set(native_to_v8(scope, result));
}

/// Shutdown the JavaScript runtime and release resources
#[no_mangle]
pub extern "C" fn js_runtime_shutdown() {
    bump_v8_entry(V8EntryKind::RuntimeShutdown);
    // The runtime will be cleaned up when the thread exits
    log::debug!("JS runtime shutdown requested");
}
