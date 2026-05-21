//! Native bridge calls (Expr::NativeMethodCall) - perry/ui, perry/system, console.*, math.*, etc.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_native_method(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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

            _ => return false,
        }
        true
    }
}
