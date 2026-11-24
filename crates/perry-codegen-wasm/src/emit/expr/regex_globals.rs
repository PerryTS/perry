//! RegExp constructor/test and global builtins (parseInt/parseFloat/Number/isNaN/isFinite/BigInt).
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_regex_globals(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            Expr::RegExp { pattern, flags } => {
                let pat_id = self
                    .emitter
                    .string_map
                    .get(pattern.as_str())
                    .copied()
                    .unwrap_or(0);
                let pat_bits = (STRING_TAG << 48) | (pat_id as u64);
                let flags_id = self
                    .emitter
                    .string_map
                    .get(flags.as_str())
                    .copied()
                    .unwrap_or(0);
                let flags_bits = (STRING_TAG << 48) | (flags_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(pat_bits));
                self.emit_store_const(func, 1, f64::from_bits(flags_bits));
                self.emit_memcall(func, "regexp_new", 2);
            }
            // --- Global builtins ---
            // An omitted radix reaches the bridge as `undefined`, which `parseInt` reads
            // as "detect": a `0x` prefix is hexadecimal, anything else decimal.
            Expr::ParseInt { string, radix } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, radix.as_deref().unwrap_or(&Expr::Undefined));
                self.emit_memcall(func, "parse_int", 2);
            }
            Expr::ParseFloat(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "parse_float", 1);
            }
            Expr::EncodeURI(val)
            | Expr::DecodeURI(val)
            | Expr::EncodeURIComponent(val)
            | Expr::DecodeURIComponent(val) => {
                let bridge = match expr {
                    Expr::EncodeURI(_) => "encode_uri",
                    Expr::DecodeURI(_) => "decode_uri",
                    Expr::EncodeURIComponent(_) => "encode_uri_component",
                    _ => "decode_uri_component",
                };
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, bridge, 1);
            }
            Expr::NumberCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "number_coerce", 1);
            }
            // The global `isNaN` / `isFinite` apply ToNumber first (`isNaN("abc")` is
            // true), so they cannot share the non-coercing `is_nan` / `is_finite`.
            Expr::IsNaN(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "global_is_nan", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            // Bit-exact, as `js_is_undefined_or_bare_nan` in perry-runtime: the value is
            // `undefined` or the canonical NaN `0x7FF8_0000_0000_0000`. A NaN-boxed tag
            // is never canonical NaN, so `null`, strings and objects all test false.
            Expr::IsUndefinedOrBareNan(val) => {
                self.emit_expr(func, val);
                func.instruction(&Instruction::LocalTee(self.temp_local));
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                func.instruction(&Instruction::I64Eq);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Const(f64::NAN.to_bits() as i64));
                func.instruction(&Instruction::I64Eq);
                func.instruction(&Instruction::I32Or);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::IsFinite(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "global_is_finite", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            // `is_nan` / `is_finite` are `Number.isNaN` / `Number.isFinite` (no coercion).
            Expr::NumberIsNaN(val)
            | Expr::NumberIsFinite(val)
            | Expr::NumberIsInteger(val)
            | Expr::NumberIsSafeInteger(val) => {
                let bridge = match expr {
                    Expr::NumberIsNaN(_) => "is_nan",
                    Expr::NumberIsFinite(_) => "is_finite",
                    Expr::NumberIsInteger(_) => "number_is_integer",
                    _ => "number_is_safe_integer",
                };
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, bridge, 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BigIntCoerce(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            _ => return false,
        }
        true
    }
}
