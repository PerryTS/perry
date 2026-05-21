//! `FuncEmitCtx`: context for emitting a single function body. The associated
//! emit methods (`emit_stmt`, `emit_expr`, `emit_bitwise_binary`, …) live in
//! the various sibling files (`stmt.rs`, `expr/`, `binary.rs`, …).
//!
//! Pure code-movement from `mod.rs`.

use super::*;

/// Context for emitting a single function body
pub(super) struct FuncEmitCtx<'a> {
    pub(super) emitter: &'a WasmModuleEmitter,
    pub(super) local_map: &'a BTreeMap<LocalId, u32>,
    /// Block nesting depth for break/continue
    pub(super) break_depth: Vec<u32>,
    pub(super) loop_depth: Vec<u32>,
    pub(super) block_depth: u32,
    /// Stack of (label, break_depth, continue_depth) for labeled break/continue.
    /// When `Labeled { label, body }` is a loop, this ties the label to the loop's blocks.
    pub(super) label_stack: Vec<(String, u32, u32)>,
    /// Pending label to attach to the next loop encountered.
    pub(super) pending_label: Option<String>,
    /// Current class name (set when compiling class methods/constructors)
    pub(super) current_class: Option<String>,
    /// Index of a temp i64 local
    pub(super) temp_local: u32,
    /// Index of a temp i32 local (for mem_call base address)
    pub(super) temp_local_i32: u32,
    /// Index of a second temp i64 local for emit_store_arg
    pub(super) temp_store_local: u32,
    /// Current frame size for emit_store_arg address computation
    pub(super) current_frame_size: u32,
    /// Stack of saved frame sizes for nested frame support
    pub(super) frame_stack: Vec<u32>,
}

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn new(
        emitter: &'a WasmModuleEmitter,
        local_map: &'a BTreeMap<LocalId, u32>,
        temp_local: u32,
        temp_local_i32: u32,
    ) -> Self {
        Self {
            emitter,
            local_map,
            break_depth: Vec::new(),
            loop_depth: Vec::new(),
            block_depth: 0,
            label_stack: Vec::new(),
            pending_label: None,
            current_class: None,
            temp_local,
            temp_local_i32,
            temp_store_local: temp_local + 1,
            current_frame_size: 0,
            frame_stack: Vec::new(),
        }
    }
}
