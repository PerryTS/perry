//! Generic monomorphic-IC property-get dispatch extracted from
//! `property_get.rs`.
//!
//! Pure mechanical move — body is the verbatim tail of the general catch-all
//! arm (the receiver-tag guard + SSO/class-ref/PIC/invalid diamond), lifted
//! into its own function.

use super::*;

use anyhow::Result;
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_numeric_typed_array_class, is_set_expr, is_string_expr,
    is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// The generic per-site monomorphic inline-cache dispatch for `obj.property`.
/// This is the fall-through tail of the general catch-all arm: all earlier
/// specializations have been ruled out.
pub(crate) fn lower_generic_property_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
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
        let cache_name = format!("perry_ic_{}", cache_site);
        ctx.pending_declares
            .push((format!("__ic_decl_{}", cache_site), DOUBLE, vec![]));
        ctx.ic_globals.push(cache_name.clone());
        let cache_ref = format!("@{}", cache_name);
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
    let is_sso = ctx.block().icmp_eq(I64, &obj_tag, "32761"); // 0x7FF9
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
    let is_int32_class = ctx.block().icmp_eq(I64, &obj_tag, "32766"); // 0x7FFE
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
    // Three-step branch: first check SSO, then class-ref, then
    // pointer-validity. Inverse branches funnel into invalid_idx.
    let pic_or_invalid_idx = ctx.new_block("pget.check_ptr");
    let pic_or_invalid_label = ctx.block_label(pic_or_invalid_idx);
    let check_class_ref_idx = ctx.new_block("pget.check_class_ref");
    let check_class_ref_label = ctx.block_label(check_class_ref_idx);
    ctx.block()
        .cond_br(&is_sso, &sso_label, &check_class_ref_label);
    ctx.current_block = check_class_ref_idx;
    ctx.block()
        .cond_br(&is_int32_class, &class_ref_label, &pic_or_invalid_label);
    ctx.current_block = pic_or_invalid_idx;
    ctx.block().cond_br(&is_valid, &pic_label, &invalid_label);

    // Class-ref dispatch: route through the runtime helper which
    // detects INT32 class-ref bits and consults CLASS_DYNAMIC_PROPS
    // for the static field / dynamic IIFE-set property / synthetic
    // `constructor` lookup. Pass full obj_bits (NOT obj_handle —
    // the runtime needs the unmasked top16 to detect the tag).
    ctx.current_block = class_ref_idx;
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
    ctx.block().call_void(
        "js_typed_feedback_observe_property_get",
        &[
            (I64, &feedback_site_id),
            (I64, &obj_handle),
            (I64, &key_handle),
        ],
    );

    // Issue #51: monomorphic inline cache. Per-site 16-byte global
    // holds [cached_keys_array_ptr, cached_slot_index]. The fast path
    // compares obj->keys_array (offset 16) to cache[0]; on match,
    // loads the field directly at obj+24+slot*8 — no function call,
    // no hash, no linear scan. On miss, calls the slow helper which
    // does the full lookup and primes the cache for next time.
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = format!("perry_ic_{}", site_id);
    ctx.pending_declares
        .push((format!("__ic_decl_{}", site_id), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());

    // Issue #72: validate the receiver is actually a GC_TYPE_OBJECT
    // before treating offset 16 as `keys_array`. The v0.5.78 receiver
    // guard (`obj_handle > 0x100000`) keeps non-pointer NaN-boxes out,
    // but real heap pointers to Arrays/Strings/Buffers all clear that
    // threshold. A chained `obj.rowsRaw.length` (whose static type
    // analysis can't prove `obj.rowsRaw` is an Array — the outer
    // PropertyGet falls into this generic dispatch) hands the array's
    // pointer to this PIC. For an Array, offset 16 is element[1]; on
    // a freshly-allocated array element[1] is zero, the per-site
    // cache global is zero-initialized, so the keys_val comparison
    // falsely "hits" and the hit-path loads (obj+24+slot*8) — i.e.
    // element[2] — as the field value, returning 0 instead of
    // dispatching `.length`. The slow `js_object_get_field_by_name`
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
    // and `obj_handle + 16` for the keys_array slot — both
    // SIGSEGV when `obj_handle` is a small int. Funnel
    // small-handle receivers through the slow path so they
    // reach the runtime's `HANDLE_PROPERTY_DISPATCH` table
    // (axios `r.status` / `r.data`, fastify `req.query` /
    // `req.params`, etc.).
    //
    // Threshold matches `js_native_call_method`'s small-handle
    // detection (raw_ptr < 0x100000) and `js_object_get_field_by_name`'s
    // post-#340 fix that calls HANDLE_PROPERTY_DISPATCH for
    // these receivers.
    // Issue #340/#341: small-handle guard. Receivers from
    // native modules (axios, fastify, ioredis, better-sqlite3,
    // ...) are NaN-boxed POINTER values whose lower-48 is a
    // small registry id (1, 2, 3, ...). The PIC fast path
    // below deref's `obj_handle - 8` for the GcHeader byte
    // and `obj_handle + 16` for the keys_array slot — both
    // SIGSEGV when `obj_handle` is a small int. Use a select
    // to swap in a known-safe address (the per-site cache
    // global itself) for the load, then AND `is_real_ptr`
    // into the hit predicate so handle receivers cleanly
    // miss to the slow path. The slow path
    // (`js_object_get_field_ic_miss` →
    // `js_object_get_field_by_name`) routes handles to
    // `HANDLE_PROPERTY_DISPATCH` (axios `r.status` / `r.data`,
    // fastify `req.query`, etc.).
    //
    // Threshold matches `js_native_call_method`'s small-handle
    // detection (raw_ptr < 0x100000).
    let is_real_ptr = ctx.block().icmp_ugt(I64, &obj_handle, "1048575"); // 0x100000

    // Sentinel address: the per-site cache global itself —
    // always valid, 16-byte aligned, and its bytes don't
    // match GC_TYPE_OBJECT (=2) or an active keys_array, so
    // the IC will cleanly miss when we substitute it for a
    // small handle.
    let cache_ref = format!("@{}", cache_name);
    let cache_addr = ctx.block().ptrtoint(&cache_ref, I64);
    let safe_obj_handle = ctx
        .block()
        .select(I1, &is_real_ptr, I64, &obj_handle, &cache_addr);

    // GcHeader sits 8 bytes before the user pointer; obj_type is the
    // first u8 (GC_TYPE_OBJECT=2). Cost: 1 sub + 1 load i8 + 1 cmp
    // i8 + 1 and i1 — the cond_br's `is_object` operand is folded
    // into the existing branch instruction by LLVM. Branch-predicted
    // taken since real PropertyGet receivers are objects.
    let gc_type_addr = ctx.block().sub(I64, &safe_obj_handle, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let gc_type_ok = ctx.block().icmp_eq(I8, &gc_type, "2");
    let is_object = ctx.block().and(I1, &is_real_ptr, &gc_type_ok);

    // Issue #618: closures share GC_TYPE_OBJECT but their offset+16
    // is a capture slot, not `keys_array`. The PIC's keys_val ==
    // cached_keys check would spuriously hit (per-site cache global
    // is zero-initialized; capture[0] of a 0-capture wrapper is also
    // often zero) and the hit path would load garbage from the
    // capture region. Detect CLOSURE_MAGIC at +12 and force the
    // PIC to miss for closures so the read routes through
    // `js_object_get_field_ic_miss` → `js_object_get_field_by_name`,
    // which dispatches closure dynamic-prop reads via the
    // `CLOSURE_DYNAMIC_PROPS` side-table.
    let magic_addr = ctx.block().add(I64, &safe_obj_handle, "12");
    let magic_ptr = ctx.block().inttoptr(I64, &magic_addr);
    let magic_val = ctx.block().load(I32, &magic_ptr);
    // CLOSURE_MAGIC = 0x434C4F53 (4 bytes "CLOS" little-endian).
    let is_closure = ctx.block().icmp_eq(I32, &magic_val, "1129268819");
    let not_closure = ctx.block().xor(I1, &is_closure, "true");
    let is_object = ctx.block().and(I1, &is_object, &not_closure);

    // Issue #637: RegExpHeader / PromiseHeader / MapHeader / SetHeader
    // / TypedArrayHeader / ... all share GC_TYPE_OBJECT but have
    // different layouts than ObjectHeader. The first u32 of an
    // ObjectHeader is `object_type = OBJECT_TYPE_REGULAR (=1)`;
    // for these other headers the first 4 bytes are part of a
    // pointer or method table, almost never 1. Without this check,
    // a PIC site that learned a real ObjectHeader's [keys_array,
    // slot] cache could spuriously hit on a regex/promise/etc.
    // whose offset-16 happens to match (e.g. both null flags_ptr
    // and uninitialized cache[0] are 0), and the hit path would
    // load garbage from offset 24 of the non-Object header.
    // Specific repro: `function f(): any { ... return new
    // RegExp(...) } const r = f(); r.source` — fast path returns
    // garbage f64 instead of routing through `js_regexp_get_source`.
    let object_type_ptr = ctx.block().inttoptr(I64, &safe_obj_handle);
    let object_type = ctx.block().load(I32, &object_type_ptr);
    let object_type_ok = ctx.block().icmp_eq(I32, &object_type, "1");
    let is_object = ctx.block().and(I1, &is_object, &object_type_ok);

    // Load obj->keys_array at offset 16 of ObjectHeader.
    let keys_addr = ctx.block().add(I64, &safe_obj_handle, "16");
    let keys_ptr_p = ctx.block().inttoptr(I64, &keys_addr);
    let keys_val = ctx.block().load(I64, &keys_ptr_p);

    // Load cached keys_array from the per-site global.
    let cache_keys_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_keys = ctx.block().load(I64, &cache_keys_ptr);
    let keys_eq = ctx.block().icmp_eq(I64, &keys_val, &cached_keys);
    // #809: an object with `keys_array == null` (e.g. an
    // `Object.create(proto)` result, or any object with no own
    // string props) has no cacheable own-slot. The per-site cache
    // global is zero-initialized, so `keys_val (0) == cached_keys
    // (0)` spuriously "hits" and the hit path returns the empty
    // slot[0] — never invoking the miss handler, so the runtime's
    // prototype-chain walk in `js_object_get_field_by_name` is
    // skipped and `Object.create(P).m()` reads `undefined`. Require
    // a non-null keys_array for a hit so keyless receivers fall to
    // the slow path (which resolves inherited props correctly).
    let keys_nonnull = ctx.block().icmp_ne(I64, &keys_val, "0");
    let hit_keys = ctx.block().and(I1, &is_object, &keys_eq);
    let hit = ctx.block().and(I1, &hit_keys, &keys_nonnull);

    let hit_idx = ctx.new_block("pic.hit");
    let miss_idx = ctx.new_block("pic.miss");
    let merge_idx = ctx.new_block("pic.merge");
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&hit, &hit_label, &miss_label);

    // PIC hit: direct field load.
    ctx.current_block = hit_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_guard_pass",
        &[(I64, &feedback_site_id)],
    );
    let cache_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let slot = ctx.block().load(I64, &cache_slot_ptr);
    let offset = ctx.block().shl(I64, &slot, "3");
    // arm64_32 watchOS: the object fields region begins at
    // `size_of::<ObjectHeader>()` past the user pointer — 24 on 64-bit, 20 on
    // ILP32 (the trailing `keys_array` pointer is 4 bytes there). A hardcoded
    // 24 would read every cached property 4 bytes off on a 32-bit watch. Derive
    // it from the target triple (no-op on 64-bit; see `target_layout`).
    let obj_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let base = ctx.block().add(I64, &obj_handle, &obj_header_size);
    let field_addr = ctx.block().add(I64, &base, &offset);
    let field_ptr = ctx.block().inttoptr(I64, &field_addr);
    let val_hit = ctx.block().load(DOUBLE, &field_ptr);
    let hit_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // PIC miss: slow path with cache population.
    ctx.current_block = miss_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_guard_fail",
        &[(I64, &feedback_site_id)],
    );
    ctx.block().call_void(
        "js_typed_feedback_record_fallback_call",
        &[(I64, &feedback_site_id)],
    );
    let val_miss = ctx.block().call(
        DOUBLE,
        "js_object_get_field_ic_miss",
        &[(I64, &obj_handle), (I64, &key_handle), (PTR, &cache_ref)],
    );
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Merge PIC hit + miss, then jump to the outer recv-valid merge.
    ctx.current_block = merge_idx;
    let pic_val = ctx.block().phi(
        DOUBLE,
        &[(&val_hit, &hit_end_label), (&val_miss, &miss_end_label)],
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
    let undef_val = ctx.block().call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &key_handle)],
    );
    let invalid_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    // SSO receiver: dispatch directly to the runtime by-name
    // helper, which reads `.length` inline from the NaN-box
    // payload and returns `undefined` for other keys. Bypasses
    // the PIC entirely (PIC would read garbage memory). The
    // key handle has already been extracted above.
    ctx.current_block = sso_idx;
    let sso_val = ctx.block().call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &key_handle)],
    );
    let sso_end_label = ctx.block().label.clone();
    ctx.block().br(&final_merge_label);

    // Outer merge joins PIC result + invalid-receiver undefined
    // + SSO result + class-ref dispatch result.
    ctx.current_block = final_merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&pic_val, &pic_end_label),
            (&undef_val, &invalid_end_label),
            (&sso_val, &sso_end_label),
            (&class_ref_result, &class_ref_end_label),
        ],
    ))
}
