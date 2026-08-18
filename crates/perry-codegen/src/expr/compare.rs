//! Comparison operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{CompareOp, Expr};

use crate::nanbox::POINTER_MASK_I64;
use crate::rooting::with_operands_rooted;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_bool_expr,
    is_numeric_expr, is_string_expr,
};
use crate::types::{DOUBLE, I1, I32, I64, I8};

use super::{unbox_str_handle, unbox_to_i64, FnCtx};

/// Repsel Phase 3a shared dispatch for the canonical-Str compare arms:
/// lower both operands' bits, branch on "both heap `STRING_TAG`", call
/// `heap_fn(handle, handle)` on the hot arm and `boxed_fn(box, box)` on the
/// mixed/SSO/lie arm, and phi the i32 result. The caller applies its own
/// predicate tail (`!= 0` select for equality, signed compare for
/// relational).
fn canonical_str_cmp_dispatch(
    ctx: &mut FnCtx<'_>,
    l: &str,
    r: &str,
    heap_fn: &str,
    boxed_fn: &str,
    prefix: &str,
) -> String {
    let l_bits = ctx.block().bitcast_double_to_i64(l);
    let r_bits = ctx.block().bitcast_double_to_i64(r);
    let l_tag = ctx.block().lshr(I64, &l_bits, "48");
    let r_tag = ctx.block().lshr(I64, &r_bits, "48");
    let l_heap = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let r_heap = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let both_heap = ctx.block().and(crate::types::I1, &l_heap, &r_heap);

    let heap_idx = ctx.new_block(&format!("{prefix}.heap"));
    let boxed_idx = ctx.new_block(&format!("{prefix}.boxed"));
    let merge_idx = ctx.new_block(&format!("{prefix}.merge"));
    let heap_label = ctx.block_label(heap_idx);
    let boxed_label = ctx.block_label(boxed_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&both_heap, &heap_label, &boxed_label);

    ctx.current_block = heap_idx;
    let l_handle = ctx.block().and(I64, &l_bits, POINTER_MASK_I64);
    let r_handle = ctx.block().and(I64, &r_bits, POINTER_MASK_I64);
    let res_heap = ctx
        .block()
        .call(I32, heap_fn, &[(I64, &l_handle), (I64, &r_handle)]);
    let heap_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = boxed_idx;
    let res_boxed = ctx.block().call(I32, boxed_fn, &[(DOUBLE, l), (DOUBLE, r)]);
    let boxed_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I32, &[(&res_heap, &heap_pred), (&res_boxed, &boxed_pred)])
}

/// `StringHeader` field offsets, duplicated from
/// `perry-runtime::string::STRING_HEADER_ABI_MATCHES_CODEGEN` (which asserts
/// them at the definition, so a layout change fails the runtime build rather
/// than silently miscompiling these loads). The same three numbers
/// `lower_string_method/char_code_at.rs` pins.
const STRING_HEADER_BYTE_LEN_OFFSET: &str = "4";
const STRING_HEADER_SIZE: usize = 20;

/// The SSO (`SHORT_STRING_TAG`) immediate for `bytes`, or `None` when the
/// literal is too long to have one. The encoding is canonical — length in bits
/// 40..=47, bytes little-endian in bits 0..=39, everything else zero — which is
/// what `JSValue::try_short_string` builds and what `js_jsvalue_equals`'s "both
/// SSO ⇒ the bits decide" fast path already relies on.
pub(super) fn sso_immediate(bytes: &[u8]) -> Option<u64> {
    if bytes.len() > 5 {
        return None;
    }
    let mut payload = 0u64;
    for (i, &b) in bytes.iter().enumerate() {
        payload |= (b as u64) << (i * 8);
    }
    Some(crate::nanbox::SHORT_STRING_TAG | ((bytes.len() as u64) << 40) | payload)
}

/// LLVM `i8` literals are signed, so a byte >= 0x80 must be written in its
/// two's-complement form.
pub(super) fn i8_literal(b: u8) -> String {
    (b as i8).to_string()
}

/// Inline ECMAScript `===` against a compile-time string literal.
///
/// The motivating shape is a tree-walking interpreter's tag dispatch —
/// `n.kind === "num"`, `n.op === "+"` — where the operand is `any`-typed, so
/// every comparison became a `js_eq` -> `js_jsvalue_equals` call pair (~21% of
/// `gc-handoff/apps/interp.ts`). The literal side makes the dispatch decidable
/// inline, because *both* of a string's runtime representations are known at
/// compile time:
///
/// * the pooled heap `StringHeader` — one per literal per module, see
///   `crate::strings` — so **pointer identity** settles the true case in one
///   `icmp`. Every `{ kind: "num" }` object literal stores that same pooled
///   pointer, and GC evacuation rewrites the pool root and the object slot
///   together, so identity survives collection;
/// * the SSO immediate, a compile-time constant for literals of <= 5 bytes.
///   `charAt` and `JSON.parse` hand back SSO values, and `"+" === "+"` across
///   those two representations has to be true.
///
/// Everything else is decided by type: a value whose tag is not `STRING_TAG`
/// can never be `===` a string (a boxed `new String("x")` is `POINTER_TAG`, and
/// correctly unequal), and a heap string whose `byte_len` or whose first / last
/// byte differs from the literal's is unequal without reading a byte the length
/// check has not already proved the header owns. Only a same-length,
/// same-endpoints heap string reaches `js_string_equals`.
///
/// Returns an `i1` that is true iff the two operands are `===`.
fn lower_string_literal_strict_eq(
    ctx: &mut FnCtx<'_>,
    val: &str,
    lit_box: &str,
    lit: &str,
) -> String {
    let bytes = lit.as_bytes().to_vec();
    let n = bytes.len();

    let bits = ctx.block().bitcast_double_to_i64(val);
    let lit_bits = ctx.block().bitcast_double_to_i64(lit_box);

    // Blocks, in the order control flows through them. Only the ones this
    // literal's length needs are created — an empty block would have no
    // terminator and fail the LLVM verifier.
    let sso_idx = sso_immediate(&bytes).map(|_| ctx.new_block("streqlit.sso"));
    let tag_idx = ctx.new_block("streqlit.tag");
    let len_idx = ctx.new_block("streqlit.len");
    let b0_idx = (n >= 1).then(|| ctx.new_block("streqlit.b0"));
    let bl_idx = (n >= 2).then(|| ctx.new_block("streqlit.bl"));
    let slow_idx = (n >= 3).then(|| ctx.new_block("streqlit.slow"));
    let true_idx = ctx.new_block("streqlit.true");
    let false_idx = ctx.new_block("streqlit.false");
    let merge_idx = ctx.new_block("streqlit.merge");

    let tag_l = ctx.block_label(tag_idx);
    let len_l = ctx.block_label(len_idx);
    let true_l = ctx.block_label(true_idx);
    let false_l = ctx.block_label(false_idx);
    let merge_l = ctx.block_label(merge_idx);
    let sso_l = sso_idx.map(|i| ctx.block_label(i));
    let b0_l = b0_idx.map(|i| ctx.block_label(i));
    let bl_l = bl_idx.map(|i| ctx.block_label(i));
    let slow_l = slow_idx.map(|i| ctx.block_label(i));

    // Entry: pooled-pointer identity. This is the hot true case — the value
    // under test and the literal are the same pool entry.
    let ident = ctx.block().icmp_eq(I64, &bits, &lit_bits);
    let after_ident = sso_l.clone().unwrap_or_else(|| tag_l.clone());
    ctx.block().cond_br(&ident, &true_l, &after_ident);

    // SSO immediate: equal => true. SSO but a *different* immediate => the
    // encoding is canonical, so the contents differ; the tag block below
    // reports that as false, since SSO is not `STRING_TAG`.
    if let Some(idx) = sso_idx {
        let imm = crate::nanbox::i64_literal(sso_immediate(&bytes).unwrap());
        ctx.current_block = idx;
        let sso_eq = ctx.block().icmp_eq(I64, &bits, &imm);
        ctx.block().cond_br(&sso_eq, &true_l, &tag_l);
    }

    // Neither the pooled pointer nor the SSO form: only a *heap* string can
    // still be equal. Every other tag — number, int32, pointer (including a
    // boxed String wrapper), bigint, null/undefined/bool, SSO with different
    // bytes — is a different ECMAScript value.
    ctx.current_block = tag_idx;
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let hp = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    // The floor `safe_load_i32_from_ptr` uses: a `STRING_TAG` value with a null
    // or tiny payload is not a dereferenceable header.
    let hp_ok = ctx.block().icmp_ugt(I64, &hp, "4095");
    let heap_ok = ctx.block().and(I1, &is_heap, &hp_ok);
    ctx.block().cond_br(&heap_ok, &len_l, &false_l);

    // `byte_len` is the pool's `value.len()`, hence a compile-time constant.
    ctx.current_block = len_idx;
    let hdr_ptr = ctx.block().inttoptr(I64, &hp);
    let blen_ptr = ctx
        .block()
        .gep_inbounds(I8, &hdr_ptr, &[(I64, STRING_HEADER_BYTE_LEN_OFFSET)]);
    let blen = ctx.block().load(I32, &blen_ptr);
    let len_ok = ctx.block().icmp_eq(I32, &blen, &n.to_string());
    let after_len = b0_l.clone().unwrap_or_else(|| true_l.clone());
    ctx.block().cond_br(&len_ok, &after_len, &false_l);

    // First and last byte. Both sit inside the `n` bytes the length check just
    // proved this header owns, so the loads need no further guard. For n <= 2
    // they settle the answer outright.
    if let Some(idx) = b0_idx {
        ctx.current_block = idx;
        let off = STRING_HEADER_SIZE.to_string();
        let p = ctx.block().gep_inbounds(I8, &hdr_ptr, &[(I64, &off)]);
        let b = ctx.block().load(I8, &p);
        let ok = ctx.block().icmp_eq(I8, &b, &i8_literal(bytes[0]));
        let next = bl_l.clone().unwrap_or_else(|| true_l.clone());
        ctx.block().cond_br(&ok, &next, &false_l);
    }
    if let Some(idx) = bl_idx {
        ctx.current_block = idx;
        let off = (STRING_HEADER_SIZE + n - 1).to_string();
        let p = ctx.block().gep_inbounds(I8, &hdr_ptr, &[(I64, &off)]);
        let b = ctx.block().load(I8, &p);
        let ok = ctx.block().icmp_eq(I8, &b, &i8_literal(bytes[n - 1]));
        let next = slow_l.clone().unwrap_or_else(|| true_l.clone());
        ctx.block().cond_br(&ok, &next, &false_l);
    }

    // Same length, same endpoints, different pointer: a real content compare.
    // Both operands are proven heap `StringHeader*` here, so this is the narrow
    // two-pointer helper, not the generic value-equality tower.
    let slow_arm = slow_idx.map(|idx| {
        ctx.current_block = idx;
        let rp = ctx.block().and(I64, &lit_bits, POINTER_MASK_I64);
        let res = ctx
            .block()
            .call(I32, "js_string_equals", &[(I64, &hp), (I64, &rp)]);
        let bit = ctx.block().icmp_ne(I32, &res, "0");
        let pred = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        (bit, pred)
    });

    ctx.current_block = true_idx;
    let true_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = false_idx;
    let false_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    let mut incoming: Vec<(&str, &str)> = vec![("true", &true_pred), ("false", &false_pred)];
    if let Some((bit, pred)) = slow_arm.as_ref() {
        incoming.push((bit, pred));
    }
    ctx.block().phi(I1, &incoming)
}

/// Magnitude comparands for the inline heap-address test in
/// [`lower_strict_eq_inline_any`]. These mirror
/// `perry-runtime::value::addr_class::{HANDLE_BAND_MAX, is_valid_obj_ptr}`:
/// a `POINTER_TAG` payload below `HANDLE_BAND_MAX` is a registry id
/// (net.Socket, fetch, zlib, revocable Proxy, UI widget), NOT an address, and
/// dereferencing one reads unmapped low memory. Anything outside the window
/// takes the runtime call instead of a header load.
const HANDLE_BAND_MAX_I64: &str = "1048576";
const HEAP_ADDR_CEILING_I64: &str = "140737488355328";

/// Inline prefix for the generic `===`/`!==` tail — the arm where BOTH
/// operands are statically unconstrained, which emitted one
/// `js_eq` → `js_jsvalue_equals` call per comparison and nothing else.
///
/// The motivating shape is a linear scan over a generic container's key
/// array: `this.keys[i] === k` in `gc-handoff/apps/pipeline.ts`'s
/// `Registry<K, V>`, measured at 8.4% of that program. A scan is dominated by
/// **misses**, so a fast path that settles only the hit is worth nothing —
/// each case below settles one direction of the real traffic.
///
/// Four cases leave without a call. Each is an exact restatement of what
/// `js_jsvalue_equals` computes for that input, not an approximation:
///
/// * **identical bits** ⇒ equal, *unless* the value is a plain (untagged)
///   IEEE NaN. Perry's tags occupy top16 `0x7FF9..=0x7FFF`, so a tagged
///   immediate (`undefined`, a pointer, an SSO string) stays equal to itself
///   even though it *is* a NaN double, while canonical `0x7FF8…` NaN and the
///   negative `0xFFF8…` NaN libm returns fall outside the band and take the
///   call, which answers `false` (`NaN !== NaN`).
/// * **both SSO strings, different bits** ⇒ different content. The SSO
///   encoding is canonical — same bytes and same length give the same bit
///   pattern — which is the argument `lower_string_strict_eq_inline` and the
///   runtime's own both-short-string arm already rely on.
/// * **both INT32, different bits** ⇒ different integers, same argument.
/// * **both `POINTER_TAG`, different payloads, and neither header carries
///   `GC_FLAG_FORWARDED`** ⇒ distinct objects. The runtime's pointer arm is
///   `resolve_forwarding(a) == resolve_forwarding(b)`, and
///   `resolve_forwarding` returns its argument unchanged when the forwarding
///   bit is clear — so two *unforwarded* distinct addresses are exactly its
///   `0` case. Anything forwarded (a post-`js_array_grow` alias, a stale
///   pre-evacuation pointer) takes the call and gets the full walk. The
///   header read is the same one `expr/array_push.rs` emits — `gc_flags` at
///   `ptr - 7`, mask `GC_FLAG_FORWARDED` (0x80) — behind the same magnitude
///   guard the runtime applies before any `GcHeader` dereference.
///
/// Everything else — a raw-bits module-level object slot (top16 zero), a heap
/// string, a bigint, a mixed pair, a boxed wrapper — falls through to
/// `js_eq`, which is what this site emitted unconditionally before.
///
/// Returns an i64 holding `TAG_TRUE`/`TAG_FALSE` (or `js_eq`'s own tagged
/// boolean), i.e. the same value the bare call produced.
fn lower_strict_eq_inline_any(ctx: &mut FnCtx<'_>, l: &str, r: &str) -> String {
    let l_bits = ctx.block().bitcast_double_to_i64(l);
    let r_bits = ctx.block().bitcast_double_to_i64(r);

    let same_idx = ctx.new_block("anyeq.same");
    let diff_idx = ctx.new_block("anyeq.diff");
    let canon_idx = ctx.new_block("anyeq.canon");
    let band_idx = ctx.new_block("anyeq.band");
    let fwd_idx = ctx.new_block("anyeq.fwd");
    let slow_idx = ctx.new_block("anyeq.slow");
    let true_idx = ctx.new_block("anyeq.true");
    let false_idx = ctx.new_block("anyeq.false");
    let merge_idx = ctx.new_block("anyeq.merge");
    let same_l = ctx.block_label(same_idx);
    let diff_l = ctx.block_label(diff_idx);
    let canon_l = ctx.block_label(canon_idx);
    let band_l = ctx.block_label(band_idx);
    let fwd_l = ctx.block_label(fwd_idx);
    let slow_l = ctx.block_label(slow_idx);
    let true_l = ctx.block_label(true_idx);
    let false_l = ctx.block_label(false_idx);
    let merge_l = ctx.block_label(merge_idx);

    let same = ctx.block().icmp_eq(I64, &l_bits, &r_bits);
    ctx.block().cond_br(&same, &same_l, &diff_l);

    // Identical bits. Equal for every Perry tag and every non-NaN double.
    ctx.current_block = same_idx;
    let stag = ctx.block().lshr(I64, &l_bits, "48");
    let tag_lo = ctx
        .block()
        .icmp_uge(I64, &stag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let tag_hi = ctx
        .block()
        .icmp_ule(I64, &stag, crate::nanbox::STRING_TAG_TOP16_I64);
    let tagged = ctx.block().and(I1, &tag_lo, &tag_hi);
    let is_nan = ctx.block().fcmp("uno", l, l);
    let not_nan = ctx.block().xor(I1, &is_nan, "true");
    let same_ok = ctx.block().or(I1, &tagged, &not_nan);
    ctx.block().cond_br(&same_ok, &true_l, &slow_l);

    // Different bits: only a same-tag pair whose encoding is canonical, or a
    // pair of unforwarded heap pointers, is decidable here.
    ctx.current_block = diff_idx;
    let l_tag = ctx.block().lshr(I64, &l_bits, "48");
    let r_tag = ctx.block().lshr(I64, &r_bits, "48");
    let l_ptr = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::POINTER_TAG_TOP16_I64);
    let r_ptr = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::POINTER_TAG_TOP16_I64);
    let both_ptr = ctx.block().and(I1, &l_ptr, &r_ptr);
    ctx.block().cond_br(&both_ptr, &band_l, &canon_l);

    ctx.current_block = canon_idx;
    let l_sso = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let r_sso = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let both_sso = ctx.block().and(I1, &l_sso, &r_sso);
    let l_i32 = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::INT32_TAG_TOP16_I64);
    let r_i32 = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::INT32_TAG_TOP16_I64);
    let both_i32 = ctx.block().and(I1, &l_i32, &r_i32);
    let canonical = ctx.block().or(I1, &both_sso, &both_i32);
    ctx.block().cond_br(&canonical, &false_l, &slow_l);

    // Both POINTER_TAG. Classify by magnitude before touching a header.
    ctx.current_block = band_idx;
    let l_addr = ctx.block().and(I64, &l_bits, POINTER_MASK_I64);
    let r_addr = ctx.block().and(I64, &r_bits, POINTER_MASK_I64);
    let l_above = ctx.block().icmp_uge(I64, &l_addr, HANDLE_BAND_MAX_I64);
    let l_below = ctx.block().icmp_ult(I64, &l_addr, HEAP_ADDR_CEILING_I64);
    let r_above = ctx.block().icmp_uge(I64, &r_addr, HANDLE_BAND_MAX_I64);
    let r_below = ctx.block().icmp_ult(I64, &r_addr, HEAP_ADDR_CEILING_I64);
    let l_heap = ctx.block().and(I1, &l_above, &l_below);
    let r_heap = ctx.block().and(I1, &r_above, &r_below);
    let both_heap = ctx.block().and(I1, &l_heap, &r_heap);
    ctx.block().cond_br(&both_heap, &fwd_l, &slow_l);

    ctx.current_block = fwd_idx;
    let l_flags_addr = ctx.block().sub(I64, &l_addr, "7");
    let l_flags_ptr = ctx.block().inttoptr(I64, &l_flags_addr);
    let l_flags = ctx.block().load(I8, &l_flags_ptr);
    let r_flags_addr = ctx.block().sub(I64, &r_addr, "7");
    let r_flags_ptr = ctx.block().inttoptr(I64, &r_flags_addr);
    let r_flags = ctx.block().load(I8, &r_flags_ptr);
    let either = ctx.block().or(I8, &l_flags, &r_flags);
    // GC_FLAG_FORWARDED = 0x80; LLVM i8 literals are signed.
    let fwd_bits = ctx.block().and(I8, &either, "-128");
    let no_fwd = ctx.block().icmp_eq(I8, &fwd_bits, "0");
    ctx.block().cond_br(&no_fwd, &false_l, &slow_l);

    ctx.current_block = slow_idx;
    let slow_res = ctx
        .block()
        .call(I64, "js_eq", &[(I64, &l_bits), (I64, &r_bits)]);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = true_idx;
    let true_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = false_idx;
    let false_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[
            (crate::nanbox::TAG_TRUE_I64, &true_pred),
            (crate::nanbox::TAG_FALSE_I64, &false_pred),
            (&slow_res, &slow_pred),
        ],
    )
}

/// Inline prefix for the `===`/`!==` string arms that have **no** literal
/// operand — `names[i] === name` in an environment lookup, say.
///
/// Two cases are settled without leaving the function, and both were paying a
/// runtime call before:
///
/// * identical bits. True for a pooled literal against itself and, more
///   importantly, for SSO vs SSO: `charAt` and `JSON.parse` hand back inline
///   values whose encoding is canonical, so equal content *is* equal bits;
/// * both operands SSO with different bits => different content, again by
///   canonicality.
///
/// The remaining arms are exactly what each caller emitted before, so this is
/// behaviour-preserving. That matters most for `legacy_unified`, whose fallback
/// keeps the `js_get_string_pointer_unified` composition — including its
/// number-coercing behaviour for operands whose `string` annotation lies. Note
/// that composition *materializes* an SSO operand onto the heap, so routing
/// SSO x SSO around it removes two allocations per comparison as well as the
/// calls.
///
/// Returns an `i32` that is 1 iff the operands are `===`.
fn lower_string_strict_eq_inline(
    ctx: &mut FnCtx<'_>,
    l: &str,
    r: &str,
    legacy_unified: bool,
) -> String {
    let l_bits = ctx.block().bitcast_double_to_i64(l);
    let r_bits = ctx.block().bitcast_double_to_i64(r);

    let tag_idx = ctx.new_block("streq.tag");
    let heap_idx = ctx.new_block("streq.heap");
    let sso_idx = ctx.new_block("streq.ssochk");
    let boxed_idx = ctx.new_block("streq.boxed");
    let true_idx = ctx.new_block("streq.true");
    let false_idx = ctx.new_block("streq.false");
    let merge_idx = ctx.new_block("streq.merge");
    let tag_l = ctx.block_label(tag_idx);
    let heap_l = ctx.block_label(heap_idx);
    let sso_l = ctx.block_label(sso_idx);
    let boxed_l = ctx.block_label(boxed_idx);
    let true_l = ctx.block_label(true_idx);
    let false_l = ctx.block_label(false_idx);
    let merge_l = ctx.block_label(merge_idx);

    let ident = ctx.block().icmp_eq(I64, &l_bits, &r_bits);
    ctx.block().cond_br(&ident, &true_l, &tag_l);

    ctx.current_block = tag_idx;
    let l_tag = ctx.block().lshr(I64, &l_bits, "48");
    let r_tag = ctx.block().lshr(I64, &r_bits, "48");
    let l_heap = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let r_heap = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let both_heap = ctx.block().and(I1, &l_heap, &r_heap);
    ctx.block().cond_br(&both_heap, &heap_l, &sso_l);

    ctx.current_block = heap_idx;
    let lh = ctx.block().and(I64, &l_bits, POINTER_MASK_I64);
    let rh = ctx.block().and(I64, &r_bits, POINTER_MASK_I64);
    let heap_res = ctx
        .block()
        .call(I32, "js_string_equals", &[(I64, &lh), (I64, &rh)]);
    let heap_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = sso_idx;
    let l_sso = ctx
        .block()
        .icmp_eq(I64, &l_tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let r_sso = ctx
        .block()
        .icmp_eq(I64, &r_tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let both_sso = ctx.block().and(I1, &l_sso, &r_sso);
    ctx.block().cond_br(&both_sso, &false_l, &boxed_l);

    ctx.current_block = boxed_idx;
    let boxed_res = if legacy_unified {
        let lu = ctx
            .block()
            .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, l)]);
        let ru = ctx
            .block()
            .call(I64, "js_get_string_pointer_unified", &[(DOUBLE, r)]);
        ctx.block()
            .call(I32, "js_string_equals", &[(I64, &lu), (I64, &ru)])
    } else {
        ctx.block()
            .call(I32, "js_jsvalue_equals", &[(DOUBLE, l), (DOUBLE, r)])
    };
    let boxed_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = true_idx;
    let true_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = false_idx;
    let false_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            ("1", &true_pred),
            ("0", &false_pred),
            (&heap_res, &heap_pred),
            (&boxed_res, &boxed_pred),
        ],
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Compare { op, left, right } => {
            // #7979: every arm below used to lower `left`, then lower `right`,
            // then consume the original left SSA value. A call-result string
            // therefore named retired from-space whenever the right call
            // collected. Keep the root around the consuming dispatch too:
            // several arms unbox/dereference the values before returning.
            with_operands_rooted(ctx, &[left, right], |ctx, operands| {
                let l = operands[0].clone();
                let r = operands[1].clone();
                // BigInt comparison fast path: NaN-tagged BIGINT_TAG values
                // are unordered under fcmp (NaN), so `a > b` on two bigints
                // always returns false. Route through js_bigint_cmp which
                // returns -1/0/1 for the three bigint ordering outcomes.
                //
                // For RELATIONAL ops (`<`, `<=`, `>`, `>=`) this direct cmp is only
                // valid when BOTH operands are statically BigInt — `js_bigint_cmp`
                // dereferences both as BigInt pointers. A *mixed* relational like
                // `1n < Infinity` or `0n < "1"` needs the full abstract relational
                // comparison (BigInt-vs-Number / BigInt-vs-String coercion), so it
                // falls through to `js_rel_*` below. Equality (`===`/`==`) keeps the
                // either-side gate (its own cross-type handling is unchanged).
                let is_relational_op = matches!(
                    op,
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                );
                // The `js_bigint_cmp` fast path is valid ONLY when BOTH operands are
                // statically BigInt. The previous equality variant fired when *either*
                // side was BigInt and fed `js_bigint_cmp` a non-BigInt operand
                // (`0n != undefined`, `0n == ""`), dereferencing an undefined/string
                // NaN-box as a BigIntHeader → garbage. Mixed-type BigInt equality now
                // falls through to `js_loose_eq` (loose, with full BigInt coercion) /
                // `fcmp` (strict, where a type mismatch is correctly never-equal).
                // Relational mixed-type already fell through to `js_rel_*`.
                let bigint_fast_path = is_bigint_expr(ctx, left) && is_bigint_expr(ctx, right);
                if bigint_fast_path {
                    let blk = ctx.block();
                    let l_handle = unbox_to_i64(blk, &l);
                    let r_handle = unbox_to_i64(blk, &r);
                    let cmp = blk.call(I32, "js_bigint_cmp", &[(I64, &l_handle), (I64, &r_handle)]);
                    let bit = match op {
                        CompareOp::Lt => blk.icmp_slt(I32, &cmp, "0"),
                        CompareOp::Le => blk.icmp_sle(I32, &cmp, "0"),
                        CompareOp::Gt => blk.icmp_sgt(I32, &cmp, "0"),
                        CompareOp::Ge => blk.icmp_sge(I32, &cmp, "0"),
                        CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(I32, &cmp, "0"),
                        CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(I32, &cmp, "0"),
                    };
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                // Boolean equality fast path: NaN-tagged TAG_TRUE/FALSE
                // bits don't compare correctly with fcmp. For
                // ===/!== where EITHER side is statically boolean, compare
                // the raw i64 bits via icmp. icmp on bits also works for
                // any other NaN-tagged value (string ptr, object ptr) when
                // the bool literal is on one side — TAG_TRUE bits never
                // match a string/pointer, so the result is correctly false.
                // STRICT only: for LooseEq/LooseNe, booleans need coercion
                // (false == "" → true) which the later js_loose_eq handles.
                let either_bool = is_bool_expr(ctx, left) || is_bool_expr(ctx, right);
                if either_bool && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                    let blk = ctx.block();
                    let l_bits = blk.bitcast_double_to_i64(&l);
                    let r_bits = blk.bitcast_double_to_i64(&r);
                    let bit = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        blk.icmp_ne(I64, &l_bits, &r_bits)
                    } else {
                        blk.icmp_eq(I64, &l_bits, &r_bits)
                    };
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                // Null/Undefined literal fast path: `x === null` / `x === undefined` /
                // `x !== null` etc. Both TAG_NULL and TAG_UNDEFINED are NaN-tagged
                // doubles, so fcmp is unordered (always false) and the string/js_eq
                // fallbacks misclassify these tags as "invalid string → both equal".
                // Compare raw i64 bits directly.
                //
                // For LooseEq/LooseNe (== / !=), null and undefined are loosely
                // equal to each other but not to anything else. Handle that by
                // routing `x == null` to `(bits == TAG_NULL) | (bits == TAG_UNDEF)`.
                let left_is_null = matches!(left.as_ref(), Expr::Null);
                let left_is_undef = matches!(left.as_ref(), Expr::Undefined);
                let right_is_null = matches!(right.as_ref(), Expr::Null);
                let right_is_undef = matches!(right.as_ref(), Expr::Undefined);
                let either_nullish_lit =
                    left_is_null || left_is_undef || right_is_null || right_is_undef;
                if either_nullish_lit
                    && matches!(
                        op,
                        CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                    )
                {
                    let blk = ctx.block();
                    let l_bits = blk.bitcast_double_to_i64(&l);
                    let r_bits = blk.bitcast_double_to_i64(&r);
                    let is_loose = matches!(op, CompareOp::LooseEq | CompareOp::LooseNe);
                    let bit = if is_loose {
                        // Loose equality: x == null → (x === null) || (x === undefined)
                        let eq_l_r = blk.icmp_eq(I64, &l_bits, &r_bits);
                        let cmp_l_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
                        let cmp_l_undef =
                            blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
                        let cmp_r_null = blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_NULL_I64);
                        let cmp_r_undef =
                            blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_UNDEFINED_I64);
                        let l_nullish = blk.or(crate::types::I1, &cmp_l_null, &cmp_l_undef);
                        let r_nullish = blk.or(crate::types::I1, &cmp_r_null, &cmp_r_undef);
                        let both_nullish = blk.and(crate::types::I1, &l_nullish, &r_nullish);
                        blk.or(crate::types::I1, &eq_l_r, &both_nullish)
                    } else {
                        // Strict equality: bit-exact compare
                        blk.icmp_eq(I64, &l_bits, &r_bits)
                    };
                    let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        blk.xor(crate::types::I1, &bit, "true")
                    } else {
                        bit
                    };
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit_final,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                // Strict equality against a string LITERAL. Decidable inline for
                // every runtime shape (see `lower_string_literal_strict_eq`), so it
                // pre-empts all the arms below — including the `js_eq` tail that an
                // `any`-typed operand like `n.kind` would otherwise take, one call
                // pair per comparison. Strict only: loose `==` coerces (`"5" == 5`)
                // and stays on `js_loose_eq`. `Expr::WtfString` is excluded — its
                // pool bytes are the WTF-8 encoding, not `str::as_bytes`.
                let lit_on_right = matches!(right.as_ref(), Expr::String(_));
                let lit_on_left = !lit_on_right && matches!(left.as_ref(), Expr::String(_));
                if (lit_on_right || lit_on_left) && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                    // Source order: the non-literal operand may have side effects.
                    let (val, lit_box, lit) = if lit_on_right {
                        let Expr::String(s) = right.as_ref() else {
                            unreachable!("lit_on_right implies Expr::String")
                        };
                        (l, r, s.clone())
                    } else {
                        let Expr::String(s) = left.as_ref() else {
                            unreachable!("lit_on_left implies Expr::String")
                        };
                        (r, l, s.clone())
                    };
                    let bit = lower_string_literal_strict_eq(ctx, &val, &lit_box, &lit);
                    let blk = ctx.block();
                    let bit_final = if matches!(op, CompareOp::Ne) {
                        blk.xor(I1, &bit, "true")
                    } else {
                        bit
                    };
                    let tagged = blk.select(
                        I1,
                        &bit_final,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                // "One side is statically string, other is unknown"
                // fallback: `c === Color.Red` where Color is a const
                // object. Neither js_eq (bit-compare, wrong for string
                // content) nor fcmp (NaN-tagged, always false) works.
                //
                // Dispatch through js_string_equals after extracting
                // both string pointers via js_get_string_pointer_unified.
                // That helper returns null for non-string NaN-tagged
                // values, which js_string_equals treats as "not equal"
                // — the correct answer when the unknown side isn't a
                // string at runtime.
                let both_strings_check = is_string_expr(ctx, left) && is_string_expr(ctx, right);
                // The non-statically-string operand collides through this
                // fast path when, at runtime, it is ALSO a non-string. Both
                // operands then funnel through `js_get_string_pointer_unified`,
                // which returns 0 for any non-string NaN-boxed value (numbers,
                // class refs / InjectionTokens, plain objects, …). The
                // subsequent `js_string_equals(0, 0)` returns 1 (its
                // pointer-identity / both-null branches both report "equal"),
                // so two *distinct* non-string values wrongly compare `===`.
                //
                // This is exactly the NestJS DI `token === name` bug:
                // `name` is statically `string` (the destructured
                // `dependencyContext.name`) but at runtime holds a class ref
                // (e.g. `AppService`), and `token` is `any` holding a
                // *different* class ref (`AppController`) — both coerce to 0
                // and the inline `===` reports `true`, throwing
                // `UnknownDependencies` and aborting the app.
                //
                // The static `string` type is therefore a lie here (like the
                // #3576 number-vs-object case). When the OTHER operand is
                // statically `Any` (its runtime value is unconstrained and may
                // be a non-string), this fast path is unsound: route through
                // `js_eq`, which content-compares real strings (SSO + heap) AND
                // correctly distinguishes class refs / objects by identity.
                let other_side_is_any = |other: &Expr| -> bool {
                    matches!(
                        crate::type_analysis::static_type_of(ctx, other),
                        Some(HirType::Any) | None
                    )
                };
                let one_side_string = !both_strings_check
                    && ((is_string_expr(ctx, left)
                        && !is_numeric_expr(ctx, right)
                        && !is_bool_expr(ctx, right)
                        && !other_side_is_any(right))
                        || (is_string_expr(ctx, right)
                            && !is_numeric_expr(ctx, left)
                            && !is_bool_expr(ctx, left)
                            && !other_side_is_any(left)));
                // Only STRICT eq/ne use this string-pointer fast path. Loose `==`/`!=`
                // must fall through to `js_loose_eq` below: when one side is a boxed
                // String/primitive *wrapper* (a POINTER_TAG object, not a STRING_TAG
                // value), `js_get_string_pointer_unified` returns the raw ObjectHeader
                // pointer and `js_string_equals` reads it as a bogus string → wrong
                // result (`new String("x") == "x"` was `false`). `js_loose_eq` unboxes
                // the wrapper first. Strict `=== "lit"` is unaffected (both sides are
                // real strings at runtime). #boxed-loose-eq.
                if one_side_string && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                    // Reuse the no-literal string prefix instead of sending
                    // every comparison through two unified-unbox calls. This
                    // is the hot shape of a specialized generic container:
                    // `this.keys[i]` has a concrete string type while `k`
                    // retains its `K` spelling. Identical pooled strings now
                    // leave after one bit compare; distinct heap strings call
                    // only `js_string_equals`. The boxed arm is byte-for-byte
                    // the old helper composition, so a lying annotation keeps
                    // its existing behaviour.
                    let i32_eq = lower_string_strict_eq_inline(ctx, &l, &r, true);
                    let blk = ctx.block();
                    let bit = blk.icmp_ne(I32, &i32_eq, "0");
                    let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        blk.xor(crate::types::I1, &bit, "true")
                    } else {
                        bit
                    };
                    let tagged = blk.select(
                        crate::types::I1,
                        &bit_final,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                // Generic equality fallback: when neither operand is
                // statically numeric, dispatch through js_eq which
                // handles strings, booleans, objects, null, undefined
                // via NaN-tag inspection. Used by `eq` helpers in tests
                // that take `any` and pass NaN-tagged values.
                let either_non_numeric =
                    !is_numeric_expr(ctx, left) && !is_numeric_expr(ctx, right);
                let only_eq = matches!(
                    op,
                    CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                );
                // We still let the more specific paths below win for
                // statically-typed string/bool operands; this fallback
                // only handles the truly-Any case.
                let unknown_l = !is_numeric_expr(ctx, left)
                    && !is_string_expr(ctx, left)
                    && !is_bool_expr(ctx, left);
                let unknown_r = !is_numeric_expr(ctx, right)
                    && !is_string_expr(ctx, right)
                    && !is_bool_expr(ctx, right);
                if either_non_numeric && only_eq && unknown_l && unknown_r {
                    // Use js_loose_eq for == / != (handles null==undefined,
                    // cross-type coercion). STRICT `===`/`!==` gets the inline
                    // prefix instead: the operands that reach here are
                    // unconstrained, and a generic-container key scan
                    // (`this.keys[i] === k`) spends its whole cost on this one
                    // call. Loose `==`'s cross-type coercions are not
                    // bit-decidable, so it keeps the bare call.
                    let result_bits = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                        let blk = ctx.block();
                        let l_bits = blk.bitcast_double_to_i64(&l);
                        let r_bits = blk.bitcast_double_to_i64(&r);
                        blk.call(I64, "js_loose_eq", &[(I64, &l_bits), (I64, &r_bits)])
                    } else {
                        lower_strict_eq_inline_any(ctx, &l, &r)
                    };
                    let blk = ctx.block();
                    let result = blk.bitcast_i64_to_double(&result_bits);
                    if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                        let inv = blk.xor(crate::types::I1, &cmp, "true");
                        let tagged = blk.select(
                            crate::types::I1,
                            &inv,
                            I64,
                            crate::nanbox::TAG_TRUE_I64,
                            crate::nanbox::TAG_FALSE_I64,
                        );
                        return Ok(blk.bitcast_i64_to_double(&tagged));
                    }
                    return Ok(result);
                }

                // String equality fast path: fcmp doesn't work on
                // NaN-tagged string pointers (NaN comparisons are
                // unordered → always false). When both operands are
                // statically strings, dispatch through js_string_equals.
                let both_strings = is_string_expr(ctx, left) && is_string_expr(ctx, right);
                // Representation-selection Phase 3a: when a canonical-Str local
                // is an operand, tag-dispatch inline instead of paying the two
                // opaque (SSO-heap-materializing) unified unbox calls: both
                // proven heap → direct `js_string_equals(h, h)` on the raw
                // handles; any other mix → one `js_jsvalue_equals` call, which
                // content-compares heap × SSO without materializing and never
                // number-coerces (a lying annotation gets exact `===`
                // semantics, strictly closer to spec than the legacy path).
                let canonical_str_involved = matches!(
                    left.as_ref(), Expr::LocalGet(id) if crate::expr::local_is_canonical_str(ctx, *id)
                ) || matches!(
                    right.as_ref(), Expr::LocalGet(id) if crate::expr::local_is_canonical_str(ctx, *id)
                );
                if both_strings
                    && canonical_str_involved
                    && matches!(
                        op,
                        CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                    )
                {
                    let i32_eq = lower_string_strict_eq_inline(ctx, &l, &r, false);
                    let blk = ctx.block();
                    let bit = blk.icmp_ne(I32, &i32_eq, "0");
                    let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        blk.xor(crate::types::I1, &bit, "true")
                    } else {
                        bit
                    };
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &bit_final,
                        crate::types::I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged_i64));
                }
                if both_strings
                    && matches!(
                        op,
                        CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                    )
                {
                    // Issue #214: SSO-safe unbox — the inline mask returns
                    // garbage for SHORT_STRING_TAG values (e.g. SSO results
                    // from `JSON.parse('["hello"]')[0]`), causing
                    // `js_string_equals` to deref the inline payload bytes.
                    // That unbox is now the *fallback* arm: identical bits and
                    // SSO x SSO are answered inline, which is what keeps a pair of
                    // short runtime strings (`charAt`, `substring`) from
                    // materializing two throwaway heap copies per comparison.
                    let i32_eq = lower_string_strict_eq_inline(ctx, &l, &r, true);
                    let blk = ctx.block();
                    let bit = blk.icmp_ne(I32, &i32_eq, "0");
                    let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        blk.xor(crate::types::I1, &bit, "true")
                    } else {
                        bit
                    };
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &bit_final,
                        crate::types::I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged_i64));
                }
                // String relational fast path: `s1 < s2`, `s1 > s2`, etc.
                // fcmp on NaN-tagged pointers is unordered (always false),
                // so dispatch through js_string_compare which returns
                // -1/0/1 like memcmp. Then test the result against 0 with
                // the right icmp predicate.
                // Representation-selection Phase 3a: relational counterpart of
                // the canonical-Str equality arm above — both proven heap →
                // direct `js_string_compare(h, h)`; any other mix → one
                // `js_string_compare_value` call (SSO-aware, no heap
                // materialization, numbers coerced via their decimal string
                // form exactly like the legacy unified path).
                if both_strings
                    && canonical_str_involved
                    && matches!(
                        op,
                        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                    )
                {
                    let cmp_i32 = canonical_str_cmp_dispatch(
                        ctx,
                        &l,
                        &r,
                        "js_string_compare",
                        "js_string_compare_value",
                        "strcmp",
                    );
                    let blk = ctx.block();
                    let bit = match op {
                        CompareOp::Lt => blk.icmp_slt(I32, &cmp_i32, "0"),
                        CompareOp::Le => blk.icmp_sle(I32, &cmp_i32, "0"),
                        CompareOp::Gt => blk.icmp_sgt(I32, &cmp_i32, "0"),
                        CompareOp::Ge => blk.icmp_sge(I32, &cmp_i32, "0"),
                        _ => unreachable!(),
                    };
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &bit,
                        crate::types::I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged_i64));
                }
                if both_strings
                    && matches!(
                        op,
                        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                    )
                {
                    let blk = ctx.block();
                    // Issue #214: SSO-safe unbox.
                    let l_handle = unbox_str_handle(blk, &l);
                    let r_handle = unbox_str_handle(blk, &r);
                    let cmp_i32 = blk.call(
                        I32,
                        "js_string_compare",
                        &[(I64, &l_handle), (I64, &r_handle)],
                    );
                    let bit = match op {
                        CompareOp::Lt => blk.icmp_slt(I32, &cmp_i32, "0"),
                        CompareOp::Le => blk.icmp_sle(I32, &cmp_i32, "0"),
                        CompareOp::Gt => blk.icmp_sgt(I32, &cmp_i32, "0"),
                        CompareOp::Ge => blk.icmp_sge(I32, &cmp_i32, "0"),
                        _ => unreachable!(),
                    };
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &bit,
                        crate::types::I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged_i64));
                }

                // Loose equality (==, !=): dispatch through js_loose_eq
                // which handles cross-type coercion (null==undefined,
                // "1"==1, false==0, etc.). Strict === already handled
                // above by the typed fast paths.
                if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                    let blk = ctx.block();
                    let l_bits = blk.bitcast_double_to_i64(&l);
                    let r_bits = blk.bitcast_double_to_i64(&r);
                    let result_bits =
                        blk.call(I64, "js_loose_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                    if matches!(op, CompareOp::LooseNe) {
                        let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                        let inv = blk.xor(crate::types::I1, &cmp, "true");
                        let tagged = blk.select(
                            crate::types::I1,
                            &inv,
                            I64,
                            crate::nanbox::TAG_TRUE_I64,
                            crate::nanbox::TAG_FALSE_I64,
                        );
                        return Ok(blk.bitcast_i64_to_double(&tagged));
                    }
                    return Ok(blk.bitcast_i64_to_double(&result_bits));
                }

                // An ordered relational compare (`<`, `<=`, `>`, `>=`) whose
                // operands aren't BOTH statically numeric needs the full ECMAScript
                // Abstract Relational Comparison: ToPrimitive (`{valueOf}`/`Date`),
                // lexicographic string compare, BigInt-vs-Number/String coercion,
                // and null/boolean/string ToNumber. A bare `fcmp` mishandles all of
                // these (NaN-boxed operands are unordered → always `false`). Route
                // through the runtime `js_rel_*` helpers, which return a NaN-boxed
                // boolean. The statically-numeric case keeps the bare `fcmp` fast
                // path below (and Dates are subsumed — they aren't numeric_expr).
                let both_numeric = is_numeric_expr(ctx, left)
                    && is_numeric_expr(ctx, right)
                    && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, left)
                    && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, right)
                    && !is_bigint_expr(ctx, left)
                    && !is_bigint_expr(ctx, right);
                if is_relational_op && !both_numeric {
                    let blk = ctx.block();
                    let fname = match op {
                        CompareOp::Lt => "js_rel_lt",
                        CompareOp::Le => "js_rel_le",
                        CompareOp::Gt => "js_rel_gt",
                        CompareOp::Ge => "js_rel_ge",
                        _ => unreachable!(),
                    };
                    let res = blk.call(DOUBLE, fname, &[(DOUBLE, &l), (DOUBLE, &r)]);
                    return Ok(res);
                }
                // Strict ===/!== where the operands are NOT both certainly
                // numeric must NOT fall to the bare fcmp tail: a declared
                // `Number` local can carry an object at runtime (`var a = 2;
                // f(){ a = o; } f(); a === o` — the static type lies, and fcmp
                // on NaN-boxed pointers is unordered → permanently false).
                // js_eq answers correctly for every runtime shape, including
                // the honest number-vs-object case (#3576 probe family).
                if matches!(op, CompareOp::Eq | CompareOp::Ne) && !both_numeric {
                    let blk = ctx.block();
                    let l_bits = blk.bitcast_double_to_i64(&l);
                    let r_bits = blk.bitcast_double_to_i64(&r);
                    let result_bits = blk.call(I64, "js_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                    if matches!(op, CompareOp::Ne) {
                        let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                        let inv = blk.xor(crate::types::I1, &cmp, "true");
                        let tagged = blk.select(
                            crate::types::I1,
                            &inv,
                            I64,
                            crate::nanbox::TAG_TRUE_I64,
                            crate::nanbox::TAG_FALSE_I64,
                        );
                        return Ok(blk.bitcast_i64_to_double(&tagged));
                    }
                    return Ok(blk.bitcast_i64_to_double(&result_bits));
                }
                let pred = match op {
                    CompareOp::Eq => "oeq",
                    // !== uses `une` (unordered or not equal), NOT `one`.
                    // `one` is "ordered and not equal" which returns false
                    // when either operand is NaN. JS !== on NaN must return
                    // true: NaN !== NaN → !(NaN === NaN) → !false → true.
                    CompareOp::Ne => "une",
                    CompareOp::Lt => "olt",
                    CompareOp::Le => "ole",
                    CompareOp::Gt => "ogt",
                    CompareOp::Ge => "oge",
                    // LooseEq/Ne handled above
                    CompareOp::LooseEq | CompareOp::LooseNe => unreachable!(),
                };
                let blk = ctx.block();
                let bit = blk.fcmp(pred, &l, &r);
                let tag_true_i64 = crate::nanbox::TAG_TRUE_I64;
                let tag_false_i64 = crate::nanbox::TAG_FALSE_I64;
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit,
                    crate::types::I64,
                    tag_true_i64,
                    tag_false_i64,
                );
                Ok(blk.bitcast_i64_to_double(&tagged_i64))
            })
        }

        // -------- Objects (Phase B.4) --------
        // `{ k1: v1, k2: v2, … }` literal: allocate, set each field by
        // name (key string sourced from the StringPool), NaN-box the
        // pointer via js_nanbox_pointer.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
