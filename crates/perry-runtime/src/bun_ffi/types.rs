//! FFIType — numeric model, string aliases, and the JS enum object.
//!
//! Numeric values mirror Bun's `FFIType` exactly (`packages/bun-types/
//! ffi.d.ts` / the runtime object literal in `src/js/bun/ffi.ts`):
//!
//! ```text
//! char=0  i8=1  u8=2  i16=3  u16=4  i32=5  u32=6  i64=7  u64=8
//! f64=9  f32=10  bool=11  ptr=12  void=13  cstring=14
//! i64_fast=15  u64_fast=16  function=17  napi_env=18  napi_value=19
//! buffer=20
//! ```
//!
//! Stage 1 implements 0–16; `function` (17) parses but `dlopen` rejects it
//! with a "not yet supported" error (JSCallback is stage 3), and the napi /
//! buffer slots (18–20) are rejected as unsupported. The slots stay
//! reserved so later stages only need to lift the rejection.

use super::{number_value, string_value};
use crate::value::JSValue;
use std::sync::atomic::{AtomicU64, Ordering};

pub const T_CHAR: u8 = 0;
pub const T_I8: u8 = 1;
pub const T_U8: u8 = 2;
pub const T_I16: u8 = 3;
pub const T_U16: u8 = 4;
pub const T_I32: u8 = 5;
pub const T_U32: u8 = 6;
pub const T_I64: u8 = 7;
pub const T_U64: u8 = 8;
pub const T_F64: u8 = 9;
pub const T_F32: u8 = 10;
pub const T_BOOL: u8 = 11;
pub const T_PTR: u8 = 12;
pub const T_VOID: u8 = 13;
pub const T_CSTRING: u8 = 14;
pub const T_I64_FAST: u8 = 15;
pub const T_U64_FAST: u8 = 16;
pub const T_FUNCTION: u8 = 17;
pub const T_NAPI_ENV: u8 = 18;
pub const T_NAPI_VALUE: u8 = 19;
pub const T_BUFFER: u8 = 20;

/// Bun's runtime `FFIType` object literal, key for key (including the
/// numeric-string self-mapped keys `"0"`–`"17"` — and, like Bun, NOT
/// `"18"`–`"20"`). Order matches `src/js/bun/ffi.ts` for easy diffing.
const FFI_TYPE_ENTRIES: &[(&str, u8)] = &[
    ("0", 0),
    ("1", 1),
    ("2", 2),
    ("3", 3),
    ("4", 4),
    ("5", 5),
    ("6", 6),
    ("7", 7),
    ("8", 8),
    ("9", 9),
    ("10", 10),
    ("11", 11),
    ("12", 12),
    ("13", 13),
    ("14", 14),
    ("15", 15),
    ("16", 16),
    ("17", 17),
    ("bool", T_BOOL),
    ("c_int", T_I32),
    ("c_uint", T_U32),
    ("char", T_CHAR),
    ("char*", T_PTR),
    ("double", T_F64),
    ("f32", T_F32),
    ("f64", T_F64),
    ("float", T_F32),
    ("i16", T_I16),
    ("i32", T_I32),
    ("i64", T_I64),
    ("i8", T_I8),
    ("int", T_I32),
    ("int16_t", T_I16),
    ("int32_t", T_I32),
    ("int64_t", T_I64),
    ("int8_t", T_I8),
    ("isize", T_I64),
    ("u16", T_U16),
    ("u32", T_U32),
    ("u64", T_U64),
    ("u8", T_U8),
    ("uint16_t", T_U16),
    ("uint32_t", T_U32),
    ("uint64_t", T_U64),
    ("uint8_t", T_U8),
    ("usize", T_U64),
    ("void*", T_PTR),
    ("ptr", T_PTR),
    ("pointer", T_PTR),
    ("void", T_VOID),
    ("cstring", T_CSTRING),
    ("i64_fast", T_I64_FAST),
    ("u64_fast", T_U64_FAST),
    ("function", T_FUNCTION),
    ("callback", T_FUNCTION),
    ("fn", T_FUNCTION),
    ("napi_env", T_NAPI_ENV),
    ("napi_value", T_NAPI_VALUE),
    ("buffer", T_BUFFER),
];

/// Integer-class vs float-class for register assignment (`call.rs`).
pub(crate) fn is_float_class(t: u8) -> bool {
    matches!(t, T_F32 | T_F64)
}

fn alias_to_type(name: &str) -> Option<u8> {
    // Skip the numeric self-keys — a string alias lookup only matches the
    // named entries; numeric values arrive as JS numbers.
    FFI_TYPE_ENTRIES
        .iter()
        .skip(18)
        .find(|(k, _)| *k == name)
        .map(|&(_, v)| v)
}

fn throw_unsupported_type(display: &str) -> ! {
    let names: Vec<&str> = FFI_TYPE_ENTRIES.iter().skip(18).map(|&(k, _)| k).collect();
    crate::fs::validate::throw_type_error_with_code(
        &format!(
            "Unsupported type {display}. Must be one of: {}",
            names.join(", ")
        ),
        "ERR_INVALID_ARG_TYPE",
    )
}

/// Parse one `args`/`returns` entry of a `dlopen` symbol table: a numeric
/// `FFIType` value or a string alias. Throws a Bun-shaped `TypeError` on
/// anything unrecognized.
pub(crate) unsafe fn parse_ffi_type(value: f64) -> u8 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(jv, &mut sso) {
            let name = std::str::from_utf8(bytes).unwrap_or("");
            if let Some(t) = alias_to_type(name) {
                return t;
            }
            throw_unsupported_type(name);
        }
        throw_unsupported_type("<string>");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else {
        f64::NAN
    };
    if n.is_finite() && n >= 0.0 && n <= T_BUFFER as f64 && n.fract() == 0.0 {
        return n as u8;
    }
    throw_unsupported_type(&format!("{n}"));
}

// ── The cached FFIType JS object ────────────────────────────────────────

/// NaN-boxed pointer to the built-once `FFIType` object. A mutable GC root:
/// scanned (and rewritten, though the object is old-arena and won't move)
/// by `scan_bun_ffi_roots_mut`.
static FFI_TYPE_OBJECT_CACHE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn scan_ffi_type_cache_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_nanbox_u64_slot(
        &FFI_TYPE_OBJECT_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
}

/// Build (once) and return the `FFIType` enum object.
pub(crate) fn ffi_type_object_value() -> f64 {
    let cached = FFI_TYPE_OBJECT_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let obj = crate::object::js_object_alloc(0, FFI_TYPE_ENTRIES.len() as u32);
    for &(name, value) in FFI_TYPE_ENTRIES {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, number_value(value as f64));
    }
    let value = f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &FFI_TYPE_OBJECT_CACHE,
        value.to_bits(),
        Ordering::Relaxed,
    );
    value
}

// Re-exported for the namespace-constants path (`get_native_module_constant`).
pub(crate) fn suffix_value() -> f64 {
    string_value(super::suffix_str())
}
