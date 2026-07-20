//! Literals, variable load/store, update, unary, binary, comparison, logical, conditional, typeof, await, void, sequence, and misc no-op variants.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_literals_vars(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            Expr::Number(n) => {
                func.instruction(&f64_const(*n));
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::Integer(i) => {
                func.instruction(&f64_const(*i as f64));
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::Bool(true) => {
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::Bool(false) => {
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
            }
            Expr::Undefined => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::Null => {
                func.instruction(&Instruction::I64Const(TAG_NULL as i64));
            }
            Expr::String(s) => {
                let string_id = self
                    .emitter
                    .string_map
                    .get(s.as_str())
                    .copied()
                    .unwrap_or(0);
                // All values are i64 now. i64.const preserves all bits.
                let bits = (STRING_TAG << 48) | (string_id as u64);
                func.instruction(&Instruction::I64Const(bits as i64));
            }

            // --- Variables ---
            Expr::LocalGet(id) => {
                // Check module_let_globals FIRST (handles top-level Lets in current module)
                if let Some(&gidx) = self
                    .emitter
                    .module_let_globals
                    .get(&(self.emitter.current_mod_idx, *id))
                {
                    func.instruction(&Instruction::GlobalGet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    // Unknown local — push undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::LocalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&gidx) = self
                    .emitter
                    .module_let_globals
                    .get(&(self.emitter.current_mod_idx, *id))
                {
                    // Module-level let — write to WASM global, then read back to leave on stack
                    func.instruction(&Instruction::GlobalSet(gidx));
                    func.instruction(&Instruction::GlobalGet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    // Tee: set and leave on stack
                    func.instruction(&Instruction::LocalTee(idx));
                }
            }
            Expr::GlobalGet(id) => {
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    func.instruction(&Instruction::GlobalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::GlobalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    // Duplicate value on stack (set + leave result)
                    // WASM doesn't have GlobalTee, so we need a local
                    func.instruction(&Instruction::GlobalSet(idx));
                    func.instruction(&Instruction::GlobalGet(idx));
                }
            }

            // --- Update ---
            Expr::Update { id, op, prefix } => {
                if let Some(&gidx) = self
                    .emitter
                    .module_let_globals
                    .get(&(self.emitter.current_mod_idx, *id))
                {
                    if *prefix {
                        // ++x on a module-level let: increment the backing WASM
                        // global and leave the new value on the stack.
                        func.instruction(&Instruction::GlobalGet(gidx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => {
                                func.instruction(&Instruction::F64Add);
                            }
                            UpdateOp::Decrement => {
                                func.instruction(&Instruction::F64Sub);
                            }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::GlobalSet(gidx));
                        func.instruction(&Instruction::GlobalGet(gidx));
                    } else {
                        // x++ on a module-level let: return the old value while
                        // persisting the incremented value to the backing global.
                        func.instruction(&Instruction::GlobalGet(gidx));
                        // Compute new value
                        func.instruction(&Instruction::GlobalGet(gidx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => {
                                func.instruction(&Instruction::F64Add);
                            }
                            UpdateOp::Decrement => {
                                func.instruction(&Instruction::F64Sub);
                            }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::GlobalSet(gidx));
                        // Old value (i64) is still on stack
                    }
                } else if let Some(&idx) = self.local_map.get(id) {
                    if *prefix {
                        // ++x: increment then return new value
                        // local is i64, convert to f64, add 1, convert back
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => {
                                func.instruction(&Instruction::F64Add);
                            }
                            UpdateOp::Decrement => {
                                func.instruction(&Instruction::F64Sub);
                            }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::LocalTee(idx));
                    } else {
                        // x++: return old value, then increment
                        func.instruction(&Instruction::LocalGet(idx));
                        // Compute new value
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => {
                                func.instruction(&Instruction::F64Add);
                            }
                            UpdateOp::Decrement => {
                                func.instruction(&Instruction::F64Sub);
                            }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::LocalSet(idx));
                        // Old value (i64) is still on stack
                    }
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }

            Expr::Binary { op, left, right } => {
                match op {
                    BinaryOp::Add => {
                        // Use js_add for dynamic dispatch (handles string+number etc.)
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "js_add", 2);
                    }
                    // Bitwise ops need i32 truncation before the operation
                    BinaryOp::BitAnd => {
                        self.emit_bitwise_binary(func, left, right, Instruction::I32And);
                    }
                    BinaryOp::BitOr => {
                        self.emit_bitwise_binary(func, left, right, Instruction::I32Or);
                    }
                    BinaryOp::BitXor => {
                        self.emit_bitwise_binary(func, left, right, Instruction::I32Xor);
                    }
                    BinaryOp::Shl => {
                        self.emit_bitwise_binary(func, left, right, Instruction::I32Shl);
                    }
                    BinaryOp::Shr => {
                        self.emit_bitwise_binary(func, left, right, Instruction::I32ShrS);
                    }
                    BinaryOp::UShr => {
                        // ToUint32 result — see emit_bitwise_binary_u.
                        self.emit_bitwise_binary_u(func, left, right, Instruction::I32ShrU);
                    }
                    // Mod and Pow go through JS bridge (no native WASM instruction)
                    // — use emit_store_arg to keep values as i64, like Add
                    BinaryOp::Mod => {
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "js_mod", 2);
                    }
                    BinaryOp::Pow => {
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "math_pow", 2);
                    }
                    _ => {
                        // Pure numeric operations - convert i64 to f64, operate, convert back
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::F64ReinterpretI64);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::F64ReinterpretI64);
                        match op {
                            BinaryOp::Sub => {
                                func.instruction(&Instruction::F64Sub);
                            }
                            BinaryOp::Mul => {
                                func.instruction(&Instruction::F64Mul);
                            }
                            BinaryOp::Div => {
                                func.instruction(&Instruction::F64Div);
                            }
                            _ => {
                                func.instruction(&Instruction::F64Add);
                            }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                }
            }

            Expr::Compare { op, left, right } => {
                self.emit_expr(func, left);
                self.emit_expr(func, right);
                // For strict equality on mixed types, use JS bridge
                match op {
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe => {
                        // Values are i64 on stack, store them to memory via emit_store_arg pattern
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 1);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        let eq_fn = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                            "js_loose_eq"
                        } else {
                            "js_strict_eq"
                        };
                        self.emit_memcall_i32(func, eq_fn, 2);
                        if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                            func.instruction(&Instruction::I32Eqz);
                        }
                        // Convert i32 result to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                    _ => {
                        // Numeric comparisons - convert i64 to f64 first
                        // Stack: [left_i64, right_i64]
                        func.instruction(&Instruction::LocalSet(self.temp_local)); // save right_i64
                        func.instruction(&Instruction::F64ReinterpretI64); // left -> f64
                        func.instruction(&Instruction::LocalGet(self.temp_local)); // push right_i64
                        func.instruction(&Instruction::F64ReinterpretI64); // right -> f64
                        match op {
                            CompareOp::Lt => func.instruction(&Instruction::F64Lt),
                            CompareOp::Le => func.instruction(&Instruction::F64Le),
                            CompareOp::Gt => func.instruction(&Instruction::F64Gt),
                            CompareOp::Ge => func.instruction(&Instruction::F64Ge),
                            _ => unreachable!(),
                        };
                        // Convert i32 to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                }
            }

            Expr::Logical { op, left, right } => {
                match op {
                    LogicalOp::And => {
                        // Short-circuit: if left is falsy, return left; else return right
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Or => {
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Coalesce => {
                        // a ?? b: if a is null/undefined, return b; otherwise return a
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_null_or_undefined", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                }
            }

            Expr::Unary { op, operand } => {
                self.emit_expr(func, operand);
                match op {
                    UnaryOp::Neg => {
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&Instruction::F64Neg);
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                    UnaryOp::Pos => {} // no-op for numbers
                    UnaryOp::Not => {
                        self.emit_frame_begin(func, 1);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::I32Eqz);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                            ValType::I64,
                        )));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                    UnaryOp::BitNot => {
                        // ~x: convert i64 to f64, truncate to i32, bitwise not, convert back to i64
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&Instruction::I64TruncSatF64S);
                        func.instruction(&Instruction::I32WrapI64);
                        func.instruction(&Instruction::I32Const(-1));
                        func.instruction(&Instruction::I32Xor);
                        func.instruction(&Instruction::F64ConvertI32S);
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                };
            }

            // --- Conditional (ternary) ---
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                self.emit_expr(func, then_expr);
                func.instruction(&Instruction::Else);
                self.emit_expr(func, else_expr);
                func.instruction(&Instruction::End);
            }

            Expr::TypeOf(operand) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, operand);
                self.emit_memcall(func, "js_typeof", 1);
            }

            Expr::Await(e) => {
                // Evaluate inner expression, then call await_promise bridge
                // If the value is a promise handle, tries to get resolved value
                // If not a promise, returns the value as-is
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, e);
                self.emit_memcall(func, "await_promise", 1);
            }

            Expr::Void(e) => {
                self.emit_expr(func, e);
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            Expr::JsLoadModule { .. }
            | Expr::JsGetExport { .. }
            | Expr::JsCallFunction { .. }
            | Expr::JsCallMethod { .. }
            | Expr::JsGetProperty { .. }
            | Expr::JsSetProperty { .. }
            | Expr::JsNew { .. }
            | Expr::JsNewFromHandle { .. }
            | Expr::JsCreateCallback { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Misc ---
            Expr::ImportMetaUrl(_) | Expr::StaticPluginResolve(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::Yield { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::BigInt(_) | Expr::NativeModuleRef(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            Expr::Sequence(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    self.emit_expr(func, e);
                    if i < exprs.len() - 1 {
                        func.instruction(&Instruction::Drop);
                    }
                }
                if exprs.is_empty() {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            _ => return false,
        }
        true
    }
}
