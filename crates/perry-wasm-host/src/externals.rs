//! Shared WebAssembly externals used by Emscripten main/side-module graphs.

use super::*;

/// Return an opaque, store-stable handle for an instance export. It is used
/// when another WebAssembly instance imports that JS-visible export.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_export_extern(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
) -> *mut c_void {
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return std::ptr::null_mut();
    };
    inst.inner
        .instance
        .get_export(inst.store(), name)
        .map(extern_handle)
        .unwrap_or(std::ptr::null_mut())
}

/// Construct a standalone mutable/immutable numeric WebAssembly.Global in
/// this worker's shared store.
#[no_mangle]
pub extern "C" fn perry_wasm_host_global_new(kind: u8, mutable: i32, bits: u64) -> *mut c_void {
    let value = match kind {
        WASM_VAL_KIND_I32 => Val::I32(bits as u32 as i32),
        WASM_VAL_KIND_I64 => Val::I64(bits as i64),
        WASM_VAL_KIND_F32 => Val::F32(f32::from_bits(bits as u32).into()),
        WASM_VAL_KIND_F64 => Val::F64(f64::from_bits(bits).into()),
        _ => return std::ptr::null_mut(),
    };
    with_host_runtime(|runtime| {
        let global = Global::new(
            &mut runtime.store,
            value,
            if mutable != 0 {
                Mutability::Var
            } else {
                Mutability::Const
            },
        );
        extern_handle(global.into())
    })
    .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_global_get(
    handle: *mut c_void,
    out_kind: *mut u8,
    out_bits: *mut u64,
) -> i32 {
    if out_kind.is_null() || out_bits.is_null() {
        return 0;
    }
    let Some(Extern::Global(global)) = extern_from_handle(handle) else {
        return 0;
    };
    with_host_runtime(|runtime| {
        let Some((kind, bits)) = val_kind_bits(&global.get(&runtime.store)) else {
            return 0;
        };
        unsafe {
            *out_kind = kind;
            *out_bits = bits;
        }
        1
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_global_set(handle: *mut c_void, kind: u8, bits: u64) -> i32 {
    let Some(Extern::Global(global)) = extern_from_handle(handle) else {
        return 0;
    };
    let value = val_from_kind_bits(kind, bits);
    with_host_runtime(|runtime| global.set(&mut runtime.store, value).is_ok() as i32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_memory_new(initial: u32, maximum: u32) -> *mut c_void {
    let maximum = (maximum != u32::MAX).then_some(maximum);
    let ty = MemoryType::new(initial, maximum);
    with_host_runtime(|runtime| {
        Memory::new(&mut runtime.store, ty)
            .map(|memory| extern_handle(memory.into()))
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_memory_span(handle: *mut c_void, out_len: *mut usize) -> *mut u8 {
    if !out_len.is_null() {
        unsafe { *out_len = 0 };
    }
    let Some(Extern::Memory(memory)) = extern_from_handle(handle) else {
        return std::ptr::null_mut();
    };
    with_host_runtime(|runtime| {
        if !out_len.is_null() {
            unsafe { *out_len = memory.data_size(&runtime.store) };
        }
        memory.data_ptr(&runtime.store)
    })
    .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_memory_grow(handle: *mut c_void, delta: u32) -> i64 {
    let Some(Extern::Memory(memory)) = extern_from_handle(handle) else {
        return -1;
    };
    with_host_runtime(|runtime| {
        memory
            .grow(&mut runtime.store, u64::from(delta))
            .map(|pages| pages as i64)
            .unwrap_or(-1)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_table_new(
    element_kind: u8,
    initial: u32,
    maximum: u32,
) -> *mut c_void {
    let element = match element_kind {
        0 => ValType::FuncRef,
        1 => ValType::ExternRef,
        _ => return std::ptr::null_mut(),
    };
    let maximum = (maximum != u32::MAX).then_some(maximum);
    let ty = TableType::new(element, initial, maximum);
    with_host_runtime(|runtime| {
        Table::new(&mut runtime.store, ty, Val::default(element))
            .map(|table| extern_handle(table.into()))
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}

fn table_from_handle(handle: *mut c_void) -> Option<Table> {
    match extern_from_handle(handle) {
        Some(Extern::Table(table)) => Some(table),
        _ => None,
    }
}

pub(crate) fn table_value_for_store(
    store: &mut Store<()>,
    table: Table,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
) -> Option<Val> {
    let element = table.ty(&*store).element();
    if is_null != 0 {
        return Some(Val::default(element));
    }
    match element {
        ValType::ExternRef => Some(Val::from(ExternRef::new(store, bits))),
        ValType::FuncRef => match extern_from_handle(external) {
            Some(Extern::Func(function)) => Some(Val::FuncRef(Ref::Val(function))),
            _ => None,
        },
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_table_len(handle: *mut c_void) -> usize {
    let Some(table) = table_from_handle(handle) else {
        return usize::MAX;
    };
    with_host_runtime(|runtime| usize::try_from(table.size(&runtime.store)).unwrap_or(usize::MAX))
        .unwrap_or(usize::MAX)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_table_get(
    handle: *mut c_void,
    index: usize,
    out_bits: *mut u64,
    out_is_null: *mut i32,
    out_external: *mut *mut c_void,
) -> i32 {
    if out_bits.is_null() || out_is_null.is_null() {
        return 0;
    }
    let Some(table) = table_from_handle(handle) else {
        return 0;
    };
    with_host_runtime(|runtime| {
        let Some(value) = table.get(&runtime.store, index as u64) else {
            return 0;
        };
        let (bits, is_null, external) = match value {
            Val::ExternRef(Ref::Null) | Val::FuncRef(Ref::Null) => (0, 1, std::ptr::null_mut()),
            Val::ExternRef(Ref::Val(value)) => {
                let Some(bits) = value.data(&runtime.store).downcast_ref::<u64>() else {
                    return 0;
                };
                (*bits, 0, std::ptr::null_mut())
            }
            Val::FuncRef(Ref::Val(function)) => (0, 0, extern_handle(function.into())),
            _ => return 0,
        };
        unsafe {
            *out_bits = bits;
            *out_is_null = is_null;
            if !out_external.is_null() {
                *out_external = external;
            }
        }
        1
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_table_set(
    handle: *mut c_void,
    index: usize,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
) -> i32 {
    let Some(table) = table_from_handle(handle) else {
        return 0;
    };
    with_host_runtime(|runtime| {
        let Some(value) = table_value_for_store(&mut runtime.store, table, bits, is_null, external)
        else {
            return 0;
        };
        table.set(&mut runtime.store, index as u64, value).is_ok() as i32
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_table_grow(
    handle: *mut c_void,
    delta: usize,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
    out_old_len: *mut usize,
) -> i32 {
    if out_old_len.is_null() {
        return 0;
    }
    let Some(table) = table_from_handle(handle) else {
        return 0;
    };
    with_host_runtime(|runtime| {
        let Some(value) = table_value_for_store(&mut runtime.store, table, bits, is_null, external)
        else {
            return 0;
        };
        let Ok(old_len) = table.grow(&mut runtime.store, delta as u64, value) else {
            return 0;
        };
        let Ok(old_len) = usize::try_from(old_len) else {
            return 0;
        };
        unsafe { *out_old_len = old_len };
        1
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_func_arity(handle: *mut c_void) -> usize {
    let Some(Extern::Func(function)) = extern_from_handle(handle) else {
        return usize::MAX;
    };
    with_host_runtime(|runtime| function.ty(&runtime.store).params().len()).unwrap_or(usize::MAX)
}

/// Invoke a function obtained from a funcref table. This is the generic
/// counterpart to the instance-export fast path in `lib.rs`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn perry_wasm_host_func_call(
    handle: *mut c_void,
    arg_kinds: *const u8,
    arg_bits: *const u64,
    arg_count: usize,
    out_kinds: *mut u8,
    out_bits: *mut u64,
    out_capacity: usize,
    out_count: *mut usize,
    out_err: *mut *mut c_char,
) -> i32 {
    if out_count.is_null()
        || (arg_count != 0 && (arg_kinds.is_null() || arg_bits.is_null()))
        || (out_capacity != 0 && (out_kinds.is_null() || out_bits.is_null()))
    {
        capture_err(out_err, WasmHostError::Runtime("null arg".into()));
        return 0;
    }
    let Some(Extern::Func(function)) = extern_from_handle(handle) else {
        capture_err(
            out_err,
            WasmHostError::Runtime("invalid function handle".into()),
        );
        return 0;
    };
    let call_result = with_host_runtime(|runtime| -> Result<Vec<(u8, u64)>, WasmHostError> {
        let ty = function.ty(&runtime.store);
        if ty.params().len() != arg_count {
            return Err(WasmHostError::Runtime(format!(
                "table function: arity mismatch (expects {}, got {arg_count})",
                ty.params().len()
            )));
        }
        let kinds = unsafe { slice::from_raw_parts(arg_kinds, arg_count) };
        let bits = unsafe { slice::from_raw_parts(arg_bits, arg_count) };
        let mut args = Vec::with_capacity(arg_count);
        for ((kind, bits), expected) in kinds
            .iter()
            .copied()
            .zip(bits.iter().copied())
            .zip(ty.params().iter().copied())
        {
            let value = match kind {
                WASM_VAL_KIND_I32 => WasmVal::I32(bits as i32),
                WASM_VAL_KIND_I64 => WasmVal::I64(bits as i64),
                WASM_VAL_KIND_F32 => WasmVal::F32(f32::from_bits(bits as u32)),
                WASM_VAL_KIND_F64 => WasmVal::F64(f64::from_bits(bits)),
                other => {
                    return Err(WasmHostError::UnsupportedSignature(format!(
                        "arg kind {other}"
                    )))
                }
            };
            let Some(value) = coerce_numeric_value(value, expected) else {
                return Err(WasmHostError::UnsupportedSignature(format!(
                    "table function: unsupported parameter type {expected:?}"
                )));
            };
            args.push(value);
        }
        if ty.results().len() > out_capacity {
            return Err(WasmHostError::UnsupportedSignature(format!(
                "table function: {} results exceed host capacity {out_capacity}",
                ty.results().len()
            )));
        }
        let mut results: Vec<Val> = ty.results().iter().copied().map(Val::default).collect();
        function
            .call(&mut runtime.store, &args, &mut results)
            .map_err(|error| WasmHostError::Runtime(error.to_string()))?;
        results
            .iter()
            .map(|value| {
                val_kind_bits(value).ok_or_else(|| {
                    WasmHostError::UnsupportedSignature(format!(
                        "table function: unsupported result type {:?}",
                        value.ty()
                    ))
                })
            })
            .collect()
    })
    .unwrap_or_else(|| {
        Err(WasmHostError::Runtime(
            "host runtime is already in use".into(),
        ))
    });
    let values = match call_result {
        Ok(values) => values,
        Err(error) => {
            capture_err(out_err, error);
            return 0;
        }
    };
    for (index, (kind, bits)) in values.iter().copied().enumerate() {
        unsafe {
            *out_kinds.add(index) = kind;
            *out_bits.add(index) = bits;
        }
    }
    unsafe { *out_count = values.len() };
    1
}
