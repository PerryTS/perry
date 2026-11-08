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

    pub(super) fn emit_exception_guard(&self, func: &mut Function) {
        let Some(&(depth, entry_bytes)) = self.exception_handlers.last() else {
            return;
        };
        func.instruction(&Instruction::Call(self.rt().has_exception));
        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
        // Abandon unfinished argument frames before transferring to the handler.
        let unwind_bytes = self.frame_bytes() - entry_bytes;
        if unwind_bytes > 0 {
            let sp = self.emitter.nan_temp_global;
            func.instruction(&Instruction::GlobalGet(sp));
            func.instruction(&Instruction::I32Const(unwind_bytes as i32));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::GlobalSet(sp));
        }
        func.instruction(&Instruction::Br(self.block_depth - depth + 1));
        func.instruction(&Instruction::End);
    }

    /// Emit a bridge function call via WASM memory (Firefox NaN-safe, reentrant-safe).
    /// Call pattern: emit_store_arg(0, ..), emit_store_arg(1, ..), ..., emit_memcall(name, N).
    /// Handles stack pointer save/advance/restore automatically.
    /// Call a bridge function. Frame must already be set up via emit_frame_begin + emit_store_arg.
    /// Returns f64 result, then restores sp.
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
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
        if !matches!(
            bridge_fn_name,
            "try_start" | "try_end" | "has_exception" | "get_exception" | "throw_value"
        ) {
            self.emit_exception_guard(func);
        }
    }

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
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
        if !matches!(
            bridge_fn_name,
            "try_start" | "try_end" | "has_exception" | "get_exception" | "throw_value"
        ) {
            self.emit_exception_guard(func);
        }
    }

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
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
        if !matches!(
            bridge_fn_name,
            "try_start" | "try_end" | "has_exception" | "get_exception" | "throw_value"
        ) {
            self.emit_exception_guard(func);
        }
    }
}
