use super::*;
use std::cell::Cell;

extern "C" fn module_cjs_extension_noop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _module: f64,
    _filename: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_cjs_extension_function(name: &str) -> f64 {
    let func_ptr = module_cjs_extension_noop_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    crate::closure::js_register_closure_length(func_ptr, 2);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        set_bound_native_closure_name(closure, name);
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_builtin_closure_length(closure as usize, 2);
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

fn store_module_cjs_root(slot: &Cell<u64>, value: f64) -> f64 {
    slot.set(value.to_bits());
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

pub(crate) fn module_cjs_cache_value() -> f64 {
    MODULE_CJS_CACHE_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let obj = crate::object::js_object_alloc_null_proto(0, 0);
        store_module_cjs_root(slot, native_object_value(obj))
    })
}

pub(crate) fn module_cjs_path_cache_value() -> f64 {
    MODULE_CJS_PATH_CACHE_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let obj = crate::object::js_object_alloc_null_proto(0, 0);
        store_module_cjs_root(slot, native_object_value(obj))
    })
}

pub(crate) fn module_cjs_extensions_value() -> f64 {
    MODULE_CJS_EXTENSIONS_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_nanbox_f64(native_object_value(js_object_alloc(0, 3)));
        store_module_cjs_root(slot, obj.get_nanbox_f64());
        for name in [".js", ".json", ".node"] {
            let value = scope.root_nanbox_f64(module_cjs_extension_function(name));
            native_set_field(
                crate::value::js_nanbox_get_pointer(obj.get_nanbox_f64()) as *mut ObjectHeader,
                name,
                value.get_nanbox_f64(),
            );
        }
        store_module_cjs_root(slot, obj.get_nanbox_f64())
    })
}

pub(crate) fn module_cjs_global_paths_value() -> f64 {
    MODULE_CJS_GLOBAL_PATHS_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let mut paths = Vec::new();
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = std::path::PathBuf::from(home);
            paths.push(home.join(".node_modules").to_string_lossy().into_owned());
            paths.push(home.join(".node_libraries").to_string_lossy().into_owned());
        }
        let prefix = std::env::var_os("PREFIX")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent()?.parent().map(std::path::Path::to_path_buf))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/local"));
        paths.push(prefix.join("lib/node").to_string_lossy().into_owned());

        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = scope.root_nanbox_f64(f64::from_bits(
            JSValue::array_ptr(crate::array::js_array_alloc_with_length(paths.len() as u32)).bits(),
        ));
        store_module_cjs_root(slot, arr.get_nanbox_f64());
        for (i, path) in paths.iter().enumerate() {
            let value = scope.root_nanbox_f64(native_string_value(path));
            crate::array::js_array_set_f64(
                JSValue::from_bits(arr.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::array::ArrayHeader>() as *mut _,
                i as u32,
                value.get_nanbox_f64(),
            );
        }
        store_module_cjs_root(slot, arr.get_nanbox_f64())
    })
}

pub(crate) fn module_builtin_modules_value() -> f64 {
    MODULE_BUILTIN_MODULES_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = scope.root_nanbox_f64(f64::from_bits(
            JSValue::array_ptr(crate::array::js_array_alloc_with_length(
                crate::process::MODULE_BUILTIN_MODULES.len() as u32,
            ))
            .bits(),
        ));
        store_module_cjs_root(slot, arr.get_nanbox_f64());
        for (i, name) in crate::process::MODULE_BUILTIN_MODULES.iter().enumerate() {
            let value = scope.root_nanbox_f64(native_string_value(name));
            crate::array::js_array_set_f64(
                JSValue::from_bits(arr.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::array::ArrayHeader>() as *mut _,
                i as u32,
                value.get_nanbox_f64(),
            );
        }
        let value = arr.get_nanbox_f64();
        crate::object::js_object_freeze(value);
        store_module_cjs_root(slot, value)
    })
}

pub(crate) fn module_constants_value() -> f64 {
    MODULE_CONSTANTS_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let constants = scope.root_nanbox_f64(native_object_value(
            crate::object::js_object_alloc_null_proto(0, 1),
        ));
        store_module_cjs_root(slot, constants.get_nanbox_f64());
        let status = scope.root_nanbox_f64(native_object_value(
            crate::object::js_object_alloc_null_proto(0, 4),
        ));
        for (name, value) in [
            ("FAILED", 0.0),
            ("ENABLED", 1.0),
            ("ALREADY_ENABLED", 2.0),
            ("DISABLED", 3.0),
        ] {
            native_set_field(
                crate::value::js_nanbox_get_pointer(status.get_nanbox_f64()) as *mut ObjectHeader,
                name,
                value,
            );
        }
        let status_value = status.get_nanbox_f64();
        crate::object::js_object_freeze(status_value);
        native_set_field(
            crate::value::js_nanbox_get_pointer(constants.get_nanbox_f64()) as *mut ObjectHeader,
            "compileCacheStatus",
            status_value,
        );
        let value = constants.get_nanbox_f64();
        crate::object::js_object_freeze(value);
        store_module_cjs_root(slot, value)
    })
}

extern "C" fn module_wrap_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
) -> f64 {
    let value = JSValue::from_bits(source.to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let source = unsafe { crate::string::js_string_key_bytes(value, &mut sso) }
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();
    native_string_value(&format!(
        "(function (exports, require, module, __filename, __dirname) {{ {source}\n}});"
    ))
}

fn module_wrap_value() -> f64 {
    let func = module_wrap_thunk as *const u8;
    crate::closure::js_register_closure_arity(func, 1);
    crate::closure::js_register_closure_length(func, 1);
    let closure = crate::closure::js_closure_alloc(func, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        set_bound_native_closure_name(closure, "wrap");
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        set_builtin_closure_length(closure as usize, 1);
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

fn module_wrapper_value() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_nanbox_f64(f64::from_bits(
        JSValue::array_ptr(crate::array::js_array_alloc_with_length(2)).bits(),
    ));
    let prefix = scope.root_nanbox_f64(native_string_value(
        "(function (exports, require, module, __filename, __dirname) { ",
    ));
    crate::array::js_array_set_f64(
        JSValue::from_bits(arr.get_nanbox_f64().to_bits()).as_pointer::<crate::array::ArrayHeader>()
            as *mut _,
        0,
        prefix.get_nanbox_f64(),
    );
    let suffix = scope.root_nanbox_f64(native_string_value("\n});"));
    crate::array::js_array_set_f64(
        JSValue::from_bits(arr.get_nanbox_f64().to_bits()).as_pointer::<crate::array::ArrayHeader>()
            as *mut _,
        1,
        suffix.get_nanbox_f64(),
    );
    arr.get_nanbox_f64()
}

extern "C" fn module_prototype_method_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _a: f64,
    _b: f64,
    _c: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn module_prototype_load_thunk(
    _closure: *const crate::closure::ClosureHeader,
    filename: f64,
    _b: f64,
    _c: f64,
) -> f64 {
    crate::process::js_module_instance_load(filename)
}

extern "C" fn module_prototype_require_thunk(
    _closure: *const crate::closure::ClosureHeader,
    specifier: f64,
    _b: f64,
    _c: f64,
) -> f64 {
    crate::process::js_module_instance_require(specifier)
}

fn module_prototype_method(name: &str, length: u32) -> f64 {
    let func = match name {
        "load" => module_prototype_load_thunk as *const u8,
        "require" => module_prototype_require_thunk as *const u8,
        _ => module_prototype_method_thunk as *const u8,
    };
    crate::closure::js_register_closure_arity(func, 3);
    let closure = crate::closure::js_closure_alloc(func, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        set_bound_native_closure_name(closure, "");
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        set_builtin_closure_length(closure as usize, length);
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

extern "C" fn module_prototype_constructor_getter(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    // Resolve through the canonical callable cache at access time. Capturing
    // the constructor while its own attach was still in progress preserved a
    // pre-publication pointer; after moving GC, the named import and inherited
    // getter could compare as different values even though instanceof used the
    // same prototype. The cache is populated before user code can reach this
    // getter, so this returns the exact exported Module identity.
    bound_native_callable_export_value("module", "Module")
}

extern "C" fn module_prototype_false_getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
    native_bool_value(false)
}

extern "C" fn module_prototype_parent_getter(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn module_prototype_parent_setter(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_accessor(
    get_func: *const u8,
    set_func: Option<*const u8>,
    capture: Option<f64>,
) -> crate::object::AccessorDescriptor {
    let scope = crate::gc::RuntimeHandleScope::new();
    let capture = capture.map(|value| scope.root_nanbox_f64(value));
    crate::closure::js_register_closure_arity(get_func, 0);
    let getter = crate::closure::js_closure_alloc(get_func, capture.is_some() as u32);
    let getter = scope.root_raw_mut_ptr(getter);
    if let Some(value) = capture.as_ref() {
        getter.with_mut_ptr(|getter: *mut crate::closure::ClosureHeader| {
            crate::closure::js_closure_set_capture_f64(getter, 0, value.get_nanbox_f64());
        });
    }
    let setter = set_func.map(|func| {
        crate::closure::js_register_closure_arity(func, 1);
        scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func, 0))
    });
    crate::object::AccessorDescriptor {
        get: getter
            .with_mut_ptr(|getter: *mut crate::closure::ClosureHeader| {
                crate::value::js_nanbox_pointer(getter as i64)
            })
            .to_bits(),
        set: setter
            .map(|closure| {
                closure
                    .with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                        crate::value::js_nanbox_pointer(closure as i64)
                    })
                    .to_bits()
            })
            .unwrap_or(0),
    }
}

fn module_cjs_prototype_value(_module_value: f64) -> f64 {
    MODULE_CJS_PROTOTYPE_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let keys = b"_compile\0constructor\0isPreloading\0load\0parent\0require\0";
        let proto = scope.root_nanbox_f64(native_object_value(
            crate::object::js_object_alloc_with_shape(
                0xC0_00_4E,
                6,
                keys.as_ptr(),
                keys.len() as u32,
            ),
        ));
        // Publish before any nested closure/string allocation so the moving GC
        // can update both the shared singleton and this local construction.
        store_module_cjs_root(slot, proto.get_nanbox_f64());
        let undefined = JSValue::from_bits(crate::value::TAG_UNDEFINED);
        for index in 0..6 {
            crate::object::js_object_set_field(
                crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as *mut ObjectHeader,
                index,
                undefined,
            );
        }
        for (index, name, length) in [(0, "_compile", 3), (3, "load", 1), (5, "require", 1)] {
            let method = scope.root_nanbox_f64(module_prototype_method(name, length));
            let proto_ptr =
                crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as *mut ObjectHeader;
            crate::object::js_object_set_field(
                proto_ptr,
                index,
                JSValue::from_bits(method.get_nanbox_f64().to_bits()),
            );
            crate::object::set_property_attrs(
                proto_ptr as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, true, true),
            );
        }
        let install_accessor = |name: &str, descriptor| {
            let proto_ptr = crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as usize;
            crate::object::set_accessor_descriptor(proto_ptr, name.to_string(), descriptor);
            crate::object::set_property_attrs(
                proto_ptr,
                name.to_string(),
                crate::object::PropertyAttrs::new(false, false, false),
            );
        };
        install_accessor(
            "constructor",
            module_accessor(module_prototype_constructor_getter as *const u8, None, None),
        );
        install_accessor(
            "isPreloading",
            module_accessor(module_prototype_false_getter as *const u8, None, None),
        );
        install_accessor(
            "parent",
            module_accessor(
                module_prototype_parent_getter as *const u8,
                Some(module_prototype_parent_setter as *const u8),
                None,
            ),
        );
        store_module_cjs_root(slot, proto.get_nanbox_f64())
    })
}

fn current_module_cjs_prototype_value() -> Option<f64> {
    MODULE_CJS_PROTOTYPE_VALUE.with(|slot| (slot.get() != 0).then(|| f64::from_bits(slot.get())))
}

pub(crate) fn module_cjs_prototype_for_instance() -> f64 {
    if let Some(prototype) = current_module_cjs_prototype_value() {
        return prototype;
    }
    let module = bound_native_callable_export_value("module", "Module");
    current_module_cjs_prototype_value().unwrap_or_else(|| module_cjs_prototype_value(module))
}

pub(super) fn attach_module_cjs_constructor_statics(module_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_nanbox_f64(module_value);
    macro_rules! set_static {
        ($name:expr, $value:expr) => {{
            let property = scope.root_nanbox_f64($value);
            crate::closure::closure_set_dynamic_prop(
                crate::value::js_nanbox_get_pointer(module.get_nanbox_f64()) as usize,
                $name,
                property.get_nanbox_f64(),
            );
        }};
    }
    set_static!("Module", module.get_nanbox_f64());
    set_static!("_cache", module_cjs_cache_value());
    set_static!("_extensions", module_cjs_extensions_value());
    set_static!("_pathCache", module_cjs_path_cache_value());
    set_static!("globalPaths", module_cjs_global_paths_value());
    set_static!("builtinModules", module_builtin_modules_value());
    set_static!("constants", module_constants_value());
    for name in [
        "SourceMap",
        "_findPath",
        "_initPaths",
        "_load",
        "_nodeModulePaths",
        "_preloadModules",
        "_resolveFilename",
        "_resolveLookupPaths",
        "createRequire",
        "enableCompileCache",
        "findPackageJSON",
        "findSourceMap",
        "flushCompileCache",
        "getCompileCacheDir",
        "getSourceMapsSupport",
        "isBuiltin",
        "register",
        "registerHooks",
        "runMain",
        "setSourceMapsSupport",
        "stripTypeScriptTypes",
        "syncBuiltinESMExports",
    ] {
        set_static!(name, bound_native_callable_export_value("module", name));
    }
    set_static!("wrap", module_wrap_value());
    set_static!("wrapper", module_wrapper_value());
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    set_static!("_readPackage", undefined);
    set_static!("_stat", undefined);
    for name in ["wrap", "wrapper"] {
        crate::object::set_property_attrs(
            crate::value::js_nanbox_get_pointer(module.get_nanbox_f64()) as usize,
            name.to_string(),
            crate::object::PropertyAttrs::new(false, false, false),
        );
    }
    set_static!(
        "prototype",
        module_cjs_prototype_value(module.get_nanbox_f64())
    );
    module.get_nanbox_f64()
}
