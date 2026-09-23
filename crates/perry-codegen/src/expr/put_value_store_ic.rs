//! The one inline path for an existing-property store `o.k = v`.
//!
//! Every static-key, same-receiver store that no earlier specialisation took
//! (class field, POD record, scalar-replaced local, array index or length)
//! reaches [`emit_static_store_ic`], whatever its right-hand side. The emitted
//! hit is:
//!
//! ```text
//!   receiver tag test          bits ^ POINTER_TAG, top 16 bits zero
//!   small-handle test          handle > 0xFFFFF (native registry ids, proxies)
//!   ONE shape compare          u32 at handle+4 == low half of @..._packed_set
//!   per-object receiver test   _reserved & PROOF == 0, and the receiver kind
//!   the store                  handle + 16 + slot*8, slot = high half
//!   the barrier the GC needs   behind a live test of the stored bits
//! ```
//!
//! and everything else is ONE call, `js_put_value_set_packed_miss`, which runs
//! the unchanged strict-aware `[[Set]]` and publishes what it learned.
//!
//! # Evaluation order: the RHS may collect, so the receiver is read after it
//!
//! The previous static PIC admitted only a safepoint-free RHS because it
//! evaluated the reference, held the receiver in an unrooted register across
//! the RHS, and stored through it. Here the caller evaluates the target FIRST
//! (the reference is evaluated before the value, ECMA-262 §13.15.2), keeps it
//! in an operand root across the RHS (`with_operands_rooted_across`), and
//! hands this emitter the value RE-READ from that root after the RHS. Nothing
//! between that re-read and the slot store can collect: the header loads, the
//! compact-word load and the compares are plain loads, so the handle and the
//! ShapeId this code tests are the receiver's CURRENT ones — after whatever
//! moving collection or shape change the RHS caused. No expression is
//! reordered; only the point at which the receiver's shape is consulted moved,
//! and it moved to where the spec's PutValue consults it anyway.
//!
//! # What the shape compare proves, and why no other header word is read for it
//!
//! * **GC kind** (#10828, rule 3): for a POINTER-tagged value, the u32 at
//!   `+4` equals a live ShapeId only for a `GC_TYPE_OBJECT`.
//! * **Not forwarded**: a forwarding stub stores the new user address in the
//!   first eight payload bytes (`gc::types::set_forwarding_address`), so its
//!   `+4` word is the HIGH half of a user-space address — below `0x0001_0000`
//!   on every supported target, far below the ShapeId floor `0x8000_0000`. A
//!   forwarded cell therefore cannot match a primed word.
//! * **Key present, own, inline, at this slot**: the ShapeId names an
//!   immutable key list and live inline bound, and the word is published only
//!   for an inline slot (`put_value/packed_set.rs`).
//! * **Data, writable, no accessor** (#10824 / #10287, rule 1): descriptor
//!   installs transition the ShapeId; the miss entry publishes only a key the
//!   receiver's per-key descriptor summary proves plain.
//! * **Not frozen / sealed / non-extensible**: the integrity operations mint a
//!   counter-unique semantic generation, and the miss entry refuses them.
//! * **Not deleted**: #10826 makes every delete a shape transition.
//! * **Not a class object or a dictionary**: the shape's `object_kind`.
//! * **Not a Proxy**: a Proxy is a POINTER-tagged id in the handle band, below
//!   `0x100000`, so the small-handle test refuses it before any load.
//!
//! # What the shape does not carry, and is therefore re-read here
//!
//! Two per-object facts are not functions of the ShapeId today, and the hit
//! reads them from the header on every store:
//!
//! * **Receiver kind** — a class-less receiver shares ShapeIds with the
//!   exotic class-less objects whose own slots are not plain data (`URL`,
//!   `Object.prototype`, a typed-array prototype), and a native-module
//!   receiver (`class_id == 0xFFFF_FFFE`) shares them with ordinary objects.
//!   Admitted: a class instance (`class_id` not 0 / `u32::MAX-1` / `u32::MAX`),
//!   or a class-less receiver a birth site marked `OBJ_FLAG_PLAIN_ORDINARY`
//!   with `OBJ_FLAG_TYPED_ARRAY_PROTO` clear.
//! * **The Array-subclass numeric proof** (`OBJ_FLAG_PACKED_NUMERIC_PROOF`) —
//!   a per-object claim that an element prefix is numeric (optionally u32).
//!   Any owner store must retire it first, so a proof-carrying receiver takes
//!   the miss, whose runtime store retires it; later stores hit.
//!
//! # The barrier
//!
//! A store of a JSValue into an object slot owes the GC exactly what
//! `emit_jsvalue_slot_store_pointer_tested` owes it, and uses the same stem
//! (`put.pic`) so the barrier census verifies this site:
//!
//! * pointer-bearing value (`emit_may_carry_heap_pointer_check`, a superset of
//!   the runtime's own test): the string alias demotion, the GC layout note
//!   when the receiver's layout state / typed-layout bit says the note can act
//!   (`_reserved & 0xD000`; a `GC_LAYOUT_UNKNOWN` receiver without a typed
//!   descriptor is fully scanned, and `layout_note_slot` returns at once for
//!   it), and `js_write_barrier_slot_validated_parent` — the remembered-set
//!   insert AND the incremental-mark shading — behind
//!   `emit_parent_may_need_remembering_check` (TENURED parent OR a live
//!   incremental cycle).
//! * a NaN-boxed non-pointer (`undefined`, a boolean, an int32, an inline
//!   string): no barrier, but a typed-layout receiver (`INTACT`) may declare
//!   the slot raw-f64, which such bits contradict, so the note runs there.
//! * a plain double: nothing. It is raw-f64-compatible with every typed slot
//!   and pointer-free (`layout_note_slot`'s #5094 fast `Conforms`).
//!
//! No value is trusted from its static type: the only static skip is LLVM
//! folding these tests for a constant.

use crate::types::{DOUBLE, I1, I16, I32, I64, PTR};

use super::write_barrier::{
    emit_may_carry_heap_pointer_check, emit_parent_may_need_remembering_check,
};
use super::FnCtx;

/// Initial value of `@perry_ic_N_packed_set`. **Must equal
/// `perry_runtime::proxy::put_value::packed_set::PACKED_SET_EMPTY`**; pinned
/// by the runtime's `packed_set_empty_matches_codegen`.
pub(crate) const PACKED_SET_EMPTY: i64 = 0xFFFF_FFFF;

/// The handle band's exclusive ceiling: native registry ids and the Proxy id
/// band live below it, and are never dereferenced. **Must equal
/// `perry_runtime::value::addr_class::HANDLE_BAND_MAX`** (the read path's
/// small-handle test, `handle > 0xFFFFF`, is the same boundary).
const HANDLE_BAND_MAX: u64 = 0x10_0000;

/// `bits - RECEIVER_RANGE_BASE <u RECEIVER_RANGE_SPAN` is "POINTER tag and a
/// payload at or above the handle band" — derived from the tag constant, never
/// typed as a literal.
fn receiver_range_base() -> String {
    ((crate::nanbox::POINTER_TAG | HANDLE_BAND_MAX) as i64).to_string()
}
fn receiver_range_span() -> String {
    ((crate::nanbox::POINTER_MASK + 1 - HANDLE_BAND_MAX) as i64).to_string()
}

/// Ways of the site's cache the emitted code compares after a word miss.
/// **Must equal `perry_runtime::proxy::PACKED_SET_INLINE_WAYS`**; pinned by the
/// runtime's `packed_set_inline_ways_matches_codegen`.
pub(crate) const PACKED_SET_INLINE_WAYS: usize = 4;
/// `OBJ_FLAG_PACKED_NUMERIC_PROOF` (0x80).
const PROOF_FLAG_I16: &str = "128";
/// `OBJ_FLAG_PLAIN_ORDINARY | OBJ_FLAG_TYPED_ARRAY_PROTO` (0x300) and the
/// admitted value of that pair (plain, not a typed-array prototype: 0x200).
const CLASSLESS_ADMIT_MASK_I16: &str = "768";
const CLASSLESS_ADMIT_I16: &str = "512";
/// `GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` (0xD000) as a signed
/// i16: the header states in which a pointer store's layout note can act.
const NOTE_ACTS_FOR_POINTER_I16: &str = "-12288";
/// `GC_OBJ_TYPED_LAYOUT_INTACT` (0x1000): the only state in which a
/// non-pointer, non-double store's note can act.
const TYPED_INTACT_I16: &str = "4096";
/// Top-16 range of the NaN-boxed tags: `SHORT_STRING_TAG` (0x7FF9) through
/// `STRING_TAG` (0x7FFF). Bits in this range are not raw f64s.
const BOXED_TAG_FIRST_TOP16: &str = "32761";
const BOXED_TAG_SPAN: &str = "7";

/// The barrier-census stem. Shared with the census registry
/// (`barrier_stem_census_tests::VERIFIED_BARRIER_STEMS`).
pub(crate) const STORE_IC_STEM: &str = "put.pic";

/// Emit the static-key store. `obj_box` is the receiver RE-READ after the
/// RHS; `value_double` is the stored value and `value_bits` the same value as
/// the caller lowered it to i64 (used only to recognise an SSA constant).
/// Returns the
/// assignment's value (the RHS on a hit, the miss entry's result otherwise).
///
/// A nullish receiver fails the receiver test, and the miss entry's `[[Set]]`
/// throws its TypeError, so the hit path pays nothing for it.
pub(crate) fn emit_static_store_ic(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    property: &str,
    value_double: &str,
    value_bits: &str,
    strict: bool,
) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);

    // The ways cache (lazily allocated by the runtime) and the compact word.
    let cache_site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::inline_cache_global_name(ctx, cache_site);
    ctx.pending_declares
        .push((format!("__ic_decl_{cache_site}"), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());
    let cache_slot_ref = format!("@{cache_name}");

    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
    let strict_i32 = if strict { "1" } else { "0" };

    if crate::codegen::full_outline_ic_enabled() {
        let key_handle = emit_key_handle(ctx, &key_handle_global);
        return ctx.block().call(
            DOUBLE,
            "js_put_value_set_packed_miss",
            &[
                (DOUBLE, obj_box),
                (I64, &key_handle),
                (DOUBLE, value_double),
                (I32, strict_i32),
                (PTR, &cache_slot_ref),
                (PTR, "null"),
            ],
        );
    }

    let packed_site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let packed_name = format!(
        "{}_packed_set",
        super::inline_cache_global_name(ctx, packed_site)
    );
    ctx.typed_parse_rodata.push(format!(
        "@{packed_name} = private global i64 {PACKED_SET_EMPTY}, align 8"
    ));
    let packed_ref = format!("@{packed_name}");

    let tok_idx = ctx.new_block(&format!("{STORE_IC_STEM}.token"));
    let kind_idx = ctx.new_block(&format!("{STORE_IC_STEM}.kind"));
    let class_idx = ctx.new_block(&format!("{STORE_IC_STEM}.class"));
    let classless_idx = ctx.new_block(&format!("{STORE_IC_STEM}.classless"));
    let store_idx = ctx.new_block(&format!("{STORE_IC_STEM}.hit.store"));
    let miss_idx = ctx.new_block(&format!("{STORE_IC_STEM}.miss"));
    let merge_idx = ctx.new_block(&format!("{STORE_IC_STEM}.merge"));
    let tok_label = ctx.block_label(tok_idx);
    let kind_label = ctx.block_label(kind_idx);
    let class_label = ctx.block_label(class_idx);
    let classless_label = ctx.block_label(classless_idx);
    let store_label = ctx.block_label(store_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Receiver: POINTER tag AND payload above the handle band, as ONE unsigned
    // range compare. `t = bits - (POINTER_TAG | 0x100000)` is below
    // `2^48 - 0x100000` exactly when the top sixteen bits are 0x7FFD and the
    // payload is >= 0x100000 (a smaller payload, or any other tag, wraps above
    // the bound). Native registry ids and the Proxy id band live below
    // 0x100000, so neither is ever dereferenced. The handle is `t + 0x100000`.
    let biased = ctx.block().sub(I64, &obj_bits, &receiver_range_base());
    let is_object_pointer = ctx.block().icmp_ult(I64, &biased, &receiver_range_span());
    ctx.block()
        .cond_br(&is_object_pointer, &tok_label, &miss_label);

    // THE shape compare. The compact word and the receiver's ShapeId word are
    // independent loads; the ShapeId load's only use is the compare.
    ctx.current_block = tok_idx;
    let handle = ctx.block().add(I64, &biased, &HANDLE_BAND_MAX.to_string());
    let word = ctx.block().load_atomic_monotonic(I64, &packed_ref, 8);
    let sid_addr = ctx.block().add(I64, &handle, "4");
    let sid_ptr = ctx.block().inttoptr(I64, &sid_addr);
    let sid = ctx.block().load(I32, &sid_ptr);
    let stamp = ctx.block().trunc(I64, &word, I32);
    let shape_eq = ctx.block().icmp_eq(I32, &sid, &stamp);
    let ways_entry_idx = ctx.new_block(&format!("{STORE_IC_STEM}.ways"));
    let ways_entry_label = ctx.block_label(ways_entry_idx);
    ctx.block()
        .cond_br(&shape_eq, &kind_label, &ways_entry_label);
    let mut word_incoming: Vec<(String, String)> = vec![(word, tok_label.clone())];

    // A word miss: compare the first ways of the site's cache (the read path's
    // #7753 structure), each in the word's own format, so a hit on any of them
    // is the same ONE ShapeId compare and flows into the same store. A spill
    // entry is flipped out of the ShapeId range and never matches here. The
    // cache is lazily allocated: a site that has never primed has none.
    ctx.current_block = ways_entry_idx;
    let ways = super::emit_inline_cache_slot(ctx, &cache_name);
    let first_way_idx = ctx.new_block(&format!("{STORE_IC_STEM}.way"));
    let mut way_label = ctx.block_label(first_way_idx);
    ctx.block().cond_br(&ways.present, &way_label, &miss_label);
    let mut way_idx = first_way_idx;
    for w in 0..PACKED_SET_INLINE_WAYS {
        ctx.current_block = way_idx;
        let entry_ptr = ctx.block().gep(I64, &ways.cache, &[(I64, &w.to_string())]);
        let entry = ctx.block().load_atomic_monotonic(I64, &entry_ptr, 8);
        let entry_stamp = ctx.block().trunc(I64, &entry, I32);
        let way_eq = ctx.block().icmp_eq(I32, &sid, &entry_stamp);
        let next_label = if w + 1 < PACKED_SET_INLINE_WAYS {
            way_idx = ctx.new_block(&format!("{STORE_IC_STEM}.way"));
            ctx.block_label(way_idx)
        } else {
            miss_label.clone()
        };
        ctx.block().cond_br(&way_eq, &kind_label, &next_label);
        word_incoming.push((entry, way_label.clone()));
        way_label = next_label;
    }

    // The per-object facts the shape does not carry (see the module doc), as
    // a branch chain rather than one flat predicate (#7883).
    ctx.current_block = kind_idx;
    let word = {
        let incoming: Vec<(&str, &str)> = word_incoming
            .iter()
            .map(|(v, l)| (v.as_str(), l.as_str()))
            .collect();
        ctx.block().phi(I64, &incoming)
    };
    let reserved_addr = ctx.block().sub(I64, &handle, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let proof = ctx.block().and(I16, &reserved, PROOF_FLAG_I16);
    let no_proof = ctx.block().icmp_eq(I16, &proof, "0");
    ctx.block().cond_br(&no_proof, &class_label, &miss_label);

    // `class_id + 2 > 2` (unsigned) is "a class id other than 0, u32::MAX-1
    // (native module) and u32::MAX (never allocated)".
    ctx.current_block = class_idx;
    let class_ptr = ctx.block().inttoptr(I64, &handle);
    let class_id = ctx.block().load(I32, &class_ptr);
    let class_biased = ctx.block().add(I32, &class_id, "2");
    let has_class = ctx.block().icmp_ugt(I32, &class_biased, "2");
    ctx.block()
        .cond_br(&has_class, &store_label, &classless_label);

    ctx.current_block = classless_idx;
    let admit_bits = ctx.block().and(I16, &reserved, CLASSLESS_ADMIT_MASK_I16);
    let admitted = ctx.block().icmp_eq(I16, &admit_bits, CLASSLESS_ADMIT_I16);
    let classless = ctx.block().icmp_eq(I32, &class_id, "0");
    let plain_ok = ctx.block().and(I1, &admitted, &classless);
    ctx.block().cond_br(&plain_ok, &store_label, &miss_label);

    // The store, then the GC's obligations for the bits actually stored.
    ctx.current_block = store_idx;
    let slot = ctx.block().lshr(I64, &word, "32");
    let header_size = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields = ctx.block().add(I64, &handle, &header_size);
    let fields_ptr = ctx.block().inttoptr(I64, &fields);
    let slot_ptr = ctx.block().gep(DOUBLE, &fields_ptr, &[(I64, &slot)]);
    // GC_STORE_AUDIT(BARRIERED): the slot write is unconditional; the
    // bookkeeping below is guarded only by live tests of the stored bits and
    // of the receiver's header.
    ctx.block().store(DOUBLE, value_double, &slot_ptr);
    emit_static_store_ic_bookkeeping(
        ctx,
        &handle,
        &slot,
        &slot_ptr,
        &reserved,
        value_double,
        value_bits,
        "put.pic",
    );
    let hit_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    let key_handle = emit_key_handle(ctx, &key_handle_global);
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_packed_miss",
        &[
            (DOUBLE, obj_box),
            (I64, &key_handle),
            (DOUBLE, value_double),
            (I32, strict_i32),
            (PTR, &cache_slot_ref),
            (PTR, &packed_ref),
        ],
    );
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (value_double, &hit_end_label),
            (&miss_value, &miss_end_label),
        ],
    )
}

/// The GC's obligations after the unconditional slot store. Leaves the
/// current block at the join every path reaches.
///
/// Ordered by what real stores carry, cheapest first:
///
/// 1. **plain double** (top 16 bits neither a NaN-boxed tag nor zero): owes
///    nothing — one shift and two compares in the store block, then the join.
/// 2. `<stem>.classify`: [`emit_may_carry_heap_pointer_check`] (a superset of
///    the runtime's own pointer test) splits pointer-bearing bits into
///    `<stem>.gc_bookkeeping` and the remaining NaN-boxed tags into
///    `<stem>.scalar.tagged`.
/// 3. `<stem>.gc_bookkeeping`: the string alias demotion (a call only for a
///    STRING-tagged value), the layout note behind the header test that says
///    it can act, and the barrier behind the TENURED / incremental-cycle gate.
/// 4. `<stem>.scalar.tagged`: the layout note, only for a typed-layout
///    receiver (a tag contradicts a raw-f64 slot; for the +0.0 / subnormal
///    doubles that also land here the note answers `Conforms`).
///
/// Block names follow the barrier census contract (#8185): `<stem>.barrier`
/// is entered by the generation predicate, `<stem>.gc_bookkeeping` by the
/// value predicate, and the unconditional slot store dominates both.
///
/// `stem` names the blocks and is this site's identity in the barrier census:
/// `scripts/gc_store_site_inventory.py` resolves the literal at every call
/// (`STEM_EMITTER_ARG_INDEX`) and requires it in `VERIFIED_BARRIER_STEMS`.
#[allow(clippy::too_many_arguments)]
fn emit_static_store_ic_bookkeeping(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    slot: &str,
    slot_ptr: &str,
    reserved: &str,
    value_double: &str,
    value_bits_hint: &str,
    stem: &str,
) {
    // The ONE static skip: the stored value is an LLVM constant, so its bits
    // are known exactly in the emitted SSA. A plain double needs nothing, and
    // a constant tag (undefined / true / null) can never carry a pointer.
    // Every other value is classified at run time, from its bits.
    let constant =
        constant_double_bits(value_double).or_else(|| constant_i64_bits(value_bits_hint));
    if constant.is_some_and(is_plain_double_bits) {
        return;
    }
    let tagged_idx = ctx.new_block(&format!("{stem}.scalar.tagged"));
    let tagged_note_idx = ctx.new_block(&format!("{stem}.scalar.note"));
    let done_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping.done"));
    let tagged_label = ctx.block_label(tagged_idx);
    let tagged_note_label = ctx.block_label(tagged_note_idx);
    let done_label = ctx.block_label(done_idx);

    let slot_i32 = ctx.block().trunc(I64, slot, I32);
    let value_bits = ctx.block().bitcast_double_to_i64(value_double);
    let value_bits = value_bits.as_str();

    if constant.is_some() {
        // A constant NaN-boxed tag: pointer-free by construction, so only the
        // typed-layout note can apply.
        ctx.block().br(&tagged_label);
    } else {
        let classify_idx = ctx.new_block(&format!("{stem}.classify"));
        let classify_label = ctx.block_label(classify_idx);
        {
            // Plain double <=> top 16 bits neither in the NaN-boxed tag range
            // [0x7FF9, 0x7FFF] nor zero. Everything the runtime's pointer test
            // or its raw-f64 test could act on has one of the two: a tagged
            // value, or (top 16 bits zero) a bare heap address. The zero half
            // also sends +0.0 and the subnormals to the exact classification
            // below — a superset, so it costs those values time, never
            // correctness — and keeps this test to the one `shr` both halves
            // share.
            let blk = ctx.block();
            let top16 = blk.lshr(I64, value_bits, "48");
            let rel = blk.sub(I64, &top16, BOXED_TAG_FIRST_TOP16);
            let boxed_tag = blk.icmp_ult(I64, &rel, BOXED_TAG_SPAN);
            let top_zero = blk.icmp_eq(I64, &top16, "0");
            let not_plain = blk.or(I1, &boxed_tag, &top_zero);
            blk.cond_br(&not_plain, &classify_label, &done_label);
        }
        ctx.current_block = classify_idx;
        emit_pointer_arm(
            ctx,
            stem,
            handle,
            &slot_i32,
            slot_ptr,
            reserved,
            value_double,
            value_bits,
            &tagged_label,
            &done_label,
        );
    }

    // A NaN-boxed non-pointer (undefined / boolean / int32 / inline string)
    // contradicts a raw-f64 slot of a typed layout, and nothing else.
    ctx.current_block = tagged_idx;
    {
        let blk = ctx.block();
        let intact = blk.and(I16, reserved, TYPED_INTACT_I16);
        let typed = blk.icmp_ne(I16, &intact, "0");
        blk.cond_br(&typed, &tagged_note_label, &done_label);
    }
    ctx.current_block = tagged_note_idx;
    {
        let blk = ctx.block();
        blk.call_void(
            "js_gc_note_slot_layout",
            &[(I64, handle), (I32, &slot_i32), (I64, value_bits)],
        );
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
}

/// The pointer-bearing arm, entered from `<stem>.classify` on
/// `emit_may_carry_heap_pointer_check` (a superset of the runtime's own test,
/// so it can only send a value here that the runtime would ignore).
#[allow(clippy::too_many_arguments)]
fn emit_pointer_arm(
    ctx: &mut FnCtx<'_>,
    stem: &str,
    handle: &str,
    slot_i32: &str,
    slot_ptr: &str,
    reserved: &str,
    value_double: &str,
    value_bits: &str,
    tagged_label: &str,
    done_label: &str,
) {
    let book_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping"));
    let alias_idx = ctx.new_block(&format!("{stem}.string_alias"));
    let layout_gate_idx = ctx.new_block(&format!("{stem}.layout_gate"));
    let note_idx = ctx.new_block(&format!("{stem}.layout_note"));
    let note_done_idx = ctx.new_block(&format!("{stem}.layout_note.done"));
    let book_label = ctx.block_label(book_idx);
    let alias_label = ctx.block_label(alias_idx);
    let layout_gate_label = ctx.block_label(layout_gate_idx);
    let note_label = ctx.block_label(note_idx);
    let note_done_label = ctx.block_label(note_done_idx);
    {
        let blk = ctx.block();
        let may_carry_pointer = emit_may_carry_heap_pointer_check(blk, value_bits);
        blk.cond_br(&may_carry_pointer, &book_label, tagged_label);
    }

    // A uniquely-owned heap string aliased into the slot is demoted to shared
    // (`js_string_addref_if_heap_string` is a no-op for every other tag, so
    // the call is made only for a STRING-tagged value).
    ctx.current_block = book_idx;
    {
        let blk = ctx.block();
        let top16 = blk.lshr(I64, value_bits, "48");
        let is_string = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
        blk.cond_br(&is_string, &alias_label, &layout_gate_label);
    }
    ctx.current_block = alias_idx;
    {
        let blk = ctx.block();
        blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        blk.br(&layout_gate_label);
    }
    ctx.current_block = layout_gate_idx;
    {
        let blk = ctx.block();
        let state = blk.and(I16, reserved, NOTE_ACTS_FOR_POINTER_I16);
        let note_acts = blk.icmp_ne(I16, &state, "0");
        blk.cond_br(&note_acts, &note_label, &note_done_label);
    }
    ctx.current_block = note_idx;
    {
        let blk = ctx.block();
        blk.call_void(
            "js_gc_note_slot_layout",
            &[(I64, handle), (I32, slot_i32), (I64, value_bits)],
        );
        blk.br(&note_done_label);
    }
    ctx.current_block = note_done_idx;
    if crate::codegen::write_barriers_enabled() {
        // Created only when emitted: `PERRY_WRITE_BARRIERS=0` exists to A/B
        // the barrier's cost, and dead IR in one arm makes that comparison lie.
        let barrier_idx = ctx.new_block(&format!("{stem}.barrier"));
        let barrier_label = ctx.block_label(barrier_idx);
        {
            let blk = ctx.block();
            // TENURED parent (remembered set) OR a live incremental cycle
            // (insertion shading) — the runtime barrier's two jobs.
            let needed = emit_parent_may_need_remembering_check(blk, handle);
            blk.cond_br(&needed, &barrier_label, done_label);
        }
        ctx.current_block = barrier_idx;
        {
            let blk = ctx.block();
            let slot_addr = blk.ptrtoint(slot_ptr, I64);
            // The shape compare proved `handle` a live, non-forwarded
            // GC_TYPE_OBJECT, which is the validated-parent entry's contract.
            blk.call_void(
                "js_write_barrier_slot_validated_parent",
                &[(I64, handle), (I64, &slot_addr), (I64, value_bits)],
            );
            blk.br(done_label);
        }
    } else {
        ctx.block().br(done_label);
    }
}

/// Bits of `value` when it is an LLVM double constant as the IR builder spells
/// one (`nanbox::double_literal`: a decimal, or `0x…` for a non-finite /
/// NaN-boxed pattern).
fn constant_double_bits(value: &str) -> Option<u64> {
    if let Some(hex) = value.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok();
    }
    let first = value.as_bytes().first()?;
    if !(first.is_ascii_digit() || *first == b'-') {
        return None;
    }
    value.parse::<f64>().ok().map(f64::to_bits)
}

/// Bits of `value` when it is an i64 literal (the native lowering's form of a
/// constant JSValue).
fn constant_i64_bits(value: &str) -> Option<u64> {
    let first = value.as_bytes().first()?;
    if !(first.is_ascii_digit() || *first == b'-') {
        return None;
    }
    value.parse::<i64>().ok().map(|v| v as u64)
}

/// Bits that the runtime's layout note treats as a raw f64 and the barrier as
/// pointer-free: not a NaN-boxed tag (`0x7FF9..=0x7FFF`) and not a bare heap
/// address (`[0x1000, 2^48)`).
fn is_plain_double_bits(bits: u64) -> bool {
    let top16 = bits >> 48;
    !(0x7FF9..=0x7FFF).contains(&top16) && !(0x1000..(1u64 << 48)).contains(&bits)
}

/// The interned key's `StringHeader*`, loaded in the CURRENT (cold) block: the
/// pool entry is a collector-rewritten global, so every consumer re-reads it.
fn emit_key_handle(ctx: &mut FnCtx<'_>, key_handle_global: &str) -> String {
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64)
}
