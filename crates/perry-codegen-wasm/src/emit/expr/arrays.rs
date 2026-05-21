//! Array literals, spread, and all HIR-level array methods (push/pop/shift/slice/map/filter/reduce/etc.).
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_arrays(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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

            _ => return false,
        }
        true
    }
}
