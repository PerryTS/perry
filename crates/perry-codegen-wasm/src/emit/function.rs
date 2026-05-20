//! Function/closure/class compilation + frame-slot helpers extracted from
//! emit/mod.rs (#1102 mechanical split).
//!
//! Pure move. `WasmModuleEmitter::{compile_function, compile_closure,
//! compile_class_constructor, compile_class_method}` and the
//! `FuncEmitCtx` frame-slot helpers (`emit_frame_begin`, `emit_slot_addr`,
//! `emit_store_arg`, `emit_store_const`, `emit_local_or_global_get`)
//! each live on a dedicated inherent `impl` block for their struct.

use super::*;

impl WasmModuleEmitter {
    pub(super) fn compile_function(&self, hir_func: &perry_hir::ir::Function) -> Function {
        // Build local map: param locals come first, then body locals
        let mut local_map = BTreeMap::new();
        for (i, param) in hir_func.params.iter().enumerate() {
            local_map.insert(param.id, i as u32);
        }

        // Scan body for local variable declarations
        let param_count = hir_func.params.len() as u32;
        let mut extra_locals = 0u32;
        collect_locals(
            &hir_func.body,
            &mut local_map,
            &mut extra_locals,
            param_count,
        );

        let temp_local_idx = param_count + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);

        // Must match func_section: `main` is always emitted as `()->i64` even when the body has no
        // `return` statement (HIR doesn't guarantee tail-return lowering yet).
        let wasm_returns_i64 = hir_func.body.iter().any(has_return) || hir_func.name == "main";
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);

        for stmt in &hir_func.body {
            ctx.emit_stmt(&mut func, stmt, wasm_returns_i64);
        }

        // If the Wasm signature includes an i64 result, fallthrough must leave one value on stack.
        if wasm_returns_i64 {
            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        }

        func.instruction(&Instruction::End);
        func
    }

    pub(super) fn compile_closure(
        &self,
        params: &[Param],
        body: &[Stmt],
        captures: &[LocalId],
        mutable_captures: &[LocalId],
    ) -> Function {
        // Closure parameters: captures first, then declared params
        let mut local_map = BTreeMap::new();
        let mut param_idx = 0u32;
        for cap in captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for cap in mutable_captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for param in params {
            local_map.insert(param.id, param_idx);
            param_idx += 1;
        }

        // Scan body for additional locals
        let mut extra_locals = 0u32;
        collect_locals(body, &mut local_map, &mut extra_locals, param_idx);

        let temp_local_idx = param_idx + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);

        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
        let _has_ret = body.iter().any(has_return);

        for stmt in body {
            ctx.emit_stmt(&mut func, stmt, true); // closures always "return"
        }

        // Default return undefined
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        func.instruction(&Instruction::End);
        func
    }

    pub(super) fn compile_class_constructor(
        &self,
        class: &perry_hir::ir::Class,
        ctor: &perry_hir::ir::Function,
    ) -> Function {
        // Local 0 = this (the instance handle)
        // Params start at local index 1
        let mut local_map = BTreeMap::new();
        // Don't insert this into local_map — Expr::This emits LocalGet(0) directly
        for (i, param) in ctor.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + ctor.params.len();
        let mut extra_locals = 0u32;
        collect_locals(
            &ctor.body,
            &mut local_map,
            &mut extra_locals,
            param_count as u32,
        );

        let temp_local_idx = param_count as u32 + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);
        let _rt = self.rt.as_ref().unwrap();

        // Emit field initializers: class_set_field(this, field_name, value) via mem_call
        for field in &class.fields {
            if let Some(init) = &field.init {
                let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
                ctx.emit_frame_begin(&mut func, 3);
                // Compute base address (sp - 24) and save to temp_i32 local
                let sp = self.nan_temp_global;
                func.instruction(&Instruction::GlobalGet(sp));
                func.instruction(&Instruction::I32Const(24));
                func.instruction(&Instruction::I32Sub);
                func.instruction(&Instruction::LocalSet(temp_i32_idx));
                // Store this handle to slot 0
                func.instruction(&Instruction::LocalGet(temp_i32_idx));
                func.instruction(&Instruction::LocalGet(0)); // this
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                // Store field name to slot 1
                let field_id = self
                    .string_map
                    .get(field.name.as_str())
                    .copied()
                    .unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                func.instruction(&Instruction::LocalGet(temp_i32_idx));
                func.instruction(&Instruction::I32Const(8));
                func.instruction(&Instruction::I32Add);
                func.instruction(&Instruction::I64Const(field_bits as i64));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                // Store value to slot 2
                ctx.emit_store_arg(&mut func, 2, init);
                // Call via mem_call
                ctx.emit_memcall_void(&mut func, "class_set_field", 3);
            }
        }

        // Emit constructor body
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
        ctx.current_class = Some(class.name.clone());
        for stmt in &ctor.body {
            ctx.emit_stmt(&mut func, stmt, false);
        }

        // Return this
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        func
    }

    pub(super) fn compile_class_method(&self, method: &perry_hir::ir::Function) -> Function {
        // Local 0 = this, params start at 1
        let mut local_map = BTreeMap::new();
        for (i, param) in method.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + method.params.len();
        let mut extra_locals = 0u32;
        collect_locals(
            &method.body,
            &mut local_map,
            &mut extra_locals,
            param_count as u32,
        );

        let temp_local_idx = param_count as u32 + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);
        let _has_ret = method.body.iter().any(has_return);
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);

        for stmt in &method.body {
            ctx.emit_stmt(&mut func, stmt, true); // methods always return f64
        }

        // Always push default return (method type is always -> f64)
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        func.instruction(&Instruction::End);
        func
    }
}

impl<'a> FuncEmitCtx<'a> {
    // emit_nan_safe_const removed - all values are i64 now, NaN canonicalization is not an issue.

    /// Advance the sp and record the frame size for emit_store_arg.
    pub(super) fn emit_frame_begin(&mut self, func: &mut Function, frame_size: u32) {
        let sp = self.emitter.nan_temp_global;
        self.frame_stack.push(self.current_frame_size);
        self.current_frame_size = frame_size;
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const((frame_size * 8) as i32));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::GlobalSet(sp));
    }

    /// Compute memory address for a slot in the current frame.
    /// Address = sp - (current_frame_size - slot) * 8
    /// This works correctly across nested calls because sp was advanced by emit_frame_begin.
    pub(super) fn emit_slot_addr(&self, func: &mut Function, slot: u32) {
        let sp = self.emitter.nan_temp_global;
        let offset_from_sp = (self.current_frame_size - slot) * 8;
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(offset_from_sp as i32));
        func.instruction(&Instruction::I32Sub);
    }

    /// Store an expression's result to memory at the current frame's slot.
    pub(super) fn emit_store_arg(&mut self, func: &mut Function, slot: u32, expr: &Expr) {
        match expr {
            Expr::String(s) => {
                let string_id = self
                    .emitter
                    .string_map
                    .get(s.as_str())
                    .copied()
                    .unwrap_or(0);
                let bits = (STRING_TAG << 48) | (string_id as u64);
                self.emit_slot_addr(func, slot);
                func.instruction(&Instruction::I64Const(bits as i64));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
            }
            _ => {
                // Evaluate expression first, save to dedicated temp_store_local.
                // Prevents slot address (i32) from sitting on stack during nested memcalls.
                self.emit_expr(func, expr);
                func.instruction(&Instruction::LocalSet(self.temp_store_local));
                self.emit_slot_addr(func, slot);
                func.instruction(&Instruction::LocalGet(self.temp_store_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
            }
        }
    }

    pub(super) fn emit_store_const(&self, func: &mut Function, slot: u32, val: f64) {
        let bits = val.to_bits();
        self.emit_slot_addr(func, slot);
        func.instruction(&Instruction::I64Const(bits as i64));
        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
    }

    /// Emit a load of a HIR local by id. Top-level `let`s are stored in WASM globals
    /// (not locals), so we must check `module_let_globals` before `local_map`. Falls
    /// back to `TAG_UNDEFINED`. Without this, Array* HIR nodes that reference a
    /// top-level `const xs = []` were pushing `I64Const(0)` into the temp — see
    /// Issue #133 item 3.
    pub(super) fn emit_local_or_global_get(&self, func: &mut Function, id: &LocalId) {
        if let Some(&gidx) = self
            .emitter
            .module_let_globals
            .get(&(self.emitter.current_mod_idx, *id))
        {
            func.instruction(&Instruction::GlobalGet(gidx));
        } else if let Some(&idx) = self.local_map.get(id) {
            func.instruction(&Instruction::LocalGet(idx));
        } else {
            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        }
    }
}
