//! The retained IIFE fold's source is a fresh ordinary function literal:
//! its own shape is known by construction. Wpfacts proves its call method
//! unchanged, or the site tests the intrinsic prototype's actual layout word.
use crate::builtin_proto::{self, BuiltinProto};
use crate::expr::FnCtx;
use crate::types::{I1, I64};

pub(crate) fn lower_proof(
    ctx: &mut FnCtx<'_>,
    family: Option<&str>,
    source: Option<&perry_hir::Expr>,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        family == Some("Function.prototype") && source.is_some(),
        "invalid IIFE proof"
    );
    let patched = if ctx.wpfacts.fnproto_borrow_unwritten && !ctx.wpfacts.interp_guard {
        "false".to_string()
    } else {
        builtin_proto::emit_patched_test(ctx, BuiltinProto::Function)
    };
    let ok = ctx.block().xor(I1, &patched, "true");
    if builtin_proto::contract_mode() {
        let check = ctx.new_block("borrow.contract");
        let done = ctx.new_block("borrow.admitted");
        let check_l = ctx.block_label(check);
        let done_l = ctx.block_label(done);
        ctx.block().cond_br(&ok, &check_l, &done_l);
        ctx.current_block = check;
        builtin_proto::emit_contract_check(ctx.block(), &[BuiltinProto::Function]);
        ctx.block().br(&done_l);
        ctx.current_block = done;
    }
    let tagged = ctx.block().select(
        I1,
        &ok,
        I64,
        crate::nanbox::TAG_TRUE_I64,
        crate::nanbox::TAG_FALSE_I64,
    );
    Ok(ctx.block().bitcast_i64_to_double(&tagged))
}
