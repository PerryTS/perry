//! HIR → WebAssembly bytecode emitter
//!
//! Translates HIR modules to WebAssembly binary format using wasm-encoder.
//! All JSValues are represented as i64 using NaN-boxing bit patterns.
//! Arithmetic operations temporarily convert to f64 and back.
//! Runtime operations (strings, console, objects) are imported from a JS bridge.
//!
//! ## Module layout
//!
//! `mod.rs` is the entry point and the cross-sibling re-export hub. Every
//! sibling file in this directory does `use super::*;` to pull in the shared
//! prelude built up below.
//!
//! - `constants` — NaN-boxing tag constants + `EnumResolvedValue` + `f64_const`
//! - `runtime_imports` — `RuntimeImports` struct (JS-bridge function indices)
//! - `ui_method_map` — `map_ui_method` perry/ui name → bridge function map
//! - `module_emitter` — `WasmModuleEmitter` struct + `new` / `intern_string` / `get_type_idx`
//! - `compile` — the giant `WasmModuleEmitter::compile` orchestration method
//! - `func_emit_ctx` — `FuncEmitCtx` struct + `new`
//! - `binary`, `closures`, `expr/`, `function`, `js_fallback`, `locals`,
//!   `memcall`, `method_call`, `stmt`, `string_collection` — pre-existing
//!   wave-1 sibling files

mod binary;
mod closures;
mod compile;
mod constants;
mod expr;
mod func_emit_ctx;
mod function;
mod js_fallback;
mod locals;
mod memcall;
mod method_call;
mod module_emitter;
mod runtime_imports;
mod stmt;
mod string_collection;
mod ui_method_map;

// Shared prelude — each sibling re-imports these via `use super::*;`.
use perry_hir::ir::*;
use perry_hir::types::{FuncId, GlobalId, LocalId};
use std::collections::BTreeMap;
use wasm_encoder::{
    CodeSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, GlobalSection, GlobalType, Ieee64, ImportSection, Instruction,
    MemorySection, MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
};

// Explicit named re-exports of cross-sibling items. Sub-modules with
// `use super::*;` see these names at module scope.
use closures::{collect_closures_from_expr, collect_closures_from_stmts};
// `f64_const_bits` is held alive for future use (matches the pre-split
// `#[allow(dead_code)]` annotation on its original definition).
use constants::f64_const_bits;
use constants::{
    f64_const, EnumResolvedValue, STRING_TAG, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
};
use func_emit_ctx::FuncEmitCtx;
use locals::{
    collect_exported_names, collect_locals, collect_module_let_ids, resolve_export_to_func,
    resolve_export_to_let, resolve_source_module_idx,
};
use module_emitter::WasmModuleEmitter;
use runtime_imports::RuntimeImports;
use stmt::has_return;
use ui_method_map::map_ui_method;

/// Output from WASM compilation: binary + extra JS for async functions.
pub struct WasmCompileOutput {
    pub wasm_bytes: Vec<u8>,
    pub async_js: String,
    /// FFI function names that must be provided as imports under the "ffi" namespace.
    pub ffi_imports: Vec<String>,
}

/// Compile HIR modules to a WebAssembly binary.
pub fn compile_to_wasm(modules: &[(String, perry_hir::ir::Module)]) -> Vec<u8> {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules).wasm_bytes
}

/// Compile HIR modules to WASM binary + generated JS for async functions.
pub fn compile_to_wasm_with_async(
    modules: &[(String, perry_hir::ir::Module)],
) -> WasmCompileOutput {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules)
}
