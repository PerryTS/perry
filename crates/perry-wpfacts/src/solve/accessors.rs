//! Accessor hits and groups: the getters and setters a walk finds, how
//! they bind, and the walks deferred until an accessor key is installed.

use crate::builtins;
use crate::model::*;

impl Model {
    fn numeric_accessor_key(&self, a: Atom) -> bool {
        self.atoms[a as usize] == crate::STAR_KEY || self.atoms[a as usize].parse::<f64>().is_ok()
    }

    /// A key becomes an accessor key: generation names an accessor under it,
    /// or the solve installs one. The first time, the reads and writes that
    /// arrived before it start the walks they deferred, whichever side named
    /// the key first: in the activation pass generation and the solve
    /// interleave, so a read of an inherited literal getter (`Object.create`,
    /// `setPrototypeOf`) can be solved before generation walks the literal.
    pub(crate) fn add_accessor_atom(&mut self, a: Atom) {
        if !self.accessor_atoms.insert(a) {
            return;
        }
        // Rule I: a class whose construction read `a` (or any key: STAR)
        // through the chain no longer knows its inert keys present.
        let star = self.atoms[a as usize] == crate::STAR_KEY;
        let hit: Vec<TmplId> = (0..self.tmpls.len() as TmplId)
            .filter(|&tm| {
                let r = &self.tmpls[tm as usize].inert_reads;
                !r.is_empty() && (star || r.contains(&a))
            })
            .collect();
        for &tm in &hit {
            self.flush_present_reads(tm);
        }
        if !hit.is_empty() {
            let births = std::mem::take(&mut self.inert_births);
            for (tm, b, v) in births {
                if self.inert_sure(tm, b) {
                    self.inert_births.push((tm, b, v));
                } else {
                    self.add_prim(v, prim::UNDEF);
                }
            }
        }
        self.numeric_index_walks(a);
        self.deferred_walks(a);
    }

    /// Numeric reads registered before the first numeric accessor start
    /// their deferred walks: installing a descriptor does not activate its
    /// body on their behalf.
    fn numeric_index_walks(&mut self, a: Atom) {
        if self.numeric_accessor || !self.numeric_accessor_key(a) {
            return;
        }
        self.numeric_accessor = true;
        let num = self.atom(NUM_KEY);
        let mut pending: Vec<_> = self
            .visits
            .iter()
            .flat_map(|(&t, visits)| {
                visits
                    .iter()
                    .copied()
                    .filter_map(move |(origin, key, act)| {
                        (key == Key::Named(num)).then_some((origin, t, act))
                    })
            })
            .collect();
        pending.sort_unstable_by_key(|&(origin, t, act)| {
            let (kind, n) = match act {
                ChainAct::Load(n, _) => (0, n),
                ChainAct::Call(n) => (1, n),
                ChainAct::Store(n, _) => (2, n),
                ChainAct::Lookup { dst, setter } => (3 + u8::from(setter), dst),
            };
            (origin, t, kind, n)
        });
        for (origin, t, act) in pending {
            self.accessor_walk(origin, t, Key::any(), act);
        }
    }

    /// Accessors named `key` (any key: `Key::Any`) declared on `holder`:
    /// invocation actions run them with `origin` as `this` through the holder's
    /// group; a descriptor query returns their function tokens directly.
    pub fn accessor_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        if let ChainAct::Lookup { dst, setter } = act {
            self.accessor_functions(holder, key, dst, setter);
            return;
        }
        // A getter the realm declares on a realm prototype of the chain
        // (`builtins::realm_proto_getter`) runs with the receiver as `this`.
        if let Key::Named(a) = key {
            if let Some(e) = builtins::realm_proto_getter(self, holder, a) {
                match act {
                    ChainAct::Load(dst, _) => {
                        self.with_b(|m| builtins::realm_getter_load(m, e, origin, a, dst))
                    }
                    ChainAct::Call(call) => {
                        let v = self.node_l("realm-getter-callee");
                        self.with_b(|m| builtins::realm_getter_load(m, e, origin, a, v));
                        self.bind_via(v, call, origin);
                    }
                    ChainAct::Store(..) | ChainAct::Lookup { .. } => {}
                }
            }
            if let ChainAct::Store(src, region) = act {
                self.realm_setter_hit(origin, holder, a, src, region);
            }
        }
        if let (Key::Any(_), ChainAct::Store(src, region)) = (key, act) {
            for a in self.with_b(|m| builtins::realm_proto_setter_keys(m, holder)) {
                if self.key_may_name(key, a) {
                    self.realm_setter_hit(origin, holder, a, src, region);
                }
            }
        }
        // A key whose shape splits the holder (`shape_splits`) runs only the
        // accessors named by a name it admits, and the computed-name ones.
        if self.shape_splits(holder, key).is_some() {
            let star = self.atom(crate::STAR_KEY);
            let names = self.accessor_keys.get(&holder).cloned().unwrap_or_default();
            for a in names {
                if (a == star || self.key_may_name(key, a))
                    && self.accessors.contains_key(&(holder, a))
                {
                    let k = Key::Named(a);
                    self.acc_group(holder, k);
                    self.group_hit(origin, holder, k, act);
                }
            }
            return;
        }
        // Accessor groups are keyed by the computed key itself.
        let key = key.unshaped();
        if let Key::Named(a) = key {
            let star = self.atom(crate::STAR_KEY);
            if a != star && self.accessors.contains_key(&(holder, star)) {
                let wildcard = Key::Named(star);
                self.acc_group(holder, wildcard);
                self.group_hit(origin, holder, wildcard, act);
            }
        }
        let has = match key {
            Key::Named(a) => self.accessors.contains_key(&(holder, a)),
            Key::Any(_) => self
                .accessor_keys
                .get(&holder)
                .is_some_and(|ks| !ks.is_empty()),
        };
        if !has {
            return;
        }
        self.acc_group(holder, key);
        self.group_hit(origin, holder, key, act);
    }

    /// A store from `origin` under `a` finds a realm accessor on the chain
    /// of realm prototype `holder`: its declared setter runs with `origin`
    /// as `this` (ECMA-262 10.1.9.2 step 7). A getter-only accessor runs
    /// nothing and keeps nothing (`RealmSetter::GetterOnly`). A setter the
    /// realm declares no effects for is an explicit stop: the seed names the
    /// accessor, the stored value escapes and what the setter does to the
    /// receiver is unknown.
    fn realm_setter_hit(&mut self, origin: Tok, holder: Tok, a: Atom, src: NodeId, region: NodeId) {
        match builtins::realm_proto_setter(self, holder, a) {
            None | Some(builtins::RealmSetter::GetterOnly) => {}
            Some(builtins::RealmSetter::Declared(e)) => {
                self.with_b(|m| builtins::realm_setter_store(m, e, origin, a, src))
            }
            Some(builtins::RealmSetter::Undeclared(what)) => {
                let reason = self.reason(&format!(
                    "stored through realm accessor {what} whose setter the realm does not declare"
                ));
                self.cons(src, Cons::TopSink { reason });
                let v = self.node_l("realm-setter-undeclared");
                self.add_top(v, |_| {
                    format!("store through realm accessor {what}: setter effects undeclared")
                });
                let n = self.node_l("realm-setter-this");
                self.add_tok(n, origin);
                let site = self.anon_post_site("undeclared realm setter");
                self.cons(
                    n,
                    Cons::Store {
                        key: Key::any(),
                        src: v,
                        site,
                        region,
                    },
                );
            }
        }
    }

    /// One lookup from `origin` reaches group (holder, key).
    pub(super) fn group_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        let g = &self.acc_groups[&(holder, key)];
        let (gt, ar, st, sp, rg, rs, live) = (g.gt, g.ar, g.st, g.sp, g.rg, g.rs, g.has_setter);
        // ACCREGION: an accessor runs inside the [[Get]]/[[Set]] of the
        // access (ECMA-262 10.1.8.1 step 7, 10.1.9.2 step 7): what it
        // throws completes that access, in the access's region.
        let (raise, region) = match act {
            ChainAct::Load(_, r) => (rg, r),
            ChainAct::Store(_, r) => (rs, r),
            ChainAct::Call(call) => (rg, self.calls[call as usize].region),
            ChainAct::Lookup { .. } => unreachable!("descriptor queries never invoke a group"),
        };
        self.edge(raise, region);
        match act {
            ChainAct::Lookup { .. } => unreachable!("descriptor queries never invoke a group"),
            ChainAct::Load(dst, _) => {
                self.add_tok(gt, origin);
                self.edge(ar, dst);
            }
            ChainAct::Call(call) => {
                // `o.k()` where k is a getter: the getter runs, then its
                // result is called with `o` as `this`.
                self.add_tok(gt, origin);
                self.bind_via(ar, call, origin);
            }
            ChainAct::Store(src, _) => {
                self.add_tok(st, origin);
                if live {
                    self.store_live(src);
                    self.edge(src, sp);
                } else {
                    self.acc_groups
                        .get_mut(&(holder, key))
                        .unwrap()
                        .pending
                        .push(src);
                }
            }
        }
    }

    /// The group of (holder, key), created with every accessor declared
    /// there so far (an Any group: linked to every named group of the
    /// holder).
    pub(super) fn acc_group(&mut self, holder: Tok, key: Key) {
        if self.acc_groups.contains_key(&(holder, key)) {
            return;
        }
        let gt = self.node_l("acc-this");
        let ar = self.node_l("acc-ret");
        let st = self.node_l("acc-setthis");
        let sp = self.node_l("acc-setarg");
        let rg = self.node_l("acc-get-raise");
        let rs = self.node_l("acc-set-raise");
        self.acc_groups.insert(
            (holder, key),
            AccGroup {
                gt,
                ar,
                st,
                sp,
                rg,
                rs,
                has_setter: false,
                pending: Vec::new(),
            },
        );
        match key {
            Key::Named(a) => {
                let (ng, ns) = self
                    .accessors
                    .get(&(holder, a))
                    .map_or((0, 0), |(g, s)| (g.len(), s.len()));
                for i in 0..ng {
                    let f = self.accessors[&(holder, a)].0[i];
                    self.group_connect(holder, key, f, false);
                }
                for i in 0..ns {
                    let f = self.accessors[&(holder, a)].1[i];
                    self.group_connect(holder, key, f, true);
                }
                if self.acc_groups.contains_key(&(holder, Key::any())) {
                    self.group_link(holder, a);
                }
            }
            Key::Any(_) => {
                let n = self.accessor_keys.get(&holder).map_or(0, |v| v.len());
                for i in 0..n {
                    let a = self.accessor_keys[&holder][i];
                    if !self.accessors.contains_key(&(holder, a)) {
                        continue;
                    }
                    self.acc_group(holder, Key::Named(a));
                    self.group_link(holder, a);
                }
            }
        }
    }

    /// Link named group (holder, a) under the holder's Any group.
    pub(super) fn group_link(&mut self, holder: Tok, a: Atom) {
        let any = self.acc_groups[&(holder, Key::any())].clone();
        let g = self.acc_groups[&(holder, Key::Named(a))].clone();
        self.edge_quiet(any.gt, g.gt);
        self.edge_quiet(g.ar, any.ar);
        self.edge_quiet(g.rg, any.rg);
        self.edge_quiet(g.rs, any.rs);
        self.edge_quiet(any.st, g.st);
        self.edge_quiet(any.sp, g.sp);
        if g.has_setter {
            self.group_live(holder, Key::any());
        }
    }

    /// Accessor `f` hangs off group (holder, key).
    pub(super) fn group_connect(&mut self, holder: Tok, key: Key, f: Tok, setter: bool) {
        let func = matches!(self.toks[f as usize], TokKind::Func(..));
        // DECISIONS 225: a lookup reached the accessor's group.
        if self.act.is_some() && func {
            let g = self.tok_fn(f);
            self.activate(g, "accessor");
        }
        let g = self.acc_groups[&(holder, key)].clone();
        // An accessor with a lexical `this` (arrow) never sees the
        // receiver, as for a call (`call_tok`). A class or builtin
        // accessor gets its receivers through its call (`group_call`).
        let this =
            (func && !self.funcs[self.tok_fn(f) as usize].lexical_this).then(|| self.this_any(f));
        if setter {
            if let Some(this) = this {
                self.edge_quiet(g.st, this);
            }
            self.group_bind(holder, key, f, true);
            self.group_live(holder, key);
            if self.acc_groups.contains_key(&(holder, Key::any())) {
                self.group_live(holder, Key::any());
            }
        } else {
            if let Some(this) = this {
                self.edge_quiet(g.gt, this);
            }
            self.group_bind(holder, key, f, false);
        }
    }

    /// The parameter/result half of `group_connect`.
    pub(crate) fn group_bind(&mut self, holder: Tok, key: Key, ft: Tok, setter: bool) {
        if !matches!(self.toks[ft as usize], TokKind::Func(..)) {
            if !self.sum.ready {
                self.defer(crate::summary::Deferred::Group(holder, key, ft, setter));
                return;
            }
            return self.group_call(holder, key, ft, setter);
        }
        let f = self.tok_fn(ft);
        self.activate(f, "accessor");
        if self.diagnostic.is_some() {
            self.diagnostic_reach(f, "accessor", u32::MAX);
        }
        // What the accessor throws reaches the group's getter or setter
        // raise node, which feeds the region of every access that runs it
        // (`group_hit`).
        let raise = self.raise_of(f);
        let g = &self.acc_groups[&(holder, key)];
        let rs = if setter { g.rs } else { g.rg };
        self.edge(raise, rs);
        if !self.sum.ready {
            self.defer(crate::summary::Deferred::Group(holder, key, ft, setter));
            return;
        }
        let (sp, ar, this) = {
            let g = &self.acc_groups[&(holder, key)];
            (g.sp, g.ar, if setter { g.st } else { g.gt })
        };
        if self.summarized(f) {
            // The group's receivers are the instance's `this`, as they are
            // the shared `this` in `group_connect` (none for a lexical one).
            let this = (!self.funcs[f as usize].lexical_this).then_some(this);
            self.group_summarized(ft, this, sp, ar, setter);
            return;
        }
        // A getter has no arguments; a setter has exactly its stored value.
        // Missing arguments are undefined, including for default parameters.
        for i in usize::from(setter)..self.funcs[f as usize].params.len() {
            let p = self.funcs[f as usize].params[i];
            self.add_prim(p, prim::UNDEF);
        }
        if setter {
            if let Some(&p0) = self.funcs[f as usize].params.first() {
                self.edge_quiet(sp, p0);
            }
        } else {
            let ret = self.funcs[f as usize].ret;
            self.edge_quiet(ret, ar);
        }
    }

    /// An accessor whose function is a class, a builtin or a bound function
    /// (B.2.2.2-3 `__defineGetter__(k, Object)`, a descriptor `get: C`):
    /// [[Get]] runs `Call(getter, receiver)`, [[Set]] runs
    /// `Call(setter, receiver, « v »)`, as for a program accessor. The
    /// group's call of `f`, with each receiver of the group as `this`
    /// (`Cons::NativeThis`), runs that value's ordinary call: a class
    /// throws a TypeError before it reads `this` (nothing flows), `Object()`
    /// is a fresh object, an `Object.prototype` method runs on the receiver,
    /// a bound function calls its target. What it throws reaches the
    /// groups raise node, as a program accessors does (`group_bind`).
    fn group_call(&mut self, holder: Tok, key: Key, f: Tok, setter: bool) {
        let g = self.acc_groups[&(holder, key)].clone();
        let ret = self.node_l(if setter {
            "acc-call-setret"
        } else {
            "acc-call-ret"
        });
        if !setter {
            self.edge(ret, g.ar);
        }
        let (recv, args) = if setter {
            (g.st, vec![g.sp])
        } else {
            (g.gt, vec![])
        };
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Node(recv),
            args,
            spread: vec![],
            ret,
            site: 0,
            region: if setter { g.rs } else { g.rg },
        });
        self.why_mint(call, None, u32::MAX);
        self.rel(|| format!("accessorcall {call} {holder} {f} {}", setter as u8));
        self.cons(recv, Cons::NativeThis { call, f });
    }

    /// A group has a setter: its waiting store sources flow from now on.
    pub(super) fn group_live(&mut self, holder: Tok, key: Key) {
        let g = self.acc_groups.get_mut(&(holder, key)).unwrap();
        if g.has_setter {
            return;
        }
        g.has_setter = true;
        let pending = std::mem::take(&mut g.pending);
        let sp = g.sp;
        for src in pending {
            self.store_live(src);
            self.edge(src, sp);
        }
    }

    /// The one getter under own accessor key `a` of `h` that is a contract
    /// function of this unit (`Gen::note_own_getters`): no setter, every
    /// getter token one function of this unit, in `contract_fns`. Other
    /// units run it through its contract, which checks its result; the
    /// literal's leave walk does not load it (`leave_walk`).
    pub(crate) fn contract_getter(&self, h: Tok, a: Atom) -> Option<FnIdx> {
        if !self.getter_owners.contains(&h) {
            return None;
        }
        let (gs, ss) = self.accessors.get(&(h, a))?;
        if !ss.is_empty() {
            return None;
        }
        let mut one = None;
        for &g in gs {
            let f = self.tok_fn(g);
            if f == NO_FN
                || self.funcs[f as usize].func_id == u32::MAX
                || !self.contract_fns.contains(&f)
                || one.is_some_and(|o| o != f)
            {
                return None;
            }
            one = Some(f);
        }
        // Its readers run the stub once a census broke its contract.
        one.filter(|&f| {
            !self
                .broken_fns
                .contains(&format!("fn:{}", self.funcs[f as usize].name))
        })
    }

    /// What other units read from published literal `t` through its
    /// accessors loads into `dst` (`Gen::fields_leave`): every accessor on
    /// its prototypes, as a computed-key walk finds them, and its own
    /// accessors except its contract getters (`contract_getter`), whose
    /// results leave through their contracts. An accessor installed on `t`
    /// later joins (`accessor_installed`).
    pub(crate) fn leave_walk(&mut self, t: Tok, dst: NodeId) {
        let seen = self.leave_loads.insert(t, dst).is_some();
        if !self.needs_accessor_walk(Key::any()) {
            return;
        }
        let act = ChainAct::Load(dst, self.thrown);
        let cp = self.cp(t);
        if self.any_walk_done.insert((cp, t, act)) {
            self.any_walks.push((cp, t, t, act));
            self.cons(
                cp,
                Cons::AnyAccessors {
                    origin: t,
                    key: Key::any(),
                    act,
                },
            );
        }
        if seen {
            return;
        }
        let keys = self.accessor_keys.get(&t).cloned().unwrap_or_default();
        for a in keys {
            self.leave_key(t, a);
        }
    }

    /// Own accessor key `a` of a leaving literal `t` (`leave_walk`): unless
    /// it holds a contract getter, what it returns loads into the leave
    /// node. The computed-key group contributes its possible getter results too.
    fn leave_key(&mut self, t: Tok, a: Atom) {
        let Some(&dst) = self.leave_loads.get(&t) else {
            return;
        };
        if !self.accessors.contains_key(&(t, a)) {
            return;
        }
        // Other units run a contract getter on the literal: so does its
        // contract function here (as `note_field_fns` for a method).
        if let Some(f) = self.contract_getter(t, a) {
            if !self.funcs[f as usize].lexical_this {
                for g in self.accessors[&(t, a)].0.clone() {
                    let this = self.this_any(g);
                    self.add_tok(this, t);
                }
            }
            return;
        }
        self.acc_group(t, Key::Named(a));
        self.group_hit(t, t, Key::Named(a), ChainAct::Load(dst, self.thrown));
    }

    /// Accessor `f` was installed on (holder, key) while solving. With
    /// `replay`, lookups that reached the holder before it had a group for
    /// this key run it now; later ones find it through the group.
    pub(super) fn accessor_installed(
        &mut self,
        holder: Tok,
        key: Atom,
        f: Tok,
        setter: bool,
        replay: bool,
    ) {
        let k = Key::Named(key);
        let wildcard = self.atoms[key as usize] == crate::STAR_KEY;
        let visits: Vec<_> = self
            .visits
            .get(&holder)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|&(_, kk, _)| {
                        wildcard || kk == k || (kk.is_any() && self.shape_admits(holder, kk, key))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for &(_, _, act) in &visits {
            if let ChainAct::Lookup {
                dst,
                setter: wanted,
            } = act
            {
                if wanted == setter {
                    self.add_tok(dst, f);
                }
            }
        }
        let visits: Vec<_> = visits
            .into_iter()
            .filter(|&(_, _, act)| !matches!(act, ChainAct::Lookup { .. }))
            .collect();
        if self.acc_groups.contains_key(&(holder, k)) {
            self.group_connect(holder, k, f, setter);
        } else {
            let any_exists = self.acc_groups.contains_key(&(holder, Key::any()));
            if (replay && !visits.is_empty()) || any_exists {
                // Creating the group picks up every accessor there (this
                // one too) and links it under an existing Any group.
                self.acc_group(holder, k);
            }
            if replay {
                for (origin, _, act) in visits {
                    self.group_hit(origin, holder, k, act);
                }
            }
        }
        self.leave_key(holder, key);
        self.add_accessor_atom(key);
        if replay && !self.acc_groups.contains_key(&(holder, Key::any())) {
            let walks = self
                .any_walk_holders
                .get(&holder)
                .cloned()
                .unwrap_or_default();
            for (origin, wk, act) in walks {
                if self.shape_splits(holder, wk).is_some() {
                    if wildcard || self.key_may_name(wk, key) {
                        self.acc_group(holder, k);
                        self.group_hit(origin, holder, k, act);
                    }
                } else {
                    self.acc_group(holder, Key::any());
                    self.group_hit(origin, holder, Key::any(), act);
                }
            }
        }
    }

    /// A computed-key lookup `kk` that reached `holder` may name accessor
    /// key `a`: always, unless its shape splits the holder and excludes `a`.
    fn shape_admits(&self, holder: Tok, kk: Key, a: Atom) -> bool {
        self.shape_splits(holder, kk).is_none() || self.key_may_name(kk, a)
    }

    /// Reuse recorded visits and leave nodes to start the walks that were
    /// not needed when those reads/stores first arrived (`add_accessor_atom`).
    fn deferred_walks(&mut self, key: Atom) {
        let wildcard = self.atoms[key as usize] == crate::STAR_KEY;
        let numeric = self.numeric_accessor_key(key);
        let num = self.atom(NUM_KEY);
        let mut pending = Vec::new();
        for (&t, visits) in &self.visits {
            for &(origin, k, act) in visits {
                let numeric_read = k == Key::Named(num) && numeric;
                if !matches!(act, ChainAct::Lookup { .. })
                    && (wildcard || k.is_any() || k == Key::Named(key) || numeric_read)
                {
                    pending.push((origin, t, if numeric_read { Key::any() } else { k }, act));
                }
            }
        }
        // Hash-map order must not choose the worklist's first-arrival path.
        pending.sort_unstable_by_key(|&(origin, t, k, act)| {
            let k = match k {
                Key::Named(a) => (0, a),
                Key::Any(s) => (1, s),
            };
            let act = match act {
                ChainAct::Load(n, _) => (0, n),
                ChainAct::Store(n, _) => (1, n),
                ChainAct::Call(c) => (2, c),
                ChainAct::Lookup { dst, setter } => (3 + u8::from(setter), dst),
            };
            (origin, t, k, act)
        });
        for (origin, t, k, act) in pending {
            self.accessor_walk(origin, t, k, act);
        }
        let mut leaves: Vec<_> = self.leave_loads.iter().map(|(&t, &n)| (t, n)).collect();
        leaves.sort_unstable();
        for (t, n) in leaves {
            self.leave_walk(t, n);
        }
    }
}
