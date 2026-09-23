//! Class capture environment (`Expr::ClassEnvGet` / `Expr::ClassEnvSet`).
//!
//! A class whose definition is evaluated at most once keeps its captured
//! outer values with the class rather than on every instance (see
//! `perry_hir::lower::run_once` and `synthesize_class_captures`). Each slot is
//! one module-state global — `@perry_classenv_<module>__<class>__<index>` —
//! registered as a mutable GC root with the static-field globals, so a read is
//! one load and a write one rooted store. Codegen defines a slot for every
//! index the class's constructor publishes (it publishes all of them).

use anyhow::Result;
use perry_hir::{Class, Expr, Stmt};

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32};

use super::{emit_root_nanbox_store_for_expr, lower_expr, FnCtx};

/// Number of environment slots `class` uses: one past the highest index its
/// constructor publishes. Zero for a class that keeps instance captures.
pub(crate) fn class_env_slot_count(class: &Class) -> u32 {
    class
        .constructor
        .as_ref()
        .map(|ctor| {
            ctor.body
                .iter()
                .filter_map(|stmt| match stmt {
                    Stmt::Expr(Expr::ClassEnvSet {
                        class_name, index, ..
                    }) if *class_name == class.name => Some(*index + 1),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// The global holding `class_name`'s environment slot `index`, when this
/// module defines one.
pub(crate) fn class_env_global(ctx: &FnCtx<'_>, class_name: &str, index: u32) -> Option<String> {
    ctx.static_field_globals
        .get(&(
            class_name.to_string(),
            perry_hir::cap_fields::class_env_slot_key(index),
        ))
        .map(|name| format!("@{name}"))
}

/// Store an already-lowered capture value into the environment slot, if the
/// class has one. `expr` is the HIR the value came from (drives the proven-
/// scalar store elision of the rooted store).
pub(crate) fn store_class_env_slot(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    index: u32,
    value: &str,
    expr: &Expr,
) {
    if let Some(slot) = class_env_global(ctx, class_name, index) {
        // GC_STORE_AUDIT(ROOT): environment slots are registered mutable
        // roots (`register_module_globals_as_gc_roots` walks every
        // static-field global, and these live in that map).
        emit_root_nanbox_store_for_expr(ctx, value, &slot, expr);
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ClassEnvGet { class_name, index } => {
            if let Some(slot) = class_env_global(ctx, class_name, *index) {
                return Ok(ctx.block().load(DOUBLE, &slot));
            }
            // No slot in this module (nothing published this index): the
            // decl-site snapshot holds the same single evaluation's values.
            Ok(match ctx.class_ids.get(class_name).copied() {
                Some(cid) => {
                    let cid = cid.to_string();
                    let idx = index.to_string();
                    ctx.block().call(
                        DOUBLE,
                        "js_class_capture_value",
                        &[(I32, &cid), (I32, &idx)],
                    )
                }
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            })
        }
        Expr::ClassEnvSet {
            class_name,
            index,
            value,
        } => {
            let v = lower_expr(ctx, value)?;
            store_class_env_slot(ctx, class_name, *index, &v, value);
            Ok(v)
        }
        _ => unreachable!("class_env::lower called with {expr:?}"),
    }
}
