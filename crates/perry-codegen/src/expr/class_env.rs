//! Class capture environment (`Expr::ClassEnvGet` / `ClassEnvSet` /
//! `ClassEnvStamp`).
//!
//! A class whose definition is evaluated at most once, or a class expression
//! evaluated to a fresh class object per evaluation, keeps its captured outer
//! values with the class rather than on every instance (see
//! `perry_hir::lower::run_once` and `synthesize_class_captures`). Each slot is
//! one module-state global — `@perry_classenv_<module>__<class>__<index>` —
//! registered as a mutable GC root with the static-field globals, so a read is
//! one load and a write one rooted store.
//!
//! A GUARDED class (the fresh-class-expression case) also has a state global,
//! `0.0` while the class has had a single evaluation and `1.0` after a second
//! one. Guarded reads and writes compare it against zero and take the slot
//! directly; otherwise they call into `perry-runtime`'s `class_env`, which
//! resolves the receiver's own evaluation. The evaluation and refresh of a
//! guarded class publish through the runtime, which only lets the class's
//! first evaluation reach the slots.

use anyhow::Result;
use perry_hir::{Class, Expr, Stmt};

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, PTR};

use super::{emit_root_nanbox_store_for_expr, lower_expr, FnCtx};

/// `(slot count, guarded)` for `class`: one past the highest index its
/// constructor publishes, and whether that publish is guarded. Zero slots for
/// a class that keeps instance captures.
pub(crate) fn class_env_layout(class: &Class) -> (u32, bool) {
    let mut count = 0;
    let mut guarded = false;
    if let Some(ctor) = class.constructor.as_ref() {
        for stmt in &ctor.body {
            if let Stmt::Expr(Expr::ClassEnvSet {
                class_name,
                index,
                guarded: g,
                publish: true,
                ..
            }) = stmt
            {
                if *class_name == class.name {
                    count = count.max(*index + 1);
                    guarded |= *g;
                }
            }
        }
    }
    (count, guarded)
}

/// Number of environment slots `class` uses (see [`class_env_layout`]).
pub(crate) fn class_env_slot_count(class: &Class) -> u32 {
    class_env_layout(class).0
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

/// The state global of a guarded class, when this module defines one.
pub(crate) fn class_env_state_global(ctx: &FnCtx<'_>, class_name: &str) -> Option<String> {
    ctx.static_field_globals
        .get(&(
            class_name.to_string(),
            perry_hir::cap_fields::class_env_state_key(),
        ))
        .map(|name| format!("@{name}"))
}

/// Store an already-lowered capture value into an UNGUARDED class's slot. A
/// guarded class publishes through the runtime instead (only its first
/// evaluation may reach the slots), so this is a no-op for it.
pub(crate) fn store_class_env_slot(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    index: u32,
    value: &str,
    expr: &Expr,
) {
    if class_env_state_global(ctx, class_name).is_some() {
        return;
    }
    if let Some(slot) = class_env_global(ctx, class_name, index) {
        // GC_STORE_AUDIT(ROOT): environment slots are registered mutable
        // roots (`register_module_globals_as_gc_roots` walks every
        // static-field global, and these live in that map).
        emit_root_nanbox_store_for_expr(ctx, value, &slot, expr);
    }
}

/// `js_class_env_evaluate` / `js_class_env_refresh` for a guarded class:
/// `class_value` (NaN-boxed) evaluated or refreshed with capture array
/// `caps` (NaN-boxed). No-op for an unguarded class.
pub(crate) fn publish_guarded(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    runtime_fn: &str,
    class_value: &str,
    caps: &str,
) {
    if class_env_state_global(ctx, class_name).is_none() {
        return;
    }
    let Some(cid) = ctx.class_ids.get(class_name).copied() else {
        return;
    };
    let cid = cid.to_string();
    ctx.block().call_void(
        runtime_fn,
        &[(I32, &cid), (DOUBLE, class_value), (DOUBLE, caps)],
    );
}

/// Emit `if (state == 0) { fast } else { slow }` and join the two values.
/// `fast`/`slow` run with the current block set to their branch.
fn branch_on_state(
    ctx: &mut FnCtx<'_>,
    state: &str,
    fast: impl FnOnce(&mut FnCtx<'_>) -> String,
    slow: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> String {
    let st = ctx.block().load(DOUBLE, state);
    let single = ctx.block().fcmp("oeq", &st, "0.0");
    let fast_idx = ctx.new_block("classenv.single");
    let slow_idx = ctx.new_block("classenv.multi");
    let done_idx = ctx.new_block("classenv.done");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&single, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let fast_v = fast(ctx);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&done_label);

    ctx.current_block = slow_idx;
    let slow_v = slow(ctx);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    ctx.block()
        .phi(DOUBLE, &[(&fast_v, &fast_end), (&slow_v, &slow_end)])
}

/// Store `v` into `slot` unless it is `undefined` (see the publish arm of
/// `Expr::ClassEnvSet`).
fn publish_unless_undefined(ctx: &mut FnCtx<'_>, v: &str, slot: &str, value: &Expr) {
    let bits = ctx.block().bitcast_double_to_i64(v);
    let undefined = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
    let present = ctx.block().icmp_ne(crate::types::I64, &bits, &undefined);
    let store_idx = ctx.new_block("classenv.publish");
    let done_idx = ctx.new_block("classenv.publish.done");
    let store_label = ctx.block_label(store_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&present, &store_label, &done_label);
    ctx.current_block = store_idx;
    // GC_STORE_AUDIT(ROOT): registered mutable root slot.
    emit_root_nanbox_store_for_expr(ctx, v, slot, value);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

fn receiver(ctx: &mut FnCtx<'_>) -> String {
    if let Some(this_slot) = ctx.this_stack.last().cloned() {
        ctx.block().load(DOUBLE, &this_slot)
    } else {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    }
}

/// Module-init registration of every guarded class environment this module
/// defines, so the runtime's slow paths can reach the state and slot globals.
pub(crate) fn register_class_envs(ctx: &mut FnCtx<'_>) {
    let mut classes: Vec<(String, u32)> = ctx
        .classes
        .values()
        .map(|c| (c.name.clone(), class_env_slot_count(c)))
        .filter(|(_, n)| *n > 0)
        .collect();
    classes.sort();
    for (name, slots) in classes {
        let Some(state) = class_env_state_global(ctx, &name) else {
            continue;
        };
        let Some(cid) = ctx.class_ids.get(&name).copied() else {
            continue;
        };
        let cid = cid.to_string();
        ctx.block()
            .call_void("js_class_env_register_state", &[(I32, &cid), (PTR, &state)]);
        for index in 0..slots {
            if let Some(slot) = class_env_global(ctx, &name, index) {
                let idx = index.to_string();
                ctx.block().call_void(
                    "js_class_env_register_slot",
                    &[(I32, &cid), (I32, &idx), (PTR, &slot)],
                );
            }
        }
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ClassEnvGet {
            class_name, index, ..
        } => {
            let Some(slot) = class_env_global(ctx, class_name, *index) else {
                // No slot in this module (nothing published this index): the
                // decl-site snapshot holds the same evaluation's values.
                return Ok(match ctx.class_ids.get(class_name).copied() {
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
                });
            };
            let Some(state) = class_env_state_global(ctx, class_name) else {
                return Ok(ctx.block().load(DOUBLE, &slot));
            };
            let cid = ctx
                .class_ids
                .get(class_name)
                .copied()
                .unwrap_or(0)
                .to_string();
            let idx = index.to_string();
            Ok(branch_on_state(
                ctx,
                &state,
                |ctx| ctx.block().load(DOUBLE, &slot),
                |ctx| {
                    let recv = receiver(ctx);
                    ctx.block().call(
                        DOUBLE,
                        "js_class_env_get",
                        &[(DOUBLE, &recv), (I32, &cid), (I32, &idx)],
                    )
                },
            ))
        }
        Expr::ClassEnvSet {
            class_name,
            index,
            value,
            publish,
            ..
        } => {
            let v = lower_expr(ctx, value)?;
            let Some(slot) = class_env_global(ctx, class_name, *index) else {
                return Ok(v);
            };
            let state = class_env_state_global(ctx, class_name);
            if *publish {
                // The constructor's capture params are not always filled from
                // the class's evaluation: `super(...args)` reaching an
                // ancestor constructor through the runtime fills them from the
                // decl-site snapshot, which a class expression does not have,
                // so they arrive `undefined`. The evaluation itself (and every
                // refresh) publishes the real values, so a publish must never
                // overwrite them with a missing param. A guarded class is
                // always a fresh class expression whose evaluation publishes,
                // so its publish is dropped entirely.
                if state.is_none() {
                    publish_unless_undefined(ctx, &v, &slot, value);
                }
                return Ok(v);
            }
            let Some(state) = state else {
                // GC_STORE_AUDIT(ROOT): registered mutable root slot.
                emit_root_nanbox_store_for_expr(ctx, &v, &slot, value);
                return Ok(v);
            };
            let cid = ctx
                .class_ids
                .get(class_name)
                .copied()
                .unwrap_or(0)
                .to_string();
            let idx = index.to_string();
            branch_on_state(
                ctx,
                &state,
                |ctx| {
                    // GC_STORE_AUDIT(ROOT): registered mutable root slot.
                    emit_root_nanbox_store_for_expr(ctx, &v, &slot, value);
                    v.clone()
                },
                |ctx| {
                    let recv = receiver(ctx);
                    ctx.block().call_void(
                        "js_class_env_set",
                        &[(DOUBLE, &recv), (I32, &cid), (I32, &idx), (DOUBLE, &v)],
                    );
                    v.clone()
                },
            );
            Ok(v)
        }
        Expr::ClassEnvStamp {
            class_name,
            instance,
            evaluation,
        } => {
            let inst = lower_expr(ctx, instance)?;
            let Some(state) = class_env_state_global(ctx, class_name) else {
                return Ok(inst);
            };
            let cid = ctx
                .class_ids
                .get(class_name)
                .copied()
                .unwrap_or(0)
                .to_string();
            let mut lowered: Result<()> = Ok(());
            let out = branch_on_state(
                ctx,
                &state,
                |_| inst.clone(),
                |ctx| match lower_expr(ctx, evaluation) {
                    Ok(eval) => ctx.block().call(
                        DOUBLE,
                        "js_class_env_stamp",
                        &[(DOUBLE, &inst), (I32, &cid), (DOUBLE, &eval)],
                    ),
                    Err(e) => {
                        lowered = Err(e);
                        inst.clone()
                    }
                },
            );
            lowered?;
            Ok(out)
        }
        Expr::ClassEnvCurrent { class_name } => {
            let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let Some(state) = class_env_state_global(ctx, class_name) else {
                return Ok(undefined);
            };
            let cid = ctx
                .class_ids
                .get(class_name)
                .copied()
                .unwrap_or(0)
                .to_string();
            Ok(branch_on_state(
                ctx,
                &state,
                |_| undefined.clone(),
                |ctx| {
                    let recv = receiver(ctx);
                    ctx.block().call(
                        DOUBLE,
                        "js_class_env_current",
                        &[(DOUBLE, &recv), (I32, &cid)],
                    )
                },
            ))
        }
        _ => unreachable!("class_env::lower called with {expr:?}"),
    }
}
