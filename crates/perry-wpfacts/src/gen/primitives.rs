use super::*;
impl<'a> Gen<'a> {
    /// A node holding exactly the primitive kinds `p`. A union of kinds is
    /// its own node: the shared all-primitives node would add `STR` and
    /// `SYMBOL`, and a string index reads every named member.
    pub fn prim_node(&mut self, p: u16) -> NodeId {
        match p {
            prim::NUM => self.prim_const[0],
            prim::STR => self.prim_const[1],
            prim::BOOL => self.prim_const[2],
            prim::NULL => self.prim_const[3],
            prim::UNDEF => self.prim_const[4],
            prim::BIGINT => self.prim_const[5],
            prim::SYMBOL => self.prim_const[6],
            prim::ALL => self.m.all_prim_node,
            _ => {
                let n = self.m.node_l("PRIMS");
                self.m.own(n);
                self.m.add_prim(n, p);
                n
            }
        }
    }

    /// The node of a numeric literal: a shared node per integer value,
    /// carrying its point interval (`model::range`), or the Number node.
    pub fn num_literal(&mut self, v: f64) -> NodeId {
        let iv = crate::model::Iv::literal(v);
        if iv.is_top() {
            return self.prim_node(prim::NUM);
        }
        if let Some(&n) = self.num_lits.get(&iv.lo) {
            return n;
        }
        let n = self.m.node_named("NUMLIT", || format!("NUMLIT{}", iv.lo));
        {
            let nd = &mut self.m.nodes[n as usize];
            nd.ruled = true;
            nd.range = iv;
        }
        self.m.add_prim(n, prim::NUM);
        self.num_lits.insert(iv.lo, n);
        n
    }

    /// Register the interval rule of a numeric operator: `dst` takes its
    /// interval from `op` over `l` (and `r`). On both operands, so a growth
    /// of either re-evaluates it.
    pub fn range_rule(
        &mut self,
        op: crate::model::RangeOp,
        l: NodeId,
        r: Option<NodeId>,
        dst: NodeId,
    ) {
        let other = r.unwrap_or(crate::model::NO_NODE);
        self.m.cons(
            l,
            Cons::Range {
                op,
                other,
                dst,
                right: false,
            },
        );
        if let Some(r) = r {
            self.m.cons(
                r,
                Cons::Range {
                    op,
                    other: l,
                    dst,
                    right: true,
                },
            );
        }
    }
}
