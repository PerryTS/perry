//! Spread sources, shadowed keys and chain-miss bookkeeping: which own
//! keys stop a lookup, and the walks re-run when a key is reshaped.

use crate::builtins;
use crate::model::*;

impl Model {
    /// `a` is a key every object of spread literal `tm` holds.
    pub(crate) fn spread_sure(&self, tm: TmplId, a: Atom) -> bool {
        self.lit_spread.get(&tm).is_some_and(|l| {
            l.named.contains(&a)
                || l.sites
                    .iter()
                    .any(|(_, keys)| keys.as_ref().is_none_or(|k| k.contains(&a)))
        })
    }

    /// The keys a spread copies surely from source `s`: the definite keys of
    /// a fact unit's object; none from anything else.
    pub(crate) fn spread_source_keys(&self, s: Tok) -> Vec<Atom> {
        match self.tmpl_of(s) {
            Some(tm)
                if self.tmpls[tm as usize].kind.is_fact_unit()
                    && !self.keys_unsure.contains(&tm) =>
            {
                let mut keys = self.tmpls[tm as usize].definite.clone();
                if let Some(l) = self.lit_spread.get(&tm) {
                    keys.extend(l.named.iter().copied());
                }
                keys
            }
            _ => Vec::new(),
        }
    }

    /// Spread site `site` of literal `tm` copies from a source holding
    /// surely `keys` (empty: a primitive or an opaque value).
    pub(crate) fn spread_source(&mut self, tm: TmplId, site: SiteId, keys: Vec<Atom>) {
        let Some(l) = self.lit_spread.get_mut(&tm) else {
            return;
        };
        let Some((_, common)) = l.sites.iter_mut().find(|(s, _)| *s == site) else {
            return;
        };
        let shrank = match common {
            None => {
                *common = Some(keys);
                true
            }
            Some(c) => {
                let before = c.len();
                c.retain(|k| keys.contains(k));
                c.len() != before
            }
        };
        if shrank {
            self.flush_present_reads(tm);
        }
    }

    /// DECISIONS 234: does own key `a` shadow the builtin prototypes on
    /// every object `t` denotes? It does when each of them holds `a` as an
    /// own data property from birth (a definite birth key of a fact unit,
    /// no key of which can disappear; for a receiver token or marker, of
    /// each class it stands for, `class_holds`), and nothing redefined `a`
    /// on a builtin prototype or through ⊤ (`reshaped_keys`), where a setter
    /// or a read-only value would intercept the store that makes the key.
    pub(crate) fn shadows(&self, t: Tok, a: Atom) -> bool {
        if t != TOP && self.regexp_last_index(t, a) {
            return true;
        }
        // EUC S: a static method the class object holds as an own data property
        // from class evaluation (CreateMethodProperty, so no accessor on a builtin
        // prototype intercepts it) stops the lookup at the class object while
        // nothing can have deleted or redefined it (no delete/defineProperty met
        // the class object: `hard`) and unknown code never held it (`escaped`).
        // Read by the load and the call lookup alike (both ask `shadows`).
        if let TokKind::ClassObj(tm) = self.toks[t as usize] {
            let c = &self.tmpls[tm as usize];
            if c.static_methods.contains(&a)
                && !c.per_evaluation
                && !self.hard.contains_key(&t)
                && !self.escaped.contains_key(&t)
            {
                return true;
            }
        }
        if t == TOP
            || self.reshaped_any
            || self.reshaped_keys.contains(&a)
            || self.objproto_accessor
        {
            return false;
        }
        let top = self.top_reshaped_any || self.top_reshaped_keys.contains(&a);
        if let Some(classes) = self.virt_protos.get(&t) {
            if self.virt_tok_open.contains(&t) {
                return false;
            }
            // Instances of these classes made in another unit may have
            // escaped there, unseen here: a delete or defineProperty
            // through ⊤ revokes the key on them.
            return !top && !classes.is_empty() && classes.iter().all(|&p| self.class_holds(p, a));
        }
        match self.tmpl_of(t) {
            Some(tm) => {
                self.tmpls[tm as usize].kind.is_fact_unit()
                    && !self.keys_unsure.contains(&tm)
                    && !self.inst_lose.contains(&tm)
                    && !(top && self.inst_escaped.contains(&tm))
                    && (self.tmpls[tm as usize].definite.contains(&a) || self.spread_sure(tm, a))
            }
            None => false,
        }
    }

    /// Every instance of class `p` and of each subclass holds `a` as an own
    /// data property: a definite birth key of `p` (its constructor runs in
    /// every subclass's `super()`), no instance or receiver of `p` met a
    /// delete, defineProperty or `with` (`inst_lose`; the keys an instance
    /// gains, `virt_open`, take none away), no accessor on the chain
    /// intercepts the store,
    /// and no subclass of another unit defines `a` or deletes or redefines
    /// a key (census `override a of p`, `override * of p`).
    fn class_holds(&self, p: TmplId, a: Atom) -> bool {
        let tp = &self.tmpls[p as usize];
        let star = self.atom_index.get(crate::STAR_KEY).copied();
        tp.definite.contains(&a)
            && !self.keys_unsure.contains(&p)
            && !self.inst_lose.contains(&p)
            && !self.chain_accessor_key(p, a)
            && !self
                .override_keys
                .get(&p)
                .is_some_and(|v| v.contains(&a) || star.is_some_and(|s| v.contains(&s)))
    }

    /// The key a lookup of `key` on `t` finds as an own data property of
    /// every object `t` denotes (`shadows`), where it stops: a read and a
    /// method call's callee lookup alike (both are a [[Get]]).
    /// OPTIMISTIC rule (owner 2026-11-12, measurement only; off unless
    /// `PERRY_FACTS_OPTIMISTIC_JSON_KEY=1`), form B: a computed key (not a
    /// compile-time constant) on a receiver that is a `JSON.parse` object
    /// or a program object literal or Array names no member of a built-in
    /// prototype and none of `constructor`, `__proto__`, `prototype`: the
    /// read finds the receiver's own properties and what the program wrote
    /// on its chain; the store writes an own property. Node finds the
    /// built-in member for such a key (`{}["toString"]`); the rule assumes
    /// the program's data never spells one. The one predicate of the rule.
    pub(crate) fn json_key_optimistic(&self, t: Tok, key: Key) -> bool {
        self.optimistic_json_key
            && matches!(key, Key::Any(_))
            && t != TOP
            && !self.is_marker(t)
            && match &self.toks[t as usize] {
                // A template's prototype stands for the rest of the chain of
                // its instances (the transitive link `cv_of(q, ..)` of
                // `Cons::ChainUnion`): the same receiver, one link up.
                TokKind::Obj(tm, _) | TokKind::Proto(tm) => matches!(
                    self.tmpls[*tm as usize].kind,
                    TmplKind::Lit { .. }
                        | TmplKind::Class {
                            literal_shape: true,
                            ..
                        }
                        | TmplKind::Arr { .. }
                        | TmplKind::BObj { what: "json", .. }
                ),
                _ => false,
            }
    }

    pub(crate) fn shadow_key(&self, t: Tok, key: Key) -> Option<Atom> {
        match key {
            Key::Named(a) if !self.is_proto_key(key) && self.shadows(t, a) => Some(a),
            _ => None,
        }
    }

    /// ECMA-262 10.1.8.1 OrdinaryGet: a [[Get]] of `a` on any object `t`
    /// denotes finds an own data property, whose value is the result: no
    /// prototype's value or accessor under `a` is consulted. It holds when
    /// no object `t` denotes ever
    /// reached unknown code (`inst_escaped`: there it may be deleted or
    /// redefined unseen) and no accessor on its own class chain stands
    /// under `a` (`chain_accessor_key`). Then, for an object literal (a
    /// literal class or literal site), `a` is a definite birth key (or one
    /// every spread surely copies) of a template no delete, defineProperty
    /// or `with` met (`keys_unsure`, `inst_lose`): its birth defines the
    /// key (CreateDataProperty, 13.2.5.5), which no prototype intercepts.
    /// A class instance's key is made by [[Set]] (`this.k = v`), which an
    /// accessor or read-only value on a builtin prototype may intercept:
    /// it needs `shadows` (DECISIONS 234), which also rules that out. Every condition only turns false during a solve;
    /// a lookup answered by it is filed (`own_stopped`) and runs again in
    /// full when it does (`flush_own_stopped`).
    pub(crate) fn surely_own_data(&self, t: Tok, a: Atom) -> bool {
        if t == TOP || self.is_proto_key(Key::Named(a)) || self.is_private_atom(a) {
            return false;
        }
        // R9BIRTH4: a class constructor owns `prototype` from its creation,
        // non-writable and non-configurable (ECMA-262 15.7.14
        // ClassDefinitionEvaluation: MakeConstructor(F, false, proto)), so no
        // store, delete or definition removes or replaces it: `C.prototype`
        // reads Cs own prototype object, never a parent classs.
        if let TokKind::ClassObj(tm) = self.toks[t as usize] {
            return self.atoms[a as usize] == "prototype"
                && matches!(
                    self.tmpls[tm as usize].kind,
                    crate::model::TmplKind::Class {
                        literal_shape: false,
                        ..
                    }
                )
                // The class's own field holds that object (`Gen` class
                // values); a layout without one keeps the chain walk.
                && self.tok_index.get(&TokKind::Proto(tm)).is_some_and(|&p| {
                    self.fields
                        .get(&(t, a))
                        .is_some_and(|&f| self.nodes[f as usize].pts.contains_tok(p))
                });
        }
        let mut cs: Vec<TmplId> = self.tmpl_of(t).into_iter().collect();
        if let Some(ps) = self.virt_protos.get(&t) {
            cs.extend(ps.iter().copied());
        }
        if cs.is_empty()
            || !cs
                .iter()
                .all(|&c| !self.inst_escaped.contains(&c) && !self.chain_accessor_key(c, a))
        {
            return false;
        }
        if !self.virt_protos.contains_key(&t) {
            if let Some(tm) = self.tmpl_of(t) {
                let tp = &self.tmpls[tm as usize];
                // A realm birth's declared own field (`realm_born`) is made
                // as a literal's is.
                if matches!(
                    tp.kind,
                    crate::model::TmplKind::Lit { .. }
                        | crate::model::TmplKind::Class {
                            literal_shape: true,
                            ..
                        }
                ) || self.realm_born.contains(&tm)
                {
                    return !self.keys_unsure.contains(&tm)
                        && !self.inst_lose.contains(&tm)
                        && (tp.definite.contains(&a) || self.spread_sure(tm, a));
                }
            }
        }
        self.shadows(t, a)
    }

    /// A lookup `act` of own key `a` on `t` answered by the own field alone
    /// (`surely_own_data`): filed for `flush_own_stopped`.
    pub(super) fn note_own_stopped(&mut self, origin: Tok, t: Tok, a: Atom, act: ChainAct) {
        self.file_shadow_index(t, a);
        let v = self.own_stopped.entry(t).or_default();
        if !v.seen.contains(&(origin, a, act)) {
            v.push((origin, a, act));
        }
    }

    /// The lookups on `t` answered by an own key that is no longer surely an
    /// own data property run again, in full (chain and accessors).
    fn flush_own_stopped(&mut self, t: Tok) {
        let Some(reads) = self.own_stopped.remove(&t) else {
            return;
        };
        let mut kept = Vec::new();
        for (origin, a, act) in reads.list {
            if self.surely_own_data(t, a) {
                kept.push((origin, a, act));
                continue;
            }
            match act {
                ChainAct::Load(dst, _) => self.load_from(origin, t, Key::Named(a), dst),
                ChainAct::Call(call) => self.method_on(t, Key::Named(a), call),
                _ => {}
            }
        }
        if !kept.is_empty() {
            let v = self.own_stopped.entry(t).or_default();
            for e in kept {
                v.push(e);
            }
        }
    }

    /// A lookup (a read or a method call) that skipped the builtin members
    /// (`shadows`): resumed by `flush_shadowed` once the key no longer
    /// shadows them.
    pub(crate) fn note_shadowed(&mut self, t: Tok, a: Atom, act: ChainAct, own_members: bool) {
        let v = self.shadowed.entry(t).or_default();
        if !v.contains(&(a, act, own_members)) {
            v.push((a, act, own_members));
        }
        self.file_shadow_index(t, a);
    }

    /// Files receiver `t` by key `a` and by the classes `shadows` and
    /// `surely_own_data` consult for it: the events that revoke either
    /// re-check its lookups (`flush_shadowed_toks`).
    fn file_shadow_index(&mut self, t: Tok, a: Atom) {
        if self.shadowed_keyed.insert((t, a)) {
            self.shadowed_by_key.entry(a).or_default().push(t);
        }
        if self.shadowed_classed.insert(t) {
            let mut cs: Vec<TmplId> = self.tmpl_of(t).into_iter().collect();
            if let Some(ps) = self.virt_protos.get(&t) {
                cs.extend(ps.iter().copied());
            }
            for c in cs {
                self.shadowed_by_class.entry(c).or_default().push(t);
            }
        }
    }

    /// The shadowed lookups of a key no longer shadowing once `a` may have
    /// been redefined (`shadowed_by_key`).
    fn flush_shadowed_key(&mut self, a: Atom) {
        let toks = self.shadowed_by_key.get(&a).cloned().unwrap_or_default();
        self.flush_shadowed_toks(toks);
    }

    /// The shadowed lookups whose `shadows` consults one of classes `cs`
    /// (`shadowed_by_class`), after something changed on them.
    pub(crate) fn flush_shadowed_classes(&mut self, cs: &[TmplId]) {
        let mut toks = Vec::new();
        for c in cs {
            if let Some(v) = self.shadowed_by_class.get(c) {
                toks.extend(v.iter().copied());
            }
        }
        self.flush_shadowed_toks(toks);
    }

    /// An accessor on `Object.prototype` (E23): no own key shadows it, on
    /// any receiver.
    pub(crate) fn note_objproto_accessor(&mut self) {
        if !std::mem::replace(&mut self.objproto_accessor, true) {
            self.flush_shadowed(None);
        }
    }

    /// Debug builds, at the end of a solve: every lookup still skipped
    /// still shadows; an event that revoked one without re-checking it
    /// would leave it here.
    #[cfg(debug_assertions)]
    pub(crate) fn assert_shadowed_complete(&self) {
        for (&t, reads) in &self.shadowed {
            for &(a, ..) in reads {
                assert!(
                    self.shadows(t, a),
                    "wpfacts: lookup of {} on {} skipped the builtin members, \
                     but the key no longer shadows them",
                    self.atoms[a as usize],
                    self.tok_name(t)
                );
            }
        }
    }

    /// The builtin members a lookup of `key` on `t` finds unless an own key
    /// shadows them (`shadow_key`): those of `t`'s own kind (`own_members`)
    /// and the chain's builtin prototypes; for a method call, each of them
    /// is a callee.
    pub(crate) fn builtin_members(&mut self, t: Tok, key: Key, act: ChainAct, own_members: bool) {
        match act {
            ChainAct::Lookup { .. } => self.chain_step(0, t, t, key, act),
            ChainAct::Load(dst, _) => {
                let cv = self.cv(t, key);
                self.edge(cv, dst);
                if own_members {
                    self.with_b(|m| builtins::tok_load(m, t, key, dst));
                }
            }
            ChainAct::Call(call) => {
                if own_members {
                    self.with_native(call, |m, call| builtins::own_method(m, t, key, call));
                }
                // Methods on the chain (shared) and builtin prototype
                // methods.
                let cv = self.cv(t, key);
                self.bind_via(cv, call, t);
                let cp = self.cp(t);
                self.cons(
                    cp,
                    Cons::ProtoMethods {
                        origin: t,
                        key,
                        call,
                    },
                );
            }
            ChainAct::Store(_, _) => {}
        }
    }

    /// The shadowed reads (of `t`; `None`: of every receiver) whose key no
    /// longer shadows the builtin prototypes take their members now.
    pub(crate) fn flush_shadowed(&mut self, of: Option<Tok>) {
        let toks: Vec<Tok> = match of {
            Some(t) => vec![t],
            None => self.shadowed.keys().copied().collect(),
        };
        self.flush_shadowed_toks(toks);
    }

    /// The shadowed lookups of receivers `toks`, in receiver order.
    fn flush_shadowed_toks(&mut self, mut toks: Vec<Tok>) {
        toks.sort_unstable();
        toks.dedup();
        for t in toks {
            self.flush_own_stopped(t);
            let Some(reads) = self.shadowed.remove(&t) else {
                continue;
            };
            let mut kept = Vec::new();
            for (a, act, own_members) in reads {
                if self.shadows(t, a) {
                    kept.push((a, act, own_members));
                    continue;
                }
                self.builtin_members(t, Key::Named(a), act, own_members);
            }
            if !kept.is_empty() {
                self.shadowed.insert(t, kept);
            }
        }
    }

    /// DECISIONS 234: key `key` (`None`: any key) of a builtin prototype may
    /// have been deleted or redefined (through ⊤, or on the prototype).
    pub(crate) fn reshape(&mut self, key: Option<Atom>) {
        let star = self.atom_index.get(crate::STAR_KEY).copied();
        match key {
            Some(a) if Some(a) != star => {
                if self.reshaped_keys.insert(a) {
                    if self.atoms[a as usize] == "length" {
                        self.ta_len_open_all();
                    }
                    self.flush_shadowed_key(a);
                }
            }
            _ => {
                if !std::mem::replace(&mut self.reshaped_any, true) {
                    self.ta_len_open_all();
                    self.flush_shadowed(None);
                }
            }
        }
    }

    /// DECISIONS 234: key `key` (`None`: any key) of an object ⊤ denotes,
    /// an escaped one, may have been deleted or redefined.
    pub(crate) fn top_reshape(&mut self, key: Option<Atom>) {
        let star = self.atom_index.get(crate::STAR_KEY).copied();
        match key {
            Some(a) if Some(a) != star => {
                if !self.top_reshaped_any && self.top_reshaped_keys.insert(a) {
                    self.flush_shadowed_key(a);
                }
            }
            _ => {
                if !std::mem::replace(&mut self.top_reshaped_any, true) {
                    self.flush_shadowed(None);
                }
            }
        }
    }

    /// The template of object `t` and its classes up the chain (for a
    /// receiver token or marker: the classes it stands for).
    fn inst_classes(&self, t: Tok) -> Vec<TmplId> {
        if let Some(ps) = self.virt_protos.get(&t) {
            return ps.clone();
        }
        let Some(tm) = self.tmpl_of(t) else {
            return Vec::new();
        };
        let mut seen: Vec<TmplId> = Vec::new();
        let mut work = vec![tm];
        while let Some(c) = work.pop() {
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let tp = &self.tmpls[c as usize];
            work.extend(tp.parent);
            work.extend(tp.parent_links.iter().copied());
        }
        seen
    }

    /// DECISIONS 234: `t` escaped (`lose` false) or met a delete,
    /// defineProperty or `with` (`lose`): an own key of it may be gone.
    pub(crate) fn note_inst(&mut self, t: Tok, lose: bool) {
        let mut fresh: Vec<TmplId> = Vec::new();
        for c in self.inst_classes(t) {
            let f = if lose {
                self.inst_lose.insert(c)
            } else {
                self.inst_escaped.insert(c)
            };
            if f {
                fresh.push(c);
            }
        }
        self.json_reviver_flush(&fresh);
        // An escape revokes `surely_own_data` on the objects of these
        // classes; `shadows` only once a key may be redefined through ⊤.
        if !fresh.is_empty() {
            self.flush_shadowed_classes(&fresh);
        }
    }

    /// DECISIONS 242(2): a [[Get]], call or [[Set]] of `key` on `t` that may
    /// pass `t`'s own keys onto its prototype chain marks `t`'s template
    /// (`Template::chain_miss`). A definite own data key (DECISIONS 234's
    /// `definite`, or a key every spread source surely holds) stops it at
    /// the object; a key a class prototype of its chain defines stops it
    /// before `Object.prototype`.
    pub(crate) fn note_chain_miss(&mut self, t: Tok, key: Key) {
        // On a marker the miss site is another unit's: the contract names
        // it (`reach_keys`) and the caller runs it on its argument
        // (`Cons::Miss`), where the argument may not own the key
        // (DECISIONS 242, contract half).
        if self.is_marker(t) {
            if let Key::Named(a) = key {
                if self.is_private_atom(a) {
                    return;
                }
            }
            let ks = self.marker_reads.entry(t).or_default();
            if !ks.contains(&key) {
                ks.push(key);
            }
            return;
        }
        let Some(tm) = self.tmpl_of(t) else {
            return;
        };
        let tpl = &self.tmpls[tm as usize];
        if tpl.chain_miss_open || !tpl.kind.is_fact_unit() {
            return;
        }
        if let Key::Named(a) = key {
            if self.is_private_atom(a) {
                return;
            }
            if !self.is_proto_key(key) && (tpl.definite.contains(&a) || self.spread_sure(tm, a)) {
                return;
            }
            let mut p = Some(tm);
            while let Some(c) = p {
                if self.tmpls[c as usize].proto_members.contains(&a) {
                    return;
                }
                p = self.tmpls[c as usize].parent;
            }
        }
        let open = match key {
            Key::Named(a) => !crate::builtins::object_final_read(&self.atoms[a as usize]),
            Key::Any(_) => true,
        };
        let tpl = &mut self.tmpls[tm as usize];
        tpl.chain_miss = true;
        tpl.chain_miss_open |= open;
    }

    /// DECISIONS 242 (contract half): a defineProperty on marker `t`
    /// (`None`: under a computed key), published per parameter.
    pub(crate) fn note_marker_define(&mut self, t: Tok, key: Option<Atom>, data: bool) {
        let ds = self.marker_defines.entry(t).or_default();
        if !ds.contains(&(key, data)) {
            ds.push((key, data));
        }
    }
}
