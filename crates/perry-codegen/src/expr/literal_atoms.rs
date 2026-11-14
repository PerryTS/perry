//! Literal atoms (wpfacts `literal_strings::atoms`): `x === "lit"` for an
//! operand whose every String is the string pool's one String of its
//! spelling.
//!
//! - `ElisionFacts::literal_atom_locals`: every String the local holds is
//!   the value of a program string literal the pool atomises;
//! - `ElisionFacts::literal_atom_fields` (`FieldProof::literal_atoms`): the
//!   same for a guard-free read of a definite birth field.
//!
//! The pool keeps one String per atomised spelling (an image record named
//! by its spelling program-wide; in unloadable output the agent's one
//! property atom of it, `js_string_pool_atom`), and the literal under test
//! is that String when it is atomised too (not empty, at most
//! `INTERN_MAX_BYTE_LEN` bytes). So the operand equals the literal exactly
//! when it is the literal's pointer: a String of another spelling is
//! another pointer, and a value that is no String is unequal and no pool
//! pointer. The compare is `bits == literal bits`, with none of the other
//! arms of `compare::lower_string_literal_strict_eq` (the other union
//! members, the SSO immediate, tag, atom flag, length and bytes).
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the complete compare runs beside
//! the pointer compare, and a difference is a false proof
//! (`perry_wpfacts_check_miss`, "literal atom", exit 86).
//! `PERRY_FACTS_LITERAL_ATOMS=0` turns the fact and this lowering off.

use perry_hir::Expr;

use crate::types::{DOUBLE, I1, I32, I64};

use super::FnCtx;

/// perry-runtime `string/intern.rs` `INTERN_MAX_BYTE_LEN`.
const ATOM_MAX_BYTE_LEN: usize = 64;

fn usable(ctx: &FnCtx<'_>) -> bool {
    !ctx.wpfacts_off && !ctx.wpfacts.interp_guard && !super::typed_feedback_emission_enabled()
}

/// What proves that `operand` holds only pool atoms as Strings, for the
/// check-mode report: a local of `literal_atom_locals`, or a guard-free
/// read of a field of `literal_atom_fields`.
fn proven(ctx: &FnCtx<'_>, operand: &Expr) -> Option<String> {
    if !usable(ctx) {
        return None;
    }
    match operand {
        Expr::LocalGet(id) => ctx
            .wpfacts
            .literal_atom_locals
            .contains(id)
            .then(|| format!("local {id} holds literal atoms")),
        Expr::PropertyGet {
            object, property, ..
        } => {
            // Only a receiver that can have a field proof at all is asked.
            if ctx.wpfacts.literal_atom_fields.is_empty()
                || !matches!(object.as_ref(), Expr::LocalGet(_) | Expr::This)
            {
                return None;
            }
            let proof = super::wpfacts_access::prove(
                ctx,
                object,
                property,
                super::wpfacts_access::Access::Get,
            )?;
            (proof.literal_atoms && !proof.guarded())
                .then(|| format!("field {property} holds literal atoms"))
        }
        _ => None,
    }
}

/// `val === lit` as an `i1` when the facts prove `operand` (lowered as
/// `val`) holds only pool atoms and `lit` (lowered as `lit_box`, its pool
/// handle) is one: the pointer compare (module docs).
pub(crate) fn strict_eq(
    ctx: &mut FnCtx<'_>,
    operand: &Expr,
    val: &str,
    lit_box: &str,
    lit: &str,
) -> Option<String> {
    if lit.is_empty() || lit.len() > ATOM_MAX_BYTE_LEN {
        return None;
    }
    let what = proven(ctx, operand)?;
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::LiteralAtom,
        || serde_json::json!({ "compare": what, "literal": lit }),
    );
    let bits = ctx.block().bitcast_double_to_i64(val);
    let lit_bits = ctx.block().bitcast_double_to_i64(lit_box);
    let same = ctx.block().icmp_eq(I64, &bits, &lit_bits);
    if crate::verify_facts::enabled() {
        // The complete compare must agree with the pointer compare.
        let full = super::compare::lower_string_literal_strict_eq(ctx, val, lit_box, lit);
        let miss = ctx.new_block("litatom.verify_miss");
        let next = ctx.new_block("litatom.admitted");
        let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
        crate::verify_facts::tick(ctx);
        let differ = ctx.block().xor(I1, &same, &full);
        ctx.block().cond_br(&differ, &miss_l, &next_l);
        ctx.current_block = miss;
        let site =
            crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::LiteralAtom, &[]);
        let _ = crate::verify_facts::check(
            ctx,
            I32,
            "perry_wpfacts_check_miss",
            &[(DOUBLE, val)],
            &site,
        );
        ctx.block().unreachable();
        ctx.current_block = next;
    }
    Some(same)
}

#[cfg(test)]
#[path = "literal_atoms_tests.rs"]
mod tests;
