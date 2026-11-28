//! Rule R-IN: the result of `key in o` and `Reflect.has(o, key)` (ECMA-262
//! 13.10.1, 28.1.9: HasProperty, 7.3.12; OrdinaryHasProperty 10.1.7.1).
//!
//! The result node holds `true` once some value of `o` may have the key and
//! `false` once some value may lack it. An ordinary object of the program (a
//! literal, a class instance, an `Object.create` result, a function's
//! instance, a class prototype) has the key when it or a link of its
//! prototype chain holds a value under the key or under an unknown key, has
//! an accessor for it, has a `[[Prototype]]` write, or when the chain leaves
//! the program's prototypes before `%Object.prototype%`, or that intrinsic
//! has the key or may be written under it: the negation of
//! `derive::chain_absent`, the proof of an absent read. Each of those only
//! grows during the solve, so the answer is re-asked when the worklist is
//! empty (`settle_has_prop`) and stays `true` once given. Every other value
//! (⊤, a builtin object, an array, a function, a Proxy, a primitive key of
//! no known name) may have the key and may lack it.
use super::*;

impl Model {
    /// One value `t` of the object of a HasProperty with result `dst`.
    pub(super) fn has_prop_result(&mut self, t: Tok, key: Key, dst: NodeId) {
        let Key::Named(a) = key else {
            return self.add_prim(dst, prim::BOOL);
        };
        if !self.has_prop_ordinary(t) {
            return self.add_prim(dst, prim::BOOL);
        }
        // No proof that a value surely has the key: it may lack it.
        self.add_prim(dst, prim::FALSE);
        if self.nodes[dst as usize].prim & prim::TRUE != 0 || !self.has_seen.insert((t, a, dst)) {
            return;
        }
        // The chain's first link exists before the settle asks for it.
        self.proto_node(t);
        self.has_pending.push((t, a, dst));
    }

    /// An object whose own keys are what the program stores and defines on
    /// it, and whose chain the solve records (`proto_node`).
    fn has_prop_ordinary(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Class { .. }
                    | TmplKind::Lit { .. }
                    | TmplKind::FnInst { .. }
                    | TmplKind::ObjCreate { .. }
            ),
            TokKind::Proto(_) => true,
            _ => false,
        }
    }

    /// May `t` have key `a`: its own keys or its chain's (the negation of
    /// `derive::chain_absent`; a numeric name also as a Number-keyed store
    /// writes it, `NUM_KEY`), or a hard operation's unknown keys.
    fn has_prop_may(&mut self, t: Tok, a: Atom) -> bool {
        if self.hard.contains_key(&t) {
            return true;
        }
        let key = self.atoms[a as usize].clone();
        if !crate::derive::chain_absent(self, t, &key, a, true) {
            return true;
        }
        // A store under a computed key writes its shaped node until a
        // field of a name it admits exists (`key_store_node`,
        // `field_shaped`): on `t` or on a prototype of its chain.
        if self.has_shaped_store(t, a) {
            return true;
        }
        if let Some(&cp) = self.cp_nodes.get(&t) {
            let protos = self.nodes[cp as usize].pts.to_sorted();
            if protos.into_iter().any(|p| self.has_shaped_store(p, a)) {
                return true;
            }
        }
        if self.is_num_atom(a) {
            let num = self.atom(crate::model::NUM_KEY);
            if num != a && !crate::derive::chain_absent(self, t, crate::model::NUM_KEY, num, true) {
                return true;
            }
        }
        false
    }

    /// A computed-key store into `h` whose key may be `a` stored a value.
    fn has_shaped_store(&self, h: Tok, a: Atom) -> bool {
        self.shaped_of.get(&h).is_some_and(|v| {
            v.iter().any(|&(s, n, store)| {
                let nd = &self.nodes[n as usize];
                store
                    && (nd.dropped || !nd.pts.is_empty() || nd.prim != 0)
                    && self.key_may_name(Key::Any(s), a)
            })
        })
    }

    /// Rule R-IN at an empty worklist: each pending (object, key, result)
    /// whose object may now have the key gives `true`.
    pub(crate) fn settle_has_prop(&mut self) -> bool {
        if self.has_pending.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.has_pending);
        let mut kept = Vec::with_capacity(pending.len());
        for (t, a, dst) in pending {
            if self.nodes[dst as usize].prim & prim::TRUE != 0 {
                continue;
            }
            if self.has_prop_may(t, a) {
                self.add_prim(dst, prim::TRUE);
            } else {
                kept.push((t, a, dst));
            }
        }
        self.has_pending = kept;
        !self.wl.is_empty()
    }
}
