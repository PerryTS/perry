//! `recv.bind(args…)` (PLAN 3h): when `recv` is a function whose shape is the
//! base function shape (a closure with no own key and `%Function.prototype%`
//! as its [[Prototype]]) and `%Function.prototype%.bind`, `.name` and
//! `.length` are still the builtin's own data properties, the call IS
//! `Function.prototype.bind` and lowers to `js_function_bind` directly. No
//! user code runs in it (the target's `name`/`length` are the prototype's
//! data properties), so a family constructor's run of stores continues
//! across it ([`super::layout_read::family_run_arm`]).
//!
//! The receiver's proof is its shape word, one compare. The prototype's is a
//! closed-world fact (wpfacts `fnproto_bind_written`): compiled code never
//! writes those keys, so the site tests nothing more; where interpreted code
//! can run (decision 150) it tests the runtime's set-only
//! `PERRY_INTERP_PROTO_WRITTEN` word, O(1). A program without facts (pass
//! off, solve over its bound) tests `Pristine(Function)`, the shadow bit of
//! the agent's intrinsic. Any other receiver, or a failed test, takes the
//! site's complete generic dispatch.

use crate::types::{DOUBLE, I1, I16, I32, I64, PTR};

use super::FnCtx;

/// Lower `recv_box.bind(lowered_args…)`: the direct builtin when the tests
/// above hold, else `generic` (the site's dispatch, emitted in the miss
/// block). Arms a family run's bind watch with whether the builtin ran.
pub(crate) fn emit_bind_call(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    lowered_args: &[String],
    generic: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> String {
    use crate::expr::receiver_range::{emit_field_ptr, RECEIVER_BIAS, RECEIVER_SPAN};
    let kind_offset = -(crate::runtime_abi::GC_HEADER_SIZE as i64);
    let kind_mask = (0xFFu16 | (u16::from(crate::runtime_abi::GC_FLAG_FORWARDED) << 8)).to_string();
    let closure_kind = crate::runtime_abi::GC_TYPE_CLOSURE.to_string();
    let kind_idx = ctx.new_block("bind.kind");
    let hit_idx = ctx.new_block("bind.builtin");
    let miss_idx = ctx.new_block("bind.generic");
    let merge_idx = ctx.new_block("bind.merge");
    let kind_l = ctx.block_label(kind_idx);
    let hit_l = ctx.block_label(hit_idx);
    let miss_l = ctx.block_label(miss_idx);
    let merge_l = ctx.block_label(merge_idx);
    let u = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let u = blk.sub(I64, &bits, &(RECEIVER_BIAS as i64).to_string());
        let heap = blk.icmp_ult(I64, &u, &(RECEIVER_SPAN as i64).to_string());
        blk.cond_br(&heap, &kind_l, &miss_l);
        u
    };
    ctx.current_block = kind_idx;
    let shaped = {
        let blk = ctx.block();
        let kp = emit_field_ptr(blk, &u, kind_offset);
        let kind = blk.load(I16, &kp);
        let kind = blk.and(I16, &kind, &kind_mask);
        let is_closure = blk.icmp_eq(I16, &kind, &closure_kind);
        let sp = emit_field_ptr(blk, &u, 4);
        let shape = blk.load(I32, &sp);
        let base = blk.icmp_eq(
            I32,
            &shape,
            &(crate::runtime_abi::FUNCTION_LAYOUT_WORD as i32).to_string(),
        );
        blk.and(I1, &is_closure, &base)
    };
    let ok = if ctx.wpfacts.fnproto_bind_unwritten {
        if ctx.wpfacts.interp_guard {
            let blk = ctx.block();
            let w = blk.load(I32, "@PERRY_INTERP_PROTO_WRITTEN");
            let clean = blk.icmp_eq(I32, &w, "0");
            blk.and(I1, &shaped, &clean)
        } else {
            shaped
        }
    } else {
        let patched = crate::builtin_proto::emit_patched_test(
            ctx,
            crate::builtin_proto::BuiltinProto::Function,
        );
        let blk = ctx.block();
        let pristine = blk.xor(I1, &patched, "true");
        blk.and(I1, &shaped, &pristine)
    };
    ctx.block().cond_br(&ok, &hit_l, &miss_l);

    ctx.current_block = hit_idx;
    crate::builtin_proto::emit_contract_check(
        ctx.block(),
        &[crate::builtin_proto::BuiltinProto::Function],
    );
    let (args_ptr, argc) = crate::lower_call::emit_method_args_buffer(ctx, lowered_args);
    let hit_val = ctx.block().call(
        DOUBLE,
        "js_function_bind",
        &[(DOUBLE, recv_box), (PTR, &args_ptr), (I64, &argc)],
    );
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = miss_idx;
    let miss_val = generic(ctx);
    let miss_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    ctx.current_block = merge_idx;
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&hit_val, &hit_end), (&miss_val, &miss_end)]);
    let armed = matches!(
        ctx.inline_ctor_return.last().map(|b| &b.bind_watch),
        Some(Some(None))
    );
    if armed {
        let builtin = ctx
            .block()
            .phi(I1, &[("true", &hit_end), ("false", &miss_end)]);
        if let Some(body) = ctx.inline_ctor_return.last_mut() {
            body.bind_watch = Some(Some(builtin));
        }
    }
    value
}
