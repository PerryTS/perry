//! Inline shadow-slot stores (#7088).
//!
//! # What this replaces
//!
//! Every store to a GC-rooted local used to be an `extern "C"` call —
//! `js_shadow_slot_bind(idx, &local)` for a pointer-capable value,
//! `js_shadow_slot_set(idx, 0)` for the "dead from here" clear. #7079 made
//! those functions cheap internally, but a call costs twice: the call itself,
//! and the fact that it is **opaque to LLVM**, which forces a spill of every
//! live value around it and blocks hoisting across it.
//!
//! Read out of the shipped `aarch64` archive, `js_shadow_slot_bind`'s fast path
//! was ~35 instructions: a 4-instruction prologue/epilogue pair, a **TLSDESC
//! indirect call** (`adrp`/`ldr`/`add`/`blr` plus the resolver) to reach the
//! thread-local, a lazy destructor-registration check, then the six or so
//! instructions that actually do the work. The inline form below is those six
//! plus two guards, with no call and no TLS sequence at all.
//!
//! # How the thread-local block is reached
//!
//! Not by re-deriving the TLS address in generated code — that would have to
//! model Rust's TLS model per platform (TLSDESC on this Linux build, `tlv` on
//! macOS) and would be a second, unverified path to the same memory.
//!
//! Instead the address is *obtained from the runtime* and cached for the
//! activation. `js_shadow_frame_enter` is `js_shadow_frame_push` with the
//! address of this thread's `ShadowStackState` as its return value, so codegen
//! pays exactly the one TLS lookup per activation that the push already paid.
//! The pointer goes into an entry alloca; every slot store loads it back.
//! Because it comes from the runtime's own `SHADOW.with`, it is the same
//! memory `js_shadow_slot_set` writes, by construction rather than by
//! coincidence — and `js_shadow_state_addr` lets a Rust test poke the buffer
//! through these very offsets and read it back through the runtime accessor.
//!
//! Caching it is sound because it is the address of a `const`-initialized,
//! drop-free `thread_local!`: fixed for the thread's lifetime, never
//! reallocated. The *buffer* it points at does move when a deeper frame grows
//! it, which is exactly why `ptr`, `len` and `frame_top` are re-loaded from the
//! state at every store instead of a frame base being cached. One activation of
//! a compiled function runs entirely on one thread, so the cached pointer never
//! escapes to another.
//!
//! # The three root properties
//!
//! * **Liveness** — the inline store writes the same `ShadowEntry.value` and
//!   sets the same `SLOT_ACTIVE` bit in `meta` that the runtime function does,
//!   at the same index, so `visit_shadow_stack_root_slots` marks it identically.
//! * **Rewritability** — `meta` still carries the bound compiled-local address,
//!   with the same alignment fallback, so an evacuating collection rewrites the
//!   alloca the mutator reads after the safepoint, not just the mirror.
//! * **The value the mutator stored** — the value is read from the local slot
//!   at the store site, in the same position the call occupied, and written
//!   immediately. Nothing re-reads the slot at a later safepoint.
//!
//! The incremental-mark root shading barrier is emitted inline too, behind the
//! same `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` gate the runtime and
//! `emit_persistent_shadow_root_barrier` already use.

use super::*;

use crate::types::{I1, I32, I64, PTR};

/// Byte offset of `ShadowStackState::ptr`.
///
/// This block mirrors `perry_runtime::gc::roots::SHADOW_STATE_*`. `perry-codegen`
/// deliberately does not depend on `perry-runtime`, so the two copies are
/// pinned together by `perry`'s `shadow_layout_contract` test, which does
/// depend on both and fails if either moves.
pub const SHADOW_STATE_PTR_OFFSET: u64 = 0;
/// Byte offset of `ShadowStackState::len`.
pub const SHADOW_STATE_LEN_OFFSET: u64 = 8;
/// Byte offset of `ShadowStackState::frame_top`.
pub const SHADOW_STATE_FRAME_TOP_OFFSET: u64 = 24;
/// `size_of::<ShadowEntry>()`. A power of two, so indexing is a shift.
pub const SHADOW_ENTRY_SIZE: u64 = 16;
/// `log2(SHADOW_ENTRY_SIZE)`.
pub const SHADOW_ENTRY_SHIFT: u64 = 4;
/// Byte offset of `ShadowEntry::meta` within an entry.
pub const SHADOW_ENTRY_META_OFFSET: u64 = 8;
/// Liveness bit in `ShadowEntry::meta`.
pub const SHADOW_SLOT_ACTIVE_BIT: u64 = 1;
/// Header entries a frame reserves; `frame_top == frame_handle + this`.
pub const SHADOW_STACK_HEADER_SLOTS: u64 = 1;

const _: () = {
    assert!(SHADOW_ENTRY_SIZE == 1 << SHADOW_ENTRY_SHIFT);
    assert!(SHADOW_ENTRY_META_OFFSET < SHADOW_ENTRY_SIZE);
    assert!(SHADOW_SLOT_ACTIVE_BIT == 1);
};

/// What the inline sequence writes into the entry.
enum InlineSlotWrite<'a> {
    /// Mirror `*local_slot` and bind the entry to `local_slot`. Equivalent to
    /// `js_shadow_slot_bind(idx, local_slot)`.
    Bind { local_slot: &'a str },
    /// Zero the value and drop the liveness bit, keeping the binding.
    /// Equivalent to `js_shadow_slot_set(idx, 0)`.
    Clear,
}

/// Emit the inline equivalent of `js_shadow_slot_bind(slot_idx, local_slot)`.
///
/// Returns `false` when this function has no cached state pointer (so the
/// caller must fall back to the `extern "C"` call).
pub(crate) fn emit_inline_slot_bind(ctx: &mut FnCtx<'_>, slot_idx: u32, local_slot: &str) -> bool {
    emit_inline_slot_write(ctx, slot_idx, InlineSlotWrite::Bind { local_slot })
}

/// Emit the inline equivalent of `js_shadow_slot_set(slot_idx, 0)`.
pub(crate) fn emit_inline_slot_clear(ctx: &mut FnCtx<'_>, slot_idx: u32) -> bool {
    emit_inline_slot_write(ctx, slot_idx, InlineSlotWrite::Clear)
}

fn emit_inline_slot_write(ctx: &mut FnCtx<'_>, slot_idx: u32, what: InlineSlotWrite<'_>) -> bool {
    if !crate::codegen::helpers::inline_shadow_slot_enabled() {
        return false;
    }
    let Some(state_slot) = ctx.func.shadow_state_slot().map(str::to_owned) else {
        return false;
    };

    // The slow arm is not dead code kept for tidiness: the state alloca is
    // null-initialized in the entry block, so any path that could reach a slot
    // store before the frame push ran still writes the root — through the
    // runtime function, which does its own TLS lookup. Where the push provably
    // dominates (every shipped path), `js_shadow_frame_enter`'s `nonnull`
    // return lets LLVM fold this branch away entirely.
    let slow_idx = ctx.new_block("ss.slow");
    let chk_top_idx = ctx.new_block("ss.chk_top");
    let chk_len_idx = ctx.new_block("ss.chk_len");
    let store_idx = ctx.new_block("ss.store");
    let done_idx = ctx.new_block("ss.done");
    let slow_label = ctx.block_label(slow_idx);
    let chk_top_label = ctx.block_label(chk_top_idx);
    let chk_len_label = ctx.block_label(chk_len_idx);
    let store_label = ctx.block_label(store_idx);
    let done_label = ctx.block_label(done_idx);

    let state = ctx.block().load(PTR, &state_slot);
    let state_null = ctx.block().icmp_eq(PTR, &state, "null");
    ctx.block()
        .cond_br(&state_null, &slow_label, &chk_top_label);

    // --- slow arm: the runtime function, byte-for-byte the old behaviour ---
    ctx.current_block = slow_idx;
    match what {
        InlineSlotWrite::Bind { local_slot } => ctx.block().call_void(
            "js_shadow_slot_bind",
            &[(I32, &slot_idx.to_string()), (PTR, local_slot)],
        ),
        InlineSlotWrite::Clear => ctx.block().call_void(
            "js_shadow_slot_set",
            &[(I32, &slot_idx.to_string()), (I64, "0")],
        ),
    }
    ctx.block().br(&done_label);

    // --- `if top == usize::MAX { return }` ---
    ctx.current_block = chk_top_idx;
    let frame_top_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &state,
        &[(I64, &SHADOW_STATE_FRAME_TOP_OFFSET.to_string())],
    );
    let frame_top = ctx.block().load(I64, &frame_top_ptr);
    // `usize::MAX` is `-1` as an i64 bit pattern.
    let no_frame = ctx.block().icmp_eq(I64, &frame_top, "-1");
    ctx.block().cond_br(&no_frame, &done_label, &chk_len_label);

    // --- `let slot = top + idx; if slot >= len { return }` ---
    //
    // The `usize::MAX` test above is what makes this addition safe to do
    // untrapped: with `top` ruled out as the sentinel it is a real buffer
    // index, so `top + idx` cannot wrap. Testing only `slot < len` would let
    // the sentinel through — `usize::MAX + idx` wraps to `idx - 1`, which is
    // in bounds and would corrupt a *different* frame's entry.
    ctx.current_block = chk_len_idx;
    let slot = ctx
        .block()
        .add(I64, &frame_top, &u64::from(slot_idx).to_string());
    let len_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &state,
        &[(I64, &SHADOW_STATE_LEN_OFFSET.to_string())],
    );
    let len = ctx.block().load(I64, &len_ptr);
    let in_bounds = ctx.block().icmp_ult(I64, &slot, &len);
    ctx.block().cond_br(&in_bounds, &store_label, &done_label);

    // --- the entry write ---
    ctx.current_block = store_idx;
    let buf = if SHADOW_STATE_PTR_OFFSET == 0 {
        ctx.block().load(PTR, &state)
    } else {
        let p = ctx.block().gep_inbounds(
            crate::types::I8,
            &state,
            &[(I64, &SHADOW_STATE_PTR_OFFSET.to_string())],
        );
        ctx.block().load(PTR, &p)
    };
    let byte_off = ctx.block().shl(I64, &slot, &SHADOW_ENTRY_SHIFT.to_string());
    let entry = ctx
        .block()
        .gep_inbounds(crate::types::I8, &buf, &[(I64, &byte_off)]);
    let meta_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &entry,
        &[(I64, &SHADOW_ENTRY_META_OFFSET.to_string())],
    );

    match what {
        InlineSlotWrite::Bind { local_slot } => {
            // Snapshot the word the mutator just stored. This load sits
            // immediately after the caller's `store` to the same alloca, so
            // LLVM forwards it; it is never re-read at a later safepoint.
            let value = ctx.block().load(I64, local_slot);
            let raw = ctx.block().ptrtoint(local_slot, I64);
            // `bound_slot_meta`: an address whose bit 0 would collide with the
            // liveness tag is recorded active-but-unbound rather than
            // truncated, so the collector is never handed a mis-derived
            // address to write a forwarded pointer into. Compiled local slots
            // are `i64`/`double` allocas and so 8-byte aligned, which lets
            // LLVM fold this select away; it exists for the mis-emitted case.
            let low = ctx
                .block()
                .and(I64, &raw, &SHADOW_SLOT_ACTIVE_BIT.to_string());
            let aligned = ctx.block().icmp_eq(I64, &low, "0");
            let bound = ctx.block().select(I1, &aligned, I64, &raw, "0");
            let meta = ctx
                .block()
                .or(I64, &bound, &SHADOW_SLOT_ACTIVE_BIT.to_string());
            ctx.block().store(I64, &value, &entry);
            ctx.block().store(I64, &meta, &meta_ptr);
            emit_inline_root_shading_barrier(ctx, &value, &done_label);
        }
        InlineSlotWrite::Clear => {
            // Codegen's "dead from here" clear: drop the liveness bit but keep
            // the binding, so a later re-activation still writes through to
            // the same compiled local slot. No shading barrier — a zero value
            // is not a heap reference.
            let old_meta = ctx.block().load(I64, &meta_ptr);
            let cleared = ctx.block().and(
                I64,
                &old_meta,
                &format!("{}", !SHADOW_SLOT_ACTIVE_BIT as i64),
            );
            ctx.block().store(I64, "0", &entry);
            ctx.block().store(I64, &cleared, &meta_ptr);
            ctx.block().br(&done_label);
        }
    }

    ctx.current_block = done_idx;
    true
}

/// The incremental-mark root shading barrier, gated inline on
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`.
///
/// Identical in kind to `emit_persistent_shadow_root_barrier` and to the
/// runtime's own `root_shading_barrier`: a zero count *proves* this thread's
/// `INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null, because
/// `incremental_mark_barrier_enable` installs the thread-local before
/// incrementing the count. Skipping the call on a zero count is therefore
/// observationally identical, not a weaker barrier.
///
/// Terminates the current block with a branch to `done_label`.
fn emit_inline_root_shading_barrier(ctx: &mut FnCtx<'_>, value_bits: &str, done_label: &str) {
    let active =
        ctx.block()
            .load_atomic_seq_cst(I32, "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT", 4);
    let needed = ctx.block().icmp_ne(I32, &active, "0");
    let barrier_idx = ctx.new_block("ss.barrier");
    let barrier_label = ctx.block_label(barrier_idx);
    ctx.block().cond_br(&needed, &barrier_label, done_label);

    ctx.current_block = barrier_idx;
    ctx.block()
        .call_void("js_write_barrier_root_nanbox", &[(I64, value_bits)]);
    ctx.block().br(done_label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Param, Stmt};

    /// A function with one pointer-typed local, reassigned first from another
    /// pointer-typed local (forcing a root bind) and then from a number
    /// (forcing the "dead from here" clear).
    fn rooted_local_ir() -> String {
        let mut hir = HirModule::new("inline_shadow_slot_test");
        hir.functions.push(Function {
            id: 0,
            name: "roots".to_string(),
            type_params: Vec::new(),
            params: vec![Param {
                id: 0,
                name: "o".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 1,
                    name: "a".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::LocalGet(0)),
                },
                // pointer-capable store -> root bind
                Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::LocalGet(0)))),
                // proven-non-pointer store -> root clear
                Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Number(1.0)))),
                Stmt::Return(Some(Expr::LocalGet(1))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    fn roots_body(ir: &str) -> String {
        ir.split("\ndefine ")
            .find(|f| f.contains("@perry_fn_inline_shadow_slot_test__roots("))
            .unwrap_or_else(|| panic!("no `roots` body in IR:\n{ir}"))
            .to_string()
    }

    /// Every emitted inline-store block, in order.
    fn store_blocks(body: &str) -> Vec<String> {
        let mut blocks: Vec<String> = Vec::new();
        let mut current: Option<String> = None;
        for line in body.lines() {
            if line.starts_with("ss.store.") {
                current = Some(String::new());
                continue;
            }
            if let Some(buf) = current.as_mut() {
                if line.trim().starts_with("br ") {
                    blocks.push(std::mem::take(buf));
                    current = None;
                } else {
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
        }
        assert!(
            !blocks.is_empty(),
            "no inline shadow-slot store block emitted; body:\n{body}"
        );
        blocks
    }

    /// The register holding the entry address in a store block: the operand of
    /// the `shl`-indexed `getelementptr` into the entry buffer.
    fn entry_reg(blk: &str) -> String {
        for line in blk.lines() {
            // `  %rN = getelementptr inbounds i8, ptr %rBUF, i64 %rSHIFTED`
            if line.contains("getelementptr inbounds i8, ptr %")
                && line.trim_end().ends_with(|c: char| c.is_ascii_digit())
                && line.contains(", i64 %")
            {
                if let Some(name) = line.trim().split(" = ").next() {
                    return name.trim_start_matches('%').to_string();
                }
            }
        }
        panic!("no entry-address getelementptr in store block:\n{blk}");
    }

    /// The block that performs a *bind* (mirrors a value and sets the liveness
    /// bit), located by content so the fixture's statement order can change
    /// without silently testing the wrong block.
    fn bind_block(body: &str) -> String {
        store_blocks(body)
            .into_iter()
            .find(|b| b.contains("select i1 ") && b.contains("ptrtoint ptr "))
            .unwrap_or_else(|| panic!("no inline bind block in:\n{body}"))
    }

    /// The block that performs a *clear* (zeroes the mirror, masks the
    /// liveness bit out of meta).
    fn clear_block(body: &str) -> String {
        store_blocks(body)
            .into_iter()
            .find(|b| b.contains("store i64 0, ptr %"))
            .unwrap_or_else(|| panic!("no inline clear block in:\n{body}"))
    }

    /// The frame push must go through `js_shadow_frame_enter` and derive the
    /// pop handle from `frame_top`, because the state pointer — not the handle
    /// — is what the inline stores need.
    ///
    /// Sabotage check: point `shadow_frame_push_line` back at
    /// `js_shadow_frame_push` and the first two assertions fail; drop the
    /// `sub` in `shadow_frame_handle_lines` and the last one does.
    #[test]
    fn frame_push_uses_frame_enter_and_derives_the_handle() {
        // This test asserts on the SHADOW-STACK lowering. Native roots are the
        // default now, so it has to say which lowering it is testing.
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let body = roots_body(&rooted_local_ir());
        assert!(
            body.contains("call ptr @js_shadow_frame_enter(i32 "),
            "frame push must go through js_shadow_frame_enter; body:\n{body}"
        );
        assert!(
            !body.contains("@js_shadow_frame_push("),
            "the handle-returning push must no longer be emitted; body:\n{body}"
        );
        assert!(
            body.contains(&format!(
                "getelementptr inbounds i8, ptr %r3, i64 {}",
                SHADOW_STATE_FRAME_TOP_OFFSET
            )) || body.contains(&format!(", i64 {}\n", SHADOW_STATE_FRAME_TOP_OFFSET)),
            "handle recovery must load ShadowStackState::frame_top at offset \
             {SHADOW_STATE_FRAME_TOP_OFFSET}; body:\n{body}"
        );
        assert!(
            body.contains(&format!("sub i64 %r5, {}", SHADOW_STACK_HEADER_SLOTS)),
            "pop handle must be frame_top - {SHADOW_STACK_HEADER_SLOTS}; body:\n{body}"
        );
    }

    /// The hot per-store root write must be a store, not a call.
    ///
    /// Sabotage check: make `emit_inline_slot_bind` return `false` and there is
    /// no `ss.store` block at all.
    #[test]
    fn pointer_store_roots_inline_with_the_runtime_entry_layout() {
        // This test asserts on the SHADOW-STACK lowering. Native roots are the
        // default now, so it has to say which lowering it is testing.
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let body = roots_body(&rooted_local_ir());
        let blk = bind_block(&body);
        assert!(
            blk.contains(&format!(", {}\n", SHADOW_ENTRY_SHIFT)) && blk.contains("shl i64 %"),
            "entry address must index the buffer by shifting the slot index by \
             {SHADOW_ENTRY_SHIFT}; block:\n{blk}"
        );
        assert!(
            blk.contains(&format!(
                "getelementptr inbounds i8, ptr %{}, i64 {}",
                entry_reg(&blk),
                SHADOW_ENTRY_META_OFFSET
            )),
            "meta word must sit at offset {SHADOW_ENTRY_META_OFFSET} of the \
             entry; block:\n{blk}"
        );
        assert!(
            blk.contains(&format!(", {}\n", SHADOW_SLOT_ACTIVE_BIT)) && blk.contains("or i64 %"),
            "inline bind must set the liveness bit in meta, or the scanner \
             skips the root; block:\n{blk}"
        );
        assert!(
            blk.contains("select i1 %") && blk.contains(", i64 0\n"),
            "inline bind must keep `bound_slot_meta`'s alignment fallback so a \
             tag-colliding address is recorded unbound, never truncated; \
             block:\n{blk}"
        );
        // Both words written, value first.
        assert!(
            blk.matches("store i64 %").count() == 2,
            "inline bind must write both the mirrored value and meta; \
             block:\n{blk}"
        );
    }

    /// Both guards must be present. Dropping the sentinel test is not a missing
    /// safety net but a corruption: `usize::MAX + idx` wraps to `idx - 1`,
    /// which passes a `slot < len` test and overwrites a *live* entry of
    /// another frame — the same wrap-around class #7079 fixed in `frame_pop`.
    ///
    /// Sabotage check: delete either guard from `emit_inline_slot_write`.
    #[test]
    fn inline_store_keeps_the_sentinel_and_bounds_guards() {
        // This test asserts on the SHADOW-STACK lowering. Native roots are the
        // default now, so it has to say which lowering it is testing.
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let body = roots_body(&rooted_local_ir());
        assert!(
            body.contains("ss.chk_top") && body.contains("ss.chk_len"),
            "inline store must keep both guards; body:\n{body}"
        );
        assert!(
            body.contains("icmp eq i64 %r13, -1"),
            "frame_top must be tested against the usize::MAX no-frame sentinel; \
             body:\n{body}"
        );
        assert!(
            body.contains("icmp ult i64 %r15, %r17"),
            "slot index must be bounds-checked against ShadowStackState::len; \
             body:\n{body}"
        );
        assert!(
            body.contains(&format!(
                "getelementptr inbounds i8, ptr %r10, i64 {}",
                SHADOW_STATE_LEN_OFFSET
            )),
            "the bounds check must read len at offset {SHADOW_STATE_LEN_OFFSET}; \
             body:\n{body}"
        );
    }

    /// The incremental-mark root shading barrier must survive inlining, gated
    /// on the counter the runtime uses.
    ///
    /// Sabotage check: drop the `emit_inline_root_shading_barrier` call — a
    /// pointer written into a root after the collector scanned roots is then
    /// never shaded, and an in-flight incremental cycle frees a live object.
    #[test]
    fn inline_bind_keeps_the_gated_root_shading_barrier() {
        // This test asserts on the SHADOW-STACK lowering. Native roots are the
        // default now, so it has to say which lowering it is testing.
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let body = roots_body(&rooted_local_ir());
        assert!(
            body.contains(
                "load atomic i32, ptr @PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT seq_cst"
            ),
            "inline bind must gate on the incremental-mark active count; \
             body:\n{body}"
        );
        assert!(
            body.contains("call void @js_write_barrier_root_nanbox(i64 %r23)"),
            "inline bind must shade the value it just stored when a cycle is in \
             flight; body:\n{body}"
        );
    }

    /// The clear must drop the liveness bit while keeping the binding, so a
    /// later re-activation still writes through to the same compiled local.
    ///
    /// Sabotage check: zero the whole meta word instead of masking and the
    /// `-2` constant disappears.
    #[test]
    fn dead_local_clear_is_inline_and_preserves_the_binding() {
        // This test asserts on the SHADOW-STACK lowering. Native roots are the
        // default now, so it has to say which lowering it is testing.
        let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
        let body = roots_body(&rooted_local_ir());
        let blk = clear_block(&body);
        let mask = !(SHADOW_SLOT_ACTIVE_BIT as i64);
        assert!(
            blk.contains(&format!(", {}\n", mask)) && blk.contains("and i64 %"),
            "inline clear must AND meta with !SLOT_ACTIVE ({mask}), keeping the \
             bound address; block:\n{blk}"
        );
        assert!(
            blk.contains("store i64 0, ptr %"),
            "inline clear must zero the mirrored value; block:\n{blk}"
        );
        assert!(
            !blk.contains("@js_write_barrier_root_nanbox"),
            "a zero value is not a heap reference and needs no shading; \
             block:\n{blk}"
        );
    }
}
