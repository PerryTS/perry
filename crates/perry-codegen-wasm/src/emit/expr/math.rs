//! Math.* operations (floor/ceil/abs/sqrt/round/pow/min/max/random/trig/hyperbolic/log/etc.) and Date.now.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_math(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            Expr::MathFloor(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Floor);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathCeil(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Ceil);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathAbs(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Abs);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathSqrt(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Sqrt);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathRound(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Nearest);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathPow(base, exp) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, base);
                self.emit_store_arg(func, 1, exp);
                self.emit_memcall(func, "math_pow", 2);
            }
            Expr::MathMin(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::F64ReinterpretI64);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Min);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathMax(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::F64ReinterpretI64);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Max);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathRandom => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "math_random", 0);
            }

            Expr::MathLog2(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_log2", 1);
            }
            Expr::MathLog10(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_log10", 1);
            }
            // Issue #133 item 4: trig / exp / etc. are lowered to Expr::Math* at the HIR level
            // (see perry-hir/src/lower.rs). Route them through the Firefox-safe mem_call bridge.
            Expr::MathSin(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_sin", 1);
            }
            Expr::MathCos(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_cos", 1);
            }
            Expr::MathTan(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_tan", 1);
            }
            Expr::MathAsin(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_asin", 1);
            }
            Expr::MathAcos(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_acos", 1);
            }
            Expr::MathAtan(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_atan", 1);
            }
            Expr::MathAtan2(y, x) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, y);
                self.emit_store_arg(func, 1, x);
                self.emit_memcall(func, "math_atan2", 2);
            }
            Expr::MathSinh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_sinh", 1);
            }
            Expr::MathCosh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_cosh", 1);
            }
            Expr::MathTanh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_tanh", 1);
            }
            Expr::MathAsinh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_asinh", 1);
            }
            Expr::MathAcosh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_acosh", 1);
            }
            Expr::MathAtanh(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_atanh", 1);
            }
            Expr::MathCbrt(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_cbrt", 1);
            }
            Expr::MathExp(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_exp", 1);
            }
            Expr::MathExpm1(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_expm1", 1);
            }
            Expr::MathLog1p(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_log1p", 1);
            }
            Expr::MathFround(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_fround", 1);
            }
            Expr::MathClz32(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_clz32", 1);
            }
            Expr::MathHypot(args) => {
                // Variadic: iteratively fold via math_hypot(acc, x)
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_hypot", 2);
                    }
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }
            Expr::MathImul(a, b) => {
                self.emit_expr(func, a);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::I32TruncF64S);
                self.emit_expr(func, b);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::I32TruncF64S);
                func.instruction(&Instruction::I32Mul);
                func.instruction(&Instruction::F64ConvertI32S);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathMin(args) if args.len() != 2 => {
                // Variadic min — use bridge
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_min", 2);
                    }
                } else {
                    func.instruction(&f64_const(f64::INFINITY));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }
            Expr::MathMax(args) if args.len() != 2 => {
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_max", 2);
                    }
                } else {
                    func.instruction(&f64_const(f64::NEG_INFINITY));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }

            Expr::DateNow => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "date_now", 0);
            }

            _ => return false,
        }
        true
    }
}
