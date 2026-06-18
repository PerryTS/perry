//! Destructuring lowering.
//!
//! Contains functions for lowering destructuring assignments and variable
//! declarations with destructuring patterns.
//!
//! Organized into topical sub-modules:
//! - [`helpers`] — small utility predicates / pattern recognizers shared
//!   across the other sub-modules (e.g. `useState` tuple rewrite,
//!   recursive AST scans).
//! - [`assignment_stmt`] — `[a, b] = expr;` lowered as a statement.
//! - [`assignment_expr`] — `[a, b] = expr` lowered as an expression.
//! - [`pattern_binding`] — recursive binding-pattern walker
//!   (`let { a, b } = expr`).
//! - [`var_decl`] — full variable-declaration lowering, including the
//!   destructuring case.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

mod assignment_expr;
mod assignment_stmt;
mod helpers;
mod pattern_binding;
mod var_decl;
mod var_decl_sources;

pub(crate) use assignment_expr::lower_destructuring_assignment;
pub(crate) use assignment_stmt::{
    lower_destructuring_assignment_stmt, lower_destructuring_assignment_stmt_from_local,
};
pub(crate) use helpers::{ast_expr_contains_function_expr, rewrite_use_state_tuple};
pub(crate) use pattern_binding::lower_pattern_binding;
pub(crate) use var_decl::lower_var_decl_with_destructuring;
pub(crate) use var_decl_sources::resolvable_native_module_for_spec;
