//! LLVM IR function builder.
//!
//! Port of `anvil/src/llvm/function.ts`. A function owns a `RegCounter` shared
//! by all its blocks (see `block.rs`), an ordered list of blocks, and emits
//! itself as an LLVM `define` when serialized.

use std::rc::Rc;

use crate::block::{FpFlags, LlBlock, RegCounter};
use crate::types::LlvmType;

pub struct LlFunction {
    pub name: String,
    pub return_type: LlvmType,
    pub params: Vec<(LlvmType, String)>,
    /// Optional LLVM linkage string, e.g. `"internal"` or `"private"`. Empty
    /// string means external (default) linkage.
    pub linkage: String,
    /// When true, the function body contains a `try` statement (setjmp/longjmp),
    /// so the definition gets `#1` (`noinline`) and `to_ir` runs the volatile
    /// promotion pass.
    ///
    /// The setjmp hazard: `longjmp` restores the callee-saved registers and
    /// stack pointer that `setjmp` snapshotted, so any local LLVM parked in a
    /// register across the setjmp call reverts to its setjmp-time value when
    /// the exception fires — the try body's mutations vanish in the catch.
    /// `returns_twice` on the setjmp call is not sufficient at -O2 on aarch64.
    ///
    /// This used to be handled by stamping `optnone` on the whole function,
    /// which is correct (at -O0 every value is frame-resident) but cost ~5x on
    /// the surrounding code even when nothing ever throws (#6385). We now apply
    /// C's `volatile` rule precisely instead: only the allocas the try body
    /// actually stores into get volatile accesses, and everything else in the
    /// function stays optimizable. See [`crate::volatile_setjmp`].
    pub has_try: bool,
    /// When true, emit `alwaysinline` attribute. Forces LLVM to inline this
    /// function at every call site, exposing integer operations to the
    /// caller's optimizer context (critical for vectorization of clamp patterns).
    pub force_inline: bool,
    blocks: Vec<LlBlock>,
    block_counter: u32,
    reg_counter: Rc<RegCounter>,
    fp_flags: FpFlags,
    /// Allocas hoisted to the function entry block. These are emitted at
    /// the very top of block 0 at IR-serialization time, so they dominate
    /// every use everywhere in the function.
    ///
    /// LLVM convention is that all `alloca` instructions live in the
    /// function entry block — that way the slot pointer is in scope from
    /// every reachable basic block. Putting an alloca inside an `if` arm
    /// works only when its uses are also in that arm; the moment a closure
    /// captures the slot from a sibling branch (or any code reached after
    /// the if-merge), we get "Instruction does not dominate all uses" from
    /// the LLVM verifier.
    ///
    /// Use `LlFunction::alloca_entry(ty)` to allocate; the helper bumps
    /// the shared register counter so the returned `%r<N>` name is unique
    /// function-wide, then appends `"  %r<N> = alloca <ty>"` to this list.
    /// `to_ir()` prepends the list to entry-block instructions in order.
    entry_allocas: Vec<String>,
    /// Hoisted setup instructions (loads, stores, calls) that must run
    /// AFTER the entry block's "init prelude" — `js_gc_init` and the
    /// `__perry_init_strings_*` calls — but BEFORE any user code, so
    /// they dominate every reachable use yet see the up-to-date module
    /// state. Used by the inline-allocator hoist for per-class
    /// `keys_array` global loads: the global is populated by
    /// `__perry_init_strings_*`, so loading it at the very top of the
    /// entry block (in `entry_allocas`) reads zero. Splicing the load
    /// in just after the init calls fixes that without losing the
    /// loop-invariant hoisting benefit on the hot allocation path.
    ///
    /// `to_ir()` splices these instructions into block 0 at the
    /// `entry_init_boundary` instruction index. If no boundary is set
    /// (e.g. user functions, which have no init prelude), they are
    /// emitted immediately after entry allocas and before the first
    /// block instruction so the dominance guarantee still holds.
    entry_post_init_setup: Vec<String>,
    /// Index in block 0's instruction list where `entry_post_init_setup`
    /// should be spliced in. Set by `mark_entry_init_boundary` after
    /// the init prelude has been emitted; left as `None` for functions
    /// with no init prelude.
    entry_init_boundary: Option<usize>,
    /// Shadow-stack frame slot (gen-GC Phase A sub-phase 2). When
    /// `Some(slot_reg)`, `to_ir()` rewrites every `ret` in the
    /// function body to call `js_shadow_frame_pop` first, reading
    /// the frame handle stored at `slot_reg`. The push is emitted by
    /// either `enable_shadow_frame` (top of entry) or
    /// `enable_post_init_shadow_frame` (after the entry init prelude).
    ///
    /// `None` means no shadow frame — `ret` instructions pass
    /// through unchanged. Currently gated per-function so we can
    /// land wiring incrementally (e.g. just `main`) before
    /// flipping the default across every user function.
    shadow_frame_slot: Option<String>,
    /// Runtime hooks emitted immediately before each non-pointer `ret`.
    /// Entry/module-init functions use this for process-level diagnostics
    /// that must run regardless of which block reaches the normal epilogue.
    pre_return_void_calls: Vec<String>,
}

impl LlFunction {
    pub fn new(
        name: impl Into<String>,
        return_type: LlvmType,
        params: Vec<(LlvmType, String)>,
    ) -> Self {
        Self::new_with_fp_flags(name, return_type, params, FpFlags::default())
    }

    pub fn new_with_fp_flags(
        name: impl Into<String>,
        return_type: LlvmType,
        params: Vec<(LlvmType, String)>,
        fp_flags: FpFlags,
    ) -> Self {
        Self {
            name: name.into(),
            return_type,
            params,
            linkage: String::new(),
            has_try: false,
            force_inline: false,
            blocks: Vec::new(),
            block_counter: 0,
            reg_counter: Rc::new(RegCounter::new()),
            fp_flags,
            entry_allocas: Vec::new(),
            entry_post_init_setup: Vec::new(),
            entry_init_boundary: None,
            shadow_frame_slot: None,
            pre_return_void_calls: Vec::new(),
        }
    }

    /// Enable shadow-stack frame emission for this function (gen-GC
    /// Phase A sub-phase 2). Emits `js_shadow_frame_push(slot_count)`
    /// into `entry_allocas` so it runs at the top of block 0, stores
    /// the returned u64 handle into a fresh alloca, and records the
    /// slot for the `to_ir()` ret-rewriting pass to load from.
    ///
    /// Safe to call at most once per function. After this call,
    /// `to_ir()` will insert a matching
    /// `js_shadow_frame_pop(loaded_handle)` before every `ret` in
    /// the function body, regardless of which codegen path emitted
    /// the ret. Frame balance is preserved automatically.
    ///
    /// Passing `slot_count = 0` is a no-op: the frame would only carry
    /// a (prev_top, slot_count) header with no GC-root slots — that is
    /// pure overhead, an extra TLS-touching call per function entry +
    /// per ret. Today every leaf function with no pointer-typed locals
    /// (clampIdx, clampU8, imul32, …) hits this case, and when the
    /// function is `alwaysinline` the push/pop pair gets duplicated
    /// into every caller's hot loop. Skip the frame entirely; the
    /// to_ir() rewrite pass keys off `shadow_frame_slot.is_some()`,
    /// so no matching pop is emitted either.
    pub fn enable_shadow_frame(&mut self, slot_count: u32) {
        self.enable_shadow_frame_inner(slot_count, false);
    }

    /// Enable shadow-stack frame emission for entry/module-init functions
    /// whose first block contains runtime init prelude calls. The handle slot
    /// still lives in `entry_allocas` so it dominates all returns, but the
    /// `js_shadow_frame_push` call runs through `entry_post_init_setup`, after
    /// `mark_entry_init_boundary()` has marked `js_gc_init` / string-init
    /// completion and before any top-level user code is lowered.
    pub fn enable_post_init_shadow_frame(&mut self, slot_count: u32) {
        self.enable_shadow_frame_inner(slot_count, true);
    }

    fn enable_shadow_frame_inner(&mut self, slot_count: u32, post_init: bool) {
        use crate::types::I64;
        if self.shadow_frame_slot.is_some() {
            return;
        }
        if slot_count == 0 {
            return;
        }
        let handle_slot = self.alloca_entry(I64);
        let handle_reg = format!("%r{}", self.reg_counter.next());
        let push_line = format!(
            "  {} = call i64 @js_shadow_frame_push(i32 {})",
            handle_reg, slot_count
        );
        let store_line = format!("  store i64 {}, ptr {}", handle_reg, handle_slot);
        if post_init {
            self.entry_post_init_setup.push(push_line);
            self.entry_post_init_setup.push(store_line);
        } else {
            self.entry_allocas.push(push_line);
            self.entry_allocas.push(store_line);
        }
        self.shadow_frame_slot = Some(handle_slot);
    }

    /// Mark the current end of the entry block as the boundary between
    /// the init prelude (`js_gc_init`, `__perry_init_strings_*`) and
    /// user code. Hoisted post-init setup (cached global loads) is
    /// spliced in at this point so it dominates every use yet sees the
    /// initialized module state. Call this once, immediately after the
    /// codegen has emitted the init prelude into block 0 and before any
    /// user statement is lowered.
    pub fn mark_entry_init_boundary(&mut self) {
        if let Some(blk) = self.blocks.first() {
            self.entry_init_boundary = Some(blk.instruction_count());
        } else {
            self.entry_init_boundary = Some(0);
        }
    }

    pub fn add_pre_return_void_call(&mut self, func_name: impl Into<String>) {
        self.pre_return_void_calls.push(func_name.into());
    }

    /// Open a setjmp-protected region (#6385). Every `store` emitted into any
    /// block of this function until the matching [`exit_try_region`] is
    /// recorded as "modified between the setjmp and a possible longjmp", and
    /// the alloca behind it is given volatile accesses by `to_ir`.
    ///
    /// Call this around the lowering of a `try` body and of a `catch` body
    /// that a `finally` re-protects — i.e. exactly the code that a `longjmp`
    /// can cut short. Regions nest; the depth counter handles that.
    ///
    /// [`exit_try_region`]: LlFunction::exit_try_region
    pub fn enter_try_region(&self) {
        self.reg_counter.enter_try_region();
    }

    pub fn exit_try_region(&self) {
        self.reg_counter.exit_try_region();
    }

    /// Allocate a fresh stack slot in the function entry block. Returns
    /// the SSA pointer name (e.g. `%r42`). The instruction is emitted at
    /// the top of block 0, ahead of any existing entry-block code, so
    /// the slot dominates every reachable use — even from inside nested
    /// if/else branches that would otherwise produce a "does not dominate
    /// all uses" verifier error.
    pub fn alloca_entry(&mut self, ty: LlvmType) -> String {
        let r = format!("%r{}", self.reg_counter.next());
        self.entry_allocas.push(format!("  {} = alloca {}", r, ty));
        r
    }

    /// Allocate a fixed-size `[count x elem_ty]` array slot in the function
    /// entry block. Returned register is a `ptr` to the array; index it with
    /// `gep(elem_ty, reg, [(I64, i)])`.
    ///
    /// LLVM lowers a non-entry-block `alloca` as a runtime `sub %rsp, N`
    /// with no matching restore — every loop iteration through such a block
    /// permanently shrinks the stack. Issue #167 hit this for the args-array
    /// allocas in `js_native_call_method` dispatch sites: a tight
    /// `for (i = 0; i < N; i++) buf.readInt32BE(i*4)` ate ~16 bytes of stack
    /// per iteration and SIGSEGV'd around iteration 250k–300k. The cure is
    /// to hoist these allocas to the entry block (executed once at function
    /// prologue) — what this helper enforces.
    pub fn alloca_entry_array(&mut self, elem_ty: LlvmType, count: usize) -> String {
        let r = format!("%r{}", self.reg_counter.next());
        self.entry_allocas
            .push(format!("  {} = alloca [{} x {}]", r, count, elem_ty));
        r
    }

    /// Allocate a byte buffer in the entry block with an explicit ABI
    /// alignment. Used for C-layout POD records where field GEPs must rest on
    /// a verifier-checked stack object, not JS object storage.
    pub fn alloca_entry_bytes_aligned(&mut self, size: u32, alignment: u32) -> String {
        let r = format!("%r{}", self.reg_counter.next());
        self.entry_allocas.push(format!(
            "  {} = alloca [{} x i8], align {}",
            r, size, alignment
        ));
        r
    }

    /// Push a store instruction into the entry-block alloca section.
    /// Used to initialize allocas to a safe default (e.g. TAG_UNDEFINED)
    /// at the top of the function, before any user code runs.
    pub fn entry_allocas_push_store(&mut self, ty: crate::types::LlvmType, val: &str, ptr: &str) {
        self.entry_allocas
            .push(format!("  store {} {}, ptr {}", ty, val, ptr));
    }

    /// Emit a one-time void call in the function-entry setup region.
    ///
    /// Use this for metadata/registration work that must happen before
    /// any reachable hot-path use but does not need to run at each use
    /// site. If the function has an init prelude boundary, the call is
    /// spliced after runtime/string initialization; otherwise it is
    /// emitted at the top of the entry block with the other entry setup.
    pub fn entry_setup_call_void(&mut self, func_name: &str, args: &[(LlvmType, &str)]) {
        crate::ext_registry::record_ffi_call(func_name);
        let arg_str = args
            .iter()
            .map(|(ty, value)| format!("{} {}", ty, value))
            .collect::<Vec<_>>()
            .join(", ");
        let line = format!("  call void @{}({})", func_name, arg_str);
        self.entry_post_init_setup.push(line);
    }

    /// Emit a one-time function-entry init sequence: allocate a `ptr`
    /// slot, call `func_name()` (no args), store the result in the
    /// slot, return the slot pointer name. Used by the inline bump
    /// allocator to cache the per-thread `InlineArenaState` pointer
    /// once per JS function (instead of paying a TLS access on every
    /// `new ClassName()`).
    ///
    /// Lives in `entry_allocas` so the call + store run before any
    /// user code in the entry block, dominating every reachable use.
    /// The slot pointer is returned for the caller to load from at
    /// each subsequent allocation site.
    pub fn entry_init_call_ptr(&mut self, func_name: &str) -> String {
        let slot = self.alloca_entry(crate::types::PTR);
        let result_reg = format!("%r{}", self.reg_counter.next());
        self.entry_allocas
            .push(format!("  {} = call ptr @{}()", result_reg, func_name));
        self.entry_allocas
            .push(format!("  store ptr {}, ptr {}", result_reg, slot));
        slot
    }

    /// Emit a one-time function-entry load of a module global into a
    /// stack slot, returning the slot pointer. Used by the inline
    /// bump allocator to cache class-static values like the per-class
    /// `keys_array` global once per function instead of reloading it
    /// inside the hot allocation loop.
    ///
    /// LLVM's LICM should hoist a loop-invariant global load on its
    /// own, but doesn't when the loop body contains a call to an
    /// external function (like `js_inline_arena_slow_alloc`) that
    /// LLVM can't prove won't modify the global. Hoisting manually
    /// at the codegen layer sidesteps the alias-analysis question.
    pub fn entry_init_load_global(
        &mut self,
        global_name: &str,
        ty: crate::types::LlvmType,
    ) -> String {
        let slot = self.alloca_entry(ty);
        let result_reg = format!("%r{}", self.reg_counter.next());
        // The alloca dominates everything, but the load+store of the
        // global must run AFTER the entry-block init prelude (which is
        // what populates module-init globals like `@perry_class_keys_*`).
        // If a boundary has been marked, splice the load+store into
        // `entry_post_init_setup`; otherwise (no init prelude in this
        // function) we can put them right at the top with the alloca.
        let load_line = format!("  {} = load {}, ptr @{}", result_reg, ty, global_name);
        let store_line = format!("  store {} {}, ptr {}", ty, result_reg, slot);
        if self.entry_init_boundary.is_some() {
            self.entry_post_init_setup.push(load_line);
            self.entry_post_init_setup.push(store_line);
        } else {
            self.entry_allocas.push(load_line);
            self.entry_allocas.push(store_line);
        }
        slot
    }

    /// Create a new basic block with the given semantic name (e.g. "entry",
    /// "if.then"). A numeric suffix is appended to make the label unique
    /// across the function.
    pub fn create_block(&mut self, name: &str) -> &mut LlBlock {
        let label = format!("{}.{}", name, self.block_counter);
        self.block_counter += 1;
        let block = LlBlock::new_with_fp_flags(label, self.reg_counter.clone(), self.fp_flags);
        self.blocks.push(block);
        // Safe unwrap: we just pushed.
        self.blocks.last_mut().unwrap()
    }

    /// Accessor for an earlier block by index — needed when codegen has to
    /// come back and append to a predecessor (e.g. patching an unreachable
    /// fallthrough).
    pub fn block_mut(&mut self, idx: usize) -> Option<&mut LlBlock> {
        self.blocks.get_mut(idx)
    }

    pub fn blocks(&self) -> &[LlBlock] {
        &self.blocks
    }

    pub fn num_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// Label of the last-created block — convenience for expression codegen
    /// that needs to feed a phi node the predecessor label after compiling a
    /// sub-expression whose control flow may have split.
    pub fn last_block_label(&self) -> Option<&str> {
        self.blocks.last().map(|b| b.label.as_str())
    }

    /// Cheap estimate of this function's rendered IR size in bytes, used to
    /// balance codegen-unit partitioning (#5391) without rendering twice. Sums
    /// the byte length of every instruction + entry alloca (the dominant terms);
    /// block labels/headers are a small fixed overhead per block.
    pub fn estimated_ir_bytes(&self) -> usize {
        let body: usize = self
            .blocks
            .iter()
            .map(|b| b.instructions_iter().map(|i| i.len() + 1).sum::<usize>() + b.label.len() + 4)
            .sum();
        let allocas: usize = self.entry_allocas.iter().map(|a| a.len() + 1).sum();
        body + allocas + self.name.len() + 64
    }

    pub fn to_ir(&self) -> String {
        let param_str = self
            .params
            .iter()
            .map(|(t, n)| format!("{} {}", t, n))
            .collect::<Vec<_>>()
            .join(", ");

        let linkage = if self.linkage.is_empty() {
            String::new()
        } else {
            format!("{} ", self.linkage)
        };

        let attrs = if self.has_try {
            " #1"
        } else if self.force_inline {
            " alwaysinline"
        } else {
            ""
        };
        let mut ir = format!(
            "define {}{} @{}({}){} {{\n",
            linkage, self.return_type, self.name, param_str, attrs
        );

        for (i, blk) in self.blocks.iter().enumerate() {
            if i > 0 {
                ir.push('\n');
            }
            // Block 0 (entry) gets two splices in its body:
            //   1. `entry_allocas`: hoisted allocas + a few simple init
            //      sequences. These go at the very top, between the
            //      label line and any block instructions, so they
            //      dominate every reachable use in the function.
            //   2. `entry_post_init_setup`: hoisted setup that must
            //      run AFTER the init prelude (gc_init / init_strings
            //      calls) so it sees the up-to-date module state. The
            //      splice point is `entry_init_boundary`, which the
            //      codegen marks immediately after emitting the
            //      prelude.
            // Both splices are textual: we re-render the block label,
            // the prefix instructions (up to the boundary), the
            // post-init setup, and then the rest of the block body.
            if i == 0 && (!self.entry_allocas.is_empty() || !self.entry_post_init_setup.is_empty())
            {
                ir.push_str(&blk.label);
                ir.push_str(":\n");
                // 1. Allocas + simple inits at the very top.
                for alloca in &self.entry_allocas {
                    ir.push_str(alloca);
                    ir.push('\n');
                }
                // 2. Render the block instructions, with the post-init
                //    splice at the boundary index.
                let boundary = self
                    .entry_init_boundary
                    .unwrap_or(0)
                    .min(blk.instruction_count());
                let mut idx = 0;
                for inst in blk.instructions_iter() {
                    if idx == boundary {
                        for line in &self.entry_post_init_setup {
                            ir.push_str(line);
                            ir.push('\n');
                        }
                    }
                    ir.push_str(inst);
                    ir.push('\n');
                    idx += 1;
                }
                // Boundary at end-of-block (or empty block).
                if idx == boundary {
                    for line in &self.entry_post_init_setup {
                        ir.push_str(line);
                        ir.push('\n');
                    }
                }
            } else {
                ir.push_str(&blk.to_ir());
                ir.push('\n');
            }
        }

        ir.push_str("}\n");

        // Return-site rewrite hooks.
        //
        // Shadow-stack pop (gen-GC Phase A sub-phase 2) and entry
        // diagnostics both need to run before every normal return,
        // regardless of which lowering path emitted it. Textual rewrite
        // on the full IR catches implicit returns, Stmt::Return, and any
        // hand-emitted `ret`.
        let ir = if self.shadow_frame_slot.is_some() || !self.pre_return_void_calls.is_empty() {
            let mut out = String::with_capacity(ir.len() + 512);
            let mut seq: u32 = 0;
            for line in ir.lines() {
                let trimmed = line.trim_start();
                if (trimmed.starts_with("ret ") || trimmed == "ret void")
                    && !trimmed.starts_with("ret ptr ")
                // skip rare ptr rets
                {
                    for func_name in &self.pre_return_void_calls {
                        out.push_str(&format!("  call void @{}()\n", func_name));
                    }
                    if let Some(handle_slot) = &self.shadow_frame_slot {
                        let load_reg = format!("%shadow_pop_l_{}", seq);
                        seq += 1;
                        out.push_str(&format!("  {} = load i64, ptr {}\n", load_reg, handle_slot));
                        out.push_str(&format!(
                            "  call void @js_shadow_frame_pop(i64 {})\n",
                            load_reg
                        ));
                    }
                }
                out.push_str(line);
                out.push('\n');
            }
            out
        } else {
            ir
        };

        // setjmp volatile promotion (#6385).
        //
        // Runs LAST so it sees every instruction, including the ones the
        // return-site rewrite above just spliced in. Any alloca this function
        // stores into between a `setjmp` and its `longjmp` (recorded by
        // `LlBlock::emit` while a try region was open) gets `volatile` loads
        // and stores, which is what stops mem2reg/SROA from promoting it into
        // a register that `longjmp` would revert. This replaces the old
        // `optnone`-the-whole-function hammer.
        if self.has_try {
            let try_stores = self.reg_counter.try_region_stores();
            if !try_stores.is_empty() {
                return crate::volatile_setjmp::apply_setjmp_volatile(&ir, &try_stores);
            }
        }

        ir
    }
}
