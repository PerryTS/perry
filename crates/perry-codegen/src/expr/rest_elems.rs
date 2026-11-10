//! I32 at rest, PR B (representation RFC §10.7; DESIGN-n1-at-rest §3, §6):
//! the element sites and the births of at-rest Arrays.
//!
//! wpfacts (`rest_elems`) admits an Array template at rest when every
//! element of every Array it births is an int32 Number with int demand, the
//! Array's length is fixed and dense below a floor (its storage never grows,
//! moves or holds a hole), and nothing but the element sites below touches
//! its elements. Such an Array holds each element as an I32 word
//! `INT32_TAG | (u32)v`. The facts name the sites by `element_offset`
//! (`ElisionFacts::rest_reads` / `rest_writes`) and the births by their
//! `fill` call's offset (`rest_births`); every receiver such a site may see
//! is an at-rest Array, so no site tests a kind at run time.
//!
//! - **Birth** (`encode_birth`): `new Array(k).fill(v)` lowers as it always
//!   does; its result is handed to `js_array_rest_encode`, which encodes the
//!   words and marks the header (`perry-runtime` `layout_slot`).
//! - **Read** (`lower_read`, `lower_read_i32`, `read_at`): a key that is an
//!   integer in `[0, floor)` loads the low half of the slot at
//!   `h + 24 + 8 * key` (the floor is defined, so the elements start behind
//!   the header, and the storage never moves). An int consumer takes the
//!   i32 as it is; any other gets its canonical double. Any other key takes
//!   the cold `js_array_rest_get` (the accessor, then the generic `Get`).
//! - **Store** (`lower_write`): the value is an int32 Number by the facts;
//!   `store i64 INT32_TAG | zext(i32)`. Any other key takes the cold
//!   `js_array_rest_set`.
//!
//! These sites are lowered before every element tier (`index_get::lower`,
//! the number-context entries, `index_set::lower`, the native i32 entry), and
//! a loop that holds one takes no loop tier (`in_loop`): the tiers read and
//! write element words as doubles or JSValues. A b1 field read call reads
//! through `read_at` when its body's read is at rest (`FactsFieldReadCall::
//! rest`): its synthesized read is lowered with `FnCtx::rest_b1` naming the
//! body read's own verdict, as that read's offset is the body module's.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): every at-rest load first tests that
//! the slot's high half is `INT32_TAG`, and every store that its value is an
//! int32 Number that is not `-0`; a miss reports the false proof and exits
//! 86 (`Fact::ArrayRep`). Negative control `PERRY_FACTS_SABOTAGE=restraw`:
//! a store writes the value's double word, unencoded.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::FnCtx;
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I1, I32, I64};

/// `INT32_TAG` (`0x7FFE_0000_0000_0000`) and its high half.
const INT32_TAG_I64: &str = "9222809086901354496";
const INT32_TAG_HI_I32: &str = "2147352576";

/// The floor of an at-rest element read `e`.
pub(crate) fn read_floor(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    match e {
        Expr::IndexGet { element_offset, .. } if *element_offset != 0 => match ctx.rest_b1 {
            Some((offset, floor)) if offset == *element_offset => floor,
            _ => ctx.wpfacts.rest_reads.get(element_offset).copied(),
        },
        _ => None,
    }
}

/// The floor of an at-rest element store `e`.
pub(crate) fn write_floor(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    match e {
        Expr::IndexSet { element_offset, .. } if *element_offset != 0 => {
            ctx.wpfacts.rest_writes.get(element_offset).copied()
        }
        _ => None,
    }
}

/// `e` is the `fill` call of an at-rest birth (wpfacts keys it by the
/// member's offset, else the call's).
pub(crate) fn is_birth(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::Call {
        callee,
        byte_offset,
        ..
    } = e
    else {
        return false;
    };
    let Expr::PropertyGet {
        property,
        byte_offset: member,
        ..
    } = callee.as_ref()
    else {
        return false;
    };
    let offset = if *member != 0 { *member } else { *byte_offset };
    property == "fill" && offset != 0 && ctx.wpfacts.rest_births.contains(&offset)
}

/// The at-rest Array `arr` (a boxed word) is born: encode its words.
pub(crate) fn encode_birth(ctx: &mut FnCtx<'_>, arr: &str) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "birth" }),
    );
    crate::facts_consumed::used();
    ctx.block()
        .call(DOUBLE, "js_array_rest_encode", &[(DOUBLE, arr)])
}

/// An expression of `stmts` / `exprs` (closures aside) is an at-rest site.
pub(crate) fn in_loop(ctx: &FnCtx<'_>, exprs: &[&Expr], stmts: &[Stmt]) -> bool {
    if ctx.wpfacts.rest_reads.is_empty() && ctx.wpfacts.rest_writes.is_empty() {
        return false;
    }
    fn deep(ctx: &FnCtx<'_>, e: &Expr) -> bool {
        if read_floor(ctx, e).is_some() || write_floor(ctx, e).is_some() {
            return true;
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(e, &mut |c| found = found || deep(ctx, c));
        found
    }
    exprs.iter().any(|e| deep(ctx, e))
        || stmts
            .iter()
            .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| deep(ctx, e)))
}

/// What a read's consumer takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    /// The JSValue (a double; `undefined` past the length).
    Value,
    /// ToNumber of the value, a canonical double (a number context).
    Number,
    /// ToInt32 of the value, an i32 (an int consumer).
    I32,
}

/// Lower the at-rest read `object[index]` (floor `floor`) for `want`.
pub(crate) fn lower_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    floor: u32,
    want: Want,
) -> Result<String> {
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, v| {
        Ok(read_at(ctx, &v[0], &v[1], floor, want))
    })
}

/// The key `key` (a double) is an integer in `[0, floor)`, and that i32.
fn index_in(ctx: &mut FnCtx<'_>, key: &str, floor: u32) -> (String, String) {
    let blk = ctx.block();
    let low = blk.fcmp("oge", key, "0.0");
    let high = blk.fcmp("olt", key, &double_literal(f64::from(floor)));
    let inside = blk.and(I1, &low, &high);
    // In range the conversion is exact or truncates; out of range it is
    // never taken (`select` keeps it defined).
    let safe = blk.select(I1, &inside, DOUBLE, key, "0.0");
    let idx = blk.fptosi(DOUBLE, &safe, I32);
    let back = blk.sitofp(I32, &idx, DOUBLE);
    let exact = blk.fcmp("oeq", &back, key);
    (blk.and(I1, &inside, &exact), idx)
}

/// The slot of element `idx` (an i32 below the floor) of the boxed `arr`.
fn slot(ctx: &mut FnCtx<'_>, arr: &str, idx: &str) -> String {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr);
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let i = blk.zext(I32, idx, I64);
    let offset = blk.shl(I64, &i, "3");
    let base = blk.add(
        I64,
        &handle,
        &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
    );
    let addr = blk.add(I64, &base, &offset);
    blk.inttoptr(I64, &addr)
}

/// Read element `key` (a double) of the at-rest Array `arr` for `want`
/// (module docs).
pub(crate) fn read_at(ctx: &mut FnCtx<'_>, arr: &str, key: &str, floor: u32, want: Want) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "read", "floor": floor }),
    );
    crate::facts_consumed::used();
    let (hit, idx) = index_in(ctx, key, floor);
    let load = ctx.new_block("rest.load");
    let cold = ctx.new_block("rest.cold");
    let merge = ctx.new_block("rest.merge");
    let (load_l, cold_l, merge_l) = (
        ctx.block_label(load),
        ctx.block_label(cold),
        ctx.block_label(merge),
    );
    ctx.block().cond_br(&hit, &load_l, &cold_l);

    ctx.current_block = load;
    let ptr = slot(ctx, arr, &idx);
    if crate::verify_facts::enabled() {
        witness_word(ctx, arr, &ptr);
    }
    let v = ctx.block().load(I32, &ptr);
    let fast = if want == Want::I32 {
        v
    } else {
        ctx.block().sitofp(I32, &v, DOUBLE)
    };
    let load_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = cold;
    let got = ctx
        .block()
        .call(DOUBLE, "js_array_rest_get", &[(DOUBLE, arr), (DOUBLE, key)]);
    let slow = match want {
        Want::Value => got,
        Want::Number => ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &got)]),
        Want::I32 => {
            let n = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &got)]);
            ctx.toint32_wrap(&n)
        }
    };
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge;
    let ty = if want == Want::I32 { I32 } else { DOUBLE };
    ctx.block()
        .phi(ty, &[(&fast, &load_end), (&slow, &cold_end)])
}

/// Lower the at-rest store `object[index] = value` (floor `floor`); its
/// value is the expression's.
pub(crate) fn lower_write(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    floor: u32,
) -> Result<String> {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "write", "floor": floor }),
    );
    crate::facts_consumed::used();
    crate::rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, v| {
        Ok(store_at(ctx, &v[0], &v[1], &v[2], floor))
    })
}

/// Store `val` as element `key` of the at-rest Array `arr` (module docs).
fn store_at(ctx: &mut FnCtx<'_>, arr: &str, key: &str, val: &str, floor: u32) -> String {
    let (hit, idx) = index_in(ctx, key, floor);
    let store = ctx.new_block("rest.store");
    let cold = ctx.new_block("rest.store_cold");
    let merge = ctx.new_block("rest.stored");
    let (store_l, cold_l, merge_l) = (
        ctx.block_label(store),
        ctx.block_label(cold),
        ctx.block_label(merge),
    );
    ctx.block().cond_br(&hit, &store_l, &cold_l);

    ctx.current_block = store;
    if crate::verify_facts::enabled() {
        witness_value(ctx, val);
    }
    let word = if sabotaged("restraw") {
        ctx.block().bitcast_double_to_i64(val)
    } else {
        let v = ctx.block().toint32(val);
        let wide = ctx.block().zext(I32, &v, I64);
        ctx.block().or(I64, &wide, INT32_TAG_I64)
    };
    let ptr = slot(ctx, arr, &idx);
    // GC_STORE_AUDIT(POINTER_FREE): an I32 word is in the 0x7FFE band, which no collector takes for a pointer.
    ctx.block().store(I64, &word, &ptr);
    ctx.block().br(&merge_l);

    ctx.current_block = cold;
    ctx.block().call_void(
        "js_array_rest_set",
        &[(DOUBLE, arr), (DOUBLE, key), (DOUBLE, val)],
    );
    ctx.block().br(&merge_l);

    ctx.current_block = merge;
    val.to_string()
}

/// The negative control `PERRY_FACTS_SABOTAGE=<arm>`.
fn sabotaged(arm: &str) -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok(arm)
}

/// Check mode: the slot at `ptr` holds an I32 word.
fn witness_word(ctx: &mut FnCtx<'_>, arr: &str, ptr: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "word" }),
    );
    let raw = ctx.block().load(I64, ptr);
    let hi = ctx.block().lshr(I64, &raw, "32");
    let hi = ctx.block().trunc(I64, &hi, I32);
    let ok = ctx.block().icmp_eq(I32, &hi, INT32_TAG_HI_I32);
    miss_unless(
        ctx,
        &ok,
        arr,
        "rest elem: an at-rest slot holds no I32 word",
    );
}

/// Check mode: the stored `val` is an int32 Number that is not `-0`.
fn witness_value(ctx: &mut FnCtx<'_>, val: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "value" }),
    );
    let blk = ctx.block();
    let ge = blk.fcmp("oge", val, "-2147483648.0");
    let le = blk.fcmp("ole", val, "2147483647.0");
    let in_range = blk.and(I1, &ge, &le);
    let safe = blk.select(I1, &in_range, DOUBLE, val, "0.5");
    let int = blk.fptosi(DOUBLE, &safe, I32);
    let back = blk.sitofp(I32, &int, DOUBLE);
    let exact = blk.fcmp("oeq", &back, val);
    let bits = blk.bitcast_double_to_i64(val);
    let not_negative_zero = blk.icmp_ne(I64, &bits, "-9223372036854775808");
    let ok = blk.and(I1, &exact, &not_negative_zero);
    miss_unless(
        ctx,
        &ok,
        val,
        "rest elem: an at-rest store of a value that is no int32",
    );
}

/// Report a false proof (exit 86) unless `ok`.
fn miss_unless(ctx: &mut FnCtx<'_>, ok: &str, value: &str, what: &str) {
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("rest.verify_miss");
    let next = ctx.new_block("rest.verify_ok");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::ArrayRep, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
