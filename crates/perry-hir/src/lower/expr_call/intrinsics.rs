//! Compile-time intrinsics and bare-callee builtin dispatch.

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
