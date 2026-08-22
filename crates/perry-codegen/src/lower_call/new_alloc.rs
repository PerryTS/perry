//! The `new ClassName(...)` **instance allocation**, split out of `new.rs`
//! (#7615 slice 8).
//!
//! A pure move: this is `lower_new_impl_inner`'s field-count computation and
//! its three-arm object allocation, verbatim, wrapped in one function that
//! returns the raw instance handle. Nothing else in the `new` lowering reads
//! any of the locals it defines (`field_count`, `cid_str`, `parent_cid_str`,
//! `n_str`, `packed_keys`, `alloc_field_count`), which is what made the
//! boundary a boundary rather than a cut.
//!
//! The split exists because `new.rs` reached 1,988 lines against
//! `scripts/check_file_size.sh`'s 2,000-line cap, and the Layer 1 rooting
//! migration (#7615) has to ADD lines to it — `refresh_rooted_args` and the
//! `temp_root_scope_*` marker become a `RootedGroup`. Doing the move as its
//! own commit keeps that diff readable: this file has no rooting decision in
//! it at all, because everything it emits sits ABOVE the instance root (the
//! push is `new.rs`'s first act on the returned handle).

use perry_hir::Class;

use crate::expr::{load_inline_arena_state, FnCtx};
use crate::types::{I32, I64, I8, PTR};

/// #7469: is the `new` site being lowered inside a **loop body**?
///
/// This is the gate on the inline bump allocator. Inlining removes the
/// `js_object_alloc_class_inline_keys` FFI call and, with it, the thread-local
/// resolutions that call performs — measured 1.81× on `churn_alloc`, 1.78× on
/// `push_cls`. But it costs ~268 bytes of machine code **per site** (measured
/// over 10 / 200 / 800-site programs: +0, +49,536, +214,656 bytes), so it
/// cannot be the unconditional default against this repo's binary-size
/// campaign — a 5,000-site application would pay ~1.3 MB.
///
/// Loop membership is the cheapest sound proxy for "this site runs many
/// times", and it bounds the size cost to loop bodies. A `new` executed once
/// keeps the outlined call and contributes nothing to binary growth.
///
/// `loop_targets` is reused rather than a new counter added, because it is
/// already maintained by all three loop lowerings. It also carries `switch`
/// frames, which push an EMPTY continue label (`switch_stmt.rs`) while every
/// loop pushes a real one — the same discriminator `Stmt::Continue`'s
/// scan-outward-past-switch-frames logic uses. A `new` inside a bare `switch`
/// is therefore correctly treated as not-in-a-loop.
fn new_site_is_in_loop(ctx: &FnCtx<'_>) -> bool {
    if ctx
        .loop_targets
        .iter()
        .any(|(continue_label, _, _)| !continue_label.is_empty())
    {
        return true;
    }
    // #7834: a `new` in a function the hot-loop-callee pre-pass admitted is a
    // `new` in a loop, one frame out.
    //
    // The gate below this comment is about SPEED-vs-SIZE, and
    // `collect_hot_loop_callees` answers exactly the question the loop test
    // does — is this site hot enough to be worth ~268 bytes — with the
    // anti-bloat backstop already attached: it admits only a function that
    // (a) has a direct call site inside a loop and (b) has at most
    // `inline_hot_small_max_call_sites` (4) direct call sites in the whole
    // module. So the added code is bounded by 4 × (news in the function),
    // which is the same order the loop arm already accepts.
    //
    // Deliberately NOT `func.inline_hint`: that is this set intersected with a
    // 9..=20-statement window and with "not already `alwaysinline`", and the
    // functions this needs most fall out of BOTH. `makeCycle` is 5 statements,
    // so it is `alwaysinline` and never hinted — while being the single
    // hottest function in the program.
    //
    // `cycles.ts` is the shape that needs it: `makeCycle` is called 10M times
    // from `main`'s loop and allocates two `Cell`s, but its own body has no
    // loop, so both allocations took the outlined
    // `js_object_alloc_class_inline_keys` — 22% of the program's samples, plus
    // a further 5% in `arena_alloc`'s inline-state sync, for work the inline
    // bump does in eight stores.
    //
    // Reading `func.hot_loop_callee` here is well-ordered: `codegen/function.rs`
    // sets it from `cross_module.hot_loop_callees` before the entry block is
    // created and before any expression is lowered.
    if ctx.func.hot_loop_callee {
        return true;
    }
    // #7871: the same question, asked with the right cost model.
    //
    // `hot_loop_callee` above carries `inline_hot_small_max_call_sites` (4),
    // which is `inlinehint`'s anti-bloat backstop — it bounds a cost that
    // scales with CALL SITES because LLVM duplicates the callee body at each
    // one. The inline bump allocator's cost is ~268 bytes per `new` SITE in
    // this function, paid once regardless of how many callers there are. So
    // the cap prices a cost that does not exist here, and it excludes exactly
    // the functions that earn the inline form: a recursive-descent evaluator's
    // hot function has one call site per recursion arm.
    //
    // `gc-handoff/apps/interp.ts`'s `evalNode` had 11 (10 of them its own
    // recursion) and allocated a `Value` per invocation through the outlined
    // call, ~20M times. Whole-corpus A/B with `PERRY_INLINE_NEW=1` (the
    // force-everywhere knob, i.e. a strict superset of this rule): `interp`
    // −16.2%, `iso_miss` −10.4%, `pipeline` −8.4%, zero regressions outside a
    // ±1.6% floor — and 15 of 19 binaries came out byte-identical, so the
    // widening reaches four programs, not the corpus.
    ctx.func.alloc_hot
}

/// Whether the raw inline allocator can publish the class's pre-minted
/// descriptor without asking the runtime to repair its live-slot facts.
fn inline_shape_descriptor_facts_exact(
    canonical_key_count: Option<u32>,
    allocation_field_count: u32,
) -> bool {
    canonical_key_count.is_some_and(|key_count| key_count == allocation_field_count)
}

/// Emit the instance allocation for `new <class_name>(...)` and return the raw
/// object handle (an `i64` user pointer, NOT NaN-boxed).
///
/// Three arms, in the order the original `if`/`else if`/`else` had them:
/// a dynamic-parent subclass (`class X extends _mod.default`), a class with a
/// per-class keys global (inline bump allocator or the outlined
/// `js_object_alloc_class_inline_keys` call, chosen per site by
/// [`new_site_is_in_loop`]), and the `js_object_alloc_class_with_keys`
/// fallback.
///
/// **No rooting decision is made here and none is possible.** The returned
/// handle is live in an SSA register only until the caller's very next
/// emission, which is the `RootedGroup::adopt_emitted` push that roots it for
/// the constructor body; nothing between the allocator call and that push can
/// collect.
/// What [`emit_instance_alloc`] produced: the instance's user pointer, plus
/// whether the allocation already stamped this class's canonical typed-shape
/// layout into the object's `GcHeader` constant (#7834).
pub(super) struct InstanceAlloc {
    pub(super) handle: String,
    /// `true` when the allocator publishes this class's canonical keys and
    /// ShapeId at birth. Both the inline bump and the stamped outlined
    /// allocator provide the structural proof needed by constructor-free
    /// field initialization.
    pub(super) constructor_stores_ready: bool,
    /// `true` ⟹ the header already reads `GC_LAYOUT_POINTER_FREE |
    /// GC_OBJ_TYPED_LAYOUT_INTACT`, so the construction site owes the runtime
    /// only the address-dependent half of `js_gc_declare_typed_shape_layout`
    /// (clearing a recycled address's stale per-object record).
    pub(super) typed_layout_baked: bool,
}

pub(super) fn emit_instance_alloc(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    class: &Class,
) -> InstanceAlloc {
    let mut typed_layout_baked = false;
    let mut constructor_stores_ready = false;
    let handle = emit_instance_alloc_inner(
        ctx,
        class_name,
        class,
        &mut typed_layout_baked,
        &mut constructor_stores_ready,
    );
    InstanceAlloc {
        handle,
        constructor_stores_ready,
        typed_layout_baked,
    }
}

fn emit_instance_alloc_inner(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    class: &Class,
    typed_layout_baked: &mut bool,
    constructor_stores_ready: &mut bool,
) -> String {
    // Compute total field count including inherited parent fields.
    // The runtime allocates at least 8 inline slots regardless, so this
    // mostly matters for shapes >8 fields.
    let mut field_count = class.fields.len() as u32;
    // Imported classes now carry their real field_names from the source
    // module. If the field count is still 0 (no fields info available),
    // use a generous default as a safety net.
    if field_count == 0 && class.constructor.is_none() {
        field_count = 32;
    }
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        if let Some(p) = ctx.classes.get(parent_name).copied() {
            field_count += p.fields.len() as u32;
            parent = p.extends_name.as_deref();
        } else {
            break;
        }
    }
    // Issue #26 / #321: prefer the authoritative per-class field count computed
    // by the source-prefix-disambiguated keys-global builder. The walk above
    // resolves parents via `ctx.classes` — a name-keyed map that holds only
    // ONE same-named stub — so when a cross-module parent name collides
    // (effect's `Type` in SchemaAST.ts vs ParseResult.ts) it counts the wrong
    // parent's fields. Using the keys-global's count keeps the allocated slot
    // count and the header `field_count` in lockstep with the keys array,
    // which `Object.keys()` walks. Falls back to the computed walk when this
    // class has no keys global (anonymous / no-keys path).
    if let Some(&authoritative) = ctx.class_field_counts.get(class_name) {
        field_count = authoritative;
    }
    // #6812 (w16): a per-site empty-literal anon-shape class may carry a
    // compile-time proven builder width. Allocate that many inline slots so
    // the FIRST instance of the site is as wide as the runtime-learned
    // resizes make every later one — a lone under-sized first instance
    // permanently vetoes whole-loop clone eligibility for arrays built at
    // the site. Capacity only: the keys array stays authoritative for
    // enumeration, and the runtime treats header field_count as alloc_limit.
    if class.alloc_width_hint > field_count {
        field_count = class.alloc_width_hint;
    }

    // Allocate the object with the per-class id and (if applicable)
    // parent class id, so the runtime registers the inheritance
    // chain for instanceof / virtual dispatch lookups.
    //
    // Use `js_object_alloc_class_with_keys`, which pre-populates the
    // `keys_array` with the class's field names in declaration order
    // (parent fields first, walking from the deepest ancestor down,
    // then own fields). This is REQUIRED so the LLVM PropertyGet/Set
    // fast path's slot indices match the runtime's by-name dispatch
    // (which walks `keys_array`). Mixing the two access patterns on
    // the same object — e.g. constructor writes via the fast path,
    // PropertyUpdate reads via the runtime helper — only produces
    // consistent results when both agree on the slot mapping.
    //
    // The packed-keys constant is interned via the StringPool. Two
    // classes with the same field-name set + order share one constant.
    let cid = ctx.class_ids.get(class_name).copied().unwrap_or(0);
    let parent_cid = class
        .extends_name
        .as_deref()
        .and_then(|p| ctx.class_ids.get(p).copied())
        .unwrap_or(0);
    let cid_str = cid.to_string();
    let parent_cid_str = parent_cid.to_string();
    let n_str = field_count.to_string();

    // Fast path: if the class has a per-class keys global (built once
    // at module init via `js_build_class_keys_array`), emit INLINE
    // bump-allocator IR — no function call into the runtime at all on
    // the hot path. The runtime exposes a `InlineArenaState` struct
    // (data ptr at offset 0, current bump offset at offset 8, current
    // block size at offset 16) via `js_inline_arena_state()`. Ordinary
    // allocation kernels cache that pointer at entry; self-recursive
    // allocators receive it from their public wrapper and forward it through
    // recursive calls. We then emit a 5-instruction bump check +
    // GcHeader/ObjectHeader
    // store sequence at every `new ClassName()` site. The slow path
    // (block overflow) calls `js_inline_arena_slow_alloc` which syncs
    // the inline state back to the underlying arena, allocates a new
    // block, and updates the inline state.
    //
    // Cycles per inlined alloc on the M-series fast path:
    //    load offset       (1)
    //    add+and align     (2)
    //    add new_offset    (1)
    //    load size + cmp   (2)
    //    cond br           (predicted, 0)
    //    store offset      (1)
    //    load data + gep   (2)
    //    write GcHeader    (1)  — packed i64 store
    //    write ObjectHeader (1)  — class id + ShapeId
    //    write null meta    (1)
    //  total: ~11 cycles vs ~140 cycles for the function-call path.
    //
    // Layout assumption: GcHeader is 8 bytes
    //    {obj_type:u8, gc_flags:u8, _reserved:u16, size:u32}
    // and ObjectHeader is 16 bytes on LP64 and ILP32 (#8047)
    //    {class_id:u32, parent_class_id:u32, meta:*ptr [, ILP32 pad:u32]}
    // followed by `max(field_count, INLINE_SLOT_FLOOR)` 8-byte field
    // slots. The user pointer the rest of the codegen sees is `raw + 8`
    // (i.e. the ObjectHeader address) — same as what
    // `js_object_alloc_class_inline_keys` returns.
    //
    // #8113 note on the SHAPE WORD: `parent_class_id` carries the
    // module-init ShapeId, and that descriptor is now the ONLY record of
    // the object's live inline-slot bound. The `descriptor_facts_exact`
    // gate below is therefore load-bearing, not an optimization: an
    // inline allocation whose slot bound differs from the id's descriptor
    // would publish an object the runtime bounds-checks against the WRONG
    // number. Mismatches take the outlined
    // `js_object_alloc_class_inline_keys_stamped` entry point, which
    // installs an exact local descriptor.
    //
    // Layout constants are duplicated here from the runtime; if
    // `GcHeader` or `ObjectHeader` ever change in
    // `crates/perry-runtime/src/{gc,object}.rs`, update both sides.
    if class.extends_expr.is_some() {
        // Wall 45: dynamic-parent subclass (`class X extends _mod.default`).
        // The parent's field layout is unknown at this compile time (the
        // `extends` target is an unresolvable cross-module value, so the
        // parent-chain walk above contributed 0 fields and `field_count` /
        // `packed_keys` cover only X's OWN fields). Allocating with that
        // own-only layout under-sizes and mis-lays-out the instance: the
        // parent's constructor and inherited methods address the inherited
        // fields at the PARENT's slot indices (parent fields first), which fall
        // past X's own slots → OOB heap reads (captures read as garbage).
        // Route to `js_object_alloc_class_dynamic_parent`, which resolves the
        // runtime-registered parent edge + keys-array (both established at
        // module init by `js_register_class_parent_dynamic` /
        // `js_build_class_keys_array`, before any `new X()`) and allocates with
        // the merged `[parent keys..] ++ [own keys..]` layout. Bypasses the
        // inline bump-alloc fast path (which would bake the wrong layout).
        let mut packed_keys = String::new();
        for f in &class.fields {
            if f.key_expr.is_some() {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();
        ctx.block().call(
            I64,
            "js_object_alloc_class_dynamic_parent",
            &[
                (I32, &cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    } else if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
        // Both arms below stamp the canonical class keys and ShapeId. The
        // outlined arm may allocate an old-generation object when a learned
        // width crosses the large-object threshold; constructor-free pointer
        // stores retain the ordinary generation-tested write barrier.
        *constructor_stores_ready = true;
        // [#bloat] Outline the per-`new`-site allocator EXCEPT inside a loop.
        //
        // Outlining collapses ~145 lines of per-class-constant IR per site into
        // a single `js_object_alloc_class_inline_keys` call (~3 lines), which
        // performs the identical bump alloc + header init + slot zero-fill and
        // returns the same user pointer. That is the right default for code
        // size, and the size half of the original measurement still holds:
        // ~268 bytes of machine code per site, +214,656 bytes over an 800-site
        // program.
        //
        // The SPEED half of that measurement has since inverted. It read
        // "~17% faster on an 8M-allocation loop (the inline bump bloated the
        // hot loop, hurting icache/regalloc more than the saved call)"; today
        // the outlined form is **1.81× SLOWER** on `churn_alloc` and 1.78× on
        // `push_cls`. Nothing about the inline bump changed — everything
        // *around* the allocation got cheaper (#7474, #7486, #7487, #7501,
        // #7525, #7532, #7535, #7536, #7552), so the surviving FFI call and
        // the thread-local resolutions it performs now dominate what the
        // inline bump's code bloat costs. On Darwin those resolutions cannot
        // be made cheaper — Mach-O has no local-exec TLS model, and building
        // the runtime with `-Ztls-model=local-exec` leaves the `blr` through
        // the TLV descriptor byte-identical (measured: 1.02×). Only their
        // COUNT can be reduced, and inlining removes them outright.
        //
        // So the choice is per site, not global: a `new` inside a loop takes
        // the inline bump (it runs many times, and the size cost is bounded to
        // loop bodies); everything else keeps the outlined call and
        // contributes nothing to binary growth. `PERRY_INLINE_NEW=1` forces
        // the inline form for A/B measurement only when the exact descriptor
        // facts below admit raw inline allocation; missing or mismatched facts
        // still use the outlined entry point.
        //
        // NOTE the env test is `is_none()`: `PERRY_INLINE_NEW=""` *enables*
        // the inline path, because an empty string is `Some("")`.
        let force_inline_new = std::env::var_os("PERRY_INLINE_NEW").is_some();
        // #8067: the raw inline allocator cannot ask the runtime to validate
        // descriptor facts after writing the ShapeId. Admit it only when the
        // allocation's live-slot bound exactly equals the module-init keys
        // count used to mint that id. Width-hinted/mismatched allocations use
        // the outlined entry point, which installs an exact local descriptor.
        let descriptor_facts_exact = inline_shape_descriptor_facts_exact(
            ctx.class_field_counts.get(class_name).copied(),
            field_count,
        );
        if !descriptor_facts_exact || (!force_inline_new && !new_site_is_in_loop(ctx)) {
            let keys_slot = if let Some(s) = ctx.class_keys_slots.get(class_name).cloned() {
                s
            } else {
                let s = crate::expr::entry_init_load_rooted_global(ctx, &keys_global_name, I64);
                ctx.class_keys_slots
                    .insert(class_name.to_string(), s.clone());
                s
            };
            let keys_ptr = ctx.block().load(I64, &keys_slot);
            let shape_id =
                crate::typed_shape::load_class_shape_id(ctx, class_name, &keys_global_name);
            ctx.pending_declares.push((
                "js_object_alloc_class_inline_keys_stamped".to_string(),
                I64,
                vec![I32, I32, I32, I64, I32],
            ));
            ctx.block().call(
                I64,
                "js_object_alloc_class_inline_keys_stamped",
                &[
                    (I32, &cid_str),
                    (I32, &parent_cid_str),
                    (I32, &field_count.to_string()),
                    (I64, &keys_ptr),
                    (I32, &shape_id),
                ],
            )
        } else {
            // Compile-time layout constants.
            const GC_HEADER_SIZE: u64 = 8;
            // `size_of::<ObjectHeader>()` is 16 on LP64 and padded ILP32.
            // Derive from the target
            // triple so the inline alloc size and field-region base match the
            // target-compiled runtime (no-op on 64-bit; see `target_layout`).
            let object_header_size: u64 =
                crate::target_layout::object_header_size_bytes(ctx.target_triple);
            // #6759 Phase B: pointer width for the trailing `meta` header
            // field (computed here, before `ctx.block()` mutably borrows).
            let meta_ptr_size: u64 = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
                4
            } else {
                8
            };
            const FIELD_SLOT_SIZE: u64 = 8;
            // Inline-slot floor — MUST match perry-runtime `object::INLINE_SLOT_FLOOR`
            // (they independently pad `new` objects to the same minimum; a mismatch
            // where codegen allocs fewer slots than the runtime's get/set bound-check
            // assumes is heap corruption). Single source of truth, paired with the
            // runtime by `target_layout::tests::inline_slot_floor_matches_runtime`.
            // Lowered 8->4 (#6712) then 4->2 (#7916) to shrink small-object footprint.
            const MIN_FIELD_SLOTS: u64 = crate::target_layout::INLINE_SLOT_FLOOR;
            const GC_TYPE_OBJECT: u64 = 2;
            const GC_FLAG_ARENA: u64 = 0x02;
            // PR #1146: pointer-free hint for inline-allocated regular
            // objects. The field-store sites issue per-slot
            // `js_gc_note_slot_layout` so the GC sees real pointer-bearing
            // slots regardless of this initial tag.
            const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
            /// `GC_OBJ_TYPED_LAYOUT_INTACT` — the bit
            /// `class_field_inline_guard` requires before it will read or write
            /// a raw-f64 slot directly. Runtime-side name:
            /// `gc::layout::GC_OBJ_TYPED_LAYOUT_INTACT`.
            const GC_OBJ_TYPED_LAYOUT_INTACT: u64 = 0x1000;

            // #7834: when this class's canonical layout is declarable at
            // allocation AND its pointer mask is statically empty, the state
            // this header already carries (`GC_LAYOUT_POINTER_FREE`) is the
            // FINAL one, and the only thing `js_gc_declare_typed_shape_layout`
            // would add per instance is the intact bit. Stamping it into the
            // same constant store removes the call: on `churn_alloc` /
            // `push_cls` that call was ~30% of the program, almost all of it
            // re-deriving per object a fact that is a property of the SHAPE
            // (see `gc::shape_install`'s module docs — the memo already reduced
            // the map round-trip to a direct-mapped probe, and what is left is
            // that probe, the type-table lookup, and the call itself).
            //
            // Requires `field_count == slot_count`: that mismatch is the one
            // case `init_typed_shape_layout` answers by DOWNGRADING
            // (`layout_set_typed_unknown`), and a constant cannot express "it
            // depends". Computed here, before `ctx.block()` takes its mutable
            // borrow.
            let inline_typed_layout =
                super::typed_shape_init::layout_at_allocation(ctx, class_name, field_count);
            *typed_layout_baked = inline_typed_layout.is_baked();
            let (layout_bits, typed_intact_bits) = match inline_typed_layout {
                crate::target_layout::InlineTypedLayout::None => (GC_LAYOUT_POINTER_FREE, 0),
                crate::target_layout::InlineTypedLayout::PointerFree => {
                    (GC_LAYOUT_POINTER_FREE, GC_OBJ_TYPED_LAYOUT_INTACT)
                }
                crate::target_layout::InlineTypedLayout::SideMask => {
                    (0x8000, GC_OBJ_TYPED_LAYOUT_INTACT)
                }
            };

            let alloc_field_count = std::cmp::max(field_count as u64, MIN_FIELD_SLOTS);
            let payload_size = object_header_size + alloc_field_count * FIELD_SLOT_SIZE;
            // Round the whole allocation up to FIELD_SLOT_SIZE (8). The inline
            // bump allocator's offset invariant (below) requires every
            // allocation to be a multiple of 8. #8047 makes the object header
            // 16 bytes on both pointer widths, so this is currently a no-op.
            let total_size = (GC_HEADER_SIZE + payload_size).next_multiple_of(FIELD_SLOT_SIZE);
            let total_size_str = total_size.to_string();

            // Inline bump-allocator IR.
            let state_ptr = load_inline_arena_state(ctx);
            let blk = ctx.block();

            // offset = state.offset (at byte offset 8 in InlineArenaState).
            // The offset is invariant 8-aligned: arena blocks start at offset 0
            // (8-aligned), every allocation is a multiple of 8 (`total_size`
            // includes the 8-byte GcHeader and `MIN_FIELD_SLOTS=2` slots ×
            // 8 bytes), and `js_inline_arena_slow_alloc` only ever swings the
            // state to `block.offset` which is also always 8-aligned. So we
            // skip the `(offset + 7) & -8` align-up step entirely — saves
            // 2 instructions per iter on the hot path.
            let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
            let offset_val = blk.load(I64, &offset_field_ptr);
            let aligned_off = offset_val.clone();

            // new_offset = aligned + total_size
            let new_offset = blk.add(I64, &aligned_off, &total_size_str);

            // size = state.size (at byte offset 16)
            let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
            let size_val = blk.load(I64, &size_field_ptr);

            // fits = new_offset <= size
            let fits = blk.icmp_ule(I64, &new_offset, &size_val);

            // Set up fast/slow/merge basic blocks.
            let fast_idx = ctx.new_block("alloc.fast");
            let slow_idx = ctx.new_block("alloc.slow");
            let merge_idx = ctx.new_block("alloc.merge");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            let merge_label = ctx.block_label(merge_idx);

            ctx.block().cond_br(&fits, &fast_label, &slow_label);

            // ---- Fast path: bump and return data + aligned ----
            ctx.current_block = fast_idx;
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): inline arena bump offset is allocator metadata, not a JS heap edge.
            blk.store(I64, &new_offset, &offset_field_ptr);
            // data ptr is at byte offset 0 in InlineArenaState
            let data_ptr = blk.load(PTR, &state_ptr);
            let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
            let fast_pred_label = blk.label.clone();
            blk.br(&merge_label);

            // ---- Slow path: call into the runtime ----
            ctx.current_block = slow_idx;
            let raw_slow = ctx.block().call(
                PTR,
                "js_inline_arena_slow_alloc",
                &[(PTR, &state_ptr), (I64, &total_size_str), (I64, "8")],
            );
            let slow_pred_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            // ---- Merge: phi the raw pointer, write headers, NaN-box ----
            ctx.current_block = merge_idx;
            let blk = ctx.block();
            let raw = blk.phi(
                PTR,
                &[(&raw_fast, &fast_pred_label), (&raw_slow, &slow_pred_label)],
            );

            // Write the 16-byte header prefix — GcHeader (8 bytes) followed by
            // the first ObjectHeader word — with ONE `<2 x i64>` store.
            //
            // GcHeader packing (little-endian):
            //   bits  0..7   = obj_type (u8)
            //   bits  8..15  = gc_flags (u8)
            //   bits 16..31  = _reserved (u16)
            //   bits 32..63  = size (u32)
            //
            // The ObjectHeader word at raw + 8: #8113 collapsed the two packed
            // words into one: `class_id` (u32, low) | ShapeId (u32, high).
            // The module-init runtime call either publishes a usable ShapeId
            // or fail-stops on exhaustion; there is no pointer-token fallback.
            // The deleted `object_type` was a constant and the deleted
            // `field_count` is now the ShapeId descriptor's
            // `live_inline_slot_count`, which the `descriptor_facts_exact`
            // gate above proved equals this site's `field_count`.
            //
            // #8122: the pair is composed ONCE per function
            // (`LlFunction::entry_init_object_header_image`) rather than
            // stored as two scalars here. With `object_type` gone the second
            // word is no longer a constant, so two scalar stores made LLVM
            // rematerialise the 40-bit `gc_packed` immediate at every
            // allocation (`mov` + two `movk`) and shift/or the ShapeId per
            // site — measured +4.5 instructions per `new`. The vector image is
            // one live register (or one reload) and one `str q` per
            // allocation, the shape the pre-#8113 constant pair compiled to.
            let gc_packed: u64 = crate::target_layout::inline_alloc_gc_packed(
                ctx.target_triple,
                field_count,
                inline_typed_layout,
            );
            debug_assert_eq!(
                gc_packed,
                GC_TYPE_OBJECT
                    | (GC_FLAG_ARENA << 8)
                    | ((layout_bits | typed_intact_bits) << 16)
                    | ((total_size as u64) << 32),
                "inline_alloc_gc_packed must reproduce this site's packed header word"
            );
            // Prefer the module-level image global — composed once at module
            // init from the SAME `inline_alloc_gc_packed` derivation — and use
            // it only when the table's packed word equals this site's own. A
            // header word is not something to take on trust; a mismatch means
            // the per-function compose (below) is used instead of a wrong
            // header. `image_slot` is an entry-hoisted copy (like the keys
            // global), so a site inside a loop or a recursive allocator pays
            // one vector load per function entry and one `str q` per `new`.
            let image_key = (class_name.to_string(), gc_packed);
            let image_source = if let Some(source) = ctx.class_header_images.get(&image_key) {
                source.clone()
            } else {
                let module_image = ctx
                    .class_header_image_globals
                    .get(class_name)
                    .filter(|(_, module_gc_packed, module_cid)| {
                        *module_gc_packed == gc_packed && *module_cid == cid
                    })
                    .map(|(global, _, _)| global.clone());
                let source = if let Some(image_global) = module_image {
                    crate::expr::HeaderImageSource::EntrySlot(
                        ctx.func.entry_init_load_global(&image_global, "<2 x i64>"),
                    )
                } else {
                    // Fallback: compose the pair once per function from the
                    // ShapeId global's entry slot.
                    let shape_slot = crate::typed_shape::ensure_class_shape_slot(
                        ctx,
                        class_name,
                        &keys_global_name,
                    );
                    crate::expr::HeaderImageSource::EntryValue(
                        ctx.func
                            .entry_init_object_header_image(&shape_slot, gc_packed, cid),
                    )
                };
                ctx.class_header_images.insert(image_key, source.clone());
                source
            };
            let header_image = match image_source {
                crate::expr::HeaderImageSource::EntrySlot(slot) => {
                    ctx.block().load("<2 x i64>", &slot)
                }
                crate::expr::HeaderImageSource::EntryValue(value) => value,
            };
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): inline headers initialize freshly allocated unpublished object storage.
            blk.emit_raw(format!(
                "store <2 x i64> {}, ptr {}, align 8",
                header_image, raw
            ));

            // #6759 Phase B: null the `meta` record pointer — the LAST header
            // field, at header offset (object_header_size - pointer_size).
            // Pointer-width store: on ILP32 the field is 4 bytes at a
            // 4-aligned offset, and an i64 store there would violate the
            // arm64_32 `i64:64` ABI alignment (and spill into slot 0).
            let meta_off = GC_HEADER_SIZE + object_header_size - meta_ptr_size;
            let meta_addr = blk.gep(I8, &raw, &[(I64, &meta_off.to_string())]);
            // GC_STORE_AUDIT(INIT): fresh inline object starts with no per-object meta record (#6759 B).
            let meta_store_ty = if meta_ptr_size == 4 { I32 } else { I64 };
            blk.store(meta_store_ty, "0", &meta_addr);

            // PerryTS/perry#4717: zero-fill the field slots with `undefined`, mirroring
            // `js_object_alloc_with_parent` (runtime object/alloc.rs), which deliberately
            // initializes ALL `max(field_count, 8)` slots "to prevent stale data from
            // previously freed GC objects from bleeding through." This inline bump path
            // wrote only the headers and left the slots uninitialized, so a field
            // read-before-write — or a GC that scans the still-constructing instance —
            // observed stale arena bytes. When those bytes were a previously-freed
            // `undefined`/pointer (e.g. `marked`'s `this.defaults`), the constructor
            // crashed with "Cannot read properties of undefined". Slots start
            // at raw + GcHeader(8) + ObjectHeader(16) = raw + 24 (#8047).
            for i in 0..alloc_field_count {
                let slot_off = GC_HEADER_SIZE + object_header_size + i * FIELD_SLOT_SIZE;
                let slot_ptr = blk.gep(I8, &raw, &[(I64, &slot_off.to_string())]);
                // GC_STORE_AUDIT(INIT): freshly allocated inline object slot initialized to undefined.
                blk.store(I64, crate::nanbox::TAG_UNDEFINED_I64, &slot_ptr);
            }

            // User pointer = raw + 8 (the ObjectHeader address — what the
            // function-call path returned). Convert to i64 to match what
            // the existing nanbox_pointer_inline expects.
            let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
            blk.ptrtoint(&user_ptr, I64)
        }
    } else {
        // Fallback: build the packed-keys string at this site and
        // call the slower SHAPE_CACHE-aware allocator. Used when the
        // class isn't in `class_keys_globals` (e.g. anonymous /
        // synthetic classes that compile_module doesn't pre-emit a
        // global for).
        let mut packed_keys = String::new();
        let mut parent_chain: Vec<&perry_hir::Class> = Vec::new();
        let mut p = class.extends_name.as_deref();
        while let Some(parent_name) = p {
            if let Some(pc) = ctx.classes.get(parent_name).copied() {
                parent_chain.push(pc);
                p = pc.extends_name.as_deref();
            } else {
                break;
            }
        }
        // Skip computed-key fields: their key is an expression evaluated at
        // construction time, not a stable string, so they don't get an inline
        // slot. The runtime stores them via IndexSet → js_object_set_field /
        // js_object_set_symbol_property paths in `apply_field_initializers_recursive`.
        // Including their synthetic `__computed_field_*` names in packed_keys
        // would surface them as enumerable own properties on Object.keys().
        for pc in parent_chain.iter().rev() {
            for f in &pc.fields {
                if f.key_expr.is_some() {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &class.fields {
            if f.key_expr.is_some() {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        ctx.block().call(
            I64,
            "js_object_alloc_class_with_keys",
            &[
                (I32, &cid_str),
                (I32, &parent_cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::inline_shape_descriptor_facts_exact;

    #[test]
    fn raw_inline_shape_stamp_requires_exact_descriptor_facts() {
        assert!(inline_shape_descriptor_facts_exact(Some(5), 5));
        assert!(!inline_shape_descriptor_facts_exact(Some(5), 8));
        assert!(!inline_shape_descriptor_facts_exact(None, 5));
    }
}
