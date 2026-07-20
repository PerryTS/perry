//! Closures, function references, class instantiation, super, static field/method, enum members, instanceof.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_classes(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
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
                } else if let Some(&func_idx) = self
                    .emitter
                    .imported_func_indices
                    .get(&mod_key)
                    .or_else(|| self.emitter.func_name_map.get(name))
                {
                    // Create a closure wrapper with 0 captures (like FuncRef).
                    // Consumer import table first — bare names collide.
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
                        return true;
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
                        return true;
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
                        return true;
                    }
                    // `new Array()` / `new Array(n)` / `new Array(a, b, ...)`.
                    // Without this case the constructor fell through to the
                    // generic `class_new` path, which allocates a plain object
                    // — element writes landed as properties but Array.isArray
                    // was false, so `.length` read 0 forever. Mirrors the
                    // native builtin (perry-codegen lower_call/builtin.rs):
                    // no args → empty; one arg → runtime type check (number =
                    // length, ES2015 §22.1.1); ≥2 args → element-list form,
                    // identical to the array literal.
                    "Array" => {
                        if args.is_empty() {
                            self.emit_frame_begin(func, 0);
                            self.emit_memcall(func, "array_new", 0);
                        } else if args.len() == 1 {
                            self.emit_frame_begin(func, 1);
                            self.emit_store_arg(func, 0, &args[0]);
                            self.emit_memcall(func, "array_constructor_single", 1);
                        } else {
                            self.emit_expr(func, &Expr::Array(args.clone()));
                        }
                        return true;
                    }
                    "Map" => {
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "map_new", 0);
                        return true;
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
                        return true;
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
                        return true;
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
            Expr::NewDynamic { callee, args, .. } => {
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
                        return true;
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
            _ => return false,
        }
        true
    }
}
