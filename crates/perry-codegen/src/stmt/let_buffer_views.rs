//! Buffer/typed-array view-slot registration for `Stmt::Let` bindings.
//! Mechanically extracted from `let_stmt.rs` (file-size gate); behavior
//! unchanged. `pub(super)` — consumed only by the `stmt` module.

use crate::expr::FnCtx;
use crate::native_value::{
    AliasState, BufferElem, BufferIndexUnit, BufferViewPointerState, BufferViewSlot, LengthSource,
    NativeOwnedViewSlot,
};
use crate::types::{I32, I64, PTR};

pub(super) struct BufferViewInit {
    elem: BufferElem,
    element_width_bytes: u32,
    index_unit: BufferIndexUnit,
    length_offset_from_data: i32,
    length_source: LengthSource,
    native_owner_local_id: Option<u32>,
    native_byte_offset: Option<i64>,
    native_byte_length: Option<i64>,
    /// Resolve element zero through the runtime's buffer-view registry instead
    /// of assuming bytes start inline at `header + data_offset_bytes`.
    /// See `BufferViewSlot::storage_inline_proven` — true only when the
    /// construction form proves fresh inline (non-view) storage.
    storage_inline_proven: bool,
}

pub(super) fn register_noalias_buffer_view(
    ctx: &mut FnCtx<'_>,
    id: u32,
    init_expr: &perry_hir::Expr,
    value: &str,
) {
    // Every caller registers only a `known_noalias_buffer_locals` member: an
    // immutable binding whose construction the HIR fact layer proved fresh
    // and owned (`collectors/hir_facts.rs::is_owned_u8_buffer_alloc`).
    let owned_by_fact = ctx.known_noalias_buffer_locals.contains(&id);
    // Construction alone is not a lasting fact. Observing `.buffer` rebinds a
    // typed array to an external backing (its elements leave `header + 16`)
    // and `buffer.transfer()` then detaches it, so the cached data pointer,
    // the construction length and the inline-storage proof hold only for a
    // SEALED binding, one no use can hand to code that reads its `.buffer`
    // (`collectors/sealed_buffers.rs`), or for a binding exposed only by
    // statements of its own body until the first of them
    // ([`distrust_views_the_stmt_may_expose`]). Invalidating at the exposing
    // use itself would not do: on a loop back edge an access that precedes
    // the use runs after it. An arena view keeps its own owner and dispose
    // lifecycle.
    let sealed = ctx.sealed_buffer_locals.contains(&id);
    let late = ctx.late_exposed_buffer_locals.contains(&id);
    let Some(init) = buffer_view_init_for_expr(ctx, init_expr, owned_by_fact) else {
        return;
    };
    let blk = ctx.block();
    let handle = crate::expr::unbox_to_i64(blk, value);
    let handle_ptr = blk.inttoptr(I64, &handle);
    let data_ptr = blk.call(
        PTR,
        "js_native_buffer_data_ptr",
        &[(crate::types::DOUBLE, value)],
    );
    let data_slot = ctx.func.alloca_entry(PTR);
    ctx.block().store(PTR, &data_ptr, &data_slot);
    let length_slot = {
        let len_value = ctx
            .block()
            .call(I32, "js_buffer_length", &[(PTR, &handle_ptr)]);
        let slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, &len_value, &slot);
        Some(slot)
    };
    let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
    ctx.buffer_data_slots
        .insert(id, (data_slot.clone(), scope_idx));
    let native_owned = match init.native_owner_local_id {
        Some(owner_local_id) => {
            let owner_local_id = crate::expr::native_arena_canonical_owner_id(ctx, owner_local_id);
            Some(NativeOwnedViewSlot {
                owner_local_id,
                byte_offset: init.native_byte_offset,
                byte_length: init.native_byte_length,
                owner_rooted: true,
                disposed: false,
                pointer_free_backing: true,
            })
        }
        None => None,
    };
    ctx.receiver_descriptors.materialize_buffer_view(
        id,
        BufferViewSlot {
            data_slot,
            length_slot,
            scope_idx: Some(scope_idx),
            elem: init.elem,
            element_width_bytes: init.element_width_bytes,
            index_unit: init.index_unit,
            view_byte_offset: Some(0),
            length_offset_from_data: init.length_offset_from_data,
            alias: AliasState::NoAliasProven,
            length_source: Some(init.length_source),
            native_owned,
            pointer_state: BufferViewPointerState::Stable,
            storage_inline_proven: init.storage_inline_proven && (sealed || late),
            // Invariant only while nothing can ever detach it: a late view's
            // length must not be carried past its exposing statement. Arena
            // views carry their own length slot.
            length_fixed: sealed && init.native_owner_local_id.is_none(),
        },
    );
    if ctx.native_facts.interior_byte_locals().contains(&id) {
        crate::expr::byte_cell::retain_fresh_local_owner(ctx, value);
    }
    if !sealed && !late && init.native_owner_local_id.is_none() {
        // Exposed from elsewhere (a closure, another function): the view
        // never serves a native access.
        distrust_view(ctx, id);
    }
}

/// Before `stmt` lowers: a late-exposed view (`late_exposed_buffer_locals`)
/// that `stmt` may expose stops being trusted, for `stmt` and everything after
/// it. Arena views keep their own owner and dispose lifecycle. A loop containing the exposing use is itself such a statement, so its
/// whole body (the back edge included) runs untrusted.
pub(crate) fn distrust_views_the_stmt_may_expose(ctx: &mut FnCtx<'_>, stmt: &perry_hir::Stmt) {
    if ctx.late_exposed_buffer_locals.is_empty() {
        return;
    }
    let exposed: Vec<u32> = ctx
        .late_exposed_buffer_locals
        .iter()
        .copied()
        .filter(|id| {
            ctx.receiver_descriptors
                .buffer_view(*id)
                .is_some_and(|view| view.pointer_state.is_stable() && view.native_owned.is_none())
                && crate::collectors::sealed_buffers::stmt_may_expose(stmt, *id)
        })
        .collect();
    for id in exposed {
        distrust_view(ctx, id);
    }
}

/// The end of the statements from `i` on that may lower as one unit: the
/// index of the next statement after `i` that may expose a still-trusted
/// late view, or the end of the list. A matcher that versions a run of
/// statements decides once for the whole run, so the run must stop before
/// the statement that will distrust the view.
pub(crate) fn late_exposure_limit(ctx: &FnCtx<'_>, stmts: &[perry_hir::Stmt], i: usize) -> usize {
    if ctx.late_exposed_buffer_locals.is_empty() {
        return stmts.len();
    }
    let trusted: Vec<u32> = ctx
        .late_exposed_buffer_locals
        .iter()
        .copied()
        .filter(|id| {
            ctx.receiver_descriptors
                .buffer_view(*id)
                .is_some_and(|view| view.pointer_state.is_stable() && view.native_owned.is_none())
        })
        .collect();
    if trusted.is_empty() {
        return stmts.len();
    }
    (i + 1..stmts.len())
        .find(|j| {
            trusted
                .iter()
                .any(|id| crate::collectors::sealed_buffers::stmt_may_expose(&stmts[*j], *id))
        })
        .unwrap_or(stmts.len())
}

/// The view's cached pointer and construction facts no longer hold: its
/// `.buffer` may have been observed (rebinding the storage) and the buffer
/// detached (length 0).
fn distrust_view(ctx: &mut FnCtx<'_>, id: u32) {
    if let Some(view) = ctx.receiver_descriptors.buffer_view_mut(id) {
        view.length_source = Some(LengthSource::Unknown);
        view.storage_inline_proven = false;
        view.length_fixed = false;
    }
    ctx.bounded_buffer_index_pairs
        .retain(|fact| fact.buffer_local_id != id);
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.buffer_local_id != id);
    ctx.min_length_bounds
        .retain(|_, buffer_ids| !buffer_ids.contains(&id));
    crate::expr::invalidate_buffer_view_pointer(
        ctx,
        id,
        crate::native_value::MaterializationReason::MutableAlias,
    );
}

/// A constructor argument that is a literal element count (or absent) proves
/// fresh inline storage: the view form (`new TA(arrayBuffer)`) requires a
/// pointer-valued argument, which a numeric literal can never be.
pub(super) fn ctor_arg_is_literal_length(arg: Option<&perry_hir::Expr>) -> bool {
    match arg {
        None => true,
        Some(perry_hir::Expr::Integer(_)) => true,
        Some(perry_hir::Expr::Number(n)) => n.is_finite() && n.fract() == 0.0,
        _ => false,
    }
}

fn buffer_view_init_for_expr(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    owned_by_fact: bool,
) -> Option<BufferViewInit> {
    match expr {
        perry_hir::Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if module == "buffer" && method == "copyBytesFrom" => Some(BufferViewInit {
            elem: BufferElem::U8,
            element_width_bytes: 1,
            index_unit: BufferIndexUnit::Byte,
            length_offset_from_data: 0,
            length_source: buffer_alloc_length_source(ctx, expr),
            native_owner_local_id: None,
            native_byte_offset: None,
            native_byte_length: None,
            // copyBytesFrom always allocates a fresh inline buffer.
            storage_inline_proven: true,
        }),
        perry_hir::Expr::BufferAlloc { .. } | perry_hir::Expr::BufferAllocUnsafe(_) => {
            Some(BufferViewInit {
                elem: BufferElem::U8,
                element_width_bytes: 1,
                index_unit: BufferIndexUnit::Byte,
                length_offset_from_data: 0,
                length_source: buffer_alloc_length_source(ctx, expr),
                native_owner_local_id: None,
                native_byte_offset: None,
                native_byte_length: None,
                // Buffer.alloc/allocUnsafe always allocate fresh inline bytes.
                storage_inline_proven: true,
            })
        }
        perry_hir::Expr::Uint8ArrayNew(arg) => Some(BufferViewInit {
            elem: BufferElem::U8,
            element_width_bytes: 1,
            index_unit: BufferIndexUnit::Byte,
            length_offset_from_data: 0,
            length_source: buffer_alloc_length_source(ctx, expr),
            native_owner_local_id: None,
            native_byte_offset: None,
            native_byte_length: None,
            // `new Uint8Array(buffer)` is the VIEW form — only a literal
            // length (or no argument) proves inline storage.
            storage_inline_proven: ctor_arg_is_literal_length(arg.as_deref()),
        }),
        perry_hir::Expr::TypedArrayNew { kind, arg } => {
            let (elem, width) = typed_array_elem_width_for_kind(*kind)?;
            Some(BufferViewInit {
                elem,
                element_width_bytes: width,
                index_unit: BufferIndexUnit::Element,
                length_offset_from_data: 0,
                length_source: buffer_alloc_length_source(ctx, expr),
                native_owner_local_id: None,
                native_byte_offset: None,
                native_byte_length: None,
                // The view form (`new TA(arrayBuffer)`) and the copy forms
                // (`new TA(typedArray)`, `new TA(arrayLike)`) all need an
                // Object argument. A literal length is never one, and an owned
                // binding's argument was proven never one by the fact layer
                // (`is_fresh_uint8array_length_expr`: literals, fixed-length
                // locals and non-Object locals), so either proves fresh inline
                // storage. The pre-pass proves the plain-array-source form
                // separately for params.
                storage_inline_proven: owned_by_fact || ctor_arg_is_literal_length(arg.as_deref()),
            })
        }
        perry_hir::Expr::NativeArenaView {
            owner,
            kind,
            byte_offset,
            length,
        } => {
            let (elem, width) = typed_array_elem_width_for_kind(*kind)?;
            let owner_local_id = match owner.as_ref() {
                perry_hir::Expr::LocalGet(id) => Some(*id),
                _ => None,
            }?;
            let byte_offset_const = const_i64_expr(byte_offset);
            let length_const = const_i64_expr(length);
            let native_byte_length = length_const.and_then(|len| len.checked_mul(width as i64));
            Some(BufferViewInit {
                elem,
                element_width_bytes: width,
                index_unit: BufferIndexUnit::Element,
                length_offset_from_data: 0,
                length_source: length_source_from_expr(ctx, length)
                    .unwrap_or(LengthSource::Unknown),
                native_owner_local_id: Some(owner_local_id),
                native_byte_offset: byte_offset_const,
                native_byte_length,
                // Arena views have their own owner/dispose lifecycle — never
                // eligible for the proven checked tier.
                storage_inline_proven: false,
            })
        }
        _ => None,
    }
}

fn typed_array_elem_width_for_kind(kind: u8) -> Option<(BufferElem, u32)> {
    match kind {
        perry_hir::TYPED_ARRAY_KIND_INT8 => Some((BufferElem::I8, 1)),
        perry_hir::TYPED_ARRAY_KIND_UINT8 => Some((BufferElem::U8, 1)),
        perry_hir::TYPED_ARRAY_KIND_UINT8_CLAMPED => Some((BufferElem::U8Clamped, 1)),
        perry_hir::TYPED_ARRAY_KIND_INT16 => Some((BufferElem::I16, 2)),
        perry_hir::TYPED_ARRAY_KIND_UINT16 => Some((BufferElem::U16, 2)),
        perry_hir::TYPED_ARRAY_KIND_INT32 => Some((BufferElem::I32, 4)),
        perry_hir::TYPED_ARRAY_KIND_UINT32 => Some((BufferElem::U32, 4)),
        perry_hir::TYPED_ARRAY_KIND_FLOAT32 => Some((BufferElem::F32, 4)),
        perry_hir::TYPED_ARRAY_KIND_FLOAT64 => Some((BufferElem::F64, 8)),
        _ => None,
    }
}

pub(super) fn math_min_length_buffer_ids(expr: &perry_hir::Expr) -> Option<Vec<u32>> {
    let perry_hir::Expr::MathMin(args) = expr else {
        return None;
    };
    if args.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for arg in args {
        if let Some(id) = length_of_local_buffer_id(arg) {
            out.push(id);
        } else {
            return None;
        }
    }
    out.sort_unstable();
    out.dedup();
    (!out.is_empty()).then_some(out)
}

fn length_of_local_buffer_id(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::Uint8ArrayLength(inner) | perry_hir::Expr::BufferLength(inner) => {
            match inner.as_ref() {
                perry_hir::Expr::LocalGet(id) => Some(*id),
                _ => None,
            }
        }
        perry_hir::Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            perry_hir::Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn buffer_alloc_length_source(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> LengthSource {
    let len = match expr {
        perry_hir::Expr::BufferAlloc { size, .. } => Some(size.as_ref()),
        perry_hir::Expr::BufferAllocUnsafe(size) => Some(size.as_ref()),
        perry_hir::Expr::Uint8ArrayNew(Some(size)) => Some(size.as_ref()),
        perry_hir::Expr::TypedArrayNew {
            arg: Some(size), ..
        } => Some(size.as_ref()),
        perry_hir::Expr::TypedArrayNew { arg: None, .. } => {
            return LengthSource::Constant(0);
        }
        perry_hir::Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if module == "buffer" && method == "copyBytesFrom" => None,
        perry_hir::Expr::NativeArenaView { length, .. } => Some(length.as_ref()),
        _ => None,
    };
    len.and_then(|expr| length_source_from_expr(ctx, expr))
        .unwrap_or(LengthSource::Unknown)
}

fn const_i64_expr(expr: &perry_hir::Expr) -> Option<i64> {
    match expr {
        perry_hir::Expr::Integer(n) => Some(*n),
        perry_hir::Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
        _ => None,
    }
}

fn length_source_from_expr(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> Option<LengthSource> {
    if let Some(range) = crate::expr::int_range_expr(ctx, expr) {
        if range.min == range.max {
            return Some(LengthSource::Constant(range.min));
        }
    }
    match expr {
        perry_hir::Expr::Integer(n) => Some(LengthSource::Constant(*n)),
        perry_hir::Expr::LocalGet(id) => Some(LengthSource::Local { id: *id, addend: 0 }),
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (perry_hir::Expr::LocalGet(id), perry_hir::Expr::Integer(addend))
            | (perry_hir::Expr::Integer(addend), perry_hir::Expr::LocalGet(id)) => {
                Some(LengthSource::Local {
                    id: *id,
                    addend: *addend,
                })
            }
            _ => None,
        },
        _ => None,
    }
}
