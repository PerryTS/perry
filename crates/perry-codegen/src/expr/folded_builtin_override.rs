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
use perry_hir::Expr;
use std::cell::Cell;

use super::{lower_expr, FnCtx};
use crate::lower_call::property_get::own_override_guard::emit_own_override_branch;
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

/// Where a folded node's receiver comes from.
enum Receiver<'a> {
    /// A receiver expression, which may be effectful and is therefore
    /// materialised once.
    Expr(&'a Expr),
    /// A local the fold captured. Re-reading a local is free and cannot be
    /// observed, so it needs no materialisation.
    Local(u32),
}

struct FoldedCall<'a> {
    receiver: Receiver<'a>,
    /// The name a user property would have to carry to shadow this call.
    method: &'static str,
    args: Vec<&'a Expr>,
}

/// The folded builtin-method nodes this guard covers, and what each one is a
/// call of. A node absent from this table keeps today's behaviour: its builtin
/// runs even when an own property shadows it, which is #10943 for that
/// spelling and is why the table is meant to grow to every folded method.
fn folded_call(expr: &Expr) -> Option<FoldedCall<'_>> {
    let call = match expr {
        Expr::MapGet { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "get",
            args: vec![key],
        },
        Expr::MapSet { map, key, value } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "set",
            args: vec![key, value],
        },
        Expr::MapHas { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "has",
            args: vec![key],
        },
        Expr::MapDelete { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "delete",
            args: vec![key],
        },
        Expr::SetHas { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "has",
            args: vec![value],
        },
        Expr::SetDelete { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "delete",
            args: vec![value],
        },
        Expr::SetAdd { set_id, value } => FoldedCall {
            receiver: Receiver::Local(*set_id),
            method: "add",
            args: vec![value],
        },
        // `Expr::ArrayPush` is deliberately ABSENT: it honours an own `push`
        // itself (#11021, `expr/array_push_own.rs`). A diamond around this node
        // costs it the INLINE STORE -- measured at +94 instructions per call,
        // against +6 for `indexOf`, which is a call on both arms either way --
        // and buys nothing: every inline push tier's admission mask already
        // tests the array's named layout (`ArrayHeader.named`), where every own
        // named property of an array lives, so a receiver that owns `push` lands
        // in a slow arm, and each slow arm has an exit whose value is the
        // method's return.
        // `a.push()` with NO arguments is not an `ArrayPush`: HIR folds it to
        // this native call so `Set(O, "length", …)` still throws on a frozen
        // array. It is a call on both arms either way, so unlike `ArrayPush`
        // a diamond here costs no inline store (#11021).
        Expr::NativeMethodCall {
            module,
            method,
            object: Some(object),
            args,
            ..
        } if module == "array" && method == "push" && args.is_empty() => FoldedCall {
            receiver: Receiver::Expr(object),
            method: "push",
            args: Vec::new(),
        },
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "indexOf",
            args: match from_index {
                Some(from) => vec![value, from],
                None => vec![value],
            },
        },
        Expr::ArraySlice {
            array, start, end, ..
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "slice",
            args: match end {
                Some(end) => vec![start, end],
                None => vec![start],
            },
        },
        // Date. HIR folds every Date accessor into its own variant, and they
        // reach `js_date_apply_setter` / the getter helpers without passing
        // the property chain at all — which is why `d.setHours = () => 1`
        // emitted no guard and ran the builtin. A getter carries the receiver
        // alone; a setter carries its argument list.
        // #11915: every folded Array method reads `%Array.prototype%`, so it
        // takes the same guard: an own property OR a modified prototype goes
        // to the dispatcher, whose runtime entry performs the real `Get`.
        Expr::ArrayForEach { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "forEach",
            args: vec![callback],
        },
        Expr::ArrayMap { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "map",
            args: vec![callback],
        },
        Expr::ArrayFilter { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "filter",
            args: vec![callback],
        },
        Expr::ArrayFind { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "find",
            args: vec![callback],
        },
        Expr::ArrayFindIndex { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findIndex",
            args: vec![callback],
        },
        Expr::ArrayFindLast { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findLast",
            args: vec![callback],
        },
        Expr::ArrayFindLastIndex { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findLastIndex",
            args: vec![callback],
        },
        Expr::ArraySome { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "some",
            args: vec![callback],
        },
        Expr::ArrayEvery { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "every",
            args: vec![callback],
        },
        Expr::ArrayFlatMap { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "flatMap",
            args: vec![callback],
        },
        Expr::ArraySort { array, comparator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "sort",
            args: vec![comparator],
        },
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "reduce",
            args: std::iter::once(callback.as_ref())
                .chain(initial.as_deref())
                .collect(),
        },
        Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "reduceRight",
            args: std::iter::once(callback.as_ref())
                .chain(initial.as_deref())
                .collect(),
        },
        Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "lastIndexOf",
            args: std::iter::once(value.as_ref())
                .chain(from_index.as_deref())
                .collect(),
        },
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "includes",
            args: std::iter::once(value.as_ref())
                .chain(from_index.as_deref())
                .collect(),
        },
        Expr::ArrayAt { array, index } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "at",
            args: vec![index],
        },
        Expr::ArrayJoin { array, separator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "join",
            args: separator.as_deref().into_iter().collect(),
        },
        Expr::ArrayFlat { array } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "flat",
            args: Vec::new(),
        },
        Expr::ArrayToReversed { array } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toReversed",
            args: Vec::new(),
        },
        Expr::ArrayToSorted { array, comparator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toSorted",
            args: comparator.as_deref().into_iter().collect(),
        },
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toSpliced",
            args: [start.as_ref(), delete_count.as_ref()]
                .into_iter()
                .chain(items.iter())
                .collect(),
        },
        Expr::ArrayWith {
            array,
            index,
            value,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "with",
            args: vec![index, value],
        },
        Expr::ArrayEntries(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "entries",
            args: Vec::new(),
        },
        Expr::ArrayKeys(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "keys",
            args: Vec::new(),
        },
        Expr::ArrayValues(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "values",
            args: Vec::new(),
        },
        Expr::ArrayShift(array_id) => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "shift",
            args: Vec::new(),
        },
        Expr::ArrayUnshift { array_id, value } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "unshift",
            args: vec![value],
        },
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "splice",
            args: std::iter::once(start.as_ref())
                .chain(delete_count.as_deref())
                .chain(items.iter())
                .collect(),
        },
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "copyWithin",
            args: [target.as_ref(), start.as_ref()]
                .into_iter()
                .chain(end.as_deref())
                .collect(),
        },
        Expr::DateGetTime(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getTime",
            args: Vec::new(),
        },
        Expr::DateToISOString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toISOString",
            args: Vec::new(),
        },
        Expr::DateGetFullYear(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getFullYear",
            args: Vec::new(),
        },
        Expr::DateGetMonth(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMonth",
            args: Vec::new(),
        },
        Expr::DateGetDate(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getDate",
            args: Vec::new(),
        },
        Expr::DateGetDay(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getDay",
            args: Vec::new(),
        },
        Expr::DateGetHours(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getHours",
            args: Vec::new(),
        },
        Expr::DateGetMinutes(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMinutes",
            args: Vec::new(),
        },
        Expr::DateGetSeconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getSeconds",
            args: Vec::new(),
        },
        Expr::DateGetMilliseconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMilliseconds",
            args: Vec::new(),
        },
        Expr::DateGetUtcDay(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCDay",
            args: Vec::new(),
        },
        Expr::DateGetUtcFullYear(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCFullYear",
            args: Vec::new(),
        },
        Expr::DateGetUtcMonth(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMonth",
            args: Vec::new(),
        },
        Expr::DateGetUtcDate(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCDate",
            args: Vec::new(),
        },
        Expr::DateGetUtcHours(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCHours",
            args: Vec::new(),
        },
        Expr::DateGetUtcMinutes(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMinutes",
            args: Vec::new(),
        },
        Expr::DateGetUtcSeconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCSeconds",
            args: Vec::new(),
        },
        Expr::DateGetUtcMilliseconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMilliseconds",
            args: Vec::new(),
        },
        // The remaining zero-argument Date folds (#11493). They reach their
        // formatters exactly as the getters above do, and were simply missing
        // from this table: `d.toDateString = () => 1` ran the builtin.
        //
        // `Expr::DateToUTCString` is deliberately ABSENT. HIR folds both
        // `toUTCString` and `toGMTString` into it, so the node does not record
        // which name the source used, and the guard needs that name twice: to
        // test which property the receiver owns, and to dispatch it. Guessing
        // `toUTCString` would make `d.toGMTString()` call an own `toUTCString`,
        // which is wrong in the other direction. Guarding it needs the fold to
        // keep its spelling.
        Expr::DateGetTimezoneOffset(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getTimezoneOffset",
            args: Vec::new(),
        },
        Expr::DateToJSON(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toJSON",
            args: Vec::new(),
        },
        Expr::DateToDateString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toDateString",
            args: Vec::new(),
        },
        Expr::DateToTimeString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toTimeString",
            args: Vec::new(),
        },
        Expr::DateToLocaleDateString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleDateString",
            args: Vec::new(),
        },
        Expr::DateToLocaleTimeString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleTimeString",
            args: Vec::new(),
        },
        Expr::DateToLocaleString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleString",
            args: Vec::new(),
        },
        Expr::DateSetFullYear { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setFullYear",
            args: args.iter().collect(),
        },
        Expr::DateSetMonth { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMonth",
            args: args.iter().collect(),
        },
        Expr::DateSetDate { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setDate",
            args: args.iter().collect(),
        },
        Expr::DateSetHours { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setHours",
            args: args.iter().collect(),
        },
        Expr::DateSetMinutes { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMinutes",
            args: args.iter().collect(),
        },
        Expr::DateSetSeconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setSeconds",
            args: args.iter().collect(),
        },
        Expr::DateSetMilliseconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMilliseconds",
            args: args.iter().collect(),
        },
        Expr::DateSetTime { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setTime",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcFullYear { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCFullYear",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMonth { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMonth",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcDate { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCDate",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcHours { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCHours",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMinutes { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMinutes",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcSeconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCSeconds",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMilliseconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMilliseconds",
            args: args.iter().collect(),
        },
        _ => return None,
    };
    Some(call)
}

/// Branch to `own_label` when `recv` may own a property named `method`, else
/// to `builtin_label`.

/// The builtin-prototype family a folded node's method belongs to (#11915):
/// the receiver kind the fold proved. Every row of [`folded_call`] that is
/// not a Map, Set or Array method is a Date method.
fn folded_family(expr: &Expr) -> Option<crate::builtin_proto::BuiltinProto> {
    use crate::builtin_proto::BuiltinProto;
    folded_call(expr)?;
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
    let Some(call) = folded_call(expr) else {
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
    let family = folded_family(expr).unwrap_or(crate::builtin_proto::BuiltinProto::Object);
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
        emit_own_override_branch(
            ctx,
            method,
            &recv,
            receiver_is_array,
            Some(family),
            &generic_label,
            &builtin_label,
        );

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
        if perry_hir::analysis::pure_numeric_order(comparator).is_some() {
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
    emit_own_override_branch(
        ctx,
        call.method,
        &recv,
        receiver_is_array,
        Some(crate::builtin_proto::BuiltinProto::Array),
        &failed_label,
        &held_label,
    );
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
    use super::{folded_call, Receiver};
    use perry_hir::Expr;

    fn lit(n: f64) -> Expr {
        Expr::Number(n)
    }

    /// Every entry answers the three things the diamond needs, and the method
    /// name is the one a user would have to write to shadow the call.
    #[test]
    fn the_table_answers_receiver_method_and_arguments() {
        let map_get = Expr::MapGet {
            map: Box::new(Expr::LocalGet(1)),
            key: Box::new(lit(1.0)),
        };
        let call = folded_call(&map_get).expect("MapGet is a folded `get`");
        assert_eq!(call.method, "get");
        assert_eq!(call.args.len(), 1);
        assert!(matches!(call.receiver, Receiver::Expr(_)));

        let map_set = Expr::MapSet {
            map: Box::new(Expr::LocalGet(1)),
            key: Box::new(lit(1.0)),
            value: Box::new(lit(2.0)),
        };
        let call = folded_call(&map_set).expect("MapSet is a folded `set`");
        assert_eq!((call.method, call.args.len()), ("set", 2));
    }

    /// A fold that captured the receiver as a LOCAL needs no materialisation:
    /// re-reading a local is free and cannot be observed. `ArrayIndexOf`
    /// carries its receiver as an expression, so the local case is pinned on
    /// the node that still has one.
    #[test]
    fn a_local_receiver_is_not_materialized() {
        let indexof = Expr::ArrayIndexOf {
            array: Box::new(Expr::LocalGet(7)),
            value: Box::new(lit(1.0)),
            from_index: None,
        };
        let call = folded_call(&indexof).expect("a folded indexOf");
        assert_eq!(call.method, "indexOf");
        assert!(matches!(call.receiver, Receiver::Expr(Expr::LocalGet(7))));
    }

    /// `push` is not guarded HERE. The table must not carry it, or the diamond
    /// comes back and with it the +94 instructions per call that made this one
    /// node the exception: guarding `push` costs it the INLINE STORE. An own
    /// `push` is honoured by the node's own slow arms instead (#11021), whose
    /// admission is the header bit the inline tier already tests.
    #[test]
    fn array_push_is_not_a_folded_guard_target() {
        let push = Expr::ArrayPush {
            array_id: 7,
            value: Box::new(lit(1.0)),
            field_writeback: None,
        };
        assert!(
            folded_call(&push).is_none(),
            "ArrayPush must not be guarded: a diamond costs it the inline store, \
             and its slow arms already honour an own `push` (#11021)"
        );
    }

    /// A zero-argument `a.push()` is a native call, not an `ArrayPush`, and is
    /// guarded like the other call-only folds (#11021). The single-argument
    /// `push_single` spelling is left alone: it is not this node's shape.
    #[test]
    fn a_zero_argument_push_is_a_folded_guard_target() {
        let zero = Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::LocalGet(7))),
            method: "push".to_string(),
            args: Vec::new(),
        };
        let call = folded_call(&zero).expect("a zero-argument push is guarded");
        assert_eq!((call.method, call.args.len()), ("push", 0));
        assert!(matches!(call.receiver, Receiver::Expr(Expr::LocalGet(7))));
        let single = Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::LocalGet(7))),
            method: "push_single".to_string(),
            args: vec![lit(1.0)],
        };
        assert!(folded_call(&single).is_none());
    }

    /// An optional argument is part of the call when present and absent when
    /// not — the dispatcher arm must pass exactly what the source passed.
    #[test]
    fn optional_arguments_follow_the_source() {
        let one = Expr::ArraySlice {
            layout: None,
            array: Box::new(Expr::LocalGet(1)),
            start: Box::new(lit(0.0)),
            end: None,
        };
        assert_eq!(folded_call(&one).unwrap().args.len(), 1);
        let two = Expr::ArraySlice {
            layout: None,
            array: Box::new(Expr::LocalGet(1)),
            start: Box::new(lit(0.0)),
            end: Some(Box::new(lit(2.0))),
        };
        assert_eq!(folded_call(&two).unwrap().args.len(), 2);
    }

    /// The zero-argument Date folds missing from the table until #11493, each
    /// under the name a user would write to shadow it.
    #[test]
    fn the_remaining_date_folds_are_guarded_under_their_own_names() {
        let date = || Box::new(Expr::LocalGet(1));
        let cases = [
            (Expr::DateGetTimezoneOffset(date()), "getTimezoneOffset"),
            (Expr::DateToJSON(date()), "toJSON"),
            (Expr::DateToDateString(date()), "toDateString"),
            (Expr::DateToTimeString(date()), "toTimeString"),
            (Expr::DateToLocaleDateString(date()), "toLocaleDateString"),
            (Expr::DateToLocaleTimeString(date()), "toLocaleTimeString"),
            (Expr::DateToLocaleString(date()), "toLocaleString"),
        ];
        for (node, name) in cases {
            let call = folded_call(&node).unwrap_or_else(|| panic!("{name} is guarded"));
            assert_eq!((call.method, call.args.len()), (name, 0));
        }
    }

    /// `DateToUTCString` is both `toUTCString` and `toGMTString`, so the node
    /// cannot name the property to test. Guarding it under either name would
    /// dispatch the other spelling to the wrong own method.
    #[test]
    fn the_shared_utc_gmt_fold_is_not_guarded() {
        let node = Expr::DateToUTCString(Box::new(Expr::LocalGet(1)));
        assert!(folded_call(&node).is_none());
    }

    /// A node that is not a folded builtin method call is left alone: the
    /// guard must not wrap arbitrary expressions in a diamond.
    #[test]
    fn a_non_call_node_is_not_guarded() {
        assert!(folded_call(&lit(1.0)).is_none());
        assert!(folded_call(&Expr::LocalGet(1)).is_none());
        assert!(folded_call(&Expr::MapSize(Box::new(Expr::LocalGet(1)))).is_none());
    }
}
