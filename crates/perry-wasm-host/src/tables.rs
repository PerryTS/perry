//! wasm table accessors, split from `lib.rs` for the 2000-line file cap
//! (#9611 grew this crate with the zero-copy memory publication).

use super::*;

pub(crate) fn instance_table(inst: &WasmInstanceHandle, name: &str) -> Option<Table> {
    inst.inner.instance.get_table(inst.store(), name)
}

/// Return the current length of an exported table, or `usize::MAX` when the
/// export does not exist or cannot be represented on this platform.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_len(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
) -> usize {
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return usize::MAX;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return usize::MAX;
    };
    if let Some(len) = with_active_instance_tables(inst as *const _ as usize, |active| {
        active.lengths.get(name).copied()
    }) {
        return len.unwrap_or(usize::MAX);
    }
    instance_table(inst, name)
        .and_then(|table| usize::try_from(table.size(inst.store())).ok())
        .unwrap_or(usize::MAX)
}

/// Read an `externref` table entry. Perry stores the nan-boxed JavaScript
/// value bits inside wasmi's opaque `ExternRef`; null remains a real null ref.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_get(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    index: usize,
    out_bits: *mut u64,
    out_is_null: *mut i32,
    out_external: *mut *mut c_void,
) -> i32 {
    if out_bits.is_null() || out_is_null.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    if let Some(value) = with_active_instance_tables(inst as *const _ as usize, |active| {
        active.overrides.get(&(name.to_string(), index)).copied()
    }) {
        let Some(value) = value else {
            // The instance Store is already borrowed by wasmi. Existing
            // entries that JS has not overwritten cannot be inspected until
            // the call unwinds.
            return 0;
        };
        unsafe {
            *out_bits = value.bits;
            *out_is_null = value.is_null as i32;
            if !out_external.is_null() {
                *out_external = value.external;
            }
        }
        return 1;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    let Some(value) = table.get(inst.store(), index as u64) else {
        return 0;
    };
    match value {
        Val::ExternRef(Ref::Null) | Val::FuncRef(Ref::Null) => unsafe {
            *out_bits = 0;
            *out_is_null = 1;
            if !out_external.is_null() {
                *out_external = std::ptr::null_mut();
            }
        },
        Val::ExternRef(Ref::Val(value)) => {
            let Some(bits) = value.data(inst.store()).downcast_ref::<u64>() else {
                return 0;
            };
            unsafe {
                *out_bits = *bits;
                *out_is_null = 0;
                if !out_external.is_null() {
                    *out_external = std::ptr::null_mut();
                }
            }
        }
        Val::FuncRef(Ref::Val(function)) => unsafe {
            *out_bits = 0;
            *out_is_null = 0;
            if !out_external.is_null() {
                *out_external = extern_handle(function.into());
            }
        },
        _ => return 0,
    }
    1
}

pub(crate) fn table_value(
    inst: &mut WasmInstanceHandle,
    table: Table,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
) -> Option<Val> {
    let element = table.ty(inst.store()).element();
    if is_null != 0 {
        return Some(Val::default(element));
    }
    match element {
        ValType::ExternRef => Some(Val::from(ExternRef::new(inst.store_mut(), bits))),
        ValType::FuncRef => match extern_from_handle(external) {
            Some(Extern::Func(function)) => Some(Val::FuncRef(Ref::Val(function))),
            _ => None,
        },
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_set(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    index: usize,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
) -> i32 {
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let pending_value = PendingTableValue {
        bits,
        is_null: is_null != 0,
        external,
    };
    if let Some(queued) = with_active_instance_tables(inst as *mut _ as usize, |active| {
        let Some(len) = active.lengths.get(name).copied() else {
            return false;
        };
        if index >= len {
            return false;
        }
        active
            .overrides
            .insert((name.to_string(), index), pending_value);
        active.ops.push(PendingTableOp::Set {
            name: name.to_string(),
            index,
            value: pending_value,
        });
        true
    }) {
        return queued as i32;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    let Some(value) = table_value(inst, table, bits, is_null, external) else {
        return 0;
    };
    table.set(inst.store_mut(), index as u64, value).is_ok() as i32
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_grow(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    delta: usize,
    bits: u64,
    is_null: i32,
    external: *mut c_void,
    out_old_len: *mut usize,
) -> i32 {
    if out_old_len.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let pending_value = PendingTableValue {
        bits,
        is_null: is_null != 0,
        external,
    };
    if let Some(old_len) = with_active_instance_tables(inst as *mut _ as usize, |active| {
        let old_len = *active.lengths.get(name)?;
        let new_len = old_len.checked_add(delta)?;
        active.lengths.insert(name.to_string(), new_len);
        for index in old_len..new_len {
            active
                .overrides
                .insert((name.to_string(), index), pending_value);
        }
        active.ops.push(PendingTableOp::Grow {
            name: name.to_string(),
            delta,
            value: pending_value,
        });
        Some(old_len)
    }) {
        let Some(old_len) = old_len else {
            return 0;
        };
        unsafe { *out_old_len = old_len };
        return 1;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    let Some(value) = table_value(inst, table, bits, is_null, external) else {
        return 0;
    };
    let Ok(old_len) = table.grow(inst.store_mut(), delta as u64, value) else {
        return 0;
    };
    let Ok(old_len) = usize::try_from(old_len) else {
        return 0;
    };
    unsafe { *out_old_len = old_len };
    1
}

/// Consume the status captured by WASI `proc_exit`, if that import ran.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_take_exit_code(
    inst: *mut WasmInstanceHandle,
    out_code: *mut i32,
) -> i32 {
    if out_code.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let code = inst.inner.exit_code.swap(i32::MIN, Ordering::Relaxed);
    if code == i32::MIN {
        return 0;
    }
    unsafe { *out_code = code };
    1
}

/// Numeric value type tags for the C ABI — must match
/// `perry_wasm_host_call_export`'s `arg_kinds` / `ret_kind` encoding.
pub const WASM_VAL_KIND_I32: u8 = 0;
pub const WASM_VAL_KIND_I64: u8 = 1;
pub const WASM_VAL_KIND_F32: u8 = 2;
pub const WASM_VAL_KIND_F64: u8 = 3;
pub const WASM_VAL_KIND_EXTERNREF: u8 = 4;
pub const WASM_VAL_KIND_NONE: u8 = 0xFF;

/// Call an export by name. Args are encoded as parallel arrays:
/// `arg_kinds[i]` is the type tag, `arg_bits[i]` is the raw 64-bit payload
/// (i32/f32 widened, i64/f64 as-is). On success writes every result into the
/// parallel output arrays and sets `*out_count`. On error returns 0 and writes
/// `*out_err`.
/// Resolve an export function to a stable handle so repeated calls skip the
/// by-name lookup and the `FuncType` clone (#9611). Returns 0 when the
/// instance has no function export by that name; any other value is an opaque
/// handle for [`perry_wasm_host_call_export_by_handle`].
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_export_handle(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
) -> usize {
    if inst.is_null() || name.is_null() {
        return 0;
    }
    let inst = unsafe { &mut *inst };
    let name_bytes = unsafe { slice::from_raw_parts(name as *const u8, name_len) };
    let Ok(name) = std::str::from_utf8(name_bytes) else {
        return 0;
    };
    resolve_export(inst, name).map_or(0, |index| index + 1)
}
