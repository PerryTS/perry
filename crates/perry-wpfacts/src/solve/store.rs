use super::*;

impl Model {
    /// DECISIONS 118/149: `src` stored under `key` into parameter `i`'s
    /// argument of contract function `f` (or what it reaches, marker `t`)
    /// is a flow of `f`'s contract (`Cons::StoreOut`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn store_flow_out(
        &mut self,
        t: Tok,
        to: Tok,
        f: FnIdx,
        i: u32,
        reach: bool,
        key: Key,
        src: NodeId,
    ) {
        // Into the parameter's own keyed reach: under `a` of what the
        // argument holds under `via`, not into all it reaches.
        let via = self.param_reach_key(f, i, to);
        self.store_flows.entry((f, i, reach, via, key)).or_default();
        if self.store_out_done.insert((src, f, i, reach, via, key, to)) {
            let reason = self.reason("stored into another unit's value");
            self.cons(
                src,
                Cons::StoreOut {
                    f,
                    dst: i,
                    reach,
                    via,
                    key,
                    reason,
                    at: t,
                    to,
                },
            );
        }
    }

    /// DECISIONS 149b: a receiver-rooted store (its object is reached from
    /// its function G's `this` by loads) is G's store into what its
    /// receiver reaches, which the whole program runs in the caller's own
    /// objects: a contract function's marker it meets there (a
    /// constructor's argument kept in a field), or ⊤ (a value another unit
    /// gave), is the caller's value. G's parameters stored there are G's
    /// receiver flow; any other value takes the path it took before
    /// (`Cons::StoreOut`). Whether the target is reached is the reader's
    /// own solve: its view of the receiver, ⊤ fields included.
    pub(super) fn rooted_store(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) -> bool {
        // A replayed summary keeps no store sites: nothing there is rooted.
        let Some(s) = self.stores.get(site as usize) else {
            return false;
        };
        let g = s.func;
        if !s.rooted || self.closed_params.contains_key(&g) {
            return false;
        }
        match self.param_marker.get(&t) {
            Some(&(f, _)) if self.closed_params.contains_key(&f) => return false,
            // G's own receiver (and what it reaches) is DECISIONS 149's.
            Some(&(f, u32::MAX)) if f == g => return false,
            Some(_) => {}
            None if t == TOP => {}
            None => return false,
        }
        let Some(at) = self
            .marker_of
            .get(&(g, u32::MAX))
            .and_then(|m| self.marker_reach.get(m))
            .copied()
        else {
            return false;
        };
        self.store_flow_out(at, t, g, u32::MAX, true, key, src);
        true
    }

    /// DECISIONS 149d: a deep receiver-rooted store of contract function G
    /// (its object is loaded from `this`, `rooted.rs`) into an object of
    /// this unit (`this.storage[n] = e`, the storage G's constructor made). Its caller's argument is the caller's value,
    /// which the whole program stores into the caller's view of that object:
    /// G's parameter (and argument) markers are not stored there: they are
    /// G's receiver flow, which the reader replays on what its receiver
    /// reaches (a published field's stand-in, whose stores come back here
    /// through their contract, or ⊤, where it escapes). The flow is the
    /// store's into the receiver's keyed reach marker, which the store's
    /// object holds beside this object by construction (it is loaded from
    /// G's `this` marker, and a load from a marker reads its reach markers,
    /// DECISIONS 111). Every other value is stored as before. Returns the
    /// node to store.
    pub(super) fn rooted_own_store(&mut self, t: Tok, src: NodeId, site: SiteId) -> NodeId {
        if t == TOP || !matches!(self.toks[t as usize], TokKind::Obj(..)) {
            return src;
        }
        let Some(s) = self.stores.get(site as usize) else {
            return src;
        };
        let g = s.func;
        if !s.deep || self.closed_params.contains_key(&g) {
            return src;
        }
        if !self.marker_of.contains_key(&(g, u32::MAX)) {
            return src;
        }
        let own = match self.rooted_own.get(&(src, g)) {
            Some(&n) => n,
            None => {
                let n = self.node_named("rooted", || format!("rooted-own({src})"));
                self.rooted_own.insert((src, g), n);
                self.cons(src, Cons::OwnOnly { f: g, dst: n });
                n
            }
        };
        own
    }

    /// Every builtin function value may have been written: every property
    /// read of one is ⊤ from now on.
    pub(super) fn write_builtin_fns(&mut self, r: u32) {
        if self.bf_written.is_some() {
            return;
        }
        self.bf_written = Some(r);
        self.note_shared(SharedWrite::Store(Shared::BuiltinFn, None));
        self.bf_stores.clear();
        for dst in std::mem::take(&mut self.bf_reads) {
            self.add_top(dst, |_| {
                "property of a written builtin function value".into()
            });
        }
    }

    /// The stores into a builtin function value, once the worklist is
    /// empty: one at a node without ⊤ wrote one (true: solve on). A node
    /// that holds ⊤ keeps it, so its stores are stores into ⊤ for good. So
    /// does one holding a bare marker: another unit's argument whole, which
    /// every rule reads as ⊤ (DECISIONS 119).
    pub(super) fn settle_bf_written(&mut self) -> bool {
        if self.bf_written.is_some() || self.bf_stores.is_empty() {
            return false;
        }
        let stores = std::mem::take(&mut self.bf_stores);
        let holds_top = |m: &Self, n: NodeId| {
            let pts = &m.nodes[n as usize].pts;
            pts.contains_tok(TOP) || pts.to_sorted().into_iter().any(|t| m.bare_marker(t))
        };
        match stores.iter().find(|&&(n, _)| !holds_top(self, n)) {
            Some(&(_, r)) => {
                self.write_builtin_fns(r);
                true
            }
            None => false,
        }
    }

    /// DECISIONS 193: the readers argument marker `x` stands for gain
    /// `new` (a marker below it, or a keyed reach of one, `arg_readers`):
    /// the stores made on `x` and on its keyed reaches reach them, and so do
    /// those of each argument marker `x` is a member of, which gains them in
    /// turn. Each (marker, reader) is added once and each is solve work, so
    /// a store costs its readers, and a link the readers it brings to the
    /// markers above it; no argument marker on the way keeps a store.
    pub(crate) fn arg_readers_add(&mut self, x: Tok, new: Vec<(Tok, Option<Atom>)>) {
        let mut st = vec![(x, new)];
        while let Some((x, zs)) = st.pop() {
            if self.aborted {
                return;
            }
            let fresh: Vec<(Tok, Option<Atom>)> = zs
                .into_iter()
                .filter(|&(z, k)| self.arg_reader_seen.insert((x, z, k)))
                .collect();
            if fresh.is_empty() {
                continue;
            }
            self.work += fresh.len() as u64;
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            self.arg_readers
                .entry(x)
                .or_default()
                .extend(fresh.iter().copied());
            if let Some(ss) = self.arg_stores.get(&x).cloned() {
                for &(z, k) in &fresh {
                    self.arg_store_reaches(z, k, None, &ss);
                }
            }
            for (a, kr) in self.keyed_reach.get(&x).cloned().unwrap_or_default() {
                if let Some(ss) = self.arg_stores.get(&kr).cloned() {
                    for &(z, k) in &fresh {
                        self.arg_store_reaches(z, k, Some(a), &ss);
                    }
                }
            }
            for q in self.arg_parents.get(&x).cloned().unwrap_or_default() {
                st.push((q, fresh.clone()));
            }
            // `x`'s keyed reach under `a` is a member of `q`: `q` stands for
            // what each marker below `x` reaches under `a` (DECISIONS 121).
            for (a, q) in self.arg_keyed_parents.get(&x).cloned().unwrap_or_default() {
                let v: Vec<(Tok, Option<Atom>)> = fresh
                    .iter()
                    .filter(|&&(_, k)| k.is_none())
                    .map(|&(z, _)| (z, Some(a)))
                    .collect();
                if !v.is_empty() {
                    st.push((q, v));
                }
            }
        }
    }

    /// The readers marker `m` brings to the argument marker it links into:
    /// itself (a parameter's marker or its reach), what an argument marker
    /// stands for, or for an argument marker's keyed reach under `a` what
    /// the markers below its base reach under `a`.
    /// DECISIONS 194: a closed function no other unit calls has every
    /// caller here; a store through its parameter composes on their values
    /// beside its marker, and no flow crosses a unit edge: its marker reads
    /// no store.
    pub(super) fn arg_readers_of(&self, m: Tok) -> Vec<(Tok, Option<Atom>)> {
        if !self.is_arg_marker(m) {
            if self.param_marker.get(&m).is_some_and(|&(f, _)| {
                self.closed_params.contains_key(&f) && !self.closed_called_out.contains(&f)
            }) {
                return Vec::new();
            }
            return vec![(m, None)];
        }
        match self.keyed_parent.get(&m).copied() {
            Some((p, a)) => self
                .arg_readers
                .get(&p)
                .into_iter()
                .flatten()
                .filter(|&&(_, k)| k.is_none())
                .map(|&(z, _)| (z, Some(a)))
                .collect(),
            None => self.arg_readers.get(&m).cloned().unwrap_or_default(),
        }
    }

    /// Stores `ss`, made on an argument marker (`k`: on its keyed reach
    /// under `k`), reaching reader `z` (`kz`: what `z` reaches under `kz`).
    /// A keyed reach's own keyed reach: no member stands under it.
    pub(super) fn arg_store_reaches(
        &mut self,
        z: Tok,
        kz: Option<Atom>,
        k: Option<Atom>,
        ss: &[(Key, NodeId, SiteId)],
    ) {
        let y = match (kz, k) {
            (None, None) => z,
            (Some(a), None) | (None, Some(a)) => self.keyed_reach_of(z, a),
            (Some(_), Some(_)) => return,
        };
        for &(key, src, site) in ss {
            self.work += 1;
            self.store_into(y, key, src, site);
        }
    }

    /// A keyed rule on key node `n` (FVA 4.3b) applies once per operand:
    /// `Some(Some(k))` for a for-in run's one key, `Some(None)` for any
    /// other key, `None` when it already applied.
    pub(super) fn keyed_once(&mut self, n: NodeId, x: NodeId, kind: u8) -> Option<Option<Atom>> {
        if !self.keyed_done.insert((n, x, kind)) {
            return None;
        }
        Some(self.key_nodes.get(&n).copied())
    }

    pub(crate) fn store_into(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) {
        {
            self.virt_revoke(
                t,
                match key {
                    Key::Named(a) => Some(a),
                    Key::Any => None,
                },
            );
        }
        // A store into a builtin function value is decided once the
        // worklist is empty (`settle_bf_written`): `write_builtin_fns` notes it.
        // The boundary's store for other units' code is theirs to publish.
        let theirs = self.store_at.is_some() && self.store_at == self.other_units_store;
        if let Some(s) = self
            .shared_of(t)
            .filter(|s| *s != Shared::BuiltinFn && !theirs)
        {
            let k = match key {
                Key::Named(a) => Some(self.atoms[a as usize].clone()),
                Key::Any => None,
            };
            self.note_shared(SharedWrite::Store(s, k));
        }
        // A program-owned global: an ordinary field of the global object.
        if let (TokKind::Builtin(BuiltinNs::GlobalThis), Key::Named(a)) =
            (&self.toks[t as usize], key)
        {
            if builtins::global_user_key(&self.atoms[a as usize])
                && !builtins::native_written(self, t, a)
            {
                let f = builtins::global_field(self, t, a);
                self.edge(src, f);
                return;
            }
        }
        self.note_fnproto_write(t, Some(key), "store");
        builtins::note_native_write(self, t, Some(key), "store");
        if matches!(key, Key::Any)
            || matches!(key, Key::Named(a) if self.atoms[a as usize] == "__proto__")
        {
            self.super_relink.insert(t);
            if let TokKind::ClassObj(tm) = self.toks[t as usize] {
                let parent = self.proto_node(t);
                self.edge(src, parent);
                let instance = self.tok(TokKind::Obj(tm, NO_CTX));
                let reason = self.reason("E6 possible computed superclass write");
                self.mark_hard(instance, Hard::E6SetPrototypeOf, reason);
            }
        }
        if t == TOP {
            // DECISIONS 149b: ⊤ reached from the receiver is the reader's.
            if self.rooted_store(t, key, src, site) {
                return;
            }
            let r = self.reason("stored into a value from unmodeled code");
            self.sink_node(src, r);
            return;
        }
        if matches!(self.toks[t as usize], TokKind::BuiltinFn) {
            // A builtin function value is opaque: it has no field storage
            // here. The value is gone to the runtime, and every property
            // read of such a value is ⊤ from now on (`load_from`).
            let r = self.reason("stored into a builtin function value");
            self.sink_node(src, r);
            // Beside ⊤ the value is that ⊤ (`Model::top_beside`), but it may
            // reach the node before ⊤ does: the node decides once the
            // worklist is empty. A store from no rule's node decides now.
            if self.bf_written.is_none() {
                match self.store_at {
                    Some(n) => self.bf_stores.push((n, r)),
                    None => self.write_builtin_fns(r),
                }
            }
            return;
        }
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Store(key, src, site));
            for i in is {
                self.store_into(i, key, src, site);
            }
        }
        // DECISIONS 118: an argument marker stands for its members (DECISIONS
        // 44): a store into it under a named key is a store into each one,
        // replayed on each one that links in later. A store into its keyed
        // reach under `k` goes into each member's keyed reach under `k`
        // alone, not into all its members reach (DECISIONS 121). Under a
        // computed key, the same into each member under any key (DECISIONS
        // 205): the marker has no fields, and no value sits beside it to
        // take the store, so it went nowhere. It still marks the argument
        // escaped, as before.
        if self.is_arg_marker(t) {
            if self.aborted {
                return;
            }
            if key == Key::Any {
                self.mark_use(t, crate::model::MARK_ESCAPE);
            }
            if self.arg_store_done.insert((t, key, src)) {
                // DECISIONS 187/193: kept on `t` alone, where it was made,
                // and handed to the readers `t` stands for (a keyed reach:
                // its base's readers, under its key); a reader that comes
                // later takes it from `t` (`arg_readers_add`).
                self.arg_stores.entry(t).or_default().push((key, src, site));
                let s = [(key, src, site)];
                match self.keyed_parent.get(&t).copied() {
                    Some((p, k)) => {
                        for (z, kz) in self.arg_readers.get(&p).cloned().unwrap_or_default() {
                            self.arg_store_reaches(z, kz, Some(k), &s);
                        }
                    }
                    None => {
                        for (z, kz) in self.arg_readers.get(&t).cloned().unwrap_or_default() {
                            self.arg_store_reaches(z, kz, None, &s);
                        }
                    }
                }
            }
            return;
        }
        // A store into a published method's receiver marker is a key of its
        // receiver contract, not an escape (DECISIONS 57): the reader puts
        // what the method stores under the key of the instance it admits.
        // DECISIONS 179: under a named key, what it stores is a flow of the
        // contract, as for a parameter (DECISIONS 118, 176): the reader's
        // arguments and receiver it names, this unit's values given out.
        if let Some(&(f, u32::MAX)) = self.param_marker.get(&t) {
            if !self.reach_marker.contains(&t) {
                match key {
                    Key::Named(a) => {
                        let ks = self.this_stores.entry(f).or_default();
                        if !ks.contains(&a) {
                            ks.push(a);
                        }
                        // A closed constructor too: a subclass instance
                        // of another unit runs it on the reader's object.
                        self.store_flow_out(t, t, f, u32::MAX, false, key, src);
                    }
                    Key::Any => {
                        self.this_open.insert(f);
                    }
                }
                return;
            }
        }
        // DECISIONS 149b: a receiver-rooted store meeting a marker.
        if self.param_marker.contains_key(&t) && self.rooted_store(t, key, src, site) {
            return;
        }
        // The same for a contract function's parameter (DECISIONS 84): the
        // reader stores an unknown value under the key of its argument. The
        // stored value goes into another unit's object: it escapes. A
        // closed function's callers' values are beside its marker: the
        // store reaches them there (decision 13).
        if let Some(&(f, i)) = self.param_marker.get(&t) {
            // DECISIONS 118: under a named key, the store is a flow of the
            // contract. A parameter's argument stored into another's (or
            // into what that reaches) is the reader's store of its own
            // values, as the whole program runs it; any other stored value
            // goes into another unit's object: it escapes (`Cons::StoreOut`).
            // DECISIONS 149: so is a store into what an argument or the
            // receiver reaches under a computed key (`this.items[i] = e`):
            // the reader stores into its own object under any key, as the
            // whole program does. (A store into the receiver itself is a
            // key of its receiver contract, above.)
            let reach = self.reach_marker.contains(&t);
            if !self.closed_params.contains_key(&f)
                && (i != u32::MAX || reach)
                && (reach || matches!(key, Key::Named(_)))
            {
                if let (false, Key::Named(a)) = (reach, key) {
                    let ks = self.param_stores.entry((f, i)).or_default();
                    if !ks.contains(&a) {
                        ks.push(a);
                    }
                }
                self.store_flow_out(t, t, f, i, reach, key, src);
                return;
            }
            if !self.reach_marker.contains(&t) && !self.closed_params.contains_key(&f) {
                match key {
                    Key::Named(_) => unreachable!("a named store is a flow (DECISIONS 118)"),
                    Key::Any => {
                        self.param_open.insert((f, i));
                    }
                }
                let r = self.reason("stored into another unit's value");
                self.sink_node(src, r);
                return;
            }
        }
        self.mark_use(t, crate::model::MARK_ESCAPE);
        // A parameter marker stands for another unit's value (design §11.2):
        // it has no fields in this unit. The store is its contract's escape
        // (above), and the stored value goes where the receiver's other
        // values take it: ⊤ beside a contract parameter's marker (stored into
        // unmodeled code: it escapes), a closed function's callers' values
        // beside its marker (decision 13). A field of the marker would only
        // hold every value stored into any marker it meets (DECISIONS 41).
        if self.is_marker(t) {
            return;
        }
        let src = self.rooted_own_store(t, src, site);
        // A store into another unit's instance, class object or prototype
        // is beyond its contract (census report).
        let tm = match self.toks[t as usize] {
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) | TokKind::Proto(tm) => Some(tm),
            _ => None,
        };
        // A store into an existing data field of another unit's literal
        // layout opens nothing (§5): a store effect its defining unit
        // performs (decision 17) with what the stored value's contract
        // describes, published as a published binding's value is
        // (`Gen::contract_check`: a value no contract describes escapes, and
        // the field holds an unknown value). A class instance's
        // field is existing when every instance holds it from its birth
        // (definite) or its slot is born with it, present or a hole
        // (declared, `__perry_declared_keys`: DECISIONS 188): the store adds
        // no key (CW2 C, binarytrees_arena; the CommonJS cache).
        // A literal layout's new data key is a shape transition its defining
        // unit performs the same way (design §16, DECISIONS 66): the layout
        // gains the key. `__proto__` and an accessor key store no data.
        let effect = match (tm.and_then(|tm| self.foreign.get(&tm)), key) {
            (Some(facts), Key::Named(a)) if matches!(self.toks[t as usize], TokKind::Obj(..)) => {
                let k = &self.atoms[a as usize];
                match facts.kind {
                    "litclass" | "lit" => {
                        k != "__proto__"
                            && !facts.fields.iter().any(|f| !f.data_field && &f.key == k)
                    }
                    "class" => facts
                        .fields
                        .iter()
                        .any(|f| f.data_field && &f.key == k && (f.definite || f.declared)),
                    _ => false,
                }
            }
            // DECISIONS 188: a declared-keys object (`__perry_declared_keys`,
            // the CommonJS cache) under a computed key: a store its defining
            // unit performs as one under a computed key into its own object
            // (the runtime caching a record under a path it computes,
            // `cjsInstall`); published under `*`, it opens the keys there as
            // the whole program does. Any other layout a computed key opens:
            // a census report (§5).
            (Some(facts), Key::Any) if matches!(self.toks[t as usize], TokKind::Obj(..)) => {
                facts.kind == "class" && facts.fields.iter().any(|f| f.declared)
            }
            _ => false,
        };
        if let Some(tm) = tm.filter(|tm| self.foreign.contains_key(tm)) {
            match key {
                // The stored value is published with the effect, checked
                // as a published value (`Gen::contract_check`, §17.16 (c)).
                Key::Named(a) if effect => {
                    self.foreign_stores.insert((tm, a, src));
                }
                Key::Any if effect => {
                    let a = self.atom(crate::STAR_KEY);
                    self.foreign_stores.insert((tm, a, src));
                }
                _ => {
                    self.foreign_written.insert(tm);
                }
            }
        }
        if self.is_array_length(t, key) {
            let r = self.reason("ToNumber of an array length store");
            self.cons(
                src,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            return;
        }
        // DECISIONS 94: a named key stored into an array other units hold.
        if matches!(key, Key::Named(_)) && self.global_cells {
            if self.cell_arrs.contains_key(&t) {
                self.arr_named.insert(t);
                let r =
                    self.reason("stored into a named field of an array shared with other units");
                self.sink_node(src, r);
            } else if self.is_cell_arr(t) {
                self.arr_named_srcs.entry(t).or_default().push(src);
            }
        }
        match key {
            Key::Named(a) => {
                let f = self.field(t, a);
                self.edge(src, f);
                if let Some(tm) = tm {
                    let ctor = self.stores[site as usize].birth || self.is_ctor_phase(site, tm);
                    // Writers per object: an instance's, the class object's,
                    // the prototype's.
                    let w = match self.toks[t as usize] {
                        TokKind::ClassObj(_) => self.class_writers.entry((tm, a)),
                        TokKind::Proto(_) => self.proto_writers.entry((tm, a)),
                        _ => self.writers.entry((tm, a)),
                    }
                    .or_default();
                    if ctor {
                        if !w.ctor.contains(&site) {
                            w.ctor.push(site);
                        }
                    } else if !w.post.contains(&site) {
                        w.post.push(site);
                    }
                    // A store effect's key exists in the defining unit's layout.
                    if !self.tmpls[tm as usize].birth_keys.contains(&a) && !ctor && !effect {
                        *self.adds_keys.entry(tm).or_default() += 1;
                    }
                }
            }
            Key::Any => {
                let f = self.field_any(t);
                self.edge(src, f);
                if let Some(tm) = tm {
                    let birth = self.stores[site as usize].birth;
                    let w = self.any_writers.entry(tm).or_default();
                    if birth {
                        w.ctor.push(site);
                    } else {
                        w.post.push(site);
                        // A store effect (DECISIONS 188): its defining unit
                        // opens the key set, performing it.
                        if !effect {
                            *self.adds_keys.entry(tm).or_default() += 1;
                        }
                    }
                }
            }
        }
        // Setters on the chain (and own accessors).
        self.accessor_walk(t, t, key, ChainAct::Store(src));
        if !self.is_array_like(t) {
            let cp = self.cp(t);
            if self.callthis_done.insert((cp, u32::MAX, t)) {
                self.cons(cp, Cons::StoreTopProto { origin: t, src });
            }
        }
        self.with_b(|m| builtins::tok_store(m, t, key, src));
    }
}
