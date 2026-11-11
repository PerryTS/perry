//! Literal strings (wpfacts `literal_strings`): a store or an alias of a
//! value whose every String is a program string literal's value has no
//! string to demote.
//!
//! `js_string_addref_if_heap_string` marks a uniquely owned string
//! (`StringHeader::refcount == 1`, an append accumulator) shared when a
//! second holder appears, so a later in-place append copies instead. A
//! literal's value is born shared (an image record or a pool atom,
//! `refcount == 0`) and nothing appends to it in place, so for a value the
//! whole-program facts prove is, when it is a String at all, a literal's
//! value, the tag test and the call are dead. The facts:
//!
//! - `ElisionFacts::literal_string_locals`: the local holds only such
//!   Strings (a parameter every caller passes literals, a `let` of them);
//! - `ElisionFacts::literal_string_fields` (`FieldProof::literal_strings`):
//!   every store of any unit into the field stores only such Strings, so
//!   the value this proven store writes is one.
//!
//! Both are empty with `PERRY_FACTS_LITERAL_STRINGS=0` and when interpreted
//! code can run. Check mode (`PERRY_VERIFY_FACTS=1`) tests the claim on the
//! value at each consuming site: a `STRING_TAG` value whose header is not
//! shared is a false proof ("literal string", exit 86).

use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64};

use super::FnCtx;

/// `StringHeader::refcount` (perry-runtime `string/mod.rs`, pinned there by
/// the header's ABI assertion): `utf16_len`, `byte_len`, `capacity`, then
/// the refcount word.
const STRING_REFCOUNT_OFFSET: &str = "12";

fn usable(ctx: &FnCtx<'_>) -> bool {
    !ctx.wpfacts_off && !ctx.wpfacts.interp_guard && !super::typed_feedback_emission_enabled()
}

/// Local `id` holds only literals' values as Strings, by the facts.
fn local(ctx: &FnCtx<'_>, id: u32) -> bool {
    usable(ctx) && ctx.wpfacts.literal_string_locals.contains(&id)
}

/// The locals whose values `value` evaluates to, when it is a read of one
/// or a choice between such reads.
fn locals_of(value: &Expr, out: &mut Vec<u32>) -> bool {
    match value {
        Expr::LocalGet(id) => {
            out.push(*id);
            true
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => locals_of(then_expr, out) && locals_of(else_expr, out),
        Expr::Sequence(exprs) => exprs.last().is_some_and(|last| locals_of(last, out)),
        _ => false,
    }
}

/// The alias `let id = <value>` / `id = <value>` (lowered as `lowered`)
/// copies no string that needs demoting: the target, or every local the
/// value reads, holds only literals' values as Strings.
pub(crate) fn alias(ctx: &mut FnCtx<'_>, id: u32, value: &Expr, lowered: &str) -> bool {
    let mut sources = Vec::new();
    let by_source =
        locals_of(value, &mut sources) && sources.iter().all(|source| local(ctx, *source));
    if !by_source && !local(ctx, id) {
        return false;
    }
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::LiteralString,
        || serde_json::json!({ "local": id, "sources": sources }),
    );
    witness(ctx, lowered, &format!("local {id} holds literal strings"));
    true
}

/// The store of `value` (lowered as `lowered`) needs no string demote: the
/// proven field it writes holds only literals' values as Strings
/// (`field`), or every local the value reads does.
pub(crate) fn store(
    ctx: &mut FnCtx<'_>,
    field: Option<(&super::wpfacts_access::FieldProof, &str)>,
    value: &Expr,
    lowered: &str,
) -> bool {
    let by_field = field.is_some_and(|(proof, _)| usable(ctx) && proof.literal_strings);
    let mut sources = Vec::new();
    let by_source =
        locals_of(value, &mut sources) && sources.iter().all(|source| local(ctx, *source));
    if !by_field && !by_source {
        return false;
    }
    crate::facts_consumed::used();
    let what = match field {
        Some((proof, key)) if by_field => {
            crate::verify_facts::covered(
                crate::verify_facts::Fact::LiteralString,
                || serde_json::json!({ "classes": proof.class_ids, "key": key }),
            );
            format!("field {key} holds literal strings")
        }
        _ => {
            crate::verify_facts::covered(
                crate::verify_facts::Fact::LiteralString,
                || serde_json::json!({ "sources": sources }),
            );
            format!("locals {sources:?} hold literal strings")
        }
    };
    witness(ctx, lowered, &what);
    true
}

/// Check mode: `value` is no String, or a String whose header is shared.
fn witness(ctx: &mut FnCtx<'_>, value: &str, what: &str) {
    if !crate::verify_facts::enabled() {
        return;
    }
    // `STRING_TAG` in the word's top 16 bits.
    const TAG_MASK_I64: &str = "-281474976710656";
    const STRING_TAG_I64: &str = "9223090561878065152";
    let header = ctx.new_block("litstr.verify_header");
    let miss = ctx.new_block("litstr.verify_miss");
    let next = ctx.new_block("litstr.admitted");
    let (header_l, miss_l, next_l) = (
        ctx.block_label(header),
        ctx.block_label(miss),
        ctx.block_label(next),
    );
    crate::verify_facts::tick(ctx);
    let bits = ctx.block().bitcast_double_to_i64(value);
    let tag = ctx.block().and(I64, &bits, TAG_MASK_I64);
    let string = ctx.block().icmp_eq(I64, &tag, STRING_TAG_I64);
    ctx.block().cond_br(&string, &header_l, &next_l);
    ctx.current_block = header;
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let addr = ctx.block().add(I64, &handle, STRING_REFCOUNT_OFFSET);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let refcount = ctx.block().load(I32, &ptr);
    let shared = ctx.block().icmp_eq(I32, &refcount, "0");
    ctx.block().cond_br(&shared, &next_l, &miss_l);
    ctx.current_block = miss;
    let site =
        crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::LiteralString, &[]);
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

#[cfg(test)]
#[path = "literal_strings_tests.rs"]
mod tests;
