//! Where a `new ClassName(…)` site establishes the instance's canonical
//! typed-shape layout — and, since #7510, *when*.
//!
//! There are two runtime entry points and they differ in one thing: whether
//! the descriptor is **observed** from the instance's current slot contents or
//! **declared** over a fresh one.
//!
//! - [`emit_typed_shape_layout_init`] emits `js_gc_init_typed_shape_layout`
//!   after the constructor has run. The runtime validates every slot before
//!   promoting, so this form needs no proof from codegen at all.
//! - [`emit_typed_shape_layout_declare`] emits
//!   `js_gc_declare_typed_shape_layout` at the allocation site, before the
//!   constructor. The runtime skips validation — it has to, since a fresh slot
//!   holds `TAG_UNDEFINED` and would fail it — so the proof moves here.
//!
//! The second form exists because the first arrives too late to matter: a
//! raw-f64 class-field store *inside* a constructor tests the
//! `GC_OBJ_TYPED_LAYOUT_INTACT` bit, which the post-constructor install has not
//! set yet, so every such store fell back to `js_put_value_set` (#7512). One
//! predicate, [`layout_declared_at_allocation`], chooses between them, and both
//! emitters consult it — the declaration is emitted **iff** the
//! post-constructor install is suppressed, so they cannot drift into
//! double-installing or into leaving an instance with no descriptor.
//!
//! Split out of `new.rs` to stay under the repo's 2000-line-per-file cap
//! (`scripts/check_file_size.sh`).

use crate::expr::FnCtx;
use crate::types::{I32, I64, PTR};

/// #7510: may `class_name`'s layout be declared at allocation instead of
/// validated after the constructor?
///
/// Resolves the class and hands both halves of the proof to
/// [`crate::typed_shape::class_layout_declarable_at_allocation`], which
/// documents what they are and why they are enough.
pub(super) fn layout_declared_at_allocation(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    if !ctx.class_keys_globals.contains_key(class_name) {
        return false;
    }
    let single = ctx.classes.get(class_name).is_some_and(|class| {
        let prologue = super::field_init::ctor_prologue_param_assigned_fields(class);
        crate::typed_shape::class_layout_declarable_at_allocation(class, &prologue)
    });
    if single {
        return true;
    }
    // #7512-followup: the single-class rule refuses every class with heritage,
    // which denies an at-allocation declaration to every subclass instance and
    // puts every constructor store on the whole chain — the base class's own
    // included — on the by-name fallback. Try the chain form.
    super::field_init::chain_prologue_assigned_fields(ctx.classes, class_name).is_some_and(
        |chain| {
            crate::typed_shape::class_chain_layout_declarable_at_allocation(ctx.classes, &chain)
        },
    )
}

/// #7834: is `class_name`'s at-allocation declaration expressible as a
/// **constant** — the state `GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT`
/// stamped straight into the inline-bump path's packed `GcHeader` store?
///
/// Three conditions, and each maps to one branch of
/// `gc::layout::init_typed_shape_layout` that would otherwise decide it at
/// runtime, per instance:
///
/// 1. [`layout_declared_at_allocation`] — the declare form is what would have
///    been emitted at all, so the fresh-slot proof is already discharged.
/// 2. The pointer mask is **statically empty**, so the state is
///    `GC_LAYOUT_POINTER_FREE` and the shape needs no `SHAPE_LAYOUTS`
///    descriptor: with no pointer-bearing slot there is nothing for a mask to
///    select, and `heap_payload_slot_selection` skips the payload outright.
/// 3. `field_count == slot_count` — the runtime's one *downgrading* branch
///    (`layout_set_typed_unknown`), which a constant cannot express.
///
/// What is deliberately NOT folded in is `layout_forget_object`: it depends on
/// the recycled ADDRESS, not on the shape. The caller emits it separately,
/// behind the `PERRY_PER_OBJECT_LAYOUTS_ANY` gate.
pub(super) fn layout_pointer_free_at_allocation(
    ctx: &FnCtx<'_>,
    class_name: &str,
    field_count: u32,
) -> bool {
    if !layout_declared_at_allocation(ctx, class_name) {
        return false;
    }
    let Some(typed_layout) = resolve_typed_layout(ctx, class_name) else {
        return false;
    };
    typed_layout.pointer_mask_words.is_empty() && typed_layout.slot_count == field_count
}

/// Emit the `js_gc_declare_typed_shape_layout` call that registers a **freshly
/// allocated** instance's layout, before its constructor runs, so the
/// constructor's own field stores can pass the intact-bit guard (#7510/#7512).
///
/// No-op unless [`layout_declared_at_allocation`] holds. **Must be emitted
/// while the instance's slots are still the allocator's fill** — that is the
/// runtime contract, and it is not checkable from the runtime side.
///
/// #7834: when the allocation already stamped the layout into the header
/// constant (`typed_layout_baked`), the shape half of this call is already
/// done and only the address half is left — clearing whatever per-object
/// record a previous tenant of this recycled address left behind. That is
/// gated on a process-global emptiness proof, so the steady state is one
/// never-taken branch instead of a six-argument runtime call.
pub(super) fn emit_typed_shape_layout_declare(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
    typed_layout_baked: bool,
) {
    if !layout_declared_at_allocation(ctx, class_name) {
        return;
    }
    if typed_layout_baked {
        emit_gated_forget_object_layout(ctx, obj_handle);
        return;
    }
    emit_typed_shape_layout_call(
        ctx,
        class_name,
        obj_handle,
        "js_gc_declare_typed_shape_layout",
    );
}

/// `if (PERRY_PER_OBJECT_LAYOUTS_ANY) js_gc_forget_object_layout(obj);`
///
/// The count is the runtime's authoritative atomic state (#7873), not a mirror:
/// a zero load proves no thread is armed, while a non-zero count conservatively
/// takes the call. A never-taken branch per allocation is the entire
/// steady-state cost.
fn emit_gated_forget_object_layout(ctx: &mut FnCtx<'_>, obj_handle: &str) {
    let call_idx = ctx.new_block("layout_forget.armed");
    let done_idx = ctx.new_block("layout_forget.done");
    let call_label = ctx.block_label(call_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let any = blk.load_atomic_monotonic(crate::types::I32, "@PERRY_PER_OBJECT_LAYOUTS_ANY", 4);
        let armed = blk.icmp_ne(crate::types::I32, &any, "0");
        blk.cond_br(&armed, &call_label, &done_label);
    }
    ctx.current_block = call_idx;
    ctx.block()
        .call_void("js_gc_forget_object_layout", &[(I64, obj_handle)]);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

/// Emit the `js_gc_init_typed_shape_layout` call that registers the freshly
/// constructed instance's raw-f64 / pointer slot masks with the GC so the
/// typed-feedback class-field fast path engages. Must run AFTER the constructor
/// body has set the declared fields to their numeric values (the runtime
/// validates each raw-f64 slot currently holds a plain double before
/// promoting). No-op for classes without an inline-keys shape global. Refs the
/// standalone `<class>_constructor` symbol path, which previously returned
/// before reaching this — leaving every numeric class field permanently on the
/// by-name hashmap fallback (10M `counter.increment()` ran ~640ns/call instead
/// of slot-direct).
///
/// #7510: suppressed for a class already declared at its allocation site. Every
/// store since has maintained that descriptor — or downgraded it, which
/// re-validating here must not silently undo — and re-installing would cost
/// exactly the work this ticket removes.
pub(super) fn emit_typed_shape_layout_init(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
) {
    if layout_declared_at_allocation(ctx, class_name) {
        return;
    }
    emit_typed_shape_layout_call(ctx, class_name, obj_handle, "js_gc_init_typed_shape_layout");
}

/// The class's canonical typed slot layout, resolved the way both emitters
/// need it.
///
/// Refs #5094: prefer the prefix-disambiguated chain so slot/word counts agree
/// with the mask globals emitted in `compile_module` (same-named cross-module
/// parents mis-resolve in the name-keyed walk). Returns `None` when the class
/// has no keys global, which is the same condition
/// [`emit_typed_shape_layout_call`] bails on.
fn resolve_typed_layout(
    ctx: &FnCtx<'_>,
    class_name: &str,
) -> Option<crate::typed_shape::TypedShapeLayout> {
    ctx.class_keys_globals.get(class_name)?;
    Some(
        ctx.class_init_chains
            .get(class_name)
            .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
            .unwrap_or_else(|| crate::typed_shape::class_typed_layout(ctx.classes, class_name)),
    )
}

/// The shared operand build. Both entry points take the identical six-argument
/// signature, so the only thing that varies is the callee name.
fn emit_typed_shape_layout_call(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
    callee: &str,
) {
    let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() else {
        return;
    };
    let Some(typed_layout) = resolve_typed_layout(ctx, class_name) else {
        return;
    };
    let slot_count_str = typed_layout.slot_count.to_string();
    let raw_mask_word_count_str = typed_layout.raw_f64_mask_words.len().to_string();
    let pointer_mask_word_count_str = typed_layout.pointer_mask_words.len().to_string();
    let raw_mask_ref = if typed_layout.raw_f64_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::raw_f64_mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    let pointer_mask_ref = if typed_layout.pointer_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    ctx.block().call_void(
        callee,
        &[
            (I64, obj_handle),
            (I32, &slot_count_str),
            (PTR, &raw_mask_ref),
            (I32, &raw_mask_word_count_str),
            (PTR, &pointer_mask_ref),
            (I32, &pointer_mask_word_count_str),
        ],
    );
}
