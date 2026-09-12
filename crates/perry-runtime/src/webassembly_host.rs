//! C ABI declarations for the optional `perry-wasm-host` archive.

use std::ffi::{c_char, c_void};

pub(super) const WASM_VAL_KIND_I32: u8 = 0;
pub(super) const WASM_VAL_KIND_I64: u8 = 1;
pub(super) const WASM_VAL_KIND_F32: u8 = 2;
pub(super) const WASM_VAL_KIND_F64: u8 = 3;
pub(super) const WASM_VAL_KIND_EXTERNREF: u8 = 4;
pub(super) const WASM_VAL_KIND_NONE: u8 = 0xFF;
pub(super) const WASM_EXTERN_KIND_FUNCTION: u8 = 0;
pub(super) const WASM_EXTERN_KIND_TABLE: u8 = 1;
pub(super) const WASM_EXTERN_KIND_MEMORY: u8 = 2;
pub(super) const WASM_EXTERN_KIND_GLOBAL: u8 = 3;

pub(super) type WasmImportCallback = unsafe extern "C" fn(
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

pub(super) type WasmImportResolverCallback = unsafe extern "C" fn(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
    kind: u8,
) -> *mut c_void;

extern "C" {
    pub(super) fn perry_wasm_host_string_free(s: *mut c_char);
    pub(super) fn perry_wasm_host_validate(bytes: *const u8, len: usize) -> i32;
    pub(super) fn perry_wasm_host_module_new(
        bytes: *const u8,
        len: usize,
        out_err: *mut *mut c_char,
    ) -> *mut c_void;
    pub(super) fn perry_wasm_host_module_drop(module: *mut c_void);
    pub(super) fn perry_wasm_host_module_exports_len(module: *mut c_void) -> usize;
    pub(super) fn perry_wasm_host_module_export_at(
        module: *mut c_void,
        index: usize,
        out_name: *mut *const c_char,
        out_name_len: *mut usize,
        out_kind: *mut u8,
    ) -> i32;
    pub(super) fn perry_wasm_host_module_export_func_arity(
        module: *mut c_void,
        index: usize,
    ) -> usize;
    pub(super) fn perry_wasm_host_module_imports_len(module: *mut c_void) -> usize;
    pub(super) fn perry_wasm_host_module_import_at(
        module: *mut c_void,
        index: usize,
        out_module: *mut *const c_char,
        out_module_len: *mut usize,
        out_name: *mut *const c_char,
        out_name_len: *mut usize,
        out_kind: *mut u8,
    ) -> i32;
    pub(super) fn perry_wasm_host_module_custom_sections_len(
        module: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> usize;
    pub(super) fn perry_wasm_host_module_custom_section_at(
        module: *mut c_void,
        name: *const c_char,
        name_len: usize,
        nth: usize,
        out_data: *mut *const u8,
        out_data_len: *mut usize,
    ) -> i32;
    pub(super) fn perry_wasm_host_instance_new(
        module: *mut c_void,
        import_callback: Option<WasmImportCallback>,
        import_resolver: Option<WasmImportResolverCallback>,
        import_context: u64,
        out_err: *mut *mut c_char,
    ) -> *mut c_void;
    #[allow(dead_code)]
    pub(super) fn perry_wasm_host_instance_drop(inst: *mut c_void);
    pub(super) fn perry_wasm_host_instance_memory_span(
        inst: *mut c_void,
        out_len: *mut usize,
    ) -> *mut u8;
    pub(super) fn perry_wasm_host_instance_table_len(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> usize;
    pub(super) fn perry_wasm_host_instance_table_get(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        index: usize,
        out_bits: *mut u64,
        out_is_null: *mut i32,
        out_external: *mut *mut c_void,
    ) -> i32;
    pub(super) fn perry_wasm_host_instance_table_set(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        index: usize,
        bits: u64,
        is_null: i32,
        external: *mut c_void,
    ) -> i32;
    pub(super) fn perry_wasm_host_instance_table_grow(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        delta: usize,
        bits: u64,
        is_null: i32,
        external: *mut c_void,
        out_old_len: *mut usize,
    ) -> i32;
    pub(super) fn perry_wasm_host_instance_export_handle(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> usize;
    pub(super) fn perry_wasm_host_instance_export_extern(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> *mut c_void;
    pub(super) fn perry_wasm_host_instance_take_exit_code(
        inst: *mut c_void,
        out_code: *mut i32,
    ) -> i32;
    pub(super) fn perry_wasm_host_global_new(kind: u8, mutable: i32, bits: u64) -> *mut c_void;
    pub(super) fn perry_wasm_host_global_get(
        handle: *mut c_void,
        out_kind: *mut u8,
        out_bits: *mut u64,
    ) -> i32;
    pub(super) fn perry_wasm_host_global_set(handle: *mut c_void, kind: u8, bits: u64) -> i32;
    pub(super) fn perry_wasm_host_memory_new(initial: u32, maximum: u32) -> *mut c_void;
    pub(super) fn perry_wasm_host_memory_span(handle: *mut c_void, out_len: *mut usize) -> *mut u8;
    pub(super) fn perry_wasm_host_memory_grow(handle: *mut c_void, delta: u32) -> i64;
    pub(super) fn perry_wasm_host_table_new(
        element_kind: u8,
        initial: u32,
        maximum: u32,
    ) -> *mut c_void;
    pub(super) fn perry_wasm_host_table_len(handle: *mut c_void) -> usize;
    pub(super) fn perry_wasm_host_table_get(
        handle: *mut c_void,
        index: usize,
        out_bits: *mut u64,
        out_is_null: *mut i32,
        out_external: *mut *mut c_void,
    ) -> i32;
    pub(super) fn perry_wasm_host_table_set(
        handle: *mut c_void,
        index: usize,
        bits: u64,
        is_null: i32,
        external: *mut c_void,
    ) -> i32;
    pub(super) fn perry_wasm_host_table_grow(
        handle: *mut c_void,
        delta: usize,
        bits: u64,
        is_null: i32,
        external: *mut c_void,
        out_old_len: *mut usize,
    ) -> i32;
    pub(super) fn perry_wasm_host_func_arity(handle: *mut c_void) -> usize;
    pub(super) fn perry_wasm_host_func_call(
        handle: *mut c_void,
        arg_kinds: *const u8,
        arg_bits: *const u64,
        arg_count: usize,
        out_kinds: *mut u8,
        out_bits: *mut u64,
        out_capacity: usize,
        out_count: *mut usize,
        out_err: *mut *mut c_char,
    ) -> i32;
    pub(super) fn perry_wasm_host_call_export(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        arg_kinds: *const u8,
        arg_bits: *const u64,
        arg_count: usize,
        out_kinds: *mut u8,
        out_bits: *mut u64,
        out_capacity: usize,
        out_count: *mut usize,
        out_err: *mut *mut c_char,
    ) -> i32;
    pub(super) fn perry_wasm_host_call_export_by_handle(
        inst: *mut c_void,
        handle: usize,
        arg_kinds: *const u8,
        arg_bits: *const u64,
        arg_count: usize,
        out_kinds: *mut u8,
        out_bits: *mut u64,
        out_capacity: usize,
        out_count: *mut usize,
        out_err: *mut *mut c_char,
    ) -> i32;
}
