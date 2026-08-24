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

/// Words in a per-site `@perry_ic_N` property-read cache global.
///
/// **Must equal `perry_runtime::object::field_get_set::PIC_CACHE_WORDS`** —
/// the runtime writes this memory through a `*mut [i64; PIC_CACHE_WORDS]`, so a
/// smaller global here is an out-of-bounds store. perry-codegen does not depend
/// on perry-runtime (the same reason `INLINE_SLOT_FLOOR` is duplicated in
/// `target_layout`), so the pairing is held by `pic_cache_layout_matches_runtime`
/// here and `pic_cache_words_match_codegen` in the runtime: change one and both
/// fail.
pub(crate) const PIC_CACHE_WORDS: usize = 12;
/// First word of the polymorphic way array (words 0..2 are the MRU entry and
/// word 3 is the gate). Mirrors the runtime's `PIC_WAY_BASE`.
pub(crate) const PIC_WAY_BASE: usize = 4;
/// `(token, slot)` ways beyond the MRU entry; a site resolves `PIC_WAYS + 1`
/// shapes inline. Mirrors the runtime's `PIC_WAYS`.
pub(crate) const PIC_WAYS: usize = 4;
/// Way-state word: `> 0` means at least one way is populated and the compares
/// are worth running; `0` (fresh) and a negative megamorphic countdown
/// both skip them. Mirrors the runtime's `PIC_WAY_STATE`.
pub(crate) const PIC_WAY_STATE: usize = 3;

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
    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
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
        let cache_site = ctx.ic_site_counter;
        ctx.ic_site_counter += 1;
        let cache_name = super::super::inline_cache_global_name(ctx, cache_site);
        ctx.pending_declares
            .push((format!("__ic_decl_{}", cache_site), DOUBLE, vec![]));
        ctx.ic_globals.push(cache_name.clone());
        let cache_ref = format!("@{}", cache_name);
        let key_handle = emit_key_handle(ctx, &key_handle_global);
        let val = ctx.block().call(
            DOUBLE,
            "js_object_get_field_ic",
            &[
                (I64, &obj_bits),
                (I64, &key_handle),
                (I64, &feedback_site_id),
                (PTR, &cache_ref),
            ],
        );
        return Ok(val);
    }

    // Issue #70/#73/#128: guard against non-pointer receivers
    // before the PIC deref. Tag-based check on the unmasked
    // NaN-box: real heap references have high-16-bits POINTER_TAG
    // (0x7FFD) or STRING_TAG (0x7FFF). `AND 0xFFFD` collapses both
    // to 0x7FFD; everything else (undefined/null/bool=0x7FFC,
    // int32=0x7FFE, bigint=0x7FFA, plain f64 like 0.0 globalThis
    // or 3.14, corrupt bit-patterns like 0x00FF_0000_0000 read as
    // a BufferHeader) falls through to the invalid branch and
    // returns undefined safely.
    //
    // Previously used a Darwin mimalloc heap-window check
    // (`> 2 TB && < 128 TB`). On aarch64-linux-android (issue
    // #128) Bionic Scudo allocations live far below 2 TB, so
    // every real object pointer failed the guard and the IC
    // returned undefined — `obj.x` read as NaN everywhere,
    // silently corrupting FFI args and pure-TS field compares.
    // Tag check is platform-independent: same two LLVM ops
    // (`lshr` + `and`) + one `icmp`, branch-predicted taken.
    let obj_tag = ctx.block().lshr(I64, &obj_bits, "48");
    // SSO receiver fast path (Step 1.5 of SSO migration).
    // SHORT_STRING_TAG = 0x7FF9 can't pass the POINTER/STRING
    // check (its masked tag is 0x7FF9, not 0x7FFD) and we
    // can't widen the mask because the PIC fast path's
    // `*(obj_handle + 16)` would read arbitrary memory from
    // the SSO data bits. Instead: check SSO explicitly first,
    // route to a dedicated block that calls the SSO-aware
    // `js_object_get_field_by_name_f64` runtime entry (which
    // handles `.length` directly from the NaN-box length
    // byte and returns `undefined` for other keys).
    // v0.5.747: INT32-tagged class refs (top16 == 0x7FFE) used
    // as PropertyGet receivers. Pre-fix these fell through to
    // the invalid-recv path (returning undefined) because the
    // 0xFFFD-masked tag check (0x7FFE & 0xFFFD = 0x7FFC, not
    // 0x7FFD) treated them as non-pointer values. Drizzle's
    // `is(value, type)` chain depends on `Cls.kind` reads through
    // an Any-typed local. Refs #420 / #618 followup.
    //
    // Note: this also catches plain int32 numeric values (e.g.
    // `(42).property`). The runtime helper's INT32-tag arm at
    // js_object_get_field_by_name returns undefined for any
    // class_id not registered in CLASS_DYNAMIC_PROPS, matching
    // the previous behavior — pure ints have no static fields.
    let obj_tag_masked = ctx.block().and(I64, &obj_tag, "65533"); // 0xFFFD
    let is_valid = ctx.block().icmp_eq(I64, &obj_tag_masked, "32765"); // 0x7FFD
    let sso_idx = ctx.new_block("pget.recv_sso");
    let pic_idx = ctx.new_block("pget.recv_ok");
    let invalid_idx = ctx.new_block("pget.recv_bad");
    let class_ref_idx = ctx.new_block("pget.recv_class_ref");
    let final_merge_idx = ctx.new_block("pget.recv_merge");
    let sso_label = ctx.block_label(sso_idx);
    let pic_label = ctx.block_label(pic_idx);
    let invalid_label = ctx.block_label(invalid_idx);
    let class_ref_label = ctx.block_label(class_ref_idx);
    let final_merge_label = ctx.block_label(final_merge_idx);
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
    // GC_TYPE_OBJECT receiver by construction (#72), so EVERY such read misses
    // to `js_object_get_field_ic_miss` and walks a ladder built for objects —
    // closure-magic deref, buffer and typed-array registry probes, then
    // `js_object_get_field_by_name`'s own dispatch, which decodes the key with
    // `str::from_utf8` again before reaching the string arm. On `pipeline.ts`
    // that one read was ~9% of total run time.
    //
    // Both string tags are disjoint from POINTER_TAG, so serving them here is
    // a pure short-circuit: a primitive string's `length` is non-writable and
    // non-configurable, cannot be shadowed by an own property, and is exactly
    // what the runtime ladder computes. Everything else keeps the tower.
    let inline_string_length = property == "length";
    let strlen_heap_idx = if inline_string_length {
        Some(ctx.new_block("pget.strlen_heap"))
    } else {
        None
    };
    // #7883: the POINTER/STRING test goes FIRST, and the two rare tags are
    // discriminated in a cold block off its false edge. The three tag classes
    // are pairwise disjoint — `is_valid` is `(tag & 0xFFFD) == 0x7FFD`, true
    // only for 0x7FFD/0x7FFF, while SSO is 0x7FF9 and an INT32 class ref is
    // 0x7FFE — so testing them in any order gives the same routing. The old
    // order (SSO, then class-ref, then pointer) put two 16-bit constant
    // materialisations, two compares and two branches in front of every real
    // object receiver: 13 instructions before the PIC on the path that is
    // taken essentially always. Now it is `lshr` + `and` + `cmp` + branch.
    let other_idx = ctx.new_block("pget.recv_other");
    let other_label = ctx.block_label(other_idx);
    let check_class_ref_idx = ctx.new_block("pget.check_class_ref");
    let check_class_ref_label = ctx.block_label(check_class_ref_idx);
    ctx.block().cond_br(&is_valid, &pic_label, &other_label);
    ctx.current_block = other_idx;
    let is_sso = ctx.block().icmp_eq(I64, &obj_tag, "32761"); // 0x7FF9
    ctx.block()
        .cond_br(&is_sso, &sso_label, &check_class_ref_label);
    ctx.current_block = check_class_ref_idx;
    let is_int32_class = ctx.block().icmp_eq(I64, &obj_tag, "32766"); // 0x7FFE
    ctx.block()
        .cond_br(&is_int32_class, &class_ref_label, &invalid_label);

    // Class-ref dispatch: route through the runtime helper which
    // detects INT32 class-ref bits and consults CLASS_DYNAMIC_PROPS
    // for the static field / dynamic IIFE-set property / synthetic
    // `constructor` lookup. Pass full obj_bits (NOT obj_handle —
    // the runtime needs the unmasked top16 to detect the tag).
    ctx.current_block = class_ref_idx;
    let key_handle = emit_key_handle(ctx, &key_handle_global);
    let class_ref_result = ctx.block().call(
        DOUBLE,
        "js_typed_feedback_object_get_field_by_name_f64",
        &[
            (I64, &feedback_site_id),
            (I64, &obj_bits),
            (I64, &key_handle),
        ],
    );
    let class_ref_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    ctx.current_block = pic_idx;
    let observed_key = key_handle_observed.clone().unwrap_or_default();
    crate::expr::emit_typed_feedback_record_call(
        ctx.block(),
        "js_typed_feedback_observe_property_get",
        &[
            (I64, &feedback_site_id),
            (I64, &obj_handle),
            (I64, &observed_key),
        ],
    );

    // Split the heap-string receiver off before the PIC. Placed AFTER the
    // typed-feedback observation on purpose: the site keeps recording every
    // receiver it sees, so a mixed object/string site cannot be mis-profiled
    // as monomorphic-object by the arm that is no longer traced here.
    if let Some(heap_idx) = strlen_heap_idx {
        let strlen_heap_label = ctx.block_label(heap_idx);
        let not_string_idx = ctx.new_block("pget.recv_obj");
        let not_string_label = ctx.block_label(not_string_idx);
        let is_heap_string =
            ctx.block()
                .icmp_eq(I64, &obj_tag, crate::nanbox::STRING_TAG_TOP16_I64);
        ctx.block()
            .cond_br(&is_heap_string, &strlen_heap_label, &not_string_label);
        ctx.current_block = not_string_idx;
    }

    // Monomorphic inline cache. The per-site global holds an authoritative
    // ShapeId token and its cached slot; word 2 is non-identity scratch.
    // The fast path compares the receiver's discriminated ShapeId token to
    // cache[0] and, on match, loads
    // the field directly at obj+ObjectHeader::SIZE+slot*8: no function call, no hash,
    // no linear scan. On miss, calls the slow helper which does the
    // full lookup and primes the cache for next time.
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::super::inline_cache_global_name(ctx, site_id);
    ctx.pending_declares
        .push((format!("__ic_decl_{}", site_id), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());

    // Issue #72: validate the receiver is actually a GC_TYPE_OBJECT
    // before reading its ShapeId. The receiver
    // guard (`obj_handle > 0x100000`) keeps non-pointer NaN-boxes out,
    // but real heap pointers to Arrays/Strings/Buffers all clear that
    // threshold. A chained `obj.rowsRaw.length` (whose static type
    // analysis can't prove `obj.rowsRaw` is an Array — the outer
    // PropertyGet falls into this generic dispatch) hands the array's
    // pointer to this PIC. Reading an ObjectHeader ShapeId from that payload
    // would be invalid. The slow `js_object_get_field_by_name`
    // already routes by `gc_type` (handles Array.length, String.length,
    // Set.size, Buffer.length, Error.message, etc.), so funneling
    // non-OBJECT receivers through the miss handler fixes correctness
    // without giving up the PIC for real objects.
    //
    // Issue #340/#341: small-handle guard. Receivers from
    // native modules (axios, fastify, ioredis, better-sqlite3,
    // ...) are NaN-boxed POINTER values whose lower-48 is a
    // small registry id (1, 2, 3, ...). The PIC fast path
    // below deref's `obj_handle - 8` for the GcHeader byte
    // and `obj_handle + 8` for the ShapeId slot — both
    // SIGSEGV when `obj_handle` is a small int. Funnel
    // small-handle receivers through the slow path so they
    // reach the runtime's `HANDLE_PROPERTY_DISPATCH` table
    // (axios `r.status` / `r.data`, fastify `req.query` /
    // `req.params`, etc.).
    //
    // Threshold matches `js_native_call_method`'s small-handle
    // detection (raw_ptr < 0x100000).
    let cache_ref = format!("@{}", cache_name);
    let is_real_ptr = ctx.block().icmp_ugt(I64, &obj_handle, "1048575"); // 0x100000

    // #7883: the hit/miss/merge blocks are minted here so the guard chain
    // below can BRANCH OUT to the miss on the first failing predicate
    // instead of AND-ing eight of them into one flat `hit`. LLVM if-converts
    // a flat predicate, so every receiver paid every load and every compare
    // even after the very first one had
    // already decided the answer. Each group now ends in its own `cond_br`;
    // the miss block reconstructs what the polymorphic-way compares need
    // through phis (`false`/`0` on the early-exit edges, which is exactly
    // what the flat predicate computed there).
    let hit_idx = ctx.new_block("pic.hit");
    let miss_idx = ctx.new_block("pic.miss");
    // #7907: the two receiver-validation failures get their own landing block
    // so `pic.miss` is dominated by `pic.token`. See the comment on
    // `pic.miss.cold` below for why that is the whole point of this split.
    let cold_idx = ctx.new_block("pic.miss.cold");
    let call_idx = ctx.new_block("pic.miss.call");
    let merge_idx = ctx.new_block("pic.merge");
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let cold_label = ctx.block_label(cold_idx);
    let call_label = ctx.block_label(call_idx);
    let merge_label = ctx.block_label(merge_idx);
    let hdr_idx = ctx.new_block("pic.recv_hdr");
    let hdr_label = ctx.block_label(hdr_idx);
    let tok_idx = ctx.new_block("pic.token");
    let tok_label = ctx.block_label(tok_idx);
    // Small-handle receivers (native-module registry ids) must never be
    // dereferenced. Pre-#7883 they were kept out of the loads by selecting a
    // sentinel address and AND-ing `is_real_ptr` into `hit`; the branch does
    // the same job without putting a `select` (and the sentinel's address
    // materialisation) in front of every real object read.
    // A small-handle receiver can never resolve a way (`way_hit` requires a
    // real object), so it leaves for `pic.miss.cold` and never enters the
    // block the ways live in.
    ctx.block().cond_br(&is_real_ptr, &hdr_label, &cold_label);
    ctx.current_block = hdr_idx;

    // GcHeader sits 8 bytes before the user pointer; obj_type is the
    // first u8 (GC_TYPE_OBJECT=2). Cost: 1 sub + 1 load i8 + 1 cmp
    // i8 + 1 and i1 — the cond_br's `is_object` operand is folded
    // into the existing branch instruction by LLVM. Branch-predicted
    // taken since real PropertyGet receivers are objects.
    let gc_type_addr = ctx.block().sub(I64, &obj_handle, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let is_object = ctx.block().icmp_eq(I8, &gc_type, "2");

    // Closures and RegExp values have distinct GC kinds. Every
    // `GC_TYPE_OBJECT` payload is therefore an ObjectHeader and its ShapeId is
    // the remaining exact layout discriminator.

    // #6080: a receiver that has ever had a property/accessor descriptor
    // installed (`Object.defineProperty`) needs descriptor-aware dispatch —
    // an accessor must fire on reads, a non-writable slot must reject stores.
    // The PIC hit path is a raw slot load: if the site was primed on a plain
    // data property and `defineProperty` later converts that key to a getter
    // (or a different descriptor), `keys_array` is unchanged, so the stale
    // hit path would return the raw slot and bypass the getter entirely.
    // OBJ_FLAG_HAS_DESCRIPTORS lives in the GcHeader `_reserved` i16 at
    // offset -6; force a miss (→ `js_object_get_field_ic_miss`, which honors
    // descriptors) whenever it is set. Mirrors the guard in
    // `class_field_inline_guard.rs`. Cost: 1 sub + load i16 + and + cmp,
    // folded into the existing `hit` cond_br.
    let reserved_addr = ctx.block().sub(I64, &obj_handle, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(crate::types::I16, &reserved_ptr);
    let has_desc = ctx.block().and(crate::types::I16, &reserved, "2048"); // OBJ_FLAG_HAS_DESCRIPTORS (0x800)
    let no_desc = ctx.block().icmp_eq(crate::types::I16, &has_desc, "0");
    let is_object = ctx.block().and(I1, &is_object, &no_desc);

    // #7883: first exit. The header predicates above are kept as one flat
    // `and` on purpose — they are loads from the same cache line and LLVM
    // fuses their compares into a `ccmp` chain, which is
    // cheaper than four branches. What was NOT worth folding is everything
    // below: the ShapeId load and token select hang off the same predicate,
    // so a non-object receiver used to execute
    // them before the flat `hit` could reject it.
    //
    // #7907: the false edge goes to `pic.miss.cold`, not `pic.miss` — a
    // receiver that is not a plain descriptor-free `ObjectHeader` fails
    // `way_hit` by construction, so consulting the ways for it was always dead
    // work, and keeping it out is what lets `pic.miss` reuse this block's
    // values instead of re-deriving them.
    ctx.block().cond_br(&is_object, &tok_label, &cold_label);
    ctx.current_block = tok_idx;

    // The receiver token is derived solely from its authoritative ShapeId.
    // Invalid/unstamped payloads miss closed.
    // #8113: the ShapeId word moved from header offset 8 to 4.
    let pcid_addr = ctx.block().add(I64, &obj_handle, "4");
    let pcid_ptr = ctx.block().inttoptr(I64, &pcid_addr);
    let pcid = ctx.block().load(I32, &pcid_ptr);
    let pcid64 = ctx.block().zext(I32, &pcid, I64);
    // PIC_ID_TOKEN_BIT = 1 << 62. The token is formed UNCONDITIONALLY — the
    // in-range test the emitted code used to run first
    // (`(pcid - 0x8000_0000) <u 0x4000_0000`, then a `select` to zero, then a
    // separate non-zero compare) was redundant against the cache compare below,
    // and cost six AArch64 instructions on the hit path of EVERY generic
    // property read.
    //
    // # Why the range test was implied
    //
    // `pic_prime_get` is the only writer of word 0 (`js_put_value_set_ic_miss`
    // writes a *different*, set-side cache), and it is only ever handed
    // `object_shape_stamp(obj) | PIC_ID_TOKEN_BIT` — and `object_shape_stamp`
    // answers `0` for anything outside `SHAPE_ID_BASE..SHAPE_ID_END`. So a
    // cached token's low 32 bits are either a *valid ShapeId* or *zero*:
    //
    // * cached low32 is a valid ShapeId ⇒ `pcid == cached_low32` puts `pcid`
    //   inside the id range, which is exactly what `is_stamp` tested. An equal
    //   token therefore proves the receiver carries that shape, as before.
    // * cached low32 is zero (primed by an unstamped receiver) ⇒ the only
    //   `pcid` that could alias it is `0`, and `pcid != 0` below excludes it.
    //   That single compare replaces the range test: it is what keeps a
    //   `parent_class_id == 0` receiver — a class instance with no parent, or
    //   an `Object.create(proto)` result with no own string props (#809) —
    //   from spuriously hitting the empty slot instead of taking the
    //   prototype-chain walk in `js_object_get_field_by_name`.
    //
    // A receiver whose `parent_class_id` holds a real (non-shape) parent class
    // id keeps missing exactly as it did: its token is `id | bit62`, and no
    // cached token can ever carry a non-shape low32.
    //
    // `token_nonnull` keeps its old NAME and its old JOB (#809 — a keyless
    // receiver must reach `js_object_get_field_by_name`'s prototype-chain walk
    // instead of hitting an empty cache), only now it tests the stamp word
    // rather than the derived token. The ways below AND it in for the same
    // reason: a way primed from an unstamped receiver holds exactly
    // `PIC_ID_TOKEN_BIT`, which is what a `pcid == 0` receiver would compute.
    let token = ctx.block().or(I64, &pcid64, "4611686018427387904");
    let token_nonnull = ctx.block().icmp_ne(I32, &pcid, "0");

    // Load the cached token from the per-site global.
    let cache_keys_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_token = ctx.block().load(I64, &cache_keys_ptr);
    let token_eq = ctx.block().icmp_eq(I64, &token, &cached_token);
    let hit = ctx.block().and(I1, &token_eq, &token_nonnull);

    ctx.block().cond_br(&hit, &hit_label, &miss_label);

    // `js_object_get_field_ic_miss` primes only slots below the descriptor's
    // exact `live_inline_slot_count`. ShapeIds are never reused, so an exact
    // token hit permanently proves that the cached slot remains live and
    // makes the raw load below safe without a compatibility-header bound.
    ctx.current_block = hit_idx;
    let cache_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let slot = ctx.block().load(I64, &cache_slot_ptr);
    crate::expr::emit_typed_feedback_record_call(
        ctx.block(),
        "js_typed_feedback_record_guard_pass",
        &[(I64, &feedback_site_id)],
    );
    let offset = ctx.block().shl(I64, &slot, "3");
    // arm64_32 watchOS: the object fields region begins at
    // `size_of::<ObjectHeader>()` past the user pointer — 16 on LP64 and
    // padded ILP32 since #8047. Derive it from the target triple.
    let obj_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let base = ctx.block().add(I64, &obj_handle, &obj_header_size);
    let field_addr = ctx.block().add(I64, &base, &offset);
    let field_ptr = ctx.block().inttoptr(I64, &field_addr);
    let val_hit = ctx.block().load(DOUBLE, &field_ptr);
    let hit_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // PIC miss on the MRU entry — before paying for the call, try the
    // polymorphic ways (#7753).
    //
    // `js_object_get_field_ic_miss` is not a cheap fallback: it re-derives the
    // receiver kind from scratch (proxy band, closure magic, registered-buffer
    // and typed-array registries, small-handle dispatch), reads the
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
    // Its only predecessor is `pic.token` after the MRU token did not match.
    // The exact descriptor identity proves cached-slot bounds, so `token` and
    // `token_nonnull` are everything the way compares need.
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
    //
    // Sending the two validation failures to `pic.miss.cold` instead is what
    // establishes the dominance. Nothing about the predicate changed: a
    // receiver that fails either check also fails `way_hit` (which ANDs
    // `is_object` in), so it could never have resolved a way — the compares
    // were dead work for it.
    ctx.current_block = miss_idx;
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
    // `token_nonnull` is the value `pic.token` computed, from the same memory
    // with no intervening store, so the predicate is unchanged.
    let mut way_hit = token_nonnull.clone();
    // Reduced as a BALANCED TREE, not as a left fold. At most one way can hold
    // a given token (`pic_prime_get` evicts a duplicate before it writes one,
    // and a zero token is excluded by `token_nonnull`), so the association is
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
    way_hit = ctx.block().and(I1, &way_hit, &way_any);
    let way_load_idx = ctx.new_block("pic.way.load");
    let way_load_label = ctx.block_label(way_load_idx);
    ctx.block().cond_br(&way_hit, &way_load_label, &call_label);

    ctx.current_block = way_load_idx;
    let way_offset = ctx.block().shl(I64, &way_slot, "3");
    let way_base = ctx.block().add(I64, &obj_handle, &obj_header_size);
    let way_field_addr = ctx.block().add(I64, &way_base, &way_offset);
    let way_field_ptr = ctx.block().inttoptr(I64, &way_field_addr);
    let val_way = ctx.block().load(DOUBLE, &way_field_ptr);
    let way_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // #7907: receiver-validation failure. `way_hit` requires a real pointer to
    // a plain descriptor-free `ObjectHeader`, so a receiver that got here can
    // never match a way — it goes straight to the handler, which reproduces the
    // whole ladder anyway (proxy band, closure magic, buffer/typed-array
    // registries, small-handle dispatch). The typed-feedback counters are the
    // same two `pic.miss` records on the same edges, so the feedback signal is
    // byte-identical to what the merged block reported.
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

    // PIC miss: slow path with cache population.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let miss_key_handle = emit_key_handle(ctx, &key_handle_global);
    let val_miss = ctx.block().call(
        DOUBLE,
        "js_object_get_field_ic_miss",
        &[
            (I64, &obj_handle),
            (I64, &miss_key_handle),
            (PTR, &cache_ref),
        ],
    );
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Merge PIC hit + way hit + miss, then jump to the outer recv-valid merge.
    ctx.current_block = merge_idx;
    let pic_val = ctx.block().phi(
        DOUBLE,
        &[
            (&val_hit, &hit_end_label),
            (&val_way, &way_end_label),
            (&val_miss, &miss_end_label),
        ],
    );
    let pic_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    // Invalid receiver: per JS spec, `undefined` and `null`
    // throw a TypeError; other non-pointer tags (int32, bool,
    // plain f64, bigint) should auto-box and look up via the
    // primitive's prototype. Perry doesn't implement primitive
    // auto-boxing yet, so non-nullish primitives continue to
    // return `undefined` to preserve existing behavior.
    //
    // Issue #462: bare `obj.foo` against TAG_UNDEFINED /
    // TAG_NULL silently returned undefined, which masked
    // unimplemented-API bugs (e.g. `crypto.subtle.encrypt(...)`
    // ran to completion as a chain of no-ops). Funnel the
    // nullish receiver into the runtime helper which prints a
    // node-shaped diagnostic and aborts.
    ctx.current_block = invalid_idx;
    let is_undef = ctx
        .block()
        .icmp_eq(I64, &obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
    let is_null = ctx
        .block()
        .icmp_eq(I64, &obj_bits, crate::nanbox::TAG_NULL_I64);
    let is_nullish = ctx.block().or(I1, &is_undef, &is_null);
    let throw_idx = ctx.new_block("pget.throw_nullish");
    let undef_idx = ctx.new_block("pget.recv_undef_return");
    let throw_label = ctx.block_label(throw_idx);
    let undef_label = ctx.block_label(undef_idx);
    ctx.block().cond_br(&is_nullish, &throw_label, &undef_label);

    // Throw path: helper aborts the process; block ends with
    // `unreachable` because the helper's `-> !` return is
    // not visible to LLVM.
    ctx.current_block = throw_idx;
    let prop_entry = ctx.strings.entry(key_idx);
    let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
    let prop_len_str = prop_entry.byte_len.to_string();
    let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
    ctx.block().call_void(
        "js_throw_type_error_property_access",
        &[
            (I32, &is_null_i32),
            (PTR, &prop_bytes_global),
            (I64, &prop_len_str),
        ],
    );
    ctx.block().unreachable();

    // Undef-return path: existing fall-through for non-nullish
    // invalid receivers. Route through the runtime helper first
    // so non-pointer typed shapes can still report a sensible
    // value when the runtime knows what they are. Today this
    // unblocks Date `.constructor` (Date stores as a raw f64
    // timestamp, so the codegen receiver-tag check at line ~4212
    // rejects it as non-pointer — yet the runtime's
    // `js_object_get_field_by_name_f64` recognizes the bit
    // pattern via `DATE_REGISTRY` and returns the global Date
    // constructor closure). Date-fns `constructFrom` blocker.
    ctx.current_block = undef_idx;
    let undef_key_handle = emit_key_handle(ctx, &key_handle_global);
    let undef_val = ctx.block().call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &undef_key_handle)],
    );
    let invalid_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    // SSO receiver: dispatch directly to the runtime by-name
    // helper, which reads `.length` inline from the NaN-box
    // payload and returns `undefined` for other keys. Bypasses
    // the PIC entirely (PIC would read garbage memory). The
    // key handle has already been extracted above.
    ctx.current_block = sso_idx;
    let sso_val = if inline_string_length {
        // `.length` of an SSO string is the length byte in bits 40..47 of the
        // NaN-box itself — the same extract `js_object_get_field_by_name_f64`
        // performs, minus the call and the key decode.
        let len_shifted = ctx.block().lshr(I64, &obj_bits, "40");
        let len_byte = ctx.block().and(I64, &len_shifted, "255");
        ctx.block().uitofp(I64, &len_byte, DOUBLE)
    } else {
        let sso_key_handle = emit_key_handle(ctx, &key_handle_global);
        ctx.block().call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &obj_bits), (I64, &sso_key_handle)],
        )
    };
    let sso_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    // Heap string `.length`: `utf16_len` is the leading `u32` of
    // `StringHeader` — the identical load the proven-string lowering in
    // `property_get.rs` emits (`strlen.heap`). `safe_load_i32_from_ptr`
    // keeps a sub-page handle off the load.
    let strlen_heap_arm = if let Some(heap_idx) = strlen_heap_idx {
        ctx.current_block = heap_idx;
        let len_i32 = ctx.block().safe_load_i32_from_ptr(&obj_handle);
        let heap_len = ctx.block().uitofp(I32, &len_i32, DOUBLE);
        let heap_end_label = ctx.block().label.clone();
        ctx.block().br(&final_merge_label);
        Some((heap_len, heap_end_label))
    } else {
        None
    };

    // Outer merge joins PIC result + invalid-receiver undefined
    // + SSO result + class-ref dispatch result (+ heap-string `.length`).
    ctx.current_block = final_merge_idx;
    let mut incoming: Vec<(&str, &str)> = vec![
        (&pic_val, &pic_end_label),
        (&undef_val, &invalid_end_label),
        (&sso_val, &sso_end_label),
        (&class_ref_result, &class_ref_end_label),
    ];
    if let Some((heap_len, heap_end_label)) = strlen_heap_arm.as_ref() {
        incoming.push((heap_len, heap_end_label));
    }
    Ok(ctx.block().phi(DOUBLE, &incoming))
}
