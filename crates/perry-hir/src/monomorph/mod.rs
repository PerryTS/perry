//! Monomorphization pass for generics
//!
//! This module implements monomorphization - the process of generating
//! specialized versions of generic functions and classes for each unique
//! type instantiation. This is similar to how Rust handles generics.
//!
//! For example, given:
//!   function identity<T>(x: T): T { return x; }
//!   identity<number>(42);
//!   identity<string>("hello");
//!
//! We generate:
//!   function identity_number(x: number): number { return x; }
//!   function identity_string(x: string): string { return x; }

use crate::ir::*;
use crate::types::{FuncId, ObjectType, Type};
use std::collections::{HashMap, HashSet, VecDeque};

mod constraints;
mod context;
mod defaults;
mod driver;
mod infer;
mod inference_lookup;
mod mangle;
mod specialize;
mod substitute_expr;
mod substitute_type;
mod update_call_sites;

#[cfg(test)]
mod tests;

pub use constraints::ConstraintError;
pub use context::MonomorphizationContext;
pub use driver::monomorphize_module;
pub use specialize::{specialize_class, specialize_function};
pub use substitute_type::substitute_type;

pub(crate) use constraints::{check_class_constraints, check_function_constraints};
pub(crate) use context::ModuleIndex;
pub(crate) use defaults::fill_default_arguments;
pub(crate) use infer::{
    infer_type_args, infer_type_args_for_class, is_array_like_generic, type_contains_type_var,
    unify_rest_param_types, unify_types,
};
pub(crate) use inference_lookup::{ClassInfo, FuncInfo, InferenceLookup};
pub(crate) use mangle::mangle_type_args;
pub use mangle::{generate_specialized_name, mangle_type};
pub(crate) use substitute_expr::{substitute_expr, substitute_stmts};
pub(crate) use update_call_sites::update_call_sites;
