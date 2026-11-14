//! Object/property/index emission: object literals, spread, get/set/update, keys/values/entries, delete, in.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_objects(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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

            Expr::ObjectAssign { target, sources } => {
                self.emit_expr(func, target);
                for source in sources {
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
                    self.emit_memcall(func, "object_assign", 2);
                }
            }

            Expr::PropertyGet {
                object, property, ..
            } => {
                // Namespace-import member read (`import * as W; W.MEMBER`):
                // the object lowers to ExternFuncRef("W"), which as a value is
                // undefined — resolve the member against the source module's
                // promoted-let global instead (registered under the dotted key
                // in compile.rs). Must run before every other special case,
                // including .length: `W.length` is a module member here, not
                // a string/array length.
                if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                    let key = (
                        self.emitter.current_mod_idx,
                        format!("{}.{}", name, property),
                    );
                    if let Some(&gidx) = self.emitter.imported_var_globals.get(&key) {
                        func.instruction(&Instruction::GlobalGet(gidx));
                        return true;
                    }
                    // Member is an exported FUNCTION used as a value
                    // (`const f = W.fn`): wrap in a zero-capture closure,
                    // mirroring the ExternFuncRef value arm in classes.rs.
                    // (Direct calls take the fast path in calls.rs instead.)
                    if let Some(&func_idx) = self.emitter.imported_ns_funcs.get(&key) {
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
                        return true;
                    }
                }
                // Unresolved-global members are properties of the host global.
                if let Expr::GlobalGet(id) = object.as_ref() {
                    if !self.emitter.global_map.contains_key(id) {
                        let key_id = self
                            .emitter
                            .string_map
                            .get(property.as_str())
                            .copied()
                            .unwrap_or(0);
                        self.emit_frame_begin(func, 1);
                        self.emit_store_const(
                            func,
                            0,
                            f64::from_bits((STRING_TAG << 48) | (key_id as u64)),
                        );
                        self.emit_memcall(func, "global_property", 1);
                        return true;
                    }
                }
                // Special case: .length uses string_len which handles both strings and arrays
                if property == "length" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "string_len", 1);
                    return true;
                }
                // Special case: .message on error objects
                if property == "message" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "error_message", 1);
                    return true;
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
                ..
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
                ..
            } => {
                let key_id = self
                    .emitter
                    .string_map
                    .get(property.as_str())
                    .copied()
                    .unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_member_update(func, "class_get_field", "class_set_field", op, *prefix);
            }

            Expr::IndexGet { object, index, .. } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "object_get_dynamic", 2);
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                // Preserve assignment-expression semantics by returning the
                // assigned value after the dynamic write.  Keep the current
                // object -> index -> value evaluation order, but save the
                // value in a temp local so the void bridge call can consume the
                // frame without losing the expression result.
                self.emit_expr(func, value);
                func.instruction(&Instruction::LocalSet(self.temp_store_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_store_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall_void(func, "object_set_dynamic", 3);
                func.instruction(&Instruction::LocalGet(self.temp_store_local));
            }
            // #5016: assignment PutValue for property/index references
            // (`obj.prop = v` / `obj[i] = v`). The HIR lowers BOTH member-write
            // forms to `PutValueSet`; without this arm it fell through to the
            // `_ => TAG_UNDEFINED` catch-all and the write was silently dropped
            // — module-level array element mutation (the recommended mutable
            // state pattern) appeared immutable on the web/wasm target.
            //
            // Mirror the pre-`PutValueSet` lowering split so behavior matches
            // the still-handled `PropertySet`/`IndexSet` arms: a string-literal
            // key (`obj.prop = v`) routes through `class_set_field` so class
            // getters/setters still fire; any other key (`obj[i] = v`) routes
            // through `object_set_dynamic`. Both return the assigned RHS value
            // to preserve assignment-expression semantics.
            Expr::PutValueSet {
                target, key, value, ..
            } => {
                if let Expr::String(property) = key.as_ref() {
                    let key_id = self
                        .emitter
                        .string_map
                        .get(property.as_str())
                        .copied()
                        .unwrap_or(0);
                    let key_bits = (STRING_TAG << 48) | (key_id as u64);
                    self.emit_frame_begin(func, 3);
                    self.emit_store_arg(func, 0, target);
                    self.emit_store_const(func, 1, f64::from_bits(key_bits));
                    self.emit_expr(func, value);
                    func.instruction(&Instruction::LocalSet(self.temp_store_local));
                    self.emit_slot_addr(func, 2);
                    func.instruction(&Instruction::LocalGet(self.temp_store_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_memcall_void(func, "class_set_field", 3);
                    func.instruction(&Instruction::LocalGet(self.temp_store_local));
                } else {
                    self.emit_frame_begin(func, 3);
                    self.emit_store_arg(func, 0, target);
                    self.emit_store_arg(func, 1, key);
                    self.emit_expr(func, value);
                    func.instruction(&Instruction::LocalSet(self.temp_store_local));
                    self.emit_slot_addr(func, 2);
                    func.instruction(&Instruction::LocalGet(self.temp_store_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_memcall_void(func, "object_set_dynamic", 3);
                    func.instruction(&Instruction::LocalGet(self.temp_store_local));
                }
            }
            Expr::IndexUpdate {
                object,
                index,
                op,
                prefix,
                ..
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_member_update(
                    func,
                    "object_get_dynamic",
                    "object_set_dynamic",
                    op,
                    *prefix,
                );
            }

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
                Expr::PropertyGet {
                    object, property, ..
                } => {
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
                Expr::IndexGet { object, index, .. } => {
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

            _ => return false,
        }
        true
    }

    /// `++`/`--` on a member whose base and key already sit in slots 0 and 1 of
    /// an open three-slot frame. Base and key are evaluated once, so the read
    /// and the write hit the same object; the getter's frame copies them from
    /// the open frame, whose memory outlives any nested call, because the fixed
    /// temp locals do not survive the expressions that produced them. Slot 2
    /// carries the updated value to `setter`; the result is the updated value
    /// for prefix and the old one for postfix.
    fn emit_member_update(
        &mut self,
        func: &mut Function,
        getter: &str,
        setter: &str,
        op: &BinaryOp,
        prefix: bool,
    ) {
        let outer = self.current_frame_size;
        self.emit_frame_begin(func, 2);
        for slot in 0..2 {
            self.emit_slot_addr(func, slot);
            func.instruction(&Instruction::GlobalGet(self.emitter.nan_temp_global));
            func.instruction(&Instruction::I32Const(((2 + outer - slot) * 8) as i32));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::I64Load(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }));
            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }));
        }
        self.emit_memcall(func, getter, 2);
        func.instruction(&Instruction::LocalTee(self.temp_local));
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&f64_const(1.0));
        func.instruction(&match op {
            BinaryOp::Sub => Instruction::F64Sub,
            _ => Instruction::F64Add,
        });
        func.instruction(&Instruction::I64ReinterpretF64);
        func.instruction(&Instruction::LocalSet(self.temp_result_local));
        self.emit_slot_addr(func, 2);
        func.instruction(&Instruction::LocalGet(self.temp_result_local));
        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        self.emit_memcall_void(func, setter, 3);
        func.instruction(&Instruction::LocalGet(if prefix {
            self.temp_result_local
        } else {
            self.temp_local
        }));
    }
}
