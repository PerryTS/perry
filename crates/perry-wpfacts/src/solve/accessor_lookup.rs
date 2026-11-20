//! Passive descriptor traversal over the ordinary prototype/visit relations.
use crate::builtins;
use crate::model::*;

impl Model {
    pub(super) fn lookup_holder(
        &mut self,
        origin: Tok,
        holder: Tok,
        key: Key,
        dst: NodeId,
        setter: bool,
    ) {
        self.add_prim(dst, prim::UNDEF);
        if holder == TOP || (self.is_marker(holder) && !self.virt_protos.contains_key(&holder)) {
            self.add_top(dst, |_| {
                "accessor descriptor on an unknown prototype".into()
            });
            return;
        }
        let act = ChainAct::Lookup { dst, setter };
        if self
            .visits
            .entry(holder)
            .or_default()
            .insert((origin, key, act))
        {
            // The walk names every accessor it can see (`accessor_functions`
            // returns those under computed keys through their STAR_KEY
            // records); it is unknown exactly when unknown code may have
            // installed one on the holder: its any-key node holds ⊤ (the
            // holder escaped, or an accessor was defined under a computed
            // key). A data value under an unknown key is no accessor (an
            // ordinary [[Set]] makes a data property), so it leaves the walk
            // known. ⊤ is the rule's own result (`reason`), so the census
            // sees the any-key node that fired it as its source.
            let any = self.any_key_node(holder);
            let reason = self.reason("accessor descriptor of a changed holder");
            self.cons(any, Cons::TopIfTop { dst, reason });
            self.with_b(|m| {
                builtins::lookup_native_descriptor(m, origin, holder, key, dst, setter)
            });
        }
        if let Some(a) = self.shadow_key(holder, key) {
            self.note_shadowed(holder, a, act, false);
            return;
        }
        if self.stops_at(holder, key) {
            self.accessor_hit(origin, holder, key, act);
            self.stopped
                .entry(holder)
                .or_default()
                .push((origin, key, act));
            return;
        }
        self.chain_from(origin, holder, key, act);
    }

    /// SKINDESC1: the accessor functions an OWN property descriptor of `t`
    /// holds (ECMA-262 10.1.5 [[GetOwnProperty]], 6.2.6.4
    /// FromPropertyDescriptor): `lookup_holder` at `t` alone, no
    /// prototype walk. `undefined` for a data property or an absent key;
    /// an object whose own accessors the model does not name (a marker, an
    /// escaped holder, an unrepresented native accessor) gives ⊤ with the
    /// walk's own reasons.
    pub(crate) fn own_accessor_lookup(&mut self, t: Tok, key: Key, dst: NodeId, setter: bool) {
        self.add_prim(dst, prim::UNDEF);
        if self.is_marker(t) && !self.virt_protos.contains_key(&t) {
            self.add_top(dst, |_| "accessor descriptor of an unknown object".into());
            return;
        }
        let act = ChainAct::Lookup { dst, setter };
        if self.visits.entry(t).or_default().insert((t, key, act)) {
            let any = self.any_key_node(t);
            let reason = self.reason("accessor descriptor of a changed holder");
            self.cons(any, Cons::TopIfTop { dst, reason });
            self.with_b(|m| builtins::lookup_native_descriptor(m, t, t, key, dst, setter));
        }
        self.accessor_hit(t, t, key, act);
    }

    /// Return the canonical installed function tokens, including wildcard
    /// descriptors. No invocation group is created by a descriptor query.
    pub(super) fn accessor_functions(&mut self, holder: Tok, key: Key, dst: NodeId, setter: bool) {
        let star = self.atom(crate::STAR_KEY);
        let keys = match key {
            Key::Named(a) if a != star => vec![a, star],
            Key::Named(a) => vec![a],
            Key::Any(_) => self.accessor_keys.get(&holder).cloned().unwrap_or_default(),
        };
        for a in keys {
            if let Some((getters, setters)) = self.accessors.get(&(holder, a)) {
                let functions = if setter { setters } else { getters }.clone();
                for f in functions {
                    self.add_tok(dst, f);
                }
            }
        }
    }
}
