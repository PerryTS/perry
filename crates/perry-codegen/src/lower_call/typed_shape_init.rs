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
    ctx.classes.get(class_name).is_some_and(|class| {
        let prologue = super::field_init::ctor_prologue_param_assigned_fields(class);
        crate::typed_shape::class_layout_declarable_at_allocation(class, &prologue)
    })
}

/// Emit the `js_gc_declare_typed_shape_layout` call that registers a **freshly
/// allocated** instance's layout, before its constructor runs, so the
/// constructor's own field stores can pass the intact-bit guard (#7510/#7512).
///
/// No-op unless [`layout_declared_at_allocation`] holds. **Must be emitted
/// while the instance's slots are still the allocator's fill** — that is the
/// runtime contract, and it is not checkable from the runtime side.
pub(super) fn emit_typed_shape_layout_declare(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
) {
    if !layout_declared_at_allocation(ctx, class_name) {
        return;
    }
    emit_typed_shape_layout_call(
        ctx,
        class_name,
        obj_handle,
        "js_gc_declare_typed_shape_layout",
    );
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
    // Refs #5094: prefer the prefix-disambiguated chain so slot/word counts
    // agree with the mask globals emitted in compile_module (same-named
    // cross-module parents mis-resolve in the name-keyed walk).
    let typed_layout = ctx
        .class_init_chains
        .get(class_name)
        .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
        .unwrap_or_else(|| crate::typed_shape::class_typed_layout(ctx.classes, class_name));
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
