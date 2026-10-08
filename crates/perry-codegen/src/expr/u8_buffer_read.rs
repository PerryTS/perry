//! Header-guarded Uint8Array and Buffer reads through the common byte cell.
//! Owner reads resolve inline/out-of-line data; fixed views resolve their
//! flattened owner. Bagged, resizable, detached and shared stores use the
//! runtime arm. Loop parameters hoist data/length and root receiver/owner.

use perry_hir::Expr;

use super::FnCtx;
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `PERRY_U8_INLINE_READ=0` kill switch (default on).
pub(crate) fn u8_inline_read_enabled() -> bool {
    match std::env::var("PERRY_U8_INLINE_READ") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false" | "OFF" | "FALSE"),
        Err(_) => true,
    }
}

/// Static receiver eligibility: a plain local/module-global read whose class
/// proves `Uint8Array`, not owned by the (stronger) tracked-view path. The
/// runtime header guard is the safety net — a stale hint takes the runtime arm —
/// but reassigned bindings are excluded anyway, mirroring
/// `ta_param_f64_read::checked_typed_array_f64_kind`'s reasoning.
pub(crate) fn u8_buffer_receiver_eligible(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if ctx.receiver_descriptors.contains_buffer_view(id) {
        return false;
    }
    if ctx
        .receiver_descriptors
        .byte_view_param(*id)
        .is_some_and(|access| {
            access.brands.iter().all(|brand| {
                matches!(
                    *brand,
                    crate::runtime_abi::GC_TYPE_BUFFER
                        | crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY
                )
            })
        })
    {
        return true;
    }
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| {
            if ctx.reassigned_locals.contains(id) {
                return None;
            }
            match ctx.module_global_proven_types.get(id) {
                Some(perry_hir::types::Type::Named(name)) => Some(name.clone()),
                _ => None,
            }
        })
        .or_else(|| {
            // #9363: a declared `Uint8Array` parameter, on the same
            // guard-validated-hint terms as the typed-array lanes.
            if ctx.reassigned_locals.contains(id) {
                return None;
            }
            match ctx.local_type_hint(id) {
                Some(perry_hir::types::Type::Named(name)) => Some(name.clone()),
                _ => None,
            }
        });
    class.as_deref() == Some("Uint8Array")
}

// ---------------------------------------------------------------------------
// The common byte-cell admission, for i32-ABI reads and for writes.
//
// The header contract (`perry-runtime/src/buffer/header.rs`) is that a proof
// admits a Buffer/Uint8Array owner or fixed view through its common header.
// Views resolve the current owner state and byteOffset; no propagation or
// address cache is needed. Unsupported owner states, property bags and
// invalid receivers take the runtime accessor.
// ---------------------------------------------------------------------------

/// Resolve `obj_box` through its current byte-cell header.
fn emit_u8_header_admission(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    miss: &str,
) -> super::byte_cell::Access {
    super::byte_cell::resolve(
        ctx,
        obj_box,
        &[
            crate::runtime_abi::GC_TYPE_BUFFER,
            crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY,
        ],
        miss,
    )
}

/// Access uses the common cell header and current owner storage. Any derived
/// data address is consumed without collection, or retained with its owner.
pub(crate) fn emit_u8_inline_header_guard(blk: &mut crate::block::LlBlock, raw: &str) -> String {
    let h = super::byte_cell::header_word(blk, raw);
    let t = blk.and(I64, &h, &(0xffu64 | (1 << 23)).to_string());
    let node = blk.icmp_eq(I64, &t, &crate::runtime_abi::GC_TYPE_BUFFER.to_string());
    let u8 = blk.icmp_eq(
        I64,
        &t,
        &crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY.to_string(),
    );
    blk.or(I1, &node, &u8)
}

/// `idx ult length` against an admitted buffer's `u32` length at offset 0.
/// `ult` also rejects a negative (or `-1`-sentinel) index.
fn emit_u8_in_bounds(ctx: &mut FnCtx<'_>, raw: &str, idx_i32: &str) -> String {
    let blk = ctx.block();
    let hdr_ptr = blk.inttoptr(I64, raw);
    let len = blk.load(I32, &hdr_ptr);
    blk.icmp_ult(I32, idx_i32, &len)
}

fn emit_u8_byte_ptr(ctx: &mut FnCtx<'_>, raw: &str, idx_i32: &str) -> String {
    let blk = ctx.block();
    let data_base = raw;
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let addr = blk.add(I64, &data_base, &idx_i64);
    blk.inttoptr(I64, &addr)
}

/// Guarded inline byte READ in the runtime helper's native i32 ABI:
/// `slow_fn(handle: i64, idx: i32) -> i32` (`js_uint8array_get` /
/// `js_buffer_get`, which answer the `0` byte sentinel out of range).
pub(crate) fn emit_u8_cached_get_i32(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    obj_box: &str,
    idx_i32: &str,
    slow_fn: &str,
) -> String {
    let chk_idx = ctx.new_block("u8c.get.chk");
    let load_idx = ctx.new_block("u8c.get.load");
    let slow_idx = ctx.new_block("u8c.get.slow");
    let merge_idx = ctx.new_block("u8c.get.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let raw = super::unbox_to_i64(ctx.block(), obj_box);
    let access = super::byte_cell::resolve_read(
        ctx,
        object,
        obj_box,
        &[
            crate::runtime_abi::GC_TYPE_BUFFER,
            crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY,
        ],
        &slow_label,
    );
    let hit = "true";
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = ctx.block().icmp_ult(I32, idx_i32, &access.len);
    ctx.block().cond_br(&in_bounds, &load_label, &slow_label);

    ctx.current_block = load_idx;
    let ptr = emit_u8_byte_ptr(ctx, &access.data, idx_i32);
    let (fast_val, fast_end) = {
        let blk = ctx.block();
        let byte = blk.load(I8, &ptr);
        let val = blk.zext(I8, &byte, I32);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = slow_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let val = blk.call(I32, slow_fn, &[(I64, &raw), (I32, idx_i32)]);
        let end = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        (val, end)
    };

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_val.as_str(), fast_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    )
}

/// Guarded inline byte read yielding a JS value: the byte as a Number in
/// bounds, else `slow_fn(handle, idx) -> double` (`js_uint8array_index_get_value`
/// / `js_buffer_index_get_value`, which answer `undefined` out of range).
pub(crate) fn emit_u8_cached_get_value(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_i32: &str,
    slow_fn: &str,
) -> String {
    let chk_idx = ctx.new_block("u8c.getv.chk");
    let load_idx = ctx.new_block("u8c.getv.load");
    let slow_idx = ctx.new_block("u8c.getv.slow");
    let merge_idx = ctx.new_block("u8c.getv.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let access = emit_u8_header_admission(ctx, obj_box, &slow_label);
    let raw = &access.raw;
    let hit = "true";
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = ctx.block().icmp_ult(I32, idx_i32, &access.len);
    ctx.block().cond_br(&in_bounds, &load_label, &slow_label);

    ctx.current_block = load_idx;
    let ptr = emit_u8_byte_ptr(ctx, &access.data, idx_i32);
    let (fast_val, fast_end) = {
        let blk = ctx.block();
        let byte = blk.load(I8, &ptr);
        let val = blk.uitofp(I8, &byte, DOUBLE);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = slow_idx;
    let fallback_idx = ctx.new_block("u8c.getv.fallback");
    let fallback_label = ctx.block_label(fallback_idx);
    let (view_val, view_end) = emit_u8_view_get_value(
        ctx,
        obj_box,
        &raw,
        idx_i32,
        &fallback_label,
        &merge_label,
        None,
    );
    ctx.current_block = fallback_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let val = blk.call(DOUBLE, slow_fn, &[(I64, &raw), (I32, idx_i32)]);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (fast_val.as_str(), fast_end.as_str()),
            (view_val.as_str(), view_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    )
}

/// Access uses the common cell header and current owner storage. Any derived
/// data address is consumed without collection, or retained with its owner.
pub(crate) fn emit_u8_cached_dyn_set(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    slow: impl FnOnce(&mut FnCtx<'_>),
) {
    let chk_idx = ctx.new_block("u8d.set.chk");
    let store_idx = ctx.new_block("u8d.set.store");
    let slow_idx = ctx.new_block("u8d.set.slow");
    let merge_idx = ctx.new_block("u8d.set.merge");
    let chk_label = ctx.block_label(chk_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let access = super::byte_cell::resolve_write(
        ctx,
        obj_box,
        &[
            crate::runtime_abi::GC_TYPE_BUFFER,
            crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY,
        ],
        &slow_label,
    );
    let hit = "true";
    {
        let blk = ctx.block();
        // Range tests before any `fptosi`, whose out-of-range result is poison.
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "2147483648.0");
        let val_bits = blk.bitcast_double_to_i64(val_double);
        // 0x7FF9 << 48: the lowest NaN-box tag.
        let val_is_number = blk.icmp_slt(I64, &val_bits, "9221401712017801216");
        let val_lt = blk.fcmp("olt", val_double, "2147483648.0");
        let val_gt = blk.fcmp("ogt", val_double, "-2147483648.0");
        let g = blk.and(I1, &hit, &idx_ge0);
        let g = blk.and(I1, &g, &idx_lt);
        let g = blk.and(I1, &g, &val_is_number);
        let g = blk.and(I1, &g, &val_lt);
        let g = blk.and(I1, &g, &val_gt);
        blk.cond_br(&g, &chk_label, &slow_label);
    }

    ctx.current_block = chk_idx;
    let (idx_i32, val_i32) = {
        let blk = ctx.block();
        let idx_i32 = blk.fptosi(DOUBLE, idx_d, I32);
        let val_i32 = blk.fptosi(DOUBLE, val_double, I32);
        (idx_i32, val_i32)
    };
    let ok = {
        let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &access.len);
        let blk = ctx.block();
        let idx_back = blk.sitofp(I32, &idx_i32, DOUBLE);
        let is_int = blk.fcmp("oeq", &idx_back, idx_d);
        blk.and(I1, &is_int, &in_bounds)
    };
    ctx.block().cond_br(&ok, &store_label, &slow_label);

    ctx.current_block = store_idx;
    let ptr = emit_u8_byte_ptr(ctx, &access.data, &idx_i32);
    {
        let blk = ctx.block();
        let byte = blk.trunc(I32, &val_i32, I8);
        blk.store(I8, &byte, &ptr);
        blk.br(&merge_label);
    }

    ctx.current_block = slow_idx;
    slow(ctx);
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
}

/// Guarded inline byte WRITE in the runtime helper's i32 ABI: `val_i32` is
/// already ToInt32'd by the caller, and the store keeps its low byte exactly
/// as `js_buffer_set` does (`value & 0xFF`). Misses call
/// `slow_fn(handle, idx, value)` (`js_uint8array_set` / `js_buffer_set`).
pub(crate) fn emit_u8_cached_set_i32(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    obj_box: &str,
    idx_i32: &str,
    val_i32: &str,
    slow_fn: &str,
) {
    let chk_idx = ctx.new_block("u8c.set.chk");
    let store_idx = ctx.new_block("u8c.set.store");
    let slow_idx = ctx.new_block("u8c.set.slow");
    let merge_idx = ctx.new_block("u8c.set.merge");
    let chk_label = ctx.block_label(chk_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let raw = super::unbox_to_i64(ctx.block(), obj_box);
    let access = super::byte_cell::resolve_indexed_write(
        ctx,
        object,
        obj_box,
        &[
            crate::runtime_abi::GC_TYPE_BUFFER,
            crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY,
        ],
        &slow_label,
    );
    let hit = "true";
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = ctx.block().icmp_ult(I32, idx_i32, &access.len);
    ctx.block().cond_br(&in_bounds, &store_label, &slow_label);

    ctx.current_block = store_idx;
    let ptr = emit_u8_byte_ptr(ctx, &access.data, idx_i32);
    {
        let blk = ctx.block();
        let byte = blk.trunc(I32, val_i32, I8);
        blk.store(I8, &byte, &ptr);
        blk.br(&merge_label);
    }

    ctx.current_block = slow_idx;
    {
        let blk = ctx.block();
        blk.call_void(slow_fn, &[(I64, &raw), (I32, idx_i32), (I32, val_i32)]);
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
}

/// The view miss arm. A POINTER-tagged, aligned heap receiver must carry a
/// byte-view brand and the pointer-storage flag before its private payload is
/// read. Symbols lack GC headers, so screen their payload magic as well.
/// No calls, allocations or user expressions occur in this window.
pub(crate) fn emit_u8_view_data_guard(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    _raw: &str,
    miss_label: &str,
) -> String {
    emit_u8_header_admission(ctx, obj_box, miss_label).data
}

fn emit_u8_view_get_value(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    raw: &str,
    idx: &str,
    miss_label: &str,
    merge_label: &str,
    param_access: Option<&crate::collectors::ByteViewParamAccess>,
) -> (String, String) {
    let data = emit_u8_view_param_or_guard(ctx, obj_box, raw, miss_label, param_access);
    let load_idx = ctx.new_block("u8v.load");
    let oob_idx = ctx.new_block("u8v.oob");
    let done_idx = ctx.new_block("u8v.done");
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let done_label = ctx.block_label(done_idx);
    let bounds = emit_u8_in_bounds(ctx, raw, idx);
    ctx.block().cond_br(&bounds, &load_label, &oob_label);
    ctx.current_block = load_idx;
    let offset = ctx.block().zext(I32, idx, I64);
    let addr = ctx.block().add(I64, &data, &offset);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let val = super::ta_element_read::emit_element(ctx.block(), &ptr, 1);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = oob_idx;
    let oob_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    let undef = double_literal(f64::from_bits(TAG_UNDEFINED));
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&val, &load_end), (&undef, &oob_end)]);
    let end = ctx.block().label.clone();
    ctx.block().br(merge_label);
    (value, end)
}

/// The hoisted access proof for a byte read or write on local `object`,
/// revalidated for the current receiver `boxed` when it is dirty or was
/// resolved for another value. Construction-proven module bindings use the
/// same proof as locals; only the constructed binding is invariant, not storage.
/// Captured (boxed) locals keep the per-access header resolution.
pub(crate) fn byte_view_param_for(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    boxed: &str,
    brands: &[u8],
) -> Option<crate::collectors::ByteViewParamAccess> {
    if ctx.is_async_fn {
        return None;
    }
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let constructed_global =
        ctx.module_globals.contains_key(id) && ctx.module_global_proven_types.contains_key(id);
    if ctx.boxed_vars.contains(id) || (!ctx.locals.contains_key(id) && !constructed_global) {
        return None;
    }
    let mut access = if constructed_global {
        ctx.receiver_descriptors
            .byte_view_access(*id, brands)?
            .clone()
    } else {
        super::byte_cell::access_for(ctx, *id, brands)?
    };
    super::byte_cell::revalidate(ctx, &access, boxed);
    let state = ctx.block().load(crate::types::I8, &access.valid_slot);
    access.valid_i1 = ctx.block().icmp_eq(crate::types::I8, &state, "1");
    access.data_i64 = ctx.block().load(I64, &access.data_slot);
    Some(access)
}

/// The brand set of a Uint8Array/Buffer receiver proof.
pub(crate) const U8_BRANDS: [u8; 2] = [
    crate::runtime_abi::GC_TYPE_BUFFER,
    crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY,
];

/// Amortize entry validation only for indexed accesses in loops. Single-access
/// helpers retain their existing owning-cache hit and add no entry calls.
/// Nested closures get their own receiver guards when they are lowered.
pub(crate) fn byte_view_param_is_used(body: &[perry_hir::Stmt], id: u32) -> bool {
    u8_inline_read_enabled() && loop_param_is_accessed(body, id)
}

pub(crate) fn loop_param_is_accessed(body: &[perry_hir::Stmt], id: u32) -> bool {
    fn reads(expr: &Expr, id: u32) -> bool {
        if matches!(expr, Expr::Closure { .. }) {
            return false;
        }
        if matches!(expr,
            Expr::IndexGet { object, .. } | Expr::IndexSet { object, .. }
                | Expr::Uint8ArrayGet { array: object, .. } | Expr::Uint8ArraySet { array: object, .. }
                | Expr::BufferIndexGet { buffer: object, .. } | Expr::BufferIndexSet { buffer: object, .. }
                if matches!(object.as_ref(), Expr::LocalGet(receiver) if *receiver == id))
        {
            return true;
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(expr, &mut |child| found |= reads(child, id));
        found
    }
    fn loop_reads(stmt: &perry_hir::Stmt, id: u32) -> bool {
        use perry_hir::Stmt;
        let any_read = |body: &[Stmt]| {
            body.iter()
                .any(|stmt| perry_hir::walker::stmt_any_expr(stmt, &mut |expr| reads(expr, id)))
        };
        match stmt {
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                reads(condition, id) || any_read(body)
            }
            Stmt::For {
                condition,
                update,
                body,
                ..
            } => {
                condition.as_ref().is_some_and(|expr| reads(expr, id))
                    || update.as_ref().is_some_and(|expr| reads(expr, id))
                    || any_read(body)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                then_branch.iter().any(|stmt| loop_reads(stmt, id))
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| branch.iter().any(|stmt| loop_reads(stmt, id)))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().any(|stmt| loop_reads(stmt, id))
                    || catch
                        .as_ref()
                        .is_some_and(|clause| clause.body.iter().any(|stmt| loop_reads(stmt, id)))
                    || finally
                        .as_ref()
                        .is_some_and(|body| body.iter().any(|stmt| loop_reads(stmt, id)))
            }
            Stmt::Switch { cases, .. } => cases
                .iter()
                .any(|case| case.body.iter().any(|stmt| loop_reads(stmt, id))),
            Stmt::Labeled { body, .. } => loop_reads(body, id),
            _ => false,
        }
    }
    body.iter().any(|stmt| loop_reads(stmt, id))
}

pub(crate) fn materialize_byte_view_param(ctx: &mut FnCtx<'_>, id: u32, boxed: &str) {
    super::byte_cell::materialize_param(ctx, id, boxed, &U8_BRANDS);
}

pub(crate) fn emit_u8_view_param_or_guard(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    raw: &str,
    miss_label: &str,
    param_access: Option<&crate::collectors::ByteViewParamAccess>,
) -> String {
    if let Some(access) = param_access {
        let hit_idx = ctx.new_block("u8v.param.hit");
        let hit_label = ctx.block_label(hit_idx);
        ctx.block()
            .cond_br(&access.valid_i1, &hit_label, miss_label);
        ctx.current_block = hit_idx;
        access.data_i64.clone()
    } else {
        emit_u8_view_data_guard(ctx, obj_box, raw, miss_label)
    }
}

#[cfg(test)]
mod byte_loop_proof_tests {
    use super::*;
    use perry_hir::Stmt;

    #[test]
    fn entry_resolution_requires_a_loop_read_of_this_parameter() {
        let read = Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        });
        assert!(!byte_view_param_is_used(&[read.clone()], 1));
        let loop_read = Stmt::For {
            init: None,
            condition: None,
            update: None,
            body: vec![read.clone()],
        };
        assert!(byte_view_param_is_used(&[loop_read.clone()], 1));
        assert!(!byte_view_param_is_used(&[loop_read], 2));
        let init_only = Stmt::For {
            init: Some(Box::new(read)),
            condition: None,
            update: None,
            body: vec![],
        };
        assert!(!byte_view_param_is_used(&[init_only], 1));
        // The HIR specializes `bytes[i]` to Uint8ArrayGet before codegen.
        let native_read = Stmt::Expr(Expr::Uint8ArrayGet {
            array: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        });
        assert!(!byte_view_param_is_used(&[native_read.clone()], 1));
        assert!(byte_view_param_is_used(
            &[Stmt::For {
                init: None,
                condition: None,
                update: None,
                body: vec![native_read],
            }],
            1
        ));
    }
}
