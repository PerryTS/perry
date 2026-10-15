//! Object model step 5, case 2c (OBJECT_MODEL_2026-10-13.md §3.7): the read
//! of a coloured key through a receiver nothing proves.
//!
//! A coloured key `k` (`layout_families`) has one program-wide slot, and
//! every layout record carries a mask of the coloured keys its receivers hold
//! as inline data at their colour slot (perry-runtime `object/layout_probe.rs`
//! derives it from the record's immutable key list). The site is:
//!
//! ```text
//! tag test -> layout word -> record (one address computation) -> mask bit
//!          -> one load at the colour slot -> hole compare
//! ```
//!
//! * The layout word, shadow bit cleared, minus `SHAPE_ID_BASE` indexes the
//!   agent's flat record array (`shapes_store/flat.rs`): base and committed
//!   bound sit in the agent's shape directory, which the agent pointer block
//!   names. Any word that is not an ordinary-band layout id falls past the
//!   bound.
//! * The mask bit says the layout holds `k` at its colour slot as data; the
//!   slot's value is the property's value unless it is the hole marker.
//! * Everything else (a non-object, another layout, the bit clear, a hole)
//!   takes the site's ordinary path: the unknown-receiver probe, then the
//!   complete slow operation.
//!
//! No state is learned or validated: the word, the record and the mask are
//! the object's own data and immutable layout facts.

use crate::nanbox::TAG_HOLE_I64;
use crate::types::{DOUBLE, I32, I64, I8, PTR};

use super::FnCtx;

/// An emitted 2c arm: its hit value and the block it was produced in. The
/// current block, on return from [`begin`], is the miss path.
pub(crate) struct LayoutRead {
    hit_value: String,
    hit_label: String,
    merge_idx: usize,
}

/// The (mask bit, slot) when `property` is read through the 2c arm.
pub(crate) fn admitted(ctx: &FnCtx<'_>, property: &str) -> Option<(u32, u32)> {
    if property == "length" || property == "size" || property.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || ctx.target_triple.contains("wasm")
    {
        return None;
    }
    ctx.program_layouts.colored(property)
}

/// The record mask word of `bit` for the receiver whose biased payload is
/// `biased`, in the current block, branching to `miss_label` when the
/// receiver's layout word names no record. Leaves the current block at the
/// mask test's true edge and returns the loaded mask word's test result.
fn emit_mask_test(
    ctx: &mut FnCtx<'_>,
    biased: &str,
    bit: u32,
    miss_label: &str,
    mask_offset: usize,
) -> String {
    let blk = ctx.block();
    let word_ptr = crate::expr::receiver_range::emit_field_ptr(blk, biased, 4);
    let word = blk.load(I32, &word_ptr);
    let id = blk.and(
        I32,
        &word,
        &((!crate::runtime_abi::LAYOUT_SHADOW_BIT) as i32).to_string(),
    );
    let rel = blk.sub(
        I32,
        &id,
        &(crate::runtime_abi::SHAPE_ID_BASE as i32).to_string(),
    );
    let rel64 = blk.zext(I32, &rel, I64);
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        "@PERRY_EMPTY_SHAPE_DIR",
    );
    let blk = ctx.block();
    let base_ptr = blk.gep(
        I8,
        &dir,
        &[(I64, &crate::runtime_abi::SHAPE_DIR_FLAT_BASE.to_string())],
    );
    let base = blk.load(PTR, &base_ptr);
    let ids_ptr = blk.gep(
        I8,
        &dir,
        &[(
            I64,
            &(crate::runtime_abi::SHAPE_DIR_FLAT_BASE + 8).to_string(),
        )],
    );
    let ids = blk.load(I64, &ids_ptr);
    let inside = blk.icmp_ult(I64, &rel64, &ids);
    let rec_idx = ctx.new_block("l2c.record");
    let rec_label = ctx.block_label(rec_idx);
    ctx.block().cond_br(&inside, &rec_label, miss_label);
    ctx.current_block = rec_idx;
    let blk = ctx.block();
    let offset = blk.mul(
        I64,
        &rel64,
        &crate::runtime_abi::SHAPE_RECORD_SIZE.to_string(),
    );
    let rec = blk.gep(I8, &base, &[(I64, &offset)]);
    let mask_at = blk.gep(
        I8,
        &rec,
        &[(I64, &(mask_offset + 8 * (bit / 64) as usize).to_string())],
    );
    let mask_word = blk.load(I64, &mask_at);
    let bit_value = blk.and(I64, &mask_word, &((1u64 << (bit % 64)) as i64).to_string());
    blk.icmp_ne(I64, &bit_value, "0")
}

/// Emit the 2c arm for `receiver_bits.property` if the key is coloured.
/// On `Some`, the caller lowers its ordinary site in the current block (the
/// miss path) and passes its value to [`finish`].
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    property: &str,
) -> Option<LayoutRead> {
    let (bit, slot) = admitted(ctx, property)?;
    crate::expr::note_site("case2c");
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
    let word_idx = ctx.new_block("l2c.word");
    let load_idx = ctx.new_block("l2c.load");
    let hit_idx = ctx.new_block("l2c.hit");
    let miss_idx = ctx.new_block("l2c.miss");
    let merge_idx = ctx.new_block("l2c.merge");
    let word_label = ctx.block_label(word_idx);
    let load_label = ctx.block_label(load_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);

    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), receiver_bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    let holds = emit_mask_test(
        ctx,
        &recv.biased,
        bit,
        &miss_label,
        crate::runtime_abi::SHAPE_RECORD_COLOUR_MASK,
    );
    ctx.block().cond_br(&holds, &load_label, &miss_label);

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let value_ptr = crate::expr::receiver_range::emit_field_ptr(
        blk,
        &recv.biased,
        (header + 8 * slot as u64) as i64,
    );
    let value = blk.load(I64, &value_ptr);
    let hole = blk.icmp_eq(I64, &value, TAG_HOLE_I64);
    blk.cond_br(&hole, &miss_label, &hit_label);

    ctx.current_block = hit_idx;
    let merge_label = ctx.block_label(merge_idx);
    let blk = ctx.block();
    let hit_value = blk.bitcast_i64_to_double(&value);
    blk.br(&merge_label);
    let hit_label_now = ctx.block_label(ctx.current_block);

    ctx.current_block = miss_idx;
    Some(LayoutRead {
        hit_value,
        hit_label: hit_label_now,
        merge_idx,
    })
}

/// Join the 2c hit with the ordinary site's value `generic` (produced in the
/// current block).
pub(crate) fn finish(ctx: &mut FnCtx<'_>, arm: LayoutRead, generic: &str) -> String {
    let merge_label = ctx.block_label(arm.merge_idx);
    let generic_label = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = arm.merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (arm.hit_value.as_str(), arm.hit_label.as_str()),
            (generic, generic_label.as_str()),
        ],
    )
}

/// An emitted 2c store arm (see [`begin_store`]).
pub(crate) struct LayoutStore {
    hit_label: String,
    merge_idx: usize,
}

/// The store form of the 2c site for a coloured key: the receiver's layout
/// holds `property` at its colour slot as a plain writable `Any`-lane data
/// property of a store-admitted layout (the record's store mask), the object
/// carries no metadata record (no prototype role, no exotic receiver), and
/// the slot is present; then the assignment is one store plus the write
/// barrier's bookkeeping. On `Some`, the caller lowers its ordinary store
/// site in the current block (the miss path) and passes its value to
/// [`finish_store`].
pub(crate) fn begin_store(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    property: &str,
    value_double: &str,
    value_bits: &str,
    value_is_scalar: bool,
) -> Option<LayoutStore> {
    let (bit, slot) = admitted(ctx, property)?;
    crate::expr::note_site("case2c-store");
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
    let word_idx = ctx.new_block("l2s.word");
    let meta_idx = ctx.new_block("l2s.meta");
    let load_idx = ctx.new_block("l2s.load");
    let hit_idx = ctx.new_block("l2s.hit");
    let miss_idx = ctx.new_block("l2s.miss");
    let merge_idx = ctx.new_block("l2s.merge");
    let word_label = ctx.block_label(word_idx);
    let meta_label = ctx.block_label(meta_idx);
    let load_label = ctx.block_label(load_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), receiver_bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    let holds = emit_mask_test(
        ctx,
        &recv.biased,
        bit,
        &miss_label,
        crate::runtime_abi::SHAPE_RECORD_COLOUR_STORE_MASK,
    );
    ctx.block().cond_br(&holds, &meta_label, &miss_label);

    // `ObjectHeader.meta` is the header's last word.
    ctx.current_block = meta_idx;
    let blk = ctx.block();
    let meta_ptr =
        crate::expr::receiver_range::emit_field_ptr(blk, &recv.biased, (header - 8) as i64);
    let meta = blk.load(I64, &meta_ptr);
    let plain = blk.icmp_eq(I64, &meta, "0");
    blk.cond_br(&plain, &load_label, &miss_label);

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let slot_ptr = crate::expr::receiver_range::emit_field_ptr(
        blk,
        &recv.biased,
        (header + 8 * slot as u64) as i64,
    );
    let current = blk.load(I64, &slot_ptr);
    let hole = blk.icmp_eq(I64, &current, TAG_HOLE_I64);
    blk.cond_br(&hole, &miss_label, &hit_label);

    ctx.current_block = hit_idx;
    // GC_STORE_AUDIT(BARRIERED): the bookkeeping below classifies the stored
    // bits exactly as the store site's hit does.
    ctx.block().store(DOUBLE, value_double, &slot_ptr);
    if !value_is_scalar {
        let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &recv.biased);
        crate::expr::put_value_store_ic::emit_static_store_ic_bookkeeping(
            ctx,
            &handle,
            &slot_ptr,
            value_double,
            value_bits,
            "l2s",
        );
    }
    let hit_end = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = miss_idx;
    Some(LayoutStore {
        hit_label: hit_end,
        merge_idx,
    })
}

/// Join the 2c store hit (whose value is `value_double`) with the ordinary
/// site's result `generic` (produced in the current block).
pub(crate) fn finish_store(
    ctx: &mut FnCtx<'_>,
    arm: LayoutStore,
    value_double: &str,
    generic: &str,
) -> String {
    let merge_label = ctx.block_label(arm.merge_idx);
    let generic_label = ctx.block_label(ctx.current_block);
    ctx.block().br(&merge_label);
    ctx.current_block = arm.merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (value_double, arm.hit_label.as_str()),
            (generic, generic_label.as_str()),
        ],
    )
}
