//! Date constructors/getters/setters and Error/TypeError/AggregateError.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_date_error(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            Expr::DateNew(args) => {
                if let Some(a) = args.first() {
                    self.emit_expr(func, a);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "date_new", 1);
            }
            Expr::DateGetTime(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_time", 1);
            }
            Expr::DateToISOString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_iso_string", 1);
            }
            Expr::DateGetFullYear(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_full_year", 1);
            }
            Expr::DateGetMonth(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_month", 1);
            }
            Expr::DateGetDate(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_date", 1);
            }
            Expr::DateGetDay(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_day", 1);
            }
            Expr::DateGetHours(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_hours", 1);
            }
            Expr::DateGetMinutes(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_minutes", 1);
            }
            Expr::DateGetSeconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_seconds", 1);
            }
            Expr::DateGetMilliseconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_milliseconds", 1);
            }
            Expr::DateParse(s) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, s);
                self.emit_memcall(func, "date_parse", 1);
            }
            Expr::DateUtc(args) => {
                let n = args.len().max(1) as u32;
                self.emit_frame_begin(func, n);
                for (i, a) in args.iter().enumerate() {
                    self.emit_store_arg(func, i as u32, a);
                }
                self.emit_memcall(func, "date_utc", n);
            }
            Expr::DateGetUtcDay(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_day", 1);
            }
            Expr::DateGetUtcFullYear(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_full_year", 1);
            }
            Expr::DateGetUtcMonth(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_month", 1);
            }
            Expr::DateGetUtcDate(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_date", 1);
            }
            Expr::DateGetUtcHours(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_hours", 1);
            }
            Expr::DateGetUtcMinutes(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_minutes", 1);
            }
            Expr::DateGetUtcSeconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_seconds", 1);
            }
            Expr::DateGetUtcMilliseconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_milliseconds", 1);
            }
            Expr::DateValueOf(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_value_of", 1);
            }
            Expr::DateGetTimezoneOffset(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_timezone_offset", 1);
            }
            Expr::DateToDateString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_date_string", 1);
            }
            Expr::DateToTimeString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_time_string", 1);
            }
            Expr::DateToLocaleDateString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_date_string", 1);
            }
            Expr::DateToLocaleTimeString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_time_string", 1);
            }
            Expr::DateToLocaleString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_string", 1);
            }
            Expr::DateToJSON(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_json", 1);
            }
            // The wasm backend still uses single-value Date setter memcalls
            // (its JS runtime ignores optional trailing components — the
            // multi-arg #2851 work targets the native LLVM backend). Emit the
            // leading argument (Undefined when the call site passes none).
            Expr::DateSetUtcFullYear { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_full_year", 2);
            }
            Expr::DateSetUtcMonth { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_month", 2);
            }
            Expr::DateSetUtcDate { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_date", 2);
            }
            Expr::DateSetUtcHours { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_hours", 2);
            }
            Expr::DateSetUtcMinutes { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_minutes", 2);
            }
            Expr::DateSetUtcSeconds { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_seconds", 2);
            }
            Expr::DateSetUtcMilliseconds { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_utc_milliseconds", 2);
            }
            Expr::DateSetFullYear { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_full_year", 2);
            }
            Expr::DateSetMonth { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_month", 2);
            }
            Expr::DateSetDate { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_date", 2);
            }
            Expr::DateSetHours { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_hours", 2);
            }
            Expr::DateSetMinutes { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_minutes", 2);
            }
            Expr::DateSetSeconds { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_seconds", 2);
            }
            Expr::DateSetMilliseconds { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_milliseconds", 2);
            }
            Expr::DateSetTime { date, args } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, args.first().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "date_set_time", 2);
            }

            // --- Error ---
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.emit_expr(func, m);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::ErrorMessage(err) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, err);
                self.emit_memcall(func, "error_message", 1);
            }
            Expr::ErrorNewWithCause { message, cause: _ } => {
                // WASM stub: ignore cause for now, falls back to plain Error
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::ErrorNewWithOptions {
                message,
                options: _,
                ..
            } => {
                // WASM stub: ignore options/cause, falls back to plain Error
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::TypeErrorNew(msg)
            | Expr::RangeErrorNew(msg)
            | Expr::ReferenceErrorNew(msg)
            | Expr::SyntaxErrorNew(msg) => {
                // WASM stub: alias to error_new
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, msg);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::AggregateErrorNew {
                errors: _,
                message,
                options: _,
            } => {
                // WASM stub: alias to error_new (drops errors array)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }

            _ => return false,
        }
        true
    }
}
