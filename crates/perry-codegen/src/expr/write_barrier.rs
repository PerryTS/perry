//! GC write-barrier emission helpers + stream-subclass `super(...)`
//! lowering (extracted from `expr.rs`, issue #1098). Pure move — no
//! logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I32, I64};

/// Gen-GC Phase C2 helper: emit a write barrier after heap-store sites
/// by default. Only explicit `PERRY_WRITE_BARRIERS=0`/`off`/`false`
/// disables emission. Sites with a precise field/element address use
/// `js_write_barrier_slot`; opaque helper stores keep using the
/// compatibility wrapper, which conservatively marks the parent span.
/// The env gate is read once and OnceLock-cached at codegen time.
pub(crate) fn emit_write_barrier(ctx: &mut FnCtx<'_>, parent_bits: &str, child_bits: &str) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    let child_bits_value = LoweredValue::js_value_bits(child_bits.to_string());
    ctx.record_lowered_value(
        "WriteBarrier",
        None,
        "write_barrier.child_bits",
        &child_bits_value,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    ctx.block()
        .call_void("js_write_barrier", &[(I64, parent_bits), (I64, child_bits)]);
}

pub(crate) fn emit_write_barrier_slot_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_addr: &str,
    child_bits: &str,
) {
    if !crate::codegen::write_barriers_enabled() {
        return;
    }
    blk.call_void(
        "js_write_barrier_slot",
        &[(I64, parent_bits), (I64, slot_addr), (I64, child_bits)],
    );
}

pub(crate) fn emit_root_nanbox_store_on_block(blk: &mut LlBlock, value: &str, root_slot: &str) {
    blk.store(DOUBLE, value, root_slot);
    let value_bits = blk.bitcast_double_to_i64(value);
    blk.call_void("js_write_barrier_root_nanbox", &[(I64, &value_bits)]);
}

pub(crate) fn emit_root_heap_word_store_on_block(
    blk: &mut LlBlock,
    value_bits: &str,
    root_slot: &str,
) {
    blk.store(I64, value_bits, root_slot);
    blk.call_void("js_write_barrier_root_heap_word", &[(I64, value_bits)]);
}

/// GC layout-note emission (refs #1090) — at heap-slot stores whose
/// content is known statically, record the per-slot value type so the
/// generational GC can decide whether the slot can be pointer-free
/// (skipped during minor scan). Unlike `emit_write_barrier_slot_on_block`
/// this fires unconditionally — the runtime fn is a no-op when slot
/// tracking is off.
pub(crate) fn emit_layout_note_slot_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_index: &str,
    value_bits: &str,
) {
    blk.call_void(
        "js_gc_note_slot_layout",
        &[(I64, parent_bits), (I32, slot_index), (I64, value_bits)],
    );
}

/// Scalar-aware layout note: passes the slot's previous value (`old_bits`) so
/// the runtime can skip the thread-local layout hashmap when the store does not
/// change the slot's pointer-ness (scalar-over-scalar). See
/// `js_gc_note_slot_layout_aware`.
pub(crate) fn emit_layout_note_slot_aware_on_block(
    blk: &mut LlBlock,
    parent_bits: &str,
    slot_index: &str,
    value_bits: &str,
    old_bits: &str,
) {
    blk.call_void(
        "js_gc_note_slot_layout_aware",
        &[
            (I64, parent_bits),
            (I32, slot_index),
            (I64, value_bits),
            (I64, old_bits),
        ],
    );
}

pub(crate) fn emit_array_numeric_write_note_on_block(
    blk: &mut LlBlock,
    array_bits: &str,
    value_bits: &str,
) {
    blk.call_void(
        "js_array_note_numeric_write",
        &[(I64, array_bits), (I64, value_bits)],
    );
}

pub(crate) fn emit_jsvalue_slot_store_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        false,
        None,
    )
}

pub(crate) fn emit_jsvalue_slot_store_with_value_bits_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    value_bits: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        false,
        Some(value_bits),
    )
}

/// As [`emit_jsvalue_slot_store_on_block`], but for an **in-place element
/// overwrite** of a slot that already holds a valid value: routes the layout
/// note through `js_gc_note_slot_layout_aware`, which loads the previous slot
/// value and skips the thread-local layout hashmap when neither old nor new is
/// a heap pointer. Use only where the slot is guaranteed initialized (array
/// `arr[i] = …` overwrites), not for fresh-slot appends/literals or object
/// field writes (which are POINTER_FREE-dominated and only pay the extra load).
/// This is the dominant per-write cost on downgraded `any[]` numeric loops
/// (#5094) and gives ~9× on `bench_numeric_array_downgrade` without regressing
/// `bench_object_property`.
pub(crate) fn emit_jsvalue_slot_store_scalar_aware_on_block(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
) -> Option<String> {
    emit_jsvalue_slot_store_on_block_inner(
        blk,
        slot_ptr,
        value_double,
        layout_parent_bits,
        slot_index,
        layout_note_needed,
        barrier_parent_bits,
        slot_addr,
        write_barrier_needed,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_jsvalue_slot_store_on_block_inner(
    blk: &mut LlBlock,
    slot_ptr: &str,
    value_double: &str,
    layout_parent_bits: &str,
    slot_index: &str,
    layout_note_needed: bool,
    barrier_parent_bits: &str,
    slot_addr: &str,
    write_barrier_needed: bool,
    scalar_aware: bool,
    value_bits_override: Option<&str>,
) -> Option<String> {
    // The scalar-aware layout note needs the slot's PREVIOUS value to decide
    // whether the slot's pointer-ness actually changed; load it before the
    // store overwrites it. Only when both a note is needed and the caller opted
    // into the scalar-aware path (the slot is a valid in-place overwrite).
    let old_bits = if scalar_aware && layout_note_needed {
        let old_double = blk.load(DOUBLE, slot_ptr);
        Some(blk.bitcast_double_to_i64(&old_double))
    } else {
        None
    };
    // GC_STORE_AUDIT(BARRIERED): generated heap JSValue stores route through this shared emitter.
    blk.store(DOUBLE, value_double, slot_ptr);
    // A uniquely-owned (refcount==1) string written into this slot now aliases
    // it — demote it to shared so a later in-place `+=` on the source local
    // allocates fresh instead of mutating the stored element. This is the inline
    // codegen choke point the runtime store functions (`js_array_push_f64`, …)
    // are bypassed for on the fast paths. Tag-checked at runtime (a no-op for
    // SSO / non-string), and only emitted when the value can be a heap pointer
    // (`layout_note_needed`), so numeric stores pay nothing. Mirrors the
    // object-field demote in `runtime_store_jsvalue_slot` (#5533).
    if layout_note_needed {
        blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
    }
    if !layout_note_needed && !write_barrier_needed {
        return None;
    }
    let value_bits = value_bits_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| blk.bitcast_double_to_i64(value_double));
    if layout_note_needed {
        match old_bits.as_deref() {
            // Scalar-over-scalar stores leave the GC slot layout unchanged — the
            // aware note skips the thread-local layout hashmap when neither the
            // new nor the old value is a heap pointer (#5094).
            Some(old) => emit_layout_note_slot_aware_on_block(
                blk,
                layout_parent_bits,
                slot_index,
                &value_bits,
                old,
            ),
            None => {
                emit_layout_note_slot_on_block(blk, layout_parent_bits, slot_index, &value_bits)
            }
        }
    }
    if write_barrier_needed {
        emit_write_barrier_slot_on_block(blk, barrier_parent_bits, slot_addr, &value_bits);
    }
    Some(value_bits)
}

/// Issue #562 — `super({ ... })` for `class X extends ReadableStream`,
/// `WritableStream`, or `TransformStream`. Extracts the underlying
/// source/sink/transformer callbacks from the inline object literal,
/// lowers each one (TAG_UNDEFINED for missing fields), and calls the
/// runtime `*_subclass_init` shim — which allocates the stream registry
/// handle and stashes it on `this` under `__perry_stream_handle__`.
///
/// `kind` is one of `"readable"` / `"writable"` / `"transform"` —
/// matches the SuperCall arm's `parent_name` switch in expr.rs.
pub(crate) fn lower_stream_super_init(
    ctx: &mut FnCtx<'_>,
    kind: &str,
    super_args: &[Expr],
) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

    // Pre-extract field exprs so we don't hold a borrow across `lower_expr`.
    let opts_props: Option<Vec<(String, Expr)>> = super_args
        .first()
        .and_then(|first| crate::lower_call::extract_options_fields(ctx, first));
    let qstrat_props: Option<Vec<(String, Expr)>> = super_args
        .get(1)
        .and_then(|second| crate::lower_call::extract_options_fields(ctx, second));

    // Lower the canonical callback set per stream kind. Fields not
    // present (or callable arg shape that isn't an inline literal) fall
    // back to TAG_UNDEFINED — matches the existing `new ReadableStream
    // / WritableStream / TransformStream` lowerings in
    // `lower_call/builtin.rs`.
    let mut start = undef_lit.clone();
    let mut pull = undef_lit.clone();
    let mut cancel = undef_lit.clone();
    let mut write = undef_lit.clone();
    let mut close = undef_lit.clone();
    let mut abort = undef_lit.clone();
    let mut transform = undef_lit.clone();
    let mut flush = undef_lit.clone();

    if let Some(props) = opts_props {
        for (k, vexpr) in &props {
            match (kind, k.as_str()) {
                ("readable", "start") => start = lower_expr(ctx, vexpr)?,
                ("readable", "pull") => pull = lower_expr(ctx, vexpr)?,
                ("readable", "cancel") => cancel = lower_expr(ctx, vexpr)?,
                ("writable", "write") => write = lower_expr(ctx, vexpr)?,
                ("writable", "close") => close = lower_expr(ctx, vexpr)?,
                ("writable", "abort") => abort = lower_expr(ctx, vexpr)?,
                ("transform", "transform") => transform = lower_expr(ctx, vexpr)?,
                ("transform", "flush") => flush = lower_expr(ctx, vexpr)?,
                _ => {
                    // Lower for side effects (closure-capture collection,
                    // string-pool registration, etc.) but discard the value.
                    let _ = lower_expr(ctx, vexpr)?;
                }
            }
        }
    } else if let Some(first) = super_args.first() {
        // Caller passed something that isn't a recognized shape — lower
        // for side effects so closure analysis stays consistent.
        let _ = lower_expr(ctx, first)?;
    }

    let mut hwm = double_literal(1.0);
    if let Some(qprops) = qstrat_props {
        for (k, vexpr) in &qprops {
            if k == "highWaterMark" {
                hwm = lower_expr(ctx, vexpr)?;
            } else {
                let _ = lower_expr(ctx, vexpr)?;
            }
        }
    } else if let Some(second) = super_args.get(1) {
        let _ = lower_expr(ctx, second)?;
    }

    // `this` (NaN-boxed pointer) — the runtime shim stashes the handle
    // on it via `js_object_set_field_by_name`.
    let this_slot = ctx.this_stack.last().cloned();
    let this_box = match this_slot {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    let runtime_fn = match kind {
        "readable" => "js_readable_stream_subclass_init",
        "writable" => "js_writable_stream_subclass_init",
        "transform" => "js_transform_stream_subclass_init",
        _ => unreachable!("lower_stream_super_init: unexpected kind {}", kind),
    };

    let blk = ctx.block();
    match kind {
        "readable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &start),
                    (DOUBLE, &pull),
                    (DOUBLE, &cancel),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "writable" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &write),
                    (DOUBLE, &close),
                    (DOUBLE, &abort),
                    (DOUBLE, &hwm),
                ],
            );
        }
        "transform" => {
            blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (DOUBLE, &this_box),
                    (DOUBLE, &transform),
                    (DOUBLE, &flush),
                    (DOUBLE, &hwm),
                ],
            );
        }
        _ => unreachable!(),
    }

    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
}

/// Initialize Node's classic stream base class on an existing subclass
/// instance. Unlike `new Readable/Writable(opts)`, `super(opts)` must keep the
/// object identity allocated for the derived class and attach stream state to it.
pub(crate) fn lower_node_stream_super_init(
    ctx: &mut FnCtx<'_>,
    kind: &str,
    super_args: &[Expr],
) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let opts = if let Some(first) = super_args.first() {
        lower_expr(ctx, first)?
    } else {
        undef_lit.clone()
    };
    for arg in super_args.iter().skip(1) {
        let _ = lower_expr(ctx, arg)?;
    }

    let this_box = match ctx.this_stack.last().cloned() {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    let runtime_fn = match kind {
        "readable" => "js_node_stream_readable_subclass_init",
        "writable" => "js_node_stream_writable_subclass_init",
        "duplex" => "js_node_stream_duplex_subclass_init",
        "transform" => "js_node_stream_transform_subclass_init",
        _ => unreachable!(
            "lower_node_stream_super_init: unexpected Node stream kind {}",
            kind
        ),
    };
    ctx.block()
        .call(DOUBLE, runtime_fn, &[(DOUBLE, &this_box), (DOUBLE, &opts)]);

    Ok(undef_lit)
}

/// `super(n)` for a source-compiled `class X extends Array` (lru-cache's
/// `ZeroArray` etc.): perry models the instance as a plain object, so size it
/// and install the Array surface via the runtime `js_array_subclass_init`
/// (mirrors `lower_node_stream_super_init` / the EventEmitter subclass init).
pub(crate) fn lower_array_super_init(ctx: &mut FnCtx<'_>, super_args: &[Expr]) -> Result<String> {
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let n = if let Some(first) = super_args.first() {
        lower_expr(ctx, first)?
    } else {
        undef_lit.clone()
    };
    for arg in super_args.iter().skip(1) {
        let _ = lower_expr(ctx, arg)?;
    }

    let this_box = match ctx.this_stack.last().cloned() {
        Some(slot) => ctx.block().load(DOUBLE, &slot),
        None => undef_lit.clone(),
    };

    ctx.block().call(
        DOUBLE,
        "js_array_subclass_init",
        &[(DOUBLE, &this_box), (DOUBLE, &n)],
    );

    Ok(undef_lit)
}

/// #5137: install the bare EventEmitter listener/emit surface onto `this_box`
/// for a source-compiled `class X extends EventEmitter` (node:events). Shared
/// by the explicit-`super()` arm (`expr/this_super_call.rs`) and the
/// no-own-constructor `new` path (`lower_call/new.rs`). The runtime helper
/// reuses the generic `ns_*` emitter closures (they key all state off the
/// receiver), so a plain object that never went through a stream constructor
/// gets working `.on`/`.emit`/`.once`/…. Reached when an EventEmitter
/// subclass's real npm source is compiled — e.g. commander's `Command` under
/// `perry.compilePackages`, where the `new Command()` → `js_commander_*`
/// native-shim path is intentionally off.
pub(crate) fn lower_event_emitter_subclass_init(ctx: &mut FnCtx<'_>, this_box: &str) {
    ctx.block().call(
        DOUBLE,
        "js_event_emitter_subclass_init",
        &[(DOUBLE, this_box)],
    );
}
