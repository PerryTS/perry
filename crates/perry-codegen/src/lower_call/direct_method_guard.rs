//! The inline receiver guard of a compiled class-method site: the exact
//! birth-pair compare.
//!
//! Split out of `method_override.rs` for the 2000-line file-size gate. The
//! header constants and their anti-drift tests stay there.

use super::method_override::{
    GC_OBJECT_METHOD_GUARD_MASK_I32, GC_TYPE_OBJECT, LAYOUT_TAG_NEG_I32, ORDINARY_LAYOUT_RANGE_LEN,
    POINTER_TAG_HI16,
};
use crate::expr::FnCtx;
use crate::types::{I1, I32, I64, I8};

/// Emit the single-arm equivalent of the runtime `js_method_direct_shape_guard`
/// inline: the holder shape proof, a pointer gate, then the exact
/// `(class_id, ShapeId)` compare against the birth pair.
pub(crate) fn emit_inline_direct_method_shape_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    expected_class_id: &str,
    expected_shape_id: &str,
    holder_ok: &str,
    fast_label: &str,
    fallback_label: &str,
    // `true` at sites whose receiver may arrive in the internal raw-address
    // form (top word zero); `false` where the receiver is a user-visible
    // NaN-box, so that a plain double whose bit pattern happens to land in
    // the heap address range (a positive subnormal) can never reach the
    // header load — it misses to the runtime guard instead.
    accept_raw_ptr: bool,
    // `(family layout id, label)`: the receiver class's compiled births carry
    // that layout id as their ShapeId word (`new_alloc::class_born_on_family`).
    // A miss of the birth pair whose word is `(class_id, family id)` takes
    // `label`: the word proves the class and that no own key outside the
    // family's keys exists, which is all method identity needs. It proves
    // the birth pair's slot offsets only when the family holds the birth
    // layout ([`family_holds_birth_layout`]); then `label` is the fast arm,
    // else a call of the ordinary method body.
    family_arm: Option<(&str, &str)>,
) {
    let deref_idx = ctx.new_block("method_direct.inline_deref");
    let deref_label = ctx.block_label(deref_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();

    {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.lshr(I64, &recv_bits, "48");
        let is_tagged_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        // Internal method ABIs also carry an unboxed raw object address in a
        // double-sized slot. `normalize_raw_object_addr` accepts exactly this
        // top-word-zero form; all other non-pointer NaN-box tags remain
        // rejected before dereference.
        let is_ptr = if accept_raw_ptr {
            let is_raw_ptr = blk.icmp_eq(I64, &tag, "0");
            blk.or(I1, &is_tagged_ptr, &is_raw_ptr)
        } else {
            is_tagged_ptr
        };
        let above_floor = blk.icmp_uge(I64, &recv_handle, &heap_floor);
        let below_ceiling = blk.icmp_ult(I64, &recv_handle, &heap_ceiling);
        let in_heap_range = blk.and(I1, &above_floor, &below_ceiling);
        let ptr_safe = blk.and(I1, &is_ptr, &in_heap_range);
        let can_deref = blk.and(I1, holder_ok, &ptr_safe);
        blk.cond_br(&can_deref, &deref_label, fallback_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &recv_handle);

        let gc_header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gc_header = blk.load(I32, &gc_header_ptr);
        let guarded_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_METHOD_GUARD_MASK_I32);
        let gc_header_ok = blk.icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);

        // ObjectHeader begins with adjacent `class_id: u32` and
        // `shape_id: u32`. Compare them as one packed word. The expected
        // ShapeId is still range-checked below, so equality proves the live
        // receiver's class is non-zero and its ShapeId is in-domain without
        // four separate field predicates.
        let class_shape = blk.load(I64, &obj_ptr);
        let expected_shape_i64 = blk.zext(I32, expected_shape_id, I64);
        let expected_shape_high = blk.shl(I64, &expected_shape_i64, "32");
        let expected_class_shape = blk.or(I64, &expected_shape_high, expected_class_id);
        let class_shape_ok = crate::typed_shape::emit_compatible_class_shape_eq(
            blk,
            &class_shape,
            expected_class_id,
            expected_shape_id,
            &expected_class_shape,
            &[],
        );

        // `is_shape_id` is `[0x8000_0000, 0xC000_0000)`. Subtract the base
        // modulo i32 and compare with the range length, matching the runtime
        // helper. Equality above transfers this proof to the live header.
        let shape_id_rel = blk.add(I32, expected_shape_id, LAYOUT_TAG_NEG_I32);
        let shape_valid = blk.icmp_ult(I32, &shape_id_rel, ORDINARY_LAYOUT_RANGE_LEN);

        let pass = blk.and(I1, &gc_header_ok, &class_shape_ok);
        let pass = blk.and(I1, &pass, &shape_valid);
        let Some((family_id, family_label)) = family_arm else {
            blk.cond_br(&pass, fast_label, fallback_label);
            return;
        };
        let family_idx = ctx.new_block("method_direct.family");
        let family_test = ctx.block_label(family_idx);
        ctx.block().cond_br(&pass, fast_label, &family_test);
        ctx.current_block = family_idx;
        let blk = ctx.block();
        let family_i64 = blk.zext(I32, family_id, I64);
        let family_high = blk.shl(I64, &family_i64, "32");
        let expected_family_word = blk.or(I64, &family_high, expected_class_id);
        let family_ok = blk.icmp_eq(I64, &class_shape, &expected_family_word);
        // 0: the runtime could not register the layout (then no birth uses it).
        let registered = blk.icmp_ne(I32, family_id, "0");
        let family_ok = blk.and(I1, &family_ok, &registered);
        let family_ok = blk.and(I1, &family_ok, &gc_header_ok);
        blk.cond_br(&family_ok, family_label, fallback_label);
    }
}

/// The i32 family layout id instances of `class_name` carry when its
/// compiled or runtime births use the family layout,
/// loaded from the descriptor's `layout_id` word (written once at startup,
/// before user code). `None` when births use the birth pair, for static
/// methods, or when a family key spells `property` (then the word does not
/// rule out an own method).
pub(crate) fn family_method_word(
    ctx: &mut FnCtx<'_>,
    _class_name: &str,
    class_id: u32,
    property: &str,
    direct_fn: &str,
) -> Option<String> {
    if direct_fn.starts_with("perry_static_") {
        return None;
    }
    // Runtime construction also adopts a family's layout after validating
    // the registered parent. The exact receiver word is the proof, regardless
    // of which allocation path produced it.
    if !crate::layout_desc::layouts_enabled("executable", ctx.target_triple) {
        return None;
    }
    let fam = ctx.program_layouts.families.get(&class_id)?;
    if fam.keys.iter().any(|(k, _)| k == property) {
        return None;
    }
    let desc = crate::layout_desc::note_family_desc_use(class_id);
    let offset = crate::layout_desc::DESC_LAYOUT_ID_OFFSET.to_string();
    let blk = ctx.block();
    let id_ptr = blk.gep(I8, &desc, &[(I64, &offset)]);
    Some(blk.load(I32, &id_ptr))
}

/// Does class `class_name`'s family layout hold exactly the instance its
/// birth pair describes? Its keys are the class layout's keyable fields, in
/// the same order at the same slots, every one stored by the inert
/// constructor prologue (`FamilyLayout::init_count`, so present before any
/// code can observe the instance), and nothing else: no late key, no private
/// element. The exact family word then proves what the exact birth pair
/// proves to a body compiled for that pair (the proven-`this` clone's fixed slot
/// offsets, every key present, no other own key: an overflow sets the shadow
/// bit, so the full-word compare misses), and its arm may enter that body.
/// The family's slots are boxed: the clone's raw-`F64` reads load the bits a
/// boxed Number has too and keep their plain-finite check, and its raw stores
/// write only plain finite doubles.
pub(crate) fn family_holds_birth_layout(ctx: &FnCtx<'_>, class_name: &str, class_id: u32) -> bool {
    let Some(fam) = ctx.program_layouts.families.get(&class_id) else {
        return false;
    };
    let Some(chain) = ctx.class_init_chains.get(class_name) else {
        return false;
    };
    let mut slot = 0usize;
    for (_, fields) in chain {
        for f in fields {
            if f.key_expr.is_some() || f.is_private {
                return false;
            }
            match fam.keys.get(slot) {
                Some((k, s)) if k.name() == Some(f.name.as_str()) && *s as usize == slot => {}
                _ => return false,
            }
            slot += 1;
        }
    }
    slot == fam.keys.len() && slot <= fam.init_count as usize
}

/// The exact family word `class_id | family layout id << 32` a certificate
/// arm admits beside the birth pair, when the family holds the birth layout
/// ([`family_holds_birth_layout`]) and [`family_method_word`] gives the id.
/// An unregistered layout (id 0) never matches: the method probe's word is 0
/// for a zero ShapeId.
pub(super) fn family_clone_word(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    class_id: u32,
    property: &str,
    direct_fn: &str,
) -> Option<String> {
    if !family_holds_birth_layout(ctx, class_name, class_id) {
        return None;
    }
    let id = family_method_word(ctx, class_name, class_id, property, direct_fn)?;
    let blk = ctx.block();
    let high = blk.zext(I32, &id, I64);
    let high = blk.shl(I64, &high, "32");
    Some(blk.or(I64, &high, &class_id.to_string()))
}

/// A declared instance field is an own property on every constructed object,
/// so it wins over a same-named prototype method. The direct-method guards
/// prove the receiver's class/shape and prototype stability, but that is not
/// enough to skip ordinary own-property lookup when the expected shape itself
/// contains the method name. Computed fields are conservatively treated as a
/// possible shadow because their runtime key is not available here.
pub(super) fn class_chain_may_declare_method_field(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    let mut seen = std::collections::HashSet::new();
    for _ in 0..64 {
        let Some(name) = current else {
            return false;
        };
        if !seen.insert(name.clone()) {
            return true;
        }
        let Some(class) = ctx.classes.get(&name).copied() else {
            return true;
        };
        if class
            .fields
            .iter()
            .any(|field| field.key_expr.is_some() || (!field.is_private && field.name == property))
        {
            return true;
        }
        current = class.extends_name.clone();
    }
    true
}
