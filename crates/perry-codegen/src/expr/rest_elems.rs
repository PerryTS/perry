//! I32 at rest, PR B (representation RFC §10.7; DESIGN-n1-at-rest §3, §6):
//! the element sites and the births of at-rest Arrays.
//!
//! wpfacts (`rest_elems`) admits an Array template at rest when every
//! element of every Array it births is an int32 Number with int demand, the
//! Array's length is fixed, or moves only inside the capacity its birth
//! reserves, and dense below a floor (its storage never grows or moves and
//! holds no hole below the floor), and nothing but the element sites below
//! touches its elements. Such an Array holds each element as an I32 word
//! `INT32_TAG | (u32)v`. The facts name the sites by `element_offset`
//! (`ElisionFacts::rest_reads` / `rest_writes`) and the births by their
//! `fill` call's offset (`rest_births`); every receiver such a site may see
//! is an at-rest Array, so no site tests a kind at run time.
//!
//! - **Birth** (`encode_birth`): `new Array(k).fill(v)` lowers as it always
//!   does; its result is handed to `js_array_rest_encode`, which encodes the
//!   words and marks the header (`perry-runtime` `layout_slot`). A birth
//!   the facts give a capacity (`rest_reserves`, wpfacts `rest_elems::
//!   reserve`: its length may move inside a small proven hull) is handed to
//!   `js_array_rest_reserve` instead, which first reserves that many slots.
//!   Every store the facts allow then lands below the capacity, where the
//!   cold `js_array_rest_set` lengthens the Array in place, so the storage
//!   never moves (`FactsArrRep::unmoved`). `PERRY_REST_CAPACITY=0` turns the
//!   reservation off in the facts (no such Array is at rest).
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
//! **B2 (`PERRY_REST_TIERS=0` turns it off; B's lowering above).** The
//! storage kind is also part of the receiver's representation
//! (`FactsArrRep::at_rest`, wpfacts `ArrRep::at_rest`), so the rep-keyed
//! tiers that address `h + 24 + 8i` take an at-rest Array too and read its
//! word through [`decode`], this module's one decoding of the word:
//! - a direct read whose receiver the facts name (`proven_array::rep_read`)
//!   and whose key is an int32 takes the rep-keyed tier: in its floor (wpfacts
//!   `element_keys`) the load has no test at all, else one unsigned bound
//!   and the hole test;
//! - any other direct read or store with an int32 key tests `k <u floor`
//!   (one compare, no double round trip);
//! - a b1 field read call whose body read is at rest takes C3's in-floor
//!   load and the held bases of PR-D / N2 (`index_get::field_base`) as a
//!   double Array does.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): every at-rest load first tests that
//! the slot's high half is `INT32_TAG`, and every store that its value is an
//! int32 Number that is not `-0`; a miss reports the false proof and exits
//! 86 (`Fact::ArrayRep`). A cold store first tests that its key is below
//! the capacity ("never past capacity", `witness_capacity`). Negative
//! controls `PERRY_FACTS_SABOTAGE=restraw`: a store writes the value's double
//! word, unencoded; `restcap`: a birth reserves nothing.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::FnCtx;
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I1, I32, I64};

/// `INT32_TAG` (`0x7FFE_0000_0000_0000`) and its high half.
const INT32_TAG_I64: &str = "9222809086901354496";
const INT32_TAG_HI_I32: &str = "2147352576";

/// `PERRY_REST_TIERS=0` (`off`, `false`) keeps B's lowering: at-rest reads
/// take no rep-keyed tier, no held base and no int32 key (module docs).
pub(crate) fn tiers_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_REST_TIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// THE decoding of an at-rest slot word `raw` (an i64 that is no hole) for
/// `want`: its low half is the i32; any consumer but an int one takes its
/// canonical double. Every tier that loads an at-rest word calls this.
pub(crate) fn decode(ctx: &mut FnCtx<'_>, raw: &str, want: Want) -> String {
    let v = ctx.block().trunc(I64, raw, I32);
    match want {
        Want::I32 => v,
        Want::Value | Want::Number => ctx.block().sitofp(I32, &v, DOUBLE),
    }
}

/// The cold read of an at-rest Array (the accessor, then the generic
/// `Get`): element `idx` (an i32) of the boxed `arr`, coerced when
/// `numeric`.
pub(crate) fn cold_read(ctx: &mut FnCtx<'_>, arr: &str, idx: &str, numeric: bool) -> String {
    let key = ctx.block().sitofp(I32, idx, DOUBLE);
    let got = ctx.block().call(
        DOUBLE,
        "js_array_rest_get",
        &[(DOUBLE, arr), (DOUBLE, &key)],
    );
    if numeric {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &got)])
    } else {
        got
    }
}

/// Check mode: the slot word `raw` is an I32 word (`INT32_TAG` high half);
/// on a miss, jump to `miss_l`, else go on in a new block.
pub(crate) fn witness_raw(ctx: &mut FnCtx<'_>, raw: &str, next_l: &str, miss_l: &str) {
    let hi = ctx.block().lshr(I64, raw, "32");
    let hi = ctx.block().trunc(I64, &hi, I32);
    let ok = ctx.block().icmp_eq(I32, &hi, INT32_TAG_HI_I32);
    ctx.block().cond_br(&ok, next_l, miss_l);
}

/// The probe `PERRY_REST_DIAG=1`: one line per at-rest site lowered, with
/// the form it took (`rep` / `rep_floor`: the rep-keyed tier, without or
/// with its floor; `held`: a PR-D / N2 base; `c3`: a b1 call's in-floor
/// load; `i32`: one unsigned test; `f64`: B's double key).
pub(crate) fn probe(ctx: &FnCtx<'_>, op: &str, form: &str, offset: u32) {
    if std::env::var("PERRY_REST_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry rest] fn={} {op} @{offset} form={form}",
            ctx.func.name
        );
    }
}

/// The at-rest read `e` is a present slot by the facts: its key's interval
/// (wpfacts `element_keys`, this module's own site) is in the site's floor,
/// or a counter entry check proved its counter key in the floor
/// ([`counter_in_floor`]). Its value is then an int32 Number, never
/// `undefined` (`facts_canonical::of`, the `??` removal).
pub(crate) fn number_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::IndexGet {
        object,
        index,
        element_offset,
        ..
    } = e
    else {
        return false;
    };
    if !tiers_enabled() || ctx.wpfacts_off {
        return false;
    }
    let Some(floor) = read_floor(ctx, e) else {
        return false;
    };
    let foreign = matches!(ctx.rest_b1, Some((o, _)) if o == *element_offset);
    let keyed = !foreign
        && ctx
            .wpfacts
            .element_keys
            .get(element_offset)
            .is_some_and(|&(lo, hi)| lo >= 0 && hi < i64::from(floor));
    keyed || counter_in_floor(ctx, object, index)
}

/// The receiver `object` is an at-rest local and `index` the counter (or
/// `counter | 0`) of an enclosing loop copy whose entry check proved every
/// counter value in that local's floor (`FnCtx::rest_counter`).
pub(crate) fn counter_in_floor(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    let Expr::LocalGet(local) = object else {
        return false;
    };
    let Some(counter) = counter_of(index) else {
        return false;
    };
    ctx.rest_counter
        .iter()
        .any(|&(c, l, _)| c == counter && l == *local)
}

/// `index` is a local `i`, or `i | 0`: that local.
pub(crate) fn counter_of(index: &Expr) -> Option<u32> {
    match index {
        Expr::LocalGet(id) => Some(*id),
        Expr::Binary {
            op: perry_hir::BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => counter_of(left),
        _ => None,
    }
}

/// The floor a counter entry check may test for the at-rest local `id`: the
/// local's representation is an at-rest, dense `Arr` with a floor
/// (`proven_array::rep_read`).
pub(crate) fn local_floor(ctx: &FnCtx<'_>, id: u32) -> Option<u32> {
    if !tiers_enabled() || ctx.wpfacts_off {
        return None;
    }
    let object = Expr::LocalGet(id);
    let read = super::index_get::proven_array::rep_read(ctx, &object, 0)?;
    (read.rep.at_rest && read.rep.dense && read.rep.floor > 0).then_some(read.rep.floor)
}

/// `index` is an int32 at every evaluation: its i32 lowering is exact.
fn int32_key(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    super::index_get::numeric_index_has_integer_array_index_proof(ctx, index)
        || super::int_range_expr(ctx, index)
            .is_some_and(|r| r.min >= i64::from(i32::MIN) && r.max <= i64::from(i32::MAX))
}

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

/// The floor of an at-rest element store `e` (`a[k] = v` as an `IndexSet`
/// or a `PutValueSet`).
pub(crate) fn write_floor(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    match e {
        Expr::IndexSet { element_offset, .. } | Expr::PutValueSet { element_offset, .. }
            if *element_offset != 0 =>
        {
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

/// The capacity the at-rest birth `e` (`is_birth`) reserves, if any.
pub(crate) fn birth_reserve(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    let Expr::Call {
        callee,
        byte_offset,
        ..
    } = e
    else {
        return None;
    };
    let Expr::PropertyGet {
        byte_offset: member,
        ..
    } = callee.as_ref()
    else {
        return None;
    };
    let offset = if *member != 0 { *member } else { *byte_offset };
    ctx.wpfacts.rest_reserves.get(&offset).copied()
}

/// The at-rest Array `arr` (a boxed word) is born: reserve `reserve` slots
/// when the facts give a capacity (the head may be a new one), and encode
/// its words. The value is the born Array.
pub(crate) fn encode_birth(ctx: &mut FnCtx<'_>, arr: &str, reserve: Option<u32>) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "birth", "reserve": reserve }),
    );
    crate::facts_consumed::used();
    match reserve.filter(|_| !sabotaged("restcap")) {
        Some(cap) => ctx.block().call(
            DOUBLE,
            "js_array_rest_reserve",
            &[(DOUBLE, arr), (I32, &cap.to_string())],
        ),
        None => ctx
            .block()
            .call(DOUBLE, "js_array_rest_encode", &[(DOUBLE, arr)]),
    }
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

/// Lower the at-rest read `e` (`object[index]`, floor `floor`) for `want`.
pub(crate) fn lower_read(ctx: &mut FnCtx<'_>, e: &Expr, floor: u32, want: Want) -> Result<String> {
    let Expr::IndexGet {
        object,
        index,
        element_offset,
        ..
    } = e
    else {
        unreachable!("an at-rest read is an IndexGet")
    };
    let (object, index) = (object.as_ref(), index.as_ref());
    if tiers_enabled() && !ctx.wpfacts_off && int32_key(ctx, index) {
        // A b1 call's synthesized read carries the body module's offset: the
        // key facts here are this module's, so only the call's own verdicts
        // (`lower_call::field_read_call`) bound its key.
        let foreign = matches!(ctx.rest_b1, Some((o, _)) if o == *element_offset);
        let counted = counter_in_floor(ctx, object, index);
        let rep = super::index_get::proven_array::rep_read(ctx, object, *element_offset)
            .filter(|r| r.rep.at_rest)
            .map(|r| super::index_get::proven_array::RepRead {
                in_floor: (r.in_floor && !foreign) || counted,
                ..r
            });
        let form = match rep {
            Some(_) if counted => "counter",
            Some(r) if r.in_floor => "rep_floor",
            Some(_) => "rep",
            None => "i32",
        };
        probe(ctx, "read", form, *element_offset);
        return crate::rooting::with_operands_rooted_across(
            ctx,
            &[object],
            &[index],
            |ctx| super::lower_expr_as_i32(ctx, index),
            |ctx, v, idx| {
                Ok(match rep {
                    Some(read) => {
                        let numeric = want != Want::Value;
                        let value = super::index_get::proven_array::lower_rest(
                            ctx, &v[0], &idx, read, numeric,
                        );
                        match want {
                            Want::I32 if read.total(numeric) => {
                                ctx.block().fptosi(DOUBLE, &value, I32)
                            }
                            Want::I32 => ctx.toint32_wrap(&value),
                            _ => value,
                        }
                    }
                    None => read_at_i32(ctx, &v[0], &idx, floor, want),
                })
            },
        );
    }
    probe(ctx, "read", "f64", *element_offset);
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, v| {
        Ok(read_at(ctx, &v[0], &v[1], floor, want))
    })
}

/// [`read_at`] for an int32 key `idx`: `idx <u floor` is the whole test.
fn read_at_i32(ctx: &mut FnCtx<'_>, arr: &str, idx: &str, floor: u32, want: Want) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "read_i32", "floor": floor }),
    );
    crate::facts_consumed::used();
    let hit = ctx.block().icmp_ult(I32, idx, &floor.to_string());
    let key = ctx.block().sitofp(I32, idx, DOUBLE);
    read_slot(ctx, arr, &hit, idx, &key, want)
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
    read_slot(ctx, arr, &hit, &idx, key, want)
}

/// The slot `idx` of `arr` when `hit`, else the cold read of `key`.
fn read_slot(
    ctx: &mut FnCtx<'_>,
    arr: &str,
    hit: &str,
    idx: &str,
    key: &str,
    want: Want,
) -> String {
    let load = ctx.new_block("rest.load");
    let cold = ctx.new_block("rest.cold");
    let merge = ctx.new_block("rest.merge");
    let (load_l, cold_l, merge_l) = (
        ctx.block_label(load),
        ctx.block_label(cold),
        ctx.block_label(merge),
    );
    ctx.block().cond_br(hit, &load_l, &cold_l);

    ctx.current_block = load;
    let ptr = slot(ctx, arr, idx);
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
    if tiers_enabled() && !ctx.wpfacts_off && int32_key(ctx, index) {
        let counted = counter_in_floor(ctx, object, index);
        probe(ctx, "write", if counted { "counter" } else { "i32" }, 0);
        return crate::rooting::with_operands_rooted_across(
            ctx,
            &[object],
            &[index, value],
            |ctx| {
                let idx = super::lower_expr_as_i32(ctx, index)?;
                let val = super::lower_expr(ctx, value)?;
                Ok((idx, val))
            },
            |ctx, v, (idx, val)| {
                // A counter key a loop entry check proved in the floor needs
                // no test.
                let hit = if counted {
                    "true".to_string()
                } else {
                    ctx.block().icmp_ult(I32, &idx, &floor.to_string())
                };
                let key = ctx.block().sitofp(I32, &idx, DOUBLE);
                Ok(store_slot(ctx, &v[0], &hit, &idx, &key, &val))
            },
        );
    }
    probe(ctx, "write", "f64", 0);
    crate::rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, v| {
        Ok(store_at(ctx, &v[0], &v[1], &v[2], floor))
    })
}

/// Store `val` as element `key` of the at-rest Array `arr` (module docs).
fn store_at(ctx: &mut FnCtx<'_>, arr: &str, key: &str, val: &str, floor: u32) -> String {
    let (hit, idx) = index_in(ctx, key, floor);
    store_slot(ctx, arr, &hit, &idx, key, val)
}

/// Store `val` in slot `idx` of `arr` when `hit`, else the cold store of
/// `key`.
fn store_slot(
    ctx: &mut FnCtx<'_>,
    arr: &str,
    hit: &str,
    idx: &str,
    key: &str,
    val: &str,
) -> String {
    let store = ctx.new_block("rest.store");
    let cold = ctx.new_block("rest.store_cold");
    let merge = ctx.new_block("rest.stored");
    let (store_l, cold_l, merge_l) = (
        ctx.block_label(store),
        ctx.block_label(cold),
        ctx.block_label(merge),
    );
    ctx.block().cond_br(hit, &store_l, &cold_l);

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
    let ptr = slot(ctx, arr, idx);
    // GC_STORE_AUDIT(POINTER_FREE): an I32 word is in the 0x7FFE band, which no collector takes for a pointer.
    ctx.block().store(I64, &word, &ptr);
    ctx.block().br(&merge_l);

    ctx.current_block = cold;
    if crate::verify_facts::enabled() {
        witness_capacity(ctx, arr, key);
    }
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

/// Check mode: the key `key` (a double) of a cold store is below the
/// capacity of `arr`. The facts bound every index store of an at-rest Array
/// by the capacity its birth has (its fixed length, or the reservation), so
/// no store reallocates the storage. A negative key names a property, no
/// slot.
fn witness_capacity(ctx: &mut FnCtx<'_>, arr: &str, key: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "rest": "capacity" }),
    );
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr);
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &handle);
    let cap_ptr = blk.gep(crate::types::I8, &ptr, &[(I64, "4")]);
    let cap = blk.load(I32, &cap_ptr);
    let cap = blk.uitofp(I32, &cap, DOUBLE);
    // Unordered or below: a NaN key is no index either.
    let ok = blk.fcmp("ult", key, &cap);
    miss_unless(
        ctx,
        &ok,
        arr,
        "rest elem: an at-rest store at or past the capacity",
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
