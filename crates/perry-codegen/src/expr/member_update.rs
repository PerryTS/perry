//! `o.f++` / `o.f--` / `a[i]++` / `--a[i]` — the member read-modify-write arms.
//!
//! Split out of `expr/instance_misc1.rs` in #7628, which pushed that file past
//! the 2000-line cap. The arm bodies are verbatim; the dispatch entry is
//! `lower`, called from that file's `Expr::PropertyUpdate | Expr::IndexUpdate`
//! arm.
//!
//! # The rooting story, and the part of #7628 that did not survive measurement
//!
//! Both arms consume their operands at four calls with collection points
//! between them:
//!
//! ```text
//! old     = <read>(obj, key)            ; a getter / Proxy trap
//! old_num = js_to_numeric(old)          ; a valueOf
//! new     = js_numeric_step(old_num, s) ; allocates only (#7198)
//!           <write>(obj, key, new)      ; a setter / Proxy trap
//! ```
//!
//! #7628 filed the group-wide single re-read as a live #7154 and asked for a
//! per-use-re-read combinator. Slice 6 had already built one (`RootedGroup`),
//! so both arms use it and no new primitive arrived with this caller — but the
//! operand half turned out **not to be a live bug**: `root_reload` (#7280)
//! rematerialises the slot load at every use a collection point can reach,
//! including through the `ptrtoint` + `and POINTER_MASK` handle derivation.
//! Collapsing the re-reads back to one leaves the emitted IR the same. The
//! per-use form is kept because it is free and removes the dependence on a pass
//! with a documented side condition; it is not the repair.
//!
//! The repair is the **result**. For a BigInt element `js_to_numeric` /
//! `js_numeric_step` hand back a heap `BigIntHeader`, and the one the
//! expression yields — `old_num` for postfix, `new` for prefix — is live across
//! the write, i.e. across a user setter, as a bare call result with **no slot**
//! for `root_reload` to reload from. `RootedGroup::adopt_emitted` closes it,
//! gated on `is_provably_not_bigint` so a typed-array `ta[i]++` pays nothing.
//! `expr/issue7628_rooting_tests.rs` carries the measurement and the sabotage
//! arms.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use crate::nanbox::POINTER_MASK_I64;
use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I32, I64, I8};

use super::{lower_expr, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        // -------- obj.field++ / obj.field-- (PropertyUpdate) --------
        // Lowered as: load → fadd/fsub 1.0 → store. Same as the
        // Update variant but for a property instead of a local.
        Expr::PropertyUpdate {
            object,
            property,
            op,
            prefix,
        } => {
            // Scalar replacement fast path: load → fadd/fsub 1.0 → store
            // on the field's alloca, no heap traffic.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let blk = ctx.block();
                    let old = blk.load(DOUBLE, &slot);
                    let old_num = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &old)]);
                    let new = match op {
                        BinaryOp::Sub => blk.fsub(&old_num, "1.0"),
                        _ => blk.fadd(&old_num, "1.0"),
                    };
                    blk.store(DOUBLE, &new, &slot);
                    return Ok(if *prefix { new } else { old_num });
                }
            }
            if let Expr::This = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_ctor_target
                    .last()
                    .and_then(|tid| ctx.scalar_replaced.get(tid))
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let blk = ctx.block();
                    let old = blk.load(DOUBLE, &slot);
                    let old_num = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &old)]);
                    let new = match op {
                        BinaryOp::Sub => blk.fsub(&old_num, "1.0"),
                        _ => blk.fadd(&old_num, "1.0"),
                    };
                    blk.store(DOUBLE, &new, &slot);
                    return Ok(if *prefix { new } else { old_num });
                }
            }
            // Representation-selection Phase 3b: `o.f++` on a shape-proven
            // Ptr<Shape> local whose field is numeric-proven — bare
            // load/fadd/store at the fixed offset, no by-name runtime calls.
            // The store keeps the raw-slot plain-finite discipline (an
            // Inf-crossing update side-exits to the by-name setter, which
            // performs the layout downgrade the GC scan relies on).
            // (Phase 5a's proven `this` never claims numeric fields, so this
            // site remains Phase-3b-local-only in practice.)
            {
                let fact = ctx.ptr_shape_receiver_fact(object.as_ref()).cloned();
                {
                    if let Some(fact) = fact {
                        if fact.numeric_fields.contains(property.as_str()) {
                            if let Some(field_index) =
                                crate::type_analysis::class_field_global_index(
                                    ctx,
                                    &fact.class_name,
                                    property,
                                )
                            {
                                ctx.note_ptr_shape_consumed(object.as_ref(), "ptr_shape_update");
                                let recv_box = lower_expr(ctx, object)?;
                                let field_idx_str = field_index.to_string();
                                let header_skip = crate::target_layout::object_header_size_bytes(
                                    ctx.target_triple,
                                )
                                .to_string();
                                let (obj_handle, field_ptr, old, new) = {
                                    let blk = ctx.block();
                                    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                                    let obj_ptr = blk.inttoptr(I64, &obj_handle);
                                    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                    let field_ptr =
                                        blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                                    let old = blk.load(DOUBLE, &field_ptr);
                                    let new = match op {
                                        BinaryOp::Sub => blk.fsub(&old, "1.0"),
                                        _ => blk.fadd(&old, "1.0"),
                                    };
                                    (obj_handle, field_ptr, old, new)
                                };
                                let store_idx = ctx.new_block("ptr_shape_update.raw_store");
                                let cold_idx = ctx.new_block("ptr_shape_update.downgrade");
                                let merge_idx = ctx.new_block("ptr_shape_update.merge");
                                let store_label = ctx.block_label(store_idx);
                                let cold_label = ctx.block_label(cold_idx);
                                let merge_label = ctx.block_label(merge_idx);
                                {
                                    let blk = ctx.block();
                                    let new_bits = blk.bitcast_double_to_i64(&new);
                                    let finite = crate::expr::class_field_inline_guard::
                                        emit_plain_finite_number_check(blk, &new_bits);
                                    blk.cond_br(&finite, &store_label, &cold_label);
                                }
                                ctx.current_block = store_idx;
                                {
                                    // Reached only when the finite check above
                                    // proved `new`'s exponent is NOT all-ones;
                                    // every NaN-box tag (INT32/STRING/POINTER/
                                    // BIGINT) has an all-ones exponent.
                                    let blk = ctx.block();
                                    // GC_STORE_AUDIT(POINTER_FREE): a genuine
                                    // unboxed double by the proof above, never
                                    // a GC pointer — no edge, so no barrier.
                                    blk.store(DOUBLE, &new, &field_ptr);
                                    blk.br(&merge_label);
                                }
                                ctx.current_block = cold_idx;
                                {
                                    let key_idx = ctx.strings.intern(property);
                                    let key_handle_global =
                                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                    let blk = ctx.block();
                                    let key_box = blk.load(DOUBLE, &key_handle_global);
                                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                                    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                    blk.call_void(
                                        "js_object_set_field_by_name",
                                        &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &new)],
                                    );
                                    blk.br(&merge_label);
                                }
                                ctx.current_block = merge_idx;
                                return Ok(if *prefix { new } else { old });
                            }
                        }
                    }
                }
            }
            // #7628's scope note: the same read-modify-write skeleton as
            // `Expr::IndexUpdate`, and the same repair — with one extra shape
            // on top. `obj_handle` is a RAW `i64` derived from the receiver
            // before `js_object_get_field_by_name_f64` (a getter) and
            // `js_to_numeric` (a `valueOf`), and re-read from it afterwards by
            // `js_object_set_field_by_name`. That is the #7280 taxonomy's case
            // (a): a pointer already unboxed to raw `i64` cannot be repaired by
            // re-reading a slot, so the fix is to root the BOXED receiver and
            // re-derive the handle below the window, not to root the handle.
            //
            // `key_handle` is the same shape and needs no slot: it is derived
            // from a `__perry_init_strings_*` handle global, which is a
            // registered root the collector rewrites, and the literal is
            // immutable — so re-loading it below the window is `Reload`,
            // `operand_is_reloadable`'s exact argument, at two instructions.
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let field_read = Expr::PropertyGet {
                object: object.clone(),
                property: property.clone(),
                byte_offset: 0,
            };
            let result_may_be_heap =
                !crate::type_analysis::is_provably_not_bigint(ctx, &field_read);
            rooting::with_rooted_group(ctx, 1, |ctx, group| {
                let obj = group.lower(ctx, object, true)?;
                let derive_handles = |ctx: &mut FnCtx<'_>, obj_box: &str| {
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(obj_box);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    (obj_handle, key_handle)
                };
                let old = {
                    let obj_box = group.reread(ctx, obj)?;
                    let (obj_handle, key_handle) = derive_handles(ctx, &obj_box);
                    ctx.block().call(
                        DOUBLE,
                        "js_object_get_field_by_name_f64",
                        &[(I64, &obj_handle), (I64, &key_handle)],
                    )
                };
                // ToNumeric + Type(old)::add/sub(old, unit): a BigInt field stays a
                // BigInt (`var x = {y:0n}; ++x.y === 1n`), not the Number `1`. Mirrors
                // the identifier `Expr::Update` path. #4918 prefix/postfix bigint.
                let old_num = ctx.block().call(DOUBLE, "js_to_numeric", &[(DOUBLE, &old)]);
                let old_num_root =
                    group.adopt_emitted(ctx, Repr::Boxed, &old_num, result_may_be_heap && !*prefix);
                let step_arg = match op {
                    BinaryOp::Sub => "0",
                    _ => "1",
                };
                let new = ctx.block().call(
                    DOUBLE,
                    "js_numeric_step",
                    &[(DOUBLE, &old_num), (I32, step_arg)],
                );
                let new_root =
                    group.adopt_emitted(ctx, Repr::Boxed, &new, result_may_be_heap && *prefix);
                {
                    let obj_box = group.reread(ctx, obj)?;
                    let (obj_handle, key_handle) = derive_handles(ctx, &obj_box);
                    let new_arg = group.reread_emitted(ctx, new_root);
                    ctx.block().call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &new_arg)],
                    );
                }
                Ok(if *prefix {
                    group.reread_emitted(ctx, new_root)
                } else {
                    group.reread_emitted(ctx, old_num_root)
                })
            })
        }

        // -------- arr[idx]++ / arr[idx]-- / ++arr[idx] / --arr[idx] --------
        //
        // Issue #957: lodash's `countBy` uses `++result[key]` which previously
        // bailed `expression IndexUpdate not yet supported` and stubbed the
        // entire module, leaving `import _ from "lodash"` resolving to
        // undefined. Lower as a tag-aware read+modify+write through the
        // `js_dyn_index_get` / `js_dyn_index_set` runtime helpers — they
        // dispatch by gc_type at runtime, so the same emission works for
        // arrays, plain objects, and TypedArrays without static type
        // knowledge. `object` and `index` lower once into SSA registers so
        // side effects are not re-evaluated.
        Expr::IndexUpdate {
            object,
            index,
            op,
            prefix,
        } => {
            // #7628. The operand pair is consumed by FOUR calls with collection
            // points between them:
            //
            //   old     = js_dyn_index_get(obj, idx)   ; a getter / Proxy trap
            //   old_num = js_to_numeric(old)           ; a valueOf
            //   new     = js_numeric_step(old_num, s)  ; allocates only (#7198)
            //             js_dyn_index_set(obj, idx, new)  ; a setter / Proxy trap
            //
            // `with_operands_rooted` (#7615 slice 2) re-reads at exactly ONE
            // point, at the end of the operand list. `RootedGroup` (slice 6)
            // re-reads at any number of caller-chosen points, which is this
            // shape, so the operand pair is now re-read per use and no new
            // combinator arrives with this caller.
            //
            // ★ That half is belt-and-braces, NOT the repair — see the module
            // header. `root_reload` already rematerialises these slot loads at
            // each use; collapsing them back to one leaves the emitted IR the
            // same, measured.
            //
            // The repair is the RESULT, and it is not in the issue. For a
            // BigInt element `js_to_numeric` / `js_numeric_step` hand back a
            // heap `BigIntHeader`, and whichever of the two the expression
            // yields — `old_num` for postfix, `new` for prefix — is live across
            // `js_dyn_index_set`, i.e. across a user setter, as a bare call
            // result with no slot for `root_reload` to reload from. `protect`
            // is the element's own non-BigInt proof, so a typed-array `ta[i]++`
            // keeps the IR it had.
            let element_read = Expr::IndexGet {
                object: object.clone(),
                index: index.clone(),
            };
            let result_may_be_heap =
                !crate::type_analysis::is_provably_not_bigint(ctx, &element_read);
            rooting::with_rooted_group(ctx, 2, |ctx, group| {
                let obj = group.lower(ctx, object, true)?;
                let idx = group.lower(ctx, index, true)?;
                let old = {
                    let (obj_box, idx_box) = (group.reread(ctx, obj)?, group.reread(ctx, idx)?);
                    ctx.block().call(
                        DOUBLE,
                        "js_dyn_index_get",
                        &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
                    )
                };
                // ToNumeric + numeric step so a BigInt element stays BigInt
                // (`var x = [0n]; ++x[0] === 1n`). Mirrors the identifier Update +
                // PropertyUpdate paths. #4918 prefix/postfix bigint.
                let old_num = ctx.block().call(DOUBLE, "js_to_numeric", &[(DOUBLE, &old)]);
                let old_num_root =
                    group.adopt_emitted(ctx, Repr::Boxed, &old_num, result_may_be_heap && !*prefix);
                let step_arg = match op {
                    BinaryOp::Sub => "0",
                    _ => "1",
                };
                let new = ctx.block().call(
                    DOUBLE,
                    "js_numeric_step",
                    &[(DOUBLE, &old_num), (I32, step_arg)],
                );
                let new_root =
                    group.adopt_emitted(ctx, Repr::Boxed, &new, result_may_be_heap && *prefix);
                {
                    let (obj_box, idx_box) = (group.reread(ctx, obj)?, group.reread(ctx, idx)?);
                    let new_arg = group.reread_emitted(ctx, new_root);
                    ctx.block().call(
                        DOUBLE,
                        "js_dyn_index_set",
                        &[(DOUBLE, &obj_box), (DOUBLE, &idx_box), (DOUBLE, &new_arg)],
                    );
                }
                Ok(if *prefix {
                    group.reread_emitted(ctx, new_root)
                } else {
                    group.reread_emitted(ctx, old_num_root)
                })
            })
        }

        _ => unreachable!("expr/instance_misc1.rs dispatched a non-update variant here"),
    }
}
