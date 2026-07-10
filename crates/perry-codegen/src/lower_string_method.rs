//! String method and concatenation lowering.
//!
//! Contains `lower_string_method`, `lower_string_self_append`,
//! `lower_string_coerce_concat`, and `lower_string_concat`.

use anyhow::{anyhow, bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{
    i32_bool_to_nanbox, lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_str_handle,
    FnCtx,
};
use crate::type_analysis::is_string_expr;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

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
    // Optimistic any-typed path: `property_get` routes `(x: any).charAt(i)` /
    // `.split(…)` here even when `x` is not statically a string, because most
    // such receivers ARE strings (e.g. `readFileSync(p).split('\n')`). But a
    // boxed/object receiver — `new Boolean().charAt = String.prototype.charAt;
    // …charAt(i)`, a `{ toString }` object — must have `ToString(this)` applied
    // (ECMA-262 §22.1.3) before the inline string helpers run, or they would
    // bit-cast the object pointer as a string and read garbage. A statically
    // string-typed receiver skips this (fast path, no coercion).
    let recv_box = if is_string_expr(ctx, object) {
        recv_box
    } else {
        let blk = ctx.block();
        let coerced = blk.call(I64, "js_string_coerce", &[(DOUBLE, &recv_box)]);
        nanbox_string_inline(blk, &coerced)
    };

    match property {
        "indexOf" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: String.indexOf expects 0, 1 or 2 args, got {}",
                    args.len()
                );
            }
            // No `searchString` → `undefined`, which `js_string_coerce`
            // stringifies to "undefined" (`"".indexOf()` === -1).
            let needle_box = if args.is_empty() {
                ctx.block()
                    .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64)
            } else {
                lower_expr(ctx, &args[0])?
            };
            // An object `searchString` must be `ToString`-coerced (running its
            // user `toString`/`valueOf`) BEFORE `ToNumber(position)`, per
            // ECMA-262 §22.1.3.8. A statically string-typed arg skips this.
            let needle_is_str = !args.is_empty() && is_string_expr(ctx, &args[0]);
            // Optional fromIndex.
            let from_idx_double = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = if needle_is_str {
                unbox_str_handle(blk, &needle_box)
            } else {
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &needle_box)])
            };
            let result_i32 = if let Some(from_d) = from_idx_double {
                // `ToIntegerOrInfinity(position)` via the runtime helper (runs
                // user `valueOf`, handles ±Infinity/NaN), NOT a raw `fptosi`.
                let from_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &from_d)]);
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
            Ok(blk.sitofp(I32, &result_i32, DOUBLE))
        }
        "slice" | "substring" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: String.{} expects 0, 1 or 2 args, got {}",
                    property,
                    args.len()
                );
            }
            // Issue #316: 0-arg form is the spec'd "clone" idiom —
            // `s.slice()` ≡ `s.slice(0, length)`. Was rejected at
            // codegen with "expects 1 or 2 args, got 0" before this fix.
            let start_d = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            // 2-arg form: explicit end (may be `undefined` → treated as `len`).
            let end_d = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // String length (i32 at header offset 0). Used as the default end
            // (0/1-arg form, issue #316/#214) and the `undefined`-end fallback.
            // Routed through SSO-safe unbox so SHORT_STRING_TAG receivers work.
            let len_ptr = blk.inttoptr(I64, &recv_handle);
            let len_i32 = blk.load(I32, &len_ptr);
            // `ToIntegerOrInfinity` via the runtime helper, NOT a raw `fptosi`:
            // `fptosi(±Infinity/NaN → i32)` is UB and on x86 yields the integer-
            // indefinite `i32::MIN`, so `"abc".slice(Infinity)` / `.substring(
            // Infinity, NaN)` clamped to the wrong end. The helper truncates and
            // clamps ±Infinity to the i32 bounds (and runs ToNumber on a boxed
            // arg), matching the char-access methods and the runtime dispatch arm.
            let start_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &start_d)]);
            // An explicit `undefined` end means `len` (not `ToInteger(undefined)
            // === 0`), per spec — `s.substring(0, undefined) === s`.
            let end_i32 = match &end_d {
                Some(end_d) => blk.call(
                    I32,
                    "js_string_end_index_to_i32",
                    &[(DOUBLE, end_d), (I32, &len_i32)],
                ),
                None => len_i32.clone(),
            };
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
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "split" => {
            // Issue #567: accept the optional 2nd `limit: number` arg.
            // `str.split()` with no args is valid: an `undefined` separator
            // yields `[str]` (handled by `js_string_split_value`).
            if args.len() > 2 {
                bail!(
                    "perry-codegen: String.split expects 0, 1, or 2 args (delimiter[, limit]), got {}",
                    args.len()
                );
            }
            // A literal separator with no `limit` cannot invoke user code,
            // cannot be a RegExp, and has the unbounded limit directly
            // expressible by `js_string_split_n`. Avoid the boxed dispatch and
            // its coercion/undefined/RegExp checks for this common hot path.
            if args.len() == 1 && matches!(&args[0], Expr::String(_) | Expr::WtfString(_)) {
                let delim_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let recv_handle = unbox_str_handle(blk, &recv_box);
                let delim_handle = unbox_str_handle(blk, &delim_box);
                let result_arr = blk.call(
                    I64,
                    "js_string_split_n",
                    &[(I64, &recv_handle), (I64, &delim_handle), (I32, "-1")],
                );
                return Ok(crate::expr::nanbox_pointer_inline(blk, &result_arr));
            }
            // Route through `js_string_split_value`, which takes the BOXED
            // separator and limit and performs the full spec coercion:
            // `ToUint32(limit)` before `ToString(separator)`, an `undefined`
            // separator → `[S]`, `limit === 0` → `[]`, and RegExp-separator
            // delegation (detected via the regex-pointer registry). A raw
            // `unbox_str_handle` of an object/undefined separator would
            // bit-cast garbage; a raw `fptosi` of a boxed limit skips its
            // `valueOf`.
            let delim_box = if args.is_empty() {
                None
            } else {
                Some(lower_expr(ctx, &args[0])?)
            };
            let limit_box = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // No separator → pass `undefined`, which `js_string_split_value`
            // resolves to `[S]`.
            let delim_box = match delim_box {
                Some(v) => v,
                None => blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64),
            };
            let limit_box = match limit_box {
                Some(v) => v,
                None => blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64),
            };
            let result_arr = blk.call(
                I64,
                "js_string_split_value",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &delim_box),
                    (DOUBLE, &limit_box),
                ],
            );
            // Returns an array pointer (ArrayHeader*) — NaN-box with POINTER_TAG.
            Ok(crate::expr::nanbox_pointer_inline(blk, &result_arr))
        }
        // toLocaleLowerCase / toLocaleUpperCase — honor the `locales` arg:
        // validate BCP 47 tags (throwing RangeError on a bad tag) and apply
        // Turkic (tr/az) dotted/dotless `I` casing. Other locales fall back to
        // language-neutral Unicode casing. Closes #2781. (#592: Effect's
        // `aliasOrValue` at Cron.ts:846 was the original user-impact site.)
        "toLocaleLowerCase" | "toLocaleUpperCase" => {
            if args.len() > 1 {
                bail!(
                    "perry-codegen: String.{} expects 0 or 1 args, got {}",
                    property,
                    args.len()
                );
            }
            // The `locales` arg is passed as a NaN-boxed JSValue (double) to the
            // runtime, which extracts/validates it. Missing → undefined.
            let locales_box = if args.is_empty() {
                None
            } else {
                Some(lower_expr(ctx, &args[0])?)
            };
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
            Ok(nanbox_string_inline(blk, &result))
        }
        // Unary string-returning methods (no args).
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" => {
            // These methods take no args; JS ignores any extras but still
            // evaluates them for side effects (ECMA-262 argument evaluation
            // precedes the call). Evaluate then discard.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
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
            let value_d = if args.is_empty() {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            } else {
                lower_expr(ctx, &args[0])?
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let value_handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &value_d)]);
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
            Ok(nanbox_string_inline(blk, &result))
        }
        "charAt" => {
            // #2787: a missing index defaults to 0; the provided index is
            // coerced with JS `ToIntegerOrInfinity` (undefined/NaN -> 0) rather
            // than a raw `fptosi`, which is UB on a NaN bit pattern.
            // #3987: JS ignores extra args to `charAt` but still evaluates them
            // (left-to-right, for side effects). Use args[0] as the index and
            // lower the rest, discarding their values, instead of bailing.
            let idx_d = if args.is_empty() {
                crate::nanbox::double_literal(0.0)
            } else {
                lower_expr(ctx, &args[0])?
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &idx_d)]);
            let result = blk.call(
                I64,
                "js_string_char_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "repeat" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.repeat expects 1 arg, got {}",
                    args.len()
                );
            }
            let count_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_repeat",
                &[(I64, &recv_handle), (DOUBLE, &count_d)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "replace" | "replaceAll" => {
            if args.len() != 2 {
                bail!(
                    "perry-codegen: String.{} expects 2 args, got {}",
                    property,
                    args.len()
                );
            }
            // First arg is either a string or a regex literal. The
            // second arg can be a string OR a function (replacer
            // callback). Pick the right runtime function based on
            // both shapes.
            let needle_is_regex = matches!(&args[0], Expr::RegExp { .. })
                || matches!(&args[0], Expr::LocalGet(id) if matches!(
                    ctx.local_types.get(id),
                    Some(HirType::Named(n)) if n == "RegExp"
                ));
            // Detect a function replacer: a Closure literal, a FuncRef,
            // or a LocalGet of a function-typed local.
            let repl_is_function = matches!(&args[1], Expr::Closure { .. } | Expr::FuncRef(_))
                || matches!(&args[1], Expr::LocalGet(id) if ctx.local_closure_func_ids.contains_key(id));
            // Detect a string literal that includes $<name> back-refs
            // so we route to the named-group-aware runtime variant.
            let repl_has_named = matches!(&args[1], Expr::String(s) if s.contains("$<"));
            // A non-RegExp, non-static-string `searchValue` is `ToString`-coerced
            // (running user `toString`/`valueOf`, may throw) BEFORE the
            // replacement is coerced, per ECMA-262 §22.1.3.19. Likewise a
            // non-function, non-static-string `replaceValue`.
            let needle_is_str = is_string_expr(ctx, &args[0]);
            let repl_is_str = is_string_expr(ctx, &args[1]);
            let needle_box = lower_expr(ctx, &args[0])?;
            let repl_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // #4871: a `searchValue` codegen can't type (an object-property
            // read, a destructured loop binding, a call result) may still be
            // a RegExp at runtime. ToString-coercing it here turned
            // `str.replace(obj.regex, …)` into a literal search for "/foo/g"
            // — a silent no-op. Route through the runtime search dispatcher,
            // which checks the registered-RegExp set before coercing (and
            // handles every replacement shape via the `_dyn` family).
            if !needle_is_regex && !needle_is_str {
                let runtime_fn = if property == "replaceAll" {
                    "js_string_replace_all_search_dyn"
                } else {
                    "js_string_replace_search_dyn"
                };
                let result = blk.call(
                    I64,
                    runtime_fn,
                    &[
                        (I64, &recv_handle),
                        (DOUBLE, &needle_box),
                        (DOUBLE, &repl_box),
                    ],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            let needle_handle = if needle_is_regex || needle_is_str {
                unbox_str_handle(blk, &needle_box)
            } else {
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &needle_box)])
            };
            if repl_is_function {
                // repl_box is a NaN-boxed closure pointer (double).
                // The callback helpers take the callback as f64.
                let runtime_fn = match (needle_is_regex, property) {
                    (true, "replaceAll") => "js_string_replace_all_regex_fn",
                    (true, _) => "js_string_replace_regex_fn",
                    (false, "replaceAll") => "js_string_replace_all_string_fn",
                    (false, _) => "js_string_replace_string_fn",
                };
                let result = blk.call(
                    I64,
                    runtime_fn,
                    &[
                        (I64, &recv_handle),
                        (I64, &needle_handle),
                        (DOUBLE, &repl_box),
                    ],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            // A replacement whose shape codegen can't prove (not a Closure
            // literal/FuncRef/function-typed local AND not a static string)
            // may still be a FUNCTION at runtime — an IIFE-returned closure,
            // a call result, a property read (test262 10.4.3-1-102-s). Route
            // those through the `_dyn` runtime dispatchers, which check
            // callability before ToString-coercing.
            if !repl_is_str {
                let runtime_fn = match (needle_is_regex, property) {
                    (true, "replaceAll") => "js_string_replace_all_regex_dyn",
                    (true, _) => "js_string_replace_regex_dyn",
                    (false, "replaceAll") => "js_string_replace_all_string_dyn",
                    (false, _) => "js_string_replace_string_dyn",
                };
                let result = blk.call(
                    I64,
                    runtime_fn,
                    &[
                        (I64, &recv_handle),
                        (I64, &needle_handle),
                        (DOUBLE, &repl_box),
                    ],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            // Issue #214: SSO-safe unbox of replacement string; a non-static-
            // string replacement is `ToString`-coerced (after `searchValue`).
            let repl_handle = if repl_is_str {
                unbox_str_handle(blk, &repl_box)
            } else {
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &repl_box)])
            };
            let runtime_fn = if needle_is_regex {
                if property == "replaceAll" {
                    if repl_has_named {
                        "js_string_replace_all_regex_named"
                    } else {
                        "js_string_replace_all_regex"
                    }
                } else if repl_has_named {
                    "js_string_replace_regex_named"
                } else {
                    "js_string_replace_regex"
                }
            } else if property == "replaceAll" {
                "js_string_replace_all_string"
            } else {
                "js_string_replace_string"
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[
                    (I64, &recv_handle),
                    (I64, &needle_handle),
                    (I64, &repl_handle),
                ],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        // str.at(i) / str.charCodeAt(i) / str.codePointAt(i)
        "at" => {
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // `js_string_at` already resolves negative indices relative to len.
            // #3987: ignore extra args (still evaluate for side effects).
            let idx_d = if args.is_empty() {
                crate::nanbox::double_literal(0.0)
            } else {
                lower_expr(ctx, &args[0])?
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &idx_d)]);
            // js_string_at returns a NaN-boxed string or undefined directly.
            Ok(blk.call(
                DOUBLE,
                "js_string_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "codePointAt" => {
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // #3987: ignore extra args (still evaluate for side effects).
            let idx_d = if args.is_empty() {
                crate::nanbox::double_literal(0.0)
            } else {
                lower_expr(ctx, &args[0])?
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &idx_d)]);
            // Returns NaN-boxed number or undefined directly.
            Ok(blk.call(
                DOUBLE,
                "js_string_code_point_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "charCodeAt" => {
            // #2787: missing index -> 0; JS index coercion (undefined/NaN -> 0).
            // #3987: ignore extra args (still evaluate for side effects).
            let idx_d = if args.is_empty() {
                crate::nanbox::double_literal(0.0)
            } else {
                lower_expr(ctx, &args[0])?
            };
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &idx_d)]);
            // js_string_char_code_at returns a plain f64 (NaN for OOB).
            Ok(blk.call(
                DOUBLE,
                "js_string_char_code_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "lastIndexOf" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: String.lastIndexOf expects 0, 1 or 2 args, got {}",
                    args.len()
                );
            }
            // No `searchString` → `undefined` → "undefined"
            // (`"".lastIndexOf()` === -1).
            let needle_box = if args.is_empty() {
                ctx.block()
                    .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64)
            } else {
                lower_expr(ctx, &args[0])?
            };
            // `ToString(searchString)` runs the arg's user `toString`/`valueOf`
            // (ECMA-262 §22.1.3.9) before `ToNumber(position)`; static strings
            // skip the coercion.
            let needle_is_str = !args.is_empty() && is_string_expr(ctx, &args[0]);
            // Optional `position` (2nd arg). Without it, use the plain
            // last-index-of (search to the end); with it, the position-aware
            // variant. Mirrors the `indexOf` arm.
            let pos_double = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = if needle_is_str {
                unbox_str_handle(blk, &needle_box)
            } else {
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &needle_box)])
            };
            let i32_v = if let Some(pos_d) = pos_double {
                // `ToNumber(position)` (runs user `valueOf`, preserves NaN/±Inf
                // for the helper's clamp), NOT the raw NaN-boxed bits.
                let pos_num = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &pos_d)]);
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
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "padStart" | "padEnd" => {
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.{} expects 1 or 2 args, got {}",
                    property,
                    args.len()
                );
            }
            let len_d = lower_expr(ctx, &args[0])?;
            // `ToLength(maxLength)` (ECMA-262 §22.1.3.16 `StringPad` step 1)
            // must run — including any `valueOf`/`toString` on an object
            // `maxLength` — BEFORE `ToString(fillString)` below. Coercing
            // eagerly here (rather than leaving the raw boxed value for the
            // runtime `to_length` helper to interpret as a plain f64) also
            // fixes a correctness bug: a non-number `maxLength` (e.g. an
            // object) was previously read as garbage/NaN bit-pattern instead
            // of running `ToNumber`.
            let len_d = {
                let blk = ctx.block();
                blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &len_d)])
            };
            // Optional pad string; defaults to " " when missing. A provided
            // fill is `ToString`-coerced (ECMA-262 §22.1.3.16) via
            // `js_string_pad_fill` — `undefined` → null handle (runtime falls
            // back to " "), otherwise ToString — so non-string fills (numbers,
            // booleans, `null`, `{ toString }`) render correctly instead of
            // being bit-cast and dropped.
            let pad_handle = if args.len() == 2 {
                let pad_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.call(I64, "js_string_pad_fill", &[(DOUBLE, &pad_box)])
            } else {
                let sp_idx = ctx.strings.intern(" ");
                let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                let blk = ctx.block();
                let sp_box = blk.load(DOUBLE, &sp_global);
                unbox_str_handle(blk, &sp_box)
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
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
            Ok(nanbox_string_inline(blk, &result))
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
            if args.len() > 3 {
                bail!(
                    "perry-codegen: String.localeCompare expects 0-3 args, got {}",
                    args.len()
                );
            }
            // A missing/undefined `that` argument coerces to the string
            // "undefined" (ECMA-262 §22.1.3.10: `ToString(that)`), so
            // `s.localeCompare()` === `s.localeCompare(undefined)` ===
            // `s.localeCompare("undefined")`.
            let other_is_str = !args.is_empty() && is_string_expr(ctx, &args[0]);
            let other_box = if args.is_empty() {
                ctx.block()
                    .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64)
            } else {
                lower_expr(ctx, &args[0])?
            };
            // `options` is the 3rd arg; `locales` (2nd) is validated for its
            // RangeError side effect (#2781) but collation ordering stays
            // locale-neutral (full ICU deferred). With an options object
            // present, route to the variant that honors `{ numeric: true }`.
            let locales_box = if args.len() >= 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let options_box = if args.len() == 3 {
                Some(lower_expr(ctx, &args[2])?)
            } else {
                None
            };
            let blk = ctx.block();
            if let Some(loc) = &locales_box {
                blk.call_void("js_string_validate_locales", &[(DOUBLE, loc)]);
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // A non-string `that` (undefined/number/object) must be
            // `ToString`-coerced, not bit-cast as a string pointer.
            let other_handle = if other_is_str {
                unbox_str_handle(blk, &other_box)
            } else {
                blk.call(I64, "js_string_coerce", &[(DOUBLE, &other_box)])
            };
            // Returns a plain f64 (-1/0/1) — NOT NaN-tagged.
            if let Some(opts) = options_box {
                Ok(blk.call(
                    DOUBLE,
                    "js_string_locale_compare_opts",
                    &[(I64, &recv_handle), (I64, &other_handle), (DOUBLE, &opts)],
                ))
            } else {
                Ok(blk.call(
                    DOUBLE,
                    "js_string_locale_compare",
                    &[(I64, &recv_handle), (I64, &other_handle)],
                ))
            }
        }
        "search" => {
            if args.len() > 1 {
                bail!(
                    "perry-codegen: String.search expects 0 or 1 arg, got {}",
                    args.len()
                );
            }
            // The arg may be a RegExp OR any value that `RegExpCreate` coerces
            // via `ToString` (a string pattern, `undefined`, a `{ toString }`
            // object). Pass it BOXED to `js_string_search_value`, which detects
            // a RegExp pointer and otherwise builds `RegExpCreate(ToString(arg))`
            // — a raw `unbox_str_handle` would bit-cast a non-regex arg as a
            // regex header and always return -1. A missing arg is `undefined`.
            let re_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let i32_v = blk.call(
                I32,
                "js_string_search_value",
                &[(I64, &recv_handle), (DOUBLE, &re_box)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "match" => {
            if args.len() > 1 {
                bail!(
                    "perry-codegen: String.match expects 0 or 1 arg, got {}",
                    args.len()
                );
            }
            // Like `search`, coerce a non-RegExp arg via `RegExpCreate(ToString
            // (arg))` by passing it BOXED to `js_string_match_value`. A missing
            // arg is `undefined` → the empty `/(?:)/` regex.
            let re_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_match_value",
                &[(I64, &recv_handle), (DOUBLE, &re_box)],
            );
            // Runtime may return null (0) on no-match. Convert that to
            // TAG_NULL so `s.match(re) !== null` behaves correctly.
            let is_null = blk.icmp_eq(I64, &result, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &result);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected =
                ctx.block()
                    .select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &ptr_bits);
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }
        "matchAll" => {
            if args.len() > 1 {
                bail!(
                    "perry-codegen: String.matchAll expects 0 or 1 arg, got {}",
                    args.len()
                );
            }
            let pattern_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_match_all_value",
                &[(I64, &recv_handle), (DOUBLE, &pattern_box)],
            );
            // matchAll returns a RegExp String Iterator object.
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "isWellFormed" => {
            // No-arg method; extras are evaluated for side effects then ignored.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
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
            let blk = ctx.block();
            let mut acc_handle = unbox_str_handle(blk, &recv_box);
            for a in args {
                let a_is_str = is_string_expr(ctx, a);
                let s_box = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let s_handle = if a_is_str {
                    unbox_str_handle(blk, &s_box)
                } else {
                    blk.call(I64, "js_string_coerce", &[(DOUBLE, &s_box)])
                };
                acc_handle = blk.call(
                    I64,
                    "js_string_concat",
                    &[(I64, &acc_handle), (I64, &s_handle)],
                );
            }
            Ok(nanbox_string_inline(ctx.block(), &acc_handle))
        }
        "substr" => {
            // Legacy substr(start, length) — distinct from substring/slice:
            // negative start counts from the end, the 2nd arg is a LENGTH, and
            // a non-positive length yields "" while an `undefined` length takes
            // the rest of the string. Routed to the dedicated runtime helper
            // `js_string_substr`, which coerces both args via ToIntegerOrInfinity
            // (#2897).
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.substr expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            let start_d = lower_expr(ctx, &args[0])?;
            let len_d = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // Pass the raw NaN-boxed args straight through: `js_string_substr`
            // runs `ToIntegerOrInfinity` on both (spec order, start first) so a
            // boolean / `{ valueOf }` / `Symbol` coerces or throws like Node,
            // and decides "rest of string" from an `undefined` length itself —
            // a raw `fptosi` here would mis-truncate NaN and lose that signal.
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
            Ok(nanbox_string_inline(blk, &result))
        }
        "startsWith" | "endsWith" => {
            // Spec allows the 2-arg form: startsWith(searchString, position)
            // and endsWith(searchString, endPosition). Closes #315.
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.{} expects 1 or 2 args, got {}",
                    property,
                    args.len()
                );
            }
            let other_box = lower_expr(ctx, &args[0])?;
            let pos_d = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let method_id = regexp_search_method_id(property);
            let other_handle = blk.call(
                I64,
                "js_string_search_value_to_string",
                &[(DOUBLE, &other_box), (I32, &method_id)],
            );
            let result_i32 = if let Some(pos_d) = pos_d {
                // ToIntegerOrInfinity(position): route through the runtime
                // coercion (not a raw `fptosi` on the NaN-boxed value) so a
                // boolean/numeric-string/`{valueOf}` coerces per spec and a
                // Symbol position throws a TypeError (matches the dynamic
                // dispatch path and the `includes` arm below).
                let pos_i32 = blk.call(I32, "js_string_index_to_i32", &[(DOUBLE, &pos_d)]);
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
            Ok(i32_bool_to_nanbox(blk, &result_i32))
        }
        "includes" => {
            // str.includes(sub, position?) -> boolean. Implemented as
            // js_string_index_of_from(str, sub, position) != -1, then
            // NaN-tagged. #2812: the optional `position` argument must be
            // honored (search starts there), matching the dynamic dispatch
            // path. Negative/NaN clamp to 0 and Infinity saturates past the
            // end inside js_string_index_of_from.
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.includes expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            // Preserve evaluation of the second argument for side effects and
            // use it as the start index when present.
            let pos_d = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let method_id = regexp_search_method_id(property);
            let needle_handle = blk.call(
                I64,
                "js_string_search_value_to_string",
                &[(DOUBLE, &needle_box), (I32, &method_id)],
            );
            let from_i32 = match pos_d {
                // Use the runtime ToIntegerOrInfinity helper rather than a raw
                // `fptosi`, which is undefined for Infinity/NaN.
                Some(pos_d) => blk.call(I32, "js_string_position_to_index", &[(DOUBLE, &pos_d)]),
                None => "0".to_string(),
            };
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
            Ok(blk.bitcast_i64_to_double(&tagged))
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

/// Lower the `str = str + rhs` self-append pattern. Uses the in-place
/// `js_string_append` runtime function (refcount=1 → mutate in place,
/// otherwise allocate). The returned pointer is stored back to the local
/// slot — `js_string_append` may realloc when growing past capacity.
///
/// This is the load-bearing optimization for the canonical `let str = "";
/// for (...) str = str + "a"` string-build pattern.
pub(crate) fn lower_string_self_append(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    rhs: &Expr,
) -> Result<String> {
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("string self-append: local {} not in scope", local_id))?
        .clone();

    // Lower the RHS first (might be a string literal, a local, or a
    // computed expression). For non-string RHS we'd need to coerce, but
    // the bench_string_ops case always uses a string literal, so for the
    // first slice we require the RHS to be a known string.
    if !is_string_expr(ctx, rhs) {
        // Fall back to the slower concat path: load the local, do a
        // generic concat-coerce, store back.
        let lhs_val = ctx.block().load(DOUBLE, &slot);
        let _lhs = lhs_val.clone();
        let rhs_val = lower_expr(ctx, rhs)?;
        let blk = ctx.block();
        // Issue #214: SSO-safe unbox.
        let l_handle = unbox_str_handle(blk, &lhs_val);
        // Coerce non-string RHS to a string handle.
        let r_handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &rhs_val)]);
        let result = blk.call(
            I64,
            "js_string_append",
            &[(I64, &l_handle), (I64, &r_handle)],
        );
        let new_box = nanbox_string_inline(blk, &result);
        blk.store(DOUBLE, &new_box, &slot);
        return Ok(new_box);
    }

    let rhs_box = lower_expr(ctx, rhs)?;
    let blk = ctx.block();
    let lhs_box = blk.load(DOUBLE, &slot);
    // Issue #214: SSO-safe unbox.
    let l_handle = unbox_str_handle(blk, &lhs_box);
    let r_handle = unbox_str_handle(blk, &rhs_box);
    let new_handle = blk.call(
        I64,
        "js_string_append",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    let new_box = nanbox_string_inline(blk, &new_handle);
    blk.store(DOUBLE, &new_box, &slot);
    Ok(new_box)
}

/// Lower `string + non_string` (or vice versa) concat with runtime
/// coercion of the non-string side. The non-string operand passes through
/// `js_jsvalue_to_string` which inspects its NaN tag and produces the
/// canonical JS string form (numbers via the formatter at
/// `crates/perry-runtime/src/value.rs:825`, booleans → `"true"`/`"false"`,
/// objects → `"[object Object]"`, etc.).
///
/// The string-typed side still uses the fast inline `bitcast double → i64;
/// and POINTER_MASK_I64` unbox; only the non-string side pays the function
/// call. Both operand handles then feed `js_string_concat`.
pub(crate) fn lower_string_coerce_concat(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    l_is_string: bool,
    r_is_string: bool,
) -> Result<String> {
    let l_box = lower_expr(ctx, left)?;
    let r_box = lower_expr(ctx, right)?;
    let blk = ctx.block();

    // Issue #58: fused string+value concat — when one side is a string
    // and the other is not, use the fused runtime call that collapses
    // js_jsvalue_to_string + js_string_concat into a single allocation
    // for number operands (the common `"item_" + i` pattern).
    if l_is_string && !r_is_string {
        // Issue #214: SSO-safe unbox — see lower_string_concat.
        let l_handle = unbox_str_handle(blk, &l_box);
        let result_handle = blk.call(
            I64,
            "js_string_concat_value",
            &[(I64, &l_handle), (DOUBLE, &r_box)],
        );
        return Ok(nanbox_string_inline(blk, &result_handle));
    }

    if !l_is_string && r_is_string {
        // Issue #214: SSO-safe unbox — see lower_string_concat.
        let r_handle = unbox_str_handle(blk, &r_box);
        let result_handle = blk.call(
            I64,
            "js_value_concat_string",
            &[(DOUBLE, &l_box), (I64, &r_handle)],
        );
        return Ok(nanbox_string_inline(blk, &result_handle));
    }

    // Both non-string (shouldn't normally reach here) — fall back to
    // the generic path.
    let l_handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &l_box)]);
    let r_handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &r_box)]);

    let result_handle = blk.call(
        I64,
        "js_string_concat",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    Ok(nanbox_string_inline(blk, &result_handle))
}

/// Lower a static `s1 + s2` string concatenation. Both operands must
/// already be statically string-typed (caller's responsibility — see
/// `is_string_expr`).
///
/// Pattern:
/// ```llvm
/// ; %l_box and %r_box are NaN-boxed strings (double values with STRING_TAG)
/// %l_bits = bitcast double %l_box to i64
/// %l_handle = and i64 %l_bits, 281474976710655   ; POINTER_MASK_I64
/// %r_bits = bitcast double %r_box to i64
/// %r_handle = and i64 %r_bits, 281474976710655
/// %result_handle = call i64 @js_string_concat(i64 %l_handle, i64 %r_handle)
/// %result_box = call double @js_nanbox_string(i64 %result_handle)
/// ```
///
/// The bitcast+and is the inline-fast unboxing pattern. We avoid calling
/// the slower `js_nanbox_get_pointer` (which does the same thing in Rust)
/// to keep concat hot-path overhead minimal.
pub(crate) fn lower_string_concat(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    let l_box = lower_expr(ctx, left)?;
    let r_box = lower_expr(ctx, right)?;
    let blk = ctx.block();
    // SSO-aware fast path: pass operands as NaN-boxed f64s directly to
    // `js_string_concat_sso`, which keeps SSO operands inline (no
    // materialise-to-heap defeat) and returns the result NaN-boxed —
    // SSO when the total fits 5 bytes, heap-pointer otherwise. Saves up
    // to 3 heap allocations per concat on hot paths like ABC451D's
    // recursive `before + after` (1.4M concats with 1-9 byte operands).
    Ok(blk.call(
        DOUBLE,
        "js_string_concat_box",
        &[(DOUBLE, &l_box), (DOUBLE, &r_box)],
    ))
}

/// Cap the per-call part count for the n-way fold. Must match the
/// runtime's `MAX_PARTS` in `js_string_concat_chain`. 32 covers every
/// realistic CSV / log-line / template chain in user code.
const CONCAT_CHAIN_MAX_PARTS: usize = 32;

/// Try to flatten a left-spine of `Binary { Add }` nodes where every Add
/// has at least one statically-string operand. Returns the parts in
/// left-to-right (source-order) order. Returns `None` if the chain is
/// shorter than the existing pairwise fast path's preference, has too
/// many parts, or contains an Add node where neither side is statically
/// string (which would risk numeric semantics under JS spec).
///
/// Caller passes the OUTERMOST Add's children. If the outermost Add's
/// left child is itself a string-shaped Add, we recurse into it; right
/// children are always leaves in our flat representation.
pub(crate) fn flatten_string_add_chain<'a>(
    ctx: &FnCtx<'_>,
    left: &'a Expr,
    right: &'a Expr,
) -> Option<Vec<&'a Expr>> {
    use perry_hir::BinaryOp;

    let mut parts: Vec<&Expr> = Vec::with_capacity(8);
    parts.push(right);

    // Walk down the left spine. At each step, the current `cur` was the
    // left child of an Add we already accepted — so we know `cur + ...`
    // is string-shaped at the level above. We need each Add we descend
    // INTO to itself be string-shaped (≥1 statically-string operand), so
    // the entire chain has unambiguous string semantics.
    let mut cur: &Expr = left;
    loop {
        match cur {
            Expr::Binary {
                op: BinaryOp::Add,
                left: l,
                right: r,
            } => {
                let l_str = crate::type_analysis::is_definitely_string_expr(ctx, l);
                let r_str = crate::type_analysis::is_definitely_string_expr(ctx, r);
                if !l_str && !r_str {
                    // Stop the descent — this Add isn't unambiguously
                    // string-shaped. Treat the entire `cur` subtree as
                    // one opaque part.
                    parts.push(cur);
                    break;
                }
                parts.push(r);
                cur = l;
                if parts.len() >= CONCAT_CHAIN_MAX_PARTS {
                    return None;
                }
            }
            _ => {
                parts.push(cur);
                break;
            }
        }
    }

    parts.reverse();
    Some(parts)
}

/// Lower a flat parts list to a single `js_string_concat_chain` call.
/// Each part is lowered to its NaN-boxed value, then stored into a
/// stack-allocated `[CONCAT_CHAIN_MAX_PARTS x double]` buffer; we pass
/// the base pointer + N to the runtime helper, which produces a single
/// allocation containing the entire concatenated result.
///
/// The buffer is fixed-size (always sized to MAX_PARTS) and hoisted to
/// the function entry block via `alloca_entry_array`. A non-entry-block
/// alloca lowers to a runtime `sub %rsp, N` with no matching restore;
/// inside a loop body that's a stack leak (issue #167 — same shape that
/// blew up `buf.readInt32BE` in tight loops). Function-entry allocas
/// run once at prologue and the slot dominates every reachable use.
/// One per-function buffer is shared across all chain call sites — fine
/// because each chain call writes its parts and immediately calls into
/// the runtime helper before any other call site can clobber the slots.
pub(crate) fn lower_string_concat_chain(ctx: &mut FnCtx<'_>, parts: &[&Expr]) -> Result<String> {
    debug_assert!(parts.len() >= 2);
    debug_assert!(parts.len() <= CONCAT_CHAIN_MAX_PARTS);

    // Lower each part first (in source order); side effects must fire
    // left-to-right per JS spec.
    let mut lowered: Vec<String> = Vec::with_capacity(parts.len());
    for p in parts {
        lowered.push(lower_expr(ctx, p)?);
    }

    let n = lowered.len();
    // Hoist the buffer to the function entry block. Issue #167.
    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, CONCAT_CHAIN_MAX_PARTS);
    let blk = ctx.block();
    for (i, val) in lowered.iter().enumerate() {
        let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
        blk.store(DOUBLE, val, &slot);
    }
    // Pass the array's base pointer as i64 (codegen ABI uses i64 for
    // raw pointer args matching the existing `js_string_concat` shape).
    let base_i64 = blk.next_reg();
    blk.emit_raw(format!("{} = ptrtoint ptr {} to i64", base_i64, buf_reg));

    let result_handle = blk.call(
        I64,
        "js_string_concat_chain",
        &[(I64, &base_i64), (I32, &format!("{}", n))],
    );
    Ok(nanbox_string_inline(blk, &result_handle))
}
