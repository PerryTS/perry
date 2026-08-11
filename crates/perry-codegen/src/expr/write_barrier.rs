//! GC write-barrier emission helpers + stream-subclass `super(...)`
//! lowering (extracted from `expr.rs`, issue #1098). Pure move — no
//! logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

/// `GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` (`0xD000`) as a signed
/// i16 — the emitted IR is textual, so the constant is written the way LLVM
/// parses an i16 literal (mirrors `class_field_inline_guard`'s convention).
const LAYOUT_STATE_AND_INTACT_MASK_I16: &str = "-12288";
/// `GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` (`0x9000`), same encoding.
const LAYOUT_SIDE_MASK_INTACT_I16: &str = "-28672";

/// Gen-GC Phase C2 helper: emit a write barrier after heap-store sites
/// by default. Only explicit `PERRY_WRITE_BARRIERS=0`/`off`/`false`
/// disables emission. Sites with a precise field/element address use
/// `js_write_barrier_slot`; opaque helper stores keep using the
/// compatibility wrapper, which conservatively marks the parent span.
/// The env gate is read once and OnceLock-cached at codegen time.
pub(crate) fn emit_write_barrier(ctx: &mut FnCtx<'_>, parent_bits: &str, child_bits: &str) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    let child_bits_value = LoweredValue::js_value_bits(child_bits.to_string());
    ctx.record_lowered_value(
        "WriteBarrier",
        None,
        "write_barrier.child_bits",
        &child_bits_value,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    ctx.block()
        .call_void("js_write_barrier", &[(I64, parent_bits), (I64, child_bits)]);
}

pub(crate) fn emit_write_barrier_slot_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_addr: &str,
    child_bits: &str,
) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    blk.call_void(
        "js_write_barrier_slot",
        &[(I64, parent_bits), (I64, slot_addr), (I64, child_bits)],
    );
}

/// #7511 — `GC_FLAG_TENURED`, the one header bit that decides whether a
/// parent's slot store can possibly need remembering.
///
/// Pinned against the runtime constant by
/// `perry-runtime`'s `gc::tests::inline_generation_gate_contract`; codegen
/// cannot `use` the runtime crate, so the value is duplicated and the test is
/// what keeps the two from drifting.
const GC_FLAG_TENURED_I8: &str = "32"; // 0x20

/// #7511 — emit the `i1` predicate "this store may need remembered-set work",
/// as a **superset** of the condition the runtime barrier itself acts on.
///
/// ## What the barrier actually does on these workloads
///
/// On `push_cls` / `churn_alloc` / `churn` the array push is the only surviving
/// barrier call site, and `PERRY_GC_TRACE` counts 19,945,222 calls of which
/// 19,743,573 (99.0%) end in `parent_not_old_skips` — with
/// `old_to_young_slow_hits == 0` and `new_inserts == 0`. The remembered set is
/// **never inserted into, not once**, yet the call is made 20 million times and
/// costs ~30% of leaf profile. #7536's value-side test cannot reach any of it:
/// `non_pointer_child_skips == 0`, because the pushed value genuinely *is* a
/// heap pointer. The waste is entirely parent-side.
///
/// ## The two clauses
///
/// The call is skipped only when BOTH are false.
///
/// 1. **`gc_flags & GC_FLAG_TENURED`** — the remembered set exists so a minor
///    GC can skip retracing parents it treats as black leaves. Those are
///    exactly the objects that are physically old-gen
///    (`barrier_parent_needs_remembering`'s `classify_heap_generation == Old`)
///    or logically tenured (`GC_FLAG_TENURED`, whose doc records that a tenured
///    object may stay physically in the nursery while "the trace pretends
///    they're old-gen" — `gc/trace.rs:747`). A parent that is neither is
///    **fully traced by every minor GC**, so the edge is rediscovered and needs
///    no record.
///
///    Soundness rests on `Old ⟹ TENURED`, i.e. `!TENURED ⟹ !Old`, so this
///    predicate can only skip a subset of what the runtime already skips — the
///    same superset discipline `emit_may_carry_heap_pointer_check` uses. Every
///    path that places an object into an old-gen block sets the bit in the same
///    breath, and nothing ever clears it:
///      * `gc/copying.rs:612–637` — one `promote` expression selects
///        `arena_alloc_gc_old` AND `GC_FLAG_TENURED`.
///      * `gc/oldgen.rs:1740`, `gc/oldgen.rs:1838` — evacuation / old-page
///        defrag both OR the bit onto the destination header.
///      * `buffer/header.rs:486`, `typedarray/mod.rs:722` — direct old-gen
///        allocations set it immediately after allocating.
///      * `json_tape.rs` allocates through `arena_alloc_gc_old_born_tenured`.
///    The three sites that write the bit all OR it in; none masks it out.
///
/// 2. **`PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT != 0`** — skipping the
///    call also skips `barrier_child_prologue`'s
///    `incremental_mark_barrier_value`, the insertion/SATB shading, which is
///    NOT a generational question and must never be dropped while a cycle is
///    live. A zero count *proves* this thread's
///    `INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null, because
///    `incremental_mark_barrier_enable` installs the thread-local BEFORE
///    incrementing the count (`gc/barrier.rs:676–693`, where that ordering is
///    documented as load-bearing for exactly this reason). This is the same
///    gate, on the same global, that `expr/shadow_inline.rs` and
///    `expr/shadow_slot.rs` already emit for the root shading barrier.
///
/// ## Why a live test and not a static claim
///
/// #7501: even a static layout *declaration* is revoked at runtime. Generation
/// is strictly worse — an object's generation changes underneath any static
/// proof the moment a collection promotes it, and `keep` in `chunk()` crosses
/// hundreds of collections between its allocation and its last push. There is
/// no by-construction proof that a parent is young, which is precisely why this
/// reads the live header at the store instead.
///
/// `parent_handle` must be a **validated, non-forwarded GC user pointer** —
/// the caller is responsible for having tested that, because this dereferences
/// `parent_handle - 7`.
pub(crate) fn emit_parent_may_need_remembering_check(
    blk: &mut LlBlock,
    parent_handle: &str,
) -> String {
    let gc_flags_addr = blk.sub(I64, parent_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let tenured_bits = blk.and(I8, &gc_flags, GC_FLAG_TENURED_I8);
    let is_tenured = blk.icmp_ne(I8, &tenured_bits, "0");
    let active = blk.load_atomic_seq_cst(I32, "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT", 4);
    let incremental_active = blk.icmp_ne(I32, &active, "0");
    blk.or(I1, &is_tenured, &incremental_active)
}

/// #7511 — `js_write_barrier_slot` behind
/// [`emit_parent_may_need_remembering_check`].
///
/// Terminates the current block. On return `ctx.current_block` is the
/// continuation, which every path reaches, so the caller goes on emitting
/// exactly as before.
///
/// Emits nothing at all when barrier emission is compile-time disabled, so
/// `PERRY_WRITE_BARRIERS=0` does not leave a predicate and two blocks wrapped
/// around an empty arm — that knob exists to A/B the barrier's cost, and dead
/// IR in one arm makes the comparison lie (the same reasoning
/// `emit_jsvalue_slot_store_pointer_tested` records).
pub(crate) fn emit_write_barrier_slot_generation_tested(
    ctx: &mut FnCtx<'_>,
    parent_handle: &str,
    parent_bits: &str,
    slot_addr: &str,
    child_bits: &str,
    stem: &str,
) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    let barrier_idx = ctx.new_block(&format!("{}.barrier", stem));
    let done_idx = ctx.new_block(&format!("{}.barrier.done", stem));
    let barrier_label = ctx.block_label(barrier_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let needed = emit_parent_may_need_remembering_check(blk, parent_handle);
        blk.cond_br(&needed, &barrier_label, &done_label);
    }
    ctx.current_block = barrier_idx;
    {
        let blk = ctx.block();
        emit_write_barrier_slot_on_block(blk, parent_bits, slot_addr, child_bits);
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
}

pub(crate) fn emit_root_nanbox_store_on_block(blk: &mut LlBlock, value: &str, root_slot: &str) {
    blk.store(DOUBLE, value, root_slot);
    let value_bits = blk.bitcast_double_to_i64(value);
    blk.call_void("js_write_barrier_root_nanbox", &[(I64, &value_bits)]);
}

pub(crate) fn emit_root_heap_word_store_on_block(
    blk: &mut LlBlock,
    value_bits: &str,
    root_slot: &str,
) {
    blk.store(I64, value_bits, root_slot);
    blk.call_void("js_write_barrier_root_heap_word", &[(I64, value_bits)]);
}

/// GC layout-note emission (refs #1090) — at heap-slot stores whose
/// content is known statically, record the per-slot value type so the
/// generational GC can decide whether the slot can be pointer-free
/// (skipped during minor scan). Unlike `emit_write_barrier_slot_on_block`
/// this fires unconditionally — the runtime fn is a no-op when slot
/// tracking is off.
pub(crate) fn emit_layout_note_slot_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_index: &str,
    value_bits: &str,
) {
    blk.call_void(
        "js_gc_note_slot_layout",
        &[(I64, parent_bits), (I32, slot_index), (I64, value_bits)],
    );
}

/// Scalar-aware layout note: passes the slot's previous value (`old_bits`) so
/// the runtime can skip the thread-local layout hashmap when the store does not
/// change the slot's pointer-ness (scalar-over-scalar). See
/// `js_gc_note_slot_layout_aware`.
pub(crate) fn emit_layout_note_slot_aware_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_index: &str,
    value_bits: &str,
    old_bits: &str,
) {
    blk.call_void(
        "js_gc_note_slot_layout_aware",
        &[
            (I64, parent_bits),
            (I32, slot_index),
            (I64, value_bits),
            (I64, old_bits),
        ],
    );
}

pub(crate) fn emit_array_numeric_write_note_on_block(
    blk: &mut LlBlock,
    array_bits: &str,
    value_bits: &str,
) {
    blk.call_void(
        "js_array_note_numeric_write",
        &[(I64, array_bits), (I64, value_bits)],
    );
}

pub(crate) fn emit_jsvalue_slot_store_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        layout_note_needed,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        false,
        None,
    )
}

/// As [`emit_jsvalue_slot_store_on_block`], but with the string-addref demote
/// and the GC layout note gated **independently** (Phase 4b.1).
///
/// The two calls answer different questions and are provably dead under
/// different conditions: the addref is a no-op unless the value is a heap
/// `STRING_TAG` string, while the note is a no-op when the slot's mask class is
/// already fixed. Class-field stores on a shape-proven receiver can retire one
/// without the other — e.g. `Foo`-typed field ← `LocalGet` elides the note
/// (pointer-masked slot) but must keep the addref (the local could hold a
/// uniquely-owned string that a later `+=` would rewrite in place).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_jsvalue_slot_store_with_flags_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    string_addref_needed: bool,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        string_addref_needed,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        false,
        None,
    )
}

/// As [`emit_jsvalue_slot_store_on_block`] with a caller-supplied `value_bits`,
/// and — like [`emit_jsvalue_slot_store_with_flags_on_block`] — with the
/// string-addref demote gated INDEPENDENTLY of the layout note.
///
/// The array push (#7469) needs both halves at once: it can retire the layout
/// note on a header-proven all-pointer array while the pushed value is a
/// `new C()` whose constructor return override could still make it a
/// uniquely-owned heap string.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_jsvalue_slot_store_with_value_bits_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    value_bits: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    string_addref_needed: bool,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        string_addref_needed,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        false,
        Some(value_bits),
    )
}

/// As [`emit_jsvalue_slot_store_on_block`], but for an **in-place element
/// overwrite** of a slot that already holds a valid value: routes the layout
/// note through `js_gc_note_slot_layout_aware`, which loads the previous slot
/// value and skips the thread-local layout hashmap when neither old nor new is
/// a heap pointer. Use only where the slot is guaranteed initialized (array
/// `arr[i] = …` overwrites), not for fresh-slot appends/literals or object
/// field writes (which are POINTER_FREE-dominated and only pay the extra load).
/// This is the dominant per-write cost on downgraded `any[]` numeric loops
/// (#5094) and gives ~9× on `bench_numeric_array_downgrade` without regressing
/// `bench_object_property`.
pub(crate) fn emit_jsvalue_slot_store_scalar_aware_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        layout_note_needed,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        true,
        None,
    )
}

/// #7511 — emit the `i1` predicate "these NaN-boxed bits MAY carry a heap
/// pointer", as a superset of every heap address the runtime can decode.
///
/// This is the codegen mirror of `perry-runtime::gc::barrier::decode_heap_addr`
/// and `gc::layout::layout_pointer_bearing_bits`, narrowed to the part that can
/// be decided from the top 16 bits alone. Both runtime functions resolve a heap
/// address in exactly two situations:
///
/// - the NaN-box tag is `POINTER_TAG` / `STRING_TAG` / `BIGINT_TAG`
///   (`0x7FFD` / `0x7FFF` / `0x7FFA`), or
/// - the value is a **bare** heap address: all-zero high 16 bits
///   (`decode_heap_addr`'s `(bits >> 48) != 0` reject,
///   `layout_pointer_bearing_bits`' `bits <= POINTER_MASK` range) and at or
///   above a low floor — `0x10000` in `decode_heap_addr`, `0x1000` in
///   `layout_pointer_bearing_bits`. The **lower** of the two floors is used
///   here, so neither runtime predicate can accept a value this one rejects.
///
/// Everything else — every plain IEEE double, `SHORT_STRING_TAG` (inline data,
/// not a pointer), `JS_HANDLE`, the `0x7FFC` primitives, `INT32_TAG`,
/// `STATIC_DISPATCH_TAG` — is rejected by both, so this predicate is `false`
/// for them. The floor is what keeps `0.0` — whose bit pattern is all zeros,
/// and which is one of the most-stored values in any program — on the fast
/// path instead of colliding with the bare-address arm.
///
/// **The direction of the approximation is load-bearing.** The refinements the
/// runtime applies *after* these tests (a null pointer payload, a misaligned
/// address, an arena page-map miss) are all further REJECTIONS. Dropping them
/// can only make this predicate say "maybe" where the runtime would say "no" —
/// an extra call that does nothing. It can never say "no" where the runtime
/// would say "yes", which is the direction that strands a child.
/// `perry-runtime`'s `gc::tests::inline_pointer_bearing_contract` enumerates
/// the entire 16-bit tag space against both runtime predicates to pin exactly
/// that, and `nanbox::inline_pointer_bearing_top16_set_covers_every_heap_tag`
/// pins the comparand set against the tag constants.
pub(crate) fn emit_may_carry_heap_pointer_check(blk: &mut LlBlock, value_bits: &str) -> String {
    use crate::nanbox::{BIGINT_TAG_TOP16_I64, POINTER_TAG_TOP16_I64, STRING_TAG_TOP16_I64};
    // `layout_pointer_bearing_bits`' floor (`0x1000`), the lower of the two.
    const BARE_HEAP_ADDR_FLOOR_I64: &str = "4096";
    let top16 = blk.lshr(I64, value_bits, "48");
    let top16_zero = blk.icmp_eq(I64, &top16, "0");
    let above_floor = blk.icmp_uge(I64, value_bits, BARE_HEAP_ADDR_FLOOR_I64);
    let is_raw_addr = blk.and(I1, &top16_zero, &above_floor);
    let is_pointer_tag = blk.icmp_eq(I64, &top16, POINTER_TAG_TOP16_I64);
    let is_string_tag = blk.icmp_eq(I64, &top16, STRING_TAG_TOP16_I64);
    let is_bigint_tag = blk.icmp_eq(I64, &top16, BIGINT_TAG_TOP16_I64);
    let tagged = blk.or(I1, &is_pointer_tag, &is_string_tag);
    let tagged = blk.or(I1, &tagged, &is_bigint_tag);
    blk.or(I1, &tagged, &is_raw_addr)
}

/// #7511 — a class-field JSValue slot store whose three GC-bookkeeping calls
/// are placed behind ONE inline, live test of the stored value.
///
/// ## Why a live test rather than a wider static proof
///
/// The bookkeeping this guards costs 16.1% of `churn_alloc`'s profile on a
/// program whose stores are all doubles, and #5334 lever D — which elides it
/// for a value that is a non-pointer BY CONSTRUCTION — never fires there. The
/// reason is structural, not a missing arm: since the `[#bloat]`
/// `force_ctor_call` default (`lower_call/new.rs`), a class with its own
/// constructor is NOT inlined at the `new` site. Its body is compiled once as
/// the shared `<class>_constructor(this, p0, …)` symbol, and HIR rewrites every
/// closed-shape object literal into a `New` of a synthesized anon-shape class
/// with exactly that shape (`lower/context.rs::mint_anon_shape_class`). So the
/// expression reaching the field store is `Expr::LocalGet(<ctor param>)` — an
/// LLVM *function argument* of a function shared by every `new` site in the
/// module, including ones passing pointers. No by-construction proof about that
/// value can exist, and a declared `v: number` is not a layout fact (CLAUDE.md,
/// "No runtime type *validation*"): the field legitimately receives a string
/// through an `any`.
///
/// What CAN be decided there is the same question the three callees each ask
/// first, at runtime, one at a time, across three cross-crate calls. Asking it
/// ONCE inline and branching over all three is exactly #7501's shape (a live
/// test at the store standing in for a static claim that cannot be made).
///
/// ## Why each call is dead when the test says "no pointer"
///
/// - `js_write_barrier_slot` → `write_barrier_slot_inner` opens with
///   `barrier_child_prologue`, which returns immediately when
///   `decode_heap_addr(child) == 0`. Nothing else in the barrier runs — not the
///   incremental-mark shading (there is no heap object to shade), not the
///   remembered set (a non-pointer publishes no old→young edge).
/// - `js_string_addref_if_heap_string` is tag-checked and a no-op for every
///   non-`STRING_TAG` value, SSO short strings included.
/// - `js_gc_note_slot_layout` for a non-pointer value can only ever CLEAR mask
///   state, never set it, so skipping it is never the difference between a slot
///   being scanned and a live child being stranded. That is the identical
///   argument `class_field_store_needs_layout_note` already ships for the
///   static case, including its precondition — the caller only reaches here
///   with `requires_raw_f64 == false`, so the note's raw-f64-mask arm (the one
///   that MUST downgrade) is unreachable. Turning a static claim into a live
///   test does not weaken it.
///
/// The store itself stays unconditional and outside the branch — only the
/// bookkeeping moves.
///
/// Callers that already proved the value statically pass all three flags
/// `false`; then no test and no blocks are emitted at all, and lever D's
/// existing elision is unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_jsvalue_slot_store_pointer_tested(
    ctx: &mut FnCtx<'_>,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    string_addref_needed: bool,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
    layout_note_conforming: bool,
) -> Option<String> {
    {
        let blk = ctx.block();
        // GC_STORE_AUDIT(BARRIERED): the slot write itself is unconditional;
        // the barrier below is guarded only by a live test that the stored
        // bits carry no heap pointer, which is the barrier's own first test.
        blk.store(DOUBLE, value_double, slot_ptr);
    }
    // `emit_write_barrier_slot_on_block` emits nothing when barrier emission is
    // compile-time disabled, so counting `write_barrier_needed` alone would put
    // a predicate, a `cond_br` and two blocks around an arm holding only a
    // `br` under `PERRY_WRITE_BARRIERS=0`. That knob exists to A/B the barrier's
    // cost; leaving dead IR in one arm of the A/B is exactly the kind of thing
    // that makes such a comparison lie.
    let write_barrier_emitted = write_barrier_needed && crate::codegen::write_barriers_enabled();
    if !string_addref_needed && !layout_note_needed && !write_barrier_emitted {
        return None;
    }
    let value_bits = ctx.block().bitcast_double_to_i64(value_double);
    let bookkeeping_idx = ctx.new_block("class_field_set.gc_bookkeeping");
    let done_idx = ctx.new_block("class_field_set.gc_bookkeeping.done");
    let bookkeeping_label = ctx.block_label(bookkeeping_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let may_carry_pointer = emit_may_carry_heap_pointer_check(blk, &value_bits);
        blk.cond_br(&may_carry_pointer, &bookkeeping_label, &done_label);
    }
    ctx.current_block = bookkeeping_idx;
    {
        let blk = ctx.block();
        if string_addref_needed {
            blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        }
    }
    // Phase 4b.2 (#5094): a pointer stored into a slot the class's own pointer
    // mask declares is a `Conforms` no-op inside `layout_note_slot` for every
    // receiver that carries an intact side-mask descriptor. Test that header
    // state inline and skip the cross-crate call — see
    // `class_field_store_layout_note_is_conforming` for why the two together
    // are a proof, and why the fallback arm is kept rather than eliding
    // outright.
    if layout_note_needed && layout_note_conforming {
        let note_idx = ctx.new_block("class_field_set.layout_note");
        let after_idx = ctx.new_block("class_field_set.layout_note.done");
        let note_label = ctx.block_label(note_idx);
        let after_label = ctx.block_label(after_idx);
        {
            let blk = ctx.block();
            // GcHeader precedes the object by 8 bytes; `_reserved` is the i16 at
            // -6 (same derivation as `class_field_inline_guard`).
            let obj_ptr = blk.inttoptr(I64, layout_parent_bits);
            let res_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
            let reserved = blk.load(I16, &res_ptr);
            // (GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT) == 0xD000,
            // and the conforming value (GC_LAYOUT_SIDE_MASK | INTACT) == 0x9000.
            // Written signed because the emitted IR is textual i16.
            let masked = blk.and(I16, &reserved, LAYOUT_STATE_AND_INTACT_MASK_I16);
            let conforming = blk.icmp_eq(I16, &masked, LAYOUT_SIDE_MASK_INTACT_I16);
            blk.cond_br(&conforming, &after_label, &note_label);
        }
        ctx.current_block = note_idx;
        {
            let blk = ctx.block();
            emit_layout_note_slot_on_block(blk, layout_parent_bits, slot_index, &value_bits);
            blk.br(&after_label);
        }
        ctx.current_block = after_idx;
    } else if layout_note_needed {
        let blk = ctx.block();
        emit_layout_note_slot_on_block(blk, layout_parent_bits, slot_index, &value_bits);
    }
    {
        let blk = ctx.block();
        if write_barrier_emitted {
            emit_write_barrier_slot_on_block(blk, barrier_parent_bits, slot_addr, &value_bits);
        }
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
    Some(value_bits)
}

#[allow(clippy::too_many_arguments)]
fn emit_jsvalue_slot_store_on_block_inner(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    string_addref_needed: bool,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
    scalar_aware: bool,
    value_bits_override: Option<&str>,
) -> Option<String> {
    // The scalar-aware layout note needs the slot's PREVIOUS value to decide
    // whether the slot's pointer-ness actually changed; load it before the
    // store overwrites it. Only when both a note is needed and the caller opted
    // into the scalar-aware path (the slot is a valid in-place overwrite).
    let old_bits = if scalar_aware && layout_note_needed {
        let old_double = blk.load(DOUBLE, slot_ptr);
        Some(blk.bitcast_double_to_i64(&old_double))
    } else {
        None
    };
    // GC_STORE_AUDIT(BARRIERED): generated heap JSValue stores route through this shared emitter.
    blk.store(DOUBLE, value_double, slot_ptr);
    // A uniquely-owned (refcount==1) string written into this slot now aliases
    // it — demote it to shared so a later in-place `+=` on the source local
    // allocates fresh instead of mutating the stored element. This is the inline
    // codegen choke point the runtime store functions (`js_array_push_f64`, …)
    // are bypassed for on the fast paths. Tag-checked at runtime (a no-op for
    // SSO / non-string), and only emitted when the value can be a heap string
    // (`string_addref_needed`), so numeric stores pay nothing. Most callers tie
    // that to `layout_note_needed`; Phase 4b.1's class-field store gates it
    // separately, because a pointer-masked slot can retire the note while the
    // stored value can still be a uniquely-owned string. Mirrors the
    // object-field demote in `runtime_store_jsvalue_slot` (#5533).
    if string_addref_needed {
        blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
    }
    if !layout_note_needed && !write_barrier_needed {
        return None;
    }
    let value_bits = value_bits_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| blk.bitcast_double_to_i64(value_double));
    if layout_note_needed {
        match old_bits.as_deref() {
            // Scalar-over-scalar stores leave the GC slot layout unchanged — the
            // aware note skips the thread-local layout hashmap when neither the
            // new nor the old value is a heap pointer (#5094).
            Some(old) => emit_layout_note_slot_aware_on_block(
                blk,
                layout_parent_bits,
                slot_index,
                &value_bits,
                old,
            ),
            None => {
                emit_layout_note_slot_on_block(blk, layout_parent_bits, slot_index, &value_bits)
            }
        }
    }
    if write_barrier_needed {
        emit_write_barrier_slot_on_block(blk, barrier_parent_bits, slot_addr, &value_bits);
    }
    Some(value_bits)
}

/// Issue #562 — `super({ ... })` for `class X extends ReadableStream`,
/// `WritableStream`, or `TransformStream`. Extracts the underlying
/// source/sink/transformer callbacks from the inline object literal,
/// lowers each one (TAG_UNDEFINED for missing fields), and calls the
/// runtime `*_subclass_init` shim — which allocates the stream registry
/// handle and stashes it on `this` under `__perry_stream_handle__`.
///
/// `kind` is one of `"readable"` / `"writable"` / `"transform"` —
/// matches the SuperCall arm's `parent_name` switch in expr.rs.
pub(crate) fn lower_stream_super_init(
    ctx: &mut FnCtx<'_>,
    kind: &str,
    super_args: &[Expr],
) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

    // Pre-extract field exprs so we don't hold a borrow across `lower_expr`.
    let opts_props: Option<Vec<(String, Expr)>> = super_args
        .first()
        .and_then(|first| crate::lower_call::extract_options_fields(ctx, first));
    let qstrat_props: Option<Vec<(String, Expr)>> = super_args
        .get(1)
        .and_then(|second| crate::lower_call::extract_options_fields(ctx, second));

    // Lower the canonical callback set per stream kind. Fields not
    // present (or callable arg shape that isn't an inline literal) fall
    // back to TAG_UNDEFINED — matches the existing `new ReadableStream
    // / WritableStream / TransformStream` lowerings in
    // `lower_call/builtin.rs`.
    let mut start = undef_lit.clone();
    let mut pull = undef_lit.clone();
    let mut cancel = undef_lit.clone();
    let mut write = undef_lit.clone();
    let mut close = undef_lit.clone();
    let mut abort = undef_lit.clone();
    let mut transform = undef_lit.clone();
    let mut flush = undef_lit.clone();

    if let Some(props) = opts_props {
        for (k, vexpr) in &props {
            match (kind, k.as_str()) {
                ("readable", "start") => start = lower_expr(ctx, vexpr)?,
                ("readable", "pull") => pull = lower_expr(ctx, vexpr)?,
                ("readable", "cancel") => cancel = lower_expr(ctx, vexpr)?,
                ("writable", "write") => write = lower_expr(ctx, vexpr)?,
                ("writable", "close") => close = lower_expr(ctx, vexpr)?,
                ("writable", "abort") => abort = lower_expr(ctx, vexpr)?,
                ("transform", "transform") => transform = lower_expr(ctx, vexpr)?,
                ("transform", "flush") => flush = lower_expr(ctx, vexpr)?,
                _ => {
                    // Lower for side effects (closure-capture collection,
                    // string-pool registration, etc.) but discard the value.
                    let _ = lower_expr(ctx, vexpr)?;
                }
            }
        }
    } else if let Some(first) = super_args.first() {
        // Caller passed something that isn't a recognized shape — lower
        // for side effects so closure analysis stays consistent.
        let _ = lower_expr(ctx, first)?;
    }

    let mut hwm = double_literal(1.0);
    if let Some(qprops) = qstrat_props {
        for (k, vexpr) in &qprops {
            if k == "highWaterMark" {
                hwm = lower_expr(ctx, vexpr)?;
            } else {
                let _ = lower_expr(ctx, vexpr)?;
            }
        }
    } else if let Some(second) = super_args.get(1) {
        let _ = lower_expr(ctx, second)?;
    }

    // `this` (NaN-boxed pointer) — the runtime shim stashes the handle
    // on it via `js_object_set_field_by_name`.
    let this_slot = ctx.this_stack.last().cloned();
    let this_box = match this_slot {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    let runtime_fn = match kind {
        "readable" => "js_readable_stream_subclass_init",
        "writable" => "js_writable_stream_subclass_init",
        "transform" => "js_transform_stream_subclass_init",
        _ => unreachable!("lower_stream_super_init: unexpected kind {}", kind),
    };

    let blk = ctx.block();
    match kind {
        "readable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &start),
                    (DOUBLE, &pull),
                    (DOUBLE, &cancel),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "writable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &write),
                    (DOUBLE, &close),
                    (DOUBLE, &abort),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "transform" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &transform),
                    (DOUBLE, &flush),
                    (DOUBLE, &hwm),
                ],
            );
        }
        _ => unreachable!(),
    }

    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
}

/// Initialize Node's classic stream base class on an existing subclass
/// instance. Unlike `new Readable/Writable(opts)`, `super(opts)` must keep the
/// object identity allocated for the derived class and attach stream state to it.
pub(crate) fn lower_node_stream_super_init(
    ctx: &mut FnCtx<'_>,
    kind: &str,
    super_args: &[Expr],
) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let opts = if let Some(first) = super_args.first() {
        lower_expr(ctx, first)?
    } else {
        undef_lit.clone()
    };
    for arg in super_args.iter().skip(1) {
        let _ = lower_expr(ctx, arg)?;
    }

    let this_box = match ctx.this_stack.last().cloned() {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    let runtime_fn = match kind {
        "readable" => "js_node_stream_readable_subclass_init",
        "writable" => "js_node_stream_writable_subclass_init",
        "duplex" => "js_node_stream_duplex_subclass_init",
        "transform" => "js_node_stream_transform_subclass_init",
        _ => unreachable!(
            "lower_node_stream_super_init: unexpected Node stream kind {}",
            kind
        ),
    };
    ctx.block()
        .call(DOUBLE, runtime_fn, &[(DOUBLE, &this_box), (DOUBLE, &opts)]);

    Ok(undef_lit)
}

/// `super(n)` for a source-compiled `class X extends Array` (lru-cache's
/// `ZeroArray` etc.): perry models the instance as a plain object, so size it
/// and install the Array surface via the runtime `js_array_subclass_init`
/// (mirrors `lower_node_stream_super_init` / the EventEmitter subclass init).
pub(crate) fn lower_array_super_init(ctx: &mut FnCtx<'_>, super_args: &[Expr]) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let n = if let Some(first) = super_args.first() {
        lower_expr(ctx, first)?
    } else {
        undef_lit.clone()
    };
    for arg in super_args.iter().skip(1) {
        let _ = lower_expr(ctx, arg)?;
    }

    let this_box = match ctx.this_stack.last().cloned() {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    ctx.block().call(
        DOUBLE,
        "js_array_subclass_init",
        &[(DOUBLE, &this_box), (DOUBLE, &n)],
    );

    Ok(undef_lit)
}

/// #5137: install the bare EventEmitter listener/emit surface onto `this_box`
/// for a source-compiled `class X extends EventEmitter` (node:events). Shared
/// by the explicit-`super()` arm (`expr/this_super_call.rs`) and the
/// no-own-constructor `new` path (`lower_call/new.rs`). The runtime helper
/// reuses the generic `ns_*` emitter closures (they key all state off the
/// receiver), so a plain object that never went through a stream constructor
/// gets working `.on`/`.emit`/`.once`/…. Reached when an EventEmitter
/// subclass's real npm source is compiled — e.g. commander's `Command` under
/// `perry.compilePackages`, where the `new Command()` → `js_commander_*`
/// native-shim path is intentionally off.
pub(crate) fn lower_event_emitter_subclass_init(ctx: &mut FnCtx<'_>, this_box: &str) {
    ctx.block().call(
        DOUBLE,
        "js_event_emitter_subclass_init",
        &[(DOUBLE, this_box)],
    );
}
