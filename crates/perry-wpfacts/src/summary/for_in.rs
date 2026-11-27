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
    pub(crate) fn for_in_tok(&mut self, call: CallId, ft: Tok, t: Tok, list: KeyList) {
        if self.act.is_some() {
            let f = self.tok_fn(ft);
            self.activate(f, "for-in");
        }
        if !self.sum.ready {
            self.defer(Deferred::ForIn(call, ft, t, list));
            return;
        }
        let f = self.tok_fn(ft);
        let runs = self.summarized(f) && self.instantiable(ft);
        let Some(keys) = self.for_in_keys(f, t, list) else {
            if for_in_diag() {
                self.for_in_diag_why(f, ft, t);
            }
            let refl = list == KeyList::OwnNames || self.reflective_box(t);
            return self.for_in_every_key(call, ft, refl);
        };
        if !runs {
            self.for_in_named_run(call, ft, &keys);
            self.for_in_named.push((call, ft, t, list));
            return;
        }
        if self.for_in_key_runs(call, ft, &keys, list == KeyList::OwnNames).is_some() {
            self.for_in_runs.push((call, ft, t, list));
        }
    }

    /// The runs of body `ft` for `keys`, made on first use. `None`: the body
    /// does not take its key as a fresh slot, so it ran with every key
    /// instead; else whether a run was new.
    fn for_in_key_runs(
        &mut self,
        call: CallId,
        ft: Tok,
        keys: &[Atom],
        refl: bool,
    ) -> Option<bool> {
        let mut made = false;
        for &k in keys {
            made |= !self
                .sum
                .inst_key
                .contains_key(&(6, k, ft, crate::summary::NO_VAL));
            let inst = self.instance((6, k, ft));
            match self.sum.insts[inst as usize].params.first().copied() {
                // The run reads no key.
                Some(NO_NODE) => {}
                // A fresh slot: the run's key (its key variable's copy holds
                // the key's atom, `instance`). The run binds that one String
                // (14.7.5.10 %ForInIteratorPrototype%.next): a String
                // constant, so `'x ' + key` in the run is a constant too.
                Some(p) if self.nodes[p as usize].frame == inst + 1 => {
                    let set = self.str_set(&[k]);
                    let set = self.run_born(set);
                    self.add_s(p, &[], prim::STR, set);
                }
                _ => {
                    self.for_in_every_key(call, ft, refl);
                    return None;
                }
            }
        }
        Some(made)
    }

    /// FORIN1: a body with no per-key runs (not summarized: the activation
    /// pass, a refused summary) runs once, shared, with its key parameter
    /// holding the keys `for_in_keys` lists for each value it iterates
    /// (ECMA-262 14.7.5.9 EnumerateObjectProperties, 14.7.5.10
    /// %ForInIteratorPrototype%.next: a key is yielded only while it names
    /// an own enumerable String-keyed property of the object or of its
    /// chain; the built-in members of `%Object.prototype%` are not
    /// enumerable, 20.1.3). The union over the iterated values holds every
    /// key any iteration binds, so the run covers the loop; a key written
    /// later joins the union (`settle_for_in`); a value whose keys are not
    /// known runs the body with every key (`for_in_every_key`).
    fn for_in_named_run(&mut self, call: CallId, ft: Tok, keys: &[Atom]) -> bool {
        let k = match self.for_in_named_call.get(&(call, ft)) {
            Some(&(_, k)) => k,
            None => {
                let k = self.node_l("forin-names");
                let mut cs = self.calls[call as usize].clone();
                cs.args = vec![k];
                let nc = self.calls.len() as CallId;
                self.calls.push(cs);
                self.for_in_named_call.insert((call, ft), (nc, k));
                self.call_tok(nc, ft, None);
                k
            }
        };
        if keys.is_empty() {
            return false;
        }
        let mut v = keys.to_vec();
        v.sort_unstable();
        v.dedup();
        let set = self.str_set(&v);
        let set = self.run_born(set);
        let before = (self.nodes[k as usize].prim, self.nodes[k as usize].strs);
        self.add_s(k, &[], prim::STR, set);
        before != (self.nodes[k as usize].prim, self.nodes[k as usize].strs)
    }

    /// Run for-in body `ft` once with every key (the loop as it is).
    #[track_caller]
    /// `refl` (P-PROTOKEY): the loop walks a reflective own-key list
    /// (`Object.getOwnPropertyNames`, a descriptors object's keys), whose
    /// keys are reflective (`reflective_key_strs`), not unknown Strings.
    pub(crate) fn for_in_every_key(&mut self, call: CallId, ft: Tok, refl: bool) {
        if for_in_diag() {
            eprintln!(
                "perry: wpfacts forindiag every-key body={} at={}",
                self.funcs[self.tok_fn(ft) as usize].name,
                std::panic::Location::caller()
            );
        }
        // The loop as it is: its key is any String key of the value
        // (14.7.5.10: a String, never a Symbol); per subject, since one
        // loop may walk a reflective list and another value's keys.
        if let Some(&k) = self.calls[call as usize].args.first() {
            if refl {
                let set = self.reflective_key_strs();
                self.add_s(k, &[], prim::STR, set);
            } else {
                self.add_prim(k, prim::STR);
            }
        }
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
    ///
    /// A for-of over `Object.keys(o)` or `Object.getOwnPropertyNames(o)`
    /// (`KeyList`) visits the keys rule EK or PK lists for `t`, the same
    /// constants `Cons::OwnKeys` / `Cons::OwnNames` give its array.
    fn for_in_keys(&self, f: FnIdx, t: Tok, list: KeyList) -> Option<Vec<Atom>> {
        let keys = match list {
            KeyList::ForIn => self.enum_keys(t)?,
            KeyList::OwnKeys if t != TOP && self.own_props_modelled(t) => self.enum_keys(t)?,
            KeyList::OwnKeys => return None,
            KeyList::OwnNames if t != TOP => self.proto_names(t)?,
            KeyList::OwnNames => return None,
        };
        if self.for_in_defines.contains(&f)
            && keys.iter().any(|a| !self.for_in_define_keys.contains(a))
        {
            return None;
        }
        Some(keys)
    }

    /// The enumerable String keys of literal `t` and of its chain, as far
    /// as the solve knows them now (`for_in_keys` without its body's
    /// defines): `None` when they are not known.
    pub(crate) fn enum_keys(&self, t: Tok) -> Option<Vec<Atom>> {
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
        // The keys below are the object's own and its template's methods:
        // the chain beyond them is `%Object.prototype%`, whose members are
        // not enumerable. A chain that may hold another object (the literal
        // `{ __proto__: p }`, B.3.1; a relink) also visits that object's
        // enumerable keys (14.7.5.9 EnumerateObjectProperties, 14.7.5.10):
        // the general walk runs.
        let born_chain = self.proto_node.get(&t).is_none_or(|&pn| {
            let nd = &self.nodes[pn as usize];
            nd.prim == 0
                && nd
                    .pts
                    .to_sorted()
                    .iter()
                    .all(|&p| match self.toks[p as usize] {
                        TokKind::ObjectProto => true,
                        TokKind::Proto(pm) => pm == tm,
                        _ => false,
                    })
        });
        if !born_chain {
            return None;
        }
        let star = |a: Atom| self.atoms[a as usize] == crate::STAR_KEY;
        if self
            .accessor_keys
            .get(&t)
            .is_some_and(|ks| ks.iter().any(|&a| star(a)))
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
            // A key whose text a solver rule made (`spelled_atoms`): the
            // run of such a key may write a key made from it in turn
            // (`o[k + "x"] = v` in the body: "a", "ax", "axx", ...), so per-
            // key runs need not end. The keys a program can spell are
            // finitely many; past them the body runs once with every key.
            if self.spelled_atoms.is_some_and(|n| a as usize >= n) {
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
        Some(keys)
    }

    /// Rule EK (`Cons::OwnKeys`): dst ⊇ the own enumerable String keys of
    /// `t` (ECMA-262 7.3.23 EnumerableOwnProperties, 10.1.11.1
    /// OrdinaryOwnPropertyKeys). A literal whose keys are known
    /// (`enum_keys`: not escaped, not HARD, no computed member, never
    /// written under a key no name lists) gives those keys, a superset of
    /// its own ones; it is looked at again in `settle_for_in`. Any other
    /// value (⊤ among them) gives any String.
    pub(crate) fn own_keys_of(&mut self, t: Tok, dst: NodeId) -> bool {
        let keys = if t == TOP || !self.own_props_modelled(t) {
            None
        } else {
            self.enum_keys(t)
        };
        let before = (self.nodes[dst as usize].prim, self.nodes[dst as usize].strs);
        match keys {
            // P-PROTOKEY: the keys of a descriptors object are its source's
            // own keys, enumerable or not: a reflective key.
            None if self.reflective_box(t) => {
                let set = self.reflective_key_strs();
                self.add_s(dst, &[], prim::STR, set);
            }
            None => self.add_prim(dst, prim::STR),
            Some(mut v) => {
                if self.own_keys_seen.insert((t, dst)) {
                    self.own_keys_on.push((t, dst));
                }
                if !v.is_empty() {
                    v.sort_unstable();
                    v.dedup();
                    let set = self.str_set(&v);
                    let set = self.run_born(set);
                    self.add_s(dst, &[], prim::STR, set);
                }
            }
        }
        before != (self.nodes[dst as usize].prim, self.nodes[dst as usize].strs)
    }

    /// Rule PK: the own String keys of `t` when `t` is the prototype of a
    /// class of this unit whose keys are known: the keys its class
    /// definition creates on it (ECMA-262 15.7.14 ClassDefinitionEvaluation:
    /// `constructor`, each non-static method and accessor) plus the names
    /// the program wrote on it, a superset of its own ones. `None` (any
    /// String) when it escaped, is HARD, has a computed member, was written
    /// under a key no name lists, has an accessor of an unknown key, holds
    /// a key a solver rule made (`spelled_atoms`, as `enum_keys`) or
    /// `__proto__`, or is another unit's class (its prototype lists only
    /// its contract's methods).
    pub(crate) fn proto_names(&self, t: Tok) -> Option<Vec<Atom>> {
        let TokKind::Proto(tm) = self.toks[t as usize] else {
            return None;
        };
        let tpl = &self.tmpls[tm as usize];
        if !matches!(
            tpl.kind,
            TmplKind::Class {
                literal_shape: false,
                ..
            }
        ) || self.foreign.contains_key(&tm)
            || tpl.has_computed_members
            || self.is_escaped(t)
            || self.hard.contains_key(&t)
            || self.enum_any_written(t)
        {
            return None;
        }
        let star = |a: Atom| self.atoms[a as usize] == crate::STAR_KEY;
        let acc = self.accessor_keys.get(&t).into_iter().flatten();
        // A field node a lookup made (`o.k` read through the chain, a realm
        // read of `@@toStringTag`) names no property: [[Get]] creates none
        // (ECMA-262 10.1.8.1). A written one holds the stored value.
        let written = self.field_keys.get(&t).into_iter().flatten().filter(|&&a| {
            self.fields
                .get(&(t, a))
                .is_some_and(|&n| self.node_held(n) || self.nodes[n as usize].enum_key)
        });
        let named = self
            .proto_writers
            .keys()
            .filter(|&&(w, _)| w == tm)
            .map(|(_, a)| a);
        let mut keys: Vec<Atom> = Vec::new();
        for &a in tpl
            .proto_members
            .iter()
            .chain(acc)
            .chain(written)
            .chain(named)
        {
            let name = self.atoms[a as usize].as_str();
            if name == "__proto__" || star(a) || self.spelled_atoms.is_some_and(|n| a as usize >= n)
            {
                return None;
            }
            // A symbol is no String key.
            if !name.starts_with(crate::builtins::WK_PREFIX)
                && self.enum_string_key(a)
                && !keys.contains(&a)
            {
                keys.push(a);
            }
        }
        Some(keys)
    }

    /// Rule PK (`Cons::OwnNames`): dst ⊇ the own String keys of `t`
    /// (ECMA-262 20.1.2.10, 10.1.11.1). A class prototype whose names are
    /// known (`proto_names`) gives those names; it is looked at again in
    /// `settle_for_in`. Any other value (⊤ among them) gives any String.
    pub(crate) fn own_names_of(&mut self, t: Tok, dst: NodeId) -> bool {
        let keys = if t == TOP { None } else { self.proto_names(t) };
        let before = (self.nodes[dst as usize].prim, self.nodes[dst as usize].strs);
        match keys {
            // P-PROTOKEY: any String, a reflective key (`reflective_key_strs`).
            None => {
                let set = self.reflective_key_strs();
                self.add_s(dst, &[], prim::STR, set);
            }
            Some(mut v) => {
                if self.own_names_seen.insert((t, dst)) {
                    self.own_names_on.push((t, dst));
                }
                if !v.is_empty() {
                    v.sort_unstable();
                    v.dedup();
                    let set = self.str_set(&v);
                    let set = self.run_born(set);
                    self.add_s(dst, &[], prim::STR, set);
                }
            }
        }
        before != (self.nodes[dst as usize].prim, self.nodes[dst as usize].strs)
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
        for (call, ft, t, list) in self.for_in_runs.clone() {
            if self.for_in_any.contains(&(call, ft)) {
                continue;
            }
            let f = self.tok_fn(ft);
            match self.for_in_keys(f, t, list) {
                None => {
                    if for_in_diag() {
                        self.for_in_diag_why(f, ft, t);
                    }
                    let refl = list == KeyList::OwnNames || self.reflective_box(t);
                    self.for_in_every_key(call, ft, refl);
                    more = true;
                }
                Some(keys) => {
                    more |= self.for_in_key_runs(call, ft, &keys, list == KeyList::OwnNames)
                        != Some(false)
                }
            }
        }
        // FORIN1: the shared named runs, the same way.
        for (call, ft, t, list) in self.for_in_named.clone() {
            if self.for_in_any.contains(&(call, ft)) {
                continue;
            }
            let f = self.tok_fn(ft);
            match self.for_in_keys(f, t, list) {
                None => {
                    if for_in_diag() {
                        self.for_in_diag_why(f, ft, t);
                    }
                    let refl = list == KeyList::OwnNames || self.reflective_box(t);
                    self.for_in_every_key(call, ft, refl);
                    more = true;
                }
                Some(keys) => more |= self.for_in_named_run(call, ft, &keys),
            }
        }
        // Rule EK: the keys `Cons::OwnKeys` gave, the same way.
        for (t, dst) in self.own_keys_on.clone() {
            more |= self.own_keys_of(t, dst);
        }
        // Rule PK: the names `Cons::OwnNames` gave, the same way.
        for (t, dst) in self.own_names_on.clone() {
            more |= self.own_names_of(t, dst);
        }
        more
    }
}

/// `PERRY_WPF_FORINDIAG`: print why a for-in body runs with every key.
fn for_in_diag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_WPF_FORINDIAG").is_some())
}

impl Model {
    fn for_in_diag_why(&self, f: FnIdx, ft: Tok, t: Tok) {
        let lit = match self.toks[t as usize] {
            TokKind::Obj(tm, _) => {
                let tpl = &self.tmpls[tm as usize];
                format!(
                    "lit={} computed={}",
                    matches!(
                        tpl.kind,
                        TmplKind::Lit { .. }
                            | TmplKind::Class {
                                literal_shape: true,
                                ..
                            }
                    ),
                    tpl.has_computed_members
                )
            }
            _ => "lit=notobj".to_string(),
        };
        eprintln!(
            "perry: wpfacts forindiag body={} tok={} summarized={} inst={} {lit} subj={} escaped={} hard={} anywritten={} protowritten={}",
            self.funcs[f as usize].name,
            self.tok_name(t),
            self.summarized(f),
            self.instantiable(ft),
            self.enum_subject(t),
            self.is_escaped(t),
            self.hard.contains_key(&t),
            self.enum_any_written(t),
            self.object_proto_written()
        );
        let held = |n: NodeId| {
            let nd = &self.nodes[n as usize];
            format!("n{n}(pts={},prim={:#x})", nd.pts.len(), nd.prim)
        };
        let any = self.field_any.get(&t).map(|&n| held(n));
        let shaped: Vec<String> = self
            .shaped_of
            .get(&t)
            .into_iter()
            .flatten()
            .filter(|&&(_, _, store)| store)
            .map(|&(k, n, _)| format!("shape{k}:{}", held(n)))
            .collect();
        eprintln!("perry: wpfacts forindiag   any={any:?} shaped={shaped:?}");
        if let TokKind::Proto(tm) = self.toks[t as usize] {
            let names = |v: &mut dyn Iterator<Item = Atom>| -> Vec<String> {
                v.map(|a| {
                    let late = self.spelled_atoms.is_some_and(|n| a as usize >= n);
                    format!("{}{}", self.atoms[a as usize], if late { "(late)" } else { "" })
                })
                .collect()
            };
            eprintln!(
                "perry: wpfacts forindiag   proto names={:?} computed={} foreign={} acc={:?} written={:?} writers={:?}",
                self.proto_names(t).map(|v| v.len()),
                self.tmpls[tm as usize].has_computed_members,
                self.foreign.contains_key(&tm),
                names(&mut self.accessor_keys.get(&t).into_iter().flatten().copied()),
                names(&mut self.field_keys.get(&t).into_iter().flatten().copied()),
                names(&mut self.proto_writers.keys().filter(|&&(w, _)| w == tm).map(|&(_, a)| a)),
            );
        }
    }
}
