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
                    // Namespace-import member call (`import * as W from "./mod";
                    // W.fn(args)`): resolve to a DIRECT wasm call of the source
                    // module's function — the same lowering `fn(args)` gets via
                    // a named import. Without this the callee fell through to
                    // the class-dispatch fallback with an undefined receiver
                    // and silently returned undefined (never executing fn).
                    if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                        let key = (
                            self.emitter.current_mod_idx,
                            format!("{}.{}", name, property),
                        );
                        if let Some(&idx) = self.emitter.imported_ns_funcs.get(&key).copied().as_ref() {
                            for arg in args {
                                self.emit_expr(func, arg);
                            }
                            // Pad-up / drop-excess — see the FuncRef arm below (#183).
                            if let Some(&expected) = self.emitter.func_param_counts.get(&idx) {
                                for _ in args.len()..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                for _ in expected..args.len() {
                                    func.instruction(&Instruction::Drop);
                                }
                            }
                            func.instruction(&Instruction::Call(idx));
                            if self.emitter.void_funcs.contains(&idx) {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                            return true;
                        }
                    }
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

                // Built-in timer globals (setTimeout/setInterval/clearTimeout/
                // clearInterval). These resolve to `Expr::ExternFuncRef` with no
                // entry in `func_name_map`, so without this intercept they fall
                // through to the ExternFuncRef arm below, which drops the args and
                // pushes `undefined` — the timer is never scheduled and the
                // callback never fires (#1323). Route them through the mem_call
                // bridge to `__memDispatch.set_timeout` / `set_interval` /
                // `clear_timeout` / `clear_interval` in wasm_runtime.js, mirroring
                // how fetch/closure_call dispatch. The set_* bridges take
                // (closure, delay) and return the timer id; the clear_* bridges
                // take (id). Trailing setTimeout(fn, delay, ...args) extras are
                // dropped (Node passes them to the callback — out of scope here).
                if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                    let timer = match name.as_str() {
                        "setTimeout" => Some(("set_timeout", 2u32)),
                        "setInterval" => Some(("set_interval", 2u32)),
                        "clearTimeout" => Some(("clear_timeout", 1u32)),
                        "clearInterval" => Some(("clear_interval", 1u32)),
                        _ => None,
                    };
                    if let Some((bridge, arity)) = timer {
                        self.emit_frame_begin(func, arity);
                        for slot in 0..arity {
                            match args.get(slot as usize) {
                                Some(arg) => self.emit_store_arg(func, slot, arg),
                                None => {
                                    // Pad missing arg (e.g. setTimeout(fn)) with undefined.
                                    self.emit_slot_addr(func, slot);
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                    func.instruction(&Instruction::I64Store(
                                        wasm_encoder::MemArg {
                                            offset: 0,
                                            align: 3,
                                            memory_index: 0,
                                        },
                                    ));
                                }
                            }
                        }
                        // emit_memcall leaves the i64 result (timer id for set_*,
                        // NaN-boxed undefined for clear_*) on the stack, satisfying
                        // the call expression's value requirement.
                        self.emit_memcall(func, bridge, arity);
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
                        // Cross-module or FFI function call. The consumer's
                        // own import table wins (resolved through re-export
                        // chains); the whole-program name map is only a
                        // fallback, since its bare-name keys collide across
                        // modules. See FuncRef arm above for why both pad-up
                        // and drop-excess are required (#183).
                        let consumer_key =
                            (self.emitter.current_mod_idx, name.clone());
                        if let Some(&idx) = self
                            .emitter
                            .imported_func_indices
                            .get(&consumer_key)
                            .or_else(|| self.emitter.func_name_map.get(name))
                        {
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
