//! Buffer/typed-array view-slot registration for `Stmt::Let` bindings.
//! Mechanically extracted from `let_stmt.rs` (file-size gate); behavior
//! unchanged. `pub(super)` — consumed only by the `stmt` module.

use crate::expr::FnCtx;
use crate::native_value::{
    AliasState, BufferElem, BufferIndexUnit, BufferViewSlot, LengthSource, NativeOwnedViewSlot,
};
use crate::types::{I32, I64, I8, PTR};

pub(super) struct BufferViewInit {
    elem: BufferElem,
    element_width_bytes: u32,
    index_unit: BufferIndexUnit,
    data_offset_bytes: i32,
    length_offset_from_data: i32,
    length_source: LengthSource,
    native_owner_local_id: Option<u32>,
    native_byte_offset: Option<i64>,
    native_byte_length: Option<i64>,
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
    let Some(init) = buffer_view_init_for_expr(ctx, init_expr) else {
        return;
    };
    let blk = ctx.block();
    let handle = crate::expr::unbox_to_i64(blk, value);
    let handle_ptr = blk.inttoptr(I64, &handle);
    let data_ptr = if init.native_owner_local_id.is_some() {
        let data_field = blk.gep(I8, &handle_ptr, &[(I32, "24")]);
        blk.load(PTR, &data_field)
    } else {
        blk.gep(
            I8,
            &handle_ptr,
            &[(I32, &init.data_offset_bytes.to_string())],
        )
    };
    let data_slot = ctx.func.alloca_entry(PTR);
    ctx.block().store(PTR, &data_ptr, &data_slot);
    let length_slot = if init.native_owner_local_id.is_some() {
        let len_field = ctx.block().gep(I8, &handle_ptr, &[(I32, "0")]);
        let len_value = ctx.block().load(I32, &len_field);
        let slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, &len_value, &slot);
        Some(slot)
    } else {
        None
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
    ctx.buffer_view_slots.insert(
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
            storage_inline_proven: init.storage_inline_proven,
        },
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

fn buffer_view_init_for_expr(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> Option<BufferViewInit> {
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
            data_offset_bytes: 8,
            length_offset_from_data: -8,
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
                data_offset_bytes: 8,
                length_offset_from_data: -8,
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
            data_offset_bytes: 8,
            length_offset_from_data: -8,
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
                data_offset_bytes: 16,
                length_offset_from_data: -16,
                length_source: buffer_alloc_length_source(ctx, expr),
                native_owner_local_id: None,
                native_byte_offset: None,
                native_byte_length: None,
                // Same view-form hazard as Uint8ArrayNew: only a literal
                // length proves the non-view construction here (the pre-pass
                // proves the plain-array-source form separately for params).
                storage_inline_proven: ctor_arg_is_literal_length(arg.as_deref()),
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
                data_offset_bytes: 24,
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
