//! `new` lowering's instance-root and argument-rooting helpers: the
//! `construction_runs_user_code` predicate, the `Instance` record and its
//! `reload_instance` re-read, and constructor-argument adopt/refresh.
//!
//! Split out of `new.rs` to keep that file under the 2,000-line cap (#10750);
//! pure relocation.

use anyhow::Result;
use perry_hir::Expr;

use super::super::new_ctor_args::lower_constructor_arg;
use crate::expr::{nanbox_pointer_inline, FnCtx};
use crate::rooting::{self, EmittedValue, RootedGroup};

/// Does `new <class_name>(…)` run user code — an own or inherited constructor
/// body, or field initializers — between the instance allocation and the value
/// the `new` expression yields?
///
/// That is the window #7154 is about: user code allocates, a back-edge poll
/// inside it drives an evacuating minor, and the instance moves while the
/// caller holds it only in an SSA register. A class with none of these has no
/// window at all (`js_gc_init_typed_shape_layout` is the only thing emitted in
/// between, and it does not allocate), so it keeps its pre-#7154 IR exactly.
///
/// An unresolvable class name is `false` on purpose, not conservatively `true`:
/// the instance root is pushed only on paths that resolved the class out of
/// `ctx.classes`, so a name this returns `false` for never reaches the push and
/// would leave the scope marker as pure overhead.
pub(super) fn construction_runs_user_code(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    // #7207: an IMPORTED constructor runs user code while leaving no trace in
    // the local class table — `ctx.classes[class_name].constructor` is `None`
    // for it. A class that also declares no fields and no heritage therefore
    // answered `false` here while `lower_new_impl_inner` went on to dispatch
    // `ctx.imported_class_ctors[class_name]` (its `has_imported_ctor` arm, and
    // the `Stmt::Return` writer at the tail of this file). That left BOTH
    // consumers of this predicate unprotected across a real constructor body:
    // #7192's instance root, and the `this`-slot bind added for #7202.
    //
    // Keeping it ONE predicate rather than two is the point — the consumers
    // have to agree by construction, which is what stops the divergence
    // #7114's pair of predicates produced.
    if ctx.imported_class_ctors.contains_key(class_name) {
        return true;
    }
    ctx.classes.get(class_name).is_some_and(|class| {
        class.constructor.is_some()
            || !class.fields.is_empty()
            // #8809: a class whose only private elements are METHODS or
            // ACCESSORS declares no fields, no constructor and no heritage, and
            // answered `false` here — while `emit_field_inits` still emits
            // `js_private_brand_add` for it (#8643 added that call, keyed on
            // `has_private_instance_elements`, and its `continue` guard lets a
            // fieldless class through precisely so the brand can be installed).
            // That helper allocates the marker key and calls
            // `js_object_set_field_by_name`; its own body says "the marker-key
            // allocation can evacuate both the receiver and any live value" and
            // opens a `RuntimeHandleScope` for exactly that reason. So the
            // window this predicate claims cannot collect does, and the
            // instance was crossing it in a bare register: `new
            // WithPrivateMethod()` fed a stale handle to
            // `js_gc_init_typed_shape_layout` and then published it into the
            // caller's root slot.
            //
            // One predicate, one place — the temp root, the `this`-slot bind
            // and `reload_instance` all read this, which is what stops them
            // disagreeing the way #7114's pair did.
            || class.has_private_instance_elements()
            || class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some()
    })
}

/// Re-read the freshly-constructed instance from the temp-root slot that
/// carried it across the constructor body (#7154).
///
/// Returns `(obj_handle, obj_box)` — the bare handle and its NaN-boxed form.
/// When no root was pushed (nothing between the allocation and here can
/// collect) the original registers are handed straight back, so those sites
/// keep their old IR byte for byte.
pub(super) fn reload_instance(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    instance: &Instance,
    obj_handle: &str,
    obj_box: &str,
) -> (String, String) {
    if !instance.protected {
        return (obj_handle.to_string(), obj_box.to_string());
    }
    let handle = group.reread_emitted(ctx, instance.root);
    let boxed = nanbox_pointer_inline(ctx.block(), &handle);
    (handle, boxed)
}

/// The freshly-allocated instance's place in the `new` scope.
///
/// `protected` is `construction_runs_user_code`, taken ONCE. It gates three
/// things that must agree — the temp root, the `this`-slot bind (#7202), and
/// whether [`reload_instance`] re-reads at all — and the reason it is a field
/// rather than three calls is the same reason `construction_runs_user_code` is
/// one predicate rather than two: a fork here is how #7114's pair diverged.
pub(super) struct Instance {
    pub(super) root: EmittedValue,
    pub(super) protected: bool,
}

/// Refresh `lowered_args` after something that may have collected (#6969).
///
/// Two cases, and both are mandatory rather than defensive:
///
/// - a **rooted** argument is re-read from its slot, because the slot is a
///   *mutable* root that an evacuating cycle rewrites in place, leaving the
///   register pushed beforehand stale;
/// - an argument that was NOT rooted because it reads an *immutable* registered
///   root — a string literal, the only `operand_is_reloadable` case — is
///   **re-loaded**. It is never swept, but evacuation rewrote its handle
///   global too, so the cached register points at where the string used to be.
///   Re-lowering emits the load again and costs no runtime call. (A
///   shadow-slotted local or a module global is a registered root as well, but
///   a *mutable* one, so it takes a temp-root slot instead: re-deriving it
///   would observe an assignment made after the call-time value was taken.)
///
/// Called after the instance allocation and again before the late consumers
/// that sit behind further arbitrary lowering (field initializers, an inlined
/// constructor body) — each of those is another chance to relocate.
pub(super) fn refresh_rooted_args(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
) -> Result<Vec<String>> {
    // `RootedGroup`'s re-read re-lowers a `Reload` operand through
    // `crate::expr::lower_expr`, while the ORIGINAL lowering of every
    // constructor argument went through `lower_constructor_arg` — which is
    // `lower_expr` with `discard_expr_value` forced false (#7590: the flag
    // means "this STATEMENT's value is discarded" and is not cleared on
    // recursion). Re-lowering under a different flag would be free to pick
    // `materialize_js_value_without_record`, so the re-read is wrapped in the
    // same suppression the first lowering had. A no-op for `Root` and `Reuse`
    // operands, which emit no lowering at all.
    let prev_discard = ctx.discard_expr_value;
    ctx.discard_expr_value = false;
    let out = group.reread_all(ctx);
    ctx.discard_expr_value = prev_discard;
    out
}

/// Lower every constructor argument into `group`, rooting each one **as it is
/// produced** rather than after the list (#6969: rooting a finished list
/// publishes an already-dangling argument 0 to the scanner, which turns a
/// silent wrong answer into a SIGSEGV — strictly worse than not rooting).
///
/// Returns the group indices, in argument order, for the caller to re-read at
/// the point it emits its call. `lower_constructor_arg` rather than
/// `RootedGroup::lower` because it clears `ctx.discard_expr_value` for the
/// operand — #7590: that flag means "this STATEMENT's value is discarded" and
/// is not cleared on recursion, so lowering an operand under it can evaluate a
/// typed-array store to `0`.
pub(super) fn adopt_constructor_args<'a>(
    ctx: &mut FnCtx<'_>,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<Vec<usize>> {
    let mut slots = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let value = lower_constructor_arg(ctx, a)?;
        let collects = rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
        slots.push(group.adopt(ctx, a, &value, collects));
    }
    Ok(slots)
}
