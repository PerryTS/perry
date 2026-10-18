//! Shared values at a prototype holder. Receiver-specific accessors remain
//! in the existing accessor walks; these nodes summarize data/method values.

use crate::{builtins, model::*};

impl Model {
    /// Feed a site's result directly from shared prototype summaries. A
    /// receiver-specific union relay would duplicate every value before the
    /// site's own union and prevent deduplication across its receiver tokens.
    pub(crate) fn load_from_prototypes(&mut self, object: Tok, key: Key, dst: NodeId) {
        let parents = self.proto_node(object);
        self.cons(parents, Cons::ChainUnion { key, dst });
    }

    pub(crate) fn call_from_prototypes(
        &mut self,
        object: Tok,
        key: Key,
        call: CallId,
        origin: Tok,
    ) {
        let parents = self.proto_node(object);
        self.cons(parents, Cons::ProtoCall { origin, key, call });
    }

    pub(crate) fn prototype_call_values(
        &mut self,
        origin: Tok,
        key: Key,
        call: CallId,
        parents: &[Tok],
    ) {
        for &parent in parents {
            if parent == TOP {
                self.call_tok(call, TOP, Some(origin));
                if self.is_constructor_key(key) {
                    let function = self.tok(TokKind::Builtin(BuiltinNs::Function));
                    self.call_tok(call, function, Some(origin));
                }
            } else {
                let value = self.prototype_value(parent, key);
                self.bind_via(value, call, origin);
            }
        }
    }

    /// Transpose receivers × functions into two ordinary inclusion edges:
    /// collect receivers once, then link that set to each function's `this`.
    pub(crate) fn add_receiver_group(&mut self, callee: NodeId, origin: Tok) {
        let recv = if let Some(&recv) = self.this_groups.get(&callee) {
            recv
        } else {
            let recv = self.node_l("method-receivers");
            // A solver relay, not a new program value or widening boundary.
            self.nodes[recv as usize].internal = true;
            self.this_groups.insert(callee, recv);
            self.cons(callee, Cons::ThisGroup { recv });
            recv
        };
        self.add_tok(recv, origin);
    }

    pub(crate) fn bind_receiver_group(&mut self, recv: NodeId, funcs: &[Tok]) {
        for &t in funcs {
            match self.toks[t as usize] {
                TokKind::Func(f) if !self.funcs[f as usize].lexical_this => {
                    self.edge(recv, self.funcs[f as usize].this);
                }
                TokKind::Top | TokKind::BuiltinFn => {
                    let r = self.reason("receiver of a call of an unknown function value");
                    self.sink_node(recv, r);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn prototype_value(&mut self, holder: Tok, key: Key) -> NodeId {
        if let Some(&value) = self.prototype_values.get(&(holder, key)) {
            return value;
        }
        let value = self.node_l("prototype-value");
        // Install before following parents: evaluated heritage can be cyclic
        // in the context-insensitive graph (e.g. repeated mixin templates).
        self.prototype_values.insert((holder, key), value);
        self.rel(|| format!("prototypevalue {value} {holder} {}", self.key_name(key)));
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
            self.load_from_prototypes(holder, key, value);
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
            let result = m.node_l("site-result");
            m.load_from_prototypes(recv, key, result);
            results.push(result);
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
    #[test]
    fn shared_method_receivers_replay_late_functions_and_unknown_targets() {
        let mut g = Gen::new(vec![], false, false);
        let m = &mut g.m;
        let mut functions = Vec::new();
        for lexical_this in [false, false, true] {
            let f = m.funcs.len() as FnIdx;
            let token = m.tok(TokKind::Func(f));
            let this = m.node_l("test-this");
            let ret = m.node_l("test-return");
            let args_all = m.node_l("test-args");
            m.funcs.push(FuncInfo {
                module: 0,
                func_id: f,
                name: format!("method-{f}"),
                params: vec![],
                rest: None,
                this,
                ret,
                args_all,
                token,
                ctor_of: None,
                lexical_this,
            });
            functions.push((token, this));
        }
        let callee = m.node_l("test-methods");
        let a = m.tok(TokKind::ModNs(1));
        let b = m.tok(TokKind::ModNs(2));
        m.add_receiver_group(callee, a);
        m.add_tok(callee, functions[0].0);
        m.add_tok(callee, functions[2].0);
        m.solve();
        m.add_receiver_group(callee, b);
        m.add_tok(callee, functions[1].0);
        m.solve();
        for &(_, this) in &functions[..2] {
            assert!(m.nodes[this as usize].pts.contains_tok(a));
            assert!(m.nodes[this as usize].pts.contains_tok(b));
        }
        assert!(m.nodes[functions[2].1 as usize].pts.is_empty());
        m.add_tok(callee, TOP);
        m.solve();
        assert!(m.is_escaped(a) && m.is_escaped(b));
        let c = m.tok(TokKind::ModNs(3));
        m.add_receiver_group(callee, c);
        m.solve();
        assert!(m.is_escaped(c));
    }
}
