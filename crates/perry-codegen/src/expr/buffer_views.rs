use perry_hir::Expr;

use crate::native_value::{
    AliasState, BoundsState, BufferElem, BufferIndexUnit, BufferViewSlot, LengthSource,
    LoweredValue, MaterializationReason, NativeOwnedViewFact,
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
    if let Some(view) = ctx.buffer_view_slots.get_mut(&id) {
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
    for (view_id, view) in ctx.buffer_view_slots.iter_mut() {
        let Some(native) = view.native_owned.as_ref() else {
            continue;
        };
        if native.owner_local_id != owner_id {
            continue;
        }
        invalidate_native_owned_view(view, &reason);
        invalidated.push(*view_id);
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
    for (view_id, view) in ctx.buffer_view_slots.iter_mut() {
        if view.native_owned.is_none() {
            continue;
        }
        invalidate_native_owned_view(view, &reason);
        invalidated.push(*view_id);
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

pub(crate) fn alias_buffer_view_slot(
    ctx: &mut FnCtx<'_>,
    alias_id: u32,
    source_id: u32,
    reason: MaterializationReason,
) {
    let Some(mut view) = ctx.buffer_view_slots.get(&source_id).cloned() else {
        return;
    };
    let reason = if view.native_owned.is_some() {
        MaterializationReason::MutableAlias
    } else {
        reason
    };
    downgrade_buffer_alias(ctx, source_id, reason.clone());
    view.alias = AliasState::MayAlias;
    view.scope_idx = None;
    ctx.buffer_view_slots.insert(alias_id, view);
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

pub(crate) fn attach_native_owned_view_fact(ctx: &mut FnCtx<'_>, view: &BufferViewSlot) {
    let Some(fact) = native_owned_fact_for_view(view) else {
        return;
    };
    if let Some(record) = ctx.native_rep_records.last_mut() {
        record.native_owned_view = Some(fact);
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
        let data_slot = ctx
            .buffer_view_slots
            .get(&id)
            .map(|view| view.data_slot.clone())
            .unwrap_or_else(|| ctx.func.alloca_entry(PTR));
        ctx.block().store(PTR, &data_ptr, &data_slot);
        ctx.buffer_view_slots.insert(
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
            },
        );
    } else {
        ctx.buffer_view_slots.remove(&id);
    }
    ctx.buffer_hazard_reasons
        .insert(id, MaterializationReason::Reassignment);
}

pub(crate) fn downgrade_buffer_aliases_in_expr(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    reason: MaterializationReason,
) {
    match expr {
        Expr::LocalGet(id) => downgrade_buffer_alias(ctx, *id, reason),
        Expr::Binary { left, right, .. } => {
            downgrade_buffer_aliases_in_expr(ctx, left, reason.clone());
            downgrade_buffer_aliases_in_expr(ctx, right, reason);
        }
        Expr::PropertyGet { object, .. } => downgrade_buffer_aliases_in_expr(ctx, object, reason),
        Expr::IndexGet { object, index } => {
            downgrade_buffer_aliases_in_expr(ctx, object, reason.clone());
            downgrade_buffer_aliases_in_expr(ctx, index, reason);
        }
        _ => {}
    }
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
