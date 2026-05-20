//! Bitwise binary-op emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `FuncEmitCtx::emit_bitwise_binary` onto a dedicated
//! `impl<'a> FuncEmitCtx<'a>` block.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    /// Emit a binary bitwise operation with proper i32 truncation
    pub(super) fn emit_bitwise_binary(
        &mut self,
        func: &mut Function,
        left: &Expr,
        right: &Expr,
        op: Instruction<'static>,
    ) {
        self.emit_expr(func, left);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I32TruncF64S);
        self.emit_expr(func, right);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I32TruncF64S);
        func.instruction(&op);
        func.instruction(&Instruction::F64ConvertI32S);
        func.instruction(&Instruction::I64ReinterpretF64);
    }
}
