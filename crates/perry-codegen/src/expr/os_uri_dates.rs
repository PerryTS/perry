//! OsVersion..DateSetTime (OS/URI/date getters and setters).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

use super::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};

/// Field selector codes for `js_date_apply_setter`. Must match the runtime
/// (`crates/perry-runtime/src/date.rs`): 0=FullYear 1=Month 2=Date 3=Hours
/// 4=Minutes 5=Seconds 6=Milliseconds 7=Time.
pub(crate) const DATE_FIELD_FULL_YEAR: i32 = 0;
pub(crate) const DATE_FIELD_MONTH: i32 = 1;
pub(crate) const DATE_FIELD_DATE: i32 = 2;
pub(crate) const DATE_FIELD_HOURS: i32 = 3;
pub(crate) const DATE_FIELD_MINUTES: i32 = 4;
pub(crate) const DATE_FIELD_SECONDS: i32 = 5;
pub(crate) const DATE_FIELD_MILLISECONDS: i32 = 6;
pub(crate) const DATE_FIELD_TIME: i32 = 7;

/// Lower a `Date.prototype.set*` call (#2851). Builds a stack buffer of the
/// NaN-boxed argument values and calls the unified runtime entry point
/// `js_date_apply_setter(date, is_utc, field, args_ptr, argc)`, which applies
/// every supplied component (and the omitted-trailing / leading-undefined /
/// NaN-propagation rules). The receiver `DateCell` is mutated in place;
/// returns the numeric ms.
pub(crate) fn lower_date_setter(
    ctx: &mut FnCtx<'_>,
    date: &Expr,
    args: &[Expr],
    is_utc: bool,
    field: i32,
) -> Result<String> {
    let d = lower_expr(ctx, date)?;
    let mut arg_vals: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        arg_vals.push(lower_expr(ctx, a)?);
    }
    let blk = ctx.block();
    let (args_ptr, argc) = if arg_vals.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = arg_vals.len();
        let buf_reg = blk.next_reg();
        blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
        for (i, val) in arg_vals.iter().enumerate() {
            let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            blk.store(DOUBLE, val, &slot);
        }
        (buf_reg, format!("{}", n))
    };
    let is_utc_str = if is_utc { "1" } else { "0" };
    let field_str = format!("{}", field);
    Ok(blk.call(
        DOUBLE,
        "js_date_apply_setter",
        &[
            (DOUBLE, &d),
            (I32, is_utc_str),
            (I32, &field_str),
            (PTR, &args_ptr),
            (I32, &argc),
        ],
    ))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::OsVersion => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_version", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::ProcessMemoryUsage => {
            // Runtime returns an already NaN-boxed pointer (f64).
            Ok(ctx.block().call(DOUBLE, "js_process_memory_usage", &[]))
        }
        Expr::ProcessThreadCpuUsage(prior) => {
            // Runtime returns an already NaN-boxed pointer (f64) for
            // { user, system }.
            let prior_val = if let Some(e) = prior {
                lower_expr(ctx, e)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_process_thread_cpu_usage",
                &[(DOUBLE, &prior_val)],
            ))
        }
        Expr::ProcessAvailableMemory => {
            Ok(ctx.block().call(DOUBLE, "js_process_available_memory", &[]))
        }
        Expr::ProcessConstrainedMemory => {
            Ok(ctx
                .block()
                .call(DOUBLE, "js_process_constrained_memory", &[]))
        }
        Expr::ProcessPosixCredential(kind) => {
            let fn_name = match kind {
                perry_hir::PosixCredentialKind::Uid => "js_process_getuid",
                perry_hir::PosixCredentialKind::Euid => "js_process_geteuid",
                perry_hir::PosixCredentialKind::Gid => "js_process_getgid",
                perry_hir::PosixCredentialKind::Egid => "js_process_getegid",
            };
            Ok(ctx.block().call(DOUBLE, fn_name, &[]))
        }
        Expr::ProcessEmitWarning(args) => {
            // First three positional args (warning, type, code). Missing
            // slots are passed as TAG_UNDEFINED so the runtime can detect
            // and skip them.
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let warning = if let Some(e) = args.first() {
                lower_expr(ctx, e)?
            } else {
                undef.clone()
            };
            let type_v = if let Some(e) = args.get(1) {
                lower_expr(ctx, e)?
            } else {
                undef.clone()
            };
            let code_v = if let Some(e) = args.get(2) {
                lower_expr(ctx, e)?
            } else {
                undef.clone()
            };
            ctx.block().call_void(
                "js_process_emit_warning",
                &[(DOUBLE, &warning), (DOUBLE, &type_v), (DOUBLE, &code_v)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ProcessCpuUsage(prior) => {
            let prior_val = if let Some(e) = prior {
                lower_expr(ctx, e)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx
                .block()
                .call(DOUBLE, "js_process_cpu_usage", &[(DOUBLE, &prior_val)]))
        }
        Expr::ProcessResourceUsage => {
            Ok(ctx.block().call(DOUBLE, "js_process_resource_usage", &[]))
        }
        Expr::ProcessActiveResourcesInfo => {
            Ok(ctx
                .block()
                .call(DOUBLE, "js_process_active_resources_info", &[]))
        }
        Expr::EncodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::EncodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DateToString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToUTCString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_utc_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToJSON(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_json", &[(DOUBLE, &v)]);
            let is_null = blk.icmp_eq(I64, &handle, "0");
            let as_string = nanbox_string_inline(blk, &handle);
            let str_bits = blk.bitcast_double_to_i64(&as_string);
            let selected = blk.select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &str_bits);
            Ok(blk.bitcast_i64_to_double(&selected))
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let val_d = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_with",
                &[(I64, &arr_handle), (DOUBLE, &idx_d), (DOUBLE, &val_d)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayReverseValue { receiver } => {
            let receiver_d = lower_expr(ctx, receiver)?;
            let blk = ctx.block();
            let result = blk.call(DOUBLE, "js_array_reverse_value", &[(DOUBLE, &receiver_d)]);
            Ok(result)
        }
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let target_d = lower_expr(ctx, target)?;
            let start_d = lower_expr(ctx, start)?;
            let (has_end_str, end_d) = if let Some(e) = end {
                let v = lower_expr(ctx, e)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0.0".to_string())
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_copy_within",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &target_d),
                    (DOUBLE, &start_d),
                    (I32, &has_end_str),
                    (DOUBLE, &end_d),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            let receiver_box = lower_expr(ctx, receiver)?;
            let target_d = lower_expr(ctx, target)?;
            let start_d = lower_expr(ctx, start)?;
            let (has_end_str, end_d) = if let Some(e) = end {
                let v = lower_expr(ctx, e)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0.0".to_string())
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_array_copy_within_value",
                &[
                    (DOUBLE, &receiver_box),
                    (DOUBLE, &target_d),
                    (DOUBLE, &start_d),
                    (I32, &has_end_str),
                    (DOUBLE, &end_d),
                ],
            ))
        }
        Expr::ArrayToReversed { array } => {
            let arr_box = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_to_reversed", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayToSorted { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let result = if let Some(c) = comparator {
                let cmp_box = lower_expr(ctx, c)?;
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                // #2796: validate comparator (function | undefined) before sorting.
                let cmp_handle =
                    blk.call(I64, "js_validate_array_comparator", &[(DOUBLE, &cmp_box)]);
                blk.call(
                    I64,
                    "js_array_to_sorted_with_comparator",
                    &[(I64, &arr_handle), (I64, &cmp_handle)],
                )
            } else {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                blk.call(I64, "js_array_to_sorted_default", &[(I64, &arr_handle)])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let start_d = lower_expr(ctx, start)?;
            let count_d = lower_expr(ctx, delete_count)?;

            // Lower items to a Vec of f64 expressions
            let mut item_vals: Vec<String> = Vec::new();
            for it in items {
                item_vals.push(lower_expr(ctx, it)?);
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);

            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = item_vals.len();
                let items_count_str = format!("{}", n);
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, items_count_str)
            };

            let result = blk.call(
                I64,
                "js_array_to_spliced",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &start_d),
                    (DOUBLE, &count_d),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayAt { array, index } => {
            // arr.at(i) — negative index counts from the end. The
            // runtime handles the negative-index adjustment +
            // bounds clamp.
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(
                DOUBLE,
                "js_array_at",
                &[(I64, &arr_handle), (DOUBLE, &idx_d)],
            ))
        }
        Expr::DateSetUtcMinutes { date, args } => {
            lower_date_setter(ctx, date, args, true, DATE_FIELD_MINUTES)
        }
        Expr::DateSetUtcSeconds { date, args } => {
            lower_date_setter(ctx, date, args, true, DATE_FIELD_SECONDS)
        }
        Expr::DateSetUtcMilliseconds { date, args } => {
            lower_date_setter(ctx, date, args, true, DATE_FIELD_MILLISECONDS)
        }
        Expr::Yield { value, .. } => {
            // Generators not implemented; lower the yielded value for
            // side effects and return undefined.
            if let Some(v) = value {
                let _ = lower_expr(ctx, v)?;
            }
            Ok(double_literal(0.0))
        }
        // Each Error subclass gets its own runtime constructor so the
        // ErrorHeader's `error_kind` field is set to the right
        // ERROR_KIND_* — required for `e instanceof TypeError` etc. to
        // walk the ErrorHeader discriminant in `js_instanceof`.
        Expr::TypeErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let err_handle = blk.call(
                I64,
                "js_error_new_kind_from_value",
                &[(I32, "1"), (DOUBLE, &m)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::RangeErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let err_handle = blk.call(
                I64,
                "js_error_new_kind_from_value",
                &[(I32, "2"), (DOUBLE, &m)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::SyntaxErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let err_handle = blk.call(
                I64,
                "js_error_new_kind_from_value",
                &[(I32, "4"), (DOUBLE, &m)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::ReferenceErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let err_handle = blk.call(
                I64,
                "js_error_new_kind_from_value",
                &[(I32, "3"), (DOUBLE, &m)],
            );
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::NumberIsSafeInteger(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_safe_integer", &[(DOUBLE, &v)]))
        }
        Expr::ObjectFreeze(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_freeze", &[(DOUBLE, &v)]))
        }
        Expr::ObjectSeal(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_seal", &[(DOUBLE, &v)]))
        }
        Expr::ObjectPreventExtensions(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_prevent_extensions", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcMonth { date, args } => {
            lower_date_setter(ctx, date, args, true, DATE_FIELD_MONTH)
        }
        // Local-time Date setters (#1187 / #2851). All route through the
        // unified `js_date_apply_setter` runtime entry point.
        Expr::DateSetFullYear { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_FULL_YEAR)
        }
        Expr::DateSetMonth { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_MONTH)
        }
        Expr::DateSetDate { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_DATE)
        }
        Expr::DateSetHours { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_HOURS)
        }
        Expr::DateSetMinutes { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_MINUTES)
        }
        Expr::DateSetSeconds { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_SECONDS)
        }
        Expr::DateSetMilliseconds { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_MILLISECONDS)
        }
        Expr::DateSetTime { date, args } => {
            lower_date_setter(ctx, date, args, false, DATE_FIELD_TIME)
        }
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
