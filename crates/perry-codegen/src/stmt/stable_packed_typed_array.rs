//! Loop-local guarded views for erased ECS-style component columns.
//!
//! Type information is commonly lost at the system boundary: a pair of
//! component columns arrives as `any`, while the inner entity loop repeatedly
//! executes `a[entities[j]]` / `b[entities[j]]`. Per-access dynamic TypedArray
//! dispatch is correct but disproportionately expensive. This module admits a
//! narrow fast clone when runtime evidence proves all of the facts the native
//! buffer-view lowering needs once for the complete loop:
//!
//! * the entity list has the stable packed `u32`-index proof;
//! * two to four erased locals are used only as indexed receivers in the body;
//! * every receiver is an owning `Uint32Array`; and
//! * the receiver addresses are pairwise distinct.
//!
//! Any miss enters the unchanged generic clone. The admitted HIR body is
//! deliberately call/observer-free, so no alias can expose `.buffer` and
//! convert an owning array to a side-table view while its cached data pointer
//! is live. TypedArray headers themselves are tenured and non-moving.

use std::collections::HashMap;

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::expr::FnCtx;
use crate::native_value::{
    AliasState, BufferElem, BufferIndexUnit, BufferViewPointerState, BufferViewSlot, LengthSource,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

const MIN_RECEIVERS: usize = 2;
const MAX_RECEIVERS: usize = 4;

#[derive(Default)]
struct LocalUses {
    total: usize,
    receiver: usize,
    accesses: usize,
}

#[derive(Clone)]
pub(super) struct Candidate {
    local_ids: Vec<u32>,
}

pub(super) struct Admission {
    pub guard: String,
    raw_receivers: Vec<(u32, String)>,
}

pub(super) struct InstalledViews {
    previous: Vec<(u32, Option<BufferViewSlot>)>,
    pub common_length: String,
}

fn exact_entity_read(expr: &Expr, array_id: u32, counter_id: u32) -> bool {
    matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    )
}

fn collect_expr_uses(expr: &Expr, uses: &mut HashMap<u32, LocalUses>) {
    if let Expr::LocalGet(id) = expr {
        uses.entry(*id).or_default().total += 1;
    }
    match expr {
        Expr::IndexGet { object, .. }
        | Expr::IndexSet { object, .. }
        | Expr::IndexUpdate { object, .. } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                let use_ = uses.entry(*id).or_default();
                use_.receiver += 1;
                use_.accesses += 1;
            }
        }
        Expr::PutValueSet {
            target, receiver, ..
        } => {
            if let (Expr::LocalGet(target_id), Expr::LocalGet(receiver_id)) =
                (target.as_ref(), receiver.as_ref())
            {
                if target_id == receiver_id {
                    let use_ = uses.entry(*target_id).or_default();
                    // The central HIR walker visits both operands.
                    use_.receiver += 2;
                    use_.accesses += 1;
                }
            }
        }
        Expr::Closure { .. } => return,
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| collect_expr_uses(child, uses));
}

fn collect_stmt_uses(stmt: &Stmt, uses: &mut HashMap<u32, LocalUses>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Return(Some(expr))
        | Stmt::Throw(expr) => collect_expr_uses(expr, uses),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expr_uses(condition, uses);
            then_branch
                .iter()
                .for_each(|stmt| collect_stmt_uses(stmt, uses));
            if let Some(branch) = else_branch {
                branch.iter().for_each(|stmt| collect_stmt_uses(stmt, uses));
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            collect_expr_uses(condition, uses);
            body.iter().for_each(|stmt| collect_stmt_uses(stmt, uses));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                collect_stmt_uses(init, uses);
            }
            if let Some(condition) = condition {
                collect_expr_uses(condition, uses);
            }
            if let Some(update) = update {
                collect_expr_uses(update, uses);
            }
            body.iter().for_each(|stmt| collect_stmt_uses(stmt, uses));
        }
        Stmt::Labeled { body, .. } => collect_stmt_uses(body, uses),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(|stmt| collect_stmt_uses(stmt, uses));
            if let Some(catch) = catch {
                catch
                    .body
                    .iter()
                    .for_each(|stmt| collect_stmt_uses(stmt, uses));
            }
            if let Some(finally) = finally {
                finally
                    .iter()
                    .for_each(|stmt| collect_stmt_uses(stmt, uses));
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_expr_uses(discriminant, uses);
            for case in cases {
                if let Some(test) = case.test.as_ref() {
                    collect_expr_uses(test, uses);
                }
                case.body
                    .iter()
                    .for_each(|stmt| collect_stmt_uses(stmt, uses));
            }
        }
        _ => {}
    }
}

/// This is intentionally a whitelist, not a generic "may call" classifier.
/// It proves that no path in the admitted clone can run user code capable of
/// exposing a selected receiver's backing buffer.
fn safe_expr(expr: &Expr, selected: &[u32], entity_array_id: u32, counter_id: u32) -> bool {
    match expr {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::LocalGet(_) => true,
        Expr::IndexGet { object, index } => {
            exact_entity_read(expr, entity_array_id, counter_id)
                || matches!(object.as_ref(), Expr::LocalGet(id) if selected.contains(id))
                    && exact_entity_read(index, entity_array_id, counter_id)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            matches!(object.as_ref(), Expr::LocalGet(id) if selected.contains(id))
                && exact_entity_read(index, entity_array_id, counter_id)
                && safe_expr(value, selected, entity_array_id, counter_id)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            matches!(
                (target.as_ref(), receiver.as_ref()),
                (Expr::LocalGet(target_id), Expr::LocalGet(receiver_id))
                    if target_id == receiver_id && selected.contains(target_id)
            ) && exact_entity_read(key, entity_array_id, counter_id)
                && safe_expr(value, selected, entity_array_id, counter_id)
        }
        _ => false,
    }
}

fn safe_body(body: &[Stmt], selected: &[u32], entity_array_id: u32, counter_id: u32) -> bool {
    body.iter().all(|stmt| match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr) => safe_expr(expr, selected, entity_array_id, counter_id),
        _ => false,
    })
}

pub(super) fn find_candidate(
    ctx: &FnCtx<'_>,
    body: &[Stmt],
    entity_array_id: u32,
    counter_id: u32,
    u32_index_elements: bool,
) -> Option<Candidate> {
    if !u32_index_elements || ctx.disable_buffer_fast_path {
        return None;
    }
    let mut uses = HashMap::new();
    body.iter()
        .for_each(|stmt| collect_stmt_uses(stmt, &mut uses));
    let mut local_ids: Vec<u32> = uses
        .into_iter()
        .filter_map(|(id, use_)| {
            if id == entity_array_id
                || id == counter_id
                || use_.accesses < 2
                || use_.receiver != use_.total
                || !ctx.locals.contains_key(&id)
                || ctx.boxed_vars.contains(&id)
                || ctx.closure_captures.contains_key(&id)
                || ctx.reassigned_locals.contains(&id)
                || ctx.buffer_view_slots.contains_key(&id)
                || !matches!(
                    crate::type_analysis::static_type_of(ctx, &Expr::LocalGet(id)),
                    None | Some(perry_hir::types::Type::Any)
                        | Some(perry_hir::types::Type::Unknown)
                )
            {
                return None;
            }
            Some(id)
        })
        .collect();
    local_ids.sort_unstable();
    if !(MIN_RECEIVERS..=MAX_RECEIVERS).contains(&local_ids.len())
        || !safe_body(body, &local_ids, entity_array_id, counter_id)
    {
        return None;
    }
    Some(Candidate { local_ids })
}

pub(super) fn emit_fused_admission(
    ctx: &mut FnCtx<'_>,
    candidate: &Candidate,
    source_receiver: &str,
    bound: &str,
    descriptor: &str,
) -> Result<(Admission, String)> {
    let mut columns = Vec::with_capacity(MAX_RECEIVERS);
    for id in &candidate.local_ids {
        columns.push(crate::expr::lower_expr(ctx, &Expr::LocalGet(*id))?);
    }
    while columns.len() < MAX_RECEIVERS {
        columns.push("0.0".to_string());
    }
    let live_raw = ctx.block().call(
        I64,
        "js_packed_ecs_u32_loop_guard",
        &[
            (DOUBLE, source_receiver),
            (DOUBLE, bound),
            (DOUBLE, &columns[0]),
            (DOUBLE, &columns[1]),
            (DOUBLE, &columns[2]),
            (DOUBLE, &columns[3]),
            (I32, &candidate.local_ids.len().to_string()),
            (PTR, descriptor),
        ],
    );
    let guard = ctx.block().icmp_ne(I64, &live_raw, "0");
    let raw_receivers = candidate
        .local_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let slot = ctx
                .block()
                .gep(I64, descriptor, &[(I64, &(7 + index).to_string())]);
            (*id, ctx.block().load(I64, &slot))
        })
        .collect();
    Ok((
        Admission {
            guard,
            raw_receivers,
        },
        live_raw,
    ))
}

fn reserve_alias_scope(ctx: &mut FnCtx<'_>, data_slot: &str) -> u32 {
    let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
    // Loop-local views are removed before the generic clone is lowered, but
    // module metadata is emitted from the final map length. Retain a synthetic
    // unreachable key so scope ids stay unique and every metadata reference is
    // declared. Real HIR LocalIds cannot collide because we probe all maps.
    let mut reservation = u32::MAX;
    while ctx.locals.contains_key(&reservation)
        || ctx.module_globals.contains_key(&reservation)
        || ctx.buffer_data_slots.contains_key(&reservation)
        || ctx.buffer_view_slots.contains_key(&reservation)
    {
        reservation = reservation.wrapping_sub(1);
    }
    ctx.buffer_data_slots
        .insert(reservation, (data_slot.to_string(), scope_idx));
    scope_idx
}

pub(super) fn install_views(ctx: &mut FnCtx<'_>, admission: &Admission) -> InstalledViews {
    let mut previous = Vec::with_capacity(admission.raw_receivers.len());
    let mut common_length: Option<String> = None;
    for (id, raw) in &admission.raw_receivers {
        let (data_slot, length_slot, length) = {
            let header = ctx.block().inttoptr(I64, raw);
            let length = ctx.block().load(I32, &header);
            let data = ctx.block().gep(I8, &header, &[(I32, "16")]);
            let data_slot = ctx.func.alloca_entry(PTR);
            let length_slot = ctx.func.alloca_entry(I32);
            ctx.block().store(PTR, &data, &data_slot);
            ctx.block().store(I32, &length, &length_slot);
            (data_slot, length_slot, length)
        };
        common_length = Some(if let Some(current) = common_length {
            let shorter = ctx.block().icmp_ult(I32, &length, &current);
            ctx.block().select(I1, &shorter, I32, &length, &current)
        } else {
            length.clone()
        });
        let scope_idx = reserve_alias_scope(ctx, &data_slot);
        let old = ctx.buffer_view_slots.insert(
            *id,
            BufferViewSlot {
                data_slot,
                length_slot: Some(length_slot),
                scope_idx: Some(scope_idx),
                elem: BufferElem::U32,
                element_width_bytes: 4,
                index_unit: BufferIndexUnit::Element,
                view_byte_offset: Some(0),
                length_offset_from_data: -16,
                alias: AliasState::NoAliasGuarded {
                    guard_id: "stable_packed_u32_columns".to_string(),
                },
                length_source: Some(LengthSource::Unknown),
                native_owned: None,
                pointer_state: BufferViewPointerState::Stable,
                storage_inline_proven: true,
            },
        );
        previous.push((*id, old));
    }
    InstalledViews {
        previous,
        common_length: common_length.expect("typed-array candidate has at least two receivers"),
    }
}

pub(super) fn restore_views(ctx: &mut FnCtx<'_>, installed: InstalledViews) {
    for (id, old) in installed.previous {
        if let Some(old) = old {
            ctx.buffer_view_slots.insert(id, old);
        } else {
            ctx.buffer_view_slots.remove(&id);
        }
    }
}
