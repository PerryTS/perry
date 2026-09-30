//! What a `new ClassName(…)` site does about the instance's layout. Charter
//! step 5: the class ShapeId (minted with its birth rep) carries the lanes, so
//! no descriptor is installed per instance. [`layout_declared_at_allocation`]
//! still names classes whose constructor-prologue stores are provable.
//!
//! Split out of `new.rs` to stay under the repo's 2000-line-per-file cap
//! (`scripts/check_file_size.sh`).

use crate::expr::FnCtx;

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
