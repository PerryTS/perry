//! `WebAssembly.Table` support: the JS table object, its `get`/`set`/`grow`
//! methods, and funcref-entry wrapping. Split out of `webassembly.rs` to keep it
//! under the 2,000-line cap (#10750); pure relocation.

use super::*;

pub(super) fn table_method_context<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    closure: *const crate::closure::ClosureHeader,
) -> (
    *mut c_void,
    *mut c_void,
    crate::gc::RuntimeHandle<'scope>,
    crate::gc::RuntimeHandle<'scope>,
) {
    let external = crate::closure::js_closure_get_capture_f64(closure, 0) as usize as *mut c_void;
    let inst = crate::closure::js_closure_get_capture_f64(closure, 1) as usize as *mut c_void;
    let name = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    let table = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 3));
    (external, inst, name, table)
}

pub(super) fn table_values(table: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let table = scope.root_nanbox_f64(table);
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    if !table_value.is_pointer() {
        return nanbox_undefined();
    }
    let key = scope.root_string_ptr(named_key(b"__wasmValues"));
    key.with_const_ptr(|key: *const crate::string::StringHeader| {
        crate::object::js_object_get_field_by_name_f64(
            JSValue::from_bits(table.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>(),
            key,
        )
    })
}

pub(super) fn wasm_function_external(value: f64) -> *mut c_void {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return std::ptr::null_mut();
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(closure as usize) })
    else {
        return std::ptr::null_mut();
    };
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE {
        return std::ptr::null_mut();
    }
    let fp = unsafe { (*closure).func_ptr };
    if !is_wasm_export_call_shim(fp) {
        return std::ptr::null_mut();
    }
    crate::closure::js_closure_get_capture_f64(closure, 6) as usize as *mut c_void
}

extern "C" fn js_wasm_table_get(closure: *const crate::closure::ClosureHeader, index: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
    let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
    if !values_value.is_pointer() || !index.is_finite() || index < 0.0 {
        return nanbox_undefined();
    }
    let index = index as usize;
    let cached = crate::array::js_array_get_f64(
        values_value.as_pointer::<crate::array::ArrayHeader>(),
        index as u32,
    );
    if cached.to_bits() != crate::value::TAG_NULL {
        return cached;
    }
    let mut bits = 0u64;
    let mut is_null = 0i32;
    let mut function_external = std::ptr::null_mut();
    let ok = if inst.is_null() {
        unsafe {
            perry_wasm_host_table_get(
                external,
                index,
                &mut bits,
                &mut is_null,
                &mut function_external,
            )
        }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_get(
                inst,
                name_ptr.cast(),
                name_len,
                index,
                &mut bits,
                &mut is_null,
                &mut function_external,
            )
        }
    } else {
        0
    };
    if ok == 0 {
        return nanbox_undefined();
    }
    let value = if is_null != 0 {
        f64::from_bits(crate::value::TAG_NULL)
    } else if !function_external.is_null() {
        make_table_function(function_external)
    } else {
        f64::from_bits(bits)
    };
    let values_ptr = JSValue::from_bits(values.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>()
        as *mut crate::array::ArrayHeader;
    crate::array::js_array_set_f64(values_ptr, index as u32, value);
    value
}

extern "C" fn js_wasm_table_set(
    closure: *const crate::closure::ClosureHeader,
    index: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let is_null = (value.to_bits() == crate::value::TAG_NULL) as i32;
    let index = index.max(0.0) as usize;
    let function = wasm_function_external(value);
    let ok = if inst.is_null() {
        unsafe { perry_wasm_host_table_set(external, index, value.to_bits(), is_null, function) }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_set(
                inst,
                name_ptr.cast(),
                name_len,
                index,
                value.to_bits(),
                is_null,
                function,
            )
        }
    } else {
        0
    };
    if ok != 0 {
        let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
        let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
        if values_value.is_pointer() {
            let values_ptr = values_value.as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            crate::array::js_array_set_f64(values_ptr, index as u32, value);
        }
    } else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Table.set(): value is not a WebAssembly function",
        ));
    }
    nanbox_undefined()
}

extern "C" fn js_wasm_table_grow(
    closure: *const crate::closure::ClosureHeader,
    delta: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let value_bits = value.to_bits();
    let is_null = matches!(
        value_bits,
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) as i32;
    let mut old_len = 0usize;
    let delta = delta.max(0.0) as usize;
    let function = wasm_function_external(value);
    let ok = if inst.is_null() {
        unsafe {
            perry_wasm_host_table_grow(external, delta, value_bits, is_null, function, &mut old_len)
        }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_grow(
                inst,
                name_ptr.cast(),
                name_len,
                delta,
                value_bits,
                is_null,
                function,
                &mut old_len,
            )
        }
    } else {
        0
    };
    if ok == 0 {
        return nanbox_undefined();
    }
    let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
    let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
    if !values_value.is_pointer() {
        return nanbox_undefined();
    }
    let mut values_ptr =
        values_value.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
    let fill = if is_null != 0 {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        value
    };
    for _ in 0..delta {
        values_ptr = crate::array::js_array_push_f64(values_ptr, fill);
        values.set_nanbox_f64(array_value(values_ptr));
    }
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    if table_value.is_pointer() {
        let _ = object_set(
            table_value.as_pointer::<crate::object::ObjectHeader>()
                as *mut crate::object::ObjectHeader,
            b"length",
            old_len.saturating_add(delta) as f64,
        );
    }
    old_len as f64
}

pub(super) fn make_table_method(
    external: *mut c_void,
    inst: *mut c_void,
    name: f64,
    table: f64,
    func_ptr: *const u8,
    arity: u32,
    display_name: &str,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let table = scope.root_nanbox_f64(table);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 4));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, external as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 1, inst as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 2, name.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 3, table.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, display_name)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

pub(super) fn make_table_function(external: *mut c_void) -> f64 {
    let arity = unsafe { perry_wasm_host_func_arity(external) };
    if arity == usize::MAX {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    let (func_ptr, declared_arity) = wasm_export_call_shim_for_arity(arity);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 7));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, declared_arity);
    for index in 0..6 {
        closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
            crate::closure::js_closure_set_capture_f64(closure, index, nanbox_undefined())
        });
    }
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 5, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 6, external as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, "wasm-table-function")
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::register_wasm_funcref_external(closure as usize, external as usize);
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

pub(super) fn make_table_object(
    external: *mut c_void,
    inst: *mut c_void,
    name: f64,
    receiver: f64,
) -> f64 {
    if external.is_null() {
        return nanbox_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let receiver = JSValue::from_bits(receiver.to_bits());
    let table_ptr = if receiver.is_pointer() {
        receiver.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader
    } else {
        crate::object::js_object_alloc(0, 0)
    };
    let table = scope.root_nanbox_f64(object_value(table_ptr));
    let len = if inst.is_null() {
        unsafe { perry_wasm_host_table_len(external) }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe { perry_wasm_host_instance_table_len(inst, name_ptr.cast(), name_len) }
    } else {
        usize::MAX
    };
    if len == usize::MAX {
        return nanbox_undefined();
    }
    let values = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len as u32)));
    for _ in 0..len {
        let values_ptr = JSValue::from_bits(values.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        let values_ptr =
            crate::array::js_array_push_f64(values_ptr, f64::from_bits(crate::value::TAG_NULL));
        values.set_nanbox_f64(array_value(values_ptr));
    }
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let _ = object_set(table_ptr, b"__wasmValues", values.get_nanbox_f64());
    let methods = [
        ("get", js_wasm_table_get as *const u8, 1u32),
        ("grow", js_wasm_table_grow as *const u8, 2u32),
        ("set", js_wasm_table_set as *const u8, 2u32),
    ];
    for (method_name, func_ptr, arity) in methods {
        let method = scope.root_nanbox_f64(make_table_method(
            external,
            inst,
            name.get_nanbox_f64(),
            table.get_nanbox_f64(),
            func_ptr,
            arity,
            method_name,
        ));
        let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
            .as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        let _ = object_set(table_ptr, method_name.as_bytes(), method.get_nanbox_f64());
    }
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let _ = object_set(table_ptr, b"length", len as f64);
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>();
    crate::object::register_wasm_extern_wrapper(table_ptr as usize, b"table", external as usize);
    table.get_nanbox_f64()
}

pub(super) fn make_export_table(inst: *mut c_void, name: &[u8]) -> f64 {
    let name_value = string_value(name);
    let external =
        unsafe { perry_wasm_host_instance_export_extern(inst, name.as_ptr().cast(), name.len()) };
    make_table_object(external, inst, name_value, nanbox_undefined())
}
