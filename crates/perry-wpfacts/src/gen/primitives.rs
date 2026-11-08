use super::*;
impl<'a> Gen<'a> {
    /// A node holding exactly the primitive kinds `p`. A union of kinds is
    /// its own node: the shared all-primitives node would add `STR` and
    /// `SYMBOL`, and a string index reads every named member.
    /// A string literal: one source node per constant, holding it alone
    /// (`Node::strs`) and, with guard intervals on, of at most one code unit
    /// its code (`Node::crange`). A spelling of the solver's own keys is any
    /// string (its code node, or the String primitive node).
    pub fn str_literal(&mut self, s: &str) -> NodeId {
        let iv = if self.guard_ranges {
            crate::model::range::char_literal(s)
        } else {
            crate::model::Iv::TOP
        };
        let Some(a) = self.m.str_const_atom(s) else {
            return self.char_literal(iv);
        };
        if let Some(&n) = self.m.str_literals.get(&a) {
            return n;
        }
        // A program-wide source, as the primitive constants are: no
        // summary packet owns it.
        #[cfg(test)]
        let scope = self.m.packet_scope.take();
        let n = self.m.node_named("STRLIT", || format!("STRLIT {s:?}"));
        if !iv.is_top() {
            let nd = &mut self.m.nodes[n as usize];
            nd.cruled = true;
            nd.crange = iv;
        }
        let set = self.m.str_set(&[a]);
        self.m.add_s(n, &[], prim::STR, set);
        #[cfg(test)]
        {
            self.m.packet_scope = scope;
        }
        self.m.str_literals.insert(a, n);
        n
    }

    /// A String of code interval `iv` that is any string: a shared node per
    /// code of one unit, else the String primitive node.
    fn char_literal(&mut self, iv: crate::model::Iv) -> NodeId {
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
