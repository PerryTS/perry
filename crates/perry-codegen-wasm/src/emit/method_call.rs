//! Method-call emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `FuncEmitCtx::emit_method_call` onto a dedicated
//! `impl<'a> FuncEmitCtx<'a>` block.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    /// Try to emit a method call on an object expression.
    /// Returns true if handled, false if not recognized.
    /// All bridge calls go through WASM memory to avoid Firefox NaN canonicalization.
    pub(super) fn emit_method_call(
        &mut self,
        func: &mut Function,
        object: &Expr,
        method: &str,
        args: &[Expr],
    ) -> bool {
        match method {
            // String methods
            "charAt" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_charAt", 2);
                true
            }
            "substring" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_substring", 3);
                true
            }
            "indexOf" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_indexOf", 2);
                true
            }
            "slice" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_slice", 3);
                true
            }
            "toLowerCase" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_toLowerCase", 1);
                true
            }
            "toUpperCase" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_toUpperCase", 1);
                true
            }
            "trim" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_trim", 1);
                true
            }
            "includes" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_includes", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "startsWith" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_startsWith", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "endsWith" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_endsWith", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "replace" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_replace", 3);
                true
            }
            "split" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_split", 2);
                true
            }
            "repeat" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_repeat", 2);
                true
            }
            "padStart" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_padStart", 3);
                true
            }
            "padEnd" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_padEnd", 3);
                true
            }
            // Array methods
            "push" if !args.is_empty() => {
                let argc = 1 + args.len() as u32;
                self.emit_frame_begin(func, argc);
                self.emit_store_arg(func, 0, object);
                for (slot, arg) in (1..).zip(args) {
                    self.emit_store_arg(func, slot, arg);
                }
                self.emit_memcall(func, "array_push", argc);
                // result is the handle; now get length
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
                true
            }
            "pop" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_pop", 1);
                true
            }
            "shift" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_shift", 1);
                true
            }
            // Array `join` takes one separator; a call with more arguments is some other
            // object's `join` (`path.win32.join(a, b)` on a namespace value).
            // With no separator the bridge gets none, so an array joins with "," and a
            // namespace value's `join()` answers ".".
            "join" if args.len() <= 1 => {
                let argc = 1 + args.len() as u32;
                self.emit_frame_begin(func, argc);
                self.emit_store_arg(func, 0, object);
                if let Some(separator) = args.first() {
                    self.emit_store_arg(func, 1, separator);
                }
                self.emit_memcall(func, "array_join", argc);
                true
            }
            "map" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_map", 2);
                true
            }
            "filter" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_filter", 2);
                true
            }
            "forEach" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_void(func, "array_forEach", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                true
            }
            "find" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_find", 2);
                true
            }
            "findIndex" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_find_index", 2);
                true
            }
            "reduce" if !args.is_empty() => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                if args.len() >= 2 {
                    self.emit_store_arg(func, 2, &args[1]);
                } else {
                    self.emit_slot_addr(func, 2);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                self.emit_memcall(func, "array_reduce", 3);
                true
            }
            "sort" => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                if !args.is_empty() {
                    self.emit_store_arg(func, 1, &args[0]);
                } else {
                    self.emit_slot_addr(func, 1);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                self.emit_memcall(func, "array_sort", 2);
                true
            }
            "reverse" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_reverse", 1);
                true
            }
            "concat" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_concat", 2);
                true
            }
            "flat" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_flat", 1);
                true
            }
            "toString" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "jsvalue_to_string", 1);
                true
            }
            // Array some/every (return i32 → convert to boolean)
            "some" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "array_some", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "every" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "array_every", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            // RegExp test
            "test" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "regexp_test", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            _ => false,
        }
    }
}
