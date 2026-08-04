//! LLVM IR function builder.
//!
//! Port of `anvil/src/llvm/function.ts`. A function owns a `RegCounter` shared
//! by all its blocks (see `block.rs`), an ordered list of blocks, and emits
//! itself as an LLVM `define` when serialized.

use std::rc::Rc;

use crate::block::{FpFlags, LlBlock, RegCounter};
use crate::types::LlvmType;

/// Precise GC roots lowered onto the native frame (statepoints / RS4GC,
/// #7173 / #7174). A sibling file only because of the 2,000-line cap.
mod precise_roots;

use precise_roots::{lower_precise_roots_to_native_stack, retype_landing_pads_for_statepoints};

pub struct LlFunction {
    pub name: String,
    pub return_type: LlvmType,
    pub params: Vec<(LlvmType, String)>,
    /// Optional LLVM linkage string, e.g. `"internal"` or `"private"`. Empty
    /// string means external (default) linkage.
    pub linkage: String,
    /// When true, emit `alwaysinline` attribute. Forces LLVM to inline this
    /// function at every call site, exposing integer operations to the
    /// caller's optimizer context (critical for vectorization of clamp patterns).
    pub force_inline: bool,
    /// When true (and `force_inline` is not), emit the `inlinehint` attribute.
    /// Unlike `alwaysinline`, `inlinehint` only *raises* LLVM's inline
    /// threshold for this callee — LLVM keeps its `-O3` growth budget and can
    /// still decline to inline into cold / many call sites. Set for small
    /// functions with a hot (in-loop) call site so a bit-mixer-style kernel
    /// gets inlined into its loop without the binary-size blowup an
    /// unconditional `alwaysinline` threshold bump causes. See the
    /// inline-hot-small heuristic in `codegen/function.rs`. `alwaysinline`
    /// already implies the hint, so the two are never emitted together.
    pub inline_hint: bool,
    /// Invoke-EH (#7302): this function contains landing pads (Itanium) or
    /// funclet pads (SEH), so its `define` line must carry
    /// `personality ptr @<name>` — `perry_eh_personality` on Mach-O/ELF,
    /// `__C_specific_handler` on windows-msvc. Set by the try/async-boundary
    /// dispatch (which knows the target triple).
    pub personality: Option<&'static str>,
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
    /// Entry alloca holding this thread's `ShadowStackState` address, so the
    /// inline slot stores (#7088) can address the buffer without a per-store
    /// thread-local lookup. Set alongside `shadow_frame_slot`.
    shadow_state_slot: Option<String>,
    /// Whether shadow-frame emission was requested for this function at all
    /// (i.e. `enable_shadow_frame` / `enable_post_init_shadow_frame` ran).
    ///
    /// Distinct from `shadow_frame_slot.is_some()`: a function whose locals
    /// were all proven non-pointer requests a frame but gets none, because a
    /// zero-slot frame is pure overhead. `reserve_shadow_slot` needs to tell
    /// that case (grow it — there is now something to root) apart from "the
    /// shadow stack is switched off for this build" (do nothing).
    shadow_frame_requested: bool,
    /// Which region the frame push belongs in — `entry_post_init_setup` when
    /// `enable_post_init_shadow_frame` was used, `entry_allocas` otherwise.
    /// Remembered so a lazily-created frame lands where the eager one would.
    shadow_frame_post_init_region: bool,
    /// Where the emitted `js_shadow_frame_push` line lives, so its slot count
    /// can be rewritten when lowering discovers a root the pre-lowering
    /// pointer analysis could not see (#6968: scalar-replaced object fields
    /// and array elements, which have no HIR local of their own).
    shadow_frame_push: Option<ShadowFramePush>,
    /// Slot count currently baked into that push line.
    shadow_frame_slot_count: u32,
    /// Research backend: preserve the existing precise-root slot numbering,
    /// but encode the slots in LLVM stack maps instead of allocating a
    /// parallel runtime shadow frame.
    stack_map_requested: bool,
    /// Logical root slots reserved by the existing liveness analysis. The
    /// final IR pass resolves these indices to the native allocas named by
    /// `js_shadow_slot_bind` calls, removes the calls, and emits stack maps.
    stack_map_slot_count: u32,
    /// Runtime hooks emitted immediately before each non-pointer `ret`.
    /// Entry/module-init functions use this for process-level diagnostics
    /// that must run regardless of which block reaches the normal epilogue.
    pre_return_void_calls: Vec<String>,
}

/// Render the frame-push instruction. Kept in one place so the eager
/// emission and the later count rewrite cannot drift.
///
/// `js_shadow_frame_enter` is `js_shadow_frame_push` returning the address of
/// this thread's `ShadowStackState` instead of the frame handle, so the inline
/// slot stores (#7088) get their base pointer without a second thread-local
/// lookup. The handle the matching pop needs is recovered from the state by
/// [`shadow_frame_handle_lines`] — `handle == frame_top - HEADER_SLOTS` — so
/// the pop side is untouched.
fn shadow_frame_push_line(state_reg: &str, slot_count: u32) -> String {
    format!(
        "  {} = call ptr @js_shadow_frame_enter(i32 {})",
        state_reg, slot_count
    )
}

/// The lines following the push: stash the state pointer for the inline slot
/// stores, then recover the frame handle from `ShadowStackState::frame_top`.
///
/// Offsets mirror `perry_runtime::gc::roots::SHADOW_STATE_FRAME_TOP_OFFSET`
/// and `SHADOW_STACK_HEADER_SLOTS`; `perry`'s `shadow_layout_contract` test
/// pins them to the runtime's.
fn shadow_frame_handle_lines(
    state_reg: &str,
    state_slot: &str,
    handle_slot: &str,
    top_ptr_reg: &str,
    top_reg: &str,
    handle_reg: &str,
) -> Vec<String> {
    use crate::expr::shadow_inline::{SHADOW_STACK_HEADER_SLOTS, SHADOW_STATE_FRAME_TOP_OFFSET};
    vec![
        format!("  store ptr {}, ptr {}", state_reg, state_slot),
        format!(
            "  {} = getelementptr inbounds i8, ptr {}, i64 {}",
            top_ptr_reg, state_reg, SHADOW_STATE_FRAME_TOP_OFFSET
        ),
        format!("  {} = load i64, ptr {}", top_reg, top_ptr_reg),
        format!(
            "  {} = sub i64 {}, {}",
            handle_reg, top_reg, SHADOW_STACK_HEADER_SLOTS
        ),
        format!("  store i64 {}, ptr {}", handle_reg, handle_slot),
    ]
}

/// Location of a function's `js_shadow_frame_push` line, so its slot-count
/// operand can be rewritten in place after the fact.
struct ShadowFramePush {
    /// `true` when the line lives in `entry_post_init_setup` rather than
    /// `entry_allocas`.
    post_init: bool,
    /// Index of the line within that region.
    line_idx: usize,
    /// SSA register the push result is assigned to, needed to re-render.
    handle_reg: String,
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
            force_inline: false,
            inline_hint: false,
            personality: None,
            blocks: Vec::new(),
            block_counter: 0,
            reg_counter: Rc::new(RegCounter::new()),
            fp_flags,
            entry_allocas: Vec::new(),
            entry_post_init_setup: Vec::new(),
            entry_init_boundary: None,
            shadow_frame_slot: None,
            shadow_state_slot: None,
            shadow_frame_requested: false,
            shadow_frame_post_init_region: false,
            shadow_frame_push: None,
            shadow_frame_slot_count: 0,
            stack_map_requested: false,
            stack_map_slot_count: 0,
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
        if crate::codegen::helpers::native_stack_roots_enabled() {
            self.shadow_frame_requested = true;
            self.shadow_frame_post_init_region = post_init;
            self.stack_map_requested = slot_count != 0;
            self.stack_map_slot_count = slot_count;
            return;
        }
        if self.shadow_frame_slot.is_some() {
            return;
        }
        // Record the request (and its region) even when no frame is emitted:
        // `reserve_shadow_slot` uses it to tell "nothing to root yet" from
        // "shadow stack disabled", and to place a lazily-created push line in
        // the same region `enable_*_shadow_frame` would have used.
        self.shadow_frame_requested = true;
        self.shadow_frame_post_init_region = post_init;
        if slot_count == 0 {
            return;
        }
        self.emit_shadow_frame_push(slot_count, post_init);
    }

    fn emit_shadow_frame_push(&mut self, slot_count: u32, post_init: bool) {
        use crate::types::{I64, PTR};
        let handle_slot = self.alloca_entry(I64);
        let state_slot = self.alloca_entry(PTR);
        // Null-initialize in `entry_allocas`, which is always spliced at the
        // very top of block 0 — so the slot is initialized even when the push
        // itself lives in `entry_post_init_setup` (spliced later, after the
        // runtime init prelude). An inline slot store that somehow ran before
        // the push would then read null and take its runtime-call arm rather
        // than an undef pointer. Where the push dominates, LLVM sees the later
        // store of a `nonnull` return and folds that arm away.
        self.entry_allocas
            .push(format!("  store ptr null, ptr {}", state_slot));
        let state_reg = format!("%r{}", self.reg_counter.next());
        let top_ptr_reg = format!("%r{}", self.reg_counter.next());
        let top_reg = format!("%r{}", self.reg_counter.next());
        let handle_reg = format!("%r{}", self.reg_counter.next());
        let push_line = shadow_frame_push_line(&state_reg, slot_count);
        let rest = shadow_frame_handle_lines(
            &state_reg,
            &state_slot,
            &handle_slot,
            &top_ptr_reg,
            &top_reg,
            &handle_reg,
        );
        let region = if post_init {
            &mut self.entry_post_init_setup
        } else {
            &mut self.entry_allocas
        };
        let line_idx = region.len();
        region.push(push_line);
        region.extend(rest);
        self.shadow_frame_push = Some(ShadowFramePush {
            post_init,
            line_idx,
            handle_reg: state_reg,
        });
        self.shadow_frame_slot_count = slot_count;
        self.shadow_frame_slot = Some(handle_slot);
        self.shadow_state_slot = Some(state_slot);
    }

    /// The entry alloca holding this thread's `ShadowStackState` address, when
    /// this function pushed a shadow frame. `None` means the inline slot
    /// stores have no base to work from and callers must use the `extern "C"`
    /// entry points.
    pub fn shadow_state_slot(&self) -> Option<&str> {
        self.shadow_state_slot.as_deref()
    }

    /// Reserve one more GC-root slot in this function's shadow frame and
    /// return its index, rewriting the already-emitted
    /// `js_shadow_frame_push` count in place.
    ///
    /// `collect_pointer_typed_locals` sizes the frame before lowering, from
    /// the HIR locals it can see. Scalar replacement (#6968) creates storage
    /// that has no HIR local of its own — one entry-block alloca per object
    /// field / array element — so a heap value living in one of those is
    /// invisible to the pre-lowering count. Rather than teach the collector
    /// to predict every scalar-replacement decision (they are taken later, on
    /// conditions the collector does not evaluate), the frame grows on demand
    /// at the store site that actually needs the root.
    ///
    /// Returns `None` when shadow-stack emission is switched off for this
    /// build, in which case the caller must not emit slot traffic either.
    /// When the frame was skipped as empty, one is created here.
    pub fn reserve_shadow_slot(&mut self) -> Option<u32> {
        if !self.shadow_frame_requested {
            return None;
        }
        if crate::codegen::helpers::native_stack_roots_enabled() {
            let idx = self.stack_map_slot_count;
            self.stack_map_slot_count += 1;
            self.stack_map_requested = true;
            return Some(idx);
        }
        if self.shadow_frame_push.is_none() {
            let post_init = self.shadow_frame_post_init_region;
            self.emit_shadow_frame_push(0, post_init);
        }
        let idx = self.shadow_frame_slot_count;
        self.shadow_frame_slot_count += 1;
        let count = self.shadow_frame_slot_count;
        let Some(push) = &self.shadow_frame_push else {
            return None;
        };
        let (post_init, line_idx) = (push.post_init, push.line_idx);
        let line = shadow_frame_push_line(&push.handle_reg, count);
        let region = if post_init {
            &mut self.entry_post_init_setup
        } else {
            &mut self.entry_allocas
        };
        region[line_idx] = line;
        Some(idx)
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

    /// Invoke-EH (#7302): enter/leave a handler scope. While a scope is
    /// active, every potentially-throwing call any block of this function
    /// emits carries an unwind edge to the scope's landing-pad label.
    pub fn push_eh_scope(&self, lpad_label: String) {
        self.reg_counter.push_eh_scope(lpad_label);
    }

    pub fn pop_eh_scope(&self) {
        self.reg_counter.pop_eh_scope();
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
        self.reg_counter
            .note_shadow_slot_bind(func_name, args.get(1).map(|(_, v)| *v));
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

    /// Mutable block list, for the whole-function passes that run after
    /// lowering. See [`crate::root_reload`].
    pub(crate) fn blocks_mut(&mut self) -> &mut Vec<LlBlock> {
        &mut self.blocks
    }

    pub(crate) fn reg_counter(&self) -> &RegCounter {
        &self.reg_counter
    }

    pub(crate) fn reg_counter_rc(&self) -> Rc<RegCounter> {
        self.reg_counter.clone()
    }

    /// Index in block 0 where `entry_post_init_setup` is spliced, if this
    /// function has an init prelude. See [`note_entry_block_insertions`].
    ///
    /// [`note_entry_block_insertions`]: LlFunction::note_entry_block_insertions
    pub(crate) fn entry_init_boundary(&self) -> Option<usize> {
        self.entry_init_boundary
    }

    /// Tell the function that `n` instructions were inserted into block 0 **at
    /// or above** the splice point, so the splice still lands in the same place
    /// relative to the prelude.
    ///
    /// ★ The count must be exactly the insertions at index ≤ the boundary.
    /// Neither error is cosmetic:
    ///
    /// - **under-counting** leaves the splice too early, and the tail of the
    ///   init prelude ends up below it — a `keys_array` global read hoisted
    ///   above the `__perry_init_strings_*` call that populates it, i.e. a
    ///   zero, silently;
    /// - **over-counting** — bumping by every insertion, including the ones
    ///   below the boundary — leaves the splice too LATE, and `to_ir` clamps an
    ///   out-of-range boundary with `.min(instruction_count())`, which moves the
    ///   whole post-init region to the END of the entry block. For a function
    ///   built by `enable_post_init_shadow_frame` that region contains the
    ///   `js_shadow_frame_enter` call itself, so every `js_shadow_slot_bind`
    ///   in the body then runs with no frame pushed and roots NOTHING. Measured:
    ///   the allocation-point acceptance arm went 30/30 → 0/30 with
    ///   `TypeError: value is not a function`, which reads exactly like the
    ///   rooting bug the pass was written to fix.
    pub(crate) fn note_entry_block_insertions(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        if let Some(b) = self.entry_init_boundary.as_mut() {
            *b += n;
        }
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
            .map(|b| b.insts().iter().map(|i| i.text_len() + 1).sum::<usize>() + b.label.len() + 4)
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

        let attrs = if self.force_inline {
            " alwaysinline"
        } else if self.inline_hint {
            " inlinehint"
        } else {
            ""
        };
        // The native-stack walker recovers frames through the x29 chain, so
        // every generated function must link one; without the attribute,
        // textual-IR input gets no frame-pointer default from the clang
        // driver and LLVM may omit the chain even while saving x29.
        let frame_pointer = if crate::codegen::helpers::native_stack_roots_enabled() {
            " \"frame-pointer\"=\"non-leaf\""
        } else {
            ""
        };
        // #7174: the `!has_try` exclusion is gone with the field. Try/catch no
        // longer lowers to setjmp/longjmp (#7302), so nothing can jump past a
        // `gc.relocate` any more and statepoints cover every function.
        let gc_strategy =
            if self.stack_map_requested && crate::codegen::helpers::native_stack_roots_enabled() {
                " gc \"statepoint-example\""
            } else {
                ""
            };
        // Invoke-EH (#7302): functions containing landing/funclet pads name
        // their personality on the define line. LLVM's grammar orders these
        // `[fn attrs] [gc] [personality]`, so the strategy precedes it.
        let personality = match self.personality {
            Some(p) => format!(" personality ptr @{}", p),
            None => String::new(),
        };
        let mut ir = format!(
            "define {}{} @{}({}){}{}{}{} {{\n",
            linkage,
            self.return_type,
            self.name,
            param_str,
            attrs,
            frame_pointer,
            gc_strategy,
            personality
        );
        self.for_each_final_line::<std::convert::Infallible>(&mut |line| {
            ir.push_str(line);
            ir.push('\n');
            Ok(())
        })
        .unwrap_or_else(|e| match e {});
        ir.push_str("}\n");

        // The return-site rewrite hooks (shadow-stack pop, entry diagnostics)
        // live in `for_each_final_item`, which the loop above already streamed
        // through. This branch used to re-apply them here; after main moved
        // them, doing both emitted `%shadow_pop_l_0` twice in the same
        // function and clang rejected every module with a shadow frame.

        // Research backend: turn the existing shadow-slot binding IR into
        // native-frame stack maps only after lowering is complete, when every
        // lazily-reserved scalar root and every call site is visible.
        //
        let ir = if self.stack_map_requested {
            lower_precise_roots_to_native_stack(&ir, &self.name, self.stack_map_slot_count)
        } else {
            ir
        };

        // RS4GC uses the unwind destination's landing pad **as the token** for
        // the relocates it inserts on the exceptional edge, so
        // `statepoint-example` requires that pad to be `landingpad token`.
        // Perry emits the Itanium `{ ptr, i32 }` form, which makes RS4GC
        // produce `gc.relocate({ ptr, i32 } %lpad, ...)` and the verifier
        // reject the module — a try-carrying function simply fails to compile.
        //
        // Retyping is safe because the pad's value is dead: `try_stmt` emits it
        // purely to anchor the edge and branches straight on, taking the
        // exception from the runtime rather than the pad payload. Only the type
        // is load-bearing, and only to RS4GC.
        //
        // Conditioned on the same fact as `gc_strategy` above — a function that
        // does not carry the strategy must keep the Itanium form, or its pad
        // becomes untypeable for ordinary EH lowering.
        let ir = if !gc_strategy.is_empty() && crate::codegen::helpers::rs4gc_enabled() {
            retype_landing_pads_for_statepoints(&ir)
        } else {
            ir
        };

        // Invoke-EH (#7302): inline invoke splits move a block's true CFG
        // tail behind `eh.contN:` labels; phi incoming-edge labels captured
        // at emit time must follow. Runs last so it sees the streamed text.
        if self.personality.is_some() && ir.contains("eh.cont") {
            return crate::eh_mode::rewrite_phi_predecessors(&ir);
        }

        ir
    }

    /// Stream every finalized BODY line of this function (block labels,
    /// entry-alloca and post-init splices, instructions, return-site
    /// rewrites; blank separator lines between blocks) in exactly the order
    /// [`to_ir`] renders them — `to_ir` IS this visitor plus the define
    /// header, closing brace, and the invoke-EH phi-predecessor rewrite
    /// (which needs whole-function analysis and therefore text; native
    /// construction bails on personality-carrying functions and takes the
    /// textual path — see codegen/mod.rs).
    ///
    /// This is the seam the native backend consumes: per finalized line,
    /// no per-function text materialization.
    pub fn for_each_final_line<E>(
        &self,
        sink: &mut dyn FnMut(&str) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut buf = String::new();
        self.for_each_final_item::<E>(&mut |item| {
            buf.clear();
            match item {
                FinalItem::Label(l) => {
                    buf.push_str(l);
                    buf.push(':');
                }
                FinalItem::Blank => {}
                FinalItem::Text(t) => buf.push_str(t),
                FinalItem::Inst(i) => i.render_into(&mut buf),
            }
            sink(&buf)
        })
    }

    /// Item-granular twin of [`for_each_final_line`], and the seam the native
    /// backend consumes: typed instructions arrive as [`FinalItem::Inst`] and
    /// are constructed directly, with no per-line text; only labels, entry
    /// splices, synthesized return-site rewrites, and `Raw` payload splits
    /// arrive as text. The two visitors cannot drift: the line visitor is a
    /// rendering adapter over this one.
    pub fn for_each_final_item<E>(
        &self,
        sink: &mut dyn FnMut(FinalItem<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        let rewrite_rets =
            self.shadow_frame_slot.is_some() || !self.pre_return_void_calls.is_empty();
        let mut seq: u32 = 0;
        for (i, blk) in self.blocks.iter().enumerate() {
            if i > 0 {
                sink(FinalItem::Blank)?;
            }
            sink(FinalItem::Label(&blk.label))?;
            let is_entry = i == 0;
            if is_entry {
                for alloca in &self.entry_allocas {
                    self.text_item(alloca, rewrite_rets, &mut seq, sink)?;
                }
            }
            let boundary = if is_entry {
                self.entry_init_boundary
                    .unwrap_or(0)
                    .min(blk.instruction_count())
            } else {
                usize::MAX
            };
            let mut idx = 0usize;
            for inst in blk.insts() {
                if idx == boundary {
                    for line in &self.entry_post_init_setup {
                        self.text_item(line, rewrite_rets, &mut seq, sink)?;
                    }
                }
                self.inst_item(inst, rewrite_rets, &mut seq, sink)?;
                idx += 1;
            }
            if is_entry && idx == boundary {
                for line in &self.entry_post_init_setup {
                    self.text_item(line, rewrite_rets, &mut seq, sink)?;
                }
            }
        }
        Ok(())
    }

    fn inst_item<E>(
        &self,
        inst: &crate::inst::LlInst,
        rewrite_rets: bool,
        seq: &mut u32,
        sink: &mut dyn FnMut(FinalItem<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        use crate::inst::LlInst;
        // Multi-line raw payloads split exactly like text rendering followed
        // by the line-based rewrite pass would.
        if let LlInst::Raw(s) = inst {
            if s.contains('\n') {
                for l in s.split('\n') {
                    self.text_item(l, rewrite_rets, seq, sink)?;
                }
                return Ok(());
            }
        }
        if rewrite_rets {
            let is_rewritable_ret = match inst {
                LlInst::Ret { ty, .. } => *ty != "ptr",
                LlInst::RetVoid => true,
                LlInst::Raw(s) => {
                    let t = s.trim_start();
                    (t.starts_with("ret ") || t == "ret void") && !t.starts_with("ret ptr ")
                }
                _ => false,
            };
            if is_rewritable_ret {
                self.yield_ret_prologue(seq, sink)?;
            }
        }
        sink(FinalItem::Inst(inst))
    }

    /// Return-site rewrite, streamed: before every `ret` (except `ret ptr`),
    /// inject the pre-return void calls and the shadow-frame pop. Byte-equal
    /// to the old whole-text line scan.
    fn text_item<E>(
        &self,
        line: &str,
        rewrite_rets: bool,
        seq: &mut u32,
        sink: &mut dyn FnMut(FinalItem<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        if rewrite_rets {
            let trimmed = line.trim_start();
            if (trimmed.starts_with("ret ") || trimmed == "ret void")
                && !trimmed.starts_with("ret ptr ")
            {
                self.yield_ret_prologue(seq, sink)?;
            }
        }
        sink(FinalItem::Text(line))
    }

    fn yield_ret_prologue<E>(
        &self,
        seq: &mut u32,
        sink: &mut dyn FnMut(FinalItem<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        for func_name in &self.pre_return_void_calls {
            sink(FinalItem::Text(&format!("  call void @{}()", func_name)))?;
        }
        if let Some(handle_slot) = &self.shadow_frame_slot {
            let load_reg = format!("%shadow_pop_l_{}", seq);
            *seq += 1;
            sink(FinalItem::Text(&format!(
                "  {} = load i64, ptr {}",
                load_reg, handle_slot
            )))?;
            sink(FinalItem::Text(&format!(
                "  call void @js_shadow_frame_pop(i64 {})",
                load_reg
            )))?;
        }
        Ok(())
    }
}

/// One finalized item of a function body. See
/// [`LlFunction::for_each_final_item`].
pub enum FinalItem<'a> {
    /// Block label (no trailing colon).
    Label(&'a str),
    /// Blank separator line between blocks.
    Blank,
    /// A pre-rendered text line (entry splices, return-site rewrites,
    /// multi-line raw payload splits).
    Text(&'a str),
    /// A typed instruction — the native backend constructs it directly.
    Inst(&'a crate::inst::LlInst),
}
