//! LLVM Code Generation for Perry
//!
//! Produces textual LLVM IR (`.ll`) from Perry's HIR, then shells out to
//! `clang -c` to build an object file linked against `libperry_runtime.a`.
//! This is Perry's sole native code generation backend (since v0.5.0).

pub mod block;
pub(crate) mod boxed_vars;
pub mod codegen;
pub(crate) mod collectors;
pub(crate) mod expr;
pub mod function;
pub mod linker;
pub(crate) mod loop_purity;
pub(crate) mod lower_array_method;
pub(crate) mod lower_call;
pub(crate) mod lower_conditional;
pub(crate) mod lower_string_method;
pub mod module;
pub mod nanbox;
pub mod runtime_decls;
pub(crate) mod stmt;
pub mod strings;
pub mod stubs;
pub(crate) mod type_analysis;
pub mod types;

pub use codegen::{compile_module, resolve_target_triple, CompileOptions, ImportedClass};
