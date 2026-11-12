//! For-in bodies of summarized functions run over the keys of the iterated value.

use super::*;

impl Model {
    /// Run for-in body `ft` (through template call `call`) over the iterated
    /// value `t` (FVA 4.3b): once per key of a literal whose enumerable keys
    /// are known (`for_in_keys`), each run's key parameter holding its one
    /// key, else once with every key. Each iteration binds one key and
    /// `t[k]` in it reads that key only (ECMA-262 14.7.5.6
    /// ForIn/OfBodyEvaluation, 14.7.5.9 EnumerateObjectProperties), so the
    /// per-key runs are the loop's whole effect on `t`.
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
            true => self.for_in_keys(f, t),
            false => None,
        };
        let Some(keys) = keys else {
            return self.for_in_every_key(call, ft);
        };
        if self.for_in_key_runs(call, ft, &keys).is_some() {
            self.for_in_runs.push((call, ft, t));
        }
    }

    /// The runs of body `ft` for `keys`, made on first use. `None`: the body
    /// does not take its key as a fresh slot, so it ran with every key
    /// instead; else whether a run was new.
    fn for_in_key_runs(&mut self, call: CallId, ft: Tok, keys: &[Atom]) -> Option<bool> {
        let mut made = false;
        for &k in keys {
            made |= !self.sum.inst_key.contains_key(&(6, k, ft, crate::summary::NO_VAL));
            let inst = self.instance((6, k, ft));
            match self.sum.insts[inst as usize].params.first().copied() {
                // The run reads no key.
                Some(NO_NODE) => {}
                // A fresh slot: the run's key (its key variable's copy holds
                // the key's atom, `instance`).
                Some(p) if self.nodes[p as usize].frame == inst + 1 => self.add_prim(p, prim::STR),
                _ => {
                    self.for_in_every_key(call, ft);
                    return None;
                }
            }
        }
        Some(made)
    }

    /// Run for-in body `ft` once with every key (the loop as it is).
    pub(crate) fn for_in_every_key(&mut self, call: CallId, ft: Tok) {
        if self.for_in_any.insert((call, ft)) {
            self.call_tok(call, ft, None);
        }
    }

    /// The keys a for-in run of body `f` over `t` visits, as far as the
    /// solve knows them now: `t` is a subject of ENUM3 (`enum_subject`:
    /// its enumerable keys are the names the program wrote on it), a
    /// literal (its chain is `Object.prototype`, or its class's prototype
    /// whose program-written keys are listed, `proto_writers`) not escaped
    /// and not HARD, never written under a key no name lists
    /// (`enum_any_written`), with no computed member and no `__proto__`,
    /// whose chain gets no enumerable key from the program (nothing writes
    /// `Object.prototype`, `object_proto_written`); a body defining a
    /// property under its key also needs each key to be one generation
    /// named (`for_in_define_keys`). The keys are its birth keys, its
    /// methods and every enumerable field the program wrote
    /// (`Node::enum_key`); keys written later get their run once the
    /// worklist is empty (`settle_for_in`).
    fn for_in_keys(&self, f: FnIdx, t: Tok) -> Option<Vec<Atom>> {
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
            || !self.enum_subject(t)
            || self.is_escaped(t)
            || self.hard.contains_key(&t)
            || self.enum_any_written(t)
            || self.object_proto_written()
        {
            return None;
        }
        let star = |a: Atom| self.atoms[a as usize] == crate::STAR_KEY;
        if self.accessor_keys.get(&t).is_some_and(|ks| ks.iter().any(|&a| star(a))) {
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
        // The enumerable fields the program wrote (ENUM3).
        let written = self.field_keys.get(&t).into_iter().flatten().filter(|&&a| {
            self.fields
                .get(&(t, a))
                .is_some_and(|&n| self.nodes[n as usize].enum_key)
        });
        for &a in own.chain(methods).chain(written) {
            let name = self.atoms[a as usize].as_str();
            if name == "__proto__" || star(a) {
                return None;
            }
            // A symbol is no for-in key.
            if !name.starts_with(crate::builtins::WK_PREFIX)
                && self.enum_string_key(a)
                && !keys.contains(&a)
            {
                keys.push(a);
            }
        }
        if self
            .proto_writers
            .keys()
            .any(|&(w, a)| w == tm && !keys.contains(&a))
        {
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

    /// Once the worklist is empty: each literal a for-in body ran per key
    /// over is looked at again. A key written on it since gets its run; one
    /// that escaped, turned HARD or was written under a key no name lists
    /// since runs the body once more with every key. True when either
    /// happened.
    pub(crate) fn settle_for_in(&mut self) -> bool {
        let mut more = false;
        for (call, ft, t) in self.for_in_runs.clone() {
            if self.for_in_any.contains(&(call, ft)) {
                continue;
            }
            let f = self.tok_fn(ft);
            match self.for_in_keys(f, t) {
                None => {
                    self.for_in_every_key(call, ft);
                    more = true;
                }
                Some(keys) => more |= self.for_in_key_runs(call, ft, &keys) != Some(false),
            }
        }
        more
    }
}
