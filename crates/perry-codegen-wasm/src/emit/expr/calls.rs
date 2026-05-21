//! Generic function calls (Expr::Call) including method-call sugar.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_calls(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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
                                return true;
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_warn", 1);
                                }
                                return true;
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_error", 1);
                                }
                                return true;
                            }
                            _ => {}
                        }
                    }
                    // String/Array method calls: expr.method(args)
                    if self.emit_method_call(func, object, property, args) {
                        return true;
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
                        return true;
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

            _ => return false,
        }
        true
    }
}
