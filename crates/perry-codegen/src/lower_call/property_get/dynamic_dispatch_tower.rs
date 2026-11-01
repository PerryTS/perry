//! Tower-of-`$pshape` routing for dynamic instance-method dispatch.
//!
//! Child module of `dynamic_dispatch.rs`, split out to stay under the
//! 2,000-line file gate; `use super::*` keeps the parent's private helpers
//! reachable.

use super::*;

/// May this dispatch-tower case take the proven-`this` clone?
///
/// Three conditions, and only the FIRST two are about correctness of the
/// *target*; the layout proof itself is emitted, not decided here.
///
/// 1. `owner` is `Some` — the receiver class of this case declares `property`
///    itself, so the clone's `this` is exactly the class it was compiled for.
/// 2. a clone was actually emitted for that pair (`pshape_methods`, already
///    pruned of symbol collisions by `prune_colliding_clones`, #6927).
/// 3. routing pays (`pshape_tower_routable`) — this site, unlike the two
///    guard-dominated ones, emits its own shape re-check and must earn it.
pub(super) fn tower_pshape_route(
    ctx: &FnCtx<'_>,
    owner: Option<&str>,
    property: &str,
    fname: &str,
    args: &[Expr],
) -> Option<TowerPshapeRoute> {
    let owner = owner?;
    // Carried forward from both existing routing sites (the #1787
    // static-receiver bug): a `perry_static_*` target needs
    // `js_class_static_method_call`, not a plain `call double`, and no
    // proven-`this` clone is ever emitted for one.
    if fname.starts_with("perry_static_") {
        return None;
    }
    let key = (owner.to_string(), property.to_string());
    if !ctx.pshape_methods.contains_key(&key) || !ctx.pshape_tower_routable.contains(&key) {
        return None;
    }
    let keys_global = ctx.class_keys_globals.get(owner)?.clone();
    let index_params = ctx.nonnegative_index_methods.get(&key);
    let index_proven = index_params.is_some_and(|params| {
        let Some(method) = ctx
            .classes
            .get(owner)
            .and_then(|class| class.methods.iter().find(|method| method.name == property))
        else {
            return false;
        };
        args.len() == method.params.len()
            && params.iter().all(|id| {
                method
                    .params
                    .iter()
                    .position(|param| param.id == *id)
                    .and_then(|position| args.get(position))
                    .is_some_and(|arg| {
                        crate::expr::numeric_index_has_integer_array_index_proof(ctx, arg)
                    })
            })
    });
    let pshape_fn = crate::collectors::pshape_method_name(fname);
    let (clone_fn, generic_fn) = if index_proven {
        let params = index_params.expect("proved indexed tower method remains registered");
        (
            crate::codegen::nonnegative_index_method_name(&pshape_fn, params),
            crate::codegen::nonnegative_index_method_name(fname, params),
        )
    } else {
        (pshape_fn, fname.to_string())
    };
    Some(TowerPshapeRoute {
        clone_fn,
        generic_fn,
        keys_global,
    })
}

/// Do the facts prove that every instance of class `cid` keeps the layout it
/// was born with?
///
/// `fixed_layout` (F3) is derived for a class that is prefix-stable (no
/// escape, no hard mark: no `delete`, `defineProperty`, `setPrototypeOf`,
/// freeze or seal reaches an instance, in any module the facts cover) and
/// that no store ever gives a key beyond its birth keys. Then the dynamic
/// recheck of [`emit_tower_pshape_call`] has nothing left to catch. It is
/// kept in a contract build (which checks the facts at run time), with
/// interpreted code present (`interp_guard`), and for every class the facts
/// do not list as fixed.
pub(super) fn tower_layout_proven(ctx: &FnCtx<'_>, cid: u32) -> bool {
    let proved = !ctx.wpfacts_off
        && !ctx.wpfacts.interp_guard
        && !crate::codegen::helpers::check_contracts()
        && ctx
            .wpfacts
            .classes
            .get(&cid)
            .is_some_and(|class| class.fixed_layout);
    if proved {
        crate::facts_consumed::used();
    }
    proved
}

/// Emit the keys-guarded diamond for one dispatch-tower case: inline shape
/// re-check → `{public}$pshape` on a match, the unchanged public body
/// otherwise. With `layout_proven` (the facts prove the arm's class keeps its
/// birth layout, [`tower_layout_proven`]) the case is the `$pshape` call alone.
///
/// The tower's case block proves `class_id`, which is NOT enough for the
/// clone's bare fixed-slot accesses — `delete inst.f` compacts the packed slots
/// while preserving `class_id`. The keys token is what closes that gap, and it
/// has to be checked *dynamically*: the `delete` shape barrier that stands the
/// whole analysis down is module-scoped while a receiver can be deleted from
/// through an alias in another module (#7143), so a static proof would be
/// exactly the wrong instrument here.
///
/// GC ordering invariant: `recv_handle` is the raw pointer masked out of the
/// receiver in `idispatch.tower`, and the header loads below dereference it.
/// That is only safe because **nothing between the mask and this point is a
/// safepoint**: the only allocating thing a case block can emit before the call
/// is the rest-array bundling (`js_array_alloc` / `js_array_push_f64`), and a
/// rest-bearing method can never reach here — `collectors/proven_this.rs`
/// rejects any method with a rest or synthesized-`arguments` parameter, so no
/// clone exists for one and `tower_pshape_route` returns `None`. The non-rest
/// preamble emits no instructions at all (already-lowered SSA values plus
/// `undefined` literals). If a future change makes the case preamble allocate,
/// this dereference must move above it — the `GC_FLAG_FORWARDED` conjunct would
/// degrade a moved receiver to the generic path rather than misread it, but
/// relying on that instead of on the ordering would be luck, not a proof.
pub(super) fn emit_tower_pshape_call(
    ctx: &mut FnCtx<'_>,
    case_no: usize,
    route: &TowerPshapeRoute,
    recv_handle: &str,
    case_arg_slices: &[(crate::types::LlvmType, &str)],
    layout_proven: bool,
) -> String {
    // A facts-verification build keeps the recheck: its miss is the witness.
    let verify = layout_proven && crate::verify_facts::enabled();
    if layout_proven && !verify {
        return ctx.block().call(DOUBLE, &route.clone_fn, case_arg_slices);
    }
    // The global is read ONCE per function (entry-hoisted); the case block only
    // reloads it from the stack slot, which mem2reg folds away.
    let expected_shape_id = match crate::typed_shape::static_class_shape_id(&route.keys_global) {
        Some(id) => id.to_string(),
        None => {
            let shape_global =
                crate::typed_shape::shape_id_global_name_from_keys_global(&route.keys_global);
            let shape_slot = ctx.func.entry_init_load_global(&shape_global, I32);
            ctx.block().load(I32, &shape_slot)
        }
    };

    let proven_idx = ctx.new_block(&format!("idispatch.case{}.pshape", case_no));
    let generic_idx = ctx.new_block(&format!("idispatch.case{}.generic", case_no));
    let join_idx = ctx.new_block(&format!("idispatch.case{}.join", case_no));
    let proven_label = ctx.block_label(proven_idx);
    let generic_label = ctx.block_label(generic_idx);
    let join_label = ctx.block_label(join_idx);

    crate::expr::class_field_inline_guard::emit_proven_shape_recheck(
        ctx,
        recv_handle,
        &expected_shape_id,
        &proven_label,
        &generic_label,
    );

    ctx.current_block = proven_idx;
    let v_proven = ctx.block().call(DOUBLE, &route.clone_fn, case_arg_slices);
    let proven_end = ctx.block().label.clone();
    ctx.block().br(&join_label);

    ctx.current_block = generic_idx;
    if verify {
        let site = crate::verify_facts::site_arg(
            ctx,
            &format!(
                "method {} layout proven: shape {expected_shape_id} ({})",
                route.clone_fn, route.keys_global
            ),
        );
        let recv_box = crate::expr::nanbox_pointer_inline(ctx.block(), recv_handle);
        let _ = ctx.block().call(
            crate::types::I32,
            "perry_wpfacts_check_miss",
            &[(DOUBLE, &recv_box), (crate::types::I64, &site)],
        );
    }
    let v_generic = ctx.block().call(DOUBLE, &route.generic_fn, case_arg_slices);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&join_label);

    ctx.current_block = join_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (v_proven.as_str(), proven_end.as_str()),
            (v_generic.as_str(), generic_end.as_str()),
        ],
    )
}
