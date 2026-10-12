//! Argument coercions a string-method arm performs BEFORE it consumes the
//! receiver.
//!
//! The receiver of a string method is evaluated first and then held — boxed in
//! a register, or unboxed to a raw `StringHeader` address — until the runtime
//! call that consumes it. Several arms coerce their arguments inside that
//! window: `ToNumber` for an index or length (`charAt`, `slice`, `padStart`'s
//! `maxLength`, …), `ToString` for a search value or fill (`indexOf`,
//! `includes`, `anchor`, `padStart`'s fill, …). On an object argument either
//! one runs a user `valueOf` / `toString`, and `ToString` of a number
//! allocates; both can collect, and an evacuating minor moves a young receiver
//! out from under the register.
//!
//! The rooting decision used to look only at whether LOWERING an argument can
//! collect (`operand_may_collect`). `s.padStart(lenObj, "0")` with `lenObj` a
//! plain local loads inertly, so the receiver was left unrooted while
//! `js_number_coerce` ran `lenObj.valueOf()`; `fresh().charAt(idxObj)`
//! additionally unboxed the receiver BEFORE `js_string_index_to_i32` ran the
//! `valueOf`, so even a rooted receiver was used through a pre-collection
//! address. Both shapes faulted under the protected from-space harness.
//!
//! The rule this module carries, for every arm that coerces:
//!
//! 1. A coercion is part of the receiver's window: [`dispatch_coercions_may_collect`]
//!    feeds the receiver's rooting decision, and every argument stays rooted
//!    across the coercions ([`lower_coerced_args`]).
//! 2. Coercions run in argument (= spec) order, each on a value re-read from
//!    its root, BEFORE the receiver is re-read and unboxed. A `ToNumber`
//!    result is a plain number; a `ToString` result is a fresh string handle,
//!    rooted across any later coercion ([`pre_coerce_string`]).
//! 3. What the consuming helper receives is then inert: a number (or
//!    `undefined`, kept so each helper's own default — `len` for an end index,
//!    "rest" for a `substr` length — still applies) or a string handle.
//!
//! [`dispatch_arg_coercion`] is the one statement of which argument each arm
//! coerces and how; the arms emit through the helpers below, which ask the
//! same leaf predicate ([`coercion_may_collect`]) whether to emit the call at
//! all, so "the receiver is rooted" and "a coercion is emitted" cannot drift
//! apart.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{unbox_str_handle, FnCtx};
use crate::rooting::{any_operand_may_collect, open_rooted_group, EmittedValue, Repr, RootedGroup};
use crate::type_analysis::{is_numeric_expr, is_string_expr};
use crate::types::{DOUBLE, I1, I32, I64};

/// How a dispatch arm coerces one argument before consuming the receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArgCoercion {
    /// `ToNumber` / `ToIntegerOrInfinity` / `ToLength`.
    Number,
    /// `ToString`.
    String,
    /// `localeCompare`'s `(locales, options)`: `CanonicalizeLocaleList` and
    /// the collator `GetOption` reads, i.e. getters and element reads.
    Options,
}

/// The coercion `lower_string_method_dispatch` applies to `args[i]` of
/// `property` before it consumes the receiver, or `None`.
pub(super) fn dispatch_arg_coercion(property: &str, i: usize) -> Option<ArgCoercion> {
    use ArgCoercion::{Number, Options, String};
    match (property, i) {
        ("indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith", 0) => Some(String),
        ("indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith", 1) => Some(Number),
        ("slice" | "substring" | "substr", 0 | 1) => Some(Number),
        ("charAt" | "at" | "codePointAt" | "charCodeAt" | "repeat", 0) => Some(Number),
        ("anchor" | "link" | "fontcolor" | "fontsize", 0) => Some(String),
        ("padStart" | "padEnd", 0) => Some(Number),
        ("padStart" | "padEnd", 1) => Some(String),
        ("localeCompare", 0) => Some(String),
        ("localeCompare", 1 | 2) => Some(Options),
        _ => None,
    }
}

/// Whether applying `coercion` to `arg` can collect.
///
/// The same predicate the emitting helpers use to decide whether to emit the
/// coercion call at all: a number needs no `ToNumber`, a string no `ToString`
/// (the arms already skip `js_string_coerce` for an `is_string_expr` operand).
pub(super) fn coercion_may_collect(ctx: &FnCtx<'_>, coercion: ArgCoercion, arg: &Expr) -> bool {
    match coercion {
        ArgCoercion::Number => !is_numeric_expr(ctx, arg),
        ArgCoercion::String => !is_string_expr(ctx, arg),
        ArgCoercion::Options => !matches!(arg, Expr::Undefined) && !is_string_expr(ctx, arg),
    }
}

/// Whether any coercion the dispatch applies to `args` can collect — part of
/// the receiver's window, so part of its rooting decision.
pub(super) fn dispatch_coercions_may_collect(
    ctx: &FnCtx<'_>,
    property: &str,
    args: &[Expr],
) -> bool {
    args.iter().enumerate().any(|(i, arg)| {
        dispatch_arg_coercion(property, i).is_some_and(|c| coercion_may_collect(ctx, c, arg))
    })
}

/// Lower the arguments of a coercing arm. Argument `i` stays rooted across the
/// arguments after it AND across every coercion, because a coercion of an
/// earlier argument runs while it is still waiting to be read.
pub(super) fn lower_coerced_args<'a>(
    ctx: &mut FnCtx<'_>,
    property: &str,
    args: &'a [Expr],
) -> Result<RootedGroup<'a>> {
    let coercions_collect = dispatch_coercions_may_collect(ctx, property, args);
    let mut group = open_rooted_group(args.len() + 2);
    for (i, arg) in args.iter().enumerate() {
        let collects = coercions_collect || any_operand_may_collect(ctx, args[i + 1..].iter());
        group.lower(ctx, arg, collects)?;
    }
    Ok(group)
}

/// Whether a coercion of an argument AFTER `i` can collect: the protection a
/// `ToString` result produced for argument `i` needs.
pub(super) fn later_coercion_may_collect(
    ctx: &FnCtx<'_>,
    property: &str,
    args: &[Expr],
    i: usize,
) -> bool {
    args.iter().enumerate().skip(i + 1).any(|(j, arg)| {
        dispatch_arg_coercion(property, j).is_some_and(|c| coercion_may_collect(ctx, c, arg))
    })
}

/// `ToNumber(args[i])`, hoisted above the receiver read. `None` when the
/// argument is absent.
///
/// The result is a plain number, or `undefined` when the argument was
/// `undefined` (`ToNumber` would turn it into NaN and erase the "omitted"
/// meaning several helpers give it). Every index helper downstream reads a
/// number without running anything; an argument that is already a number is
/// passed through with no call.
pub(super) fn pre_coerce_number(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    args: &[Expr],
    i: usize,
) -> Result<Option<String>> {
    let Some(arg) = args.get(i) else {
        return Ok(None);
    };
    let value = group.reread(ctx, i)?;
    if !coercion_may_collect(ctx, ArgCoercion::Number, arg) {
        return Ok(Some(value));
    }
    // A plain double (top 16 bits, sign stripped, below the NaN-box tag band)
    // is already a Number — the test `js_number_coerce` itself opens with — so
    // the call, and its safepoint, run only for a tagged value. The slow arm
    // takes the `undefined` test BEFORE the call and selects the constant, not
    // `value`: the argument register is not read again once the coercion
    // (which can move it) has run. Neither arm yields a pointer.
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(&value);
    let top = blk.and(I64, &bits, PLAIN_DOUBLE_TOP_MASK);
    let is_plain = blk.icmp_ult(I64, &top, NANBOX_TAG_BAND_START);
    let entry_label = ctx.block().label.clone();
    let slow_idx = ctx.new_block("strm.tonum.slow");
    let merge_idx = ctx.new_block("strm.tonum.merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_plain, &merge_label, &slow_label);

    ctx.current_block = slow_idx;
    let blk = ctx.block();
    let is_undefined = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
    let coerced = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &value)]);
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let slow_val = blk.select(I1, &is_undefined, DOUBLE, &undefined, &coerced);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&value, &entry_label), (&slow_val, &slow_pred)],
    )))
}

/// `0x7FFF_0000_0000_0000`: a double's top 16 bits with the sign stripped.
const PLAIN_DOUBLE_TOP_MASK: &str = "9223090561878065152";
/// `0x7FF9_0000_0000_0000`: the first NaN-box tag; below it is a plain double
/// (finite, infinite or a canonical NaN). Same band as `js_number_coerce`.
const NANBOX_TAG_BAND_START: &str = "9221401712017801216";

/// Which runtime `ToString` an arm applies.
#[derive(Clone, Copy)]
pub(super) enum StringCoercion<'s> {
    /// `js_string_coerce`.
    Plain,
    /// `js_string_search_value_to_string` (rejects a RegExp first), with the
    /// method id it reports in the TypeError.
    Search(&'s str),
    /// `js_string_pad_fill` (`undefined` -> null, i.e. the default " ").
    PadFill,
}

/// `ToString(args[i])` (or of `undefined` when absent), hoisted above the
/// receiver read, as a raw string handle. Rooted when `protect` (a later
/// coercion can collect); read it back with `group.reread_emitted` after the
/// receiver has been unboxed.
pub(super) fn pre_coerce_string(
    ctx: &mut FnCtx<'_>,
    group: &mut RootedGroup<'_>,
    args: &[Expr],
    i: usize,
    how: StringCoercion<'_>,
    protect: bool,
) -> Result<EmittedValue> {
    let value = match args.get(i) {
        Some(_) => group.reread(ctx, i)?,
        None => ctx
            .block()
            .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64),
    };
    let is_str = args.get(i).is_some_and(|a| is_string_expr(ctx, a));
    let blk = ctx.block();
    let handle = match how {
        StringCoercion::Search(method_id) => blk.call(
            I64,
            "js_string_search_value_to_string",
            &[(DOUBLE, &value), (I32, method_id)],
        ),
        _ if is_str => unbox_str_handle(blk, &value),
        StringCoercion::Plain => blk.call(I64, "js_string_coerce", &[(DOUBLE, &value)]),
        StringCoercion::PadFill => blk.call(I64, "js_string_pad_fill", &[(DOUBLE, &value)]),
    };
    Ok(group.adopt_emitted(ctx, Repr::Ptr, &handle, protect))
}
