//! String method and concatenation lowering.
//!
//! Contains `lower_string_method`, `lower_string_self_append`,
//! `lower_string_coerce_concat`, and `lower_string_concat`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{i32_bool_to_nanbox, lower_expr, nanbox_string_inline, unbox_str_handle, FnCtx};
use crate::lower_string_concat::{
    emit_string_concat_chain, str_operand_handle_tag_dispatched, CONCAT_CHAIN_MAX_PARTS,
};
use crate::rooting::{
    open_rooted_group, operand_may_collect, with_rooted_accumulator, with_rooted_group, Arg,
    EmittedValue, Repr, RootedGroup,
};
use crate::type_analysis::string_value_is_runtime_guaranteed;

mod char_code_at;
mod coerce;
use crate::types::{DOUBLE, I1, I32, I64, PTR};
use char_code_at::lower_char_code_at_inline;
use coerce::{
    dispatch_coercions_may_collect, later_coercion_may_collect, lower_coerced_args,
    pre_coerce_index, pre_coerce_number, pre_coerce_string, StringCoercion,
};

fn regexp_search_method_id(property: &str) -> String {
    match property {
        "startsWith" => "1".to_string(),
        "endsWith" => "2".to_string(),
        _ => "0".to_string(),
    }
}

pub(crate) fn is_known_string_method_name(name: &str) -> bool {
    matches!(
        name,
        "anchor"
            | "big"
            | "blink"
            | "bold"
            | "fixed"
            | "fontcolor"
            | "fontsize"
            | "italics"
            | "link"
            | "small"
            | "strike"
            | "sub"
            | "sup"
            | "at"
            | "charAt"
            | "charCodeAt"
            | "codePointAt"
            | "concat"
            | "endsWith"
            | "includes"
            | "indexOf"
            | "isWellFormed"
            | "lastIndexOf"
            | "localeCompare"
            | "match"
            | "matchAll"
            | "normalize"
            | "padEnd"
            | "padStart"
            | "repeat"
            | "replace"
            | "replaceAll"
            | "search"
            | "slice"
            | "split"
            | "startsWith"
            | "substr"
            | "substring"
            | "toLocaleLowerCase"
            | "toLocaleUpperCase"
            | "toLowerCase"
            | "toString"
            | "toUpperCase"
            | "toWellFormed"
            | "trim"
            | "trimEnd"
            | "trimStart"
    )
}

/// Lower `s.method(args…)` for a string-typed receiver. Currently
/// supported methods: `indexOf` (1 or 2 args), `slice`, `substring`,
/// `startsWith`, `endsWith`. Anything else bails with an actionable
/// error.
///
/// All string methods unbox the receiver pointer with the inline
/// bitcast+mask pattern, lower each arg, and call the matching runtime
/// function. Return values that are i32 (indexOf, startsWith, endsWith)
/// get sitofp'd to double; return values that are i64 string handles
/// (slice, substring) get NaN-boxed inline with STRING_TAG.
pub(crate) fn lower_string_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<String> {
    // A scalar-replaced uppercase local is only admitted when all reads are
    // covered by fused operations. Preserve the source value in its own slot
    // and perform `toUpperCase().indexOf("literal")` without allocating the
    // intermediate JS string.
    if property == "indexOf"
        && matches!(args, [Expr::String(_)])
        && matches!(object, Expr::LocalGet(_))
    {
        let Expr::LocalGet(id) = object else {
            unreachable!();
        };
        if let Some(source_slot) = ctx.scalar_replaced_uppercase_sources.get(id).cloned() {
            let source_box = ctx.block().load(DOUBLE, &source_slot);
            let needle_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let source_handle = unbox_str_handle(blk, &source_box);
            let needle_handle = unbox_str_handle(blk, &needle_box);
            let result = blk.call(
                I32,
                "js_string_to_upper_case_index_of",
                &[(I64, &source_handle), (I64, &needle_handle)],
            );
            return Ok(blk.sitofp(I32, &result, DOUBLE));
        }
    }

    let recv_box = lower_expr(ctx, object)?;
    let proof = string_proof_for(ctx, property, args);
    lower_string_method_from_box(ctx, object, property, args, recv_box, proof)
}

/// Where a string-method site proves `Pristine(String)` (#11915).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringProof {
    /// The callee proves it: the site calls the checked entry
    /// `js_string_m_<method><argc>` (or, for `charCodeAt`, folds the byte into
    /// its inline body's admission with that entry as the slow arm).
    Entry,
    /// The method has an entry, but an argument's evaluation could observe
    /// WHEN the method is read (ECMA-262 reads it before them): the site
    /// reads the byte before its arguments, evaluates them once, and calls
    /// the unchecked body `js_string_b_<method><argc>` or the generic path.
    EntryPreRead,
    /// No entry for the method and arity: the site tests the byte before its
    /// arguments and has a generic arm.
    Guard,
    /// The caller's branch already tested it.
    Proven,
}

/// How a call with these arguments proves `Pristine(String)`: through the
/// entry (`Entry` / `EntryPreRead`) when the method has one for this arity,
/// else at the site (`Guard`).
pub(crate) fn string_proof_for(ctx: &FnCtx<'_>, property: &str, args: &[Expr]) -> StringProof {
    if crate::builtin_proto::string_entry(property, args.len()).is_none() {
        StringProof::Guard
    } else if crate::expr::method_site::args_may_observe_lookup(ctx, args) {
        StringProof::EntryPreRead
    } else {
        StringProof::Entry
    }
}

/// Lower a string method after a runtime tag guard has proved that the
/// already-evaluated receiver is a heap or short string.
///
/// Keeping the receiver as an SSA value is load-bearing: the non-string arm of
/// the guard must dispatch the exact same value, rather than evaluate a
/// side-effecting receiver expression a second time.
pub(crate) fn lower_string_method_from_proven_box(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    recv_box: String,
    proof: StringProof,
) -> Result<String> {
    // `Proven`: the caller's tag guard already folded `Pristine(String)` into
    // its test (#11915), so its generic arm is this call's generic path.
    // `Entry`: the checked entry proves it.
    lower_string_method_from_box(ctx, object, property, args, recv_box, proof)
}

fn lower_string_method_from_box(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    recv_box: String,
    proof: StringProof,
) -> Result<String> {
    // #6971: the receiver is lowered BEFORE the arguments, and an argument's
    // lowering can collect — `fresh(k).concat("|" + churn(N))` dropped the
    // receiver, whose unboxed form is a BARE string address that only the
    // `gc::root_words` bare form covers. Root it across the whole dispatch;
    // the truncate below is the single release point for every one of the
    // match's ~60 return paths.
    let args_can_collect = if property == "concat" {
        // concat immediately ToString-coerces every argument. A plain object
        // local is inert to load but can run arbitrary user code while being
        // coerced, so that coercion must be part of the receiver's window.
        args.iter().any(|a| concat_arg_may_collect(ctx, a))
    } else {
        // A coercion the dispatch itself applies to an argument (`ToNumber`
        // of an index, `ToString` of a search value or fill) runs while the
        // receiver is held, exactly like an argument's own lowering: see
        // `coerce.rs`.
        args.iter().any(|a| operand_may_collect(ctx, a))
            || dispatch_coercions_may_collect(ctx, property, args)
    };
    // #7615 slice 8: the ESCAPING group form. The release has to post-dominate
    // ~60 return paths inside the dispatch, which no closure form can own
    // without swallowing the whole 1,100-line match — the same argument
    // `open_rooted_group`'s doc makes for `func_ref.rs`'s dispatch diamonds.
    // What the group buys over the raw slot index it replaces is that a caller
    // holding it cannot truncate at the wrong slot, in the wrong order, or
    // twice: it never sees an index.
    let mut group = open_rooted_group(1);
    let recv = group.adopt_emitted(ctx, Repr::Boxed, &recv_box, args_can_collect);
    let result = match proof {
        StringProof::Entry | StringProof::EntryPreRead => {
            let patched_before = (proof == StringProof::EntryPreRead).then(|| {
                crate::builtin_proto::emit_patched_test(
                    ctx.block(),
                    crate::builtin_proto::BuiltinProto::String,
                )
            });
            let (arg_values, arg_group) = crate::lower_call::lower_call_args_rooted(ctx, args)?;
            let recv_box = reread_recv(ctx, &group, recv);
            let result = emit_string_entry_call(
                ctx,
                object,
                property,
                &recv_box,
                &arg_values,
                patched_before.as_deref(),
                None,
            );
            arg_group.release(ctx);
            result
        }
        StringProof::Guard => {
            lower_string_method_pristine(ctx, object, property, args, &group, recv)
        }
        StringProof::Proven => {
            lower_string_method_dispatch(ctx, object, property, args, &group, recv)
        }
    };
    // Released only after the dispatch's consuming runtime call has run: that
    // call allocates while it reads the receiver.
    //
    // The `is_terminated` guard is load-bearing, not defensive. The
    // unknown-property arm of the dispatch throws and emits `unreachable`, then
    // still returns `Ok(placeholder)` so callers have a register to phi against
    // — so control reaches here with the block already terminated. Appending the
    // truncate there would emit an instruction after the terminator: invalid IR,
    // reachable from `("a" + churn()).nope(obj)` (an unrecognized string method
    // whose arguments can collect, which is what sets `recv_root` at all).
    // Skipping the release is sound: `unreachable` means no path resumes, and
    // the temp-root stack is cut by the enclosing scope regardless.
    //
    // NOT covered by a regression test, deliberately: an attempted HIR-level
    // reproducer (`("a" + "b").nope({})`) never reached the throwing arm — the
    // emitted IR contained no `unreachable` at all — so the test passed with
    // and without this guard. A test that is green either way is worse than no
    // test, so it was removed rather than shipped. The guard is kept as
    // defense-in-depth: emitting after a terminator is never correct, and the
    // check is free. Reachability of the arm from a TypeScript source remains
    // unproven; see the PR body.
    //
    // Not releasing on the terminated path is the SAFE half of a mis-managed
    // guard (`open_rooted_group`'s doc): over-retention, not a dangling
    // pointer. `release` on an empty group — the `args_can_collect == false`
    // case — emits nothing, so the guard alone decides.
    if !ctx.block().is_terminated() {
        group.release(ctx);
    }
    result
}

/// #11915: a string receiver owns nothing, so the method it reads is
/// `%String.prototype%`'s (or `%Object.prototype%`'s) while those are
/// unmodified — `Pristine(String)`, one load and test before the arguments.
/// A modified prototype takes the shared generic path with the same receiver
/// and arguments; the direct dispatch below is emitted once, unchanged.
fn lower_string_method_pristine(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    group: &RootedGroup<'_>,
    recv: EmittedValue,
) -> Result<String> {
    use crate::builtin_proto::{emit_patched_test, BuiltinProto};
    let direct_idx = ctx.new_block("strpristine.direct");
    let generic_idx = ctx.new_block("strpristine.generic");
    let merge_idx = ctx.new_block("strpristine.merge");
    let direct_label = ctx.block_label(direct_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    {
        let blk = ctx.block();
        let patched = emit_patched_test(blk, BuiltinProto::String);
        blk.cond_br(&patched, &generic_label, &direct_label);
    }

    ctx.current_block = generic_idx;
    let (arg_vals, arg_group) = crate::lower_call::lower_call_args_rooted(ctx, args)?;
    let generic_recv = reread_recv(ctx, group, recv);
    let generic_value = crate::builtin_proto::emit_generic_call(
        ctx,
        crate::builtin_proto::GenericRead::Receiver,
        &generic_recv,
        property,
        &arg_vals,
    );
    arg_group.release(ctx);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = direct_idx;
    crate::builtin_proto::emit_contract_check(ctx.block(), &[BuiltinProto::String]);
    let direct_value = lower_string_method_dispatch(ctx, object, property, args, group, recv)?;
    let mut incoming = vec![(generic_value, generic_end)];
    if !ctx.block().is_terminated() {
        let direct_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        incoming.push((direct_value, direct_end));
    }
    ctx.current_block = merge_idx;
    let pairs: Vec<(&str, &str)> = incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    Ok(ctx.block().phi(DOUBLE, &pairs))
}

/// The string method through its entries (#11915), on operands the caller
/// has evaluated (each rooted across the next) and re-read.
///
/// * `patched_before == None` (the arguments cannot observe the method
///   read): ONE call to the checked entry `js_string_m_<method><argc>`, which
///   proves `Pristine(String)` itself.
/// * `Some(p)`: `p` is the byte the site read BEFORE the arguments; set, it
///   takes the generic path — `generic_label` when the caller has one (the
///   tag guard's dispatcher arm, which reads the same rooted operands), else
///   one `js_builtin_proto_generic_call*`; clear, the unchecked body
///   `js_string_b_<method><argc>`.
///
/// `charCodeAt` keeps its inline body: the proof joins the index test the body
/// already has, and the entry is its slow arm.
pub(crate) fn emit_string_entry_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    recv_box: &str,
    arg_values: &[String],
    patched_before: Option<&str>,
    generic_label: Option<&str>,
) -> Result<String> {
    let checked = crate::builtin_proto::string_entry(property, arg_values.len())
        .expect("an entry proof is chosen only for a method with a checked entry");
    let body = checked.replacen("js_string_m_", "js_string_b_", 1);
    let callee = if patched_before.is_some() {
        &body
    } else {
        &checked
    };
    let call = |ctx: &mut FnCtx<'_>| -> String {
        if property == "charCodeAt" {
            let idx_d = arg_values
                .first()
                .cloned()
                .unwrap_or_else(|| crate::nanbox::double_literal(0.0));
            if let Some(value) = char_code_at::lower_char_code_at_inline_entry(
                ctx,
                object,
                recv_box,
                &idx_d,
                callee,
                arg_values,
                patched_before.is_none(),
            ) {
                return value;
            }
        }
        let mut operands: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, recv_box)];
        operands.extend(arg_values.iter().map(|a| (DOUBLE, a.as_str())));
        ctx.block().call(DOUBLE, callee, &operands)
    };
    let Some(patched) = patched_before else {
        return Ok(call(ctx));
    };
    let direct_idx = ctx.new_block("strentry.direct");
    let direct_label = ctx.block_label(direct_idx);
    match generic_label {
        Some(generic_label) => {
            ctx.block().cond_br(patched, generic_label, &direct_label);
            ctx.current_block = direct_idx;
            Ok(call(ctx))
        }
        None => {
            let generic_idx = ctx.new_block("strentry.generic");
            let merge_idx = ctx.new_block("strentry.merge");
            let generic_label = ctx.block_label(generic_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(patched, &generic_label, &direct_label);
            ctx.current_block = generic_idx;
            let generic_value = crate::builtin_proto::emit_generic_call(
                ctx,
                crate::builtin_proto::GenericRead::Receiver,
                recv_box,
                property,
                arg_values,
            );
            let generic_end = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            ctx.current_block = direct_idx;
            let direct_value = call(ctx);
            let direct_end = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            ctx.current_block = merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[
                    (generic_value.as_str(), generic_end.as_str()),
                    (direct_value.as_str(), direct_end.as_str()),
                ],
            ))
        }
    }
}

/// Re-read the string-method receiver out of its temp root (#6971).
///
/// Mandatory rather than defensive: the slot is a *mutable* root, so an
/// evacuating cycle rewrites it and the register pushed beforehand is stale.
/// Returns the original register when nothing was rooted, emitting no IR — so a
/// method whose arguments cannot collect keeps its previous code byte for byte.
fn reread_recv(ctx: &mut FnCtx<'_>, group: &RootedGroup<'_>, recv: EmittedValue) -> String {
    group.reread_emitted(ctx, recv)
}

/// Whether lowering and immediately `ToString`-coercing one concat argument
/// can collect. A plain object local does not collect when loaded, but its
/// coercion can call user `toString`, so `operand_may_collect` alone is not
/// enough for the receiver/previous-part rooting window.
fn concat_arg_may_collect(ctx: &FnCtx<'_>, arg: &Expr) -> bool {
    operand_may_collect(ctx, arg) || !string_value_is_runtime_guaranteed(ctx, arg)
}

/// Perform one `String.prototype.concat` argument's spec-mandated `ToString`
/// after call-argument evaluation has finished. Proven string values stay
/// boxed so the chain runtime can consume SSO directly; every other value is
/// coerced now and returned as a boxed heap string.
fn coerce_concat_arg_to_box(ctx: &mut FnCtx<'_>, arg: &Expr, raw_box: &str) -> String {
    if string_value_is_runtime_guaranteed(ctx, arg) {
        return raw_box.to_string();
    }

    let handle = ctx
        .block()
        .call(I64, "js_string_coerce", &[(DOUBLE, raw_box)]);
    nanbox_string_inline(ctx.block(), &handle)
}

#[allow(clippy::too_many_lines)]
fn lower_string_method_dispatch(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    group: &RootedGroup<'_>,
    recv: EmittedValue,
) -> Result<String> {
    match property {
        "indexOf" => {
            // No `searchString` → `undefined`, which `js_string_coerce`
            // stringifies to "undefined" (`"".indexOf()` === -1). An object
            // `searchString` is `ToString`-coerced (running its user
            // `toString`/`valueOf`) BEFORE `ToNumber(position)`, per ECMA-262
            // §22.1.3.8, and both run before the receiver is read (`coerce.rs`).
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let needle = pre_coerce_string(
                ctx,
                &mut arg_group,
                args,
                0,
                StringCoercion::Plain,
                later_coercion_may_collect(ctx, property, args, 0),
            )?;
            let from_i32 = pre_coerce_index(ctx, &arg_group, args, 1, false)?;
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let needle_handle = arg_group.reread_emitted(ctx, needle);
            let blk = ctx.block();
            let result_i32 = if let Some(from_i32) = from_i32 {
                // `ToIntegerOrInfinity(position)` via the runtime helper
                // (handles ±Infinity/NaN), NOT a raw `fptosi`.
                blk.call(
                    I32,
                    "js_string_index_of_from",
                    &[(I64, &recv_handle), (I64, &needle_handle), (I32, &from_i32)],
                )
            } else {
                blk.call(
                    I32,
                    "js_string_index_of",
                    &[(I64, &recv_handle), (I64, &needle_handle)],
                )
            };
            // i32 → double via sitofp (preserves the -1 sentinel for "not found").
            let result = blk.sitofp(I32, &result_i32, DOUBLE);
            arg_group.release(ctx);
            Ok(result)
        }
        "slice" | "substring" => {
            // Issue #316: 0-arg form is the spec'd "clone" idiom —
            // `s.slice()` ≡ `s.slice(0, length)`. Was rejected at
            // codegen with "expects 1 or 2 args, got 0" before this fix.
            // `ToNumber` of both indices runs before the receiver is read
            // (`coerce.rs`); an `undefined` end survives as `undefined`.
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let start_i32 = pre_coerce_index(ctx, &arg_group, args, 0, false)?
                .unwrap_or_else(|| "0".to_string());
            let end_i32 = pre_coerce_index(ctx, &arg_group, args, 1, true)?
                .unwrap_or_else(|| "2147483647".to_string());
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let runtime_fn = if property == "slice" {
                "js_string_slice"
            } else {
                "js_string_substring"
            };
            let result_handle = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (I32, &start_i32), (I32, &end_i32)],
            );
            let result = nanbox_string_inline(blk, &result_handle);
            arg_group.release(ctx);
            Ok(result)
        }
        "split" => {
            let mut arguments = open_rooted_group(2);
            let separator = args
                .first()
                .map(|arg| arguments.lower(ctx, arg, true))
                .transpose()?;
            let limit = args
                .get(1)
                .map(|arg| arguments.lower(ctx, arg, true))
                .transpose()?;
            for extra in args.iter().skip(2) {
                let _ = lower_expr(ctx, extra)?;
            }
            let undefined =
                || crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let separator = match separator {
                Some(i) => arguments.reread(ctx, i)?,
                None => undefined(),
            };
            let limit = match limit {
                Some(i) => arguments.reread(ctx, i)?,
                None => undefined(),
            };
            let receiver = reread_recv(ctx, group, recv);
            let name = "js_string_split_js";
            let result = ctx.block().call(
                DOUBLE,
                name,
                &[(DOUBLE, &receiver), (DOUBLE, &separator), (DOUBLE, &limit)],
            );
            arguments.release(ctx);
            Ok(result)
        }
        // toLocaleLowerCase / toLocaleUpperCase — honor the `locales` arg:
        // validate BCP 47 tags (throwing RangeError on a bad tag) and apply
        // Turkic (tr/az) dotted/dotless `I` casing. Other locales fall back to
        // language-neutral Unicode casing. Closes #2781. (#592: Effect's
        // `aliasOrValue` at Cron.ts:846 was the original user-impact site.)
        "toLocaleLowerCase" | "toLocaleUpperCase" => {
            // The `locales` arg is passed as a NaN-boxed JSValue (double) to the
            // runtime, which extracts/validates it. Missing → undefined.
            let (arg_values, arg_group) = crate::lower_call::lower_call_args_rooted(ctx, args)?;
            let locales_box = if args.is_empty() {
                None
            } else {
                Some(arg_values[0].clone())
            };
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let locales_box = match locales_box {
                Some(v) => v,
                None => blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64),
            };
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let runtime_fn = if property == "toLocaleLowerCase" {
                "js_string_to_locale_lower_case"
            } else {
                "js_string_to_locale_upper_case"
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (DOUBLE, &locales_box)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        // Unary string-returning methods (no args).
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" => {
            // These methods take no args; JS ignores any extras but still
            // evaluates them for side effects (ECMA-262 argument evaluation
            // precedes the call). Evaluate then discard.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let runtime_fn = match property {
                "toLowerCase" => "js_string_to_lower_case",
                "toUpperCase" => "js_string_to_upper_case",
                "trim" => "js_string_trim",
                "trimStart" => "js_string_trim_start",
                "trimEnd" => "js_string_trim_end",
                _ => unreachable!(),
            };
            let result = blk.call(I64, runtime_fn, &[(I64, &recv_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        // Annex B §B.2.2 HTML wrappers — no-arg tag wrappers. Extra args are
        // evaluated for side effects (JS ignores them) then discarded.
        "big" | "blink" | "bold" | "fixed" | "italics" | "small" | "strike" | "sub" | "sup" => {
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let runtime_fn = match property {
                "big" => "js_string_big",
                "blink" => "js_string_blink",
                "bold" => "js_string_bold",
                "fixed" => "js_string_fixed",
                "italics" => "js_string_italics",
                "small" => "js_string_small",
                "strike" => "js_string_strike",
                "sub" => "js_string_sub",
                "sup" => "js_string_sup",
                _ => unreachable!(),
            };
            let result = blk.call(I64, runtime_fn, &[(I64, &recv_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        // Annex B §B.2.2 HTML wrappers that take an attribute value. A missing
        // arg coerces `undefined` -> "undefined" via `js_string_coerce`.
        "anchor" | "link" | "fontcolor" | "fontsize" => {
            // `ToString(value)` runs before the receiver is read (`coerce.rs`).
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let value =
                pre_coerce_string(ctx, &mut arg_group, args, 0, StringCoercion::Plain, false)?;
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let value_handle = arg_group.reread_emitted(ctx, value);
            let blk = ctx.block();
            let runtime_fn = match property {
                "anchor" => "js_string_anchor",
                "link" => "js_string_link",
                "fontcolor" => "js_string_fontcolor",
                "fontsize" => "js_string_fontsize",
                _ => unreachable!(),
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (I64, &value_handle)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        "charAt" => {
            // #2787: a missing index defaults to 0; the provided index is
            // coerced with JS `ToIntegerOrInfinity` (undefined/NaN -> 0) rather
            // than a raw `fptosi`, which is UB on a NaN bit pattern.
            // #3987: JS ignores extra args to `charAt` but still evaluates them
            // (left-to-right, for side effects). Use args[0] as the index and
            // lower the rest, discarding their values, instead of bailing.
            // `ToNumber(index)` runs before the receiver is read (`coerce.rs`).
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let idx_i32 = pre_coerce_index(ctx, &arg_group, args, 0, false)?
                .unwrap_or_else(|| "0".to_string());
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_char_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        "repeat" => {
            if args.is_empty() {
                bail!(
                    "perry-codegen: String.repeat expects at least 1 arg, got {}",
                    args.len()
                );
            }
            // `ToNumber(count)` runs before the receiver is unboxed
            // (`coerce.rs`): `js_string_repeat` takes the raw receiver.
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let count_d = pre_coerce_number(ctx, &arg_group, args, 0)?
                .expect("repeat has at least one argument");
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_repeat",
                &[(I64, &recv_handle), (DOUBLE, &count_d)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        "replace" | "replaceAll" => {
            let mut arguments = open_rooted_group(2);
            let search = args
                .first()
                .map(|arg| arguments.lower(ctx, arg, true))
                .transpose()?;
            let replacement = args
                .get(1)
                .map(|arg| arguments.lower(ctx, arg, true))
                .transpose()?;
            for extra in args.iter().skip(2) {
                let _ = lower_expr(ctx, extra)?;
            }
            let undefined =
                || crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let search = match search {
                Some(i) => arguments.reread(ctx, i)?,
                None => undefined(),
            };
            let replacement = match replacement {
                Some(i) => arguments.reread(ctx, i)?,
                None => undefined(),
            };
            let receiver = reread_recv(ctx, group, recv);
            let name = if property == "replaceAll" {
                "js_string_replace_all_js"
            } else {
                "js_string_replace_js"
            };
            let result = ctx.block().call(
                DOUBLE,
                name,
                &[
                    (DOUBLE, &receiver),
                    (DOUBLE, &search),
                    (DOUBLE, &replacement),
                ],
            );
            arguments.release(ctx);
            Ok(result)
        }
        // str.at(i) / str.charCodeAt(i) / str.codePointAt(i)
        "at" => {
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // `js_string_at` already resolves negative indices relative to len.
            // #3987: ignore extra args (still evaluate for side effects).
            // `ToNumber(index)` runs before the receiver is read (`coerce.rs`).
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let idx_i32 = pre_coerce_index(ctx, &arg_group, args, 0, false)?
                .unwrap_or_else(|| "0".to_string());
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = str_operand_handle_tag_dispatched(ctx, object, &recv_box);
            let blk = ctx.block();
            // js_string_at returns a NaN-boxed string or undefined directly.
            let result = blk.call(
                DOUBLE,
                "js_string_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "codePointAt" => {
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // #3987: ignore extra args (still evaluate for side effects).
            // `ToNumber(index)` runs before the receiver is read (`coerce.rs`).
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let idx_i32 = pre_coerce_index(ctx, &arg_group, args, 0, false)?
                .unwrap_or_else(|| "0".to_string());
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = str_operand_handle_tag_dispatched(ctx, object, &recv_box);
            let blk = ctx.block();
            // Returns NaN-boxed number or undefined directly.
            let result = blk.call(
                DOUBLE,
                "js_string_code_point_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "charCodeAt" => {
            // Keep the coerced number for the inline operation: it already
            // checks and converts the index. An eager i32 conversion would
            // add an opaque call (and a round trip) on every ASCII byte read.
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // #3987: ignore extra args (still evaluate for side effects).
            // `ToNumber(index)` runs before the receiver is read (`coerce.rs`).
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let idx_d = pre_coerce_number(ctx, &arg_group, args, 0)?
                .unwrap_or_else(|| crate::nanbox::double_literal(0.0));
            let recv_box = reread_recv(ctx, group, recv);
            if let Some(value) = lower_char_code_at_inline(ctx, object, &recv_box, &idx_d) {
                let result = value;
                arg_group.release(ctx);
                return Ok(result);
            }
            let recv_handle = str_operand_handle_tag_dispatched(ctx, object, &recv_box);
            let blk = ctx.block();
            let idx_i32 = blk.call(I32, "js_string_number_index_to_i32", &[(DOUBLE, &idx_d)]);
            // js_string_char_code_at returns a plain f64 (NaN for OOB).
            let result = blk.call(
                DOUBLE,
                "js_string_char_code_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "lastIndexOf" => {
            // No `searchString` → `undefined` → "undefined"
            // (`"".lastIndexOf()` === -1).
            // `ToString(searchString)` runs the arg's user `toString`/`valueOf`
            // (ECMA-262 §22.1.3.9) before `ToNumber(position)`; static strings
            // skip the coercion. Both run before the receiver is read
            // (`coerce.rs`). Optional `position` (2nd arg): without it, use the
            // plain last-index-of (search to the end); with it, the
            // position-aware variant. Mirrors the `indexOf` arm.
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let needle = pre_coerce_string(
                ctx,
                &mut arg_group,
                args,
                0,
                StringCoercion::Plain,
                later_coercion_may_collect(ctx, property, args, 0),
            )?;
            let pos_double = pre_coerce_number(ctx, &arg_group, args, 1)?;
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let needle_handle = arg_group.reread_emitted(ctx, needle);
            let blk = ctx.block();
            let i32_v = if let Some(pos_num) = pos_double {
                // `ToNumber(position)` already ran above (NaN/±Inf preserved
                // for the helper's clamp; an `undefined` position is a NaN to
                // `f64::is_nan` as well), NOT the raw NaN-boxed bits.
                blk.call(
                    I32,
                    "js_string_last_index_of_from",
                    &[
                        (I64, &recv_handle),
                        (I64, &needle_handle),
                        (DOUBLE, &pos_num),
                        (I32, "1"),
                    ],
                )
            } else {
                blk.call(
                    I32,
                    "js_string_last_index_of",
                    &[(I64, &recv_handle), (I64, &needle_handle)],
                )
            };
            let result = blk.sitofp(I32, &i32_v, DOUBLE);
            arg_group.release(ctx);
            Ok(result)
        }
        "padStart" | "padEnd" => {
            if args.is_empty() {
                bail!(
                    "perry-codegen: String.{} expects 1 or 2 args, got {}",
                    property,
                    args.len()
                );
            }
            // `ToLength(maxLength)` (ECMA-262 §22.1.3.16 `StringPad` step 1)
            // must run — including any `valueOf`/`toString` on an object
            // `maxLength` — BEFORE `ToString(fillString)`, and both before the
            // receiver is read (`coerce.rs`): either can collect, and the
            // receiver is a young string more often than not
            // (`b.toString(16).padStart(2, "0")`). Coercing eagerly (rather
            // than leaving the raw boxed value for the runtime `to_length`
            // helper to interpret as a plain f64) is also what makes a
            // non-number `maxLength` run `ToNumber` at all.
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let len_d = pre_coerce_number(ctx, &arg_group, args, 0)?
                .expect("padStart/padEnd has at least one argument");
            // Optional pad string; defaults to " " when missing. A provided
            // fill is `ToString`-coerced (ECMA-262 §22.1.3.16) via
            // `js_string_pad_fill` — `undefined` → null handle (runtime falls
            // back to " "), otherwise ToString — so non-string fills (numbers,
            // booleans, `null`, `{ toString }`) render correctly instead of
            // being bit-cast and dropped.
            let pad = if args.len() >= 2 {
                Some(pre_coerce_string(
                    ctx,
                    &mut arg_group,
                    args,
                    1,
                    StringCoercion::PadFill,
                    false,
                )?)
            } else {
                None
            };
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let pad_handle = match pad {
                Some(pad) => arg_group.reread_emitted(ctx, pad),
                None => {
                    let sp_idx = ctx.strings.intern(" ");
                    let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                    let blk = ctx.block();
                    let sp_box = blk.load(DOUBLE, &sp_global);
                    unbox_str_handle(blk, &sp_box)
                }
            };
            let blk = ctx.block();
            // Pass `target_length` as raw DOUBLE — the runtime does the
            // ToLength coercion (NaN/negative → 0, Infinity / huge values
            // clamped to a sane max). Pre-fix the codegen did
            // `fptosi(DOUBLE → I32)` here, which is undefined behavior on
            // NaN per LLVM semantics; the resulting i32 then aliased the
            // runtime's `u32` parameter and a literal `-1` became
            // `0xFFFFFFFF`, looping to fill 4 GiB of padding before OOM.
            let runtime_fn = if property == "padStart" {
                "js_string_pad_start"
            } else {
                "js_string_pad_end"
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (DOUBLE, &len_d), (I64, &pad_handle)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        "normalize" => {
            // Takes the form from args[0]; per spec, surplus args are
            // evaluated then ignored. The runtime applies ToString + form
            // validation: omitted (undefined) → NFC default; explicit
            // null/""/"BAD" → RangeError. Pass the raw NaN-boxed form
            // value (#2782).
            let form_box = if args.is_empty() {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            } else {
                let form = lower_expr(ctx, &args[0])?;
                for extra in &args[1..] {
                    let _ = lower_expr(ctx, extra)?;
                }
                form
            };
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_normalize",
                &[(I64, &recv_handle), (DOUBLE, &form_box)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "localeCompare" => {
            // A missing/undefined `that` argument coerces to the string
            // "undefined" (ECMA-262 §22.1.3.10: `ToString(that)`), so
            // `s.localeCompare()` === `s.localeCompare(undefined)` ===
            // `s.localeCompare("undefined")`.
            //
            // ECMA-402 order: `ToString(that)`, THEN `Construct(%Collator%,
            // «locales, options»)`; both run before the receiver is read
            // (`coerce.rs`), and the coerced `that` stays rooted across the
            // collator validation's getters.
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let other = pre_coerce_string(
                ctx,
                &mut arg_group,
                args,
                0,
                StringCoercion::Plain,
                later_coercion_may_collect(ctx, property, args, 0),
            )?;
            // `(locales, options)` are resolved exactly as `Construct(%Collator%,
            // «locales, options»)` would — CanonicalizeLocaleList then every
            // InitializeCollator GetOption read, each option read ONCE — for the
            // throwing side effects and the resolved `numeric` flag (#2781,
            // #5906). Collation ordering otherwise stays locale-neutral (full
            // ICU deferred). The comparison takes that flag and reads no JS
            // value, so nothing runs user code once the string addresses are
            // taken below (#12539).
            let numeric = if args.len() >= 2 {
                let loc = arg_group.reread(ctx, 1)?;
                let opts = if args.len() >= 3 {
                    arg_group.reread(ctx, 2)?
                } else {
                    ctx.block()
                        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64)
                };
                Some(ctx.block().call(
                    I32,
                    "js_string_resolve_collator_args",
                    &[(DOUBLE, &loc), (DOUBLE, &opts)],
                ))
            } else {
                None
            };
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let other_handle = arg_group.reread_emitted(ctx, other);
            let blk = ctx.block();
            // Returns a plain f64 (-1/0/1) — NOT NaN-tagged.
            let result = if let Some(numeric) = numeric {
                Ok(blk.call(
                    DOUBLE,
                    "js_string_locale_compare_resolved",
                    &[(I64, &recv_handle), (I64, &other_handle), (I32, &numeric)],
                ))
            } else {
                Ok(blk.call(
                    DOUBLE,
                    "js_string_locale_compare",
                    &[(I64, &recv_handle), (I64, &other_handle)],
                ))
            };
            arg_group.release(ctx);
            result
        }
        "search" | "match" | "matchAll" => {
            let mut argument_group = open_rooted_group(1);
            let pattern = if let Some(arg) = args.first() {
                Some(argument_group.lower(ctx, arg, true)?)
            } else {
                None
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let pattern = match pattern {
                Some(index) => argument_group.reread(ctx, index)?,
                None => crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let recv_box = reread_recv(ctx, group, recv);
            let callee = if property == "match" {
                "js_string_match_js"
            } else if property == "matchAll" {
                "js_string_match_all_js"
            } else {
                "js_string_search_js"
            };
            let result =
                ctx.block()
                    .call(DOUBLE, callee, &[(DOUBLE, &recv_box), (DOUBLE, &pattern)]);
            argument_group.release(ctx);
            Ok(result)
        }
        "isWellFormed" => {
            // No-arg method; extras are evaluated for side effects then ignored.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // Returns a NaN-tagged boolean directly.
            Ok(blk.call(DOUBLE, "js_string_is_well_formed", &[(I64, &recv_handle)]))
        }
        "toWellFormed" => {
            // No-arg method; extras are evaluated for side effects then ignored.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(I64, "js_string_to_well_formed", &[(I64, &recv_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        "concat" => {
            // str.concat(s1, s2, …) = str + ToString(s1) + ToString(s2) + … .
            // Each arg is `ToString`-coerced (ECMA-262 §22.1.3.5) — a non-string
            // arg (`undefined`, a boolean, a `{ toString }` object) must render
            // as its string form, not be bit-cast as a string handle (which
            // dropped `undefined`/booleans). A static string arg skips coercion.
            if args.is_empty() {
                return Ok(reread_recv(ctx, group, recv));
            }

            // A call evaluates every argument before entering the method. Keep
            // those raw values rooted, then perform ToString left-to-right;
            // interleaving `lower(arg); coerce(arg)` would observably differ
            // for `concat(make("a"), make("b"))`.
            with_rooted_group(ctx, args.len(), |ctx, raw_group| {
                let mut raw_args = Vec::with_capacity(args.len());
                let raw_window_collects = args.iter().any(|arg| concat_arg_may_collect(ctx, arg));
                for arg in args {
                    // The window covers both later argument evaluation and the
                    // subsequent user-code-capable coercion pass.
                    raw_args.push(raw_group.lower(ctx, arg, raw_window_collects)?);
                }

                // Fuse the receiver plus the first 31 arguments. The runtime
                // caps one chain at 32 parts, so a pathological larger call
                // keeps the existing rooted pairwise lowering only for its tail.
                let fused_arg_count = args.len().min(CONCAT_CHAIN_MAX_PARTS - 1);
                let fused = with_rooted_group(ctx, fused_arg_count, |ctx, parts_group| {
                    let mut part_roots = Vec::with_capacity(fused_arg_count);
                    for (i, arg) in args[..fused_arg_count].iter().enumerate() {
                        let raw_box = raw_group.reread(ctx, raw_args[i])?;
                        let part_box = coerce_concat_arg_to_box(ctx, arg, &raw_box);
                        let future_can_collect = args[i + 1..fused_arg_count]
                            .iter()
                            .any(|later| concat_arg_may_collect(ctx, later));
                        part_roots.push(parts_group.adopt_emitted(
                            ctx,
                            Repr::Boxed,
                            &part_box,
                            future_can_collect,
                        ));
                    }

                    // Store only post-coercion, post-collection re-reads in the
                    // scratch array. A plain alloca is not a GC root.
                    let mut parts = Vec::with_capacity(fused_arg_count + 1);
                    parts.push(reread_recv(ctx, group, recv));
                    for part in part_roots {
                        parts.push(parts_group.reread_emitted(ctx, part));
                    }
                    Ok(emit_string_concat_chain(ctx, &parts))
                })?;

                if fused_arg_count == args.len() {
                    return Ok(fused);
                }

                // Above the cap, the fused prefix becomes the accumulator and
                // the old pairwise path handles only the remaining arguments.
                let tail = &args[fused_arg_count..];
                let acc_handle = unbox_str_handle(ctx.block(), &fused);
                let tail_can_collect = tail.iter().any(|a| concat_arg_may_collect(ctx, a));
                with_rooted_accumulator(
                    ctx,
                    Repr::Ptr,
                    &acc_handle,
                    tail_can_collect,
                    |ctx, acc| {
                        for (tail_i, arg) in tail.iter().enumerate() {
                            let raw_i = fused_arg_count + tail_i;
                            let raw_box = raw_group.reread(ctx, raw_args[raw_i])?;
                            let s_box = coerce_concat_arg_to_box(ctx, arg, &raw_box);
                            // The tail's pointer-only helper needs a heap handle.
                            // This is inert for a coerced heap string and
                            // materializes a proven SSO value.
                            let s_handle =
                                ctx.block()
                                    .call(I64, "js_string_coerce", &[(DOUBLE, &s_box)]);
                            // Write the new accumulator back, so the NEXT
                            // argument keeps this result alive, not its input.
                            acc.advance(ctx, "js_string_concat", &[Arg::Plain(I64, &s_handle)]);
                        }
                        Ok(())
                    },
                    |ctx, handle| Ok(nanbox_string_inline(ctx.block(), handle)),
                )
            })
        }
        "substr" => {
            // Legacy substr(start, length) — distinct from substring/slice:
            // negative start counts from the end, the 2nd arg is a LENGTH, and
            // a non-positive length yields "" while an `undefined` length takes
            // the rest of the string. Routed to the dedicated runtime helper
            // `js_string_substr`, which coerces both args via ToIntegerOrInfinity
            // (#2897).
            if args.is_empty() {
                bail!(
                    "perry-codegen: String.substr expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            // `ToNumber` of both arguments (spec order, start first) runs before
            // the receiver is unboxed (`coerce.rs`) — `js_string_substr` takes
            // the raw receiver and would otherwise run a `{ valueOf }` with it
            // held. What reaches it is a number, or `undefined`, from which it
            // still decides "rest of string" itself; a `Symbol` still throws in
            // the coercion, like Node.
            let arg_group = lower_coerced_args(ctx, property, args)?;
            let start_d = pre_coerce_number(ctx, &arg_group, args, 0)?
                .expect("substr has at least one argument");
            let len_d = pre_coerce_number(ctx, &arg_group, args, 1)?;
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // An omitted length is the explicit `undefined` value.
            let length_d = match len_d {
                Some(len_d) => len_d,
                None => blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64),
            };
            let result = blk.call(
                I64,
                "js_string_substr",
                &[(I64, &recv_handle), (DOUBLE, &start_d), (DOUBLE, &length_d)],
            );
            let result = nanbox_string_inline(blk, &result);
            arg_group.release(ctx);
            Ok(result)
        }
        "startsWith" | "endsWith" => {
            // Spec allows the 2-arg form: startsWith(searchString, position)
            // and endsWith(searchString, endPosition). Closes #315.
            // #10476: an omitted searchString is `undefined`, which ToString
            // turns into "undefined" (`"xundefined".endsWith()` is true). A
            // compile-time arity error here also rejected the string arm of
            // the tag guard for a user `endsWith()` on an `any` receiver.
            // `ToString(searchString)` (after the RegExp check) and
            // `ToNumber(position)` run, in that order, before the receiver is
            // read (`coerce.rs`).
            let method_id = regexp_search_method_id(property);
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let other = pre_coerce_string(
                ctx,
                &mut arg_group,
                args,
                0,
                StringCoercion::Search(&method_id),
                later_coercion_may_collect(ctx, property, args, 0),
            )?;
            let pos_i32 = pre_coerce_index(ctx, &arg_group, args, 1, false)?;
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let other_handle = arg_group.reread_emitted(ctx, other);
            let blk = ctx.block();
            let result_i32 = if let Some(pos_i32) = pos_i32 {
                // ToIntegerOrInfinity(position) on the number coerced above:
                // the runtime helper, not a raw `fptosi` (UB on NaN/±Inf).
                let runtime_fn = if property == "startsWith" {
                    "js_string_starts_with_at"
                } else {
                    "js_string_ends_with_at"
                };
                blk.call(
                    I32,
                    runtime_fn,
                    &[(I64, &recv_handle), (I64, &other_handle), (I32, &pos_i32)],
                )
            } else {
                let runtime_fn = if property == "startsWith" {
                    "js_string_starts_with"
                } else {
                    "js_string_ends_with"
                };
                blk.call(
                    I32,
                    runtime_fn,
                    &[(I64, &recv_handle), (I64, &other_handle)],
                )
            };
            let result = i32_bool_to_nanbox(blk, &result_i32);
            arg_group.release(ctx);
            Ok(result)
        }
        "includes" => {
            // str.includes(sub, position?) -> boolean. Implemented as
            // js_string_index_of_from(str, sub, position) != -1, then
            // NaN-tagged. #2812: the optional `position` argument must be
            // honored (search starts there), matching the dynamic dispatch
            // path. Negative/NaN clamp to 0 and Infinity saturates past the
            // end inside js_string_index_of_from.
            // #10476: an omitted searchString is `undefined` (see above).
            // Both coercions run before the receiver is read (`coerce.rs`); the
            // second argument is evaluated for side effects either way and
            // used as the start index when present.
            let method_id = regexp_search_method_id(property);
            let mut arg_group = lower_coerced_args(ctx, property, args)?;
            let needle = pre_coerce_string(
                ctx,
                &mut arg_group,
                args,
                0,
                StringCoercion::Search(&method_id),
                later_coercion_may_collect(ctx, property, args, 0),
            )?;
            let from_i32 = pre_coerce_index(ctx, &arg_group, args, 1, false)?
                .unwrap_or_else(|| "0".to_string());
            let recv_box = reread_recv(ctx, group, recv);
            let recv_handle = unbox_str_handle(ctx.block(), &recv_box);
            let needle_handle = arg_group.reread_emitted(ctx, needle);
            let blk = ctx.block();
            let idx_i32 = blk.call(
                I32,
                "js_string_index_of_from",
                &[(I64, &recv_handle), (I64, &needle_handle), (I32, &from_i32)],
            );
            // includes := indexOf != -1
            let neg_one = "-1".to_string();
            let bit = blk.icmp_ne(I32, &idx_i32, &neg_one);
            let tagged = blk.select(
                I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            let result = blk.bitcast_i64_to_double(&tagged);
            arg_group.release(ctx);
            Ok(result)
        }
        // `.toString()` on a union-typed receiver (string | number) may
        // arrive here when `is_string_expr` returned true because the
        // union contains String. Dispatch through js_jsvalue_to_string
        // which inspects the NaN tag at runtime — correct for both a
        // real string and a narrowed number/bool/etc.
        "toString" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let recv_box = reread_recv(ctx, group, recv);
            let blk = ctx.block();
            let handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &recv_box)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        // Issue #510: an unknown method on a string-typed receiver
        // (e.g. `s.lengt()` — typo of `length`) was previously
        // silently lowered to `Ok(recv_box)`, which evaluated the
        // call to the receiver itself and let execution continue.
        // That masked typos as no-ops and matched neither Node nor
        // the spec.
        //
        // Match Node: emit a TypeError abort via
        // `js_throw_type_error_not_a_function("string", "<prop>")`,
        // followed by `unreachable` (the helper is `-> !`). The
        // arguments to the unknown method are still lowered for
        // side effects, in case any of them have observable
        // effects on completed evaluation order.
        _ => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            // Intern the receiver-kind label and the property name
            // into the string pool so we can pass byte ptr + length
            // to the runtime helper (same shape #462 uses for
            // `js_throw_type_error_property_access`).
            let kind_idx = ctx.strings.intern("string");
            let kind_entry = ctx.strings.entry(kind_idx);
            let kind_bytes_global = format!("@{}", kind_entry.bytes_global);
            let kind_len_str = kind_entry.byte_len.to_string();
            let prop_idx = ctx.strings.intern(property);
            let prop_entry = ctx.strings.entry(prop_idx);
            let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
            let prop_len_str = prop_entry.byte_len.to_string();
            let blk = ctx.block();
            blk.call_void(
                "js_throw_type_error_not_a_function",
                &[
                    (PTR, &kind_bytes_global),
                    (I64, &kind_len_str),
                    (PTR, &prop_bytes_global),
                    (I64, &prop_len_str),
                ],
            );
            blk.unreachable();
            // The block is now terminated; downstream lowering
            // expects a value register to phi against. Return a
            // placeholder undefined — `unreachable` above means
            // this is never read at runtime.
            Ok(crate::nanbox::double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            )))
        }
    }
}
