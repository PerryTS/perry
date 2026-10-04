//! #11759 (c′): a class declaration that may be evaluated more than once
//! (`Expr::ClassExprFresh { shared_first_evaluation: Some(_) }`).
//!
//! Its first evaluation is the shared class, the one a single evaluation gets,
//! so a definition that runs once (a bundle wrapper, a function called once)
//! keeps the static class and every fast path keyed on it. The second and
//! later evaluations each create a fresh class object, as node does.
//!
//! The first evaluation is a fact of the template: one module-private word per
//! template, `@perry_class_first_eval.<template>`, defined once per module
//! ([`emit_flag_globals`]), holds the NaN-boxed shared class once the first
//! evaluation handed it out and 0 before. An evaluation loads it, compares it
//! with 0 and branches; a guarded static form (`ClassIsFirstEvaluation`)
//! compares it with the binding's value. The class function object is pinned
//! for the agent's life, so the word needs no root. A declaration
//! `lower::run_once` proves to run once never reaches here: it is lowered as
//! the shared class with no word and no check.

use anyhow::Result;
use perry_hir::Expr;

use crate::rooting::{any_operand_may_collect, with_rooted_accumulator, Arg, Repr};
use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, nanbox_pointer_inline, FnCtx};

/// The flag global of `template` (an LLVM global name, with its `@`).
pub(crate) fn flag_global_name(template: &str) -> String {
    let mut name = String::from("@perry_class_first_eval.");
    for c in template.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.') {
            name.push(c);
        } else {
            name.push_str(&format!("_{:x}_", c as u32));
        }
    }
    name
}

/// Define the flag of every template in `templates` (once each). The flag is
/// per agent when the program starts workers, as a class value is.
pub(crate) fn emit_flag_globals<'a>(
    llmod: &mut crate::module::LlModule,
    templates: impl IntoIterator<Item = &'a String>,
) {
    let tls = if crate::codegen::program_has_worker() {
        "thread_local "
    } else {
        ""
    };
    for template in templates {
        llmod.add_raw_global(format!(
            "{} = private {tls}global i64 0, align 8",
            flag_global_name(template)
        ));
    }
}

/// Lower one evaluation of `evaluation` (its `ClassExprFresh` node): the
/// shared class (running `first_init` against it) when the template's flag is
/// still clear, else a fresh class object.
pub(crate) fn lower(
    ctx: &mut FnCtx<'_>,
    template: &str,
    evaluation_owner: Option<perry_hir::types::LocalId>,
    first_init: &Expr,
    evaluation: &Expr,
) -> Result<String> {
    let Some(&cid) = ctx.class_ids.get(template) else {
        return super::static_field_meta::lower_class_evaluation_object(ctx, evaluation);
    };
    let flag = flag_global_name(template);
    let seen = ctx.block().load(I64, &flag);
    let mut first = ctx.block().icmp_eq(I64, &seen, "0");
    // A declaration extending a shared-first declaration of the same body
    // shares its first evaluation only while that parent's binding holds the
    // parent's first evaluation (the template's static heritage).
    if let Expr::ClassExprFresh {
        evaluated_parent: Some(parent),
        ..
    } = evaluation
    {
        let parent_template = ctx
            .classes
            .get(template)
            .and_then(|c| c.extends_name.clone());
        if let Some(parent_template) = parent_template {
            let parent_value = lower_expr(ctx, parent)?;
            let parent_first = ctx.block().load(I64, &flag_global_name(&parent_template));
            let parent_bits = ctx.block().bitcast_double_to_i64(&parent_value);
            let parent_is_first = ctx.block().icmp_eq(I64, &parent_bits, &parent_first);
            first = ctx.block().and(crate::types::I1, &first, &parent_is_first);
        }
    }
    let first_idx = ctx.new_block("classeval.first");
    let later_idx = ctx.new_block("classeval.later");
    let join_idx = ctx.new_block("classeval.join");
    let first_l = ctx.block_label(first_idx);
    let later_l = ctx.block_label(later_idx);
    let join_l = ctx.block_label(join_idx);
    ctx.block().cond_br(&first, &first_l, &later_l);

    // First evaluation: hand out the shared class. The flag is set before any
    // static initializer runs, so an initializer that re-enters the
    // declaration (or throws after leaking the class) leaves the shared class
    // with this evaluation, and every other evaluation is fresh.
    ctx.current_block = first_idx;
    // The class function object is pinned for the agent's life, so the value
    // stays valid across every collection the initializers may run.
    let shared = super::emit_class_value_cached(ctx, cid);
    let shared_bits = ctx.block().bitcast_double_to_i64(&shared);
    ctx.block().store(I64, &shared_bits, &flag);
    // The shared class learns it is this declaration's first evaluation: its
    // evaluation-state capture slot (an INT32, so no barrier), which the
    // runtime reads to keep later evaluations from standing in for it.
    let state_offset = crate::target_layout::closure_header_size_bytes(ctx.target_triple)
        + 8 * crate::runtime_abi::CLASS_EVALUATION_STATE_CAPTURE as u64;
    let blk = ctx.block();
    let class_ptr = blk.and(I64, &shared_bits, crate::nanbox::POINTER_MASK_I64);
    let state_addr = blk.add(I64, &class_ptr, &state_offset.to_string());
    let state_ptr = blk.inttoptr(I64, &state_addr);
    blk.store(
        I64,
        &(crate::runtime_abi::CLASS_FIRST_EVALUATION_STATE as i64).to_string(),
        &state_ptr,
    );
    if let Some(owner) = evaluation_owner {
        super::static_field_meta::store_evaluation_owner(ctx, owner, &shared);
    }
    publish_first_evaluation_captures(ctx, template, evaluation, &shared)?;
    lower_expr(ctx, first_init)?;
    let first_end = ctx.block_label(ctx.current_block);
    ctx.block().br(&join_l);

    ctx.current_block = later_idx;
    let fresh = super::static_field_meta::lower_class_evaluation_object(ctx, evaluation)?;
    let later_end = ctx.block_label(ctx.current_block);
    ctx.block().br(&join_l);

    ctx.current_block = join_idx;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&shared, &first_end), (&fresh, &later_end)]))
}

/// A class whose members read a guarded class environment (`ClassEnvGet {
/// guarded }`): the first evaluation, the shared class, becomes the
/// environment's owner and publishes its captured values into the slots, as
/// a fresh evaluation does (`js_class_env_evaluate`).
fn publish_first_evaluation_captures(
    ctx: &mut FnCtx<'_>,
    template: &str,
    evaluation: &Expr,
    shared: &str,
) -> Result<()> {
    let Expr::ClassExprFresh { captured_args, .. } = evaluation else {
        return Ok(());
    };
    if captured_args.is_empty() || super::class_env::class_env_state_global(ctx, template).is_none()
    {
        return Ok(());
    }
    let cap_len = captured_args.len().to_string();
    let caps_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_len)]);
    let protect_caps = any_operand_may_collect(ctx, captured_args.iter());
    let caps_box = with_rooted_accumulator(
        ctx,
        Repr::Ptr,
        &caps_arr,
        protect_caps,
        |ctx, acc| {
            // Captured bindings may still be in their dead zone (a `const`
            // declared after the class): the snapshot reads `undefined`, and
            // the refresh after each initializer republishes it.
            ctx.block().call_void("js_tdz_suppress_begin", &[]);
            for arg in captured_args {
                let v = lower_expr(ctx, arg)?;
                acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v)]);
            }
            ctx.block().call_void("js_tdz_suppress_end", &[]);
            Ok(())
        },
        |ctx, arr| Ok(nanbox_pointer_inline(ctx.block(), arr)),
    )?;
    super::class_env::publish_guarded(ctx, template, "js_class_env_evaluate", shared, &caps_box);
    Ok(())
}

/// Does `value` hold the first evaluation of `template`? One load of the
/// template's first-evaluation word and one compare; an `i1`.
pub(crate) fn is_first_i1(ctx: &mut FnCtx<'_>, value: &Expr, template: &str) -> Result<String> {
    let v = lower_expr(ctx, value)?;
    let flag = flag_global_name(template);
    let blk = ctx.block();
    let first = blk.load(I64, &flag);
    let bits = blk.bitcast_double_to_i64(&v);
    Ok(blk.icmp_eq(I64, &bits, &first))
}

/// `ClassIsFirstEvaluation`: does `value` hold the first evaluation of
/// `template`? A NaN-boxed boolean ([`is_first_i1`]).
pub(crate) fn lower_is_first(ctx: &mut FnCtx<'_>, value: &Expr, template: &str) -> Result<String> {
    let same = is_first_i1(ctx, value, template)?;
    Ok(ctx.block().select(
        crate::types::I1,
        &same,
        DOUBLE,
        &crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_TRUE)),
        &crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_FALSE)),
    ))
}
