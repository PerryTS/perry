//! Bitwise binary-op emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `FuncEmitCtx::emit_bitwise_binary` onto a dedicated
//! `impl<'a> FuncEmitCtx<'a>` block.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    /// Emit a binary bitwise operation with proper i32 truncation. The
    /// result is reinterpreted as a SIGNED i32 — correct for every JS
    /// bitwise operator except `>>>`, which is defined to produce a
    /// ToUint32 value (see `emit_bitwise_binary_u`).
    pub(super) fn emit_bitwise_binary(
        &mut self,
        func: &mut Function,
        left: &Expr,
        right: &Expr,
        op: Instruction<'static>,
    ) {
        self.emit_bitwise_binary_impl(func, left, right, op, false);
    }

    /// `>>>` — JS's unsigned right shift yields a ToUint32 result, so the
    /// i32 must be widened UNSIGNED. Converting it signed (as the shared
    /// path does) is invisible for any shift >= 1, because shifting in a
    /// zero clears the sign bit — but `x >>> 0`, the canonical
    /// "reinterpret this as unsigned" idiom, then hands back the negative
    /// input unchanged. Engine code packs ARGB with `(a|r|g|b) >>> 0` and
    /// got a negative f64 across the FFI, where Rust's saturating
    /// `as u32` floored it to 0 — every model tint became transparent
    /// black.
    pub(super) fn emit_bitwise_binary_u(
        &mut self,
        func: &mut Function,
        left: &Expr,
        right: &Expr,
        op: Instruction<'static>,
    ) {
        self.emit_bitwise_binary_impl(func, left, right, op, true);
    }

    fn emit_bitwise_binary_impl(
        &mut self,
        func: &mut Function,
        left: &Expr,
        right: &Expr,
        op: Instruction<'static>,
        result_unsigned: bool,
    ) {
        self.emit_expr(func, left);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I64TruncSatF64S);
        func.instruction(&Instruction::I32WrapI64);
        self.emit_expr(func, right);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I64TruncSatF64S);
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&op);
        if result_unsigned {
            func.instruction(&Instruction::F64ConvertI32U);
        } else {
            func.instruction(&Instruction::F64ConvertI32S);
        }
        func.instruction(&Instruction::I64ReinterpretF64);
    }
}
