//! Per-site inline cache for `"literal" + value` — the `obj["field_" + j]`
//! key shape of `bench_object_property`, where node was still 2 ms ahead
//! (14 vs 12) with the process-wide concat memo in place.
//!
//! The memo (`perry-runtime/src/string/concat.rs`, 512 slots keyed by content
//! hash) already made the key concat allocation-free, but its hit is still a
//! call into an ~850-instruction function: tag test, `fract`, range test,
//! itoa into a stack buffer, ASCII scan, hash, byte compare, governor
//! bookkeeping. This lane keeps the hot key off the runtime entirely.
//!
//! Every site whose left operand is a source string literal gets a private
//! `[CONCAT_SITE_SLOTS x i64] zeroinitializer` table. Slot `k` is either 0 or
//! the NaN-boxed heap string `prefix + String(k)` — by construction, because
//! the prefix cannot vary at the site and only the runtime miss arm writes
//! the table — so a filled slot needs no verification:
//!
//!  * **gate**: `0.0 <= r < SLOTS` as ordered `fcmp`s, which reject every
//!    NaN and therefore every NaN-boxed non-number, and dominate the
//!    `fptosi` (poison out of range);
//!  * **probe**: `k = fptosi r`, `sitofp k == r` (integral), load slot `k`,
//!    non-zero → the cached handle. The load is in-bounds for any gated `r`,
//!    so integrality and emptiness fold into one `and`;
//!  * **fill arm** (gated value, empty or non-integral slot):
//!    `js_string_concat_site_value(table, prefix, r)`, which answers exactly
//!    what `js_string_concat_value_box` would, fills the slot when the result
//!    is a heap string, and registers the slot as a global root through the
//!    same funnel string literals use;
//!  * **plain arm** (value outside the table, e.g. `"item_" + i` past 31):
//!    the fused `js_string_concat_value_box` call this lane replaced, so a
//!    site whose values mostly miss pays two `fcmp`s and a branch over the
//!    old cost — not an extra call level. bench_gc_pressure's 500k-iteration
//!    key site measured that level at ~1 ms before the split.
//!
//! For a loop counter already proven i32 the round trip
//! `sitofp(fptosi(sitofp k))` folds away, leaving the gate and one load.
//!
//! ## Admission
//!
//! The gate is pure cost on a site whose values sweep past the table:
//! bench_gc_pressure's `"item_" + i` runs to 500k, and its two compares and
//! branch per call measured ~0.5-1 ms over 501k calls (1-2 ns each), against
//! a hit that saves ~19 ns (bench_object_property: 210k calls, 4 ms). So a
//! site gets a table only when the right operand is PROVEN small — a loop
//! counter with a proven induction interval (`loop_bounded_i32`), a
//! compile-time integer (literal, module constant, `-`/`+`/`*` of those), or
//! `x % C` with a small `C` — inside `0..=CONCAT_SITE_ADMIT_MAX`. A sweep to
//! 255 still hits one call in eight (2.4 ns saved against 1.75 ns of gate);
//! an unproven operand keeps the plain fused call and the process-wide memo.
//!
//! The table is emitted through `typed_parse_rodata`, the per-function
//! deferred raw-global sink every lowering context already drains.
//! `PERRY_CONCAT_SITE_CACHE=0` removes the lane at build time.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use crate::expr::FnCtx;
use crate::lower_string_concat::str_operand_handle_tag_dispatched;
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I32, I64};

/// Must match `perry-runtime/src/string/concat_site.rs::CONCAT_SITE_SLOTS`.
pub(crate) const CONCAT_SITE_SLOTS: usize = 32;
/// The table's LLVM type, spelled out because the block builder keeps the
/// `gep` type string by reference.
const CONCAT_SITE_TABLE_TY: &str = "[32 x i64]";
const _: () = assert!(
    CONCAT_SITE_SLOTS == 32,
    "CONCAT_SITE_TABLE_TY must spell CONCAT_SITE_SLOTS"
);

/// Largest right-operand value a site may be proven to reach and still get a
/// table (see the admission paragraph in the module docs).
const CONCAT_SITE_ADMIT_MAX: i64 = 255;

/// `PERRY_CONCAT_SITE_CACHE=0` kill switch (default on).
fn concat_site_cache_enabled() -> bool {
    match std::env::var("PERRY_CONCAT_SITE_CACHE") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false" | "OFF" | "FALSE"),
        Err(_) => true,
    }
}

/// An integer `f64` inside i32 range, as an `i64`.
fn int_of(f: f64) -> Option<i64> {
    (f.is_finite() && f.fract() == 0.0 && f.abs() <= i32::MAX as f64).then_some(f as i64)
}

/// Compile-time integer value of `e`: an integer literal (HIR spells `19`
/// as `Expr::Integer`, `1e1` as `Expr::Number`), an integer-constant local
/// (module constant or a never-written `const` with a literal initialiser —
/// the loop proof's own set), unary minus, or `+`/`-`/`*` of those. `-0`
/// folds to 0, which is the slot JS prints it as.
fn const_int(ctx: &FnCtx<'_>, e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n) => (n.unsigned_abs() <= i32::MAX as u64).then_some(*n),
        Expr::Number(f) => int_of(*f),
        Expr::LocalGet(id) => ctx
            .native_facts
            .loop_induction()
            .integer_constants
            .get(id)
            .copied(),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => const_int(ctx, operand).and_then(|v| v.checked_neg()),
        Expr::Binary { op, left, right } => {
            let a = const_int(ctx, left)?;
            let b = const_int(ctx, right)?;
            match op {
                BinaryOp::Add => a.checked_add(b),
                BinaryOp::Sub => a.checked_sub(b),
                BinaryOp::Mul => a.checked_mul(b),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether the right operand is proven to stay inside `0..=ADMIT_MAX` (or,
/// for `x % C`, inside `(-C, C)` with `C - 1 <= ADMIT_MAX`; negative
/// remainders take the plain arm at runtime).
fn right_operand_proven_small(ctx: &FnCtx<'_>, right: &Expr) -> bool {
    let small = |v: i64| (0..=CONCAT_SITE_ADMIT_MAX).contains(&v);
    if let Some(v) = const_int(ctx, right) {
        return small(v);
    }
    match right {
        Expr::LocalGet(id) => ctx
            .native_facts
            .loop_induction()
            .intervals
            .get(id)
            .is_some_and(|iv| iv.lo >= 0 && small(iv.hi)),
        Expr::Binary {
            op: BinaryOp::Mod,
            right: modulus,
            ..
        } => const_int(ctx, modulus).is_some_and(|c| c > 0 && small(c - 1)),
        _ => false,
    }
}

fn concat_site_global_name(ctx: &FnCtx<'_>, site_id: u32) -> String {
    let prefix = ctx.strings.module_prefix();
    if prefix.is_empty() {
        format!("perry_concat_site_{site_id}")
    } else {
        format!("perry_concat_site_{prefix}__{site_id}")
    }
}

/// If `left` is a string literal and `right` is proven small, emit the
/// per-site cached concat of `left + right` (operands already lowered to
/// `l_box` / `r_box`) and return the NaN-boxed string result; otherwise
/// `Ok(None)` so the caller keeps the plain fused helper call. Both cold arms
/// recompute the literal's handle (an inline `bitcast; and` for a literal) so
/// the hot path carries nothing but the gate and the probe.
pub(crate) fn try_lower_concat_site_cached(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    l_box: &str,
    r_box: &str,
) -> Result<Option<String>> {
    if !concat_site_cache_enabled() || !matches!(left, Expr::String(_)) {
        return Ok(None);
    }
    if !right_operand_proven_small(ctx, right) {
        return Ok(None);
    }

    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let table_name = concat_site_global_name(ctx, site_id);
    ctx.typed_parse_rodata.push(format!(
        "@{table_name} = private global {CONCAT_SITE_TABLE_TY} zeroinitializer"
    ));
    let table_ref = format!("@{table_name}");

    let chk_idx = ctx.new_block("csite.chk");
    let hit_idx = ctx.new_block("csite.hit");
    let fill_idx = ctx.new_block("csite.fill");
    let plain_idx = ctx.new_block("csite.plain");
    let merge_idx = ctx.new_block("csite.merge");
    let chk_label = ctx.block_label(chk_idx);
    let hit_label = ctx.block_label(hit_idx);
    let fill_label = ctx.block_label(fill_idx);
    let plain_label = ctx.block_label(plain_idx);
    let merge_label = ctx.block_label(merge_idx);

    // ---- gate: 0.0 <= r < SLOTS (ordered, so every NaN-box fails) ----
    {
        let blk = ctx.block();
        let lo = blk.fcmp("oge", r_box, &double_literal(0.0));
        let hi = blk.fcmp("olt", r_box, &double_literal(CONCAT_SITE_SLOTS as f64));
        let in_range = blk.and(I1, &lo, &hi);
        blk.cond_br(&in_range, &chk_label, &plain_label);
    }

    // ---- probe: integral value and a filled slot ----
    ctx.current_block = chk_idx;
    let cached = {
        let blk = ctx.block();
        let k = blk.fptosi(DOUBLE, r_box, I32);
        let back = blk.sitofp(I32, &k, DOUBLE);
        let is_int = blk.fcmp("oeq", &back, r_box);
        let k64 = blk.sext(I32, &k, I64);
        let cell = blk.gep(CONCAT_SITE_TABLE_TY, &table_ref, &[(I64, "0"), (I64, &k64)]);
        let cached = blk.load(I64, &cell);
        let filled = blk.icmp_ne(I64, &cached, "0");
        let hit = blk.and(I1, &is_int, &filled);
        blk.cond_br(&hit, &hit_label, &fill_label);
        cached
    };

    // ---- hit: the slot IS the NaN-boxed result ----
    ctx.current_block = hit_idx;
    let (hit_val, hit_end) = {
        let blk = ctx.block();
        let val = blk.bitcast_i64_to_double(&cached);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    // ---- fill: gated value, slot empty (or value non-integral) ----
    ctx.current_block = fill_idx;
    let fill_handle = str_operand_handle_tag_dispatched(ctx, left, l_box);
    let (fill_val, fill_end) = {
        let blk = ctx.block();
        let table_i64 = blk.ptrtoint(&table_ref, I64);
        let val = blk.call(
            DOUBLE,
            "js_string_concat_site_value",
            &[(I64, &table_i64), (I64, &fill_handle), (DOUBLE, r_box)],
        );
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    // ---- plain: value outside the table — the call this lane replaced ----
    ctx.current_block = plain_idx;
    let plain_handle = str_operand_handle_tag_dispatched(ctx, left, l_box);
    let (plain_val, plain_end) = {
        let blk = ctx.block();
        let val = blk.call(
            DOUBLE,
            "js_string_concat_value_box",
            &[(I64, &plain_handle), (DOUBLE, r_box)],
        );
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (hit_val.as_str(), hit_end.as_str()),
            (fill_val.as_str(), fill_end.as_str()),
            (plain_val.as_str(), plain_end.as_str()),
        ],
    )))
}
