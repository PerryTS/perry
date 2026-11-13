//! An operand that holds nothing is a model gap, not "no value".
//!
//! A numeric rule (`Cons::Range`) evaluates its operands' intervals; an
//! operand with no value has the empty interval, and so has the result
//! (`range::eval`: an operation that never runs). When the other operand
//! holds values, the result node still gets the Numbers its `Arith` /
//! `Plus` rule gives, with no interval from this rule: the binding it
//! flows into keeps only its other intervals (`let sum = 0; sum +=
//! v.readInt32BE(i)` stays `[0,0]` while the read holds nothing). The
//! model has no read that holds nothing while the operation is reached
//! except a gap (a builtin result it does not describe), so once the
//! worklist is empty such a rule's result is ⊤ (`settle_empty_operands`).
//! Sound: ⊤ covers every Number.
//!
//! `PERRY_FACTS_EMPTY_OPERANDS=0` (`off`, `false`) turns the fallback off.

use super::*;

pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_EMPTY_OPERANDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

impl Model {
    /// Records a binary numeric rule attached to `n` (`Model::cons`).
    pub(crate) fn note_range_rule(&mut self, n: NodeId, c: &Cons) {
        if !self.empty_operands {
            return;
        }
        if let Cons::Range { op, other, dst, .. } = *c {
            if other != NO_NODE && !matches!(op, RangeOp::SClip(_) | RangeOp::ToNumber) {
                self.range_rules.push((n, other, dst));
            }
        }
    }

    /// Once the worklist is empty: the result of each binary numeric rule
    /// one of whose operands holds values and the other none, and which
    /// holds Numbers, is ⊤. True when one grew.
    pub(crate) fn settle_empty_operands(&mut self) -> bool {
        if self.range_rules.is_empty() {
            return false;
        }
        let empty = |m: &Model, x: NodeId| {
            let nd = &m.nodes[x as usize];
            nd.pts.is_empty() && nd.prim == 0
        };
        let mut more = false;
        for i in 0..self.range_rules.len() {
            let (n, other, dst) = self.range_rules[i];
            if self.range_gaps.contains(&dst) || empty(self, n) || !empty(self, other) {
                continue;
            }
            if self.nodes[dst as usize].prim & prim::NUM == 0 {
                continue;
            }
            self.range_gaps.insert(dst);
            let d = &mut self.nodes[dst as usize];
            if crate::solve::grow_range(d, Iv::TOP) && !d.in_wl {
                d.in_wl = true;
                self.wl.push_back(dst);
            }
            more = true;
        }
        more
    }
}
