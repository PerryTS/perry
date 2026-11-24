//! The intervals a rule reads off a node, and the joins into a node's own.

use crate::model::*;

impl Model {
    /// An operand's interval for `range::eval`: its own while it holds only
    /// Numbers (nothing to coerce), empty while it holds nothing, else ⊤.
    /// What a value-copy delivery from `n` carries beside its values.
    pub(crate) fn rg(&self, n: NodeId) -> crate::model::Rg {
        let nd = &self.nodes[n as usize];
        crate::model::Rg {
            num: nd.range,
            chr: nd.crange,
            frac: nd.frac,
        }
    }

    /// The code interval of `x`'s Strings when it holds only Strings
    /// (else ⊤; nothing: empty).
    pub(crate) fn crange_input(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            Iv::EMPTY
        } else if nd.pts.is_empty() && nd.prim == prim::STR {
            nd.crange
        } else {
            Iv::TOP
        }
    }

    /// `Number(x)` over everything `x` holds (`RangeOp::ToNumber`).
    pub(crate) fn to_number_range(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if !nd.pts.is_empty() || nd.prim & (prim::UNDEF | prim::BIGINT) != 0 {
            return Iv::TOP;
        }
        let mut out = Iv::EMPTY;
        if nd.prim & prim::NUM != 0 {
            out = out.join(nd.range);
        }
        if nd.prim & prim::STR != 0 {
            out = out.join(crate::model::range::chars_to_number(nd.crange));
        }
        if nd.prim & prim::BOOL != 0 {
            out = out.join(Iv::int(0, 1));
        }
        if nd.prim & prim::NULL != 0 {
            out = out.join(Iv::int(0, 0));
        }
        let known = prim::NUM | prim::STR | prim::BOOL | prim::NULL;
        if nd.prim & !known != 0 {
            return Iv::TOP;
        }
        out
    }

    pub(crate) fn range_input(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            Iv::EMPTY
        } else if nd.pts.is_empty() && nd.prim == prim::NUM {
            nd.range
        } else {
            Iv::TOP
        }
    }
}

/// Join `iv` into the node's interval; widen to ⊤ past `WIDEN_STEPS`
/// growths. Whether it grew.
/// Join `iv` into the node's code interval; past `WIDEN_STEPS` growths it
/// is every one-unit String (`CHAR_ANY`), unless ⊤.
pub(crate) fn grow_crange(node: &mut Node, iv: Iv) -> bool {
    let joined = node.crange.join(iv);
    if joined == node.crange {
        return false;
    }
    node.csteps = node.csteps.saturating_add(1);
    node.crange = if joined.is_top() || node.csteps <= crate::model::range::WIDEN_STEPS {
        joined
    } else if joined.within(crate::model::range::CHAR_ANY) {
        crate::model::range::CHAR_ANY
    } else {
        Iv::TOP
    };
    node.drange = true;
    true
}

pub(crate) fn grow_range(node: &mut Node, iv: Iv) -> bool {
    let joined = node.range.join(iv);
    if joined == node.range {
        return false;
    }
    use crate::model::range::{widen, WIDEN_LIMIT, WIDEN_STEPS};
    node.rsteps = node.rsteps.saturating_add(1);
    node.range = if node.rsteps > WIDEN_LIMIT {
        Iv::TOP
    } else if node.rsteps > WIDEN_STEPS {
        widen(node.range, joined)
    } else {
        joined
    };
    node.drange = true;
    true
}

/// Join `iv` into the result of a [`Cons::Range`] rule (R1, sum widening).
///
/// The rule's result is `op` over its operands' intervals at each
/// evaluation, and every operand only grows, so the join is the rule over
/// the operands' last intervals: a rule result needs no widening of its own.
/// The operands are what widen: a loop counter (a copy target, `grow_range`)
/// climbs the ladder and the loop test clips it back (`gen::narrow`), so
/// `r * 16 + f` over `r, f` in `[0, 7]` is `[0, 119]`, where widening the
/// sum after `WIDEN_STEPS` of its operands' growths jumped to a rung (127,
/// then 255).
///
/// Termination: the rule results form no cycle among themselves (each
/// rule's operands exist before its result node is ruled, and a result has
/// one rule), so every cycle passes a copy target, which widens, and a
/// result grows at most once per operand growth. Past `WIDEN_LIMIT` growths
/// it still climbs the ladder (a backstop that bounds even a cycle the
/// argument missed).
pub(crate) fn grow_rule_range(node: &mut Node, iv: Iv) -> bool {
    use crate::model::range::{widen, WIDEN_LIMIT};
    let joined = node.range.join(iv);
    if joined == node.range {
        return false;
    }
    node.rsteps = node.rsteps.saturating_add(1);
    node.range = if node.rsteps > WIDEN_LIMIT {
        widen(node.range, joined)
    } else {
        joined
    };
    node.drange = true;
    true
}
