//! For-in bodies of summarized functions run over the keys of the iterated value.

use super::*;

impl Model {
    /// Run for-in body `ft` (through template call `call`) over the iterated
    /// value `t` (FVA 4.3b): once per key of a literal whose keys are exactly
    /// its birth keys (`for_in_keys`), each run's key parameter holding its
    /// one key, else once with every key.
    pub(crate) fn for_in_tok(&mut self, call: CallId, ft: Tok, t: Tok) {
        if self.act.is_some() {
            let f = self.tok_fn(ft);
            self.activate(f, "for-in");
        }
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::ForIn(call, ft, t));
            return;
        }
        let f = self.tok_fn(ft);
        let keys = match self.summarized(f) && self.instantiable(ft) {
            true => self.for_in_keys(f, t, None),
            false => None,
        };
        let Some(keys) = keys else {
            return self.for_in_every_key(call, ft);
        };
        for k in keys {
            let inst = self.instance((6, k, ft));
            match self.sum.insts[inst as usize].params.first().copied() {
                // The run reads no key.
                Some(NO_NODE) => {}
                // A fresh slot: the run's key (its key variable's copy holds
                // the key's atom, `instance`).
                Some(p) if self.nodes[p as usize].frame == inst + 1 => self.add_prim(p, prim::STR),
                _ => return self.for_in_every_key(call, ft),
            }
        }
        self.for_in_runs.push((call, ft, t));
    }

    /// Run for-in body `ft` once with every key (the loop as it is).
    pub(crate) fn for_in_every_key(&mut self, call: CallId, ft: Tok) {
        if self.for_in_any.insert((call, ft)) {
            self.call_tok(call, ft, None);
        }
    }

    /// The keys a for-in run of body `f` over `t` visits, when they are
    /// exactly `t`'s: a literal not escaped and not HARD, with no key added
    /// after birth or stored under a computed key, no computed member, no
    /// `__proto__`, whose chain gets no enumerable key from the program
    /// (nothing writes `Object.prototype`, `object_proto_written`); a body
    /// defining a property under its key also needs each key to be one
    /// generation named (`for_in_define_keys`). `late`: the keys stored into
    /// each template (`settle_for_in`; the check before it only knows the
    /// cheap facts).
    fn for_in_keys(
        &self,
        f: FnIdx,
        t: Tok,
        late: Option<&FxHashMap<TmplId, Vec<Atom>>>,
    ) -> Option<Vec<Atom>> {
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return None;
        };
        let tpl = &self.tmpls[tm as usize];
        let literal = matches!(
            tpl.kind,
            TmplKind::Lit { .. }
                | TmplKind::Class {
                    literal_shape: true,
                    ..
                }
        );
        if !literal
            || tpl.has_computed_members
            || self.is_escaped(t)
            || self.hard.contains_key(&t)
            || self.adds_keys.get(&tm).is_some_and(|&n| n > 0)
            || self.any_writers.contains_key(&tm)
            || self.object_proto_written()
        {
            return None;
        }
        let mut keys: Vec<Atom> = Vec::new();
        let own = tpl.birth_keys.iter().chain(&tpl.accessor_keys);
        // The literal's methods, not its prototype's `constructor` (which
        // is not enumerable).
        let methods = tpl
            .proto_members
            .iter()
            .filter(|&&a| self.atoms[a as usize] != "constructor");
        for &a in own.chain(methods) {
            let name = self.atoms[a as usize].as_str();
            if name == "__proto__" {
                return None;
            }
            // A symbol is no for-in key.
            if !name.starts_with(crate::builtins::WK_PREFIX) && !keys.contains(&a) {
                keys.push(a);
            }
        }
        if late.is_some_and(|l| {
            l.get(&tm)
                .is_some_and(|ws| ws.iter().any(|a| !keys.contains(a)))
        }) {
            return None;
        }
        if self.for_in_defines.contains(&f)
            && keys.iter().any(|a| !self.for_in_define_keys.contains(a))
        {
            return None;
        }
        Some(keys)
    }

    /// May `Object.prototype`, the chain of every literal, hold a key the
    /// program wrote (`native_writes` on it: a store, an accessor, E5/E6/E7,
    /// a reach into unmodeled code, another unit's write)? A for-in over a
    /// literal visits such a key when it is enumerable. A write into ⊤ is
    /// no such write: ⊤ holds no builtin prototype (`escape`: a runtime
    /// object reaches unknown code as itself, and what may write it is
    /// noted on it), and a literal whose keys are exactly its own is not
    /// escaped, so not in ⊤ either. The write into ⊤ stays a fact about
    /// ⊤'s members, which lookups through ⊤ already read as anything.
    fn object_proto_written(&self) -> bool {
        self.native_writes
            .keys()
            .any(|&(w, _)| matches!(self.toks[w as usize], TokKind::ObjectProto))
    }

    /// Once the worklist is empty: a literal a for-in body ran per key over
    /// that gained a key after birth (or escaped, or turned HARD) since runs
    /// the body once more with every key. True when one did.
    pub(crate) fn settle_for_in(&mut self) -> bool {
        if self.for_in_runs.is_empty() {
            return false;
        }
        let tms: crate::pts::FxHashSet<TmplId> = self
            .for_in_runs
            .iter()
            .filter_map(|&(_, _, t)| self.tmpl_of(t))
            .collect();
        let mut late: FxHashMap<TmplId, Vec<Atom>> = FxHashMap::default();
        for &(tm, a) in self.writers.keys().chain(self.proto_writers.keys()) {
            if tms.contains(&tm) {
                late.entry(tm).or_default().push(a);
            }
        }
        let mut more = false;
        for (call, ft, t) in self.for_in_runs.clone() {
            if self.for_in_any.contains(&(call, ft)) {
                continue;
            }
            let f = self.tok_fn(ft);
            if self.for_in_keys(f, t, Some(&late)).is_none() {
                self.for_in_every_key(call, ft);
                more = true;
            }
        }
        more
    }
}
