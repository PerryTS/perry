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

    /// The primitive type of an enum member's value. One lookup order, the
    /// one codegen's enum table has: the module's own enum member by its
    /// declaration (a string is a string, a number a number, a computed one
    /// either). An `Expr::EnumMember` with no such own member is an imported
    /// one that `perry_hir::fix_imported_enums` already resolved before this
    /// walk: it writes `EnumMember` only for a numeric member (a string member
    /// is inlined as `Expr::String`, a computed one is left a property read),
    /// and any other member would fail codegen. So it is a number.
    pub(crate) fn enum_member_prim(&self, mi: u32, enum_name: &str, member_name: &str) -> u16 {
        use perry_hir::EnumValue;
        match self.modules[mi as usize]
            .1
            .enums
            .iter()
            .filter(|e| e.name == enum_name)
            .find_map(|e| e.members.iter().find(|m| m.name == member_name))
            .map(|m| &m.value)
        {
            Some(EnumValue::String(_)) => prim::STR,
            Some(EnumValue::Computed) => prim::NUM | prim::STR,
            Some(EnumValue::Number(_)) | None => prim::NUM,
        }
    }

    /// The node of a numeric literal: a shared node per integer value,
    /// carrying its point interval (`model::range`), or the Number node.
    pub fn num_literal(&mut self, v: f64) -> NodeId {
        let iv = crate::model::Iv::literal(v);
        if iv.is_top() || !self.int_ranges {
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

    /// A String literal: of at most one code unit, a shared node with that
    /// code (`Node::crange`); any other, the String primitive node.
    pub fn str_literal(&mut self, s: &str) -> NodeId {
        let iv = crate::model::range::char_literal(s);
        if iv.is_top() {
            return self.prim_node(prim::STR);
        }
        if let Some(&n) = self.str_lits.get(&iv.lo) {
            return n;
        }
        let n = self.m.node_named("STRLIT", || format!("STRLIT{}", iv.lo));
        {
            let nd = &mut self.m.nodes[n as usize];
            nd.cruled = true;
            nd.crange = iv;
        }
        self.m.add_prim(n, prim::STR);
        self.str_lits.insert(iv.lo, n);
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
        if !self.int_ranges {
            return;
        }
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

    /// A Math builtin with an interval rule over its ToNumber'd operands (as
    /// `prim_op`: each operand is a ToPrimitive operand, the result a
    /// Number). A list of operands folds pairwise (`Math.min(a, b, c)`).
    pub(crate) fn math_range(
        &mut self,
        fx: &Fx,
        op: crate::model::RangeOp,
        operands: &[&perry_hir::Expr],
    ) -> NodeId {
        let r = self.m.reason("ToNumber of a Math operand");
        let mut nodes = Vec::with_capacity(operands.len());
        for o in operands {
            let v = self.expr(fx, o);
            self.m.cons(
                v,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            nodes.push(v);
        }
        let (first, rest) = nodes.split_first().expect("a Math call has an operand");
        if rest.is_empty() {
            let out = self.tmp("math");
            self.range_rule(op, *first, None, out);
            self.m.add_prim(out, prim::NUM);
            return out;
        }
        let mut acc = *first;
        for &n in rest {
            let out = self.tmp("math");
            self.range_rule(op, acc, Some(n), out);
            self.m.add_prim(out, prim::NUM);
            acc = out;
        }
        acc
    }
}
