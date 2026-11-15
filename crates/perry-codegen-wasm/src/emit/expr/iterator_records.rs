//! The `__perry_runtime` iterator-record intrinsics the HIR lowers `for...of` and
//! array destructuring to (contract: `perry-hir/src/iterator_record.rs`), and the
//! iteration nodes beside them. The runtime side is the `rt_*` bridges.

use super::*;

/// Intrinsics that evaluate every argument once, in order: one bridge each.
const EAGER: &[&str] = &[
    "arrayRecordNeedsIterator",
    "arrayRecordLength",
    "arrayRecordLiteral",
    "arrayRecordClose",
    "arrayRecordCloseAbsent",
    "iteratorNextMethod",
    "iteratorStepOutput",
    "iteratorCloseIfNotDone",
    "iteratorCloseOnThrow",
    "iteratorRestAppend",
    "iteratorRestToArray",
    "typedArrayForOfAdmits",
];

/// Every bridge this module calls, for the string table.
pub(in crate::emit) const BRIDGES: &[&str] = &[
    "rt_arrayRecordNeedsIterator",
    "rt_arrayRecordLength",
    "rt_arrayRecordLiteral",
    "rt_arrayRecordClose",
    "rt_arrayRecordCloseAbsent",
    "rt_iteratorNextMethod",
    "rt_iteratorStepOutput",
    "rt_iteratorCloseIfNotDone",
    "rt_iteratorCloseOnThrow",
    "rt_iteratorRestAppend",
    "rt_iteratorRestToArray",
    "rt_iteratorStep",
    "rt_getIterator",
    "rt_iterableToArray",
    "rt_arrayIterationPatched",
    "rt_typedArrayForOfAdmits",
];

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_iterator_records(
        &mut self,
        func: &mut Function,
        expr: &Expr,
    ) -> bool {
        match expr {
            Expr::NativeMethodCall {
                module,
                method,
                args,
                object: None,
                ..
            } if module == "__perry_runtime" => self.emit_runtime_intrinsic(func, method, args),
            Expr::GetIterator(value) => {
                self.emit_bridge(func, "rt_getIterator", std::slice::from_ref(&**value));
                true
            }
            Expr::IteratorToArray(value) | Expr::ForOfToArray(value) => {
                self.emit_bridge(func, "rt_iterableToArray", std::slice::from_ref(&**value));
                true
            }
            Expr::ArrayIterationPatched => {
                self.emit_bridge(func, "rt_arrayIterationPatched", &[]);
                true
            }
            _ => false,
        }
    }

    fn emit_runtime_intrinsic(&mut self, func: &mut Function, method: &str, args: &[Expr]) -> bool {
        match (method, args) {
            // The mode-selected forms run only the arm their mode picks: in the array
            // arm `stepCall` would call an undefined `next`, and `elem` must not index
            // an iterator.
            ("arrayRecordForValue", [mode, proto_value, elem]) => {
                self.emit_select(
                    func,
                    mode,
                    |ctx, func| ctx.emit_expr(func, proto_value),
                    |ctx, func| ctx.emit_expr(func, elem),
                );
            }
            ("arrayRecordForUpdate", [mode, update]) => {
                self.emit_select(
                    func,
                    mode,
                    |ctx, func| ctx.emit_expr(func, &Expr::Number(0.0)),
                    |ctx, func| ctx.emit_expr(func, update),
                );
            }
            ("arrayRecordForBound", [mode, len, idx, step_call, state_set]) => {
                self.emit_expr(func, state_set);
                if self.expr_has_value(state_set) {
                    func.instruction(&Instruction::Drop);
                }
                let protocol = |ctx: &mut Self, func: &mut Function| {
                    ctx.emit_select(
                        func,
                        step_call,
                        |ctx, func| ctx.emit_expr(func, idx),
                        |ctx, func| ctx.emit_expr(func, &Expr::Number(f64::INFINITY)),
                    )
                };
                self.emit_select(func, mode, protocol, |ctx, func| ctx.emit_expr(func, len));
            }
            ("arrayRecordIndex", [value]) => self.emit_expr(func, value),
            // `(source, payload := ..., source := undefined)`: the payload write is the
            // value; `source` only names the binding the last operand clears.
            ("arrayRecordPayload", [_source, payload, clear]) => {
                self.emit_expr(func, payload);
                self.emit_expr(func, clear);
                if self.expr_has_value(clear) {
                    func.instruction(&Instruction::Drop);
                }
            }
            // The step runs before its output operand, which reads what it produced.
            ("iteratorStep", [iter, next, output, read @ ..]) => {
                let read = read.first().cloned().unwrap_or(Expr::Bool(true));
                self.emit_bridge(func, "rt_iteratorStep", &[iter.clone(), next.clone(), read]);
                self.emit_expr(func, output);
                if self.expr_has_value(output) {
                    func.instruction(&Instruction::Drop);
                }
            }
            (name, args) if EAGER.contains(&name) => {
                self.emit_bridge(func, &format!("rt_{name}"), args);
            }
            _ => return false,
        }
        true
    }

    /// Pushes `condition` as an i32. A record mode is `x === true`, tested on its bits,
    /// so the per-element path calls no bridge for it.
    fn emit_condition(&mut self, func: &mut Function, condition: &Expr) {
        match condition {
            Expr::Compare {
                op: CompareOp::Eq,
                left,
                right,
            } if matches!(right.as_ref(), Expr::Bool(true)) => {
                self.emit_expr(func, left);
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::I64Eq);
            }
            _ => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
            }
        }
    }

    fn emit_select(
        &mut self,
        func: &mut Function,
        condition: &Expr,
        then_arm: impl FnOnce(&mut Self, &mut Function),
        else_arm: impl FnOnce(&mut Self, &mut Function),
    ) {
        self.emit_condition(func, condition);
        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
            ValType::I64,
        )));
        self.block_depth += 1;
        then_arm(self, func);
        func.instruction(&Instruction::Else);
        else_arm(self, func);
        self.block_depth -= 1;
        func.instruction(&Instruction::End);
    }

    fn emit_bridge(&mut self, func: &mut Function, bridge: &str, args: &[Expr]) {
        let count = args.len() as u32;
        self.emit_frame_begin(func, count);
        for (slot, arg) in args.iter().enumerate() {
            self.emit_store_arg(func, slot as u32, arg);
        }
        self.emit_memcall(func, bridge, count);
    }
}
