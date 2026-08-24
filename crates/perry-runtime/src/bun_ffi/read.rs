//! Direct scalar reads for the `bun:ffi` `read` namespace.
//!
//! These mirror a native-endian `DataView` over an unsafe pointer without
//! allocating an intermediate ArrayBuffer. FFF uses them heavily for result
//! structs (`read.ptr`, `read.u32`, `read.i64`, and friends).

use crate::closure::ClosureHeader;
use crate::value::JSValue;
use std::cell::Cell;

const READERS: &[(&str, u8)] = &[
    ("ptr", super::types::T_PTR),
    ("i8", super::types::T_I8),
    ("i16", super::types::T_I16),
    ("i32", super::types::T_I32),
    ("i64", super::types::T_I64),
    ("u8", super::types::T_U8),
    ("u16", super::types::T_U16),
    ("u32", super::types::T_U32),
    ("u64", super::types::T_U64),
    ("f32", super::types::T_F32),
    ("f64", super::types::T_F64),
];

thread_local! {
    static READ_OBJECT_CACHE: Cell<u64> = const { Cell::new(0) };
}

fn throw_type(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

unsafe fn address(pointer: f64, offset: f64) -> usize {
    let base = super::call::value_to_pointer_arg(pointer);
    if base == 0 {
        throw_type("bun:ffi read pointer must be non-zero");
    }
    let offset_value = JSValue::from_bits(offset.to_bits());
    let displacement = if offset_value.is_undefined() {
        0i64
    } else if offset_value.is_int32() {
        offset_value.as_int32() as i64
    } else if offset_value.is_number() {
        let value = offset_value.as_number();
        if !value.is_finite()
            || value.fract() != 0.0
            || value < i64::MIN as f64
            || value > i64::MAX as f64
        {
            throw_type("bun:ffi read byteOffset must be an integer");
        }
        value as i64
    } else {
        throw_type("bun:ffi read byteOffset must be a number");
    };
    let result = base as i128 + displacement as i128;
    if result <= 0 || result > usize::MAX as i128 {
        crate::fs::validate::throw_range_error_named(
            "bun:ffi read pointer + byteOffset is outside the native pointer range",
            "ERR_OUT_OF_RANGE",
        );
    }
    result as usize
}

unsafe fn read_value(kind: u8, pointer: f64, offset: f64) -> f64 {
    let pointer = address(pointer, offset) as *const u8;
    match kind {
        super::types::T_PTR => super::call::convert_int_return(
            super::types::T_PTR,
            std::ptr::read_unaligned(pointer.cast::<usize>()) as u64,
        ),
        super::types::T_I8 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<i8>()) as f64)
        }
        super::types::T_U8 => super::number_value(std::ptr::read_unaligned(pointer) as f64),
        super::types::T_I16 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<i16>()) as f64)
        }
        super::types::T_U16 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<u16>()) as f64)
        }
        super::types::T_I32 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<i32>()) as f64)
        }
        super::types::T_U32 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<u32>()) as f64)
        }
        super::types::T_I64 => super::call::convert_int_return(
            super::types::T_I64,
            std::ptr::read_unaligned(pointer.cast::<i64>()) as u64,
        ),
        super::types::T_U64 => super::call::convert_int_return(
            super::types::T_U64,
            std::ptr::read_unaligned(pointer.cast::<u64>()),
        ),
        super::types::T_F32 => {
            super::number_value(std::ptr::read_unaligned(pointer.cast::<f32>()) as f64)
        }
        super::types::T_F64 => super::number_value(std::ptr::read_unaligned(pointer.cast::<f64>())),
        _ => super::undefined(),
    }
}

macro_rules! reader {
    ($name:ident, $kind:expr) => {
        extern "C" fn $name(_closure: *const ClosureHeader, pointer: f64, offset: f64) -> f64 {
            unsafe { read_value($kind, pointer, offset) }
        }
    };
}

reader!(read_ptr, super::types::T_PTR);
reader!(read_i8, super::types::T_I8);
reader!(read_i16, super::types::T_I16);
reader!(read_i32, super::types::T_I32);
reader!(read_i64, super::types::T_I64);
reader!(read_u8, super::types::T_U8);
reader!(read_u16, super::types::T_U16);
reader!(read_u32, super::types::T_U32);
reader!(read_u64, super::types::T_U64);
reader!(read_f32, super::types::T_F32);
reader!(read_f64, super::types::T_F64);

fn reader_function(kind: u8) -> *const u8 {
    match kind {
        super::types::T_PTR => read_ptr as *const u8,
        super::types::T_I8 => read_i8 as *const u8,
        super::types::T_I16 => read_i16 as *const u8,
        super::types::T_I32 => read_i32 as *const u8,
        super::types::T_I64 => read_i64 as *const u8,
        super::types::T_U8 => read_u8 as *const u8,
        super::types::T_U16 => read_u16 as *const u8,
        super::types::T_U32 => read_u32 as *const u8,
        super::types::T_U64 => read_u64 as *const u8,
        super::types::T_F32 => read_f32 as *const u8,
        _ => read_f64 as *const u8,
    }
}

fn closure(name: &str, kind: u8) -> f64 {
    let function = reader_function(kind);
    crate::closure::js_register_closure_arity(function, 2);
    crate::closure::js_register_closure_length(function, 1);
    let closure = crate::closure::js_closure_alloc(function, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, 1);
    crate::value::js_nanbox_pointer(closure as i64)
}

pub(crate) fn read_object_value() -> f64 {
    let cached = READ_OBJECT_CACHE.with(|slot| slot.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, READERS.len() as u32));
    for &(name, kind) in READERS {
        let value = scope.root_nanbox_f64(closure(name, kind));
        let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ));
        object.with_mut_ptr(|object: *mut crate::object::ObjectHeader| {
            key.with_const_ptr(|key| {
                crate::object::js_object_set_field_by_name(object, key, value.get_nanbox_f64())
            })
        });
    }
    let value =
        object.with_mut_ptr(|object: *mut u8| f64::from_bits(JSValue::object_ptr(object).bits()));
    READ_OBJECT_CACHE.with(|slot| slot.set(value.to_bits()));
    value
}

pub(crate) fn scan_read_cache_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    READ_OBJECT_CACHE.with(|slot| {
        let mut bits = slot.get();
        if bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut bits);
            slot.set(bits);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_reads_are_native_endian_and_unaligned() {
        let mut bytes = [0u8; 32];
        bytes[1..5].copy_from_slice(&0x7856_3412u32.to_ne_bytes());
        let pointer = super::super::number_value(bytes.as_mut_ptr() as usize as f64);
        let value = unsafe { read_value(super::super::types::T_U32, pointer, 1.0) };
        assert_eq!(
            JSValue::from_bits(value.to_bits()).as_number(),
            0x7856_3412u32 as f64
        );
    }
}
