//! Map and Set HIR-level operations.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_map_set(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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
            _ => return false,
        }
        true
    }
}
