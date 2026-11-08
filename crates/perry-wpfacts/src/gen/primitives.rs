use super::*;
impl<'a> Gen<'a> {
    /// A node holding exactly the primitive kinds `p`. A union of kinds is
    /// its own node: the shared all-primitives node would add `STR` and
    /// `SYMBOL`, and a string index reads every named member.
    /// A string literal: one source node per constant, holding it alone
    /// (`Node::strs`); a spelling of the solver's own keys is any string.
    pub fn str_literal(&mut self, s: &str) -> NodeId {
        let Some(a) = self.m.str_const_atom(s) else {
            return self.prim_node(prim::STR);
        };
        if let Some(&n) = self.m.str_literals.get(&a) {
            return n;
        }
        // A program-wide source, as the primitive constants are: no
        // summary packet owns it.
        #[cfg(test)]
        let scope = self.m.packet_scope.take();
        let n = self.m.node_named("STRLIT", || format!("STRLIT {s:?}"));
        let set = self.m.str_set(&[a]);
        self.m.add_s(n, &[], prim::STR, set);
        #[cfg(test)]
        {
            self.m.packet_scope = scope;
        }
        self.m.str_literals.insert(a, n);
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
}
