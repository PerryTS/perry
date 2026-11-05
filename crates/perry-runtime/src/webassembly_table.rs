//! `WebAssembly.Table` support: the JS table object, its `get`/`set`/`grow`
//! methods, and funcref-entry wrapping. Split out of `webassembly.rs` to keep it
//! under the 2,000-line cap (#10750); pure relocation.

use super::*;

/// Slots of the per-table state array kept in the wrapper's private
/// `owned_state_bag::WASM_TABLE` slot (never an own property of the table).
const STATE_VALUES: u32 = 0;
const STATE_INST: u32 = 1;
const STATE_NAME: u32 = 2;

fn table_state(table: f64, index: u32) -> f64 {
    let table = JSValue::from_bits(table.to_bits());
    if !table.is_pointer() {
        return nanbox_undefined();
    }
    let state = unsafe {
        crate::object::owned_state_bag::load(
            table.as_pointer::<u8>() as usize,
            crate::object::owned_state_bag::WASM_TABLE,
        )
    };
    let state = JSValue::from_bits(state.to_bits());
    if !state.is_pointer() {
        return nanbox_undefined();
    }
    crate::array::js_array_get_f64(state.as_pointer::<crate::array::ArrayHeader>(), index)
}

/// The host state of the table `this` names: `None` when `this` is not a
/// table wrapper this runtime produced (its record holds no table handle).
fn table_context<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    this: f64,
) -> Option<(
    *mut c_void,
    *mut c_void,
    crate::gc::RuntimeHandle<'scope>,
    crate::gc::RuntimeHandle<'scope>,
)> {
    let receiver = JSValue::from_bits(this.to_bits());
    if !receiver.is_pointer() {
        return None;
    }
    let external = crate::object::registered_extern_handle(
        receiver.as_pointer::<crate::object::ObjectHeader>() as usize,
        b"table",
    )? as *mut c_void;
    let inst = table_state(this, STATE_INST) as usize as *mut c_void;
    let name = scope.root_nanbox_f64(table_state(this, STATE_NAME));
    let table = scope.root_nanbox_f64(this);
    Some((external, inst, name, table))
}

fn table_receiver_error(api: &str) -> ! {
    crate::exception::throw_core(wasm_type_error(&format!(
        "WebAssembly.Table.{api}(): Receiver is not a WebAssembly.Table"
    )))
}

fn set_table_state(table: f64, index: u32, value: f64) {
    let table = JSValue::from_bits(table.to_bits());
    if !table.is_pointer() {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let state = crate::object::owned_state_bag::load(
            table.as_pointer::<u8>() as usize,
            crate::object::owned_state_bag::WASM_TABLE,
        );
        let state = JSValue::from_bits(state.to_bits());
        if state.is_pointer() {
            crate::array::js_array_set_f64(
                state.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader,
                index,
                value,
            );
        }
    }
}

pub(super) fn table_values(table: f64) -> f64 {
    table_state(table, STATE_VALUES)
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
    let fp = unsafe { (*closure).code() };
    if !is_wasm_export_call_shim(fp) {
        return std::ptr::null_mut();
    }
    crate::closure::wasm_funcref::external_from_capture(crate::closure::js_closure_get_capture_f64(
        closure, 6,
    ))
}

/// `WebAssembly.Table.prototype.get` (reached through the engine hook).
pub(super) extern "C" fn js_webassembly_table_get(this: f64, index: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some((external, inst, name, table)) = table_context(&scope, this) else {
        table_receiver_error("get")
    };
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
        make_table_function(
            function_external,
            crate::wasm_instance_state::from_owner(table.get_nanbox_f64()),
        )
    } else {
        f64::from_bits(bits)
    };
    let values_ptr = JSValue::from_bits(values.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>()
        as *mut crate::array::ArrayHeader;
    crate::array::js_array_set_f64(values_ptr, index as u32, value);
    value
}

/// `WebAssembly.Table.prototype.set`.
pub(super) extern "C" fn js_webassembly_table_set(this: f64, index: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some((external, inst, name, table)) = table_context(&scope, this) else {
        table_receiver_error("set")
    };
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
        crate::exception::throw_core(wasm_type_error(
            "WebAssembly.Table.set(): value is not a WebAssembly function",
        ));
    }
    nanbox_undefined()
}

/// `WebAssembly.Table.prototype.grow`.
pub(super) extern "C" fn js_webassembly_table_grow(this: f64, delta: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some((external, inst, name, table)) = table_context(&scope, this) else {
        table_receiver_error("grow")
    };
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
    set_table_state(
        table.get_nanbox_f64(),
        STATE_VALUES,
        values.get_nanbox_f64(),
    );
    old_len as f64
}

/// The `length` getter of `WebAssembly.Table.prototype`: the host table's
/// current size, read on every call (wasm code can grow the table too).
pub(super) extern "C" fn js_webassembly_table_length(this: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some((external, inst, name, _table)) = table_context(&scope, this) else {
        table_receiver_error("length")
    };
    let len = if inst.is_null() {
        unsafe { perry_wasm_host_table_len(external) }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe { perry_wasm_host_instance_table_len(inst, name_ptr.cast(), name_len) }
    } else {
        usize::MAX
    };
    if len == usize::MAX {
        0.0
    } else {
        len as f64
    }
}

pub(super) fn make_table_function(external: *mut c_void, context: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let context = scope.root_nanbox_f64(context);
    let arity = unsafe { perry_wasm_host_func_arity(external) };
    if arity == usize::MAX {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    let (info, _declared_arity) = wasm_export_call_shim_for_arity(arity);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 7));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    for index in 0..6 {
        closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
            crate::closure::js_closure_set_capture_f64(closure, index, nanbox_undefined())
        });
    }
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 4, context.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 5, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 6, nanbox_undefined())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, "wasm-table-function")
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::wasm_funcref::register_wasm_funcref_external(
            closure as usize,
            external as usize,
        );
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

/// Publish the state of a table wrapper: `[values, inst, export name]` in the
/// private `WASM_TABLE` slot of its owned-state bag (traced by the GC, never
/// an own property). The wrapper is born by the caller as an instance of
/// `WebAssembly.Table`; its members are `Table.prototype`'s.
fn publish_table_state(table: f64, values: f64, inst: *mut c_void, name: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let table = scope.root_nanbox_f64(table);
    let values = scope.root_nanbox_f64(values);
    let name = scope.root_nanbox_f64(name);
    let state = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc_with_length(3)));
    let state_ptr = JSValue::from_bits(state.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
    crate::array::js_array_set_f64(state_ptr, STATE_VALUES, values.get_nanbox_f64());
    crate::array::js_array_set_f64(state_ptr, STATE_INST, inst as usize as f64);
    crate::array::js_array_set_f64(state_ptr, STATE_NAME, name.get_nanbox_f64());
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    unsafe {
        crate::object::owned_state_bag::store(
            table_value.as_pointer::<u8>() as usize,
            crate::object::owned_state_bag::WASM_TABLE,
            state.get_nanbox_f64(),
        );
    }
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
    let table = scope.root_nanbox_f64(wrapper_birth(b"Table", receiver));
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
    publish_table_state(
        table.get_nanbox_f64(),
        values.get_nanbox_f64(),
        inst,
        name.get_nanbox_f64(),
    );
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
