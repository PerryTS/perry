//! An operand that holds nothing is a model gap, not "no value".
//!
//! A numeric rule (`Cons::Range`, with its `Arith` / `Plus` kinds)
//! evaluates its operands; an operand with no value gives the result no
//! interval (`range::eval`: an operation that never runs), and for `+` no
//! kinds either (its Number part waits on a `Guard` of the other operand).
//! The binding the result flows into keeps only its other values: `let sum
//! = 0; sum += v.readInt32BE(i)` stays `[0,0]` while the read holds
//! nothing. The model has no read that holds nothing while its operation
//! is reached except a gap (a builtin result it does not describe), so once
//! the worklist is empty an empty operand of a rule whose other operand
//! holds values takes every primitive kind with the interval ⊤
//! (`settle_empty_operands`): the rule's result is then every value such an
//! operation can make, and no interval is claimed. No object token: nothing
//! runs or escapes that did not before.
//!
//! `PERRY_FACTS_EMPTY_OPERANDS=0` (`off`, `false`; `FactsConfig::
//! no_empty_operands`) turns the fallback off.

use super::*;

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

    /// Once the worklist is empty: each operand of a binary numeric rule
    /// that holds nothing while the other operand holds values takes every
    /// primitive kind, interval ⊤. True when one did.
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
            let (n, other, _) = self.range_rules[i];
            if empty(self, n) || !empty(self, other) || !self.range_gaps.insert(other) {
                continue;
            }
            let saved = self.cur_range.take();
            self.add_prim(other, prim::ALL);
            self.cur_range = saved;
            more = true;
        }
        more
    }
}
