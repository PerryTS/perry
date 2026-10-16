//! The C entry points of the optional `perry-wasm-host` archive (wasmi), as
//! one table both sides build from this list.
//!
//! The runtime archive carries the whole `WebAssembly` implementation, but it
//! never names a `perry_wasm_host_*` symbol: a program that does not use
//! WebAssembly links no host archive, and every symbol the runtime archive
//! references must resolve on every target (MSVC resolves the references of
//! every object it loads, reachable or not). Instead the host archive fills a
//! [`WasmHostOps`] table and registers it with the runtime
//! (`js_runtime_set_wasm_host_ops`) from the static constructor of the object
//! perry's link step generates for a WebAssembly program. The runtime calls
//! the host only through that table.
//!
//! [`wasm_host_entry_points!`] is the single list of entry points: it expands
//! a caller's macro once per entry point, so the table type below, the host's
//! table value and the runtime's call wrappers cannot disagree on a name or a
//! signature. Handles cross the boundary as `*mut c_void`.

pub use core::ffi::{c_char, c_void};

pub const WASM_VAL_KIND_I32: u8 = 0;
pub const WASM_VAL_KIND_I64: u8 = 1;
pub const WASM_VAL_KIND_F32: u8 = 2;
pub const WASM_VAL_KIND_F64: u8 = 3;
pub const WASM_VAL_KIND_EXTERNREF: u8 = 4;
pub const WASM_VAL_KIND_NONE: u8 = 0xFF;
pub const WASM_EXTERN_KIND_FUNCTION: u8 = 0;
pub const WASM_EXTERN_KIND_TABLE: u8 = 1;
pub const WASM_EXTERN_KIND_MEMORY: u8 = 2;
pub const WASM_EXTERN_KIND_GLOBAL: u8 = 3;

/// The runtime's import trampoline: the host calls it for a JavaScript import.
pub type WasmImportCallback = unsafe extern "C" fn(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
    arg_kinds: *const u8,
    arg_bits: *const u64,
    arg_count: usize,
    result_kinds: *const u8,
    result_bits: *mut u64,
    result_count: usize,
) -> i32;

/// The runtime's resolver for non-function imports (tables, memories, globals).
pub type WasmImportResolverCallback = unsafe extern "C" fn(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
    kind: u8,
) -> *mut c_void;

/// Expand `$m!(name(arg: Ty, ...) -> Ret)` once per host entry point, all in
/// one invocation: `$m! { name(args) -> ret; ... }`.
#[macro_export]
macro_rules! wasm_host_entry_points {
    ($m:ident) => {
        $m! {
            perry_wasm_host_string_free(s: *mut $crate::wasm_host::c_char) -> ();
            perry_wasm_host_validate(bytes: *const u8, len: usize) -> i32;
            perry_wasm_host_module_new(bytes: *const u8, len: usize, out_err: *mut *mut $crate::wasm_host::c_char) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_module_drop(module: *mut $crate::wasm_host::c_void) -> ();
            perry_wasm_host_extern_drop(handle: *mut $crate::wasm_host::c_void) -> ();
            perry_wasm_host_module_exports_len(module: *mut $crate::wasm_host::c_void) -> usize;
            perry_wasm_host_module_export_at(module: *mut $crate::wasm_host::c_void, index: usize, out_name: *mut *const $crate::wasm_host::c_char, out_name_len: *mut usize, out_kind: *mut u8) -> i32;
            perry_wasm_host_module_export_func_arity(module: *mut $crate::wasm_host::c_void, index: usize) -> usize;
            perry_wasm_host_module_imports_len(module: *mut $crate::wasm_host::c_void) -> usize;
            perry_wasm_host_module_import_at(module: *mut $crate::wasm_host::c_void, index: usize, out_module: *mut *const $crate::wasm_host::c_char, out_module_len: *mut usize, out_name: *mut *const $crate::wasm_host::c_char, out_name_len: *mut usize, out_kind: *mut u8) -> i32;
            perry_wasm_host_module_custom_sections_len(module: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize) -> usize;
            perry_wasm_host_module_custom_section_at(module: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize, nth: usize, out_data: *mut *const u8, out_data_len: *mut usize) -> i32;
            perry_wasm_host_instance_new(module: *mut $crate::wasm_host::c_void, import_callback: Option<$crate::wasm_host::WasmImportCallback>, import_resolver: Option<$crate::wasm_host::WasmImportResolverCallback>, import_context: u64, out_err: *mut *mut $crate::wasm_host::c_char) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_instance_import_context(inst: *mut $crate::wasm_host::c_void) -> u64;
            perry_wasm_host_instance_drop(inst: *mut $crate::wasm_host::c_void) -> ();
            perry_wasm_host_instance_memory_span(inst: *mut $crate::wasm_host::c_void, out_len: *mut usize) -> *mut u8;
            perry_wasm_host_instance_table_len(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize) -> usize;
            perry_wasm_host_instance_table_get(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize, index: usize, out_bits: *mut u64, out_is_null: *mut i32, out_external: *mut *mut $crate::wasm_host::c_void) -> i32;
            perry_wasm_host_instance_table_set(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize, index: usize, bits: u64, is_null: i32, external: *mut $crate::wasm_host::c_void) -> i32;
            perry_wasm_host_instance_table_grow(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize, delta: usize, bits: u64, is_null: i32, external: *mut $crate::wasm_host::c_void, out_old_len: *mut usize) -> i32;
            perry_wasm_host_instance_export_handle(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize) -> usize;
            perry_wasm_host_instance_export_extern(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_instance_take_exit_code(inst: *mut $crate::wasm_host::c_void, out_code: *mut i32) -> i32;
            perry_wasm_host_global_new(kind: u8, mutable: i32, bits: u64) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_global_get(handle: *mut $crate::wasm_host::c_void, out_kind: *mut u8, out_bits: *mut u64) -> i32;
            perry_wasm_host_global_set(handle: *mut $crate::wasm_host::c_void, kind: u8, bits: u64) -> i32;
            perry_wasm_host_memory_new(initial: u32, maximum: u32) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_memory_span(handle: *mut $crate::wasm_host::c_void, out_len: *mut usize) -> *mut u8;
            perry_wasm_host_memory_grow(handle: *mut $crate::wasm_host::c_void, delta: u32) -> i64;
            perry_wasm_host_table_new(element_kind: u8, initial: u32, maximum: u32) -> *mut $crate::wasm_host::c_void;
            perry_wasm_host_table_len(handle: *mut $crate::wasm_host::c_void) -> usize;
            perry_wasm_host_table_get(handle: *mut $crate::wasm_host::c_void, index: usize, out_bits: *mut u64, out_is_null: *mut i32, out_external: *mut *mut $crate::wasm_host::c_void) -> i32;
            perry_wasm_host_table_set(handle: *mut $crate::wasm_host::c_void, index: usize, bits: u64, is_null: i32, external: *mut $crate::wasm_host::c_void) -> i32;
            perry_wasm_host_table_grow(handle: *mut $crate::wasm_host::c_void, delta: usize, bits: u64, is_null: i32, external: *mut $crate::wasm_host::c_void, out_old_len: *mut usize) -> i32;
            perry_wasm_host_func_arity(handle: *mut $crate::wasm_host::c_void) -> usize;
            perry_wasm_host_func_call(handle: *mut $crate::wasm_host::c_void, arg_kinds: *const u8, arg_bits: *const u64, arg_count: usize, out_kinds: *mut u8, out_bits: *mut u64, out_capacity: usize, out_count: *mut usize, out_err: *mut *mut $crate::wasm_host::c_char) -> i32;
            perry_wasm_host_call_export(inst: *mut $crate::wasm_host::c_void, name: *const $crate::wasm_host::c_char, name_len: usize, arg_kinds: *const u8, arg_bits: *const u64, arg_count: usize, out_kinds: *mut u8, out_bits: *mut u64, out_capacity: usize, out_count: *mut usize, out_err: *mut *mut $crate::wasm_host::c_char) -> i32;
            perry_wasm_host_call_export_by_handle(inst: *mut $crate::wasm_host::c_void, handle: usize, arg_kinds: *const u8, arg_bits: *const u64, arg_count: usize, out_kinds: *mut u8, out_bits: *mut u64, out_capacity: usize, out_count: *mut usize, out_err: *mut *mut $crate::wasm_host::c_char) -> i32;
        }
    };
}

macro_rules! define_ops {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        /// One field per host entry point, named after its symbol.
        #[repr(C)]
        #[allow(clippy::type_complexity)]
        pub struct WasmHostOps {
            $(pub $name: unsafe extern "C" fn($($ty),*) -> $ret,)*
        }
    };
}
wasm_host_entry_points!(define_ops);
