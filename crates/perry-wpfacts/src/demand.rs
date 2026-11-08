//! Int demand (representation RFC §10.3): a node has int demand when one of
//! the values it holds can reach an int consumer.
//!
//! An int consumer is an operand the language converts to an integer: an
//! element key (`Expr::IndexGet` / `IndexSet` / `IndexUpdate` and the
//! buffer forms) and the operands of the bitwise operators and `Math.imul`.
//! The generator marks those operand nodes (`Node::int_use`). After the solve
//! the demand flows backwards: from a node to every node with a copy edge
//! into it (`Node::succ`), and from an integer rule's result to its operands
//! (`Cons::Range` for the operators whose integer result is computed from the
//! operand's integer value: `+ - * %`, unary `-`, `++`/`--`, a loop test's
//! clip, `Math.abs` / `min` / `max` and the integral roundings).
//!
//! Demand decides no soundness question. An int32-proven Number with demand
//! takes the I32 representation, one without stays F64 (it feeds only double
//! arithmetic, so an i32 slot would only add conversions, #7128). A node the
//! walk misses keeps F64, which costs time, never correctness.
//!
//! `PERRY_FACTS_INT_DEMAND=0` seeds nothing: no node has demand.

use crate::model::{Cons, Model, NodeId, RangeOp, TokKind, NO_NODE};

/// `Model::bind_done`'s tag of a completed argument binding (`bind_args`).
const BOUND: u32 = u32::MAX - 1;

/// The rule's result carries the operand's integer value: demand on the
/// result is demand on the operand.
fn carries(op: RangeOp) -> bool {
    matches!(
        op,
        RangeOp::Add
            | RangeOp::Sub
            | RangeOp::Mul
            | RangeOp::Mod
            | RangeOp::Neg
            | RangeOp::Step(_)
            | RangeOp::Clip(_)
            | RangeOp::Abs
            | RangeOp::Min
            | RangeOp::Max
            | RangeOp::Integral
    )
}

/// The operator converts its operands to int32 (ToInt32 / ToUint32).
pub(crate) fn int_operands(op: RangeOp) -> bool {
    matches!(
        op,
        RangeOp::BitAnd
            | RangeOp::BitOr
            | RangeOp::BitXor
            | RangeOp::Shl
            | RangeOp::Shr
            | RangeOp::UShr
            | RangeOp::BitNot
            | RangeOp::Imul
    )
}

pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_INT_DEMAND").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Every node whose value can reach an int consumer gets `int_demand`.
pub(crate) fn solve(m: &mut Model) {
    let n = m.nodes.len();
    // The reverse graph, in compressed rows: `preds[start[d]..start[d + 1]]`
    // are the nodes demand on `d` reaches.
    let mut start = vec![0u32; n + 1];
    let each_edge = |m: &Model, f: &mut dyn FnMut(NodeId, NodeId)| {
        for (src, node) in m.nodes.iter().enumerate() {
            let src = src as NodeId;
            for &d in &node.succ {
                f(src, d);
            }
            for c in &node.cons {
                if let Cons::Range { op, dst, .. } = c {
                    if carries(*op) {
                        f(src, *dst);
                    }
                }
            }
        }
        // A call binds its arguments to the callee's parameters. A
        // summarized callee (`Model::bind_summarized`) adds no copy edge
        // for that, so the binding itself is the edge here: demand on a
        // parameter is demand on every argument a call passes it.
        for &(call, t, tag) in &m.bind_done {
            if tag != BOUND {
                continue;
            }
            let TokKind::Func(func, _) = m.toks[t as usize] else {
                continue;
            };
            let params = &m.funcs[func as usize].params;
            for (&a, &p) in m.calls[call as usize].args.iter().zip(params) {
                if a != NO_NODE && p != NO_NODE {
                    f(a, p);
                }
            }
        }
        // A summarized function's instance runs its body on copies of the
        // body's nodes (`Inst::fresh`); the emitted body is the shared
        // nodes. Demand a run sees (its calls are bound, the shared body's
        // are not) is demand on the node the run copies.
        for inst in 0..m.sum.insts.len() as u32 {
            m.inst_copies(inst, |orig, copy| {
                if orig != NO_NODE {
                    f(orig, copy)
                }
            });
        }
    };
    each_edge(m, &mut |_, d| start[d as usize + 1] += 1);
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut preds = vec![0 as NodeId; start[n] as usize];
    each_edge(m, &mut |s, d| {
        preds[fill[d as usize] as usize] = s;
        fill[d as usize] += 1;
    });
    drop(fill);
    let mut work: Vec<NodeId> = Vec::new();
    for (i, node) in m.nodes.iter_mut().enumerate() {
        if node.int_use {
            node.int_demand = true;
            work.push(i as NodeId);
        }
    }
    while let Some(d) = work.pop() {
        let (a, b) = (start[d as usize] as usize, start[d as usize + 1] as usize);
        for &s in &preds[a..b] {
            let node = &mut m.nodes[s as usize];
            if !node.int_demand {
                node.int_demand = true;
                work.push(s);
            }
        }
    }
}
