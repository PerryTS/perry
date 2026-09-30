//! What a `new ClassName(…)` site does about the instance's layout. Charter
//! step 5: the class ShapeId (minted with its birth rep) carries the lanes, so
//! no descriptor is installed per instance. [`layout_declared_at_allocation`]
//! still names the classes whose layout codegen proves at allocation (the
//! baked header image and the constructor-prologue store paths consult it);
//! for those, [`emit_fresh_instance_layout_forget`] clears a previous
//! tenant's per-object record at the new address.
//!
//! Split out of `new.rs` to stay under the repo's 2000-line-per-file cap
//! (`scripts/check_file_size.sh`).

use crate::expr::FnCtx;
use crate::types::I64;

/// #7510: may `class_name`'s layout be declared at allocation instead of
/// validated after the constructor?
///
/// Resolves the class and hands both halves of the proof to
/// [`crate::typed_shape::class_layout_declarable_at_allocation`], which
/// documents what they are and why they are enough.
pub(super) fn layout_declared_at_allocation(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    // A consumer module's imported Class stub carries field names/types but
    // not the defining constructor body. Treating `constructor: None` as a
    // proof that those slots may be declared before construction lets the
    // consumer mint a typed ShapeId while the producer mints the ordinary
    // structural ShapeId. Besides overstating the constructor proof, that
    // splits one runtime class across two exact identities, so a direct method
    // guard compiled in the producer can never accept an instance allocated
    // by the consumer. Imported classes stay on the validate-after-ctor path
    // until producer-authored layout proof is part of cross-module metadata.
    if ctx.imported_class_ctors.contains_key(class_name)
        && ctx
            .classes
            .get(class_name)
            .is_some_and(|class| class.constructor.is_none())
    {
        return false;
    }
    layout_declared_at_allocation_in(ctx.classes, ctx.class_keys_globals, class_name)
}

/// [`layout_declared_at_allocation`] over the module-level maps a `FnCtx`
/// carries by reference (#8122: the module-level header-image table needs the
/// same answer before any function is lowered — one implementation, two
/// callers).
pub(crate) fn layout_declared_at_allocation_in(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_keys_globals: &std::collections::HashMap<String, String>,
    class_name: &str,
) -> bool {
    if !class_keys_globals.contains_key(class_name) {
        return false;
    }
    let single = classes.get(class_name).is_some_and(|class| {
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
    super::field_init::chain_prologue_assigned_fields(classes, class_name).is_some_and(|chain| {
        crate::typed_shape::class_chain_layout_declarable_at_allocation(classes, &chain)
    })
}

/// Is `class_name`'s at-allocation declaration expressible in the packed
/// `GcHeader` store?
///
/// Three conditions, and each maps to one branch of
/// `gc::layout::init_typed_shape_layout` that would otherwise decide it at
/// runtime, per instance:
///
/// 1. [`layout_declared_at_allocation`] — the declare form is what would have
///    been emitted at all, so the fresh-slot proof is already discharged.
/// 2. `field_count == slot_count` — the runtime's one *downgrading* branch
///    (`layout_set_typed_unknown`), which a constant cannot express.
/// 3. A pointer-bearing mask is paired with a dedicated typed ShapeId at
///    module init. The runtime registers that ShapeId's exact descriptor once,
///    before the header image is published, so `SIDE_MASK | INTACT` is just as
///    self-contained at allocation as #7834's descriptor-free pointer-free
///    state.
///
/// What is deliberately NOT folded in is `layout_forget_object`: it depends on
/// the recycled ADDRESS, not on the shape. The caller emits it separately,
/// behind the `PERRY_PER_OBJECT_LAYOUTS_ANY` gate.
pub(super) fn layout_at_allocation(
    ctx: &FnCtx<'_>,
    class_name: &str,
    field_count: u32,
) -> crate::target_layout::InlineTypedLayout {
    layout_at_allocation_in(
        ctx.classes,
        ctx.class_keys_globals,
        ctx.class_init_chains,
        class_name,
        field_count,
    )
}

/// [`layout_at_allocation`] over the module-level maps (#8122; see
/// [`layout_declared_at_allocation_in`]).
pub(crate) fn layout_at_allocation_in(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_keys_globals: &std::collections::HashMap<String, String>,
    class_init_chains: &std::collections::HashMap<
        String,
        Vec<(String, Vec<perry_hir::ClassField>)>,
    >,
    class_name: &str,
    field_count: u32,
) -> crate::target_layout::InlineTypedLayout {
    use crate::target_layout::InlineTypedLayout;

    if !layout_declared_at_allocation_in(classes, class_keys_globals, class_name) {
        return InlineTypedLayout::None;
    }
    let Some(typed_layout) =
        resolve_typed_layout_in(classes, class_keys_globals, class_init_chains, class_name)
    else {
        return InlineTypedLayout::None;
    };
    if typed_layout.slot_count != field_count {
        return InlineTypedLayout::None;
    }
    if typed_layout.pointer_mask_words.is_empty() {
        InlineTypedLayout::PointerFree
    } else {
        InlineTypedLayout::SideMask
    }
}

/// Clear any per-object layout record a previous tenant of this address left
/// (`js_gc_forget_object_layout`, gated on the address sketch). Charter step
/// 5: the class ShapeId carries the lanes, so a fresh instance installs no
/// layout of its own. No-op unless [`layout_declared_at_allocation`] holds.
pub(super) fn emit_fresh_instance_layout_forget(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    obj_handle: &str,
) {
    if !layout_declared_at_allocation(ctx, class_name) {
        return;
    }
    emit_gated_forget_object_layout(ctx, obj_handle);
}

/// `if (PERRY_PER_OBJECT_LAYOUTS_ANY) js_gc_forget_object_layout(obj);`
///
/// The count is the runtime's authoritative atomic state (#7873), not a mirror:
/// a zero load proves no thread is armed, while a non-zero count conservatively
/// takes the call. A never-taken branch per allocation is the entire
/// steady-state cost.
fn emit_gated_forget_object_layout(ctx: &mut FnCtx<'_>, obj_handle: &str) {
    let young_idx = ctx.new_block("layout_forget.young");
    let sketch_idx = ctx.new_block("layout_forget.sketch");
    let call_idx = ctx.new_block("layout_forget.armed");
    let done_idx = ctx.new_block("layout_forget.done");
    let young_label = ctx.block_label(young_idx);
    let sketch_label = ctx.block_label(sketch_idx);
    let call_label = ctx.block_label(call_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let any = blk.load_atomic_monotonic(crate::types::I32, "@PERRY_PER_OBJECT_LAYOUTS_ANY", 4);
        let armed = blk.icmp_ne(crate::types::I32, &any, "0");
        blk.cond_br(&armed, &young_label, &done_label);
    }
    // Armed, but is any record keyed by an address THIS allocator could have
    // just recycled? `layout_tables::PERRY_YOUNG_LAYOUT_RECORDS` counts the
    // nursery-keyed records and is exact after every collection's death
    // prune; a long-lived masked object on an old page keeps the flag armed
    // without keeping this non-zero.
    ctx.current_block = young_idx;
    {
        let blk = ctx.block();
        let young = blk.load_atomic_monotonic(crate::types::I32, "@PERRY_YOUNG_LAYOUT_RECORDS", 4);
        let any_young = blk.icmp_ne(crate::types::I32, &young, "0");
        blk.cond_br(&any_young, &sketch_label, &done_label);
    }
    // Armed: some thread holds a per-object record. Test the process-global
    // address sketch (`layout_tables::layout_addr_filter_slot`: Fibonacci
    // hash, top 12 bits index 4,096 bits) before paying the runtime call —
    // one long-lived masked object (a harness closure, a registered listener)
    // otherwise taxes every later allocation with a thread-local probe.
    ctx.current_block = sketch_idx;
    {
        let blk = ctx.block();
        let hashed = blk.mul(I64, obj_handle, "-7046029254386353131");
        let index = blk.lshr(I64, &hashed, "52");
        let word = blk.lshr(I64, &index, "6");
        let bit_index = blk.and(I64, &index, "63");
        let bit = blk.shl(I64, "1", &bit_index);
        let word_ptr = blk.gep(I64, "@PERRY_LAYOUT_ADDR_FILTER", &[(I64, &word)]);
        let word_bits = blk.load_atomic_monotonic(I64, &word_ptr, 8);
        let masked = blk.and(I64, &word_bits, &bit);
        let may_hold = blk.icmp_ne(I64, &masked, "0");
        blk.cond_br(&may_hold, &call_label, &done_label);
    }
    ctx.current_block = call_idx;
    ctx.block()
        .call_void("js_gc_forget_object_layout", &[(I64, obj_handle)]);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

fn resolve_typed_layout_in(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_keys_globals: &std::collections::HashMap<String, String>,
    class_init_chains: &std::collections::HashMap<
        String,
        Vec<(String, Vec<perry_hir::ClassField>)>,
    >,
    class_name: &str,
) -> Option<crate::typed_shape::TypedShapeLayout> {
    class_keys_globals.get(class_name)?;
    Some(
        class_init_chains
            .get(class_name)
            .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
            .unwrap_or_else(|| crate::typed_shape::class_typed_layout(classes, class_name)),
    )
}
