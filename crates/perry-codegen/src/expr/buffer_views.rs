use perry_hir::{walker::walk_expr_children, Expr};

use crate::native_value::{
    AliasState, BoundsState, BufferElem, BufferIndexUnit, BufferViewPointerState, BufferViewSlot,
    LengthSource, LoweredValue, MaterializationReason, NativeOwnedViewFact,
};
use crate::types::{I32, I64, I8, PTR};

use super::{unbox_to_i64, FnCtx};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeArenaOwnerAliasResolution {
    Known(u32),
    Ambiguous,
    None,
}

pub(crate) fn native_arena_owner_alias_resolution(
    ctx: &FnCtx<'_>,
    owner_id: u32,
) -> NativeArenaOwnerAliasResolution {
    if let Some(owner_id) = ctx.native_arena_owner_aliases.get(&owner_id).copied() {
        NativeArenaOwnerAliasResolution::Known(owner_id)
    } else if ctx.native_arena_ambiguous_owner_aliases.contains(&owner_id) {
        NativeArenaOwnerAliasResolution::Ambiguous
    } else {
        NativeArenaOwnerAliasResolution::None
    }
}

pub(crate) fn native_arena_canonical_owner_id(ctx: &FnCtx<'_>, owner_id: u32) -> u32 {
    match native_arena_owner_alias_resolution(ctx, owner_id) {
        NativeArenaOwnerAliasResolution::Known(owner_id) => owner_id,
        NativeArenaOwnerAliasResolution::Ambiguous | NativeArenaOwnerAliasResolution::None => {
            owner_id
        }
    }
}

pub(crate) fn record_native_arena_owner_assignment(ctx: &mut FnCtx<'_>, id: u32, value: &Expr) {
    match value {
        Expr::NativeArenaAlloc(_) => {
            ctx.native_arena_owner_aliases.insert(id, id);
            ctx.native_arena_ambiguous_owner_aliases.remove(&id);
        }
        Expr::LocalGet(source_id) => match native_arena_owner_alias_resolution(ctx, *source_id) {
            NativeArenaOwnerAliasResolution::Known(owner_id) => {
                ctx.native_arena_owner_aliases.insert(id, owner_id);
                ctx.native_arena_ambiguous_owner_aliases.remove(&id);
            }
            NativeArenaOwnerAliasResolution::Ambiguous => {
                ctx.native_arena_owner_aliases.remove(&id);
                ctx.native_arena_ambiguous_owner_aliases.insert(id);
            }
            NativeArenaOwnerAliasResolution::None => {
                ctx.native_arena_owner_aliases.remove(&id);
                ctx.native_arena_ambiguous_owner_aliases.remove(&id);
            }
        },
        _ => {
            ctx.native_arena_owner_aliases.remove(&id);
            ctx.native_arena_ambiguous_owner_aliases.remove(&id);
        }
    }
}

pub(crate) fn buffer_view_lowered_value(
    data_ptr: &str,
    length: &str,
    elem: BufferElem,
    element_width_bytes: u32,
    index_unit: BufferIndexUnit,
    view_byte_offset: Option<i64>,
    length_offset_from_data: i32,
    bounds: BoundsState,
    alias: AliasState,
) -> LoweredValue {
    LoweredValue::buffer_view(
        data_ptr,
        length,
        elem,
        element_width_bytes,
        index_unit,
        view_byte_offset,
        length_offset_from_data,
        bounds,
        alias,
    )
}

pub(crate) fn downgrade_buffer_alias(ctx: &mut FnCtx<'_>, id: u32, reason: MaterializationReason) {
    let mut effective_reason = reason.clone();
    // A same-storage alias shares its source's `data_slot`
    // (`alias_buffer_view_slot`): both names denote one storage, so a hazard
    // seen through either name demotes both.
    let shared_slot = ctx
        .receiver_descriptors
        .buffer_view(id)
        .map(|view| view.data_slot.clone());
    if let Some(slot) = shared_slot {
        let mut sharers = Vec::new();
        for (other, view) in ctx.receiver_descriptors.buffer_views_mut() {
            if other != id && view.data_slot == slot && view.native_owned.is_none() {
                view.alias = AliasState::MayAlias;
                view.scope_idx = None;
                sharers.push(other);
            }
        }
        for other in sharers {
            ctx.buffer_hazard_reasons.insert(other, reason.clone());
        }
    }
    if let Some(view) = ctx.receiver_descriptors.buffer_view_mut(id) {
        if view.native_owned.is_some() && matches!(reason, MaterializationReason::UnknownCallEscape)
        {
            effective_reason = MaterializationReason::EscapingUnownedPointer;
        }
        view.alias = AliasState::MayAlias;
        view.scope_idx = None;
        if matches!(effective_reason, MaterializationReason::MissingOwnerRoot) {
            if let Some(native) = view.native_owned.as_mut() {
                native.owner_rooted = false;
            }
        }
    }
    ctx.buffer_hazard_reasons.insert(id, effective_reason);
    invalidate_native_owned_views_for_owner_alias(
        ctx,
        id,
        owner_alias_invalidation_reason(&reason),
    );
}

/// Mark a cached data pointer as unusable after an operation changes which
/// storage the receiver aliases. Alias state alone cannot express this: the
/// pointer is stale, not merely shared.
pub(crate) fn invalidate_buffer_view_pointer(
    ctx: &mut FnCtx<'_>,
    id: u32,
    reason: MaterializationReason,
) {
    let affected_ids = if let Some(data_slot) = ctx
        .receiver_descriptors
        .buffer_view(id)
        .map(|view| view.data_slot.clone())
    {
        ctx.receiver_descriptors
            .buffer_views()
            .filter_map(|(view_id, view)| (view.data_slot == data_slot).then_some(view_id))
            .collect::<Vec<_>>()
    } else {
        vec![id]
    };
    for affected_id in affected_ids {
        if let Some(view) = ctx.receiver_descriptors.buffer_view_mut(affected_id) {
            view.pointer_state = BufferViewPointerState::Invalidated {
                reason: reason.clone(),
            };
        }
        downgrade_buffer_alias(ctx, affected_id, reason.clone());
    }
}

fn owner_alias_invalidation_reason(reason: &MaterializationReason) -> MaterializationReason {
    match reason {
        MaterializationReason::UnknownCallEscape => MaterializationReason::MissingOwnerRoot,
        _ => reason.clone(),
    }
}

fn invalidate_native_owned_views_for_owner_alias(
    ctx: &mut FnCtx<'_>,
    owner_id: u32,
    reason: MaterializationReason,
) {
    match native_arena_owner_alias_resolution(ctx, owner_id) {
        NativeArenaOwnerAliasResolution::Known(owner_id) => {
            invalidate_native_owned_views_for_owner(ctx, owner_id, reason)
        }
        NativeArenaOwnerAliasResolution::Ambiguous => {
            invalidate_all_native_owned_views(ctx, reason)
        }
        NativeArenaOwnerAliasResolution::None => {
            invalidate_native_owned_views_for_owner(ctx, owner_id, reason)
        }
    }
}

pub(crate) fn invalidate_native_owned_views_for_owner(
    ctx: &mut FnCtx<'_>,
    owner_id: u32,
    reason: MaterializationReason,
) {
    let mut invalidated = Vec::new();
    for (view_id, view) in ctx.receiver_descriptors.buffer_views_mut() {
        let Some(native) = view.native_owned.as_ref() else {
            continue;
        };
        if native.owner_local_id != owner_id {
            continue;
        }
        invalidate_native_owned_view(view, &reason);
        invalidated.push(view_id);
    }
    for view_id in invalidated {
        ctx.buffer_hazard_reasons.insert(view_id, reason.clone());
    }
}

pub(crate) fn invalidate_native_owned_views_for_dispose(ctx: &mut FnCtx<'_>, owner: &Expr) {
    match owner {
        Expr::LocalGet(owner_id) => invalidate_native_owned_views_for_owner_alias(
            ctx,
            *owner_id,
            MaterializationReason::UseAfterDispose,
        ),
        _ => invalidate_all_native_owned_views(ctx, MaterializationReason::UseAfterDispose),
    }
}

fn invalidate_all_native_owned_views(ctx: &mut FnCtx<'_>, reason: MaterializationReason) {
    let mut invalidated = Vec::new();
    for (view_id, view) in ctx.receiver_descriptors.buffer_views_mut() {
        if view.native_owned.is_none() {
            continue;
        }
        invalidate_native_owned_view(view, &reason);
        invalidated.push(view_id);
    }
    for view_id in invalidated {
        ctx.buffer_hazard_reasons.insert(view_id, reason.clone());
    }
}

fn invalidate_native_owned_view(view: &mut BufferViewSlot, reason: &MaterializationReason) {
    let Some(native) = view.native_owned.as_mut() else {
        return;
    };
    view.alias = AliasState::MayAlias;
    view.scope_idx = None;
    match reason {
        MaterializationReason::UseAfterDispose => {
            native.disposed = true;
        }
        MaterializationReason::MissingOwnerRoot
        | MaterializationReason::Reassignment
        | MaterializationReason::UnknownCallEscape
        | MaterializationReason::ClosureCapture => {
            native.owner_rooted = false;
        }
        _ => {}
    }
}

/// `true` when `view` names one fixed object's inline element storage for
/// the binding's whole life: a stable cached data pointer, inline storage, an
/// element-indexed typed-array view, and a live alias scope. These are the
/// receiver gates of the proven-view tiers (`proven_view_receiver`), minus the
/// per-binding closure gate, which each use site re-checks for its own id.
fn view_names_fixed_inline_storage(view: &BufferViewSlot) -> bool {
    view.pointer_state.is_stable()
        && view.storage_inline_proven
        && view.native_owned.is_none()
        && view.index_unit == BufferIndexUnit::Element
        && view.alias.allows_noalias()
        && view.scope_idx.is_some()
}

/// `let alias = source` where `source` carries a buffer view.
///
/// #11810: when the source view names one fixed object's inline storage, the
/// alias is a second NAME for that storage, not a second storage. It shares
/// the source's view unchanged: the same `data_slot`, the same alias scope (so
/// no access through one name is declared `noalias` against the other), and
/// the same proof. The shared slot is what ties the names together afterwards:
/// a hazard seen through either name (`.buffer` exposure, an escape, a
/// closure capture) demotes or invalidates every view on that slot, see
/// [`downgrade_buffer_alias`] and [`invalidate_buffer_view_pointer`]. Neither name loses its
/// proven tier. Before this, every alias demoted BOTH names to `MayAlias`,
/// which the per-site guarded reads also refuse (a tracked view "owns" its
/// receiver), so every later access fell to the `js_typed_array_get` helper.
/// HIR's compound-assignment spill (`__cmpd_base = a` for `a[k] -= v`) is
/// such an alias, so one `a[i + 3] -= d` made the rest of the function slower
/// than the same code over an array with no proof at all.
///
/// The shared slot is written again only by a reassignment refresh, and
/// [`update_buffer_view_for_assignment`] gives the reassigned name a fresh
/// slot whenever another view still reads the old one, so reassigning either
/// name cannot redirect the other.
///
/// Other sources (native-owned arena views, byte-indexed buffers, views
/// already demoted, non-inline storage) keep the conservative demotion below.
pub(crate) fn alias_buffer_view_slot(
    ctx: &mut FnCtx<'_>,
    alias_id: u32,
    source_id: u32,
    reason: MaterializationReason,
) {
    let Some(mut view) = ctx.receiver_descriptors.buffer_view(source_id).cloned() else {
        return;
    };
    if view_names_fixed_inline_storage(&view) {
        ctx.receiver_descriptors
            .materialize_buffer_view(alias_id, view);
        return;
    }
    let reason = if view.native_owned.is_some() {
        MaterializationReason::MutableAlias
    } else {
        reason
    };
    downgrade_buffer_alias(ctx, source_id, reason.clone());
    view.alias = AliasState::MayAlias;
    view.scope_idx = None;
    ctx.receiver_descriptors
        .materialize_buffer_view(alias_id, view);
    ctx.buffer_hazard_reasons.insert(alias_id, reason);
}

pub(crate) fn native_owned_fact_for_view(view: &BufferViewSlot) -> Option<NativeOwnedViewFact> {
    let alias_group = view
        .scope_idx
        .map(|scope_idx| format!("alias_scope_{}", scope_idx))
        .unwrap_or_else(|| "unknown".to_string());
    view.native_owned
        .as_ref()
        .map(|native| native.fact(view.element_width_bytes, alias_group))
}

pub(crate) fn attach_buffer_view_facts(ctx: &mut FnCtx<'_>, view: &BufferViewSlot) {
    if let Some(record) = ctx.native_rep_records.last_mut() {
        record.buffer_view_pointer_state = Some(view.pointer_state.clone());
        record.native_owned_view = native_owned_fact_for_view(view);
    }
}

pub(crate) fn attach_buffer_view_pointer_state_for_expr(ctx: &mut FnCtx<'_>, expr: &Expr) {
    let Expr::LocalGet(id) = expr else {
        return;
    };
    let Some(state) = ctx
        .receiver_descriptors
        .buffer_view(id)
        .map(|view| view.pointer_state.clone())
    else {
        return;
    };
    if let Some(record) = ctx.native_rep_records.last_mut() {
        record.local_id = Some(*id);
        record.buffer_view_pointer_state = Some(state);
    }
}

pub(crate) fn update_buffer_view_for_assignment(
    ctx: &mut FnCtx<'_>,
    id: u32,
    value: &Expr,
    lowered_value: &str,
) {
    let is_fresh_u8_buffer = matches!(
        value,
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) | Expr::Uint8ArrayNew(_)
    ) || matches!(
        value,
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if module == "buffer" && method == "copyBytesFrom"
    );
    if is_fresh_u8_buffer {
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, lowered_value);
        let handle_ptr = blk.inttoptr(I64, &handle);
        let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
        // Reuse the binding's own slot only when no OTHER view reads it: a
        // same-storage alias (`alias_buffer_view_slot`) shares the slot, and
        // overwriting it would point that alias at this new buffer.
        let own_slot = ctx
            .receiver_descriptors
            .buffer_view(id)
            .map(|view| view.data_slot.clone())
            .filter(|slot| {
                !ctx.receiver_descriptors
                    .buffer_views()
                    .any(|(other, view)| other != id && view.data_slot == *slot)
            });
        let data_slot = own_slot.unwrap_or_else(|| ctx.func.alloca_entry(PTR));
        ctx.block().store(PTR, &data_ptr, &data_slot);
        ctx.receiver_descriptors.materialize_buffer_view(
            id,
            BufferViewSlot {
                data_slot,
                length_slot: None,
                scope_idx: None,
                elem: BufferElem::U8,
                element_width_bytes: 1,
                index_unit: BufferIndexUnit::Byte,
                view_byte_offset: Some(0),
                length_offset_from_data: -8,
                alias: AliasState::MayAlias,
                length_source: Some(LengthSource::Unknown),
                native_owned: None,
                pointer_state: BufferViewPointerState::Stable,
                // Reassignment refresh: `Uint8ArrayNew` with a non-literal arg
                // can be the view form (`new Uint8Array(buffer)`), so the
                // inline-storage proof is not re-established here.
                storage_inline_proven: false,
            },
        );
    } else {
        ctx.receiver_descriptors.dematerialize_buffer_view(id);
    }
    ctx.buffer_hazard_reasons
        .insert(id, MaterializationReason::Reassignment);
}

pub(crate) fn downgrade_buffer_aliases_in_expr(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    reason: MaterializationReason,
) {
    if let Expr::LocalGet(id) = expr {
        downgrade_buffer_alias(ctx, *id, reason.clone());
    }
    walk_expr_children(expr, &mut |child| {
        downgrade_buffer_aliases_in_expr(ctx, child, reason.clone());
    });
}

pub(crate) fn buffer_access_materialization_reason(
    ctx: &FnCtx<'_>,
    expr: &Expr,
) -> MaterializationReason {
    if let Expr::LocalGet(id) = expr {
        if let Some(reason) = ctx.buffer_hazard_reasons.get(id) {
            return reason.clone();
        }
        if ctx.closure_captures.contains_key(id) {
            return MaterializationReason::ClosureCapture;
        }
    }
    MaterializationReason::UnknownBounds
}
