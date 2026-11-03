//! Object model step 1 (OBJECT_MODEL_2026-10-13.md §3.2/§3.3): a literal
//! family's added key at a receiver codegen ties to the family.
//!
//! The family is born with a slot for the first key it adds after birth
//! (`layout_desc::FamilyAdd`), so adding the key is a store, not a shape
//! transition: the store writes the slot and the layout word of the family
//! state that holds the key (`desc.layout_id`, registered at startup). A read
//! of the key is one load from the same slot.
//!
//! Both compare the receiver's layout word against the two family states
//! first, constants of the binary rather than anything learned: a family
//! object leaves them only through a path this site does not control (a
//! field representation downgrade, a delete, an integrity change), and then
//! the site takes the ordinary `[[Set]]`/`[[Get]]`. That compare is also
//! what tells a read whether the key is present yet (§3.2's presence test;
//! under coexistence presence is the layout word, not a hole in the slot).

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::layout_desc::FamilyAdd;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, I8};

/// A family-add access: the add and the receiver's family class.
pub(crate) struct Site {
    pub add: FamilyAdd,
    pub class_name: String,
    pub proof: Option<std::sync::Arc<crate::collectors::FamilyProof>>,
}

/// The family add `object.property` names, when `object` is a `Ptr<Shape>`
/// local of a literal family whose first added key is `property`.
pub(crate) fn site(ctx: &FnCtx<'_>, object: &Expr, property: &str, store: bool) -> Option<Site> {
    if !matches!(object, Expr::LocalGet(_)) {
        return None;
    }
    let fact = if store {
        ctx.ptr_shape_store_fact(object)
    } else {
        ctx.ptr_shape_receiver_fact(object)
    }?;
    let add = crate::layout_desc::family_add(&fact.class_name, property)?;
    Some(Site {
        add,
        class_name: fact.class_name.clone(),
        proof: fact.family.clone(),
    })
}

/// `(birth id, state id)` operands and the receiver's layout word; the
/// returned i1 is "the receiver is in one of the two family states" (for a
/// store) or "the receiver holds the key" (for a read).
fn layout_test(
    ctx: &mut FnCtx<'_>,
    site: &Site,
    obj_ptr: &str,
    store: bool,
) -> Option<(String, String)> {
    let add = &site.add;
    let class_name = site.class_name.as_str();
    let keys_global = ctx.class_keys_globals.get(class_name)?.clone();
    let birth = if store {
        Some(crate::typed_shape::class_shape_id_operand(
            ctx,
            class_name,
            &keys_global,
        ))
    } else {
        None
    };
    let state = state_id(ctx, add);
    let blk = ctx.block();
    let word_ptr = blk.gep(I8, obj_ptr, &[(I64, "4")]);
    let word = blk.load(I32, &word_ptr);
    // 0: the runtime could not register the state; never take the slot.
    let registered = blk.icmp_ne(I32, &state, "0");
    let is_state = blk.icmp_eq(I32, &word, &state);
    let ok = match birth {
        Some(birth) => {
            let is_birth = blk.icmp_eq(I32, &word, &birth);
            blk.or(crate::types::I1, &is_state, &is_birth)
        }
        None => is_state,
    };
    let ok = blk.and(crate::types::I1, &registered, &ok);
    Some((ok, state))
}

/// The family state's layout id, loaded once at function entry (written
/// once at startup, before any user code).
fn state_id(ctx: &mut FnCtx<'_>, add: &FamilyAdd) -> String {
    let slot = ctx
        .func
        .entry_init_load_global(&crate::layout_desc::layout_id_global(&add.desc_global), I32);
    ctx.block().load(I32, &slot)
}

/// Whether the read `expr` sees the key present by construction.
pub(crate) fn present(site: &Site, expr: &Expr) -> bool {
    site.proof
        .as_ref()
        .is_some_and(|p| p.exact && p.present_reads.contains(&(expr as *const Expr as usize)))
}

/// Whether the read `expr` is a Number by construction: present, and every
/// store of the key on this local writes a Number.
pub(crate) fn present_number(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return false;
    };
    site(ctx, object, property, false)
        .is_some_and(|s| present(&s, expr) && s.proof.as_ref().is_some_and(|p| p.numeric))
}

fn key_box(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let key_idx = ctx.strings.intern(property);
    let handle = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    ctx.block().load(DOUBLE, &handle)
}

fn property_id(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let key_idx = ctx.strings.intern(property);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global)
}

/// `object.property = value` for a family add. Returns the stored value.
pub(crate) fn lower_set(
    ctx: &mut FnCtx<'_>,
    site: &Site,
    object: &Expr,
    property: &str,
    value: &Expr,
    strict: bool,
) -> Result<String> {
    let barrier_needed =
        !crate::expr::helpers::expr_produces_non_pointer_bits_by_construction(ctx, value);
    let addref_needed = crate::expr::helpers::class_field_store_needs_string_addref(ctx, value);
    crate::rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (vals[0].clone(), vals[1].clone());
        let (obj_bits, obj_handle, obj_ptr) = {
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            (obj_bits, obj_handle, obj_ptr)
        };
        let add = &site.add;
        if site.proof.as_ref().is_some_and(|p| p.exact) {
            // The object carries the birth state or this one: the store is
            // the slot and the state word, nothing to compare.
            let state = state_id(ctx, add);
            let header =
                crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
            let offset = (header + 8 * u64::from(add.slot)).to_string();
            let (slot_ptr, slot_addr) = {
                let blk = ctx.block();
                let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
                let slot_addr = blk.ptrtoint(&slot_ptr, I64);
                (slot_ptr, slot_addr)
            };
            crate::expr::write_barrier::emit_jsvalue_slot_store_pointer_tested(
                ctx,
                &slot_ptr,
                &val_double,
                &obj_handle,
                addref_needed,
                &obj_bits,
                &slot_addr,
                barrier_needed,
                "family_set",
            );
            let blk = ctx.block();
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            // GC_STORE_AUDIT(POINTER_FREE): the layout word is a scalar.
            blk.store(I32, &state, &word_ptr);
            return Ok(val_double);
        }
        let Some((ok, state)) = layout_test(ctx, site, &obj_ptr, true) else {
            anyhow::bail!("family slot store without a Ptr<Shape> class");
        };
        let fast_idx = ctx.new_block("family_set.slot");
        let slow_idx = ctx.new_block("family_set.generic");
        let merge_idx = ctx.new_block("family_set.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&ok, &fast_label, &slow_label);

        ctx.current_block = fast_idx;
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
        let offset = (header + 8 * u64::from(add.slot)).to_string();
        let (slot_ptr, slot_addr) = {
            let blk = ctx.block();
            let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
            let slot_addr = blk.ptrtoint(&slot_ptr, I64);
            (slot_ptr, slot_addr)
        };
        crate::expr::write_barrier::emit_jsvalue_slot_store_pointer_tested(
            ctx,
            &slot_ptr,
            &val_double,
            &obj_handle,
            addref_needed,
            &obj_bits,
            &slot_addr,
            barrier_needed,
            "family_set",
        );
        {
            let blk = ctx.block();
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            // GC_STORE_AUDIT(POINTER_FREE): the layout word is a scalar.
            blk.store(I32, &state, &word_ptr);
            blk.br(&merge_label);
        }
        let fast_end = ctx.block_label(ctx.current_block);

        ctx.current_block = slow_idx;
        let key = key_box(ctx, property);
        let slow_val = ctx.block().call(
            DOUBLE,
            "js_put_value_set",
            &[
                (DOUBLE, &recv_box),
                (DOUBLE, &key),
                (DOUBLE, &val_double),
                (DOUBLE, &recv_box),
                (I32, if strict { "1" } else { "0" }),
            ],
        );
        ctx.block().br(&merge_label);
        let slow_end = ctx.block_label(ctx.current_block);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&val_double, &fast_end), (&slow_val, &slow_end)]))
    })
}

/// `object.property` for a family add.
pub(crate) fn lower_get(
    ctx: &mut FnCtx<'_>,
    site: &Site,
    expr: &Expr,
    object: &Expr,
    property: &str,
) -> Result<String> {
    let add = &site.add;
    crate::expr::note_site("case2-family");
    let recv_box = lower_expr(ctx, object)?;
    let (obj_handle, obj_ptr) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &obj_handle);
        (obj_handle, obj_ptr)
    };
    if present(site, expr) {
        // A store of the key dominates this read on an exact family
        // object: the key is present, one load.
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
        let offset = (header + 8 * u64::from(add.slot)).to_string();
        let blk = ctx.block();
        let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
        return Ok(blk.load(DOUBLE, &slot_ptr));
    }
    let Some((present, _)) = layout_test(ctx, site, &obj_ptr, false) else {
        anyhow::bail!("family slot read without a Ptr<Shape> class");
    };
    let fast_idx = ctx.new_block("family_get.slot");
    let slow_idx = ctx.new_block("family_get.generic");
    let merge_idx = ctx.new_block("family_get.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&present, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
    let offset = (header + 8 * u64::from(add.slot)).to_string();
    let fast_val = {
        let blk = ctx.block();
        let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
        let v = blk.load(DOUBLE, &slot_ptr);
        blk.br(&merge_label);
        v
    };
    let fast_end = ctx.block_label(ctx.current_block);

    ctx.current_block = slow_idx;
    let id = property_id(ctx, property);
    let slow_val = {
        let blk = ctx.block();
        let v = blk.call(
            DOUBLE,
            "js_object_get_field_by_property_id_f64",
            &[(I64, &obj_handle), (I64, &id)],
        );
        blk.br(&merge_label);
        v
    };
    let slow_end = ctx.block_label(ctx.current_block);

    ctx.current_block = merge_idx;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&fast_val, &fast_end), (&slow_val, &slow_end)]))
}
