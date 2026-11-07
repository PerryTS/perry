//! `x.length` of a local the whole-program facts prove holds only program
//! Array literals whose length never moves from its birth length `n`
//! (`ElisionFacts::const_length_inputs`, wpfacts `solve::arr_len`): the
//! read is the constant `n`.
//!
//! The local must be a plain slot of this body (not boxed, not a module
//! global), so the read cannot meet a binding in its TDZ another body
//! observes. Check mode (`PERRY_VERIFY_FACTS=1`) reads the length the
//! ordinary way and requires it to be `n` (fact `array constant length`).
//!
//! `PERRY_FACTS_CONST_LENGTH=0` folds nothing.

use std::cell::Cell;

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I64};

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_CONST_LENGTH").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

thread_local! {
    /// The witness is lowering the ordinary read: no fold inside it.
    static WITNESSING: Cell<bool> = const { Cell::new(false) };
}

/// The constant of `expr` when it is such a read; `Ok(None)` otherwise.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> anyhow::Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return Ok(None);
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return Ok(None);
    };
    if property != "length" || !enabled() || WITNESSING.with(Cell::get) {
        return Ok(None);
    }
    let id = *id;
    if !ctx.locals.contains_key(&id)
        || ctx.boxed_vars.contains(&id)
        || ctx.module_globals.contains_key(&id)
    {
        return Ok(None);
    }
    let Some(&n) = ctx.wpfacts.const_length_inputs.get(&id) else {
        return Ok(None);
    };
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ConstLength,
        || serde_json::json!({ "local": id, "length": n }),
    );
    crate::facts_consumed::used();
    let constant = crate::nanbox::double_literal(n as f64);
    if crate::verify_facts::enabled() {
        WITNESSING.with(|w| w.set(true));
        let actual = crate::expr::property_get::lower(ctx, expr);
        WITNESSING.with(|w| w.set(false));
        let actual = actual?;
        // The ordinary read may hand back an INT32 box.
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&actual);
        let is_int = super::emit_int32_box_test(blk, &actual);
        let low = blk.trunc(I64, &bits, I32);
        let widened = blk.sitofp(I32, &low, DOUBLE);
        let number = blk.select(crate::types::I1, &is_int, DOUBLE, &widened, &actual);
        let same = blk.fcmp("oeq", &number, &constant);
        crate::verify_facts::tick(ctx);
        let miss = ctx.new_block("const.length.verify_miss");
        let next = ctx.new_block("const.length.admitted");
        let miss_l = ctx.block_label(miss);
        let next_l = ctx.block_label(next);
        ctx.block().cond_br(&same, &next_l, &miss_l);
        ctx.current_block = miss;
        crate::verify_facts::const_length_checked(id);
        let site = crate::verify_facts::site_arg(
            ctx,
            &format!("array constant length local {id} = {n}"),
            crate::verify_facts::Fact::ConstLength,
            &[],
        );
        let _ = crate::verify_facts::check(
            ctx,
            I32,
            "perry_wpfacts_check_miss",
            &[(DOUBLE, &actual)],
            &site,
        );
        ctx.block().unreachable();
        ctx.current_block = next;
    }
    Ok(Some(constant))
}
