//! Ordinary property-get dispatch: static family slots, coloured layout
//! slots, then the receiver layout's probe and complete property semantics.
//! A fixed-name read of a name some getter of the program is declared under
//! owns bounded shape/slot answers: its leaf checks them only after an own
//! miss, and its collecting exit derives them. Every other read keeps the
//! stateless probe. Static family and colour slots retain their direct
//! layout checks.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// A SPILL-located key publishes its ShapeId into the compact word with this
/// bit flipped. **Must equal `PACKED_SPILL_FLIP` in the runtime.**
///
/// ShapeIds live in [`0x8000_0000`, `0xC000_0000`), so flipping the top two
/// bits maps them into [`0x4000_0000`, `0x8000_0000`) — the one u32 band that
/// is neither a ShapeId nor any class id. (Flipping bit 30 alone would land
/// them in [`0xC000_0000`, `2^32`), which #10824 turns into the synthetic
/// class-id range.)
/// The hit path's compare therefore REFUSES a spill entry without asking a
/// question of its own, which is what lets the overflow-bit test (a 10-byte
/// `movabs`, a `test` and a branch, on every read of every site) leave the hit
/// path entirely. The spill entry is still served: the site's one miss call
/// (`js_object_get_field_ic_slow`) un-flips the bit first thing. Codegen no
/// longer reads the word's encoding; the mirror is kept for the pairing tests.
#[cfg(test)]
pub(crate) const PACKED_SPILL_FLIP: i64 = 0xC000_0000;

// Word 2 is unused: it held the Array-subclass named-prefix token, site state
// not derived from one shape, retired by S6. A site holds `(ShapeId, slot)`
// pairs only.

/// Materialise the pooled property-key `StringHeader*` in the CURRENT block.
///
/// Every consumer of the key — `js_object_get_field_ic_miss`, the two
/// `js_object_get_field_by_name_f64` arms, the class-ref helper — sits on a
/// COLD edge of the dispatch diamond, but the load used to be emitted once up
/// front, in the entry block, so the hit path of every generic property read
/// paid a dependent load of a global it never used. Emitting it per consumer
/// duplicates dead-cheap code into blocks that are already making a call, and
/// takes the load off the fast path entirely.
///
/// The pool entry is a *mutable* global — GC evacuation rewrites it — so
/// re-reading it at each consumer is not merely cheap, it is the correct
/// reading: every cold block sees the pool's current address rather than one
/// captured before whatever collected.
/// The runtime's never-written empty shape directory (`shapes_store.rs`),
/// which confirms nothing: a `length` site's front operand, and the value
/// where the agent's own directory is not readable inline.
const EMPTY_SHAPE_DIR: &str = "@PERRY_EMPTY_SHAPE_DIR";

fn emit_key_handle(ctx: &mut FnCtx<'_>, key_handle_global: &str) -> String {
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    blk.and(I64, &key_bits, POINTER_MASK_I64)
}

/// The receiver's handle (its 48-bit payload), materialised in the CURRENT
/// block: re-derived from the fused receiver test's biased value for every key
/// but `.length`, or the entry-block mask for `.length`. Only ever called on
/// an edge the receiver test's pointer branch dominates.
fn recv_handle(
    ctx: &mut FnCtx<'_>,
    fused_recv: Option<&crate::expr::receiver_range::FusedReceiver>,
    entry_handle: &str,
) -> String {
    match fused_recv {
        Some(f) => crate::expr::receiver_range::emit_handle(ctx.block(), &f.biased),
        None => entry_handle.to_string(),
    }
}

/// `GcHeader.obj_type` of a plain Array (`perry_runtime::gc::GC_TYPE_ARRAY`).
const GC_TYPE_ARRAY_I8: &str = "1";
/// `perry_runtime::gc::GC_FLAG_FORWARDED` (0x80), spelled as a signed `i8`.
const GC_FLAG_FORWARDED_I8: &str = "-128";

/// `obj_type == GC_TYPE_ARRAY` and `GC_FLAG_FORWARDED` clear, read from the
/// header in front of `handle`, emitted into the current block. Returns
/// `(is_array, live_array)`. The caller must already have proved `handle` is a
/// heap address above the handle band.
///
/// The two byte loads share one flat predicate: the flags byte sits next to
/// the kind byte, so loading it for a receiver that turns out not to be an
/// array costs one load from a line already in cache, and saves a branch on
/// every array read.
fn emit_array_header_test(ctx: &mut FnCtx<'_>, handle: &str) -> (String, String) {
    let blk = ctx.block();
    // `GcHeader` is `obj_type: u8` then `gc_flags: u8`, at offsets 0 and 1 of
    // the header on every target, so these are byte loads whatever the order.
    let type_addr = blk.sub(I64, handle, "8");
    let type_ptr = blk.inttoptr(I64, &type_addr);
    let gc_type = blk.load(I8, &type_ptr);
    let is_array = blk.icmp_eq(I8, &gc_type, GC_TYPE_ARRAY_I8);
    let flags_addr = blk.sub(I64, handle, "7");
    let flags_ptr = blk.inttoptr(I64, &flags_addr);
    let flags = blk.load(I8, &flags_ptr);
    let forwarded = blk.and(I8, &flags, GC_FLAG_FORWARDED_I8);
    let not_forwarded = blk.icmp_eq(I8, &forwarded, "0");
    let live_array = blk.and(I1, &is_array, &not_forwarded);
    (is_array, live_array)
}

/// `.length` on a receiver whose GC header says it is a plain Array (#10714),
/// answered inline. Emitted starting in the CURRENT block; everything that is
/// not a live plain Array, and any forwarding stub this does not heal (see
/// below), branches to `other_label`. Returns the length and the block that
/// carries it to `merge_label`, for the merge phi. The caller has already
/// proved `handle` is a POINTER-tagged address above the handle band.
///
/// Without this, no dynamically typed `arr.length` could be served inline: the
/// PIC requires a `GC_TYPE_OBJECT` receiver by construction (#72), so every
/// such read called out to `get_field_ic_miss_impl`, walked the ladder above
/// its array arm, and cached nothing because there is nothing to cache. On a
/// natively compiled `tsc --noEmit` that was 1.7M of 6.3M misses (27%), and
/// the three hottest miss sites in the run.
///
/// The answer is the one the runtime arm computes. A non-forwarded
/// `GC_TYPE_ARRAY`'s `length` is the `u32` at payload offset 0, sparse arrays
/// included — `js_array_length` and `clean_arr_ptr` both return that word for
/// it. Buffers, typed arrays, lazy arrays, Maps and Sets all carry their own
/// `obj_type`, and an Array subclass instance is a `GC_TYPE_OBJECT` whose
/// `length` lives in its store or its shape, so none of them can take this
/// arm. A primitive array's `length` is non-configurable, so an own property
/// cannot shadow it. This is the header test the proven Array lowerings
/// already inline (`plen.check_gc` in `property_get.rs`,
/// `kindguard.array_header`, `apop.hdr`).
///
/// # Forwarding stubs
///
/// An array that grows past its capacity moves, and its old head becomes a
/// stub: `GC_FLAG_FORWARDED` set and the live head's address in the first
/// payload word, where `length` was. Every binding the growing code wrote
/// through is re-pointed, but one it did not — a field holding the array
/// while `obj.items.push(x)` grew it, an alias — keeps the stub until a
/// collection heals it, and a loop that allocates nothing never collects. On
/// the natively compiled `tsc`, a plain-array test alone still left 477,202
/// `array_length` misses to the handler, 380,278 of them at one site. With
/// `follow_stub`, a stub follows ONE edge here, exactly as
/// `index_get/guarded_array.rs` does: the forwarding word is trusted only once
/// it is a heap address above the handle band, and the destination is
/// re-checked as a live plain Array before its `length` is read; that left
/// 1,387. A longer or malformed chain takes `other_label`, where
/// `clean_arr_ptr` follows the whole chain and compresses it to one edge, so
/// the next read heals here. All of this is on the refusal edge: a live plain
/// Array's read is the header test and the load, nothing more.
///
/// The inline tower follows stubs; the full-outline site does not. That mode
/// exists to keep each site small (#5391), and the three extra blocks cost it
/// more than they saved: on a 64M-read loop whose receivers are all live
/// arrays, the full-outline arm with the follow ran at ~1450 ms against
/// ~1040 ms without it (the emitted hot path is the same instructions; the
/// larger CFG changed the loop's register allocation). A stub there takes the
/// helper call, as every read did before this arm.
///
/// The load is a plain one: `push`/`pop`/`length =` all rewrite this word, so
/// it must not be marked invariant, and the handle is already known to be
/// above the handle band, so `safe_load_i32_from_ptr`'s sub-page select would
/// guard nothing.
fn emit_plain_array_length_arm(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    other_label: &str,
    merge_label: &str,
    follow_stub: bool,
) -> (String, String) {
    let load_idx = ctx.new_block("pget.array_length");
    let load_label = ctx.block_label(load_idx);
    let (is_array, live_array) = emit_array_header_test(ctx, handle);
    let direct_label = ctx.block().label.clone();

    let healed = if follow_stub {
        let stub_idx = ctx.new_block("pget.array_stub");
        let follow_idx = ctx.new_block("pget.array_stub_follow");
        let target_idx = ctx.new_block("pget.array_stub_target");
        let stub_label = ctx.block_label(stub_idx);
        let follow_label = ctx.block_label(follow_idx);
        let target_label = ctx.block_label(target_idx);
        ctx.block().cond_br(&live_array, &load_label, &stub_label);

        // Refused: an Array here is necessarily a forwarding stub (the live
        // test failed on the flag); anything else leaves.
        ctx.current_block = stub_idx;
        ctx.block().cond_br(&is_array, &follow_label, other_label);

        ctx.current_block = follow_idx;
        let target = {
            let blk = ctx.block();
            let stub_ptr = blk.inttoptr(I64, handle);
            let target = blk.load(I64, &stub_ptr);
            let top = blk.lshr(I64, &target, "48");
            let top_clear = blk.icmp_eq(I64, &top, "0");
            let above_band = blk.icmp_ugt(
                I64,
                &target,
                crate::expr::receiver_range::HANDLE_BAND_TOP_LITERAL,
            );
            let heap_candidate = blk.and(I1, &top_clear, &above_band);
            blk.cond_br(&heap_candidate, &target_label, other_label);
            target
        };

        ctx.current_block = target_idx;
        let (_, target_live) = emit_array_header_test(ctx, &target);
        let healed_label = ctx.block().label.clone();
        ctx.block().cond_br(&target_live, &load_label, other_label);
        Some((target, healed_label))
    } else {
        ctx.block().cond_br(&live_array, &load_label, other_label);
        None
    };

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let live = match healed.as_ref() {
        Some((target, healed_label)) => blk.phi(
            I64,
            &[
                (handle, direct_label.as_str()),
                (target.as_str(), healed_label.as_str()),
            ],
        ),
        None => handle.to_string(),
    };
    let len_ptr = blk.inttoptr(I64, &live);
    let len_i32 = blk.load(I32, &len_ptr);
    let len = blk.uitofp(I32, &len_i32, DOUBLE);
    let end_label = blk.label.clone();
    blk.br(merge_label);
    (len, end_label)
}

/// The outlined read of a named key: the unknown-receiver probe (which tests
/// the receiver itself), then the complete read entry on a decline.
pub(crate) fn probe_then_value(
    ctx: &mut FnCtx<'_>,
    obj_bits: &str,
    key_handle: &str,
    key_handle_global: &str,
    property: &str,
    site_id: &str,
) -> String {
    let entry_args = [(I64, obj_bits), (I64, key_handle), (I64, site_id)];
    if property.is_empty() || property == "length" || property.bytes().all(|b| b.is_ascii_digit()) {
        return ctx
            .block()
            .call(DOUBLE, "js_object_get_field_value", &entry_args);
    }
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        EMPTY_SHAPE_DIR,
    );
    let key_box = ctx.block().load(DOUBLE, key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let hash = (crate::layout_desc::atom_hash(property) as i64).to_string();
    crate::expr::note_site("probe-outlined");
    crate::expr::ic_fast_split::emit_hole_declining_split(
        ctx,
        "pget.outline",
        "perry_probe_get_value",
        &[(PTR, &dir), (I64, obj_bits), (I64, &key_bits), (I64, &hash)],
        "js_object_get_field_value",
        &entry_args,
    )
}

/// The ordinary layout-probe dispatch for `obj.property`, after earlier
/// specializations have been ruled out.
pub(crate) fn lower_generic_property_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
) -> Result<String> {
    lower_property_site(ctx, object, property, byte_offset, None)
}

/// A declared class field read whose receiver's shape the class layout
/// predicts (`class_field_predicted_shape`). The site is the property site
/// with its word replaced by that ShapeId: the receiver test, the receiver's
/// ShapeId against the class's, one load of the layout's slot. Everything
/// else, any receiver not on that shape included, takes the layout probe and
/// complete property read. Inside a class-guard run (`class_guard_memo`) a read of a
/// receiver whose proof an earlier site of the run set tests the run's flag
/// first.
pub(crate) fn lower_class_field_property_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
    predicted: crate::expr::class_field_inline_guard::PredictedShape,
) -> Result<String> {
    if ctx.fixed_generator_state
        || crate::codegen::full_outline_ic_enabled()
        || crate::expr::typed_feedback_emission_enabled()
        || property == "length"
    {
        return lower_generic_property_get(ctx, object, property, byte_offset);
    }
    crate::expr::note_site("case2-class");
    let obj_box = lower_expr(ctx, object)?;
    let family = crate::expr::class_family_read::begin(ctx, object, property, &obj_box);
    let value =
        lower_predicted_class_field_get(ctx, object, property, byte_offset, predicted, obj_box)?;
    Ok(match family {
        Some(arm) => crate::expr::class_family_read::finish(ctx, arm, &value),
        None => value,
    })
}

fn lower_predicted_class_field_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
    predicted: crate::expr::class_field_inline_guard::PredictedShape,
    obj_box: String,
) -> Result<String> {
    // Same location record as the generic site (see `lower_property_site`).
    crate::expr::calls::emit_call_location_at(ctx, byte_offset);
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
    let token_idx = ctx.new_block("pget.class.token");
    let hit_idx = ctx.new_block("pget.class.hit");
    let miss_idx = ctx.new_block("pget.class.miss");
    let merge_idx = ctx.new_block("pget.class.merge");
    let token_label = ctx.block_label(token_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    let memo = crate::expr::class_guard_memo::memo_for(ctx, object);
    let recv = if memo
        .as_ref()
        .and_then(|memo| memo.handle.as_ref())
        .is_none()
    {
        Some(crate::expr::receiver_range::emit_fused_receiver_test(
            ctx.block(),
            &obj_bits,
        ))
    } else {
        None
    };
    if let Some(memo) = memo.as_ref().filter(|memo| memo.reuse) {
        let site_idx = ctx.new_block("pget.class.memo_clear");
        let site_label = ctx.block_label(site_idx);
        let flag = ctx.block().load(I8, &memo.flag);
        let proven = ctx.block().icmp_ne(I8, &flag, "0");
        ctx.block().cond_br(&proven, &hit_label, &site_label);
        ctx.current_block = site_idx;
    }
    let recv = recv.unwrap_or_else(|| {
        memo.as_ref()
            .map(|memo| memo.receiver_test(ctx.block(), &obj_bits))
            .unwrap_or_else(|| {
                crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits)
            })
    });
    ctx.block()
        .cond_br(&recv.is_object_pointer, &token_label, &miss_label);

    ctx.current_block = token_idx;
    crate::expr::receiver_range::emit_route_note(
        ctx.block(),
        crate::expr::receiver_range::Route::ClassRead,
    );
    let sid_ptr = crate::expr::receiver_range::emit_field_ptr(ctx.block(), &recv.biased, 4);
    let sid = ctx.block().load(I32, &sid_ptr);
    // The static immediate, else a volatile load of the shape global (as
    // every class guard reads it: LLVM must not forward it into a cold block
    // and keep it live across the hit path).
    let expected = crate::typed_shape::class_shape_id_operand_on_block(
        ctx.block(),
        &predicted.keys_global,
        true,
    );
    let proven = crate::typed_shape::emit_compatible_shape_eq(ctx.block(), &sid, &expected, &[]);
    if let Some(memo) = memo.as_ref().filter(|memo| !memo.reuse) {
        let set_idx = ctx.new_block("pget.class.memo_set");
        let set_label = ctx.block_label(set_idx);
        ctx.block().cond_br(&proven, &set_label, &miss_label);
        ctx.current_block = set_idx;
        ctx.block().store(I8, "1", &memo.flag);
        ctx.block().br(&hit_label);
    } else {
        ctx.block().cond_br(&proven, &hit_label, &miss_label);
    }

    ctx.current_block = hit_idx;
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, predicted.form()) as i64;
    let offset = header + i64::from(predicted.slot) * 8;
    let slot_ptr = memo
        .as_ref()
        .and_then(|memo| memo.field_ptr(ctx.block(), offset))
        .unwrap_or_else(|| {
            crate::expr::receiver_range::emit_field_ptr(ctx.block(), &recv.biased, offset)
        });
    let val_hit = ctx.block().load(DOUBLE, &slot_ptr);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::PropertyGet,
        property,
        TypedFeedbackContract::object_get_by_name(),
    );
    let key_handle = emit_key_handle(ctx, &key_handle_global);
    let val_miss = probe_then_value(
        ctx,
        &obj_bits,
        &key_handle,
        &key_handle_global,
        property,
        &feedback_site_id,
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&val_hit, &hit_end), (&val_miss, &miss_end)]))
}

/// Global environment references share the ordinary site's shape-validated
/// slot loads. On a miss, resolve the binding before evaluating its getter.
pub(crate) fn lower_global_property_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    required: bool,
) -> Result<String> {
    lower_property_site(ctx, object, property, 0, Some(required))
}

fn lower_property_site(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
    global_reference: Option<bool>,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    // #5247: record this access's source location right after the receiver is
    // evaluated and before the nullish-receiver throw path (the inline diamond
    // OR the outlined property helper — both throw "Cannot
    // read properties of null/undefined"). No-op unless compiled with
    // `--debug-symbols` (the offset resolves to `None` without the debug
    // context) or when `byte_offset` is 0 (a synthesized node). Emitted after
    // the receiver so a nested `a.b.c` chain keeps the inner `.b` access's more
    // specific location when *it* is the throwing read.
    crate::expr::calls::emit_call_location_at(ctx, byte_offset);
    // #7640 section E audit: this helper lowers only `object`; the property
    // name is compile-time data. The optional debug-location call above only
    // updates TLS (`js_set_call_location`) and cannot allocate or collect, so
    // there is no second user-expression window requiring an operand group.
    // Object model step 5 (case 2c): a coloured key's read through any
    // receiver tests the receiver layout's colour bit and loads the colour
    // slot (`expr/layout_read.rs`); everything else, and every miss, is the
    // site below.
    let family = global_reference
        .is_none()
        .then(|| crate::expr::class_family_read::begin(ctx, object, property, &obj_box))
        .flatten();
    let arm = if global_reference.is_none() {
        let bits = ctx.block().bitcast_double_to_i64(&obj_box);
        crate::expr::layout_read::begin(ctx, &bits, property)
    } else {
        None
    };
    let _decision = arm.as_ref().map(|_| {
        crate::fallback_diag::decision(
            "receiver layout has no admitted colour/presence slot at runtime",
            "layout-word/colour-mask/presence check",
        )
    });
    let value = lower_property_site_tail(ctx, property, global_reference, obj_box)?;
    let value = match arm {
        Some(arm) => crate::expr::layout_read::finish(ctx, arm, &value),
        None => value,
    };
    Ok(match family {
        Some(arm) => crate::expr::class_family_read::finish(ctx, arm, &value),
        None => value,
    })
}

/// [`lower_property_site`] after the receiver is evaluated: the ordinary site.
fn lower_property_site_tail(
    ctx: &mut FnCtx<'_>,
    property: &str,
    global_reference: Option<bool>,
    obj_box: String,
) -> Result<String> {
    let global_binding = global_reference == Some(true);
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
    // The receiver test and the handle. For every key but `.length` the tag
    // test and the small-handle test are ONE unsigned range compare
    // (`crate::expr::receiver_range`): `t = bits - (POINTER_TAG | 0x10_0000)`,
    // `t <u 2^48 - 0x10_0000` is exactly "POINTER tag and a payload above the
    // native-handle band". The two separate tests cost six instructions on
    // every hit (`xor`, `mov`, `shr $48`, `jne`, `cmp $0xfffff`, `jbe`). The
    // handle is `t + 0x10_0000`, which equals `bits ^ POINTER_TAG` on the
    // pointer edge; the two hot loads below address from `t` directly, so the
    // `+ 0x10_0000` folds into their displacements, and every COLD consumer
    // re-derives the handle in its own block (`recv_handle`). Materialised
    // once up front, the handle is live into five cold blocks, so LLVM keeps
    // it in the entry block and every HIT pays its `lea` (measured on
    // `realsite`). `.length` keeps the mask and its own test: that test admits
    // STRING-tagged receivers too (the heap-string arm below), which no single
    // range over POINTER can; its handle is the entry-block mask.
    let fused_recv = (property != "length")
        .then(|| crate::expr::receiver_range::emit_fused_receiver_test(blk, &obj_bits));
    let entry_handle = match fused_recv.as_ref() {
        None => blk.and(I64, &obj_bits, POINTER_MASK_I64),
        Some(_) => String::new(),
    };
    // The key handle is materialised per consumer (see `emit_key_handle`), all
    // of which are cold. The one exception is the typed-feedback OBSERVE call,
    // which sits in the hot `pget.pic` block — so under `--typed-feedback` the
    // handle is still produced once, up front, exactly as before.
    let key_handle_observed = crate::expr::typed_feedback_emission_enabled()
        .then(|| emit_key_handle(ctx, &key_handle_global));
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::PropertyGet,
        property,
        TypedFeedbackContract::object_get_by_name(),
    );

    // The outlined site (#5391 path 3: oversized modules; generator
    // continuations): one probe call that tests the receiver itself, then the
    // complete read entry on a decline.
    if (ctx.fixed_generator_state || crate::codegen::full_outline_ic_enabled())
        && global_reference.is_none()
    {
        let key_handle = emit_key_handle(ctx, &key_handle_global);
        let entry_args = [
            (I64, obj_bits.as_str()),
            (I64, key_handle.as_str()),
            (I64, feedback_site_id.as_str()),
        ];
        if property == "length" {
            // A plain Array's `length` from its header, a flat string's from
            // its header; every other receiver takes the complete read.
            let object_idx = ctx.new_block("pget.outline_length_object");
            let primitive_idx = ctx.new_block("pget.outline_length_primitive");
            let string_idx = ctx.new_block("pget.outline_length_string");
            let other_idx = ctx.new_block("pget.outline_length_other");
            let call_idx = ctx.new_block("pget.outline_length_call");
            let done_idx = ctx.new_block("pget.outline_length_done");
            let object_label = ctx.block_label(object_idx);
            let primitive_label = ctx.block_label(primitive_idx);
            let string_label = ctx.block_label(string_idx);
            let other_label = ctx.block_label(other_idx);
            let call_label = ctx.block_label(call_idx);
            let done_label = ctx.block_label(done_idx);
            let tag = ctx.block().lshr(I64, &obj_bits, "48");
            let is_pointer = ctx.block().icmp_eq(I64, &tag, "32765"); // 0x7FFD
            let above_band = ctx.block().icmp_ugt(
                I64,
                &entry_handle,
                crate::expr::receiver_range::HANDLE_BAND_TOP_LITERAL,
            );
            let candidate = ctx.block().and(I1, &is_pointer, &above_band);
            ctx.block()
                .cond_br(&candidate, &object_label, &primitive_label);
            ctx.current_block = object_idx;
            let (len, len_end) =
                emit_plain_array_length_arm(ctx, &entry_handle, &call_label, &done_label, false);
            ctx.current_block = call_idx;
            let object_value = ctx
                .block()
                .call(DOUBLE, "js_object_get_field_value", &entry_args);
            let object_end = ctx.block().label.clone();
            ctx.block().br(&done_label);
            ctx.current_block = primitive_idx;
            let is_string = ctx.block().icmp_eq(I64, &tag, "32767"); // 0x7FFF
            ctx.block().cond_br(&is_string, &string_label, &other_label);
            ctx.current_block = string_idx;
            let string_len = ctx.block().safe_load_i32_from_ptr(&entry_handle);
            let string_value = ctx.block().uitofp(I32, &string_len, DOUBLE);
            let string_end = ctx.block().label.clone();
            ctx.block().br(&done_label);
            ctx.current_block = other_idx;
            let primitive_value = ctx.block().call(
                DOUBLE,
                "js_value_length_property_f64",
                &[(DOUBLE, &obj_box)],
            );
            let primitive_end = ctx.block().label.clone();
            ctx.block().br(&done_label);
            ctx.current_block = done_idx;
            return Ok(ctx.block().phi(
                DOUBLE,
                &[
                    (&len, &len_end),
                    (&object_value, &object_end),
                    (&string_value, &string_end),
                    (&primitive_value, &primitive_end),
                ],
            ));
        }
        if !property.is_empty() && property.bytes().all(|b| b.is_ascii_digit()) {
            // An array-index name may be an element key no layout lists.
            return Ok(ctx
                .block()
                .call(DOUBLE, "js_object_get_field_value", &entry_args));
        }
        let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
            ctx,
            crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
            EMPTY_SHAPE_DIR,
        );
        let key_box = ctx.block().load(DOUBLE, &key_handle_global);
        let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
        let hash = (crate::layout_desc::atom_hash(property) as i64).to_string();
        let memo = (!crate::target_layout::target_is_ilp32(ctx.target_triple)
            && !property.starts_with('#')
            && property != "__proto__"
            && crate::expr::ic_fast_split::read_may_reach_getter(ctx, property))
        .then(|| crate::expr::ic_fast_split::emit_read_site_memo(ctx, 5));
        let fast_args = vec![
            (PTR, dir.as_str()),
            (I64, obj_bits.as_str()),
            (I64, key_bits.as_str()),
            memo.as_ref()
                .map_or((I64, hash.as_str()), |memo| (PTR, memo.as_str())),
        ];
        let mut slow_args = entry_args.to_vec();
        if let Some(memo) = memo.as_ref() {
            slow_args.push((PTR, memo.as_str()));
        }
        crate::expr::note_site("probe-outlined");
        return Ok(crate::expr::ic_fast_split::emit_hole_declining_split(
            ctx,
            "pget.outline",
            if memo.is_some() {
                "perry_probe_get_value_memo"
            } else {
                "perry_probe_get_value"
            },
            &fast_args,
            if memo.is_some() {
                "js_object_get_field_value_site"
            } else {
                "js_object_get_field_value"
            },
            &slow_args,
        ));
    }

    // # The inline site (OBJECT_MODEL_2026-10-13.md §3.7)
    //
    // receiver test -> [`.size` on a native Map/Set] -> [`.length` on a plain
    // Array or a heap string] -> the unknown-receiver probe -> on a decline,
    // the complete read entry. The per-site shape/slot proof is shared by
    // both halves; fixed layout arms above remain direct.
    //
    // `.length` admits both pointer-ish tags (POINTER and STRING) because it
    // has a heap-string arm; every other key takes the fused POINTER range
    // test (tag and a payload above the native-handle band in one compare).
    let inline_string_length = property == "length";
    let inline_collection_size = property == "size";
    // Ordinary named reads share one complete read after either the receiver
    // test or the noncollecting probe refuses. Length retains its primitive
    // and array/string arms; a global binding must still distinguish absence
    // from an own value of undefined before invoking a getter.
    let shared_refusal = !inline_string_length && !global_binding;
    let obj_tag = ctx.block().lshr(I64, &obj_bits, "48");
    let is_valid = if inline_string_length {
        let obj_tag_masked = ctx.block().and(I64, &obj_tag, "65533"); // 0xFFFD
        ctx.block().icmp_eq(I64, &obj_tag_masked, "32765") // 0x7FFD
    } else {
        fused_recv
            .as_ref()
            .expect("every key but `.length` takes the fused receiver test")
            .is_object_pointer
            .clone()
    };

    let recv_idx = ctx.new_block("pget.recv_ok");
    let call_idx = ctx.new_block("pget.slow");
    let other_idx = ctx.new_block("pget.recv_other");
    let merge_idx = ctx.new_block("pget.recv_merge");
    let recv_label = ctx.block_label(recv_idx);
    let call_label = ctx.block_label(call_idx);
    let other_label = ctx.block_label(other_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_valid, &recv_label, &other_label);

    // Off the pointer path: an SSO string's `.length` is a byte of the
    // NaN-box. Ordinary reads join the probe's refusal below; length and
    // required global bindings retain their separate primitive entry.
    ctx.current_block = other_idx;
    let sso_arm = inline_string_length.then(|| {
        let sso_idx = ctx.new_block("pget.recv_sso");
        let sso_label = ctx.block_label(sso_idx);
        let nonptr_idx = ctx.new_block("pget.recv_nonptr");
        let nonptr_label = ctx.block_label(nonptr_idx);
        let is_sso = ctx.block().icmp_eq(I64, &obj_tag, "32761"); // 0x7FF9
        ctx.block().cond_br(&is_sso, &sso_label, &nonptr_label);
        ctx.current_block = sso_idx;
        let sso_val = super::super::string_length::lower_sso_length(ctx, &obj_bits);
        let sso_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        ctx.current_block = nonptr_idx;
        (sso_val, sso_end_label)
    });
    let nonptr_arm = if shared_refusal {
        ctx.block().br(&call_label);
        None
    } else {
        let value = if inline_string_length {
            ctx.block().call(
                DOUBLE,
                "js_value_length_property_f64",
                &[(DOUBLE, &obj_box)],
            )
        } else {
            let key = emit_key_handle(ctx, &key_handle_global);
            ctx.block().call(
                DOUBLE,
                "js_object_get_field_nonptr",
                &[(I64, &obj_bits), (I64, &key), (I64, &feedback_site_id)],
            )
        };
        let end = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        Some((value, end))
    };

    ctx.current_block = recv_idx;
    if fused_recv.is_some() {
        crate::expr::receiver_range::emit_route_note(
            ctx.block(),
            crate::expr::receiver_range::Route::Generic,
        );
    }
    let observed_key = key_handle_observed.clone().unwrap_or_default();
    let observed_handle = if crate::expr::typed_feedback_emission_enabled() {
        recv_handle(ctx, fused_recv.as_ref(), &entry_handle)
    } else {
        String::new()
    };
    crate::expr::emit_typed_feedback_record_call(
        ctx.block(),
        "js_typed_feedback_observe_property_get",
        &[
            (I64, &feedback_site_id),
            (I64, &observed_handle),
            (I64, &observed_key),
        ],
    );

    // A heap string's `.length` (its leading `u32`).
    let strlen_heap_arm = inline_string_length.then(|| {
        let heap_idx = ctx.new_block("pget.strlen_heap");
        let heap_label = ctx.block_label(heap_idx);
        let obj_idx = ctx.new_block("pget.recv_obj");
        let obj_label = ctx.block_label(obj_idx);
        let is_heap_string =
            ctx.block()
                .icmp_eq(I64, &obj_tag, crate::nanbox::STRING_TAG_TOP16_I64);
        ctx.block()
            .cond_br(&is_heap_string, &heap_label, &obj_label);
        ctx.current_block = heap_idx;
        let len_i32 = ctx.block().safe_load_i32_from_ptr(&entry_handle);
        let heap_len = ctx.block().uitofp(I32, &len_i32, DOUBLE);
        let heap_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        ctx.current_block = obj_idx;
        // A small native handle (POINTER-tagged, inside the handle band) is
        // the complete read's.
        let real = ctx.block().icmp_ugt(
            I64,
            &entry_handle,
            crate::expr::receiver_range::HANDLE_BAND_TOP_LITERAL,
        );
        let real_idx = ctx.new_block("pget.recv_real");
        let real_label = ctx.block_label(real_idx);
        ctx.block().cond_br(&real, &real_label, &call_label);
        ctx.current_block = real_idx;
        (heap_len, heap_end_label)
    });

    // Native Map/Set `.size`: their common leading `u32 size`, after the
    // exact GC-kind test.
    let collection_size_arm = inline_collection_size.then(|| {
        let kind_idx = ctx.new_block("pget.collection_kind");
        let kind_label = ctx.block_label(kind_idx);
        let collection_idx = ctx.new_block("pget.collection_size");
        let collection_label = ctx.block_label(collection_idx);
        let probe_idx = ctx.new_block("pget.collection_other");
        let probe_label = ctx.block_label(probe_idx);
        ctx.block().br(&kind_label);
        ctx.current_block = kind_idx;
        let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let gc_type_addr = ctx.block().sub(I64, &handle, "8");
        let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
        let gc_type = ctx.block().load(I8, &gc_type_ptr);
        let is_map = ctx.block().icmp_eq(I8, &gc_type, "8"); // GC_TYPE_MAP
        let is_set = ctx.block().icmp_eq(I8, &gc_type, "12"); // GC_TYPE_SET
        let is_collection = ctx.block().or(I1, &is_map, &is_set);
        ctx.block()
            .cond_br(&is_collection, &collection_label, &probe_label);
        ctx.current_block = collection_idx;
        let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let size_i32 = ctx.block().safe_load_i32_from_ptr(&handle);
        let size = ctx.block().uitofp(I32, &size_i32, DOUBLE);
        let collection_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        ctx.current_block = probe_idx;
        (size, collection_end_label)
    });

    // A plain Array's `.length` from its header.
    let array_length_arm =
        (inline_string_length && !crate::expr::typed_feedback_emission_enabled()).then(|| {
            let probe_idx = ctx.new_block("pget.array_other");
            let probe_label = ctx.block_label(probe_idx);
            let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
            let arm = emit_plain_array_length_arm(ctx, &handle, &probe_label, &merge_label, true);
            ctx.current_block = probe_idx;
            arm
        });

    // §3.7 case 3: the receiver's own layout answers (`perry_probe_get`,
    // perry-runtime `object/layout_probe.rs`) from the key's atom and its
    // compile-time hash. A `length` site asks `perry_probe_get_length`,
    // which declines an Array subclass instance (it serves `length` from its
    // elements store, which no key list names) to the complete read.
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        EMPTY_SHAPE_DIR,
    );
    let probe_recv = match fused_recv.as_ref() {
        Some(f) => f.biased.clone(),
        None => ctx.block().sub(
            I64,
            &entry_handle,
            &crate::runtime_abi::RECEIVER_HANDLE_FLOOR.to_string(),
        ),
    };
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let hash = (crate::layout_desc::atom_hash(property) as i64).to_string();
    // An array-index name may be an element key, which no layout lists, and a
    // private name (`#x` after its brand guard) may be a method or accessor of
    // the declaring ClassBody, which no layout lists either: such a site takes
    // only answers the probe finds, never absence.
    let index_name = !property.is_empty() && property.bytes().all(|b| b.is_ascii_digit());
    let private_name = property.starts_with('#');
    let memo = (!crate::target_layout::target_is_ilp32(ctx.target_triple)
        && !global_binding
        && !index_name
        && !private_name
        && !inline_string_length
        && property != "__proto__"
        && crate::expr::ic_fast_split::read_may_reach_getter(ctx, property))
    .then(|| crate::expr::ic_fast_split::emit_read_site_memo(ctx, 5));
    let probe_args = vec![
        (PTR, dir.as_str()),
        (I64, probe_recv.as_str()),
        (I64, key_bits.as_str()),
        memo.as_ref()
            .map_or((I64, hash.as_str()), |memo| (PTR, memo.as_str())),
    ];
    crate::expr::note_site("probe");
    let answered = ctx.block().call(
        DOUBLE,
        // A global binding must tell absent from `undefined`.
        if global_binding || index_name || private_name {
            "perry_probe_get_binding"
        } else if inline_string_length {
            "perry_probe_get_length"
        } else if memo.is_some() {
            "perry_probe_get_memo"
        } else {
            "perry_probe_get"
        },
        &probe_args,
    );
    let answered_bits = ctx.block().bitcast_double_to_i64(&answered);
    let served = ctx
        .block()
        .icmp_ne(I64, &answered_bits, crate::nanbox::TAG_HOLE_I64);
    let probe_end_label = ctx.block().label.clone();
    ctx.block().cond_br(&served, &merge_label, &call_label);

    // The complete read: what the probe declined (an accessor, an exotic
    // receiver), ordinary non-pointer receivers, and a small native handle
    // at a `.length` site. Reload the key only on this refusal edge.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let miss_key_handle = emit_key_handle(ctx, &key_handle_global);
    let val_miss = if shared_refusal {
        // Preserve the original boxed receiver: the shared entry classifies
        // primitives and native handles as well as declined object pointers.
        // A site that owns a memo takes the same entry's memo-aware form.
        let mut args = vec![
            (I64, obj_bits.as_str()),
            (I64, miss_key_handle.as_str()),
            (I64, feedback_site_id.as_str()),
        ];
        if let Some(memo) = memo.as_ref() {
            args.push((PTR, memo.as_str()));
        }
        ctx.block().call(
            DOUBLE,
            if memo.is_some() {
                "js_object_get_field_value_site"
            } else {
                "js_object_get_field_value"
            },
            &args,
        )
    } else {
        let miss_handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let mut args = vec![(I64, miss_handle.as_str()), (I64, miss_key_handle.as_str())];
        if let Some(memo) = memo.as_ref() {
            args.push((PTR, memo.as_str()));
        }
        ctx.block().call(
            DOUBLE,
            if global_binding {
                "js_global_get_field_slow"
            } else if memo.is_some() {
                "js_object_get_field_site_slow"
            } else {
                "js_object_get_field_slow"
            },
            &args,
        )
    };
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let mut incoming: Vec<(&str, &str)> =
        vec![(&answered, &probe_end_label), (&val_miss, &miss_end_label)];
    if let Some((value, end)) = nonptr_arm.as_ref() {
        incoming.push((value, end));
    }
    if let Some((sso_val, sso_end_label)) = sso_arm.as_ref() {
        incoming.push((sso_val, sso_end_label));
    }
    if let Some((heap_len, heap_end_label)) = strlen_heap_arm.as_ref() {
        incoming.push((heap_len, heap_end_label));
    }
    if let Some((size, collection_end_label)) = collection_size_arm.as_ref() {
        incoming.push((size, collection_end_label));
    }
    if let Some((len, array_end_label)) = array_length_arm.as_ref() {
        incoming.push((len, array_end_label));
    }
    Ok(ctx.block().phi(DOUBLE, &incoming))
}
