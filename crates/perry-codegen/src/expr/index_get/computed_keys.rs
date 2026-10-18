//! Finite computed string keys on prefix-stable receivers: one branch per
//! proven key and its existing fixed slot. No receiver or key is re-evaluated.
use crate::expr::{wpfacts_access, FnCtx};
use crate::rooting;
use crate::types::{DOUBLE, I32};
use anyhow::Result;
use perry_hir::{types::Type, Expr};

fn add_type(ty: &Type, keys: &mut Vec<String>) -> Option<()> {
    match ty {
        Type::StringLiteral(key) => {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
        Type::Union(types) => {
            for ty in types {
                add_type(ty, keys)?;
            }
        }
        _ => return None,
    }
    (keys.len() <= 6).then_some(())
}

fn candidates(ctx: &FnCtx<'_>, expr: &Expr, keys: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::String(key) => add_type(&Type::StringLiteral(key.clone()), keys),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            candidates(ctx, then_expr, keys)?;
            candidates(ctx, else_expr, keys)
        }
        Expr::LocalGet(id) => add_type(ctx.stable_local_type_proof(id)?, keys),
        _ => None,
    }
}

pub(crate) fn try_lower_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    // The fixed-slot proof is about a binding, not a claim from its TS type.
    if !matches!(object, Expr::LocalGet(_) | Expr::This) {
        return Ok(None);
    }
    let mut keys = Vec::new();
    if candidates(ctx, index, &mut keys).is_none() || keys.len() < 2 {
        return Ok(None);
    }
    let Some(proofs) = keys
        .iter()
        .map(|key| {
            let p = wpfacts_access::prove(ctx, object, key, wpfacts_access::Access::Get)?;
            (!p.guarded() && !p.nullable).then_some(p)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    rooting::with_operands_rooted(ctx, &[object, index], |ctx, values| {
        let done = ctx.new_block("finitekey.done");
        let done_l = ctx.block_label(done);
        let mut incoming = Vec::new();
        for (key, proof) in keys.iter().zip(&proofs) {
            let entry = ctx.strings.intern(key);
            let global = format!("@{}", ctx.strings.entry(entry).handle_global);
            let literal = ctx.block().load(DOUBLE, &global);
            let equal = ctx.block().call(
                I32,
                "js_jsvalue_equals",
                &[(DOUBLE, &values[1]), (DOUBLE, &literal)],
            );
            let equal = ctx.block().icmp_ne(I32, &equal, "0");
            let hit = ctx.new_block("finitekey.slot");
            let next = ctx.new_block("finitekey.next");
            let hit_l = ctx.block_label(hit);
            let next_l = ctx.block_label(next);
            ctx.block().cond_br(&equal, &hit_l, &next_l);
            ctx.current_block = hit;
            let ptr = wpfacts_access::emit_proof_slot_ptr(ctx, &values[0], proof);
            let value = ctx.block().load(DOUBLE, &ptr);
            let site = ctx.typed_feedback_site_id(ctx.ic_site_counter);
            ctx.ic_site_counter += 1;
            wpfacts_access::emit_witness(ctx, &values[0], &value, key, proof, 0, site);
            incoming.push((value, ctx.block().label.clone()));
            ctx.block().br(&done_l);
            ctx.current_block = next;
        }
        // Retain full key coercion and receiver semantics on any miss. This
        // also makes the checked branch safe if selection is widened later.
        let value = ctx.block().call(
            DOUBLE,
            "js_dyn_index_get",
            &[(DOUBLE, &values[0]), (DOUBLE, &values[1])],
        );
        incoming.push((value, ctx.block().label.clone()));
        ctx.block().br(&done_l);
        ctx.current_block = done;
        let edges: Vec<_> = incoming
            .iter()
            .map(|(v, b)| (v.as_str(), b.as_str()))
            .collect();
        Ok(Some(ctx.block().phi(DOUBLE, &edges)))
    })
}

#[cfg(test)]
#[path = "computed_keys_tests.rs"]
mod tests;
