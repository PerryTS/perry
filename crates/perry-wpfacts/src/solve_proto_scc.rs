//! Collapse cycles in the private prototype-value equations. Concrete
//! prototype chains are acyclic; context-insensitive factory templates can
//! form cycles. Their mutually inclusive value vertices have equal sets.

use crate::model::*;

impl Model {
    pub(crate) fn prototype_root(&self, mut n: NodeId) -> NodeId {
        while let Some(parent) = self.nodes[n as usize].prototype_alias {
            n = parent;
        }
        n
    }

    pub(crate) fn merge_prototype_cycle(
        &mut self,
        source: NodeId,
        dest: NodeId,
        log: bool,
    ) -> bool {
        let Some(key) = self.nodes[source as usize].prototype_key else {
            return false;
        };
        if self.nodes[dest as usize].prototype_key != Some(key) {
            return false;
        }
        let mut previous = crate::pts::FxHashMap::default();
        let mut stack = vec![dest];
        previous.insert(dest, dest);
        let mut found = false;
        // Bounded discovery is an optimization: missing a long cycle leaves
        // the ordinary inclusion edges and their work bound in force.
        while let Some(n) = stack.pop() {
            self.work += 1;
            if n == source {
                found = true;
                break;
            }
            if previous.len() > 256 {
                break;
            }
            for &next in &self.nodes[n as usize].succ {
                let next = self.prototype_root(next);
                if self.nodes[next as usize].prototype_key == Some(key)
                    && !previous.contains_key(&next)
                {
                    previous.insert(next, n);
                    stack.push(next);
                }
            }
        }
        if !found {
            return false;
        }
        if log {
            self.rel(|| format!("copy {source} {dest}"));
        }
        let mut members = vec![source];
        let mut at = source;
        while at != dest {
            at = previous[&at];
            members.push(at);
        }
        self.merge_prototype_members(members);
        true
    }

    fn merge_prototype_members(&mut self, mut members: Vec<NodeId>) {
        members.sort_unstable();
        members.dedup();
        let root = members[0];
        let mut values = Vec::new();
        let mut prims = 0;
        let mut wide = false;
        let mut succ = Vec::new();
        let mut calls = Vec::new();
        let mut constraints = Vec::new();
        let mut builtin = false;
        for &n in &members {
            let node = &self.nodes[n as usize];
            values.extend(node.pts.to_sorted());
            prims |= node.prim;
            wide |= node.widened;
            succ.extend(node.succ.iter().copied());
            succ.extend(node.prim_succ.iter().copied());
            calls.extend(node.call_succ.iter().copied());
            for &c in &node.cons {
                if matches!(c, Cons::BuiltinCallee) {
                    if builtin {
                        continue;
                    }
                    builtin = true;
                }
                constraints.push(c);
            }
        }
        // Keep losers' vectors until any active solver iteration finishes.
        // All subsequent add/edge/cons operations resolve their representative.
        for &n in &members[1..] {
            self.nodes[n as usize].prototype_alias = Some(root);
            if let Some(sites) = self.callee_sites.get(&n).cloned() {
                self.callee_sites.entry(root).or_default().extend(sites);
            }
            if self.builtin_callee.contains(&n) {
                self.builtin_callee.insert(root);
            }
            self.rel(|| format!("copy {n} {root}"));
            self.rel(|| format!("copy {root} {n}"));
        }
        let node = &mut self.nodes[root as usize];
        node.succ.clear();
        node.prim_succ.clear();
        node.call_succ.clear();
        node.succ_set = None;
        node.cons.clear();
        if wide && !node.widened {
            node.widened = true;
            node.pts = crate::pts::Pts::default();
            node.pts.insert(TOP);
            node.delta.clear();
            node.delta.push(TOP);
            if !node.in_wl {
                node.in_wl = true;
                self.wl.push_back(root);
            }
            self.rel(|| format!("widen {root}"));
        }
        self.add(root, &values, prims);
        // Replaying full sets onto the merged users preserves late fields,
        // prototype mutations, receivers, and native namespace call sites.
        let mut seen = crate::pts::FxHashSet::default();
        for dst in succ {
            let dst = self.prototype_root(dst);
            if dst != self.prototype_root(root) && seen.insert(dst) {
                self.edge(root, dst);
            }
        }
        let mut seen = crate::pts::FxHashSet::default();
        for dst in calls {
            if seen.insert(dst) {
                self.call_edge(root, dst);
            }
        }
        for c in constraints {
            self.cons(root, c);
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::{gen::Gen, model::*};

    #[test]
    fn prototype_cycle_union_keeps_late_fields() {
        let mut g = Gen::new(vec![], false, false);
        let m = &mut g.m;
        let key = m.atom("payload");
        let a = m.tok(TokKind::NativeProto("cycle-A".into()));
        let b = m.tok(TokKind::NativeProto("cycle-B".into()));
        let pa = m.proto_node(a);
        let pb = m.proto_node(b);
        m.add_tok(pa, b);
        m.add_tok(pb, a);
        let x = m.tok(TokKind::ModNs(1));
        let y = m.tok(TokKind::ModNs(2));
        let fa = m.field(a, key);
        let fb = m.field(b, key);
        m.add_tok(fa, x);
        m.add_tok(fb, y);
        let result = m.node_l("result");
        m.load_from_prototypes(a, Key::Named(key), result);
        m.solve();
        let opaque = m.tok(TokKind::BuiltinFn);
        let mut expected = vec![x, y, opaque];
        expected.sort_unstable();
        assert_eq!(m.nodes[result as usize].pts.to_sorted(), expected);
        let z = m.tok(TokKind::ModNs(3));
        m.add_tok(fb, z);
        m.solve();
        assert!(m.nodes[result as usize].pts.contains_tok(z));
    }

    #[test]
    fn a_large_prototype_cycle_does_not_materialize_each_union() {
        let mut g = Gen::new(vec![], false, false);
        let m = &mut g.m;
        let key = m.atom("payload");
        let protos: Vec<_> = (0..64)
            .map(|i| m.tok(TokKind::NativeProto(format!("cycle-{i}"))))
            .collect();
        for i in 0..64 {
            let pn = m.proto_node(protos[i]);
            m.add_tok(pn, protos[(i + 1) % 64]);
            let f = m.field(protos[i], key);
            let a = m.tok(TokKind::ModNs(i as u32 * 2));
            let b = m.tok(TokKind::ModNs(i as u32 * 2 + 1));
            m.add(f, &[a, b], 0);
        }
        let result = m.node_l("result");
        m.load_from_prototypes(protos[0], Key::Named(key), result);
        m.solve();
        assert_eq!(m.nodes[result as usize].pts.len(), 129);
        assert!(m.pts_entries < 4000, "{}", m.size_line());
    }
}
