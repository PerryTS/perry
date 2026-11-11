//! The fill birth: `new Array(k).fill(v)` born once, in its final
//! representation.
//!
//! The two-step lowering calls `js_array_constructor_single` (an Array of
//! `k` holes), tests that the receiver may not own a `fill`, calls the
//! native `js_array_fill` (every slot written again, then the layout
//! re-derived by reading every slot back), and for an at-rest birth
//! `js_array_rest_encode` (every slot rewritten as an I32 word). A birth of
//! eight zeroes is about 760 instructions.
//!
//! wpfacts names the fused fills that are one birth, with their length
//! (`ElisionFacts::fill_births`, wpfacts `fill_births`): the call is the
//! native one-argument `fill` on the fresh `new Array(k)` of an integer
//! literal `k` the runtime backs densely. Such a call is lowered here:
//!
//! - **the representation is the facts'**: an at-rest birth (`rest_births`)
//!   stores I32 words and has the capacity the facts reserve
//!   (`rest_reserves`); any other stores the value's word;
//! - **the layout is the runtime's**: the capacity is
//!   `perry_abi::fill_birth_capacity`, the layout bits
//!   `perry_abi::fill_birth_layout`, both shared with the runtime's
//!   `js_array_fill_birth`, which holds them to the two-step path;
//! - **inline** when the birth is small (`array_literal::
//!   inline_birth_admitted`: at most sixteen slots) and its value is a plain
//!   double by construction or an I32 word: the literal's bump allocation
//!   and header words (`array_literal::emit_array_birth`) and one store per
//!   slot. The value is lowered before the allocation and is no pointer, and
//!   the window from the bump to the last store holds no call, so the Array
//!   is never visible half-initialized;
//! - **one runtime entry** otherwise: `js_array_fill_birth(k, reserve, v,
//!   rest)` allocates and fills in one pass, and takes the two-step path
//!   itself for a value that is no Number.
//!
//! Evaluation order: `new Array(k)` of a literal `k` has no effect a program
//! can observe and cannot throw, so evaluating `v` before the allocation is
//! the two-step order.
//!
//! `PERRY_ARRAY_FILL_BIRTH=0` (wpfacts exports no fill birth) keeps the
//! two-step lowering. Probe (`PERRY_SITE_STATS=1`): `PERRY_SITE fill-birth
//! len=<k> cap=<slots> word=<f64|i32|value> form=<inline|call> <function>`,
//! and `fill-birth-refused why=<reason> <function>` for a listed call this
//! module does not take. Check mode (`PERRY_VERIFY_FACTS=1`): the born Array
//! has length `k`, and its first and last slot hold the fill word
//! (`Fact::ArrayRep`, exit 86). Negative control
//! `PERRY_FACTS_SABOTAGE=fillbirth`: an inline birth leaves its last slot a
//! hole.

use anyhow::Result;
use perry_hir::Expr;

use super::array_literal::{emit_array_birth, inline_birth_admitted, FillShape};
use super::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `INT32_TAG` (`0x7FFE_0000_0000_0000`).
const INT32_TAG_I64: &str = "9222809086901354496";

/// A fill birth this module lowers.
#[derive(Clone, Copy, Debug)]
struct Plan<'e> {
    /// The length `k`.
    len: u32,
    /// The elements are I32 words (an at-rest birth).
    rest: bool,
    /// The capacity an at-rest birth reserves (0: none).
    reserve: u32,
    /// The fill value.
    value: &'e Expr,
}

/// The negative control `PERRY_FACTS_SABOTAGE=<arm>`.
fn sabotaged(arm: &str) -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok(arm)
}

/// The integer a literal constructor argument is.
fn literal_length(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(k) => Some(*k),
        Expr::Number(n) if n.fract() == 0.0 && n.abs() <= 4_294_967_295.0 => {
            // `-0` is a length of 0 too.
            Some(*n as i64)
        }
        _ => None,
    }
}

/// The plan of `e` when the facts name it a fill birth and it is the shape
/// they name: `new Array(<k>).fill(<v>)`.
fn plan<'e>(ctx: &FnCtx<'_>, e: &'e Expr) -> Option<Plan<'e>> {
    if ctx.wpfacts_off || ctx.wpfacts.fill_births.is_empty() {
        return None;
    }
    let Expr::Call {
        callee,
        args,
        byte_offset,
        ..
    } = e
    else {
        return None;
    };
    let Expr::PropertyGet {
        object,
        property,
        byte_offset: member,
        ..
    } = callee.as_ref()
    else {
        return None;
    };
    let offset = if *member != 0 { *member } else { *byte_offset };
    if property != "fill" || offset == 0 {
        return None;
    }
    let len = *ctx.wpfacts.fill_births.get(&offset)?;
    let refuse = |why: &str| -> Option<Plan<'e>> {
        super::note_site(&format!("fill-birth-refused why={why} {}", ctx.func.name));
        None
    };
    // The window this module proves itself: the call is the shape the facts
    // describe, with the length they give.
    let (
        Expr::New {
            class_name,
            args: ctor_args,
            ..
        },
        [value],
    ) = (object.as_ref(), args.as_slice())
    else {
        return refuse("shape");
    };
    if class_name != "Array" || ctor_args.len() != 1 {
        return refuse("shape");
    }
    if literal_length(&ctor_args[0]) != Some(i64::from(len)) {
        return refuse("length");
    }
    let rest = ctx.wpfacts.rest_births.contains(&offset);
    let reserve = ctx
        .wpfacts
        .rest_reserves
        .get(&offset)
        .copied()
        .filter(|_| rest && !sabotaged("restcap"))
        .unwrap_or(0);
    Some(Plan {
        len,
        rest,
        reserve,
        value,
    })
}

/// Lower `e` as one birth when it is a fill birth: the born Array (a boxed
/// word). `None`: not one, the caller lowers the call as before.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(plan) = plan(ctx, e) else {
        return Ok(None);
    };
    let Plan {
        len,
        rest,
        reserve,
        value,
    } = plan;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "fill": "birth", "len": len, "rest": rest, "reserve": reserve }),
    );
    crate::facts_consumed::used();
    let capacity = perry_abi::fill_birth_capacity(len, reserve);
    // A plain double by construction needs no test and no note: its word is
    // the slot's final word.
    let canonical = !rest && crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
    let val = lower_expr(ctx, value)?;
    // The slot word, when the site knows it: the I32 word of an int32 Number
    // (the facts' element representation), or the plain double.
    let word = if rest {
        if crate::verify_facts::enabled() {
            super::rest_elems::witness_value(ctx, &val);
        }
        let blk = ctx.block();
        let int = blk.toint32(&val);
        let wide = blk.zext(I32, &int, I64);
        Some(blk.or(I64, &wide, INT32_TAG_I64))
    } else {
        canonical.then(|| val.clone())
    };
    let inline = word
        .as_ref()
        .filter(|_| inline_birth_admitted(ctx, len as usize, capacity as usize));
    super::note_site(&format!(
        "fill-birth len={len} cap={capacity} word={} form={} {}",
        if rest {
            "i32"
        } else if canonical {
            "f64"
        } else {
            "value"
        },
        if inline.is_some() { "inline" } else { "call" },
        ctx.func.name
    ));
    let handle = match inline {
        Some(word) => {
            let mut words = vec![word.clone(); len as usize];
            if sabotaged("fillbirth") {
                // The negative control: the last slot stays a hole.
                let hole = crate::nanbox::TAG_HOLE;
                words[len as usize - 1] = if rest {
                    hole.to_string()
                } else {
                    double_literal(f64::from_bits(hole))
                };
            }
            emit_array_birth(
                ctx,
                len as usize,
                Some(FillShape {
                    capacity: capacity as usize,
                    rest,
                }),
                &[],
                &[],
                false,
                |_| Ok(words),
            )?
        }
        None => ctx.block().call(
            I64,
            "js_array_fill_birth",
            &[
                (I32, &len.to_string()),
                (I32, &reserve.to_string()),
                (DOUBLE, &val),
                (I32, if rest { "1" } else { "0" }),
            ],
        ),
    };
    if crate::verify_facts::enabled() {
        witness(ctx, &handle, len, word.as_deref(), rest);
    }
    Ok(Some(nanbox_pointer_inline(ctx.block(), &handle)))
}

/// Check mode: the born Array `handle` has length `len`, and when the site
/// knows the slot word (`word`: an `i64` I32 word when `rest`, else a plain
/// double) its first and last slot hold it.
fn witness(ctx: &mut FnCtx<'_>, handle: &str, len: u32, word: Option<&str>, rest: bool) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "fill": "born", "len": len }),
    );
    let blk = ctx.block();
    let head = blk.and(I64, handle, POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &head);
    let born_len = blk.load(I32, &ptr);
    let mut ok = blk.icmp_eq(I32, &born_len, &len.to_string());
    if let Some(word) = word.filter(|_| len != 0) {
        let want = if rest {
            word.to_string()
        } else {
            blk.bitcast_double_to_i64(word)
        };
        let base = crate::target_layout::ARRAY_HEADER_SIZE_BYTES;
        for i in [0, u64::from(len) - 1] {
            let slot = blk.gep(I8, &ptr, &[(I64, &(base + 8 * i).to_string())]);
            let got = blk.load(I64, &slot);
            let same = blk.icmp_eq(I64, &got, &want);
            ok = blk.and(I1, &ok, &same);
        }
    }
    let boxed = nanbox_pointer_inline(ctx.block(), handle);
    super::rest_elems::miss_unless(
        ctx,
        &ok,
        &boxed,
        "fill birth: the born Array does not hold its length and fill value",
    );
}
