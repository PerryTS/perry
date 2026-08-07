//! Experimental `vm.Module` / `SourceTextModule` / `SyntheticModule` lifecycle.
//! Extracted from `node_vm.rs` to stay under the 2000-line file-size gate.

use super::*;

fn evaluate_source_module(module: *mut ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let status = module_status(hmut::<ObjectHeader>(&module));
    if status != STATUS_LINKED && status != STATUS_EVALUATED {
        return throw_vm_status("Module status must be linked");
    }
    if status == STATUS_EVALUATED {
        return undefined_value();
    }

    set_status(hmut::<ObjectHeader>(&module), STATUS_EVALUATING);
    let Some(namespace) = namespace_for_module(hmut::<ObjectHeader>(&module)) else {
        set_status(hmut::<ObjectHeader>(&module), STATUS_ERRORED);
        return throw_vm_status("Module namespace is unavailable");
    };
    let namespace = scope.root_raw_mut_ptr(namespace);

    let source = get_string_field(hmut::<ObjectHeader>(&module), FIELD_SOURCE).unwrap_or_default();
    let context = scope.root_nanbox_f64(get_field(hmut::<ObjectHeader>(&module), FIELD_CONTEXT));
    for (name, value) in build_import_env(hmut::<ObjectHeader>(&module)) {
        set_object_field(context.get_nanbox_f64(), &name, value);
    }
    let executable = split_source_statements(&source)
        .into_iter()
        .filter(|stmt| !stmt.starts_with("import "))
        .map(|stmt| stmt.strip_prefix("export ").unwrap_or(&stmt).to_string())
        .collect::<Vec<_>>()
        .join(";");
    let lexical = scope.root_nanbox_f64(de::script_environment(context.get_nanbox_f64(), &[]));
    if let Err(error) = crate::exception::js_call_catching(|| {
        de::eval_script_in(
            &executable,
            context.get_nanbox_f64(),
            context.get_nanbox_f64(),
            lexical.get_nanbox_f64(),
        )
    }) {
        set_field(hmut::<ObjectHeader>(&module), FIELD_ERROR, error);
        set_status(hmut::<ObjectHeader>(&module), STATUS_ERRORED);
        return error;
    }
    for export in read_exports(hmut::<ObjectHeader>(&module)) {
        set_field(
            hmut::<ObjectHeader>(&namespace),
            &export.name,
            de::script_binding(lexical.get_nanbox_f64(), &export.name),
        );
    }
    set_status(hmut::<ObjectHeader>(&module), STATUS_EVALUATED);
    undefined_value()
}

fn evaluate_synthetic_module(module: *mut ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let status = module_status(hmut::<ObjectHeader>(&module));
    if status == STATUS_EVALUATED {
        return undefined_value();
    }
    if status != STATUS_LINKED {
        return throw_vm_status("Module status must be linked");
    }

    set_status(hmut::<ObjectHeader>(&module), STATUS_EVALUATING);
    let callback = scope.root_nanbox_f64(get_field(
        hmut::<ObjectHeader>(&module),
        FIELD_EVALUATE_CALLBACK,
    ));
    let js = JSValue::from_bits(callback.get_nanbox_f64().to_bits());
    if !js.is_undefined() && !js.is_null() {
        let prev = crate::object::js_implicit_this_set(object_value(hmut::<ObjectHeader>(&module)));
        let outcome = crate::exception::js_call_catching(|| unsafe {
            crate::closure::js_native_call_value(callback.get_nanbox_f64(), std::ptr::null(), 0)
        });
        crate::object::js_implicit_this_set(prev);
        if let Err(error) = outcome {
            set_field(hmut::<ObjectHeader>(&module), FIELD_ERROR, error);
            set_status(hmut::<ObjectHeader>(&module), STATUS_ERRORED);
            return error;
        }
    }
    set_status(hmut::<ObjectHeader>(&module), STATUS_EVALUATED);
    undefined_value()
}

fn module_has_tla(module: *mut ObjectHeader) -> bool {
    let Some(source) = get_string_field(module, FIELD_SOURCE) else {
        return false;
    };
    parse_source(&source).has_top_level_await
}

fn module_has_async_graph(module: *mut ObjectHeader) -> bool {
    let mut visited = std::collections::HashSet::new();
    module_has_async_graph_inner(module, &mut visited)
}

fn module_has_async_graph_inner(
    module: *mut ObjectHeader,
    visited: &mut std::collections::HashSet<usize>,
) -> bool {
    if !visited.insert(module as usize) {
        return false;
    }
    if module_has_tla(module) {
        return true;
    }
    let Some(linked) = module_linked_modules(module) else {
        return false;
    };
    let len = crate::array::js_array_length(linked);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(linked, idx);
        if let Some(dep) = object_ptr_from_value(value) {
            if module_has_async_graph_inner(dep, visited) {
                return true;
            }
        }
    }
    false
}

fn new_module_base(kind: &str, status: &str, identifier: String) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = crate::object::js_object_alloc(0, 16);
    let module = scope.root_raw_mut_ptr(module);
    let value = string_value(kind);
    set_field(hmut::<ObjectHeader>(&module), FIELD_KIND, value);
    let value = string_value(status);
    set_field(hmut::<ObjectHeader>(&module), FIELD_STATUS, value);
    let value = string_value(status);
    set_field(hmut::<ObjectHeader>(&module), "status", value);
    let value = string_value(&identifier);
    set_field(hmut::<ObjectHeader>(&module), FIELD_IDENTIFIER, value);
    let value = string_value(&identifier);
    set_field(hmut::<ObjectHeader>(&module), "identifier", value);
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_ERROR,
        undefined_value(),
    );
    set_field(hmut::<ObjectHeader>(&module), "error", undefined_value());
    let value = array_value(crate::array::js_array_alloc(0));
    set_field(hmut::<ObjectHeader>(&module), FIELD_LINKED_MODULES, value);
    hmut::<ObjectHeader>(&module)
}

extern "C" fn module_namespace_getter(closure: *const ClosureHeader) -> f64 {
    js_vm_module_namespace(crate::closure::js_closure_get_capture_f64(closure, 0))
}

extern "C" fn module_error_getter(closure: *const ClosureHeader) -> f64 {
    js_vm_module_error(crate::closure::js_closure_get_capture_f64(closure, 0))
}

fn install_module_accessor(
    module: *mut ObjectHeader,
    name: &str,
    getter: extern "C" fn(*const ClosureHeader) -> f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let closure = crate::closure::js_closure_alloc(getter as *const u8, 1);
    let closure = scope.root_raw_mut_ptr(closure);
    crate::closure::js_register_closure_arity(getter as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(
        hmut::<ClosureHeader>(&closure),
        0,
        object_value(hmut::<ObjectHeader>(&module)),
    );
    unsafe {
        crate::closure::rebuild_closure_layout_and_barriers(hmut::<ClosureHeader>(&closure), 1);
    }
    set_field(hmut::<ObjectHeader>(&module), name, undefined_value());
    crate::object::set_builtin_accessor_descriptor(
        hmut::<ObjectHeader>(&module) as usize,
        name.to_string(),
        crate::object::AccessorDescriptor {
            get: crate::value::js_nanbox_pointer(hmut::<ClosureHeader>(&closure) as i64).to_bits(),
            set: 0,
        },
        PropertyAttrs::new(false, false, false),
    );
}

fn set_module_namespace_tag(namespace: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let namespace = scope.root_raw_mut_ptr(namespace);
    let symbol = crate::symbol::well_known_symbol("toStringTag");
    if symbol.is_null() {
        return;
    }
    let symbol = scope.root_raw_mut_ptr(symbol);
    let tag = scope.root_nanbox_f64(string_value("Module"));
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            object_value(hmut::<ObjectHeader>(&namespace)),
            crate::value::js_nanbox_pointer(hmut::<crate::symbol::SymbolHeader>(&symbol) as i64),
            tag.get_nanbox_f64(),
        );
    }
}

pub extern "C" fn js_vm_module_call() -> f64 {
    throw_type_error_no_code("Class constructor Module cannot be invoked without 'new'")
}

#[no_mangle]
pub extern "C" fn js_vm_module_constructor_error() -> f64 {
    throw_type_error_no_code("Module is not a constructor")
}

pub extern "C" fn js_vm_source_text_module_new(code: f64, options: f64) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SourceTextModule experimental gate", "3132");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let source = code_string_required(code, "code");
    validate_module_source(&source);
    let (context, identifier) = module_options(options.get_nanbox_f64());
    let context = scope.root_nanbox_f64(context);
    let hash = source_hash(CACHE_KIND_MODULE, &source, &[]);
    if let Some(bytes) = validate_cached_data_option(options.get_nanbox_f64()) {
        if !cache_bytes_accepted(&bytes, CACHE_KIND_MODULE, hash) {
            return throw_vm_module_cached_data_rejected();
        }
    }
    let parsed = parse_source(&source);
    let module = new_module_base(KIND_SOURCE, STATUS_UNLINKED, identifier);
    let module = scope.root_raw_mut_ptr(module);
    let namespace = crate::object::js_object_alloc_null_proto(0, parsed.exports.len() as u32);
    let namespace = scope.root_raw_mut_ptr(namespace);
    set_module_namespace_tag(hmut::<ObjectHeader>(&namespace));
    for export in &parsed.exports {
        set_field(
            hmut::<ObjectHeader>(&namespace),
            &export.name,
            undefined_value(),
        );
    }
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_NAMESPACE,
        object_value(hmut::<ObjectHeader>(&namespace)),
    );
    install_module_accessor(
        hmut::<ObjectHeader>(&module),
        "namespace",
        module_namespace_getter,
    );
    install_module_accessor(hmut::<ObjectHeader>(&module), "error", module_error_getter);
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_CONTEXT,
        context.get_nanbox_f64(),
    );
    let value = string_value(&source);
    set_field(hmut::<ObjectHeader>(&module), FIELD_SOURCE, value);
    let value = requests_array(&parsed.requests);
    set_field(hmut::<ObjectHeader>(&module), FIELD_REQUESTS, value);
    let value = strings_array(&parsed.requests);
    set_field(hmut::<ObjectHeader>(&module), "dependencySpecifiers", value);
    let value = requests_array(&parsed.requests);
    set_field(hmut::<ObjectHeader>(&module), "moduleRequests", value);
    let value = imports_array(&parsed.imports);
    set_field(hmut::<ObjectHeader>(&module), FIELD_IMPORTS, value);
    let value = exports_array(&parsed.exports);
    set_field(hmut::<ObjectHeader>(&module), FIELD_EXPORTS, value);
    object_value(hmut::<ObjectHeader>(&module))
}

pub extern "C" fn js_vm_synthetic_module_new(
    export_names_value: f64,
    evaluate_callback: f64,
    options: f64,
) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SyntheticModule experimental gate", "3133");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let export_names_value = scope.root_nanbox_f64(export_names_value);
    let evaluate_callback = scope.root_nanbox_f64(evaluate_callback);
    let options = scope.root_nanbox_f64(options);
    let Some(export_names) = array_ptr_from_value(export_names_value.get_nanbox_f64()) else {
        let message = format!(
            "The \"exportNames\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(export_names_value.get_nanbox_f64())
        );
        throw_invalid_arg(&message);
    };
    let export_names = scope.root_raw_mut_ptr(export_names);
    if !crate::object::value_is_callable(evaluate_callback.get_nanbox_f64()) {
        let message = format!(
            "The \"evaluateCallback\" argument must be of type function. Received {}",
            crate::fs::validate::describe_received(evaluate_callback.get_nanbox_f64())
        );
        throw_invalid_arg(&message);
    }
    let (context, identifier) = module_options(options.get_nanbox_f64());
    let context = scope.root_nanbox_f64(context);
    let module = new_module_base(KIND_SYNTHETIC, STATUS_LINKED, identifier);
    let module = scope.root_raw_mut_ptr(module);
    let namespace = crate::object::js_object_alloc_null_proto(0, 0);
    let namespace = scope.root_raw_mut_ptr(namespace);
    set_module_namespace_tag(hmut::<ObjectHeader>(&namespace));
    let len = crate::array::js_array_length(hmut::<ArrayHeader>(&export_names));
    let mut exports = Vec::new();
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(hmut::<ArrayHeader>(&export_names), idx);
        let Some(name) = string_from_value(value) else {
            let message = format!(
                "The \"exportNames[{idx}]\" argument must be of type string. Received {}",
                crate::fs::validate::describe_received(value)
            );
            throw_invalid_arg(&message);
        };
        exports.push(ExportBinding {
            name: name.clone(),
            expr: String::new(),
        });
        set_field(hmut::<ObjectHeader>(&namespace), &name, undefined_value());
    }
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_NAMESPACE,
        object_value(hmut::<ObjectHeader>(&namespace)),
    );
    install_module_accessor(
        hmut::<ObjectHeader>(&module),
        "namespace",
        module_namespace_getter,
    );
    install_module_accessor(hmut::<ObjectHeader>(&module), "error", module_error_getter);
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_CONTEXT,
        context.get_nanbox_f64(),
    );
    let value = requests_array(&[]);
    set_field(hmut::<ObjectHeader>(&module), FIELD_REQUESTS, value);
    let value = imports_array(&[]);
    set_field(hmut::<ObjectHeader>(&module), FIELD_IMPORTS, value);
    let value = exports_array(&exports);
    set_field(hmut::<ObjectHeader>(&module), FIELD_EXPORTS, value);
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_EVALUATE_CALLBACK,
        evaluate_callback.get_nanbox_f64(),
    );
    object_value(hmut::<ObjectHeader>(&module))
}

pub extern "C" fn js_vm_module_status(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    string_value(&module_status(module))
}

pub extern "C" fn js_vm_module_identifier(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    get_field(module, FIELD_IDENTIFIER)
}

pub extern "C" fn js_vm_module_error(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_status(module) != STATUS_ERRORED {
        return throw_vm_status("Module status must be errored");
    }
    get_field(module, FIELD_ERROR)
}

pub extern "C" fn js_vm_module_namespace(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let status = module_status(module);
    if module_kind(module) == KIND_SOURCE && (status == STATUS_UNLINKED || status == STATUS_LINKING)
    {
        return throw_vm_status("Module status must be linked");
    }
    get_field(module, FIELD_NAMESPACE)
}

pub extern "C" fn js_vm_module_link(module_value: f64, linker: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let module = scope.root_raw_mut_ptr(module);
    let module_value = scope.root_nanbox_f64(module_value);
    let linker = scope.root_nanbox_f64(linker);
    if module_kind(hmut::<ObjectHeader>(&module)) == KIND_SYNTHETIC {
        set_status(hmut::<ObjectHeader>(&module), STATUS_LINKED);
        return undefined_value();
    }
    if module_status(hmut::<ObjectHeader>(&module)) != STATUS_UNLINKED {
        return undefined_value();
    }

    set_status(hmut::<ObjectHeader>(&module), STATUS_LINKING);
    let requests = read_requests(hmut::<ObjectHeader>(&module));
    let mut linked = crate::array::js_array_alloc(requests.len() as u32);
    for specifier in &requests {
        let args = [
            string_value(specifier),
            module_value.get_nanbox_f64(),
            module_request_extra(),
        ];
        let dep = unsafe {
            crate::closure::js_native_call_value(linker.get_nanbox_f64(), args.as_ptr(), args.len())
        };
        linked = crate::array::js_array_push_f64(linked, dep);
    }
    set_field(
        hmut::<ObjectHeader>(&module),
        FIELD_LINKED_MODULES,
        array_value(linked),
    );
    set_status(hmut::<ObjectHeader>(&module), STATUS_LINKED);
    undefined_value()
}

pub extern "C" fn js_vm_module_evaluate(module_value: f64, _options: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let result = match module_kind(module).as_str() {
        KIND_SOURCE => evaluate_source_module(module),
        KIND_SYNTHETIC => evaluate_synthetic_module(module),
        _ => undefined_value(),
    };
    let promise = if module_status(module) == STATUS_ERRORED {
        crate::promise::js_promise_rejected(result)
    } else {
        crate::promise::js_promise_resolved(result)
    };
    crate::value::js_nanbox_pointer(promise as i64)
}

pub extern "C" fn js_vm_source_text_module_dependency_specifiers(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return array_value(crate::array::js_array_alloc(0));
    };
    strings_array(&read_requests(module))
}

pub extern "C" fn js_vm_source_text_module_module_requests(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return array_value(crate::array::js_array_alloc(0));
    };
    let requests = read_requests(module);
    requests_array(&requests)
}

pub extern "C" fn js_vm_source_text_module_create_cached_data(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return cached_data_buffer(CACHE_KIND_MODULE, 0);
    };
    if module_status(module) == STATUS_EVALUATED {
        return throw_vm_module_cannot_create_cached_data();
    }
    let source = get_string_field(module, FIELD_SOURCE).unwrap_or_default();
    cached_data_buffer(
        CACHE_KIND_MODULE,
        source_hash(CACHE_KIND_MODULE, &source, &[]),
    )
}

pub extern "C" fn js_vm_source_text_module_link_requests(
    module_value: f64,
    modules_value: f64,
) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let Some(modules) = array_ptr_from_value(modules_value) else {
        return throw_vm_type("linkRequests modules must be an array");
    };
    set_field(module, FIELD_LINKED_MODULES, array_value(modules));
    undefined_value()
}

pub extern "C" fn js_vm_source_text_module_instantiate(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_status(module) == STATUS_UNLINKED {
        set_status(module, STATUS_LINKED);
    }
    undefined_value()
}

pub extern "C" fn js_vm_source_text_module_has_top_level_await(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return bool_value(false);
    };
    bool_value(module_has_tla(module))
}

pub extern "C" fn js_vm_source_text_module_has_async_graph(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return bool_value(false);
    };
    if module_status(module) == STATUS_UNLINKED {
        return throw_vm_status("Module status must be instantiated");
    }
    bool_value(module_has_async_graph(module))
}

pub extern "C" fn js_vm_synthetic_module_set_export(
    module_value: f64,
    name_value: f64,
    value: f64,
) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_kind(module) != KIND_SYNTHETIC {
        return throw_vm_type("setExport is only supported on SyntheticModule");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let value = scope.root_nanbox_f64(value);
    let Some(name) = string_from_value(name_value) else {
        return throw_vm_type("SyntheticModule export name must be a string");
    };
    let exports = read_exports(hmut::<ObjectHeader>(&module));
    if !exports.iter().any(|export| export.name == name) {
        return throw_reference_error_no_code(&format!("Export '{name}' is not defined in module"));
    }
    let Some(namespace) = namespace_for_module(hmut::<ObjectHeader>(&module)) else {
        return throw_vm_status("SyntheticModule namespace is unavailable");
    };
    set_field(namespace, &name, value.get_nanbox_f64());
    undefined_value()
}
