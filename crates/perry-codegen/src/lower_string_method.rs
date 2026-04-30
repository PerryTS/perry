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
use crate::types::{DOUBLE, I1, I32, I64};

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
    let recv_box = lower_expr(ctx, object)?;

    match property {
        "indexOf" => {
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.indexOf expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            // Optional fromIndex.
            let from_idx_double = if args.len() == 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = unbox_str_handle(blk, &needle_box);
            let result_i32 = if let Some(from_d) = from_idx_double {
                let from_i32 = blk.fptosi(DOUBLE, &from_d, I32);
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
            // 2-arg form: explicit end. 0/1-arg form: end defaults to the
            // string's length, computed inline (load i32 at offset 0).
            let end_d = if args.len() == 2 {
                lower_expr(ctx, &args[1])?
            } else {
                // Issue #214: route through SSO-safe unbox so the
                // length read works for SHORT_STRING_TAG receivers.
                // The inline `bits & POINTER_MASK_I64` would treat
                // the SSO payload as a heap pointer.
                let blk = ctx.block();
                let recv_handle = unbox_str_handle(blk, &recv_box);
                let len_ptr = blk.inttoptr(I64, &recv_handle);
                let len_i32 = blk.load(I32, &len_ptr);
                blk.sitofp(I32, &len_i32, DOUBLE)
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_d, I32);
            let end_i32 = blk.fptosi(DOUBLE, &end_d, I32);
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
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.split expects 1 arg (delimiter), got {}",
                    args.len()
                );
            }
            // NOTE: we always call js_string_split here, even for regex
            // delimiters — the runtime will detect regex pointers via
            // their GC header and delegate to js_string_split_regex
            // internally. This avoids needing a new LLVM runtime decl.
            let delim_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let delim_handle = unbox_str_handle(blk, &delim_box);
            let result_arr = blk.call(
                I64,
                "js_string_split",
                &[(I64, &recv_handle), (I64, &delim_handle)],
            );
            // Returns an array pointer (ArrayHeader*) — NaN-box with POINTER_TAG.
            Ok(crate::expr::nanbox_pointer_inline(blk, &result_arr))
        }
        // Unary string-returning methods (no args).
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" => {
            if !args.is_empty() {
                bail!(
                    "perry-codegen: String.{} takes no args, got {}",
                    property,
                    args.len()
                );
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
        "charAt" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.charAt expects 1 arg, got {}",
                    args.len()
                );
            }
            let idx_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
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
            let count_i32 = blk.fptosi(DOUBLE, &count_d, I32);
            let result = blk.call(
                I64,
                "js_string_repeat",
                &[(I64, &recv_handle), (I32, &count_i32)],
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
            let needle_box = lower_expr(ctx, &args[0])?;
            let repl_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = unbox_str_handle(blk, &needle_box);
            if needle_is_regex && repl_is_function {
                // repl_box is a NaN-boxed closure pointer (double).
                // js_string_replace_regex_fn takes the callback as f64.
                let result = blk.call(
                    I64,
                    "js_string_replace_regex_fn",
                    &[
                        (I64, &recv_handle),
                        (I64, &needle_handle),
                        (DOUBLE, &repl_box),
                    ],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            // Issue #214: SSO-safe unbox of replacement string.
            let repl_handle = unbox_str_handle(blk, &repl_box);
            let runtime_fn = if needle_is_regex {
                if repl_has_named {
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
            if args.len() != 1 {
                bail!("perry-codegen: String.at expects 1 arg, got {}", args.len());
            }
            let idx_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            // js_string_at returns a NaN-boxed string or undefined directly.
            Ok(blk.call(
                DOUBLE,
                "js_string_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "codePointAt" => {
            if args.is_empty() || args.len() > 1 {
                bail!(
                    "perry-codegen: String.codePointAt expects 1 arg, got {}",
                    args.len()
                );
            }
            let idx_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            // Returns NaN-boxed number or undefined directly.
            Ok(blk.call(
                DOUBLE,
                "js_string_code_point_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "charCodeAt" => {
            if args.is_empty() || args.len() > 1 {
                bail!(
                    "perry-codegen: String.charCodeAt expects 1 arg, got {}",
                    args.len()
                );
            }
            let idx_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            // js_string_char_code_at returns a plain f64 (NaN for OOB).
            Ok(blk.call(
                DOUBLE,
                "js_string_char_code_at",
                &[(I64, &recv_handle), (I32, &idx_i32)],
            ))
        }
        "lastIndexOf" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.lastIndexOf expects 1 arg, got {}",
                    args.len()
                );
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = unbox_str_handle(blk, &needle_box);
            let i32_v = blk.call(
                I32,
                "js_string_last_index_of",
                &[(I64, &recv_handle), (I64, &needle_handle)],
            );
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
            // Optional pad string; defaults to " " when missing.
            let pad_handle = if args.len() == 2 {
                let pad_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                unbox_str_handle(blk, &pad_box)
            } else {
                let sp_idx = ctx.strings.intern(" ");
                let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                let blk = ctx.block();
                let sp_box = blk.load(DOUBLE, &sp_global);
                unbox_str_handle(blk, &sp_box)
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let len_i32 = blk.fptosi(DOUBLE, &len_d, I32);
            let runtime_fn = if property == "padStart" {
                "js_string_pad_start"
            } else {
                "js_string_pad_end"
            };
            let result = blk.call(
                I64,
                runtime_fn,
                &[(I64, &recv_handle), (I32, &len_i32), (I64, &pad_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "normalize" => {
            // 0 or 1 string arg. Empty arg → default ("NFC" handled by
            // the runtime when form is null).
            if args.len() > 1 {
                bail!(
                    "perry-codegen: String.normalize expects 0 or 1 args, got {}",
                    args.len()
                );
            }
            let form_handle = if args.is_empty() {
                "0".to_string()
            } else {
                let form_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                unbox_str_handle(blk, &form_box)
            };
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_string_normalize",
                &[(I64, &recv_handle), (I64, &form_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        "localeCompare" => {
            if args.is_empty() || args.len() > 3 {
                bail!(
                    "perry-codegen: String.localeCompare expects 1-3 args, got {}",
                    args.len()
                );
            }
            let other_box = lower_expr(ctx, &args[0])?;
            // Ignore optional locale/options args.
            for extra in args.iter().skip(1) {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let other_handle = unbox_str_handle(blk, &other_box);
            // Returns a plain f64 (-1/0/1) — NOT NaN-tagged.
            Ok(blk.call(
                DOUBLE,
                "js_string_locale_compare",
                &[(I64, &recv_handle), (I64, &other_handle)],
            ))
        }
        "search" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.search expects 1 arg, got {}",
                    args.len()
                );
            }
            // The arg is a regex (literal or local).
            let re_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let re_handle = unbox_str_handle(blk, &re_box);
            let i32_v = blk.call(
                I32,
                "js_string_search_regex",
                &[(I64, &recv_handle), (I64, &re_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "match" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.match expects 1 arg, got {}",
                    args.len()
                );
            }
            let re_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let re_handle = unbox_str_handle(blk, &re_box);
            let result = blk.call(
                I64,
                "js_string_match",
                &[(I64, &recv_handle), (I64, &re_handle)],
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
            if args.len() != 1 {
                bail!(
                    "perry-codegen: String.matchAll expects 1 arg, got {}",
                    args.len()
                );
            }
            let re_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let re_handle = unbox_str_handle(blk, &re_box);
            let result = blk.call(
                I64,
                "js_string_match_all",
                &[(I64, &recv_handle), (I64, &re_handle)],
            );
            // matchAll always returns an array (possibly empty), never null.
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "isWellFormed" => {
            if !args.is_empty() {
                bail!(
                    "perry-codegen: String.isWellFormed takes no args, got {}",
                    args.len()
                );
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            // Returns a NaN-tagged boolean directly.
            Ok(blk.call(DOUBLE, "js_string_is_well_formed", &[(I64, &recv_handle)]))
        }
        "toWellFormed" => {
            if !args.is_empty() {
                bail!(
                    "perry-codegen: String.toWellFormed takes no args, got {}",
                    args.len()
                );
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let result = blk.call(I64, "js_string_to_well_formed", &[(I64, &recv_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        "concat" => {
            // str.concat(s1, s2, …) = str + s1 + s2 + … . Sequential
            // js_string_concat calls; each op returns a new handle.
            let blk = ctx.block();
            let mut acc_handle = unbox_str_handle(blk, &recv_box);
            for a in args {
                let s_box = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let s_handle = unbox_str_handle(blk, &s_box);
                acc_handle = blk.call(
                    I64,
                    "js_string_concat",
                    &[(I64, &acc_handle), (I64, &s_handle)],
                );
            }
            Ok(nanbox_string_inline(ctx.block(), &acc_handle))
        }
        "substr" => {
            // Legacy substr(start, length) — map to slice(start, start+length).
            // Without a dedicated runtime helper we approximate with substring.
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.substr expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            let start_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let start_i32 = blk.fptosi(DOUBLE, &start_d, I32);
            let end_i32 = if args.len() == 2 {
                let len_d = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let len_i32 = blk.fptosi(DOUBLE, &len_d, I32);
                blk.add(I32, &start_i32, &len_i32)
            } else {
                // Default end = receiver length.
                let blk = ctx.block();
                let len_ptr = blk.inttoptr(I64, &recv_handle);
                blk.load(I32, &len_ptr)
            };
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_string_substring",
                &[(I64, &recv_handle), (I32, &start_i32), (I32, &end_i32)],
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
            let other_handle = unbox_str_handle(blk, &other_box);
            let result_i32 = if let Some(pos_d) = pos_d {
                let pos_i32 = blk.fptosi(DOUBLE, &pos_d, I32);
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
            // str.includes(sub) -> boolean. Implemented as
            // js_string_index_of(str, sub) != -1, then NaN-tagged.
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: String.includes expects 1 or 2 args, got {}",
                    args.len()
                );
            }
            let needle_box = lower_expr(ctx, &args[0])?;
            // Optional fromIndex param is ignored for the boolean form.
            if args.len() == 2 {
                let _ = lower_expr(ctx, &args[1])?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_str_handle(blk, &recv_box);
            let needle_handle = unbox_str_handle(blk, &needle_box);
            let idx_i32 = blk.call(
                I32,
                "js_string_index_of",
                &[(I64, &recv_handle), (I64, &needle_handle)],
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
        // Best-effort fallback: lower args for side effects, return
        // the receiver string. Compile succeeds; runtime gets the
        // pre-method-call value.
        _ => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(recv_box)
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
    // Issue #214: inline `bitcast + and POINTER_MASK_I64` returns garbage
    // for SSO operands (SHORT_STRING_TAG = 0x7FF9, lower 48 bits encode
    // payload, not a pointer). Route through `unbox_str_handle` →
    // `js_get_string_pointer_unified` which materializes SSO to heap.
    let l_handle = unbox_str_handle(blk, &l_box);
    let r_handle = unbox_str_handle(blk, &r_box);
    let result_handle = blk.call(
        I64,
        "js_string_concat",
        &[(I64, &l_handle), (I64, &r_handle)],
    );
    // Inline NaN-box (STRING_TAG) — concat always returns a real heap ptr.
    Ok(nanbox_string_inline(blk, &result_handle))
}
