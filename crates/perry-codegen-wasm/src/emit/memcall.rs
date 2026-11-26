//! Bridge mem-call emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of the `FuncEmitCtx::rt` RuntimeImports accessor and the
//! `emit_memcall` / `emit_memcall_void` / `emit_memcall_i32` helpers onto
//! a dedicated `impl<'a> FuncEmitCtx<'a>` block.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn rt(&self) -> &RuntimeImports {
        self.emitter.rt.as_ref().unwrap()
    }

    pub(super) fn frame_bytes(&self) -> u32 {
        (self.current_frame_size + self.frame_stack.iter().sum::<u32>()) * 8
    }

    /// Returns to the enclosing frame once a bridge call has consumed the open one.
    #[track_caller]
    fn close_frame(&mut self) {
        let enclosing = self.frame_stack.pop();
        debug_assert!(enclosing.is_some(), "bridge call without an open frame");
        self.current_frame_size = enclosing.unwrap_or(0);
    }

    pub(super) fn emit_exception_guard(&mut self, func: &mut Function) {
        let handler = self
            .exception_handlers
            .last()
            .map(|(depth, bytes, _)| (*depth, *bytes));
        func.instruction(&Instruction::Call(self.rt().has_exception));
        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
        // Abandon unfinished argument frames before transferring to the handler.
        let unwind_bytes = self.frame_bytes() - handler.map_or(0, |(_, bytes)| bytes);
        if unwind_bytes > 0 {
            let sp = self.emitter.nan_temp_global;
            func.instruction(&Instruction::GlobalGet(sp));
            func.instruction(&Instruction::I32Const(unwind_bytes as i32));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::GlobalSet(sp));
        }
        if let Some((depth, _)) = handler {
            func.instruction(&Instruction::Br(self.block_depth - depth + 1));
        } else {
            self.emit_frame_begin(func, 0);
            self.emit_memcall_void(func, "propagate_exception", 0);
            if self.returns_value {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            func.instruction(&Instruction::Return);
        }
        func.instruction(&Instruction::End);
    }

    /// The one value of the function declaration `func_idx` (`function_value`).
    pub(super) fn emit_function_value(&mut self, func: &mut Function, func_idx: u32) {
        let table_idx = self
            .emitter
            .func_to_table_idx
            .get(&func_idx)
            .copied()
            .unwrap_or(func_idx);
        self.emit_frame_begin(func, 2);
        self.emit_store_const(func, 0, table_idx as f64);
        self.emit_store_fn_info(func, 1, func_idx);
        self.emit_memcall(func, "function_value", 2);
    }

    /// Stores function `func_idx`'s `__FnInfo` record (its global) in arg slot `slot`.
    pub(super) fn emit_store_fn_info(&mut self, func: &mut Function, slot: u32, func_idx: u32) {
        self.emit_slot_addr(func, slot);
        match self.emitter.fn_info_globals.get(&func_idx) {
            Some(&gidx) => func.instruction(&Instruction::GlobalGet(gidx)),
            None => func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)),
        };
        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
    }

    /// Emit a bridge function call via WASM memory (Firefox NaN-safe, reentrant-safe).
    /// Call pattern: emit_store_arg(0, ..), emit_store_arg(1, ..), ..., emit_memcall(name, N).
    /// Handles stack pointer save/advance/restore automatically.
    /// Call a bridge function. Frame must already be set up via emit_frame_begin + emit_store_arg.
    /// Returns f64 result, then restores sp.
    #[track_caller]
    pub(super) fn emit_memcall(
        &mut self,
        func: &mut Function,
        bridge_fn_name: &str,
        arg_count: u32,
    ) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self
            .emitter
            .string_map
            .get(bridge_fn_name)
            .copied()
            .unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        // base_addr = sp - frame_size * 8
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call));
        func.instruction(&Instruction::Drop);
        // Read tagged results as i64 so their NaN payload stays intact.
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I64Load(wasm_encoder::MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        // Result is already i64, no conversion needed
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.close_frame();
        if !matches!(
            bridge_fn_name,
            "try_start"
                | "try_end"
                | "has_exception"
                | "get_exception"
                | "throw_value"
                | "propagate_exception"
        ) {
            self.emit_exception_guard(func);
        }
    }

    #[track_caller]
    pub(super) fn emit_memcall_void(
        &mut self,
        func: &mut Function,
        bridge_fn_name: &str,
        arg_count: u32,
    ) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self
            .emitter
            .string_map
            .get(bridge_fn_name)
            .copied()
            .unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call));
        func.instruction(&Instruction::Drop);
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.close_frame();
        if !matches!(
            bridge_fn_name,
            "try_start"
                | "try_end"
                | "has_exception"
                | "get_exception"
                | "throw_value"
                | "propagate_exception"
        ) {
            self.emit_exception_guard(func);
        }
    }

    /// `console.log(a, b)` prints `a b` on one line, so every argument goes to one bridge call.
    /// Like every expression, the call leaves one value: `undefined`.
    pub(super) fn emit_console_call(&mut self, func: &mut Function, bridge: &str, args: &[Expr]) {
        let count = args.len() as u32;
        self.emit_frame_begin(func, count);
        for (slot, arg) in args.iter().enumerate() {
            self.emit_store_arg(func, slot as u32, arg);
        }
        self.emit_memcall_void(func, bridge, count);
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
    }

    #[track_caller]
    pub(super) fn emit_memcall_i32(
        &mut self,
        func: &mut Function,
        bridge_fn_name: &str,
        arg_count: u32,
    ) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self
            .emitter
            .string_map
            .get(bridge_fn_name)
            .copied()
            .unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call_i32));
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.close_frame();
        if !matches!(
            bridge_fn_name,
            "try_start"
                | "try_end"
                | "has_exception"
                | "get_exception"
                | "throw_value"
                | "propagate_exception"
        ) {
            self.emit_exception_guard(func);
        }
    }
}
