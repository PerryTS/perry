//! Expression emission - dispatcher hub.
//!
//! The original `FuncEmitCtx::emit_expr` (~4.6k-line match on `Expr`) was
//! mechanically split into topical sibling modules. Each sibling exposes a
//! `try_emit_expr_<topic>` method that pattern-matches only its own slice of
//! `Expr` variants and returns `true` if it handled the expression. This
//! `emit_expr` is the single dispatcher that walks them in original-file
//! order, falling through to the `_ => TAG_UNDEFINED` catch-all at the end.
//!
//! Pure code movement - see #1102 for the parent issue. No public-API or
//! behavioral changes; per-variant semantics are identical to the pre-split
//! file.

mod arrays;
mod buffers;
mod calls;
mod classes;
mod date_error;
mod literals_vars;
mod map_set;
mod math;
mod native_method;
mod net_fetch_crypto;
mod objects;
mod regex_globals;
mod strings_json;
mod url_process_path;

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn emit_expr(&mut self, func: &mut Function, expr: &Expr) {
        if self.try_emit_expr_literals_vars(func, expr) {
            return;
        }
        if self.try_emit_expr_calls(func, expr) {
            return;
        }
        if self.try_emit_expr_native_method(func, expr) {
            return;
        }
        if self.try_emit_expr_math(func, expr) {
            return;
        }
        if self.try_emit_expr_objects(func, expr) {
            return;
        }
        if self.try_emit_expr_arrays(func, expr) {
            return;
        }
        if self.try_emit_expr_classes(func, expr) {
            return;
        }
        if self.try_emit_expr_strings_json(func, expr) {
            return;
        }
        if self.try_emit_expr_map_set(func, expr) {
            return;
        }
        if self.try_emit_expr_date_error(func, expr) {
            return;
        }
        if self.try_emit_expr_regex_globals(func, expr) {
            return;
        }
        if self.try_emit_expr_url_process_path(func, expr) {
            return;
        }
        if self.try_emit_expr_buffers(func, expr) {
            return;
        }
        if self.try_emit_expr_net_fetch_crypto(func, expr) {
            return;
        }
        // --- Catch-all: emit undefined ---
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
    }
}
