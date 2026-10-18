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

    pub(crate) fn note_prototype_arc(&mut self, source: NodeId, dest: NodeId) {
        let Some(key) = self.nodes[source as usize].prototype_key else { return; };
        if self.nodes[dest as usize].prototype_key != Some(key)
            || self.nodes[source as usize].succ.contains(&dest) { return; }
        self.nodes[dest as usize].prototype_predecessors
            .get_or_insert_with(Default::default).insert(source);
        self.prototype_edges_since_scc += 1;
    }

    pub(crate) fn drain_prototype_work(&mut self) {
        self.drain_widened();
        if (self.pops & 1023 == 0 && self.prototype_edges_since_scc >= 1024)
            || (self.wl.is_empty() && self.prototype_edges_since_scc != 0) {
            self.collapse_prototype_cycles();
        }
    }

    /// Compute whole components in linear graph passes rather than searching
    /// for a path on every arc. Only private same-key union vertices merge.
    pub(crate) fn collapse_prototype_cycles(&mut self) {
        if self.prototype_merging || self.prototype_edges_since_scc == 0 { return; }
        self.prototype_merging = true;
        self.prototype_edges_since_scc = 0;
        let mut roots: Vec<_> = self.prototype_values.values()
            .map(|&n| self.prototype_root(n)).collect();
        self.work += roots.len() as u64;
        roots.sort_unstable(); roots.dedup();
        let index: crate::pts::FxHashMap<_,_> = roots.iter().enumerate()
            .map(|(i,&n)| (n,i)).collect();
        let mut graph = vec![Vec::new(); roots.len()];
        let mut reverse = vec![Vec::new(); roots.len()];
        for (i, &n) in roots.iter().enumerate() {
            for &dest in &self.nodes[n as usize].succ {
                self.work += 1;
                let dest = self.prototype_root(dest);
                if self.nodes[dest as usize].prototype_key != self.nodes[n as usize].prototype_key { continue; }
                if let Some(&j) = index.get(&dest) {
                    graph[i].push(j); reverse[j].push(i);
                }
            }
        }
        let mut seen = vec![false; roots.len()];
        let mut order = Vec::new();
        for i in 0..roots.len() {
            if seen[i] { continue; }
            let mut stack = vec![(i, false)];
            while let Some((at, exit)) = stack.pop() {
                self.work += 1;
                if exit { order.push(at); continue; }
                if seen[at] { continue; }
                seen[at] = true;
                stack.push((at, true));
                stack.extend(graph[at].iter().map(|&next| (next, false)));
            }
        }
        seen.fill(false);
        let mut components = Vec::new();
        for &i in order.iter().rev() {
            if seen[i] { continue; }
            let mut members = Vec::new();
            let mut stack = vec![i];
            while let Some(at) = stack.pop() {
                self.work += 1;
                if seen[at] { continue; }
                seen[at] = true;
                members.push(roots[at]);
                stack.extend(reverse[at].iter().copied());
            }
            if members.len() > 1 { components.push(members); }
        }
        for members in components { self.merge_prototype_members(members); }
        self.prototype_merging = false;
    }

    /// Redirect all incoming prototype arcs after merging representatives.
    /// Without this, distinct old targets repeatedly copy into the same union.
    fn repair_prototype_successors(&mut self, source: NodeId) {
        let mut edges = std::mem::take(&mut self.nodes[source as usize].succ);
        edges.extend(std::mem::take(&mut self.nodes[source as usize].prim_succ));
        self.work += edges.len() as u64;
        let mut seen = crate::pts::FxHashSet::default();
        let mut normal = Vec::new();
        let mut primitive = Vec::new();
        for target in edges {
            let target = self.prototype_root(target);
            if target == source || !seen.insert(target) { continue; }
            if self.nodes[target as usize].widened {
                primitive.push(target);
            } else {
                normal.push(target);
            }
        }
        let node = &mut self.nodes[source as usize];
        node.esc_out |= !primitive.is_empty();
        node.succ = normal;
        node.prim_succ = primitive;
        node.succ_set = if seen.len() >= 16 { Some(Box::new(seen)) } else { None };
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
        let mut incoming = crate::pts::FxHashSet::default();
        for &n in &members {
            let node = &self.nodes[n as usize];
            if let Some(preds) = &node.prototype_predecessors {
                incoming.extend(preds.iter().copied());
            }
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
        self.nodes[root as usize].prototype_predecessors = Some(Box::new(incoming.clone()));
        let mut repaired = crate::pts::FxHashSet::default();
        for source in incoming {
            let source = self.prototype_root(source);
            if source != root && repaired.insert(source) {
                self.repair_prototype_successors(source);
            }
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
        let roots: crate::pts::FxHashSet<_> = protos.iter()
            .map(|&p| m.prototype_root(m.prototype_values[&(p, Key::Named(key))]))
            .collect();
        assert_eq!(roots.len(), 1);
        assert!(m.pts_entries < 6000, "{}", m.size_line());
    }
    #[test]
    fn late_prototype_cycle_replays_method_receivers_and_native_sites() {
        let mut g = Gen::new(vec![], false, false);
        let m = &mut g.m;
        let key = m.atom("method");
        let a = m.tok(TokKind::ModNs(100));
        let b = m.tok(TokKind::ModNs(101));
        let ra = m.tok(TokKind::ModNs(102));
        let rb = m.tok(TokKind::ModNs(103));
        let mut functions = Vec::new();
        for primitive in [prim::NUM, prim::STR] {
            let f = m.funcs.len() as FnIdx;
            let token = m.tok(TokKind::Func(f));
            let this = m.node_l("test-this");
            let ret = m.node_l("test-ret");
            let args_all = m.node_l("test-args");
            m.funcs.push(FuncInfo { module: 0, func_id: f, name: format!("f{f}"),
                params: vec![], rest: None, this, ret, args_all, token,
                ctor_of: None, lexical_this: false });
            m.add_prim(ret, primitive);
            functions.push((token, this));
        }
        let fa = m.field(a, key);
        let fb = m.field(b, key);
        m.add_tok(fa, functions[0].0);
        m.add_tok(fb, functions[1].0);
        let pa = m.proto_node(a);
        let pb = m.proto_node(b);
        m.add_tok(pa, b);
        let mut results = Vec::new();
        for (holder, receiver) in [(a, ra), (b, rb)] {
            let ret = m.node_l("test-result");
            let call = m.calls.len() as CallId;
            m.calls.push(CallSite { this: ThisArg::Undef, args: vec![],
                spread: vec![], ret, site: 0 });
            let value = m.prototype_value(holder, Key::Named(key));
            m.bind_via(value, call, receiver);
            results.push(ret);
        }
        m.solve();
        assert!(!m.nodes[functions[0].1 as usize].pts.contains_tok(rb));
        m.add_tok(pb, a);
        m.solve();
        for &(_, this) in &functions {
            assert!(m.nodes[this as usize].pts.contains_tok(ra));
            assert!(m.nodes[this as usize].pts.contains_tok(rb));
        }
        for &ret in &results {
            assert_eq!(m.nodes[ret as usize].prim, prim::NUM | prim::STR);
        }
        let object = m.tok(TokKind::Builtin(BuiltinNs::Object));
        m.add_tok(fb, object);
        m.solve();
        for ret in results {
            assert!(m.nodes[ret as usize].pts.to_sorted().iter()
                .any(|&t| matches!(m.toks[t as usize], TokKind::Obj(_))));
        }
    }

}
