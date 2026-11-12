//! The own-override guard for builtin calls HIR already folded (#10943).
//!
//! `lower_call/property_get/own_override_guard.rs` guards the calls codegen's
//! ordered chain lowers. It is not where most of them are: HIR folds
//! `m.get(k)`, `s.has(v)`, `a.push(x)` on a proven receiver into dedicated
//! nodes (`Expr::MapGet`, `Expr::SetHas`, `Expr::ArraySlice`, …) long before
//! codegen sees a `PropertyGet` call, and those nodes lower straight to
//! `js_map_get`/`js_set_has`/`js_array_push`. Measured on #10943's
//! differential: with only the chain guarded, **3** guard calls were emitted
//! and 16 of 30 rows stayed wrong, because the rows go through the fold.
//!
//! So the same diamond is applied here, at the ONE place every folded node
//! passes through — `lower_expr`'s dispatch — rather than in each of the forty
//! lowerings. The table below is what a node has to answer to be guarded: its
//! receiver, the method name a user could shadow, and its arguments.
//!
//! The rules are the chain guard's, unchanged:
//!
//! * the guard only CHOOSES A BRANCH — the other side is the universal
//!   dispatcher, which finds an own or inherited user method. It never
//!   resolves the property and calls it, because an own slot can hold a
//!   builtin thunk that dispatches by name again;
//! * the receiver is materialised once when re-evaluating it could be
//!   observed, and re-read in both arms;
//! * arguments are lowered inside each arm, because a diamond runs one arm.

use anyhow::Result;
use perry_hir::folded_call::{folded_call, FoldedCall, Receiver};
use perry_hir::Expr;
use std::cell::Cell;

use super::{lower_expr, FnCtx};
use crate::lower_call::property_get::own_override_guard::{
    emit_annotated_array_branch, emit_own_override_branch,
};
use crate::rooting;
use crate::types::DOUBLE;

thread_local! {
    /// The identity of the node whose builtin arm is currently re-entering
    /// `lower_expr`. That node — and only that node — must reach the ordinary
    /// fold rather than forming a second diamond around itself.
    ///
    /// It was a DEPTH COUNTER until a review caught what that spells: the
    /// re-entrant `lower_expr(ctx, expr)` lowers the node's ARGUMENTS too, so
    /// a folded builtin nested in an argument was suppressed as well and ran
    /// its native helper with no guard. `m1.set("k", m2.get("k"))` with
    /// `m2.get` an own property printed the native answer while the same call
    /// in statement position printed the own one — the two arms of one diamond
    /// disagreeing for the same source. Verified before fixing: reproduced on
    /// this branch AND on main, so it is #10943 surviving in argument
    /// position, not a regression this guard introduced.
    ///
    /// A positional guarantee ("everything under here is suppressed") cannot
    /// express "this node"; an identity can.
    static SUPPRESSED_NODE: Cell<usize> = const { Cell::new(0) };
}

/// Holds the node identity that was suppressed before this one, so nested
/// diamonds restore rather than clear: while an inner node's builtin arm runs,
/// the outer node is not the one being re-lowered.
struct Suppressed(usize);

impl Suppressed {
    fn enter(node: usize) -> Self {
        Suppressed(SUPPRESSED_NODE.with(|s| s.replace(node)))
    }
}

impl Drop for Suppressed {
    fn drop(&mut self) {
        SUPPRESSED_NODE.with(|s| s.set(self.0));
    }
}

/// The folded calls this guard covers: every [`folded_call`] but
/// `Expr::ArrayPush`. A diamond around that node costs it the INLINE STORE --
/// measured at +94 instructions per call, against +6 for `indexOf`, which is a
/// call on both arms either way -- and buys nothing: every inline push tier's
/// admission mask already tests the array's named layout (`ArrayHeader.named`),
/// where every own named property of an array lives, so a receiver that owns
/// `push` lands in a slow arm, and each slow arm has an exit whose value is the
/// method's return (#11021, `expr/array_push_own.rs`).
fn guarded_call(expr: &Expr) -> Option<FoldedCall<'_>> {
    if matches!(expr, Expr::ArrayPush { .. }) {
        return None;
    }
    folded_call(expr)
}

/// The native array push (`perry_hir::folded_call::native_array_push`:
/// `this.items.push(v)` in value position or in a constructor) as the call it
/// is. Its arm appends with no test of the receiver, so the node gets the
/// diamond here; the test is the inline push tiers' admission
/// (`array_proto_guard::emit_plain_array_method_branch`), behind a heap-pointer
/// test because the receiver is only annotated an Array. Not guarded: a fill
/// push of a push-filled birth (`rest_elems::push_len`), whose Array nothing
/// but its constructor has seen; and nothing with
/// `PERRY_PUSH_METHOD_TIERS=0`.
fn native_push_call<'a>(ctx: &FnCtx<'_>, expr: &'a Expr) -> Option<FoldedCall<'a>> {
    if !super::array_push_own::method_tiers_enabled() || folded_call(expr).is_some() {
        return None;
    }
    let call = perry_hir::folded_call::native_array_push(expr)?;
    match call.receiver {
        Receiver::Expr(recv) if super::rest_elems::push_len(ctx, recv).is_some() => None,
        _ => Some(call),
    }
}

/// Is this folded ARRAY method node's receiver one the function's own
/// analysis did not prove an Array (a module binding read inside a function)?
/// Its guard then reads the Array header behind a heap-pointer test
/// (`own_override_guard::emit_annotated_array_branch`) instead of the global
/// install flag, which no Array arms. `PERRY_ARRAY_METHOD_TIERS=0` keeps the
/// flag.
fn annotated_array_receiver(
    receiver_is_array: bool,
    family: crate::builtin_proto::BuiltinProto,
) -> bool {
    !receiver_is_array
        && family == crate::builtin_proto::BuiltinProto::Array
        && super::array_proto_guard::method_tiers_enabled()
}

/// The builtin-prototype family a folded node's method belongs to (#11915):
/// the receiver kind the fold proved. Every row of [`guarded_call`] that is
/// not a Map, Set or Array method is a Date method.
fn folded_family(expr: &Expr) -> Option<crate::builtin_proto::BuiltinProto> {
    use crate::builtin_proto::BuiltinProto;
    guarded_call(expr)?;
    Some(match expr {
        Expr::MapGet { .. }
        | Expr::MapSet { .. }
        | Expr::MapHas { .. }
        | Expr::MapDelete { .. } => BuiltinProto::Map,
        Expr::SetHas { .. } | Expr::SetDelete { .. } | Expr::SetAdd { .. } => BuiltinProto::Set,
        Expr::NativeMethodCall { .. }
        | Expr::ArrayIndexOf { .. }
        | Expr::ArraySlice { .. }
        | Expr::ArrayForEach { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFind { .. }
        | Expr::ArrayFindIndex { .. }
        | Expr::ArrayFindLast { .. }
        | Expr::ArrayFindLastIndex { .. }
        | Expr::ArraySome { .. }
        | Expr::ArrayEvery { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArraySort { .. }
        | Expr::ArrayReduce { .. }
        | Expr::ArrayReduceRight { .. }
        | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayIncludes { .. }
        | Expr::ArrayAt { .. }
        | Expr::ArrayJoin { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayWith { .. }
        | Expr::ArrayEntries(_)
        | Expr::ArrayKeys(_)
        | Expr::ArrayValues(_)
        | Expr::ArrayShift(_)
        | Expr::ArrayUnshift { .. }
        | Expr::ArraySplice { .. }
        | Expr::ArrayCopyWithin { .. } => BuiltinProto::Array,
        _ => BuiltinProto::Date,
    })
}

/// Guard a folded builtin-method node, or return `None` for everything else.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let node = expr as *const Expr as usize;
    if SUPPRESSED_NODE.with(|s| s.get()) == node {
        return Ok(None);
    }
    let native_push = native_push_call(ctx, expr);
    let is_native_push = native_push.is_some();
    let Some(call) = native_push.or_else(|| guarded_call(expr)) else {
        return Ok(None);
    };

    let local_receiver;
    let receiver_expr: &Expr = match call.receiver {
        Receiver::Expr(expr) => expr,
        Receiver::Local(id) => {
            local_receiver = Expr::LocalGet(id);
            &local_receiver
        }
    };
    // `(12345).toLocaleString()` shares `Expr::DateToLocaleString` with the
    // Date spelling. A number is a primitive and owns nothing, so a receiver
    // proven numeric keeps the plain fold.
    if matches!(expr, Expr::DateToLocaleString(_))
        && crate::type_analysis::is_numeric_expr(ctx, receiver_expr)
    {
        return Ok(None);
    }
    // A receiver annotated a typed array whose values the facts do not prove
    // typed arrays (`method_site`): the node is the ordinary method call.
    if crate::method_site::unproven_typed_array_call(
        ctx,
        perry_hir::folded_call::receiver_site(&call.receiver),
        receiver_expr,
        call.method,
    )? {
        return emit_generic(ctx, receiver_expr, call.method, &call.args).map(Some);
    }
    // A local receiver (and a fold that captured one) is already in a slot the
    // collector rewrites, so re-lowering it IS the re-read the rooting
    // invariant wants; only a receiver that cannot be evaluated twice needs a
    // materialisation of its own.
    //
    // #11915: the guard runs BEFORE the arguments (ECMA-262 reads `recv.m`
    // before evaluating them), so each arm lowers its own arguments — one arm
    // runs. The direct arm is the ordinary fold; the generic arm is one call
    // into the shared runtime helper, which performs the real `Get` and call.
    // #11915: a folded Array method with a checked entry, whose arguments
    // cannot observe when the method is read, is ONE call: the entry proves
    // `Pristine(Array)` and the receiver's own proof (no own property of the
    // name, no own `[[Prototype]]`) and runs the fold's body, or takes the
    // generic path. No diamond, no test at the site.
    if let Some((entry, observing)) = folded_array_entry(ctx, expr, &call) {
        if !observing {
            let mut operands: Vec<&Expr> = Vec::with_capacity(call.args.len() + 1);
            operands.push(receiver_expr);
            operands.extend(call.args.iter().copied());
            return rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
                let ops: Vec<(crate::types::LlvmType, &str)> =
                    values.iter().map(|v| (DOUBLE, v.as_str())).collect();
                Ok(ctx.block().call(DOUBLE, &entry, &ops))
            })
            .map(Some);
        }
        return lower_pre_read_entry(ctx, receiver_expr, &call, &entry).map(Some);
    }
    let materialize = matches!(call.receiver, Receiver::Expr(_))
        && !matches!(receiver_expr, Expr::LocalGet(_) | Expr::This);
    let receiver_is_array = matches!(expr, Expr::ArrayIndexOf { .. } | Expr::ArraySlice { .. })
        || crate::type_analysis::is_array_expr(ctx, receiver_expr);
    let family = if is_native_push {
        crate::builtin_proto::BuiltinProto::Array
    } else {
        folded_family(expr).unwrap_or(crate::builtin_proto::BuiltinProto::Object)
    };
    let annotated_array = annotated_array_receiver(receiver_is_array, family);
    let method = call.method;

    let receiver_value = lower_expr(ctx, receiver_expr)?;
    let key = receiver_expr as *const Expr as usize;
    let emit = |ctx: &mut FnCtx<'_>| -> Result<String> {
        let generic_idx = ctx.new_block("ownoverride.folded.own");
        let builtin_idx = ctx.new_block("ownoverride.folded.builtin");
        let merge_idx = ctx.new_block("ownoverride.folded.merge");
        let generic_label = ctx.block_label(generic_idx);
        let builtin_label = ctx.block_label(builtin_idx);
        let merge_label = ctx.block_label(merge_idx);

        let recv = match rooting::materialized_receiver_reread(ctx, key) {
            Some(value) => value,
            None => lower_expr(ctx, receiver_expr)?,
        };
        if is_native_push {
            super::array_proto_guard::emit_plain_array_method_branch(
                ctx,
                &recv,
                &builtin_label,
                &generic_label,
            );
        } else if annotated_array {
            emit_annotated_array_branch(
                ctx,
                method,
                &recv,
                Some(family),
                &generic_label,
                &builtin_label,
            );
        } else {
            emit_own_override_branch(
                ctx,
                method,
                &recv,
                receiver_is_array,
                Some(family),
                &generic_label,
                &builtin_label,
            );
        }

        ctx.current_block = generic_idx;
        let generic_value = emit_generic(ctx, receiver_expr, method, &call.args)?;
        let generic_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = builtin_idx;
        crate::builtin_proto::emit_contract_check(ctx.block(), &[family]);
        let builtin_value = {
            let _suppressed = Suppressed::enter(node);
            lower_expr(ctx, expr)?
        };
        let builtin_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[
                (generic_value.as_str(), generic_end.as_str()),
                (builtin_value.as_str(), builtin_end.as_str()),
            ],
        ))
    };

    let value = if materialize {
        rooting::with_materialized_receiver(ctx, key, &receiver_value, emit)?
    } else {
        emit(ctx)?
    };
    Ok(Some(value))
}

/// The checked entry a folded Array node calls instead of its guard
/// (#11915), when it has one for its arity, its arguments are order-free
/// (`args_may_observe_lookup`), and its fold is a plain runtime call (the
/// captureless `some` loop is inline, and keeps the guard).
fn folded_array_entry(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    call: &FoldedCall<'_>,
) -> Option<(String, bool)> {
    if folded_family(expr) != Some(crate::builtin_proto::BuiltinProto::Array) {
        return None;
    }
    // A certified sort selects its operation after the method-identity
    // guard. The shared checked entry would erase the HIR certificate and
    // always call comparator sorting, just as it would erase an inline some.
    if let Expr::ArraySort { comparator, .. } = expr {
        if super::numeric_order::numeric_order(ctx, comparator).is_some() {
            return None;
        }
    }
    if let Expr::ArraySome { callback, .. } = expr {
        if super::logical_collections::captureless_some_callback(ctx, callback).is_some() {
            return None;
        }
    }
    let entry = crate::builtin_proto::array_entry(call.method, call.args.len())?;
    let observing = call
        .args
        .iter()
        .any(|a| crate::expr::method_site::args_may_observe_lookup(ctx, std::slice::from_ref(*a)));
    Some((entry, observing))
}

/// A folded Array method whose arguments could observe WHEN the method is
/// read (#11915): the receiver, then the proof — the own-override test with
/// the Array byte, as the guard has it — BEFORE the arguments, which are then
/// evaluated ONCE; a failed proof takes one generic call, a held one the
/// unchecked body `js_array_b_<method><argc>`.
fn lower_pre_read_entry(
    ctx: &mut FnCtx<'_>,
    receiver_expr: &Expr,
    call: &FoldedCall<'_>,
    entry: &str,
) -> Result<String> {
    use crate::rooting::{open_rooted_group, Repr};
    let receiver_is_array = crate::type_analysis::is_array_expr(ctx, receiver_expr);
    let receiver_value = lower_expr(ctx, receiver_expr)?;
    let mut group = open_rooted_group(call.args.len() + 1);
    // The own-override predicate behind the test may allocate.
    let rooted_recv = group.adopt_emitted(ctx, Repr::Boxed, &receiver_value, true);
    let recv = group.reread_emitted(ctx, rooted_recv);
    let failed_idx = ctx.new_block("arrentry.pre_failed");
    let held_idx = ctx.new_block("arrentry.pre_held");
    let args_idx = ctx.new_block("arrentry.args");
    let failed_label = ctx.block_label(failed_idx);
    let held_label = ctx.block_label(held_idx);
    let args_label = ctx.block_label(args_idx);
    if annotated_array_receiver(receiver_is_array, crate::builtin_proto::BuiltinProto::Array) {
        emit_annotated_array_branch(
            ctx,
            call.method,
            &recv,
            Some(crate::builtin_proto::BuiltinProto::Array),
            &failed_label,
            &held_label,
        );
    } else {
        emit_own_override_branch(
            ctx,
            call.method,
            &recv,
            receiver_is_array,
            Some(crate::builtin_proto::BuiltinProto::Array),
            &failed_label,
            &held_label,
        );
    }
    ctx.current_block = failed_idx;
    ctx.block().br(&args_label);
    ctx.current_block = held_idx;
    ctx.block().br(&args_label);
    ctx.current_block = args_idx;
    let patched = ctx.block().phi(
        crate::types::I1,
        &[("true", &failed_label), ("false", &held_label)],
    );
    for (i, arg) in call.args.iter().enumerate() {
        let collects = rooting::any_operand_may_collect(ctx, call.args[i + 1..].iter().copied());
        group.lower(ctx, arg, collects)?;
    }
    let recv = group.reread_emitted(ctx, rooted_recv);
    let args = group.reread_all(ctx)?;

    let generic_idx = ctx.new_block("arrentry.generic");
    let direct_idx = ctx.new_block("arrentry.direct");
    let merge_idx = ctx.new_block("arrentry.merge");
    let generic_label = ctx.block_label(generic_idx);
    let direct_label = ctx.block_label(direct_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&patched, &generic_label, &direct_label);

    ctx.current_block = generic_idx;
    let generic_value = crate::builtin_proto::emit_generic_call(
        ctx,
        crate::builtin_proto::GenericRead::Receiver,
        &recv,
        call.method,
        &args,
    );
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = direct_idx;
    let body = entry.replacen("js_array_m_", "js_array_b_", 1);
    let mut operands: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, recv.as_str())];
    operands.extend(args.iter().map(|a| (DOUBLE, a.as_str())));
    let direct_value = ctx.block().call(DOUBLE, &body, &operands);
    let direct_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let value = ctx.block().phi(
        DOUBLE,
        &[
            (generic_value.as_str(), generic_end.as_str()),
            (direct_value.as_str(), direct_end.as_str()),
        ],
    );
    group.release(ctx);
    Ok(value)
}

/// The guard's generic arm: the receiver (re-read) and the arguments, each
/// rooted across the next, then ONE call into the shared runtime helper
/// (`builtin_proto::emit_generic_call`): the real `Get` — an own property, a
/// replaced prototype method — and call; the builtin itself continues by name
/// in the runtime.
fn emit_generic(
    ctx: &mut FnCtx<'_>,
    receiver: &Expr,
    method: &str,
    args: &[&Expr],
) -> Result<String> {
    let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
    operands.push(receiver);
    operands.extend(args.iter().copied());
    rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv, arg_vals) = values.split_first().expect("the receiver is operand 0");
        Ok(crate::builtin_proto::emit_generic_call(
            ctx,
            crate::builtin_proto::GenericRead::Receiver,
            recv,
            method,
            arg_vals,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::guarded_call;
    use perry_hir::Expr;

    /// `push` is not guarded HERE. The diamond would come back and with it the
    /// +94 instructions per call that made this one node the exception:
    /// guarding `push` costs it the INLINE STORE. An own `push` is honoured by
    /// the node's own slow arms instead (#11021), whose admission is the
    /// header bit the inline tier already tests.
    #[test]
    fn array_push_is_not_a_folded_guard_target() {
        let push = Expr::ArrayPush {
            array_id: 7,
            value: Box::new(Expr::Number(1.0)),
            field_writeback: None,
        };
        assert!(
            guarded_call(&push).is_none(),
            "ArrayPush must not be guarded: a diamond costs it the inline store, \
             and its slow arms already honour an own `push` (#11021)"
        );
    }
}
