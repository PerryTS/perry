//! Step 4b, stage 1, slice 1 — a read region formed over a `+` tree (#10884).
//!
//! # What a region is
//!
//! A region is defined by the CFG, not by an expression form: a single-entry
//! run of accesses over which one receiver's `(unmasked pointer, ShapeId)` pair
//! is held, entered through ONE shape compare, whose failure leaves for a
//! generic copy of the whole run and never rejoins it (design doc §L7.1–L7.3).
//!
//! This file forms the first slice of that: the runs that already sit inside
//! one `+` tree (`h += o.a + o.b + o.c`), because that is where the leaves are
//! already collected. The same program spelled across statements
//! (`const a = o.a; const b = o.b; h += a + b;`) is the next slice and is NOT
//! formed here — it is measured as a negative control so the gap is known.
//!
//! # The rule (design doc §L7.3), and why it is sound
//!
//! ```text
//! [R1] guard   tag test + unmask + ONE ShapeId compare      ─┐ the only two
//! [R2] load    every key's slot, from one atomic region word │ bail edges,
//! [R3] verify  every leaf value is a primitive Number        ─┘ before any effect
//! [R4] use     fold the tree with `fadd`
//! ```
//!
//! **The region does not compute the right answer when an operand is
//! unfriendly; it declines before computing one.** Hoisting every leaf above
//! the additions is exactly what #10904 did wrong. It is legal here *because*
//! R3 proves every leaf is a Number, so no addition can reach `ToPrimitive`
//! and therefore no user code can run between a leaf's source position and
//! where it was read. The proof is ordered load → check → use: a failed check
//! discards the loaded values and lowers the tree afresh in the generic copy,
//! in source order, so a wrong hoist is never observed.
//!
//! Discarding and re-evaluating is only legal because every leaf admitted here
//! is effect-free to evaluate: a read of the guarded receiver (the shape proves
//! it is an own data property, so no getter), a local, or a numeric literal.
//! That is precisely what #10921 could not assume for an arbitrary tree.
//!
//! # Supplier
//!
//! The expected ShapeId is learned (supplier (b), §L14.18.4): a per-region
//! atomic word primed on a miss by `js_region_guard_prime`. The id and every
//! key's slot live in ONE word so a concurrent prime can never pair one shape's
//! id with another's slots. A link-time constant (step 4) would replace the
//! word load and nothing else.
//!
//! # The miss side is today's code
//!
//! Every failure edge lands in the generic copy, which is the post-#10921
//! lowering of the same tree. A mispredicted region therefore costs a few
//! compares on top of what the tree costs without regions — never a cliff
//! (#10503's 18× is what a miss into the by-name ladder costs; this never goes
//! there). Priming is bounded to `PRIME_ATTEMPTS` per region for the life of
//! the process, so a polymorphic or inherited-read site stops paying for it.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use super::region_guard::{
    self, emit_miss, emit_prime, emit_r1, emit_slot_loads, state_globals, MAX_KEYS,
};
use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1};

enum Leaf<'a> {
    /// A read of the region's receiver; the index names its key.
    Region(usize),
    /// A local or a numeric literal — effect-free to evaluate twice.
    Other(&'a Expr),
}

struct Plan<'a> {
    receiver: u32,
    keys: Vec<&'a str>,
    leaves: Vec<Leaf<'a>>,
}

fn add_leaves<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        add_leaves(left, out);
        add_leaves(right, out);
    } else {
        out.push(expr);
    }
}

/// Slice 1's admission: every leaf is effect-free to evaluate, and at least two
/// of them read ONE local receiver by a static key.
fn plan(expr: &Expr) -> Option<Plan<'_>> {
    let mut leaves = Vec::new();
    add_leaves(expr, &mut leaves);
    let mut receiver: Option<u32> = None;
    let mut keys: Vec<&str> = Vec::new();
    let mut region_reads = 0usize;
    let mut out = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        match leaf {
            Expr::PropertyGet {
                object, property, ..
            } => {
                let Expr::LocalGet(id) = object.as_ref() else {
                    return None;
                };
                match receiver {
                    None => receiver = Some(*id),
                    Some(r) if r == *id => {}
                    Some(_) => return None,
                }
                let key = match keys.iter().position(|k| *k == property.as_str()) {
                    Some(i) => i,
                    None => {
                        keys.push(property.as_str());
                        keys.len() - 1
                    }
                };
                region_reads += 1;
                out.push(Leaf::Region(key));
            }
            Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) => out.push(Leaf::Other(leaf)),
            _ => return None,
        }
    }
    if region_reads < 2 || keys.len() > MAX_KEYS {
        return None;
    }
    Some(Plan {
        receiver: receiver?,
        keys,
        leaves: out,
    })
}

/// Rebuild the tree's shape with `fadd`, consuming leaf values in leaf order.
fn fold(ctx: &mut FnCtx<'_>, expr: &Expr, values: &[String], next: &mut usize) -> String {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        let l = fold(ctx, left, values, next);
        let r = fold(ctx, right, values, next);
        return ctx.block().fadd(&l, &r);
    }
    let v = values[*next].clone();
    *next += 1;
    v
}

/// Lower `expr` as a read region, or return `None` to use the ordinary path.
pub(crate) fn try_lower_region_add_tree(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    if !region_guard::emission_allowed() {
        return Ok(None);
    }
    let Some(plan) = plan(expr) else {
        return Ok(None);
    };
    region_guard::note_expr_region(
        plan.leaves
            .iter()
            .filter(|l| matches!(l, Leaf::Region(_)))
            .count() as u64,
    );

    // Region state: one atomic word (id + slots) and a prime-attempt counter.
    let sites = state_globals(ctx);

    // The non-receiver leaves first. They are effect-free, and lowering them
    // before the receiver means nothing that could allocate runs between the
    // receiver's unmask and its slot loads.
    let mut other_values: Vec<Option<String>> = Vec::with_capacity(plan.leaves.len());
    let mut other_needs_test: Vec<bool> = Vec::with_capacity(plan.leaves.len());
    for leaf in &plan.leaves {
        match leaf {
            Leaf::Other(e) => {
                other_values.push(Some(lower_expr(ctx, e)?));
                other_needs_test.push(!crate::type_analysis::expr_produces_canonical_raw_f64(
                    ctx, e,
                ));
            }
            Leaf::Region(_) => {
                other_values.push(None);
                other_needs_test.push(true);
            }
        }
    }

    let r2_idx = ctx.new_block("region.r2");
    let fold_idx = ctx.new_block("region.fold");
    let miss_idx = ctx.new_block("region.miss");
    let prime_idx = ctx.new_block("region.prime");
    let generic_idx = ctx.new_block("region.generic");
    let merge_idx = ctx.new_block("region.merge");
    let r2_l = ctx.block_label(r2_idx);
    let fold_l = ctx.block_label(fold_idx);
    let miss_l = ctx.block_label(miss_idx);
    let prime_l = ctx.block_label(prime_idx);
    let generic_l = ctx.block_label(generic_idx);
    let merge_l = ctx.block_label(merge_idx);

    // R1: one guard for the whole run (shared emitter).
    let recv = lower_expr(ctx, &Expr::LocalGet(plan.receiver))?;
    let entry = emit_r1(ctx, &recv, &sites, &r2_l, &miss_l, &generic_l);

    // R2 + R3: every key's slot from the same word, then prove every leaf is a
    // Number before any addition runs.
    ctx.current_block = r2_idx;
    let key_values = emit_slot_loads(ctx, &entry, plan.keys.len());
    let mut leaf_values: Vec<String> = Vec::with_capacity(plan.leaves.len());
    for (i, leaf) in plan.leaves.iter().enumerate() {
        leaf_values.push(match leaf {
            Leaf::Region(k) => key_values[*k].clone(),
            Leaf::Other(_) => other_values[i].clone().expect("lowered above"),
        });
    }
    let mut all_num: Option<String> = None;
    for (value, needs) in leaf_values.iter().zip(other_needs_test.iter()) {
        if !needs {
            continue;
        }
        let is_num = crate::stmt::emit_js_value_is_number(ctx, value);
        all_num = Some(match all_num {
            Some(prev) => ctx.block().and(I1, &prev, &is_num),
            None => is_num,
        });
    }
    match all_num {
        Some(cond) => ctx.block().cond_br(&cond, &fold_l, &generic_l),
        None => ctx.block().br(&fold_l),
    }

    // R4: nothing can call user code now, so the tree folds to `fadd`s.
    ctx.current_block = fold_idx;
    let fast = fold(ctx, expr, &leaf_values, &mut 0);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    // Miss: prime a bounded number of times, then retire (shared emitter).
    ctx.current_block = miss_idx;
    let tries = emit_miss(ctx, &sites, &prime_l, &generic_l);
    ctx.current_block = prime_idx;
    emit_prime(ctx, &sites, &entry, &tries, &plan.keys, &generic_l);

    // Generic copy: the same tree through the ordinary dispatch, in source
    // order — the code this region replaces.
    ctx.current_block = generic_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let slow = {
        let _suppressed = region_guard::Suppressed::enter();
        lower_expr(ctx, expr)?
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    Ok(Some(
        ctx.block()
            .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(recv: u32, key: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(recv)),
            property: key.to_string(),
            byte_offset: 0,
        }
    }
    fn add(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    #[test]
    fn two_reads_of_one_receiver_form_a_region() {
        let e = add(Expr::LocalGet(9), add(read(1, "a"), read(1, "b")));
        let p = plan(&e).expect("h + (o.a + o.b) is a slice-1 region");
        assert_eq!(p.receiver, 1);
        assert_eq!(p.keys, vec!["a", "b"]);
    }

    /// A repeated key is one slot, read twice.
    #[test]
    fn a_repeated_key_shares_its_slot() {
        let e = add(add(read(1, "c"), read(1, "a")), read(1, "c"));
        let p = plan(&e).unwrap();
        assert_eq!(p.keys, vec!["c", "a"]);
    }

    /// One read is not a run: the ordinary tower serves it unchanged.
    #[test]
    fn a_single_read_is_not_a_region() {
        assert!(plan(&add(Expr::LocalGet(9), read(1, "a"))).is_none());
    }

    /// Two receivers need two guards; slice 1 takes one.
    #[test]
    fn two_receivers_are_declined() {
        assert!(plan(&add(read(1, "a"), read(2, "a"))).is_none());
    }

    /// A call leaf is not effect-free, so re-evaluating it in the generic copy
    /// after a failed check could run it twice. Must decline.
    #[test]
    fn a_leaf_that_is_not_effect_free_is_declined() {
        let call = Expr::Call {
            callee: Box::new(Expr::LocalGet(7)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        };
        assert!(plan(&add(add(read(1, "a"), read(1, "b")), call)).is_none());
    }

    #[test]
    fn more_keys_than_one_word_holds_are_declined() {
        let mut e = read(1, "k0");
        for k in ["k1", "k2", "k3", "k4", "k5"] {
            e = add(e, read(1, k));
        }
        assert!(plan(&e).is_none());
    }
}
