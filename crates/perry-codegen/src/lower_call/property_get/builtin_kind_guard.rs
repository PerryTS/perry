//! Receiver-kind guards for Date / Number / Array builtin method names (#10476).
//!
//! A method NAME is not proof of its receiver's kind. dayjs and moment own
//! `toISOString`/`getTime`, decimal.js and bignumber.js own `toFixed`, and any
//! class may define `setHours`, `toPrecision` or `toSorted`. Lowering such a
//! call straight to `js_date_*` / `js_number_to_*` / `js_array_*` ran the
//! builtin on the user's object (`"NaN"`, `Invalid time value`,
//! `[object Object]`, `[]`) and never called the user's method.
//!
//! The direct builtin call is kept for a receiver the compiler has proven to be
//! a Date / number. Any other receiver without a known class is evaluated once
//! with its arguments, then its runtime kind selects the builtin or the
//! universal method dispatcher, which finds an own or inherited user method and
//! still reaches the builtin through the prototype for a Date, a number or an
//! array. Known class receivers are left to the class dispatch tower.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::rooting::{any_operand_may_collect, open_rooted_group, Repr};
use crate::type_analysis::{
    is_array_expr, is_native_module_dynamic_index, is_numeric_expr, is_string_expr,
    receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::is_date_receiver;

/// A Date method with a direct runtime entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateBuiltin {
    /// `double fn(double time)`. Accepts a Date or its time value.
    Getter(&'static str),
    /// `StringHeader* fn(double time)`. Accepts a Date or its time value.
    Formatter(&'static str),
    /// `double js_value_to_locale_string(date)`, the proven-Date
    /// `Expr::DateToLocaleString` entry point. It dispatches on the value's
    /// tag, so it needs the Date itself, never its time value.
    LocaleString,
    /// `double js_date_apply_setter(date, is_utc, field, args, argc)`.
    Setter { is_utc: bool, field: i32 },
}

/// The receiver kind a guarded builtin requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiverKind {
    Date,
    /// `toLocaleString()`: any primitive, Date or Symbol (see
    /// [`emit_receiver_kind_branch`]).
    LocaleValue,
    Number,
    Array,
}

/// The runtime entry point the HIR `Expr::Date*` arms lower each name to.
/// `toLocale*String` only has a fast path without locale/options arguments;
/// with arguments they belong to the generic dispatcher's Intl thunks.
fn date_builtin(property: &str, argc: usize) -> Option<DateBuiltin> {
    use crate::expr::os_uri_dates::{
        DATE_FIELD_DATE, DATE_FIELD_FULL_YEAR, DATE_FIELD_HOURS, DATE_FIELD_MILLISECONDS,
        DATE_FIELD_MINUTES, DATE_FIELD_MONTH, DATE_FIELD_SECONDS, DATE_FIELD_TIME,
    };
    let getter = DateBuiltin::Getter;
    let formatter = DateBuiltin::Formatter;
    let local = |field| DateBuiltin::Setter {
        is_utc: false,
        field,
    };
    let utc = |field| DateBuiltin::Setter {
        is_utc: true,
        field,
    };
    Some(match property {
        "getTime" => getter("js_date_get_time"),
        "getTimezoneOffset" => getter("js_date_get_timezone_offset"),
        "getFullYear" => getter("js_date_get_full_year"),
        "getMonth" => getter("js_date_get_month"),
        "getDate" => getter("js_date_get_date"),
        "getDay" => getter("js_date_get_day"),
        "getHours" => getter("js_date_get_hours"),
        "getMinutes" => getter("js_date_get_minutes"),
        "getSeconds" => getter("js_date_get_seconds"),
        "getMilliseconds" => getter("js_date_get_milliseconds"),
        "getUTCFullYear" => getter("js_date_get_utc_full_year"),
        "getUTCMonth" => getter("js_date_get_utc_month"),
        "getUTCDate" => getter("js_date_get_utc_date"),
        "getUTCDay" => getter("js_date_get_utc_day"),
        "getUTCHours" => getter("js_date_get_utc_hours"),
        "getUTCMinutes" => getter("js_date_get_utc_minutes"),
        "getUTCSeconds" => getter("js_date_get_utc_seconds"),
        "getUTCMilliseconds" => getter("js_date_get_utc_milliseconds"),
        "toISOString" => formatter("js_date_to_iso_string_or_throw"),
        "toDateString" => formatter("js_date_to_date_string"),
        "toTimeString" => formatter("js_date_to_time_string"),
        "toUTCString" | "toGMTString" => formatter("js_date_to_utc_string"),
        "toLocaleDateString" if argc == 0 => formatter("js_date_to_locale_date_string"),
        "toLocaleTimeString" if argc == 0 => formatter("js_date_to_locale_time_string"),
        "toLocaleString" if argc == 0 => DateBuiltin::LocaleString,
        "setFullYear" => local(DATE_FIELD_FULL_YEAR),
        "setMonth" => local(DATE_FIELD_MONTH),
        "setDate" => local(DATE_FIELD_DATE),
        "setHours" => local(DATE_FIELD_HOURS),
        "setMinutes" => local(DATE_FIELD_MINUTES),
        "setSeconds" => local(DATE_FIELD_SECONDS),
        "setMilliseconds" => local(DATE_FIELD_MILLISECONDS),
        "setTime" => local(DATE_FIELD_TIME),
        "setUTCFullYear" => utc(DATE_FIELD_FULL_YEAR),
        "setUTCMonth" => utc(DATE_FIELD_MONTH),
        "setUTCDate" => utc(DATE_FIELD_DATE),
        "setUTCHours" => utc(DATE_FIELD_HOURS),
        "setUTCMinutes" => utc(DATE_FIELD_MINUTES),
        "setUTCSeconds" => utc(DATE_FIELD_SECONDS),
        "setUTCMilliseconds" => utc(DATE_FIELD_MILLISECONDS),
        _ => return None,
    })
}

/// `StringHeader* fn(double number, double arg)` for each Number method.
fn number_builtin(property: &str) -> Option<&'static str> {
    match property {
        "toFixed" => Some("js_number_to_fixed"),
        "toPrecision" => Some("js_number_to_precision"),
        "toExponential" => Some("js_number_to_exponential"),
        _ => None,
    }
}

/// An unproven receiver that neither the String/Array lowering nor the class
/// dispatch tower owns. Module objects keep their own member dispatch.
///
/// When any compiled class defines `property`, the receiver may be one of its
/// instances — including a `class X extends Date` override, whose instance is a
/// Date at runtime and would pass the kind check — so the class dispatch tower
/// keeps the call; its fallback is the same universal method dispatch.
fn receiver_is_unproven(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && receiver_class_name(ctx, object).is_none()
        && !matches!(object, Expr::GlobalGet(_) | Expr::NativeModuleRef(_))
        && !is_native_module_dynamic_index(object)
        && !ctx.methods.keys().any(|(_, method)| method == property)
}

/// Lower a Date, Number or Array builtin-named method call whose receiver was
/// not claimed by a proven fast path. Returns `Ok(None)` for other names and
/// for receivers the class dispatch tower owns.
pub(super) fn try_lower_kind_guarded_builtin_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    if let Some(date) = date_builtin(property, args.len()) {
        let unproven_kind = if date == DateBuiltin::LocaleString {
            ReceiverKind::LocaleValue
        } else {
            ReceiverKind::Date
        };
        let guard = if is_date_receiver(ctx, object) {
            None
        } else if receiver_is_unproven(ctx, object, property) {
            Some(unproven_kind)
        } else {
            return Ok(None);
        };
        return guarded_call(
            ctx,
            object,
            property,
            args,
            call_byte_offset,
            guard,
            |ctx, recv, time, arg_vals| emit_date_builtin(ctx, date, recv, time, arg_vals),
        )
        .map(Some);
    }
    if let Some(runtime_fn) = number_builtin(property) {
        // A proven number keeps number_string.rs's direct lowering.
        if receiver_is_unproven(ctx, object, property) && !is_numeric_expr(ctx, object) {
            return guarded_call(
                ctx,
                object,
                property,
                args,
                call_byte_offset,
                Some(ReceiverKind::Number),
                |ctx, recv, _, arg_vals| emit_number_builtin(ctx, runtime_fn, recv, arg_vals),
            )
            .map(Some);
        }
    }
    // A proven array keeps `lower_array_method`, and HIR folds most.
    if crate::lower_array_method::is_array_method_on_values(property, args.len())
        && receiver_is_unproven(ctx, object, property)
    {
        return guarded_call(
            ctx,
            object,
            property,
            args,
            call_byte_offset,
            Some(ReceiverKind::Array),
            |ctx, recv, _, arg_vals| {
                crate::lower_array_method::emit_array_method_on_values(
                    ctx, property, recv, arg_vals,
                )
            },
        )
        .map(Some);
    }
    Ok(None)
}

fn emit_number_builtin(
    ctx: &mut FnCtx<'_>,
    runtime_fn: &'static str,
    recv: &str,
    arg_vals: &[String],
) -> String {
    let arg = arg_vals.first().cloned().unwrap_or_else(|| {
        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    });
    let blk = ctx.block();
    let handle = blk.call(I64, runtime_fn, &[(DOUBLE, recv), (DOUBLE, &arg)]);
    nanbox_string_inline(blk, &handle)
}

/// `time` is the Date's time value when the kind check already read it.
/// Getters and formatters take it in place of the Date, so the runtime does
/// not classify the receiver a second time; `getTime` is that value.
fn emit_date_builtin(
    ctx: &mut FnCtx<'_>,
    date: DateBuiltin,
    recv: &str,
    time: Option<&str>,
    arg_vals: &[String],
) -> String {
    match date {
        DateBuiltin::Getter("js_date_get_time") if time.is_some() => {
            time.unwrap_or(recv).to_string()
        }
        DateBuiltin::Getter(runtime_fn) => {
            let date = time.unwrap_or(recv);
            ctx.block().call(DOUBLE, runtime_fn, &[(DOUBLE, date)])
        }
        DateBuiltin::Formatter(runtime_fn) => {
            let date = time.unwrap_or(recv);
            let blk = ctx.block();
            let handle = blk.call(I64, runtime_fn, &[(DOUBLE, date)]);
            nanbox_string_inline(blk, &handle)
        }
        DateBuiltin::LocaleString => {
            ctx.block()
                .call(DOUBLE, "js_value_to_locale_string", &[(DOUBLE, recv)])
        }
        DateBuiltin::Setter { is_utc, field } => {
            // Entry-block buffer: an alloca in a loop body is a stack
            // adjustment that is never restored (#167).
            let (args_ptr, argc) = if arg_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let buf = ctx.func.alloca_entry_array(DOUBLE, arg_vals.len());
                let blk = ctx.block();
                for (i, value) in arg_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                    blk.store(DOUBLE, value, &slot);
                }
                (buf, arg_vals.len().to_string())
            };
            let is_utc = if is_utc { "1" } else { "0" };
            ctx.block().call(
                DOUBLE,
                "js_date_apply_setter",
                &[
                    (DOUBLE, recv),
                    (I32, is_utc),
                    (I32, &field.to_string()),
                    (PTR, &args_ptr),
                    (I32, &argc),
                ],
            )
        }
    }
}

/// Evaluate the receiver, then the arguments, once; then either call the
/// builtin directly (`guard == None`, a proven receiver) or branch on the
/// receiver's runtime kind between the builtin and universal method dispatch.
///
/// The receiver is rooted across argument evaluation, and both are re-read
/// below it. The group is released in the merge block, below both consuming
/// calls (`open_rooted_group`'s diamond case).
fn guarded_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
    guard: Option<ReceiverKind>,
    builtin: impl FnOnce(&mut FnCtx<'_>, &str, Option<&str>, &[String]) -> String,
) -> Result<String> {
    let mut group = open_rooted_group(args.len() + 1);
    let recv_box = lower_expr(ctx, object)?;
    let recv_collects = any_operand_may_collect(ctx, args.iter());
    let rooted_recv = group.adopt_emitted(ctx, Repr::Boxed, &recv_box, recv_collects);
    for (i, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[i + 1..].iter());
        group.lower(ctx, arg, collects)?;
    }
    let recv = group.reread_emitted(ctx, rooted_recv);
    let arg_vals = group.reread_all(ctx)?;

    let Some(kind) = guard else {
        let value = builtin(ctx, &recv, None, &arg_vals);
        group.release(ctx);
        return Ok(value);
    };

    let builtin_idx = ctx.new_block("kindguard.builtin");
    let generic_idx = ctx.new_block("kindguard.generic");
    let merge_idx = ctx.new_block("kindguard.merge");
    let builtin_label = ctx.block_label(builtin_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    let time = emit_receiver_kind_branch(ctx, kind, &recv, &builtin_label, &generic_label);

    ctx.current_block = builtin_idx;
    let builtin_value = builtin(ctx, &recv, time.as_deref(), &arg_vals);
    let builtin_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = generic_idx;
    let generic_value = super::super::console_promise::emit_native_method_str_dispatch(
        ctx,
        property,
        call_byte_offset,
        &recv,
        &arg_vals,
    );
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let value = ctx.block().phi(
        DOUBLE,
        &[
            (builtin_value.as_str(), builtin_end.as_str()),
            (generic_value.as_str(), generic_end.as_str()),
        ],
    );
    group.release(ctx);
    Ok(value)
}

/// Branch to `builtin_label` when the NaN-boxed `recv` has the kind the
/// builtin requires, else to `generic_label`. No check allocates or runs user
/// code. A Date check returns the Date's time value.
fn emit_receiver_kind_branch(
    ctx: &mut FnCtx<'_>,
    kind: ReceiverKind,
    recv: &str,
    builtin_label: &str,
    generic_label: &str,
) -> Option<String> {
    match kind {
        // A Date is a `DateCell` pointer, and only the runtime can tell one
        // from any other pointer safely (off-heap buffers carry no GC header).
        // `js_date_get_time` returns a Date's time value — a Number, which can
        // never equal the POINTER-tagged bits of the cell holding it — and
        // every other value bit for bit, so one call both classifies the
        // receiver and reads the time value the builtin arm needs.
        ReceiverKind::Date => {
            let blk = ctx.block();
            let time = blk.call(DOUBLE, "js_date_get_time", &[(DOUBLE, recv)]);
            let time_bits = blk.bitcast_double_to_i64(&time);
            let recv_bits = blk.bitcast_double_to_i64(recv);
            let is_date = blk.icmp_ne(I64, &time_bits, &recv_bits);
            blk.cond_br(&is_date, builtin_label, generic_label);
            Some(time)
        }
        // `toLocaleString()` without arguments is `Object.prototype`'s on every
        // primitive, a Date's own, and a Symbol's inherited one:
        // `js_value_to_locale_string` — the proven path's entry point — formats
        // all of them. A heap object may own the method (a user class, dayjs),
        // and a nullish receiver must throw the property read's TypeError, so
        // both take method dispatch.
        ReceiverKind::LocaleValue => {
            let heap_or_nullish_idx = ctx.new_block("kindguard.locale_heap_or_nullish");
            let heap_idx = ctx.new_block("kindguard.locale_heap");
            let symbol_idx = ctx.new_block("kindguard.locale_symbol");
            let heap_or_nullish_label = ctx.block_label(heap_or_nullish_idx);
            let heap_label = ctx.block_label(heap_idx);
            let symbol_label = ctx.block_label(symbol_idx);

            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(recv);
            let tag = blk.lshr(I64, &bits, "48");
            let is_pointer = blk.icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_TOP16_I64);
            let is_undefined = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
            let is_null = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_NULL_I64);
            let is_nullish = blk.or(I1, &is_undefined, &is_null);
            let not_primitive = blk.or(I1, &is_pointer, &is_nullish);
            blk.cond_br(&not_primitive, &heap_or_nullish_label, builtin_label);

            ctx.current_block = heap_or_nullish_idx;
            ctx.block().cond_br(&is_pointer, &heap_label, generic_label);

            ctx.current_block = heap_idx;
            let blk = ctx.block();
            let time = blk.call(DOUBLE, "js_date_get_time", &[(DOUBLE, recv)]);
            let time_bits = blk.bitcast_double_to_i64(&time);
            let is_date = blk.icmp_ne(I64, &time_bits, &bits);
            blk.cond_br(&is_date, builtin_label, &symbol_label);

            ctx.current_block = symbol_idx;
            let blk = ctx.block();
            let is_symbol = blk.call(I32, "js_is_symbol", &[(DOUBLE, recv)]);
            let is_symbol = blk.icmp_ne(I32, &is_symbol, "0");
            blk.cond_br(&is_symbol, builtin_label, generic_label);
            None
        }
        // A number is any IEEE double outside Perry's positive tag band
        // 0x7FF9..=0x7FFF (`JSValue::is_number`), or an INT32-tagged value.
        ReceiverKind::Number => {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(recv);
            let tag = blk.lshr(I64, &bits, "48");
            let band_offset = blk.sub(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
            let in_tag_band = blk.icmp_ult(I64, &band_offset, "7");
            let is_double = blk.xor(I1, &in_tag_band, "true");
            let is_int32 = blk.icmp_eq(I64, &tag, crate::nanbox::INT32_TAG_TOP16_I64);
            let is_number = blk.or(I1, &is_double, &is_int32);
            blk.cond_br(&is_number, builtin_label, generic_label);
            None
        }
        // A plain array: a POINTER-tagged heap address above the small-handle
        // band whose GcHeader is `GC_TYPE_ARRAY` and not forwarded — the header
        // test `expr/array_pop.rs` inlines. Lazy JSON arrays, Array subclass
        // instances, typed arrays and proxies take method dispatch. (A
        // header-less Buffer whose preceding byte happens to match reaches
        // helpers that dispatch on Buffer / typed-array receivers first, the
        // same helpers the unguarded pre-#10476 fold called on every receiver.)
        ReceiverKind::Array => {
            const POINTER_TAG_TOP16: &str = "32765"; // 0x7FFD
            const HANDLE_BAND_TOP: &str = "1048575"; // 0x0FFFFF
            const HEAP_LIMIT: &str = "140737488355328"; // 2^47
            const GC_TYPE_ARRAY_I8: &str = "1";
            const GC_FLAG_FORWARDED_I8: &str = "-128"; // 0x80 as i8
            let header_idx = ctx.new_block("kindguard.array_header");
            let header_label = ctx.block_label(header_idx);
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(recv);
            let tag = blk.lshr(I64, &bits, "48");
            let is_pointer = blk.icmp_eq(I64, &tag, POINTER_TAG_TOP16);
            let handle = unbox_to_i64(blk, recv);
            let above_band = blk.icmp_ugt(I64, &handle, HANDLE_BAND_TOP);
            let below_limit = blk.icmp_ult(I64, &handle, HEAP_LIMIT);
            let in_heap = blk.and(I1, &above_band, &below_limit);
            let candidate = blk.and(I1, &is_pointer, &in_heap);
            blk.cond_br(&candidate, &header_label, generic_label);

            ctx.current_block = header_idx;
            let blk = ctx.block();
            let type_addr = blk.sub(I64, &handle, "8");
            let type_ptr = blk.inttoptr(I64, &type_addr);
            let gc_type = blk.load(I8, &type_ptr);
            let is_array = blk.icmp_eq(I8, &gc_type, GC_TYPE_ARRAY_I8);
            let flags_addr = blk.sub(I64, &handle, "7");
            let flags_ptr = blk.inttoptr(I64, &flags_addr);
            let flags = blk.load(I8, &flags_ptr);
            let forwarded = blk.and(I8, &flags, GC_FLAG_FORWARDED_I8);
            let not_forwarded = blk.icmp_eq(I8, &forwarded, "0");
            let plain_array = blk.and(I1, &is_array, &not_forwarded);
            blk.cond_br(&plain_array, builtin_label, generic_label);
            None
        }
    }
}

#[cfg(test)]
#[path = "builtin_kind_guard_tests.rs"]
mod tests;
