//! An own property beats a builtin, even on a receiver whose KIND is proven
//! (#10943).
//!
//! ECMA-262 resolves `recv.m(a)` as `Get(recv, "m")` then `Call`, so an own
//! `m` wins. perry lowers a method call to a DIRECT native call whenever it
//! can prove the receiver's kind — and an own property that shadows the method
//! leaves that proof entirely intact: `const m = new Map(); m.get = () => 1`
//! is still provably a Map. The result is a silent, PLAUSIBLE wrong value: a
//! zero-argument `Map.prototype.get` returns `undefined`, `Date.getTime` a
//! real timestamp, `Array.push` a length. Nothing throws.
//!
//! #10476 fixed this for UNPROVEN receivers, whose runtime kind already picks
//! between the builtin and the universal dispatcher. **A proof of KIND was
//! being read as a proof of NO OWN OVERRIDE.**
//!
//! # Why the guard is here and not in an arm
//!
//! `try_lower_property_get_method_call` is an ORDERED CHAIN, and the only arm
//! with a runtime diamond (`builtin_kind_guard`) is near its end — behind
//! `number_string`, the array arm and `map_set`. Every proven receiver is
//! claimed upstream and never reaches it, so extending that diamond emits a
//! guard nothing executes (measured: symbol present, call count 0). The guard
//! has to sit above the whole chain.
//!
//! # What is hoisted, and what is not
//!
//! Only the RECEIVER. The condition needs its value before the branch, so an
//! arm that re-lowered the receiver afterwards would evaluate an effectful one
//! (`make().get(k)`) twice. It is therefore materialised once above the branch
//! and re-read in both arms (`rooting::with_materialized_receiver`).
//!
//! ARGUMENTS are not hoisted. Each arm lowers its own, and a diamond runs
//! exactly one arm, so every argument is still evaluated exactly once at
//! runtime; the cost is two emitted copies, which is code size and not
//! semantics. That is what keeps this change from having to convert every arm
//! in the chain to operand form.
//!
//! The receiver EXPRESSION is passed down unchanged rather than rewritten to a
//! synthetic local, because every arm's proof is keyed on it — `is_array_expr`,
//! `receiver_class_name`, `is_date_receiver`, the `Ptr<Shape>` facts. A
//! synthetic local would erase those proofs and silently un-specialise every
//! one of these calls.
//!
//! # The guard only chooses a branch
//!
//! It never resolves the property and calls it: an own slot can hold a builtin
//! thunk that dispatches by name again, and an earlier attempt at this fix did
//! exactly that and overflowed the stack. The other side of the diamond is the
//! universal dispatcher, which already finds an own or inherited user method.

use anyhow::Result;
use perry_hir::Expr;

use super::helpers::is_date_receiver;
use crate::expr::{lower_expr, FnCtx};
use crate::rooting;
use crate::type_analysis::{is_array_expr, is_map_expr, is_set_expr};
use crate::types::{DOUBLE, I32, I64};

/// Method names a specialised lowering can claim on one of the exotic kinds
/// below. Over-approximating is safe — a name the chain declines simply makes
/// both arms of the diamond reach the same dispatcher — but staying close
/// keeps the emitted diamonds where the bug is.
fn shadowable_builtin_name(property: &str) -> bool {
    matches!(
        property,
        // Map / Set
        "get" | "set" | "has" | "delete" | "add" | "clear" | "entries" | "keys" | "values"
            | "forEach"
        // Array
            | "push" | "pop" | "shift" | "unshift" | "slice" | "splice" | "indexOf"
            | "lastIndexOf" | "includes" | "join" | "concat" | "reverse" | "sort" | "fill"
            | "find" | "findIndex" | "filter" | "map" | "some" | "every" | "reduce"
            | "flat" | "flatMap" | "at"
        // Date
            | "getTime" | "getHours" | "getMinutes" | "getSeconds" | "getMilliseconds"
            | "getDate" | "getDay" | "getMonth" | "getFullYear" | "setHours" | "setMinutes"
            | "setSeconds" | "setDate" | "setMonth" | "setFullYear" | "toISOString"
            | "toJSON" | "getTimezoneOffset" | "valueOf"
        // Number
            | "toFixed" | "toPrecision" | "toExponential"
    )
}

/// Is the receiver's KIND proven to be one whose builtins are lowered
/// directly? Those are exactly the receivers #10476's diamond never sees.
///
/// A string receiver is deliberately absent: a primitive carries no own
/// properties, so its builtin cannot be shadowed on the receiver itself.
fn kind_is_proven_exotic(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    is_map_expr(ctx, object)
        || is_set_expr(ctx, object)
        || is_array_expr(ctx, object)
        || is_date_receiver(ctx, object)
}

/// Does this call need the own-override diamond?
pub(super) fn guards(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    shadowable_builtin_name(property) && kind_is_proven_exotic(ctx, object)
}

/// Branch to `own_label` when `recv` may own a property named `property` that
/// shadows the builtin, else to `builtin_label`.
///
/// The runtime answers in one relaxed load for the overwhelmingly common
/// receiver (`object/own_override.rs`), and never answers "no" for anything it
/// cannot prove: a wrong "no" is a silent wrong value, a wrong "yes" is only
/// slower.
fn emit_own_override_branch(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv: &str,
    own_label: &str,
    builtin_label: &str,
) {
    let key_idx = ctx.strings.intern(property);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let blk = ctx.block();
    let maybe = blk.call(
        I32,
        "js_receiver_may_own_named_method",
        &[(DOUBLE, recv), (I64, &method_id)],
    );
    let may_own = blk.icmp_ne(I32, &maybe, "0");
    blk.cond_br(&may_own, own_label, builtin_label);
}

/// The universal dispatcher, which finds an own or inherited user method. It
/// is the same call `builtin_kind_guard`'s generic block already makes.
fn emit_dispatcher(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<String> {
    let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
    operands.push(object);
    operands.extend(args.iter());
    rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv, arg_vals) = values.split_first().expect("the receiver is operand 0");
        Ok(super::super::console_promise::emit_native_method_str_dispatch(
            ctx,
            property,
            call_byte_offset,
            recv,
            arg_vals,
        ))
    })
}

/// Lower `object.property(args…)` under one own-override diamond.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<String> {
    let receiver = lower_expr(ctx, object)?;
    let key = object as *const Expr as usize;
    rooting::with_materialized_receiver(ctx, key, &receiver, |ctx| {
        let own_idx = ctx.new_block("ownoverride.own");
        let builtin_idx = ctx.new_block("ownoverride.builtin");
        let merge_idx = ctx.new_block("ownoverride.merge");
        let own_label = ctx.block_label(own_idx);
        let builtin_label = ctx.block_label(builtin_idx);
        let merge_label = ctx.block_label(merge_idx);

        let recv = rooting::materialized_receiver_reread(ctx, key)
            .expect("the receiver was materialised above");
        emit_own_override_branch(ctx, property, &recv, &own_label, &builtin_label);

        // The own arm: the universal dispatcher. Its operands are lowered
        // HERE, inside the arm, so they are evaluated once by whichever arm
        // runs — and the receiver operand is served by the materialisation.
        ctx.current_block = own_idx;
        let own_value = emit_dispatcher(ctx, object, property, args, call_byte_offset)?;
        let own_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // The builtin arm: the ordinary chain, unchanged. A chain that
        // declines (an over-approximated name) reaches the same dispatcher, so
        // the diamond is redundant rather than wrong.
        ctx.current_block = builtin_idx;
        let builtin_value =
            match super::lower_method_call_chain(ctx, callee, object, property, args, call_byte_offset)? {
                Some(value) => value,
                None => emit_dispatcher(ctx, object, property, args, call_byte_offset)?,
            };
        let builtin_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[
                (own_value.as_str(), own_end.as_str()),
                (builtin_value.as_str(), builtin_end.as_str()),
            ],
        ))
    })
}
