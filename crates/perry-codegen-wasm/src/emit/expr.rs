//! Expression emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of the ~4.6k-line `FuncEmitCtx::emit_expr`. Kept on the same
//! struct via a dedicated `impl<'a> FuncEmitCtx<'a>` block.
//!
//! NOTE (follow-up, per #1102): the within-`emit_expr` split by `Expr::*`
//! variant (mirroring #1099 for the LLVM backend) is intentionally deferred
//! to a later PR so reviewers can stage the work.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn emit_expr(&mut self, func: &mut Function, expr: &Expr) {
        match expr {
            // --- Literals ---
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
                if let Some(&idx) = self.local_map.get(id) {
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

            // --- Binary operations ---
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
                        self.emit_bitwise_binary(func, left, right, Instruction::I32ShrU);
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

            // --- Comparison ---
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

            // --- Logical ---
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

            // --- Unary ---
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
                        func.instruction(&Instruction::I32TruncF64S);
                        func.instruction(&Instruction::I32Const(-1));
                        func.instruction(&Instruction::I32Xor);
                        func.instruction(&Instruction::F64ConvertI32S);
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                };
            }

            // --- Function calls ---
            Expr::Call { callee, args, .. } => {
                // Check for method call patterns: obj.method(args)
                if let Expr::PropertyGet { object, property } = callee.as_ref() {
                    // console.log/warn/error
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        match property.as_str() {
                            "log" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_log", 1);
                                }
                                return;
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_warn", 1);
                                }
                                return;
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_error", 1);
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                    // String/Array method calls: expr.method(args)
                    if self.emit_method_call(func, object, property, args) {
                        return;
                    }

                    // Fallback: class/UI method dispatch via mem_call with stack-based buffer.
                    {
                        let method_name = property.as_str();
                        // Slot 0 = object, slots 1..N = args
                        self.emit_frame_begin(func, (args.len() + 1) as u32);
                        self.emit_store_arg(func, 0, object);
                        for (i, arg) in args.iter().enumerate() {
                            self.emit_store_arg(func, (i + 1) as u32, arg);
                        }
                        self.emit_memcall(func, method_name, (args.len() + 1) as u32);
                        return;
                    }
                }

                // Evaluate arguments first
                for arg in args {
                    self.emit_expr(func, arg);
                }
                // Call the function — resolve target and pad missing optional args with undefined
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&idx) = self.emitter.func_map.get(id) {
                            // Reconcile source arg count with callee arity. JS semantics
                            // allow a call to pass any number of args, but WASM `call`
                            // consumes exactly the declared param count. Pad up with
                            // `undefined` for missing optional args and drop excess
                            // evaluated args from the top of the operand stack, which
                            // would otherwise accumulate past the call and trip the
                            // validator at the enclosing `end` (#183).
                            if let Some(&expected) = self.emitter.func_param_counts.get(&idx) {
                                for _ in args.len()..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                for _ in expected..args.len() {
                                    func.instruction(&Instruction::Drop);
                                }
                            }
                            func.instruction(&Instruction::Call(idx));
                            // Void functions don't push a return value; push undefined
                            // so the caller always has a value on the stack.
                            if self.emitter.void_funcs.contains(&idx) {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        } else {
                            // Unknown function — push undefined
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                    Expr::ExternFuncRef {
                        name, return_type, ..
                    } => {
                        // Cross-module or FFI function call — look up by name.
                        // See FuncRef arm above for why both pad-up and drop-excess
                        // are required (#183).
                        if let Some(&idx) = self.emitter.func_name_map.get(name) {
                            if let Some(&expected) = self.emitter.func_param_counts.get(&idx) {
                                for _ in args.len()..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                for _ in expected..args.len() {
                                    func.instruction(&Instruction::Drop);
                                }
                            }
                            func.instruction(&Instruction::Call(idx));
                            // Void functions don't push a return value, but call
                            // expressions always need a value on the stack. Push undefined.
                            if matches!(return_type, perry_types::Type::Void)
                                || self.emitter.void_funcs.contains(&idx)
                            {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        } else {
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                    _ => {
                        // Dynamic call via closure bridge
                        // Stack has: [arg0, arg1, ..., argN] but callee not yet pushed
                        // We need callee first for closure_call. Restructure:
                        // Drop the args we already pushed, re-emit callee first, then args
                        for _ in args {
                            func.instruction(&Instruction::Drop);
                        }
                        // Now emit: callee, args... via mem_call for Firefox NaN safety
                        self.emit_frame_begin(func, (args.len() + 1) as u32);
                        self.emit_store_arg(func, 0, callee);
                        for (i, arg) in args.iter().enumerate() {
                            self.emit_store_arg(func, (i + 1) as u32, arg);
                        }
                        match args.len() {
                            0 => {
                                self.emit_memcall(func, "closure_call_0", 1);
                            }
                            1 => {
                                self.emit_memcall(func, "closure_call_1", 2);
                            }
                            2 => {
                                self.emit_memcall(func, "closure_call_2", 3);
                            }
                            3 => {
                                self.emit_memcall(func, "closure_call_3", 4);
                            }
                            _ => {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                    }
                }
            }

            // --- Native method calls (console.log, etc.) ---
            Expr::NativeMethodCall {
                module,
                method,
                object,
                args,
                class_name,
            } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "console" => match method.as_str() {
                        "log" => {
                            for arg in args {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, arg);
                                self.emit_memcall_void(func, "console_log", 1);
                            }
                        }
                        "warn" => {
                            for arg in args {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, arg);
                                self.emit_memcall_void(func, "console_warn", 1);
                            }
                        }
                        "error" => {
                            for arg in args {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, arg);
                                self.emit_memcall_void(func, "console_error", 1);
                            }
                        }
                        _ => {}
                    },
                    "JSON" => match method.as_str() {
                        "parse" => {
                            if let Some(a) = args.first() {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, a);
                                self.emit_memcall(func, "json_parse", 1);
                            } else {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                        "stringify" => {
                            if let Some(a) = args.first() {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, a);
                                self.emit_memcall(func, "json_stringify", 1);
                            } else {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                        _ => {}
                    },
                    "Math" => {
                        match method.as_str() {
                            "floor" => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Floor);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "ceil" => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Ceil);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "round" => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Nearest);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "abs" => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Abs);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "sqrt" => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Sqrt);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "pow" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_pow", 2);
                            }
                            "min" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Min);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "max" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Max);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "random" => {
                                self.emit_frame_begin(func, 0);
                                self.emit_memcall(func, "math_random", 0);
                            }
                            "log" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log", 1);
                            }
                            "log2" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log2", 1);
                            }
                            "log10" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log10", 1);
                            }
                            // Trig / exp / sign / trunc / cbrt / hypot (Issue #133 item 4)
                            "sin" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sin", 1);
                            }
                            "cos" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cos", 1);
                            }
                            "tan" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_tan", 1);
                            }
                            "asin" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_asin", 1);
                            }
                            "acos" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_acos", 1);
                            }
                            "atan" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_atan", 1);
                            }
                            "atan2" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_atan2", 2);
                            }
                            "sinh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sinh", 1);
                            }
                            "cosh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cosh", 1);
                            }
                            "tanh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_tanh", 1);
                            }
                            "exp" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_exp", 1);
                            }
                            "sign" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sign", 1);
                            }
                            "trunc" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_trunc", 1);
                            }
                            "cbrt" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cbrt", 1);
                            }
                            "hypot" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_hypot", 2);
                            }
                            _ => {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                    }
                    "perry/ui" | "perry/system" => {
                        // Memory-based dispatch: write args to WASM memory via i64.store.
                        let bridge_name = map_ui_method(method, class_name.as_deref());
                        let _name_id = self
                            .emitter
                            .string_map
                            .get(bridge_name)
                            .copied()
                            .unwrap_or(0);
                        let mut slot = 0u32;
                        let total_slots =
                            (if object.is_some() { 1 } else { 0 }) + args.len() as u32;
                        self.emit_frame_begin(func, total_slots);

                        if let Some(obj) = object {
                            self.emit_store_arg(func, slot, obj);
                            slot += 1;
                        }
                        for arg in args {
                            self.emit_store_arg(func, slot, arg);
                            slot += 1;
                        }
                        self.emit_memcall(func, bridge_name, slot);
                    }
                    "perry/thread" => match method.as_str() {
                        "parallelMap" if args.len() >= 2 => {
                            self.emit_frame_begin(func, 2);
                            self.emit_store_arg(func, 0, &args[0]);
                            self.emit_store_arg(func, 1, &args[1]);
                            self.emit_memcall(func, "thread_parallel_map", 2);
                        }
                        "parallelFilter" if args.len() >= 2 => {
                            self.emit_frame_begin(func, 2);
                            self.emit_store_arg(func, 0, &args[0]);
                            self.emit_store_arg(func, 1, &args[1]);
                            self.emit_memcall(func, "thread_parallel_filter", 2);
                        }
                        "spawn" if !args.is_empty() => {
                            self.emit_frame_begin(func, 1);
                            self.emit_store_arg(func, 0, &args[0]);
                            self.emit_memcall(func, "thread_spawn", 1);
                        }
                        _ => {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    },
                    _ => {
                        // Handle instance method calls on objects
                        if let Some(obj) = object {
                            self.emit_expr(func, obj);
                            match method.as_str() {
                                // String instance methods
                                "charAt" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_char_at", 2);
                                }
                                "substring" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_substring", 3);
                                }
                                "indexOf" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_index_of", 2);
                                }
                                "slice" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_slice", 3);
                                }
                                "toLowerCase" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "string_to_lower_case", 1);
                                }
                                "toUpperCase" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "string_to_upper_case", 1);
                                }
                                "trim" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "string_trim", 1);
                                }
                                "includes" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_includes", 2);
                                    func.instruction(&Instruction::If(
                                        wasm_encoder::BlockType::Result(ValType::I64),
                                    ));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "startsWith" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_starts_with", 2);
                                    func.instruction(&Instruction::If(
                                        wasm_encoder::BlockType::Result(ValType::I64),
                                    ));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "endsWith" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_ends_with", 2);
                                    func.instruction(&Instruction::If(
                                        wasm_encoder::BlockType::Result(ValType::I64),
                                    ));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "replace" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_replace", 3);
                                }
                                "split" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_split", 2);
                                }
                                "repeat" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_repeat", 2);
                                }
                                "padStart" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_pad_start", 3);
                                }
                                "padEnd" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_pad_end", 3);
                                }
                                // Array instance methods called via NativeMethodCall
                                "push" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_push", 2);
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_length", 1);
                                }
                                "pop" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_pop", 1);
                                }
                                "shift" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_shift", 1);
                                }
                                "unshift" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_void(func, "array_unshift", 2);
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                "join" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        let comma_id =
                                            self.emitter.string_map.get(",").copied().unwrap_or(0);
                                        let bits = (STRING_TAG << 48) | (comma_id as u64);
                                        func.instruction(&Instruction::I64Const(bits as i64));
                                    }
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_join", 2);
                                }
                                "map" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_map", 2);
                                }
                                "filter" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_filter", 2);
                                }
                                "forEach" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_void(func, "array_for_each", 2);
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                "find" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_find", 2);
                                }
                                "findIndex" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_find_index", 2);
                                }
                                "reduce" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    if args.len() >= 2 {
                                        self.emit_expr(func, &args[1]);
                                    } else {
                                        func.instruction(&Instruction::I64Const(
                                            TAG_UNDEFINED as i64,
                                        ));
                                    }
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 2);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_reduce", 3);
                                }
                                "sort" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        func.instruction(&Instruction::I64Const(
                                            TAG_UNDEFINED as i64,
                                        ));
                                    }
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_sort", 2);
                                }
                                "reverse" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_reverse", 1);
                                }
                                "concat" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_concat", 2);
                                }
                                "flat" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_flat", 1);
                                }
                                "length" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "array_length", 1);
                                }
                                // Response methods
                                "json" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "response_json", 1);
                                }
                                "text" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "response_text", 1);
                                }
                                "status" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "response_status", 1);
                                }
                                _ => {
                                    // Fall back to class_call_method via mem_call
                                    let method_id = self
                                        .emitter
                                        .string_map
                                        .get(method.as_str())
                                        .copied()
                                        .unwrap_or(0);
                                    // obj is already on the stack from emit_expr(obj) above
                                    // Save obj to temp, build args array, then store all to memory
                                    func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 0 = obj handle
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    let method_bits = (STRING_TAG << 48) | (method_id as u64);
                                    self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                                                                                                 // Build args array
                                    self.emit_frame_begin(func, 0);
                                    self.emit_memcall(func, "array_new", 0); // get new array handle
                                    for arg in args {
                                        self.emit_frame_begin(func, 2);
                                        func.instruction(&Instruction::LocalSet(self.temp_local));
                                        self.emit_slot_addr(func, 0);
                                        func.instruction(&Instruction::LocalGet(self.temp_local));
                                        func.instruction(&Instruction::I64Store(
                                            wasm_encoder::MemArg {
                                                offset: 0,
                                                align: 3,
                                                memory_index: 0,
                                            },
                                        ));
                                        self.emit_store_arg(func, 1, arg);
                                        self.emit_memcall(func, "array_push", 2);
                                        // push into array, returns handle
                                    }
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array handle
                                    self.emit_slot_addr(func, 2);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                    self.emit_memcall(func, "class_call_method", 3);
                                }
                            }
                        } else {
                            // No object — module-level function
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                }
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

            // --- Math ---
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

            // --- Typeof ---
            Expr::TypeOf(operand) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, operand);
                self.emit_memcall(func, "js_typeof", 1);
            }

            // --- Async ---
            Expr::Await(e) => {
                // Evaluate inner expression, then call await_promise bridge
                // If the value is a promise handle, tries to get resolved value
                // If not a promise, returns the value as-is
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, e);
                self.emit_memcall(func, "await_promise", 1);
            }

            // --- Object literal ---
            Expr::Object(fields) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                // Stack: [handle as i64]
                for (key, val) in fields {
                    // object_set(handle, key, value) returns handle (chaining)
                    let key_id = self
                        .emitter
                        .string_map
                        .get(key.as_str())
                        .copied()
                        .unwrap_or(0);
                    let key_bits = (STRING_TAG << 48) | (key_id as u64);
                    // Save handle from stack to temp_local, then store via emit_slot_addr
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_frame_begin(func, 3);
                    // Store handle to slot 0
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    // Store key string to slot 1
                    self.emit_slot_addr(func, 1);
                    func.instruction(&Instruction::I64Const(key_bits as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    // Store value to slot 2
                    self.emit_store_arg(func, 2, val);
                    self.emit_memcall(func, "object_set", 3);
                }
                // Handle is on stack from last object_set (or object_new if no fields)
            }

            // --- Object spread ---
            Expr::ObjectSpread { parts } => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                for (key_opt, val) in parts {
                    if let Some(key) = key_opt {
                        let key_id = self
                            .emitter
                            .string_map
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_store_arg(func, 2, val);
                        self.emit_memcall(func, "object_set", 3);
                    } else {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_arg(func, 1, val);
                        self.emit_memcall(func, "object_assign", 2);
                    }
                }
            }

            // --- Array literal ---
            Expr::Array(elements) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for elem in elements {
                    self.emit_frame_begin(func, 2);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_store_arg(func, 1, elem);
                    self.emit_memcall(func, "array_push", 2);
                }
            }

            // --- Array spread ---
            Expr::ArraySpread(elements) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => {
                            self.emit_frame_begin(func, 2);
                            func.instruction(&Instruction::LocalSet(self.temp_local));
                            self.emit_slot_addr(func, 0);
                            func.instruction(&Instruction::LocalGet(self.temp_local));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            self.emit_store_arg(func, 1, e);
                            self.emit_memcall(func, "array_push", 2);
                        }
                        ArrayElement::Spread(e) => {
                            self.emit_frame_begin(func, 2);
                            func.instruction(&Instruction::LocalSet(self.temp_local));
                            self.emit_slot_addr(func, 0);
                            func.instruction(&Instruction::LocalGet(self.temp_local));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            self.emit_store_arg(func, 1, e);
                            self.emit_memcall(func, "array_push_spread", 2);
                        }
                    }
                }
            }

            // --- Property access ---
            Expr::PropertyGet { object, property } => {
                // Special case: .length uses string_len which handles both strings and arrays
                if property == "length" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "string_len", 1);
                    return;
                }
                // Special case: .message on error objects
                if property == "message" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "error_message", 1);
                    return;
                }
                self.emit_expr(func, object);
                let key_id = self
                    .emitter
                    .string_map
                    .get(property.as_str())
                    .copied()
                    .unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Use class_get_field (works for both plain objects and class instances)
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_memcall(func, "class_get_field", 2);
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                self.emit_expr(func, object);
                let key_id = self
                    .emitter
                    .string_map
                    .get(property.as_str())
                    .copied()
                    .unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Use class_set_field (works for both plain objects and class instances)
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "class_set_field", 3);
                // class_set_field is void; push the object back for chaining
                self.emit_expr(func, object);
            }
            Expr::PropertyUpdate {
                object,
                property,
                op,
                prefix,
            } => {
                // obj.prop++ or ++obj.prop
                self.emit_expr(func, object);
                let key_id = self
                    .emitter
                    .string_map
                    .get(property.as_str())
                    .copied()
                    .unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Get current value
                // We need the object handle twice. Can't dup in WASM without locals.
                // For simplicity: re-emit object (works if object is a simple expression)
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_memcall(func, "object_get", 2);
                // Stack: [old_value_i64]
                if *prefix {
                    func.instruction(&Instruction::F64ReinterpretI64);
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                    func.instruction(&Instruction::I64ReinterpretF64);
                    // Set new value
                    self.emit_expr(func, object);
                    func.instruction(&Instruction::I64Const(key_bits as i64));
                    // Stack: [new_val, handle, key] — wrong order for object_set(handle, key, val)
                    // We need to restructure. For now, just emit the value (prefix returns new)
                    // This is imprecise but works for basic cases
                } else {
                    // postfix: return old, then update
                    // For now, just do the increment and return new value (approximate)
                    func.instruction(&Instruction::F64ReinterpretI64);
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }

            // --- Index access ---
            Expr::IndexGet { object, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "object_get_dynamic", 2);
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "object_set_dynamic", 3);
                // set_dynamic is void; push undefined as expression result
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::IndexUpdate {
                object,
                index,
                op,
                prefix: _,
            } => {
                // Approximate: get, increment, set
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "object_get_dynamic", 2);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&f64_const(1.0));
                match op {
                    BinaryOp::Add => func.instruction(&Instruction::F64Add),
                    BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                    _ => func.instruction(&Instruction::F64Add),
                };
                func.instruction(&Instruction::I64ReinterpretF64);
            }

            // --- Object/Array methods ---
            Expr::ObjectKeys(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_keys", 1);
            }
            Expr::ObjectValues(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_values", 1);
            }
            Expr::ObjectEntries(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_entries", 1);
            }
            Expr::ObjectRest { object, .. } => {
                // For now, just return a copy of the object (approximate)
                self.emit_expr(func, object);
            }
            Expr::Delete(expr) => match expr.as_ref() {
                Expr::PropertyGet { object, property } => {
                    self.emit_expr(func, object);
                    let key_id = self
                        .emitter
                        .string_map
                        .get(property.as_str())
                        .copied()
                        .unwrap_or(0);
                    let key_bits = (STRING_TAG << 48) | (key_id as u64);
                    self.emit_frame_begin(func, 2);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_store_const(func, 1, f64::from_bits(key_bits));
                    self.emit_memcall_void(func, "object_delete", 2);
                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                }
                Expr::IndexGet { object, index } => {
                    self.emit_frame_begin(func, 2);
                    self.emit_store_arg(func, 0, object);
                    self.emit_store_arg(func, 1, index);
                    self.emit_memcall_void(func, "object_delete_dynamic", 2);
                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                }
                _ => {
                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                }
            },
            Expr::In { property, object } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, property);
                self.emit_memcall_i32(func, "object_has_property", 2);
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Array methods (HIR-level) ---
            Expr::ArrayPush { array_id, value } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_arg(func, 1, value);
                // array_push returns handle, but ArrayPush typically returns new length
                // The bridge returns the array handle. We need to store back and return length.
                // For now, return the result of array_push (the handle).
                // Actually, drop result and push the new length
                self.emit_memcall(func, "array_push", 2);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "array_length", 1);
            }
            Expr::ArrayPushSpread { array_id, source } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_arg(func, 1, source);
                self.emit_memcall(func, "array_push_spread", 2);
                // Returns handle
            }
            Expr::ArrayPop(array_id) => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "array_pop", 1);
            }
            Expr::ArrayShift(array_id) => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "array_shift", 1);
            }
            Expr::ArrayUnshift { array_id, value } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "array_unshift", 2);
                // void return, push length
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "array_length", 1);
            }
            Expr::ArraySlice { array, start, end } => {
                self.emit_expr(func, array);
                self.emit_expr(func, start);
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "array_slice", 3);
            }
            Expr::ArraySplice {
                array_id,
                start,
                delete_count,
                items,
            } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_expr(func, start);
                if let Some(dc) = delete_count {
                    self.emit_expr(func, dc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "array_splice", 3);
                // Returns removed elements array handle
                // TODO: insert items if present
                let _ = items;
            }
            Expr::ArrayJoin { array, separator } => {
                self.emit_expr(func, array);
                if let Some(sep) = separator {
                    self.emit_expr(func, sep);
                } else {
                    // Default separator: ","
                    let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                    let comma_bits = (STRING_TAG << 48) | (comma_id as u64);
                    func.instruction(&Instruction::I64Const(comma_bits as i64));
                }
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
                self.emit_memcall(func, "array_join", 2);
            }
            Expr::ArrayIndexOf { array, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "array_index_of", 2);
            }
            Expr::ArrayIncludes { array, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_i32(func, "array_includes", 2);
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFlat { array } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_flat", 1);
            }
            Expr::ArrayIsArray(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "array_is_array", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFrom(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "array_from", 1);
            }
            Expr::ArrayFromMapped { iterable, map_fn } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, iterable);
                self.emit_store_arg(func, 1, map_fn);
                self.emit_memcall(func, "array_from_mapped", 2);
            }

            // --- Array higher-order methods ---
            Expr::ArrayMap { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_map", 2);
            }
            Expr::ArrayFilter { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_filter", 2);
            }
            Expr::ArrayForEach { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall_void(func, "array_for_each", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::ArrayFind { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_find", 2);
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_find_index", 2);
            }
            Expr::ArraySort { array, comparator } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, comparator);
                self.emit_memcall(func, "array_sort", 2);
            }
            Expr::ArrayReduce {
                array,
                callback,
                initial,
            }
            | Expr::ArrayReduceRight {
                array,
                callback,
                initial,
            } => {
                let is_right = matches!(expr, Expr::ArrayReduceRight { .. });
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                if let Some(init) = initial {
                    self.emit_expr(func, init);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                let name = if is_right {
                    "array_reduce_right"
                } else {
                    "array_reduce"
                };
                self.emit_memcall(func, name, 3);
            }
            Expr::ArrayToReversed { array } => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_to_reversed", 1);
            }
            Expr::ArrayToSorted { array, comparator } => {
                if let Some(cmp) = comparator {
                    self.emit_store_arg(func, 0, array);
                    self.emit_store_arg(func, 1, cmp);
                    self.emit_memcall(func, "array_to_sorted_cmp", 2);
                } else {
                    self.emit_store_arg(func, 0, array);
                    self.emit_memcall(func, "array_to_sorted", 1);
                }
            }
            Expr::ArrayToSpliced {
                array,
                start,
                delete_count,
                items: _,
            } => {
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, start);
                self.emit_store_arg(func, 2, delete_count);
                // items passed as count only for now (WASM doesn't support varargs easily)
                self.emit_memcall(func, "array_to_spliced", 3);
            }
            Expr::ArrayWith {
                array,
                index,
                value,
            } => {
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall(func, "array_with", 3);
            }
            Expr::ArrayCopyWithin {
                array_id,
                target,
                start,
                end,
            } => {
                // emit local get for array_id
                func.instruction(&Instruction::LocalGet(*array_id));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_arg(func, 1, target);
                self.emit_store_arg(func, 2, start);
                if let Some(e) = end {
                    self.emit_store_arg(func, 3, e);
                    self.emit_memcall(func, "array_copy_within", 4);
                } else {
                    self.emit_memcall(func, "array_copy_within", 3);
                }
            }
            Expr::ArrayEntries(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_entries", 1);
            }
            Expr::ArrayKeys(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_keys", 1);
            }
            Expr::ArrayValues(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_values", 1);
            }

            // --- Closure ---
            Expr::Closure {
                func_id,
                params,
                body,
                captures,
                mutable_captures,
                ..
            } => {
                // Compile closure body as a function (it was already registered if it's in module.functions)
                // If not registered, we need to handle it inline
                if let Some(&func_idx) = self.emitter.func_map.get(func_id) {
                    // Function is registered, create closure handle
                    // Use table index, not raw WASM function index
                    let table_idx = self
                        .emitter
                        .func_to_table_idx
                        .get(&func_idx)
                        .copied()
                        .unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, captures.len() as f64);
                    self.emit_memcall(func, "closure_new", 2);
                    // Set captures
                    for (i, cap_id) in captures.iter().chain(mutable_captures.iter()).enumerate() {
                        // Duplicate closure handle (it's returned by closure_new)
                        // closure_set_capture(handle, idx, value) -> handle (chaining)
                        func.instruction(&f64_const(i as f64));
                        func.instruction(&Instruction::I64ReinterpretF64);
                        if let Some(&local_idx) = self.local_map.get(cap_id) {
                            func.instruction(&Instruction::LocalGet(local_idx));
                        } else {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 2);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
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
                        self.emit_memcall(func, "closure_set_capture", 3);
                    }
                } else {
                    // Inline closure — not in function table, push undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                let _ = (params, body);
            }
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.emitter.func_map.get(id) {
                    // Create a closure wrapper with 0 captures for function reference
                    let table_idx = self
                        .emitter
                        .func_to_table_idx
                        .get(&func_idx)
                        .copied()
                        .unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, 0.0);
                    self.emit_memcall(func, "closure_new", 2);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // Issue #1071: an `ExternFuncRef` used as a value can resolve
                // either to (a) a cross-module function — wrap as closure with
                // 0 captures, or (b) a cross-module exported variable — read
                // the source module's promoted-let global directly. Variables
                // win when both apply (a let with the same name is closer to
                // the user's intent than a like-named function); in practice
                // the lookup tables are disjoint because a HIR symbol can
                // only be one or the other.
                let mod_key = (self.emitter.current_mod_idx, name.clone());
                if let Some(&gidx) = self.emitter.imported_var_globals.get(&mod_key) {
                    func.instruction(&Instruction::GlobalGet(gidx));
                } else if let Some(&func_idx) = self.emitter.func_name_map.get(name) {
                    // Create a closure wrapper with 0 captures (like FuncRef)
                    let table_idx = self
                        .emitter
                        .func_to_table_idx
                        .get(&func_idx)
                        .copied()
                        .unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, 0.0);
                    self.emit_memcall(func, "closure_new", 2);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }

            // --- Class instantiation ---
            Expr::New {
                class_name, args, ..
            } => {
                // Handle built-in constructors that need native JS objects
                match class_name.as_str() {
                    "RegExp" if !args.is_empty() => {
                        self.emit_expr(func, &args[0]);
                        if args.len() >= 2 {
                            self.emit_expr(func, &args[1]);
                        } else {
                            // Empty flags string
                            let empty_id = self.emitter.string_map.get("").copied().unwrap_or(0);
                            let empty_bits = (STRING_TAG << 48) | (empty_id as u64);
                            func.instruction(&Instruction::I64Const(empty_bits as i64));
                        }
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
                        self.emit_memcall(func, "regexp_new", 2);
                        return;
                    }
                    "Error" => {
                        if let Some(msg) = args.first() {
                            self.emit_expr(func, msg);
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
                        return;
                    }
                    "Date" => {
                        if let Some(arg) = args.first() {
                            self.emit_expr(func, arg);
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
                        return;
                    }
                    "Map" => {
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "map_new", 0);
                        return;
                    }
                    "Set" => {
                        if let Some(arg) = args.first() {
                            self.emit_frame_begin(func, 1);
                            self.emit_store_arg(func, 0, arg);
                            self.emit_memcall(func, "set_new_from_array", 1);
                        } else {
                            self.emit_frame_begin(func, 0);
                            self.emit_memcall(func, "set_new", 0);
                        }
                        return;
                    }
                    "URL" => {
                        if let Some(arg) = args.first() {
                            self.emit_expr(func, arg);
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
                        self.emit_memcall(func, "url_parse", 1);
                        return;
                    }
                    _ => {}
                }

                // User-defined class instantiation
                let class_name_id = self
                    .emitter
                    .string_map
                    .get(class_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_name_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, args.len() as f64);
                self.emit_memcall(func, "class_new", 2);
                // Call the compiled constructor if it exists
                if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(class_name.as_str()) {
                    // Stack: [instance_handle]
                    for arg in args {
                        self.emit_expr(func, arg);
                    }
                    // Keep the operand stack aligned with the ctor's arity: pad
                    // missing optional args with `undefined`, and drop excess
                    // evaluated args so they don't outlive the `call` and
                    // accumulate on the enclosing block's stack (#183).
                    if let Some(&expected) = self.emitter.func_param_counts.get(&ctor_idx) {
                        let provided = args.len() + 1;
                        for _ in provided..expected {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        for _ in expected..provided {
                            func.instruction(&Instruction::Drop);
                        }
                    }
                    func.instruction(&Instruction::Call(ctor_idx));
                }
                // If no compiled constructor, just leave the instance handle on stack
            }
            Expr::NewDynamic { callee, args } => {
                // Dynamic new — approximate with regular call via mem_call
                self.emit_frame_begin(func, (args.len() + 1) as u32);
                self.emit_store_arg(func, 0, callee);
                for (i, arg) in args.iter().enumerate() {
                    self.emit_store_arg(func, (i + 1) as u32, arg);
                }
                match args.len() {
                    0 => {
                        self.emit_memcall(func, "closure_call_0", 1);
                    }
                    1 => {
                        self.emit_memcall(func, "closure_call_1", 2);
                    }
                    2 => {
                        self.emit_memcall(func, "closure_call_2", 3);
                    }
                    3 => {
                        self.emit_memcall(func, "closure_call_3", 4);
                    }
                    _ => {
                        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    }
                }
            }
            Expr::This => {
                // 'this' is passed as first parameter (local 0) in methods
                func.instruction(&Instruction::LocalGet(0));
            }
            Expr::SuperCall(args) => {
                // Call parent constructor: super(args)
                // this is local 0 in the current constructor
                let mut called = false;
                if let Some(ref current_class) = self.current_class {
                    // Look up parent class name
                    if let Some(parent_name) = self.emitter.class_parent_map.get(current_class) {
                        if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(parent_name) {
                            // Call parent constructor with this + args
                            func.instruction(&Instruction::LocalGet(0)); // this
                            for arg in args {
                                self.emit_expr(func, arg);
                            }
                            if let Some(&expected) = self.emitter.func_param_counts.get(&ctor_idx) {
                                let provided = args.len() + 1;
                                for _ in provided..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                for _ in expected..provided {
                                    func.instruction(&Instruction::Drop);
                                }
                            }
                            func.instruction(&Instruction::Call(ctor_idx));
                            func.instruction(&Instruction::Drop); // parent ctor returns this, discard
                            called = true;
                        }
                    }
                }
                if !called {
                    // No parent constructor found, drop args
                    for arg in args {
                        self.emit_expr(func, arg);
                        func.instruction(&Instruction::Drop);
                    }
                }
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::SuperMethodCall { method, args } => {
                // Call parent method on this via class_call_method (walks parent chain)
                self.emit_slot_addr(func, 0); // this handle
                func.instruction(&Instruction::LocalGet(0));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                })); // slot 0 = this (already i64)
                let method_id = self
                    .emitter
                    .string_map
                    .get(method.as_str())
                    .copied()
                    .unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                                                                             // Build args array
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for arg in args {
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
                    self.emit_memcall(func, "array_push", 2);
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "class_call_method", 3);
            }
            Expr::ClassRef(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::StaticFieldGet {
                class_name,
                field_name,
            } => {
                let class_id = self
                    .emitter
                    .string_map
                    .get(class_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self
                    .emitter
                    .string_map
                    .get(field_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, f64::from_bits(field_bits));
                self.emit_memcall(func, "class_get_static", 2);
            }
            Expr::StaticFieldSet {
                class_name,
                field_name,
                value,
            } => {
                let class_id = self
                    .emitter
                    .string_map
                    .get(class_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self
                    .emitter
                    .string_map
                    .get(field_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                self.emit_frame_begin(func, 3);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, f64::from_bits(field_bits));
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "class_set_static", 3);
                // void return, push the value back
                self.emit_expr(func, value);
            }
            Expr::StaticMethodCall {
                class_name,
                method_name,
                args,
            } => {
                // Try to call compiled static method directly
                if let Some(statics) = self.emitter.class_static_map.get(class_name.as_str()) {
                    if let Some(&static_idx) = statics.get(method_name.as_str()) {
                        // Direct call to compiled static method (no this param).
                        // Same arity reconciliation as FuncRef/ExternFuncRef arms
                        // (#183): pad-up for missing args, drop-excess for extras.
                        for arg in args {
                            self.emit_expr(func, arg);
                        }
                        if let Some(&expected) = self.emitter.func_param_counts.get(&static_idx) {
                            for _ in args.len()..expected {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                            for _ in expected..args.len() {
                                func.instruction(&Instruction::Drop);
                            }
                        }
                        func.instruction(&Instruction::Call(static_idx));
                        return;
                    }
                }
                // Fallback: bridge dispatch via mem_call
                let class_id = self
                    .emitter
                    .string_map
                    .get(class_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let method_id = self
                    .emitter
                    .string_map
                    .get(method_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                self.emit_store_const(func, 0, f64::from_bits(class_bits)); // slot 0 = class handle
                self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                                                                             // Build args array
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for arg in args {
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
                    self.emit_memcall(func, "array_push", 2);
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "class_call_method", 3);
            }

            // --- Enum members ---
            Expr::EnumMember {
                enum_name,
                member_name,
            } => {
                // Look up resolved value from enum definitions
                let key = (enum_name.clone(), member_name.clone());
                if let Some(resolved) = self.emitter.enum_values.get(&key) {
                    match resolved.clone() {
                        EnumResolvedValue::Number(n) => {
                            func.instruction(&f64_const(n));
                            func.instruction(&Instruction::I64ReinterpretF64);
                        }
                        EnumResolvedValue::String(s) => {
                            let id = self
                                .emitter
                                .string_map
                                .get(s.as_str())
                                .copied()
                                .unwrap_or(0);
                            let bits = (STRING_TAG << 48) | (id as u64);
                            func.instruction(&Instruction::I64Const(bits as i64));
                        }
                    }
                } else if let Ok(n) = member_name.parse::<f64>() {
                    func.instruction(&f64_const(n));
                    func.instruction(&Instruction::I64ReinterpretF64);
                } else {
                    // Fallback: return the member name as a string
                    let id = self
                        .emitter
                        .string_map
                        .get(member_name.as_str())
                        .copied()
                        .unwrap_or(0);
                    let bits = (STRING_TAG << 48) | (id as u64);
                    func.instruction(&Instruction::I64Const(bits as i64));
                }
            }

            // --- InstanceOf ---
            Expr::InstanceOf { expr, ty, .. } => {
                self.emit_expr(func, expr);
                let type_id = self
                    .emitter
                    .string_map
                    .get(ty.as_str())
                    .copied()
                    .unwrap_or(0);
                let type_bits = (STRING_TAG << 48) | (type_id as u64);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(type_bits));
                self.emit_memcall_i32(func, "class_instanceof", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Void ---
            Expr::Void(e) => {
                self.emit_expr(func, e);
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- String methods ---
            Expr::StringSplit(string, delim) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, delim);
                self.emit_memcall(func, "string_split", 2);
            }
            Expr::StringFromCharCode(code) => {
                // Bridge name is the key in __memDispatch (wasm_runtime.js) — keep
                // camelCase even though Rust prefers snake_case; no dispatch entry
                // means mem_call silently falls through to __classDispatch.
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_fromCharCode", 1);
            }
            Expr::StringFromCodePoint(code) => {
                // WASM stub: same as fromCharCode for now (BMP-only).
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_fromCharCode", 1);
            }
            Expr::StringAt { string, index } => {
                // WASM stub: alias to char_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_at", 2);
            }
            Expr::StringCodePointAt { string, index } => {
                // WASM stub: alias to char_code_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_code_at", 2);
            }
            Expr::StringMatch { string, regex } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, regex);
                self.emit_memcall(func, "string_match", 2);
            }
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, pattern);
                self.emit_store_arg(func, 2, replacement);
                self.emit_memcall(func, "string_replace", 3);
            }
            Expr::StringCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "jsvalue_to_string", 1);
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_parse", 1);
            }
            Expr::JsonStringify(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_stringify", 1);
            }

            // --- Map ---
            Expr::MapNew => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "map_new", 0);
            }
            Expr::MapNewFromArray(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "map_new_from_array", 1);
            }
            Expr::MapSet { map, key, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "map_set", 3);
                // void return, push the map back
                self.emit_expr(func, map);
            }
            Expr::MapGet { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall(func, "map_get", 2);
            }
            Expr::MapHas { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall_i32(func, "map_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::MapDelete { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall_void(func, "map_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::MapSize(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_size", 1);
            }
            Expr::MapClear(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall_void(func, "map_clear", 1);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::MapEntries(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_entries", 1);
            }
            Expr::MapKeys(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_keys", 1);
            }
            Expr::MapValues(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_values", 1);
            }

            // --- Set ---
            Expr::SetNew => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "set_new", 0);
            }
            Expr::SetNewFromArray(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "set_new_from_array", 1);
            }
            Expr::SetAdd { set_id, value } => {
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "set_add", 2);
                // void, push set back
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::SetHas { set, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, set);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_i32(func, "set_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::SetDelete { set, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, set);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "set_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::SetSize(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall(func, "set_size", 1);
            }
            Expr::SetClear(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall_void(func, "set_clear", 1);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::SetValues(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall(func, "set_values", 1);
            }

            // --- Date ---
            // WASM target only handles the 0/1-arg forms. The multi-arg
            // `new Date(year, month, ...)` form (used by dayjs) is not
            // supported on this target; we pass the first arg only.
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
            Expr::DateSetUtcFullYear { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_full_year", 2);
            }
            Expr::DateSetUtcMonth { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_month", 2);
            }
            Expr::DateSetUtcDate { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_date", 2);
            }
            Expr::DateSetUtcHours { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_hours", 2);
            }
            Expr::DateSetUtcMinutes { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_minutes", 2);
            }
            Expr::DateSetUtcSeconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_seconds", 2);
            }
            Expr::DateSetUtcMilliseconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_milliseconds", 2);
            }
            Expr::DateSetFullYear { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_full_year", 2);
            }
            Expr::DateSetMonth { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_month", 2);
            }
            Expr::DateSetDate { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_date", 2);
            }
            Expr::DateSetHours { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_hours", 2);
            }
            Expr::DateSetMinutes { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_minutes", 2);
            }
            Expr::DateSetSeconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_seconds", 2);
            }
            Expr::DateSetMilliseconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_milliseconds", 2);
            }
            Expr::DateSetTime { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
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
            Expr::TypeErrorNew(msg)
            | Expr::RangeErrorNew(msg)
            | Expr::ReferenceErrorNew(msg)
            | Expr::SyntaxErrorNew(msg) => {
                // WASM stub: alias to error_new
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, msg);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::AggregateErrorNew { errors: _, message } => {
                // WASM stub: alias to error_new (drops errors array)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }

            // --- RegExp ---
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
            Expr::RegExpTest { regex, string } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, regex);
                self.emit_store_arg(func, 1, string);
                self.emit_memcall_i32(func, "regexp_test", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Global builtins ---
            Expr::ParseInt { string, radix } => {
                self.emit_expr(func, string);
                let _ = radix; // TODO: radix support
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "parse_int", 1);
            }
            Expr::ParseFloat(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "parse_float", 1);
            }
            Expr::NumberCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "number_coerce", 1);
            }
            Expr::IsNaN(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "is_nan", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::IsUndefinedOrBareNan(val) => {
                // WASM fallback: delegate to is_nan (close enough for most cases)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "is_nan", 1);
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
                self.emit_memcall_i32(func, "is_finite", 1);
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

            // --- Math extra ---
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

            // --- URL ---
            Expr::UrlNew { url, base } => {
                self.emit_expr(func, url);
                if let Some(b) = base {
                    // URL(url, base) — for now just use url
                    self.emit_expr(func, b);
                    func.instruction(&Instruction::Drop);
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
                self.emit_memcall(func, "url_parse", 1);
            }
            Expr::UrlGetHref(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_href", 1);
            }
            Expr::UrlGetPathname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_pathname", 1);
            }
            Expr::UrlGetProtocol(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_protocol", 1);
            }
            Expr::UrlGetHost(u) | Expr::UrlGetHostname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hostname", 1);
            }
            Expr::UrlGetPort(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_port", 1);
            }
            Expr::UrlGetSearch(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search", 1);
            }
            Expr::UrlGetHash(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hash", 1);
            }
            Expr::UrlGetOrigin(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_origin", 1);
            }
            Expr::UrlGetSearchParams(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search_params", 1);
            }

            // --- Process/OS ---
            Expr::ProcessArgv => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_argv", 0);
            }
            Expr::ProcessCwd => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_cwd", 0);
            }
            Expr::OsPlatform => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "os_platform", 0);
            }
            Expr::ProcessUptime
            | Expr::ProcessMemoryUsage
            | Expr::ProcessPid
            | Expr::ProcessPpid
            | Expr::ProcessVersion
            | Expr::ProcessVersions
            | Expr::ProcessHrtimeBigint
            | Expr::ProcessStdin
            | Expr::ProcessStdout
            | Expr::ProcessStderr
            | Expr::OsArch
            | Expr::OsHostname
            | Expr::OsHomedir
            | Expr::OsTmpdir
            | Expr::OsTotalmem
            | Expr::OsFreemem
            | Expr::OsUptime
            | Expr::OsType
            | Expr::OsRelease
            | Expr::OsCpus
            | Expr::OsNetworkInterfaces
            | Expr::OsUserInfo
            | Expr::OsEOL => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::ProcessNextTick(_)
            | Expr::ProcessChdir(_)
            | Expr::ProcessOn { .. }
            | Expr::ProcessKill { .. }
            | Expr::ProcessExit(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- FS stubs ---
            Expr::FsReadFileSync(_)
            | Expr::FsWriteFileSync(_, _)
            | Expr::FsExistsSync(_)
            | Expr::FsMkdirSync(_)
            | Expr::FsUnlinkSync(_)
            | Expr::FsAppendFileSync(_, _)
            | Expr::FsReadFileBinary(_)
            | Expr::FsRmRecursive(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Path ---
            Expr::PathJoin(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_join", 2);
            }
            Expr::PathWin32Join(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_win32_join", 2);
            }
            Expr::PathDirname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_dirname", 1);
            }
            Expr::PathBasename(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_basename", 1);
            }
            Expr::PathExtname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_extname", 1);
            }
            Expr::PathResolve(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_resolve", 1);
            }
            Expr::PathIsAbsolute(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall_i32(func, "path_is_absolute", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::FileURLToPath(p) => {
                self.emit_expr(func, p);
                // In WASM, just return the string as-is
            }
            Expr::PathRelative(from, to) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, from);
                self.emit_store_arg(func, 1, to);
                self.emit_memcall(func, "path_relative", 2);
            }
            Expr::PathNormalize(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_normalize", 1);
            }
            Expr::PathParse(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_parse", 1);
            }
            Expr::PathFormat(o) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, o);
                self.emit_memcall(func, "path_format", 1);
            }
            Expr::PathBasenameExt(p, ext) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, p);
                self.emit_store_arg(func, 1, ext);
                self.emit_memcall(func, "path_basename", 2);
            }
            Expr::PathSep => {
                self.emit_memcall(func, "path_sep", 0);
            }
            Expr::PathDelimiter => {
                self.emit_memcall(func, "path_delimiter", 0);
            }
            Expr::PathToNamespacedPath(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_to_namespaced_path", 1);
            }
            Expr::PathMatchesGlob(p, pat) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, p);
                self.emit_store_arg(func, 1, pat);
                self.emit_memcall(func, "path_matches_glob", 2);
            }
            Expr::PathResolveJoin(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_resolve_join", 2);
            }
            // --- WeakRef and FinalizationRegistry (stub: routes to host runtime) ---
            Expr::WeakRefNew(target) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, target);
                self.emit_memcall(func, "weakref_new", 1);
            }
            Expr::WeakRefDeref(weakref_expr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, weakref_expr);
                self.emit_memcall(func, "weakref_deref", 1);
            }
            Expr::FinalizationRegistryNew(callback) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, callback);
                self.emit_memcall(func, "finreg_new", 1);
            }
            Expr::FinalizationRegistryRegister {
                registry,
                target,
                held,
                token,
            } => {
                self.emit_frame_begin(func, 4);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, target);
                self.emit_store_arg(func, 2, held);
                if let Some(t) = token {
                    self.emit_store_arg(func, 3, t);
                } else {
                    self.emit_slot_addr(func, 3);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                self.emit_memcall(func, "finreg_register", 4);
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, token);
                self.emit_memcall(func, "finreg_unregister", 2);
            }
            // --- Buffer/TypedArray ---
            Expr::BufferAlloc { ref size, .. } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, size.as_ref());
                self.emit_memcall(func, "buffer_alloc", 1);
            }
            Expr::BufferAllocUnsafe(size) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, size);
                self.emit_memcall(func, "buffer_alloc", 1);
            }
            Expr::BufferFrom { data, encoding } => {
                self.emit_expr(func, data);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
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
                self.emit_memcall(func, "buffer_from_string", 2);
            }
            Expr::BufferToString { buffer, encoding } => {
                self.emit_expr(func, buffer);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
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
                self.emit_memcall(func, "buffer_to_string", 2);
            }
            Expr::BufferLength(buf) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, buf);
                self.emit_memcall(func, "buffer_length", 1);
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.emit_expr(func, buffer);
                if let Some(s) = start {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "buffer_slice", 3);
            }
            Expr::BufferConcat(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "buffer_concat", 1);
            }
            Expr::BufferIndexGet { buffer, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "buffer_get", 2);
            }
            Expr::BufferIndexSet {
                buffer,
                index,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "buffer_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::BufferCopy {
                source,
                target,
                target_start,
                source_start,
                source_end,
            } => {
                self.emit_expr(func, source);
                self.emit_expr(func, target);
                if let Some(ts) = target_start {
                    self.emit_expr(func, ts);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(ss) = source_start {
                    self.emit_expr(func, ss);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(se) = source_end {
                    self.emit_expr(func, se);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 5);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 4);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "buffer_copy", 5);
            }
            Expr::BufferWrite {
                buffer,
                string,
                offset,
                encoding,
            } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, string);
                if let Some(o) = offset {
                    self.emit_expr(func, o);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(e) = encoding {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "buffer_write", 4);
            }
            Expr::BufferEquals { buffer, other } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, other);
                self.emit_memcall_i32(func, "buffer_equals", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BufferIsBuffer(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "buffer_is_buffer", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BufferByteLength(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "buffer_byte_length", 1);
            }
            Expr::Uint8ArrayNew(size) => {
                if let Some(s) = size {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
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
                self.emit_memcall(func, "uint8array_new", 1);
            }
            Expr::Uint8ArrayFrom(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "uint8array_from", 1);
            }
            Expr::Uint8ArrayLength(buf) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, buf);
                self.emit_memcall(func, "uint8array_length", 1);
            }
            Expr::Uint8ArrayGet { array, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "uint8array_get", 2);
            }
            Expr::Uint8ArraySet {
                array,
                index,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "uint8array_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Child process stubs ---
            Expr::ChildProcessExecSync { .. }
            | Expr::ChildProcessSpawnSync { .. }
            | Expr::ChildProcessSpawn { .. }
            | Expr::ChildProcessExec { .. }
            | Expr::ChildProcessSpawnBackground { .. }
            | Expr::ChildProcessGetProcessStatus(_)
            | Expr::ChildProcessKillProcess(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Fetch ---
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
            } => {
                self.emit_expr(func, url);
                self.emit_expr(func, method);
                self.emit_expr(func, body);
                // Build headers object
                if headers.is_empty() {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                } else {
                    self.emit_frame_begin(func, 0);
                    self.emit_memcall(func, "object_new", 0);
                    for (key, val) in headers {
                        let key_id = self
                            .emitter
                            .string_map
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_store_arg(func, 2, val);
                        self.emit_memcall(func, "object_set", 3);
                    }
                }
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
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
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.emit_expr(func, url);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // method (default GET)
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // body
                                                                                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self
                    .emitter
                    .string_map
                    .get("Authorization")
                    .copied()
                    .unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
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
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchPostWithAuth {
                url,
                auth_header,
                body,
            } => {
                self.emit_expr(func, url);
                // POST method string
                let post_id = self.emitter.string_map.get("POST").copied().unwrap_or(0);
                let post_bits = (STRING_TAG << 48) | (post_id as u64);
                func.instruction(&Instruction::I64Const(post_bits as i64));
                self.emit_expr(func, body);
                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self
                    .emitter
                    .string_map
                    .get("Authorization")
                    .copied()
                    .unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
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
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            // --- Net stubs ---
            Expr::NetCreateServer { .. }
            | Expr::NetCreateConnection { .. }
            | Expr::NetConnect { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Crypto ---
            Expr::CryptoRandomUUID => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "crypto_random_uuid", 0);
            }
            Expr::CryptoRandomBytes(n) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, n);
                self.emit_memcall(func, "crypto_random_bytes", 1);
            }
            Expr::CryptoSha256(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_sha256", 1);
            }
            Expr::CryptoMd5(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_md5", 1);
            }
            // --- URL SearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(init_expr) = init {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, init_expr);
                    self.emit_memcall(func, "url_parse", 1);
                    self.emit_frame_begin(func, 1);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_memcall(func, "url_get_search_params", 1);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall(func, "searchparams_get", 2);
            }
            Expr::UrlSearchParamsHas {
                params,
                name,
                value: _,
            } => {
                // WASM backend doesn't yet model the 2-arg `has(name, value)`
                // variant — drops the optional value and falls back to the
                // 1-arg shape. Native LLVM backend is conformant.
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_i32(func, "searchparams_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::UrlSearchParamsSet {
                params,
                name,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsAppend {
                params,
                name,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_append", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsDelete {
                params,
                name,
                value: _,
            } => {
                // WASM backend doesn't yet model `delete(name, value)` —
                // value arg ignored, falls back to the 1-arg behavior.
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_void(func, "searchparams_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, params);
                self.emit_memcall(func, "searchparams_to_string", 1);
            }
            Expr::UrlSearchParamsGetAll { .. } | Expr::UrlSearchParamsEntries(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- JS runtime interop stubs ---
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

            // --- DateNow ---
            Expr::DateNow => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "date_now", 0);
            }

            // --- Sequence ---
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

            // --- Catch-all: emit undefined ---
            _ => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
        }
    }
}
