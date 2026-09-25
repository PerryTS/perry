//! Generic monomorphic-IC property-get dispatch extracted from
//! `property_get.rs`.
//!
//! Pure mechanical move — body is the verbatim tail of the general catch-all
//! arm (the receiver-tag guard + SSO/class-ref/PIC/invalid diamond), lifted
//! into its own function.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// Words in a per-site property-read cache.
///
/// **Must equal `perry_runtime::object::field_get_set::PIC_CACHE_WORDS`.**
/// Since #9708 codegen emits only the 8-byte slot (`@perry_ic_N = private
/// global ptr null`) and the runtime allocates the words itself, sized from
/// its own `PicCache` — so the constant is no longer an emission width, but
/// the emitted way GEPs (`PIC_WAY_BASE + PIC_WAYS * 2` words) must still
/// land inside that allocation. perry-codegen does not depend on
/// perry-runtime (the same reason `INLINE_SLOT_FLOOR` is duplicated in
/// `target_layout`), so the pairing is held by `pic_cache_layout_matches_runtime`
/// here and `pic_cache_words_match_codegen` in the runtime: change one and both
/// fail.
#[cfg(test)]
pub(crate) const PIC_CACHE_WORDS: usize = 12;
/// First word of the polymorphic way array (words 0..2 are the MRU entry and
/// word 3 is the gate). Mirrors the runtime's `PIC_WAY_BASE`.
pub(crate) const PIC_WAY_BASE: usize = 4;
/// `(token, slot)` ways beyond the MRU entry; a site resolves `PIC_WAYS + 1`
/// shapes inline. Mirrors the runtime's `PIC_WAYS`.
pub(crate) const PIC_WAYS: usize = 4;
/// The value a per-site compact MRU word (`@perry_ic_N_packed_get`) holds
/// before anything has primed it.
///
/// **Must equal `perry_runtime::object::field_get_set::PACKED_GET_EMPTY`.**
///
/// It is NOT zero, and that is the whole point. The hit path compares the
/// receiver's ShapeId word against this word's low half; an object that was
/// never shape-stamped carries `parent_class_id` at +4, which is 0 for an
/// anonymous object literal, so a zero sentinel would let such a receiver
/// MATCH a site that has never primed and take the raw load at slot 0. That
/// is why the tower used to spend a separate `test`/`je` proving the word was
/// filled. A sentinel that no receiver word can equal makes the ShapeId
/// compare prove BOTH facts, and the extra test leaves every read.
///
/// `0xFFFF_FFFF` sits above every value the word at `+4` can hold: a ShapeId
/// ([`0x8000_0000`, `0xC000_0000`)), a synthetic class id (at or above
/// 0x8000_0000 today, [`0xC000_0000`, `0xFFFF_0000`) under #10824), or an
/// ordinary HIR class id, which is a counter from 1.
pub(crate) const PACKED_GET_EMPTY: i64 = 0xFFFF_FFFF;
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
/// path entirely. The spill entry is still served: `pic.token.miss` un-flips
/// the bit, and a match branches straight to the slow entry, which decodes the
/// same word. See the design note at the head of this function.
pub(crate) const PACKED_SPILL_FLIP: i64 = 0xC000_0000;

/// Way-state word: `> 0` means at least one way is populated and the compares
/// are worth running; `0` (fresh) and a negative megamorphic countdown
/// both skip them. Mirrors the runtime's `PIC_WAY_STATE`.
pub(crate) const PIC_WAY_STATE: usize = 3;
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
            let above_band = blk.icmp_ugt(I64, &target, "1048575"); // 0x100000
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

fn overridden_cache_name(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<String> {
    let Expr::LocalGet(base_local_id) = object else {
        return None;
    };
    ctx.property_get_ic_override
        .as_ref()
        .filter(|shared| {
            shared.base_local_id == *base_local_id && shared.property.as_str() == property
        })
        .map(|shared| shared.cache_name.clone())
}

fn allocate_property_cache(ctx: &mut FnCtx<'_>) -> String {
    let cache_site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, cache_site);
    ctx.pending_declares
        .push((format!("__ic_decl_{cache_site}"), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());
    cache_name
}

/// The generic per-site monomorphic inline-cache dispatch for `obj.property`.
/// This is the fall-through tail of the general catch-all arm: all earlier
/// specializations have been ruled out.
pub(crate) fn lower_generic_property_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    // #5247: record this access's source location right after the receiver is
    // evaluated and before the nullish-receiver throw path (the inline diamond
    // OR the full-outline `js_object_get_field_ic` helper — both throw "Cannot
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

    // #5391 path 3: oversized modules full-outline the entire generic-get diamond
    // (receiver-tag routing + monomorphic IC + feedback + nullish-throw) to a
    // single `js_object_get_field_ic(...)` call. This shrinks large minified user
    // functions enough for clang to compile them at a tolerable size/time — the
    // inline diamond is the biggest per-site __text contributor. The runtime helper
    // reproduces the same branch ladder and calls the same entries, so behavior is
    // unchanged; only the inline monomorphic fast-load is traded away. Mirrors the
    // class-field GET/SET full-outline (#5334 lever B / #5391 path 2).
    if crate::codegen::full_outline_ic_enabled() {
        // Per-site monomorphic IC cache, allocated identically to the inline path
        // (below) so the helper's `js_object_get_field_ic_miss` cache-priming is
        // unchanged.
        let cache_name = overridden_cache_name(ctx, object, property)
            .unwrap_or_else(|| allocate_property_cache(ctx));
        // #9708: the helper takes the site's SLOT and resolves the cache
        // itself; nothing is read inline here, so no load is emitted.
        let cache_slot_ref = format!("@{}", cache_name);
        // #10714: `.length` on a live plain Array is answered here, ahead of
        // the helper (a forwarding stub is not followed in this mode — see
        // `emit_plain_array_length_arm`). The helper cannot serve it any better than the inline
        // tower's PIC can — an Array never matches an object ShapeId — so
        // every such read paid the call, the helper's MRU probe, and the miss
        // handler's ladder, and cached nothing. Every other receiver takes the
        // call exactly as before. Profiling (`--typed-feedback`) builds keep
        // the call for every receiver: the helper records the OBSERVE this arm
        // would skip, and those builds keep their signal byte-identical.
        let array_arm = (property == "length" && !crate::expr::typed_feedback_emission_enabled())
            .then(|| {
                let header_idx = ctx.new_block("pget.outline_array_header");
                let call_idx = ctx.new_block("pget.outline_call");
                let merge_idx = ctx.new_block("pget.outline_merge");
                let header_label = ctx.block_label(header_idx);
                let call_label = ctx.block_label(call_idx);
                let merge_label = ctx.block_label(merge_idx);
                // The inline tower's pointer path in one test: the EXACT
                // POINTER_TAG (a heap string's `.length` is the helper's) and a
                // payload above the small-handle band (#340), before either header
                // byte is read.
                let blk = ctx.block();
                let tag = blk.lshr(I64, &obj_bits, "48");
                let is_pointer = blk.icmp_eq(I64, &tag, "32765"); // 0x7FFD
                let above_band = blk.icmp_ugt(I64, &entry_handle, "1048575"); // 0x100000
                let candidate = blk.and(I1, &is_pointer, &above_band);
                blk.cond_br(&candidate, &header_label, &call_label);
                ctx.current_block = header_idx;
                let arm = emit_plain_array_length_arm(
                    ctx,
                    &entry_handle,
                    &call_label,
                    &merge_label,
                    false,
                );
                ctx.current_block = call_idx;
                (arm, merge_idx, merge_label)
            });
        let key_handle = emit_key_handle(ctx, &key_handle_global);
        let val = ctx.block().call(
            DOUBLE,
            "js_object_get_field_ic",
            &[
                (I64, &obj_bits),
                (I64, &key_handle),
                (I64, &feedback_site_id),
                (PTR, &cache_slot_ref),
            ],
        );
        let Some(((len, len_end_label), merge_idx, merge_label)) = array_arm else {
            return Ok(val);
        };
        let call_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        ctx.current_block = merge_idx;
        return Ok(ctx.block().phi(
            DOUBLE,
            &[
                (len.as_str(), len_end_label.as_str()),
                (val.as_str(), call_end_label.as_str()),
            ],
        ));
    }

    // # Inline hit, two exits (T1); the hit is tag test -> shape compare -> load
    //
    // What stays inline below is exactly the hit: the receiver test (one
    // fused range compare for every key but `.length`), the compact MRU word compared against the receiver's
    // ShapeId, the raw slot load, and the bounded polymorphic ways. EVERY
    // other arm this tower used to expand — the SSO receiver, the INT32 class
    // ref, the nullish throw, the non-object receiver, the overflow load, the
    // deleted-slot miss, the two Array-subclass named-prefix ladders, and the
    // miss+prime — is now a branch to one of TWO calls that reproduce them in
    // the same order: `js_object_get_field_ic_nonptr` for a receiver that is
    // not a heap pointer, `js_object_get_field_ic_slow` for one that is. The
    // split is not cosmetic — see `pget.recv_other` below for the +4
    // instructions per HIT that a single shared exit cost.
    //
    // The ShapeId compare is the whole receiver classification. Five guards
    // that used to sit between the small-handle test and the load are gone
    // because the fact each one tested is now a function of the ShapeId word:
    // "is this site primed?" and the overflow-bit test (#10833: an unprimed
    // word is `PACKED_GET_EMPTY`, a spill entry is flipped out of the ShapeId
    // range), the GC-kind load (#10828, rule 3), the descriptor flag (#10824,
    // rule 1) and the `TAG_HOLE` compare (#10826: delete is a shape
    // transition). Each is argued at the point where it used to be emitted.
    //
    // The arms were not cheap to keep: ~37 basic blocks, ~177 pre-RS4GC IR
    // instructions and 6-7 call sites per site, each call a statepoint whose
    // live GC values are written into `.perry_gcmap`. On @babel/parser the
    // tower was 29% of all emitted IR across 6,487 sites. The ways still
    // resolve `PIC_WAYS + 1` shapes without a call.
    //
    // Issue #70/#73/#128: guard against non-pointer receivers
    // before the PIC deref. Tag-based check on the unmasked
    // NaN-box: real heap references have high-16-bits POINTER_TAG
    // (0x7FFD) or STRING_TAG (0x7FFF). `AND 0xFFFD` collapses both
    // to 0x7FFD; everything else (undefined/null/bool=0x7FFC,
    // int32=0x7FFE, bigint=0x7FFA, plain f64 like 0.0 globalThis
    // or 3.14, corrupt bit-patterns like 0x00FF_0000_0000 read as
    // a BufferHeader) falls through to the slow entry, whose tag
    // ladder throws for a nullish receiver (#462) and routes every
    // other tag to the by-name helper.
    //
    // Previously used a Darwin mimalloc heap-window check
    // (`> 2 TB && < 128 TB`). On aarch64-linux-android (issue
    // #128) Bionic Scudo allocations live far below 2 TB, so
    // every real object pointer failed the guard and the IC
    // returned undefined — `obj.x` read as NaN everywhere,
    // silently corrupting FFI args and pure-TS field compares.
    // Tag check is platform-independent: same two LLVM ops
    // (`lshr` + `and`) + one `icmp`, branch-predicted taken.
    // `.length` on a receiver whose static type is not a proven string, and
    // `.size` on one that may be a native Map/Set, are the only two keys this
    // tower serves from a cell that is not a `GC_TYPE_OBJECT`. Both decisions
    // are made from the property name alone, and `inline_string_length`
    // decides WHICH receiver-tag test to emit, so both are hoisted above it.
    let inline_string_length = property == "length";
    let inline_collection_size = property == "size";

    // The receiver-tag test, in the cheapest form the key allows.
    //
    // `.length` is the one key with a heap-STRING arm below, so it is the one
    // key whose test must admit BOTH pointer-ish tags — POINTER (0x7FFD) and
    // STRING (0x7FFF). Collapsing them costs real instructions: LLVM folds
    // `(bits >> 48) & 0xFFFD == 0x7FFD` back into a 64-bit mask and a 64-bit
    // compare, so the emitted test is two 10-byte `movabs`, a `mov`, an `and`,
    // a `cmp` and the branch — SIX instructions and 20 bytes of immediates,
    // measured on the k1 fixture at v0.5.1618.
    //
    // Every OTHER key gets the exact POINTER test. A heap string that fails it
    // now reaches `js_object_get_field_ic_nonptr`, whose heap-string arm masks
    // the tag off and calls the same by-name helper the object exit used to
    // reach — the same answer, one branch earlier instead of four loads later.
    //
    // MEASURED: this is worth ZERO instructions today, and it is still the
    // right test. InstCombine canonicalises `(x >> 48) == 0x7FFD` straight
    // back into `x & 0xFFFF_0000_0000_0000 == 0x7FFD_0000_0000_0000`, so the
    // emitted code is the same `mov`/`and`/`cmp` pair of 10-byte `movabs`
    // either way — only the mask constant changes, 0xFFFD to 0xFFFF. Writing
    // the compare on a TRUNCATED tag (`trunc i64 ... to i32`) does not defeat
    // the fold either; both forms were checked in the k1 fixture's
    // disassembly. Getting the `shr $0x30` + imm32 `cmp` form needs something
    // the IR builder cannot express today.
    //
    // It is kept because it is a PREREQUISITE, not a micro-optimisation. The
    // collapsed test admits STRING-tagged receivers onto the pointer path, and
    // the only thing that stops one having its `StringHeader` word at +4 read
    // as a ShapeId is the GC-kind guard below — the guard #10828 is about to
    // make removable. #10828's guarantee is stated over POINTER-tagged values;
    // this is what makes the emitted test match that statement.
    let obj_tag = ctx.block().lshr(I64, &obj_bits, "48");
    let is_valid = if inline_string_length {
        let obj_tag_masked = ctx.block().and(I64, &obj_tag, "65533"); // 0xFFFD
        ctx.block().icmp_eq(I64, &obj_tag_masked, "32765") // 0x7FFD
    } else {
        // The fused range test (see `fused_recv` above): POINTER tag AND a
        // payload above the native-handle band, in one compare.
        fused_recv
            .as_ref()
            .expect("every key but `.length` takes the fused receiver test")
            .is_object_pointer
            .clone()
    };

    // `.length` on a receiver whose static type is not a proven string.
    //
    // The three-arm string-length dispatch in `property_get.rs` (SSO length
    // byte / heap `utf16_len` load / property-semantic slow call) is already
    // fully RUNTIME-guarded — it tests the NaN-box tag and only takes an
    // inline arm for a value that IS a string — yet it is gated on
    // `is_string_expr`, a compile-time proof. A receiver the front end cannot
    // type (`rec.tag.length` where `rec` is an object-literal type, a JSON
    // `any`, an array element) therefore lands in this generic tower instead,
    // where a heap string can never be served: the PIC requires a
    // GC_TYPE_OBJECT receiver by construction (#72), so EVERY such read would
    // otherwise call out and walk a ladder built for objects. On `pipeline.ts`
    // that one read was ~9% of total run time.
    //
    // Both string tags are disjoint from POINTER_TAG, so serving them here is
    // a pure short-circuit: a primitive string's `length` is non-writable and
    // non-configurable, cannot be shadowed by an own property, and is exactly
    // what the runtime ladder computes. Everything else keeps the tower.
    // A dynamically typed `receiver.size` can still be served without the
    // object PIC when the live receiver is a native Map or Set. Both payloads
    // start with the same `u32 size` field, and their distinct GcHeader kinds
    // are checked below before the load. This is deliberately a runtime brand
    // check rather than a TypeScript-type claim: nested structural reads such
    // as `this.ctx.hooks.size` commonly lose their static Set type, while an
    // erased annotation alone must never authorize a native-layout load.

    // A compact per-site word holds the exact ShapeId and slot for the last
    // cacheable receiver. The lazily allocated full cache retains bounded
    // polymorphic ways. Both are
    // materialised before the first branch now: the single slow exit takes
    // them as arguments, and every failing guard reaches it.
    let cache_name = overridden_cache_name(ctx, object, property)
        .unwrap_or_else(|| allocate_property_cache(ctx));
    let cache_slot_ref = format!("@{cache_name}");
    // A compact atomic MRU removes the cache-pointer dependency on a hit.
    // The full cache stays lazy and serves prefix/overflow/polymorphic misses.
    let packed_site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let packed_name = format!(
        "{}_packed_get",
        crate::expr::inline_cache_global_name(ctx, packed_site)
    );
    ctx.typed_parse_rodata.push(format!(
        "@{packed_name} = private global i64 {PACKED_GET_EMPTY}, align 8"
    ));
    let packed_ref = format!("@{packed_name}");

    let pic_idx = ctx.new_block("pget.recv_ok");
    // The object exit. Named `pic.miss.call` because that is what it still is:
    // the block that calls out when the inline cache cannot serve the read. It
    // now also lands every receiver-validation failure ON THE POINTER PATH,
    // which is what collapses five of the six old call sites into it.
    let call_idx = ctx.new_block("pic.miss.call");
    // The non-pointer exit. Its own block and its own callee, deliberately:
    // when the receiver-tag test and the small-handle test failed to the SAME
    // block, SimplifyCFG folded the two guards into one flat predicate
    // (`cmp; sete; cmp; setae; test; je` for `cmp; jne; cmp; ja`) and every
    // property-read HIT paid +4.00 instructions — measured on a 10M-read
    // monomorphic loop, 1,231,521,261 -> 1,271,522,669 retired. That is
    // #7883's flat-predicate cost arriving from the optimiser instead of from
    // codegen. Distinct callees keep the chain branchy, and as a bonus the
    // unmasked `obj_bits` dies here instead of staying live across the hit
    // path for a call that might need it.
    let other_idx = ctx.new_block("pget.recv_other");
    let merge_idx = ctx.new_block("pget.recv_merge");
    let pic_label = ctx.block_label(pic_idx);
    let call_label = ctx.block_label(call_idx);
    let other_label = ctx.block_label(other_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Typed-feedback bookkeeping is COMPILE-TIME gated (off by default), so
    // the recording landing block only exists when it has something to record.
    // Without it every failing guard branches straight to `pic.miss.call`;
    // with it, the guard-fail/fallback-call pair is recorded on exactly the
    // edges that recorded it before (`pic.miss.cold` and `pic.miss`), and the
    // edges that recorded nothing — the non-pointer tags, the overflow slot,
    // a way that turned out to hold a hole — still record nothing.
    let cold_idx =
        crate::expr::typed_feedback_emission_enabled().then(|| ctx.new_block("pic.miss.cold"));
    let cold_label = cold_idx
        .map(|idx| ctx.block_label(idx))
        .unwrap_or_else(|| call_label.clone());

    ctx.block().cond_br(&is_valid, &pic_label, &other_label);

    // Off the pointer path only ONE arm is still worth inlining: an SSO
    // string's `.length`, which is a byte of the NaN-box itself. Every other
    // non-pointer receiver — an SSO string with any other key, an INT32 class
    // ref, `null`/`undefined`, a plain double — is the non-pointer entry's tag
    // ladder, in the same order it evaluated them here.
    ctx.current_block = other_idx;
    let sso_arm = inline_string_length.then(|| {
        let sso_idx = ctx.new_block("pget.recv_sso");
        let sso_label = ctx.block_label(sso_idx);
        let nonptr_idx = ctx.new_block("pget.recv_nonptr");
        let nonptr_label = ctx.block_label(nonptr_idx);
        let is_sso = ctx.block().icmp_eq(I64, &obj_tag, "32761"); // 0x7FF9
        ctx.block().cond_br(&is_sso, &sso_label, &nonptr_label);

        // SSO stores a byte count; JavaScript observes UTF-16 code units.
        ctx.current_block = sso_idx;
        let sso_val = super::super::string_length::lower_sso_length(ctx, &obj_bits);
        let sso_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        ctx.current_block = nonptr_idx;
        (sso_val, sso_end_label)
    });
    // A small native-registry handle fails the fused test too, but it is a
    // POINTER-tagged value and belongs to the OBJECT exit (the slow entry's
    // small-handle dispatch), exactly where the separate small-handle test
    // used to send it. The split costs nothing on the hit path: it runs only
    // after the receiver test has already failed.
    if fused_recv.is_some() {
        let nonptr_idx = ctx.new_block("pget.recv_nonptr");
        let nonptr_label = ctx.block_label(nonptr_idx);
        let tag = ctx.block().lshr(I64, &obj_bits, "48");
        let pointer_tagged = ctx
            .block()
            .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_TOP16_I64);
        ctx.block()
            .cond_br(&pointer_tagged, &cold_label, &nonptr_label);
        ctx.current_block = nonptr_idx;
    }
    // The non-pointer exit: SSO / INT32 class ref / nullish throw / everything
    // else, in that order, behind one call that needs no cache.
    let nonptr_key_handle = emit_key_handle(ctx, &key_handle_global);
    let val_nonptr = ctx.block().call(
        DOUBLE,
        "js_object_get_field_ic_nonptr",
        &[
            (I64, &obj_bits),
            (I64, &nonptr_key_handle),
            (I64, &feedback_site_id),
        ],
    );
    let nonptr_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = pic_idx;
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

    // Split the heap-string receiver off before the PIC. Placed AFTER the
    // typed-feedback observation on purpose: the site keeps recording every
    // receiver it sees, so a mixed object/string site cannot be mis-profiled
    // as monomorphic-object by the arm that is no longer traced here.
    let strlen_heap_idx = inline_string_length.then(|| {
        let heap_idx = ctx.new_block("pget.strlen_heap");
        let strlen_heap_label = ctx.block_label(heap_idx);
        let not_string_idx = ctx.new_block("pget.recv_obj");
        let not_string_label = ctx.block_label(not_string_idx);
        let is_heap_string =
            ctx.block()
                .icmp_eq(I64, &obj_tag, crate::nanbox::STRING_TAG_TOP16_I64);
        ctx.block()
            .cond_br(&is_heap_string, &strlen_heap_label, &not_string_label);
        ctx.current_block = not_string_idx;
        heap_idx
    });

    // Issue #72: validate the receiver is actually a GC_TYPE_OBJECT
    // before reading its ShapeId. The receiver
    // guard (`obj_handle > 0x100000`) keeps non-pointer NaN-boxes out,
    // but real heap pointers to Arrays/Strings/Buffers all clear that
    // threshold. A chained `obj.rowsRaw.length` (whose static type
    // analysis can't prove `obj.rowsRaw` is an Array — the outer
    // PropertyGet falls into this generic dispatch) hands the array's
    // pointer to this PIC. Reading an ObjectHeader ShapeId from that payload
    // would be invalid. The slow entry already routes by `gc_type` (it handles
    // Array.length, String.length, Set.size, Buffer.length, Error.message,
    // etc. through the same miss handler), so funneling non-OBJECT receivers
    // to it fixes correctness without giving up the PIC for real objects.
    //
    // Issue #340/#341: small-handle guard. Receivers from
    // native modules (axios, fastify, ioredis, better-sqlite3,
    // ...) are NaN-boxed POINTER values whose lower-48 is a
    // small registry id (1, 2, 3, ...). The PIC fast path
    // below deref's `obj_handle - 8` for the GcHeader byte
    // and `obj_handle + 4` for the ShapeId — both
    // SIGSEGV when `obj_handle` is a small int. Funnel
    // small-handle receivers through the slow entry so they
    // reach the runtime's `HANDLE_PROPERTY_DISPATCH` table
    // (axios `r.status` / `r.data`, fastify `req.query` /
    // `req.params`, etc.).
    //
    // Threshold matches `js_native_call_method`'s small-handle
    // detection (raw_ptr < 0x100000).
    //
    // Under the fused receiver test (every key but `.length`) this fact is
    // already proven on every edge into `pget.recv_ok`, so no second branch
    // is emitted for it.
    let is_real_ptr = fused_recv
        .is_none()
        .then(|| ctx.block().icmp_ugt(I64, &entry_handle, "1048575")); // 0x100000

    // #7883: the guard chain BRANCHES OUT to the slow exit on the first
    // failing predicate instead of AND-ing eight of them into one flat `hit`.
    // LLVM if-converts a flat predicate, so every receiver paid every load and
    // every compare even after the very first one had already decided the
    // answer.
    let tok_idx = ctx.new_block("pic.token");
    let tok_label = ctx.block_label(tok_idx);
    let hit_idx = ctx.new_block("pic.hit");
    // The inline hit's LIVE edge goes straight to the merge unless typed
    // feedback has a guard-pass record to put on it. An empty `pic.hit.live`
    // is congruent with `pic.way.live`, so SimplifyCFG tail-merges the two and
    // the hit path pays a `jmp` to the survivor instead of falling through.
    let hit_live_idx =
        crate::expr::typed_feedback_emission_enabled().then(|| ctx.new_block("pic.hit.live"));
    let miss_idx = ctx.new_block("pic.miss");
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    // Small-handle receivers (native-module registry ids) must never be
    // dereferenced. Pre-#7883 they were kept out of the loads by selecting a
    // sentinel address and AND-ing `is_real_ptr` into `hit`; the branch does
    // the same job without putting a `select` (and the sentinel's address
    // materialisation) in front of every real object read.
    // # No GC-header load, no descriptor-flag test: the ShapeId compare is
    // the receiver classification
    //
    // Between the small-handle test and the ShapeId compare this tower used
    // to load the `GcHeader` word at `receiver - 8` and require
    // `obj_type == GC_TYPE_OBJECT` with `OBJ_FLAG_HAS_DESCRIPTORS` clear
    // (#72, #6080): a packed `i32` load, a 4-byte immediate `and`, a compare
    // and a branch on every hit, plus an endianness split in the emitter.
    // Both facts are now carried by the ShapeId word itself, so the compare
    // below proves them and the load is gone. The argument, each part held
    // by another lane's tests:
    //
    // * **Kind** (#10828, rule 3): for any POINTER-tagged value that passes
    //   the receiver-tag test, the u32 at payload `+4` equals a live object
    //   ShapeId only if the cell is a `GC_TYPE_OBJECT` carrying that shape.
    //   Every other GC kind's `+4` word is a count bounded below the ShapeId
    //   floor in release at its allocation funnel, a structurally small
    //   value, or (for `DateCell`) was moved. `object/shape_rule3.rs` walks
    //   all 21 kinds and asserts the fence-keeping set is empty. This is why
    //   the tag test above is the EXACT `POINTER_TAG` test and not the
    //   collapsed pointer-or-string one (#10833): a heap string's `+4` is
    //   its `StringHeader`, and #10828's guarantee is stated over
    //   POINTER-tagged values.
    // * **Descriptors** (#10824, rule 1): every descriptor install, per-key
    //   removal and bulk clear on an ordinary object transitions its ShapeId
    //   (the last gaps — `clear_object_descriptors` and seven raw table
    //   `remove()` calls outside `descriptor_state.rs` — are closed). A site
    //   primed on a plain data slot therefore cannot match the receiver once
    //   `defineProperty` has converted that key to a getter: the receiver's
    //   `+4` word changed. The prime side (`get_field_ic_miss_impl`) refuses
    //   a descriptor-bearing receiver, so no cached word names a shape whose
    //   slot the descriptor tables might override; the one exception, the
    //   Array-subclass named-prefix proof, carries its own per-key data-only
    //   proof for the slot it publishes.
    // * **Unstamped receivers** (#10824, rule 2): nothing but the shape
    //   allocator mints into the ShapeId range — synthetic class ids started
    //   AT the floor and moved to `[0xC000_0000, 0xFFFF_0000)` — so a
    //   receiver still carrying `parent_class_id` at `+4` cannot match a
    //   primed word. "Is this site primed?" is not asked either: the word is
    //   born holding `PACKED_GET_EMPTY`, which no `+4` word can equal.
    //
    // `.size` is the one key that still reads the header byte here, and only
    // to serve a native Map/Set — whose `size` is their leading `u32` — from
    // its own arm; the byte is NOT consulted for the object path. A receiver
    // that is neither takes the ShapeId compare exactly like every other key.
    // (`.length` reads it too, to serve a plain Array, but only AFTER the
    // compare has failed — see `pget.array_kind` below.)
    let collection_size_idx = inline_collection_size.then(|| {
        let kind_idx = ctx.new_block("pget.collection_kind");
        let kind_label = ctx.block_label(kind_idx);
        let collection_idx = ctx.new_block("pget.collection_size");
        let collection_label = ctx.block_label(collection_idx);
        match is_real_ptr.as_ref() {
            Some(real) => ctx.block().cond_br(real, &kind_label, &cold_label),
            None => ctx.block().br(&kind_label),
        }
        ctx.current_block = kind_idx;
        // `GcHeader` starts with `obj_type: u8`, at offset 0 of the header on
        // every target, so this is one byte load whatever the byte order.
        let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let gc_type_addr = ctx.block().sub(I64, &handle, "8");
        let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
        let gc_type = ctx.block().load(I8, &gc_type_ptr);
        let is_map = ctx.block().icmp_eq(I8, &gc_type, "8"); // GC_TYPE_MAP
        let is_set = ctx.block().icmp_eq(I8, &gc_type, "12"); // GC_TYPE_SET
        let is_collection = ctx.block().or(I1, &is_map, &is_set);
        ctx.block()
            .cond_br(&is_collection, &collection_label, &tok_label);
        collection_idx
    });
    if collection_size_idx.is_none() {
        match is_real_ptr.as_ref() {
            Some(real) => ctx.block().cond_br(real, &tok_label, &cold_label),
            None => ctx.block().br(&tok_label),
        }
    }
    ctx.current_block = tok_idx;

    // The compact cache is a permanently valid scalar global; its load and
    // the receiver's ShapeId load below are independent, so they overlap.
    let packed_word = ctx.block().load_atomic_monotonic(I64, &packed_ref, 8);

    // The receiver token is derived solely from its authoritative ShapeId.
    // Invalid/unstamped payloads miss closed.
    // #8113: the ShapeId word moved from header offset 8 to 4.
    let pcid_ptr = match fused_recv.as_ref() {
        // `handle + 4`, addressed from the biased value (`receiver_range`).
        Some(f) => crate::expr::receiver_range::emit_field_ptr(ctx.block(), &f.biased, 4),
        None => {
            let pcid_addr = ctx.block().add(I64, &entry_handle, "4");
            ctx.block().inttoptr(I64, &pcid_addr)
        }
    };
    // The hot ShapeId load has exactly ONE use: the compare. The two cold
    // consumers of the same word — the spill compare in `pic.token.miss` and
    // the way token in `pic.ways` — read it AGAIN there, through an atomic
    // load that GVN will not merge with this one. That is a deliberate
    // re-derivation on the miss path (one load, on a path that is about to
    // spend hundreds), and it is what lets isel fold this load into the
    // compare itself: `cmp %ecx, 4(%rdi)` instead of a `mov` and a `cmp`,
    // one instruction fewer on every hit. With the word live into the cold
    // blocks it had to sit in a register.
    let pcid = ctx.block().load(I32, &pcid_ptr);

    // A nonzero packed word contains a valid ShapeId and its slot. The
    // header guard above rejects a fresh site; matching the low 32 bits then
    // proves the shape without a discriminator OR or a wide token mask.
    let packed_stamp = ctx.block().trunc(I64, &packed_word, I32);
    let token_eq = ctx.block().icmp_eq(I32, &pcid, &packed_stamp);
    let token_miss_idx = ctx.new_block("pic.token.miss");
    let token_miss_label = ctx.block_label(token_miss_idx);
    // `.length` on a plain Array (#10714), tested on the compare's FALSE edge
    // and nowhere earlier. See `emit_plain_array_length_arm` for what it
    // answers and what it leaves to `pic.token.miss`.
    //
    // An Array can never take the hit: its `+4` word is `capacity`, a count
    // rule 3 (#10828) bounds below the ShapeId floor, so the compare above
    // already routes every Array here — and before this arm, on to the slow
    // entry, which answered from the header and cached nothing (there is
    // nothing an object cache can hold for it). Testing the header HERE, not
    // ahead of the compare the way `.size` does, keeps a `.length` site's
    // object HIT path the exact instruction sequence every other key's is;
    // the price is that an Array pays the two loads and the compare first.
    //
    // Profiling (`--typed-feedback`) builds keep the old edge: every Array
    // read records a guard-fail and a fallback-call on its way to the exit,
    // and a read served without a call would change those records. The
    // inherited-read hook below makes the same trade for the same reason.
    let array_length_arm =
        (inline_string_length && !crate::expr::typed_feedback_emission_enabled()).then(|| {
            let kind_idx = ctx.new_block("pget.array_kind");
            let kind_label = ctx.block_label(kind_idx);
            ctx.block().cond_br(&token_eq, &hit_label, &kind_label);
            ctx.current_block = kind_idx;
            // `.length` takes no fused receiver test, so this is the
            // entry-block mask; the edge here is dominated by the tag test,
            // the heap-string split and the small-handle test above, so the
            // arm's precondition (a POINTER-tagged address above the handle
            // band) holds.
            let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
            emit_plain_array_length_arm(ctx, &handle, &token_miss_label, &merge_label, true)
        });
    if array_length_arm.is_none() {
        ctx.block()
            .cond_br(&token_eq, &hit_label, &token_miss_label);
    }

    ctx.current_block = token_miss_idx;
    // The cold re-read of the ShapeId word — see the hot load above.
    let pcid = ctx.block().load_atomic_monotonic(I32, &pcid_ptr, 4);
    let pcid64 = ctx.block().zext(I32, &pcid, I64);
    // pic_prime_get is the only production writer of get-cache tokens and
    // refuses the zero-ShapeId token. All remaining tokens carry a valid,
    // never-reused ShapeId; vacant entries are zero. Equality therefore
    // proves a nonzero stamp without another check on every property read.
    // Keyless Object.create(proto) receivers still miss and walk prototypes.
    let token = ctx.block().or(I64, &pcid64, "4611686018427387904");
    // The SPILL entry — tested HERE, and nowhere on the hit path.
    //
    // A key past the object's inline region used to publish its slot into the
    // compact word with `IC_SLOT_OVERFLOW_BIT` set, and every read of every
    // site paid to ask whether the bit was there: LLVM folds
    // `((packed >> 32) & (1 << 30)) == 0` into `packed & (1 << 62)`, which is
    // a 10-byte `movabs`, a `test` and a branch on the hit path of sites whose
    // field is inline and can never see the bit.
    //
    // Now a spill entry publishes the SAME ShapeId with `PACKED_SPILL_FLIP`
    // flipped into it, which lands it outside the ShapeId range, so the hit
    // path's compare refuses it for free. Un-flipping the bit here recognises
    // it in three instructions ON THE MISS PATH ONLY, and a match branches
    // straight to the slow entry — skipping the full cache's resolution and
    // the polymorphic ways, neither of which can serve a spill key anyway
    // (`pic_prime_get` refuses to cascade an encoded slot into a way). The
    // slow entry decodes the same word and reads the spill buffer, so a spill
    // read pays the same three instructions it paid before, just in a block
    // the inline hit never enters.
    let spill_stamp = ctx
        .block()
        .xor(I32, &packed_stamp, &PACKED_SPILL_FLIP.to_string());
    let is_spill = ctx.block().icmp_eq(I32, &pcid, &spill_stamp);
    let ways_entry_idx = ctx.new_block("pic.token.ways");
    let ways_entry_label = ctx.block_label(ways_entry_idx);
    ctx.block()
        .cond_br(&is_spill, &call_label, &ways_entry_label);

    // Every way load still requires a resolved full cache. A site that has
    // never primed has no cache, so there is nothing to compare against.
    //
    // That "never primed" edge is also exactly where an INHERITED read lives:
    // a key on the prototype chain is never an own slot on the receiver's
    // shape, so a site that only ever reads it never resolves its cache, and
    // every read of it reaches this branch with `present` false. So that
    // edge, and no other, asks the inherited-read cache (#10834/#10842)
    // before calling out — see `pic.miss.inherited` below. Every other path
    // to the exit (a small handle, a spill entry, an MRU or way miss at a site
    // that HAS primed) is unchanged to the instruction; the first placement
    // asked on all of them and cost every own-key miss the price of a
    // declining probe (+88 on a megamorphic site, +89 on a spill read).
    //
    // Under `--typed-feedback` the edge keeps its old target: the recording
    // blocks put a guard-fail and a fallback-call record on precisely this
    // edge, and a read served without a call would have to change one of
    // those records. Feedback builds are profiling builds; they keep their
    // signal byte-identical and go without the hook.
    ctx.current_block = ways_entry_idx;
    let token_cache = crate::expr::emit_inline_cache_slot(ctx, &cache_name);
    let inherited_idx = (!crate::expr::typed_feedback_emission_enabled())
        .then(|| ctx.new_block("pic.miss.inherited"));
    let never_primed_label = inherited_idx
        .map(|idx| ctx.block_label(idx))
        .unwrap_or_else(|| cold_label.clone());
    ctx.block()
        .cond_br(&token_cache.present, &miss_label, &never_primed_label);

    // `js_object_get_field_ic_miss` primes only slots below the descriptor's
    // exact `live_inline_slot_count`. ShapeIds are never reused, so an exact
    // token hit permanently proves that the cached slot remains live and
    // makes the raw load below safe without a compatibility-header bound.
    ctx.current_block = hit_idx;
    if fused_recv.is_some() {
        crate::expr::receiver_range::emit_route_note(
            ctx.block(),
            crate::expr::receiver_range::Route::GenericMruHit,
        );
    }
    // A matched compact word is now, by construction, an INLINE slot: a
    // spill-located key publishes its ShapeId flipped by `PACKED_SPILL_FLIP`
    // and is recognised in `pic.token.miss` instead. The overflow-bit test
    // that used to stand between this shift and the load is gone from the hit
    // path — see the note there for what it cost and where it went.
    let slot = ctx.block().lshr(I64, &packed_word, "32");
    // arm64_32 watchOS: the object fields region begins at
    // `size_of::<ObjectHeader>()` past the user pointer — 16 on LP64 and
    // padded ILP32 since #8047. Derive it from the target triple.
    let obj_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let header_bytes = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    let base_ptr = match fused_recv.as_ref() {
        Some(f) => {
            crate::expr::receiver_range::emit_field_ptr(ctx.block(), &f.biased, header_bytes)
        }
        None => {
            let base = ctx.block().add(I64, &entry_handle, &obj_header_size);
            ctx.block().inttoptr(I64, &base)
        }
    };
    // A typed GEP rather than `shl 3` + `add`: the slot is the LAST use of the
    // packed word here, so with an explicit `shl` InstCombine folds
    // `(packed >> 32) << 3` into `(packed >> 29) & 0x5fffffff8` and isel pays a
    // 10-byte `movabs` plus an `and` for the mask. As a GEP index there is no
    // `shl` to fold, so the scaled addressing mode survives — `shr $32` plus
    // `(base,idx,8)`, which is what the pre-T1 tower emitted (it kept the plain
    // `shr` only because its overflow block consumed the same value).
    let field_ptr = ctx.block().gep(DOUBLE, &base_ptr, &[(I64, &slot)]);
    let val_hit = ctx.block().load(DOUBLE, &field_ptr);
    // The loaded value is the answer. The `TAG_HOLE` compare that used to
    // stand here (four instructions on every read: bitcast, 10-byte `movabs`
    // or a stack reload of the constant, `cmp`, branch) was the patch for one
    // operation — `delete` — which under #9064's stable tombstones kept the
    // receiver's ShapeId and marked the slot instead. #10826 made every
    // successful delete a shape transition: the receiver's `+4` word ALWAYS
    // changes, and when the keys array is owned the predecessor id is retired
    // (`shape_descriptor_by_id` -> `None`), so a compact word primed before a
    // delete cannot match after it, and a ShapeId hit proves the slot it names
    // is live. Every inline slot is born `TAG_UNDEFINED` (`object/alloc.rs`),
    // so nothing but a delete ever writes a hole into one.
    //
    // The hole stays in the SLOT, so every path that reaches a slot WITHOUT a
    // shape-hit proof — the spill arm, a keys-array scan, `object_field_at`,
    // every walker — must still treat it as absent, and does. The way path
    // below no longer compares either: the argument is stated there.
    //
    // Ordinary deletes always transition the shape before clearing the slot.
    let hit_end_label = match hit_live_idx {
        None => {
            let label = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            label
        }
        Some(idx) => {
            let live_label = ctx.block_label(idx);
            ctx.block().br(&live_label);
            ctx.current_block = idx;
            crate::expr::emit_typed_feedback_record_call(
                ctx.block(),
                "js_typed_feedback_record_guard_pass",
                &[(I64, &feedback_site_id)],
            );
            let label = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            label
        }
    };

    // PIC miss on the MRU entry — before paying for the call, try the
    // polymorphic ways (#7753).
    //
    // The slow entry is not a cheap fallback: it re-derives the receiver kind
    // from scratch (proxy band, closure magic, registered-buffer and
    // typed-array registries, small-handle dispatch), reads the
    // accessors-in-use thread-local, then linear-scans the keys array with a
    // `js_string_equals` per key. On a site whose receiver alternates between a
    // handful of shapes — the shape of every discriminated-union dispatch —
    // a single-entry cache misses on essentially every read and that whole
    // ladder runs per field access. Measured on a tree-walking interpreter it
    // was ~34% of run time.
    //
    // The ways are consulted only here, so a genuinely monomorphic site keeps
    // the exact instruction sequence it had before this block existed. The
    // typed-feedback counters are also recorded before the way compares, so a
    // way hit still reports guard-fail + fallback-call exactly as it did when
    // it was a real miss — the feedback heuristics see an unchanged signal
    // (the site IS polymorphic; only the cost of that changed).
    //
    // # Why this block is DOMINATED by `pic.token` (#7907)
    //
    // Its only predecessor is `pic.token.miss`, which is `pic.token`'s. The
    // exact descriptor identity proves cached-slot bounds, so `token` is
    // everything the way compares need, and the cache pointer arrives on one
    // edge rather than through a phi.
    //
    // #7883 could not rely on that: it routed the two receiver-validation
    // failures here as well, which left the values live on only some edges, so
    // the block **re-derived them** — header and identity loads, the token
    // select, and a safe-address select for small-handle receivers.
    // That was correct, and it was justified as cold. It is not cold: on a site
    // whose receiver rotates over more shapes than the MRU entry holds — the
    // shape #7753's ways exist for — this block runs on nearly every read, so
    // the duplicate ladder sat on the hot path. Measured on `interp.ts`'s
    // `evalNode`, the single hottest instruction in the whole program was the
    // redundant receiver reconstruction inside this block.
    ctx.current_block = miss_idx;
    let cache_ref = token_cache.cache.clone();
    crate::expr::emit_typed_feedback_record_call(
        ctx.block(),
        "js_typed_feedback_record_guard_fail",
        &[(I64, &feedback_site_id)],
    );
    crate::expr::emit_typed_feedback_record_call(
        ctx.block(),
        "js_typed_feedback_record_fallback_call",
        &[(I64, &feedback_site_id)],
    );

    // Every way contains a ShapeId token. A non-zero receiver token keeps an
    // empty way from matching; no GC-epoch guard is necessary because ids are
    // never reused and descriptor identity survives key relocation.
    //
    // The compares sit behind their own branch on `cache[PIC_WAY_STATE] > 0`
    // rather than being folded into one flat predicate, because a site whose
    // receiver rotation is WIDER than the ways hold never hits one and would
    // otherwise pay four dependent loads on every read: measured at **+37%** on
    // a 7-shape site, against a 2.5x speedup on a 5-shape one. `pic_prime_get`
    // latches that state to `-1` once a site proves itself megamorphic, and a
    // fresh site reads `0`, so for both the branch is one load,
    // one compare, and a perfectly predicted fall-through to the call — which
    // is exactly the pre-#7753 code path.
    let state_ptr = ctx
        .block()
        .gep(I64, &cache_ref, &[(I64, &PIC_WAY_STATE.to_string())]);
    let way_state = ctx.block().load(I64, &state_ptr);
    let ways_live = ctx.block().icmp_sgt(I64, &way_state, "0");
    let ways_idx = ctx.new_block("pic.ways");
    let ways_label = ctx.block_label(ways_idx);
    ctx.block().cond_br(&ways_live, &ways_label, &call_label);

    ctx.current_block = ways_idx;
    // `is_object` is not ANDed in any more: it is statically true on every edge
    // that reaches here (#7907 — see the dominance note above).
    // Reduced as a BALANCED TREE, not as a left fold. At most one way can hold
    // a given token (`pic_prime_get` evicts a duplicate before it writes one,
    // and pic_prime_get excludes zero-ShapeId tokens), so the association is
    // free to change — but the fold made `way_slot` a chain of `PIC_WAYS`
    // dependent `csel`s whose last node is the operand of the bounds compare
    // that gates the branch out of this block. On `interp.ts` that node was the
    // hottest instruction in `evalNode` (#7907). The tree halves the chain.
    let mut lanes: Vec<(String, String)> = Vec::with_capacity(PIC_WAYS);
    for w in 0..PIC_WAYS {
        let tok_ptr = ctx.block().gep(
            I64,
            &cache_ref,
            &[(I64, &(PIC_WAY_BASE + w * 2).to_string())],
        );
        let way_tok = ctx.block().load(I64, &tok_ptr);
        let eq = ctx.block().icmp_eq(I64, &way_tok, &token);
        let slot_ptr = ctx.block().gep(
            I64,
            &cache_ref,
            &[(I64, &(PIC_WAY_BASE + w * 2 + 1).to_string())],
        );
        let way_slot_val = ctx.block().load(I64, &slot_ptr);
        let lane_slot = ctx.block().select(I1, &eq, I64, &way_slot_val, "0");
        lanes.push((eq, lane_slot));
    }
    while lanes.len() > 1 {
        let mut merged: Vec<(String, String)> = Vec::with_capacity(lanes.len().div_ceil(2));
        for pair in lanes.chunks(2) {
            match pair {
                [(a_any, a_slot), (b_any, b_slot)] => {
                    let any = ctx.block().or(I1, a_any, b_any);
                    let slot = ctx.block().select(I1, a_any, I64, a_slot, b_slot);
                    merged.push((any, slot));
                }
                [single] => merged.push(single.clone()),
                _ => unreachable!("chunks(2) yields one or two elements"),
            }
        }
        lanes = merged;
    }
    let (way_any, way_slot) = lanes
        .pop()
        .expect("PIC_WAYS is non-zero, so the reduction leaves exactly one lane");
    let way_load_idx = ctx.new_block("pic.way.load");
    let way_load_label = ctx.block_label(way_load_idx);
    ctx.block().cond_br(&way_any, &way_load_label, &call_label);

    ctx.current_block = way_load_idx;
    if fused_recv.is_some() {
        crate::expr::receiver_range::emit_route_note(
            ctx.block(),
            crate::expr::receiver_range::Route::GenericWayHit,
        );
    }
    let way_offset = ctx.block().shl(I64, &way_slot, "3");
    let way_handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
    let way_base = ctx.block().add(I64, &way_handle, &obj_header_size);
    let way_field_addr = ctx.block().add(I64, &way_base, &way_offset);
    let way_field_ptr = ctx.block().inttoptr(I64, &way_field_addr);
    let val_way = ctx.block().load(DOUBLE, &way_field_ptr);
    // The loaded value is the answer here too, for the reason the shape-gated
    // hit above needs no `TAG_HOLE` compare (#10826: a successful delete
    // ALWAYS moves the receiver's ShapeId, so an exact-id match proves the
    // slot it names is live).
    //
    // A way pair is not a second kind of cache entry needing its own
    // argument. `pic_prime_get` is the ONLY writer of a way, and the only
    // values it ever writes into one are `prev_tok`/`prev_slot` — the pair
    // that was sitting in the MRU entry. Every `(token, slot)` a way holds is
    // therefore an MRU pair that aged out; the token it is compared against is
    // the same receiver ShapeId word the MRU compare reads; and ShapeIds are
    // never reused. Whatever makes the MRU pair safe to load without a hole
    // check makes the way pair safe — the entry did not become weaker by
    // moving one word over.
    //
    // The two ways in which a way pair differs from an MRU pair both narrow
    // it: an overflow-encoded slot is refused entry to a way at all, and a way
    // is consulted only after the MRU entry has already missed.
    let way_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // #7907: receiver-validation failure. A receiver that gets here can never
    // match a way — the compares require a real pointer to a plain
    // descriptor-free `ObjectHeader` — so it goes straight to the handler,
    // which reproduces the whole ladder anyway (proxy band, closure magic,
    // buffer/typed-array registries, small-handle dispatch). The typed-feedback
    // counters are the same two records on the same edges, so the feedback
    // signal is byte-identical to what the pre-T1 blocks reported.
    if let Some(cold_idx) = cold_idx {
        ctx.current_block = cold_idx;
        crate::expr::emit_typed_feedback_record_call(
            ctx.block(),
            "js_typed_feedback_record_guard_fail",
            &[(I64, &feedback_site_id)],
        );
        crate::expr::emit_typed_feedback_record_call(
            ctx.block(),
            "js_typed_feedback_record_fallback_call",
            &[(I64, &feedback_site_id)],
        );
        ctx.block().br(&call_label);
    }

    // The inherited-read hook, on the never-primed edge only (see the branch
    // that reaches it, in `pic.token.ways`). A read whose key lives on the
    // prototype chain can never take the own-slot hit — the receiver's shape
    // says the key is not own — so before this block it paid the slow entry's
    // prologue and dispatch (79 of an inherited read's 204 instructions,
    // measured by the inherited-reads lane) just to reach the same lookup
    // inside `get_field_ic_miss_impl`. `js_inherited_read_cache_hit_f64` is
    // a pure state read — it allocates nothing, triggers no GC and runs no
    // user code — so it is a leaf in `gc_call_effects.rs` and
    // `root_reload.rs`: no spill, no reload around it. `TAG_HOLE` is its
    // decline sentinel, which no ordinary value can be, so the answer is one
    // compare, with the SERVED edge as the true edge like every guard-passing
    // edge in this tower (#7883); a decline continues to the one exit exactly
    // as the never-primed edge did before. Nothing is primed from here:
    // priming stays in the miss handler, the one place that already knows
    // the key is not own without a second search. The versioned-loop deopt
    // note is emitted here as it is on the exit, so entering either cold arm
    // still records the bailout.
    let inherited_arm = inherited_idx.map(|idx| {
        ctx.current_block = idx;
        crate::expr::emit_versioned_loop_callback_deopt(ctx);
        let inh_key_handle = emit_key_handle(ctx, &key_handle_global);
        let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let recv_ptr = ctx.block().inttoptr(I64, &handle);
        let key_ptr = ctx.block().inttoptr(I64, &inh_key_handle);
        let val_inherited = ctx.block().call(
            DOUBLE,
            "js_inherited_read_cache_hit_f64",
            &[(PTR, &recv_ptr), (PTR, &key_ptr)],
        );
        let inherited_bits = ctx.block().bitcast_double_to_i64(&val_inherited);
        let inherited_served =
            ctx.block()
                .icmp_ne(I64, &inherited_bits, crate::nanbox::TAG_HOLE_I64);
        let inherited_end_label = ctx.block().label.clone();
        ctx.block()
            .cond_br(&inherited_served, &merge_label, &cold_label);
        (val_inherited, inherited_end_label)
    });

    // The object exit: one call reproducing every pointer-path arm this tower
    // used to expand.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let miss_key_handle = emit_key_handle(ctx, &key_handle_global);
    let miss_handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
    let val_miss = ctx.block().call(
        DOUBLE,
        "js_object_get_field_ic_slow",
        &[
            (I64, &miss_handle),
            (I64, &miss_key_handle),
            (PTR, &cache_slot_ref),
            (PTR, &packed_ref),
        ],
    );
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Native Map/Set `.size`: their common leading field was admitted only by
    // the exact live GC-kind checks above. Keep the read inline; calling
    // `js_map_size` / `js_set_size` would reclassify the same receiver again.
    let collection_size_arm = collection_size_idx.map(|collection_idx| {
        ctx.current_block = collection_idx;
        let handle = recv_handle(ctx, fused_recv.as_ref(), &entry_handle);
        let size_i32 = ctx.block().safe_load_i32_from_ptr(&handle);
        let size = ctx.block().uitofp(I32, &size_i32, DOUBLE);
        let collection_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        (size, collection_end_label)
    });

    // Heap string `.length`: `utf16_len` is the leading `u32` of
    // `StringHeader` — the identical load the proven-string lowering in
    // `property_get.rs` emits (`strlen.heap`). `safe_load_i32_from_ptr`
    // keeps a sub-page handle off the load.
    let strlen_heap_arm = strlen_heap_idx.map(|heap_idx| {
        ctx.current_block = heap_idx;
        let len_i32 = ctx.block().safe_load_i32_from_ptr(&entry_handle);
        let heap_len = ctx.block().uitofp(I32, &len_i32, DOUBLE);
        let heap_end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        (heap_len, heap_end_label)
    });

    // One merge for the whole tower: the inline hit, a way hit, the two slow
    // calls, and whichever short-circuit arms this key grew.
    ctx.current_block = merge_idx;
    let mut incoming: Vec<(&str, &str)> = vec![
        (&val_hit, &hit_end_label),
        (&val_way, &way_end_label),
        (&val_miss, &miss_end_label),
        (&val_nonptr, &nonptr_end_label),
    ];
    if let Some((val_inherited, inherited_end_label)) = inherited_arm.as_ref() {
        incoming.push((val_inherited, inherited_end_label));
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
