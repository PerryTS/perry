//! Buffer and Uint8Array operations.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_buffers(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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
            Expr::BufferFromArrayBuffer {
                data,
                byte_offset,
                length,
            } => {
                self.emit_expr(func, data);
                self.emit_expr(func, byte_offset);
                if let Some(len) = length {
                    self.emit_expr(func, len);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                for slot in (0..3).rev() {
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, slot);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                self.emit_memcall(func, "buffer_from_string", 3);
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
            Expr::BufferByteLength {
                data: val,
                encoding: _,
            } => {
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
            _ => return false,
        }
        true
    }
}
