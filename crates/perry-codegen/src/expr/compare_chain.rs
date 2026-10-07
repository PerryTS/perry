//! Short-circuit comparison graphs. The graph is compiler-owned control flow,
//! not a runtime cache: it contains no heap values or learned type information.
//!
//! Only stable, ordinary local reads and Number literals can be hoisted. In
//! particular globals, capture cells, writes, getters and calls are excluded.
//! The non-number arm reloads locals before EACH reached comparison, so a
//! coercion may collect without leaving a stale pointer in a carried register.

use std::collections::BTreeMap;

use anyhow::Result;
use perry_hir::{CompareOp, Expr, LogicalOp, UnaryOp};

use super::{lower_expr, FnCtx};
use crate::nanbox::{double_literal, TAG_TRUE_I64};
use crate::types::{DOUBLE, I1, I32, I64};

enum Operand {
    Local(u32),
    Number(f64),
}

type SlowInput = (String, String, String, String, String);
type SlowGroup = (usize, Vec<SlowInput>);

struct Node {
    op: CompareOp,
    left: Operand,
    right: Operand,
    yes: usize,
    no: usize,
}

// 0/1 are the false/true exits; node indices start at 2.
fn operand(ctx: &FnCtx<'_>, expr: &Expr) -> Option<Operand> {
    match expr {
        Expr::Integer(n) => Some(Operand::Number(*n as f64)),
        Expr::Number(n) => Some(Operand::Number(*n)),
        Expr::LocalGet(id)
            if ctx.locals.contains_key(id)
                && !ctx.reassigned_locals.contains(id)
                && !ctx.module_globals.contains_key(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                // Reading a scalar-replaced record can materialize a fresh
                // heap value. It is not an ordinary side-effect-free load.
                && !ctx.pod_records.contains_key(id) =>
        {
            Some(Operand::Local(*id))
        }
        _ => None,
    }
}

fn plan(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    yes: usize,
    no: usize,
    nodes: &mut Vec<Node>,
) -> Option<usize> {
    match expr {
        Expr::Bool(b) => Some(if *b { yes } else { no }),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => plan(ctx, operand, no, yes, nodes),
        Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        } => {
            let next = plan(ctx, right, yes, no, nodes)?;
            plan(ctx, left, next, no, nodes)
        }
        Expr::Logical {
            op: LogicalOp::Or,
            left,
            right,
        } => {
            let next = plan(ctx, right, yes, no, nodes)?;
            plan(ctx, left, yes, next, nodes)
        }
        Expr::Compare { op, left, right } => {
            let left = operand(ctx, left)?;
            let right = operand(ctx, right)?;
            nodes.push(Node {
                op: *op,
                left,
                right,
                yes,
                no,
            });
            Some(nodes.len() + 1)
        }
        _ => None,
    }
}

fn op_key(op: CompareOp) -> usize {
    match op {
        CompareOp::Eq => 0,
        CompareOp::Ne => 1,
        CompareOp::LooseEq => 2,
        CompareOp::LooseNe => 3,
        CompareOp::Lt => 4,
        CompareOp::Le => 5,
        CompareOp::Gt => 6,
        CompareOp::Ge => 7,
    }
}

fn numeric_pred(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq | CompareOp::LooseEq => "oeq",
        CompareOp::Ne | CompareOp::LooseNe => "une",
        CompareOp::Lt => "olt",
        CompareOp::Le => "ole",
        CompareOp::Gt => "ogt",
        CompareOp::Ge => "oge",
    }
}

fn slow_operand(ctx: &mut FnCtx<'_>, operand: &Operand) -> Result<String> {
    match operand {
        Operand::Number(n) => Ok(double_literal(*n)),
        Operand::Local(id) => lower_expr(ctx, &Expr::LocalGet(*id)),
    }
}

fn switch(ctx: &mut FnCtx<'_>, value: &str, targets: &[String]) {
    // Every destination is an index produced by the graph; the default is
    // unreachable, but uses the false exit to keep the IR total.
    let mut ir = format!("switch i32 {value}, label %{} [", targets[0]);
    for (i, target) in targets.iter().enumerate().skip(1) {
        ir.push_str(&format!(" i32 {i}, label %{target}"));
    }
    ir.push_str(" ]");
    ctx.block().emit_raw(ir);
    ctx.block().mark_terminated();
}

/// Lower a Boolean decision tree with dynamic stable operands. Classification
/// and compact-int decoding occur once per local. Numeric execution contains
/// only ordered/unordered fcmp and short-circuit branches. All non-numeric
/// comparisons of the same operator share one call and continuation switch;
/// ToPrimitive is still called separately for every reached relational node.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let mut nodes = Vec::new();
    let Some(entry) = plan(ctx, expr, 1, 0, &mut nodes) else {
        return Ok(None);
    };
    // Constant short circuits can leave nodes without incoming edges.
    // Do not classify bindings that JavaScript never reads.
    let mut live = vec![false; nodes.len()];
    let mut pending = vec![entry];
    while let Some(index) = pending.pop() {
        if index < 2 || live[index - 2] {
            continue;
        }
        live[index - 2] = true;
        pending.extend([nodes[index - 2].yes, nodes[index - 2].no]);
    }
    let mut remap = vec![0, 1];
    let mut count = 2;
    for reachable in &live {
        remap.push(count);
        if *reachable {
            count += 1;
        }
    }
    let entry = remap[entry];
    nodes = nodes
        .into_iter()
        .zip(live)
        .filter_map(|(mut node, reachable)| {
            if !reachable {
                return None;
            }
            node.yes = remap[node.yes];
            node.no = remap[node.no];
            Some(node)
        })
        .collect();
    if nodes.len() < 2 {
        return Ok(None);
    }

    let mut numbers = BTreeMap::new();
    for node in &nodes {
        for operand in [&node.left, &node.right] {
            if let Operand::Local(id) = operand {
                numbers.insert(*id, String::new());
            }
        }
    }
    if numbers.is_empty()
        || numbers.keys().all(|id| {
            crate::type_analysis::expr_produces_canonical_raw_f64(ctx, &Expr::LocalGet(*id))
        })
    {
        return Ok(None);
    }

    // Strict equality against a Number never coerces. Leaving non-number
    // tags as NaNs already gives the exact fcmp verdict, so these graphs need
    // only compact-int decoding and have no slow arm at all.
    let strict_numeric_equality = nodes.iter().all(|node| {
        matches!(node.op, CompareOp::Eq | CompareOp::Ne)
            && (matches!(node.left, Operand::Number(_)) || matches!(node.right, Operand::Number(_)))
    });
    let mut all_numbers = "true".to_string();
    for (id, number) in &mut numbers {
        let boxed = lower_expr(ctx, &Expr::LocalGet(*id))?;
        let bits = ctx.block().bitcast_double_to_i64(&boxed);
        let top = ctx.block().lshr(I64, &bits, "48");
        let int = ctx
            .block()
            .icmp_eq(I64, &top, crate::nanbox::INT32_TAG_TOP16_I64);
        if !strict_numeric_equality {
            // Signed ordering admits negative IEEE values and canonical NaNs;
            // non-number tags occupy the positive quiet-NaN band.
            let plain = ctx.block().icmp_slt(
                I64,
                &bits,
                &crate::nanbox::i64_literal(crate::nanbox::SHORT_STRING_TAG),
            );
            let numeric = ctx.block().or(I1, &plain, &int);
            all_numbers = ctx.block().and(I1, &all_numbers, &numeric);
        }
        let raw = ctx.block().trunc(I64, &bits, I32);
        let decoded = ctx.block().sitofp(I32, &raw, DOUBLE);
        *number = ctx.block().select(I1, &int, DOUBLE, &decoded, &boxed);
    }

    let false_idx = ctx.new_block("cmpgraph.false");
    let true_idx = ctx.new_block("cmpgraph.true");
    let merge_idx = ctx.new_block("cmpgraph.merge");
    let exits = [ctx.block_label(false_idx), ctx.block_label(true_idx)];
    let mut fast = exits.to_vec();
    let mut slow = exits.to_vec();
    let mut fast_blocks = Vec::new();
    let mut slow_blocks = Vec::new();
    for _ in &nodes {
        let f = ctx.new_block("cmpgraph.number");
        fast.push(ctx.block_label(f));
        fast_blocks.push(f);
        if !strict_numeric_equality {
            let s = ctx.new_block("cmpgraph.step");
            slow.push(ctx.block_label(s));
            slow_blocks.push(s);
        }
    }
    if strict_numeric_equality {
        ctx.block().br(&fast[entry]);
    } else {
        ctx.block()
            .cond_br(&all_numbers, &fast[entry], &slow[entry]);
    }

    let num_operand = |o: &Operand| match o {
        Operand::Number(n) => double_literal(*n),
        Operand::Local(id) => numbers[id].clone(),
    };
    // Each group is a shared slow comparison. Its phi includes the actual
    // JS operands and both continuations, never a coerced primitive.
    let mut groups: BTreeMap<usize, SlowGroup> = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        ctx.current_block = fast_blocks[i];
        let bit = ctx.block().fcmp(
            numeric_pred(node.op),
            &num_operand(&node.left),
            &num_operand(&node.right),
        );
        ctx.block().cond_br(&bit, &fast[node.yes], &fast[node.no]);

        if strict_numeric_equality {
            continue;
        }
        ctx.current_block = slow_blocks[i];
        let l = slow_operand(ctx, &node.left)?;
        let r = slow_operand(ctx, &node.right)?;
        let op = op_key(node.op);
        let key = if op < 4 { op & !1 } else { op };
        let (block, incoming) = groups
            .entry(key)
            .or_insert_with(|| (ctx.new_block("cmpgraph.coerce"), Vec::new()));
        let pred = ctx.block().label.clone();
        let (yes, no) = if op == 1 || op == 3 {
            (node.no, node.yes)
        } else {
            (node.yes, node.no)
        };
        incoming.push((l, r, yes.to_string(), no.to_string(), pred));
        let label = ctx.block_label(*block);
        ctx.block().br(&label);
    }
    for (key, (block, incoming)) in groups {
        ctx.current_block = block;
        let phi = |ctx: &mut FnCtx<'_>, ty, column: usize| {
            let entries: Vec<_> = incoming
                .iter()
                .map(|row| {
                    let value = match column {
                        0 => &row.0,
                        1 => &row.1,
                        2 => &row.2,
                        _ => &row.3,
                    };
                    (value.as_str(), row.4.as_str())
                })
                .collect();
            ctx.block().phi(ty, &entries)
        };
        let l = phi(ctx, DOUBLE, 0);
        let r = phi(ctx, DOUBLE, 1);
        let yes = phi(ctx, I32, 2);
        let no = phi(ctx, I32, 3);
        let bits = if key < 4 {
            let l = ctx.block().bitcast_double_to_i64(&l);
            let r = ctx.block().bitcast_double_to_i64(&r);
            ctx.block().call(
                I64,
                if key < 2 { "js_eq" } else { "js_loose_eq" },
                &[(I64, &l), (I64, &r)],
            )
        } else {
            let helper = ["js_rel_lt", "js_rel_le", "js_rel_gt", "js_rel_ge"][key - 4];
            let value = ctx
                .block()
                .call(DOUBLE, helper, &[(DOUBLE, &l), (DOUBLE, &r)]);
            ctx.block().bitcast_double_to_i64(&value)
        };
        let bit = ctx.block().icmp_eq(I64, &bits, TAG_TRUE_I64);
        let next = ctx.block().select(I1, &bit, I32, &yes, &no);
        switch(ctx, &next, &slow);
    }

    let merge = ctx.block_label(merge_idx);
    ctx.current_block = false_idx;
    ctx.block().br(&merge);
    ctx.current_block = true_idx;
    ctx.block().br(&merge);
    ctx.current_block = merge_idx;
    Ok(Some(
        ctx.block()
            .phi(I1, &[("false", &exits[0]), ("true", &exits[1])]),
    ))
}
