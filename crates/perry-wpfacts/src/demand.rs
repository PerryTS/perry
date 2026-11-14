//! Int demand (representation RFC §10.3): a node has int demand when one of
//! the values it holds can reach an int consumer.
//!
//! An int consumer is an operand the language converts to an integer: an
//! element key (`Expr::IndexGet` / `IndexSet` / `IndexUpdate` and the
//! buffer forms), the operands of the bitwise operators and `Math.imul`, and
//! (N1, `compare_demand`) the operand a strict equality or a `switch` case
//! tests against an int32 constant. The generator marks those operand nodes
//! (`Node::int_use`). After the solve
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

/// N1: an equality test against an int32 constant (`x === 5`, `x != -17`,
/// a `switch` case) is an int consumer of the other operand: on an I32
/// operand it is one integer compare, on a double it is `ucomisd` plus the
/// NaN branch. Relational tests (`i < n`) are not: a counter that only meets
/// a loop bound and feeds double arithmetic keeps F64 (#7128).
/// `PERRY_FACTS_COMPARE_DEMAND=0` seeds nothing here.
pub(crate) fn compare_demand() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_COMPARE_DEMAND").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `e` is an int32 Number constant: an integer literal, or the negation of
/// one, inside `[-2^31, 2^31)`, never `-0`.
pub(crate) fn int32_constant(e: &perry_hir::Expr) -> bool {
    use perry_hir::{Expr, UnaryOp};
    let v = match e {
        Expr::Integer(n) => *n as f64,
        Expr::Number(n) => *n,
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => match operand.as_ref() {
            Expr::Integer(n) if *n != 0 => -(*n as f64),
            Expr::Number(n) if *n != 0.0 => -*n,
            _ => return false,
        },
        _ => return false,
    };
    v.is_finite()
        && v.trunc() == v
        && !(v == 0.0 && v.is_sign_negative())
        && v >= f64::from(i32::MIN)
        && v <= f64::from(i32::MAX)
}

/// I32 at rest (RFC §10.7, PR A): the at-rest facts are on. Demand is then
/// solved before the derive, so a field's representation can read it
/// (`lib.rs`). `PERRY_FACTS_AT_REST=0` restores main's facts.
pub(crate) fn at_rest_demand() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_AT_REST").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
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
    let filters = m.zero_sign && crate::zero_sign::filter_demand();
    let each_edge = |m: &Model, f: &mut dyn FnMut(NodeId, NodeId)| {
        for (src, node) in m.nodes.iter().enumerate() {
            let src = src as NodeId;
            for &d in &node.succ {
                f(src, d);
            }
            for c in &node.cons {
                match c {
                    Cons::Range { op, dst, .. } if carries(*op) => f(src, *dst),
                    // PR D (`filters`): a filter delivers some of its host's
                    // values unchanged (`l ?? r`, `l && r`, a presence
                    // narrowing), and a guard delivers its `from` operand's
                    // (`r` of `l ?? r`, `l || r`, `l && r`): a copy of
                    // values, so demand on the result is demand on the
                    // operand.
                    Cons::NotNullish { dst }
                    | Cons::FieldTruthy { dst, .. }
                    | Cons::Fulfil { dst }
                    | Cons::Defined { dst }
                    | Cons::NotBuffer { dst, .. }
                    | Cons::NotUndefined { dst }
                    | Cons::Falsy { dst }
                    | Cons::Objects { dst }
                        if filters =>
                    {
                        f(src, *dst)
                    }
                    Cons::Guard { from, dst, .. } if filters => f(*from, *dst),
                    // A summarized run's result delivers its Numbers to the
                    // call's result unchanged (`Model::ret_into`).
                    Cons::RetInto { call, .. } if filters => {
                        let ret = m.calls[*call as usize].ret;
                        if ret != NO_NODE {
                            f(src, ret)
                        }
                    }
                    _ => {}
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
        // PR D (`filters`): an index read reads `num_fields`, the element
        // facts read the aggregate `all_fields` (`bindings::tok_arr_rep`):
        // views of the same slots, so demand on one is demand on each.
        if filters {
            crate::zero_sign::slot_views(m, f);
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
