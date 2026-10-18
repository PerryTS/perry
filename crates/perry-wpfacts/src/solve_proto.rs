//! Shared values at a prototype holder. Receiver-specific accessors remain
//! in the existing accessor walks; these nodes summarize data/method values.

use crate::{builtins, model::*};

impl Model {
    pub(crate) fn prototype_value(&mut self, holder: Tok, key: Key) -> NodeId {
        if let Some(&value) = self.prototype_values.get(&(holder, key)) {
            return value;
        }
        let value = self.node_l("prototype-value");
        // Install before following parents: evaluated heritage can be cyclic
        // in the context-insensitive graph (e.g. repeated mixin templates).
        self.prototype_values.insert((holder, key), value);
        let own = match key {
            Key::Named(a) => self.field(holder, a),
            Key::Any => self.all_fields(holder),
        };
        self.edge(own, value);
        self.with_b(|m| builtins::tok_load(m, holder, key, value));
        if self.stops_at(holder, key) {
            // Mutation resumes this one shared chain, including receivers
            // linked before the holder escaped or gained an accessor.
            self.stopped_shared
                .entry(holder)
                .or_default()
                .push((key, value));
        } else {
            let inherited = self.cv(holder, key);
            self.edge(inherited, value);
        }
        value
    }
}
#[cfg(test)]
mod tests {
    use crate::{gen::Gen, model::*};

    #[test]
    fn broad_shared_prototype_values_do_not_expand_per_receiver() {
        let mut g = Gen::new(vec![], false, false);
        let m = &mut g.m;
        m.widen_k = 128;
        let holder = m.tok(TokKind::NativeProto("test-holder".into()));
        let key = Key::Named(m.atom("payload"));
        let Key::Named(atom) = key else {
            unreachable!()
        };
        let field = m.field(holder, atom);
        let payloads: Vec<_> = (0..200).map(|i| m.tok(TokKind::ModNs(i))).collect();
        // Neither source exceeds the bound; their inherited union does.
        m.add(field, &payloads[..100], 0);
        let ancestor = m.tok(TokKind::NativeProto("test-ancestor".into()));
        let inherited = m.field(ancestor, atom);
        m.add(inherited, &payloads[100..], 0);
        let parents = m.proto_node(holder);
        m.add_tok(parents, ancestor);
        let mut results = Vec::new();
        for i in 0..1000 {
            let recv = m.tok(TokKind::NativeProto(format!("test-receiver-{i}")));
            let parents = m.proto_node(recv);
            m.add_tok(parents, holder);
            results.push(m.cv(recv, key));
        }
        m.solve();
        assert!(results
            .iter()
            .all(|&n| m.nodes[n as usize].pts.contains_tok(TOP)));
        assert!(payloads.iter().all(|&t| m.is_escaped(t)));
        // 1,000 receivers of a 200-value prototype must not materialize
        // the 200,000-value Cartesian product before widening each receiver.
        assert!(m.pts_entries < 100_000, "{}", m.size_line());
        assert!(m.work < 100_000, "work={}: {}", m.work, m.size_line());
    }
}
