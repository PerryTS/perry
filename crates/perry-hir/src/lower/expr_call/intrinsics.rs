//! Compile-time intrinsics + bare-callee CJS/UMD legacy shapes.
//!
//! Handles, in order: `require(literal)` bail, `embedWasm(literal)`,
//! the
//! `Function('return this')()` globalThis fold, and the
//! `RegExp(pattern, flags?)` bare-call fold.
//!
//! Each helper returns `Result<Option<Expr>>` — `Some` if it matched
//! and the caller should return that expression; `None` to fall
//! through. Extracted from `expr_call/mod.rs` as a mechanical move.
//!
//! The implementation lives in topical sibling modules (`require`,
//! `eval_strict`, `precompile_wasm`, `native_arena`,
//! `bare_builtins`); this trunk re-exports the
//! entry points referenced from `expr_call::mod`.

use crate::ir::*;

mod bare_builtins;
mod eval_strict;
mod native_arena;
mod native_scalars;
mod precompile_wasm;
mod require;

pub(super) use bare_builtins::{try_bare_regexp_call, try_function_return_this, try_iterator_from};
pub(super) use eval_strict::{check_eval_function_call, try_strict_eval_arguments_assignment};
pub(super) use native_arena::{
    try_native_arena_intrinsics, try_native_arena_public_api, try_native_memory_public_api,
    try_pod_layout_constants,
};
pub(super) use native_scalars::validate_native_scalar_conversion_call;
pub(super) use precompile_wasm::{try_embed_wasm, try_precompile};
pub(super) use require::{try_dynamic_require, try_import_meta_require, try_require_literal};
