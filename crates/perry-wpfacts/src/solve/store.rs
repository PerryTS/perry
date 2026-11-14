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
            self.charge_work(fresh.len() as u64);
            if self.past_work_bound() {
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
            self.charge_work(1);
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

    /// A store into a published method's receiver marker is a store into
    /// an instance of this unit as well: the marker stands for instances of
    /// the classes the method is published with (DECISIONS 59), and this
    /// unit models every instance of its own classes and their subclasses,
    /// wherever constructed (DECISIONS 125), as the class's instance token.
    /// The reader replays the store on the object it admits
    /// (`this_stores`), but a caller that runs the method unseen (an
    /// override another unit's parent constructor calls, `override_node`)
    /// replays nothing: without this the instance's layout and kinds miss
    /// the store, and per unit would prove a slot, a readonly or a kind the
    /// whole program does not (SHC6 h4). What it stores is the caller's
    /// value (`Cons::CallerValue`).
    fn store_into_own_instances(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) {
        let Some(classes) = self.virt_protos.get(&t).cloned() else {
            return;
        };
        let own: Vec<TmplId> = (0..self.tmpls.len() as TmplId)
            .filter(|&tm| {
                matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. })
                    && !self.foreign.contains_key(&tm)
                    && classes.iter().any(|&c| self.has_ancestor(tm, c))
                    && self.has_foreign_ancestor(tm)
            })
            .collect();
        if own.is_empty() {
            return;
        }
        // The stored value is relative to the caller (a marker stands for
        // what the caller passes or holds), and a caller running the method
        // unseen puts nothing beside it: a marker is ⊤ in these instances.
        let v = match self.caller_value.get(&src) {
            Some(&v) => v,
            None => {
                let v = self.node_l("caller-value");
                self.caller_value.insert(src, v);
                let reason = self.reason("stored by a published method on its receiver");
                self.cons(src, Cons::CallerValue { dst: v, reason });
                v
            }
        };
        for tm in own {
            let inst = self.tok(TokKind::Obj(tm, NO_CTX));
            self.store_into(inst, key, v, site);
        }
    }

    /// A typed array element store (`store_into`): the element field holds
    /// the Number (BigInt) TypedArraySetElement converts `src` to; the
    /// conversion's ToPrimitive runs `src`'s `@@toPrimitive` / `valueOf` /
    /// `toString` (once per value and site).
    /// What an element of typed array (Buffer) `t` is: a Number, or a
    /// BigInt for a BigInt64Array / BigUint64Array (ECMA-262 10.4.5.15
    /// TypedArrayGetElement, 23.2.7 element types); a Buffer's is a Number.
    /// A template whose kind the model does not keep may be either.
    pub(crate) fn typed_array_elem_prim(&self, t: Tok) -> u16 {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind {
                TmplKind::BObj { what: "Buffer", .. } => prim::NUM,
                _ => match self.ta_elem.get(&tm).copied() {
                    Some(perry_hir::TYPED_ARRAY_KIND_BIGINT64 | perry_hir::TYPED_ARRAY_KIND_BIGUINT64) => {
                        prim::BIGINT
                    }
                    Some(_) => prim::NUM,
                    None => prim::NUM | prim::BIGINT,
                },
            },
            _ => prim::NUM | prim::BIGINT,
        }
    }

    fn typed_array_element_store(&mut self, t: Tok, src: NodeId, site: SiteId) {
        let f = match self.is_elem_array(t) {
            true => self.field_any(t),
            false => self.numeric_field(t),
        };
        let p = self.typed_array_elem_prim(t);
        self.add_prim(f, p);
        if self.ta_store_done.insert((src, site)) {
            builtins::string_methods::to_prim(self, src, "typed array element store's ToNumber / ToBigInt");
        }
    }

    /// Does `tm` descend from a class whose code this unit does not see:
    /// another unit's class, or a parent that is no class of this unit
    /// when it was generated (`parent_value`, §17.2). That code runs on
    /// `tm`'s instances unseen (its constructor, its methods) and may call
    /// `tm`'s methods on them (`override_node`).
    fn has_foreign_ancestor(&self, tm: TmplId) -> bool {
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            if c != tm && self.foreign.contains_key(&c) {
                return true;
            }
            let t = &self.tmpls[c as usize];
            if t.parent_value.is_some() {
                return true;
            }
            work.extend(t.parent);
            work.extend(t.parent_links.iter().copied());
        }
        false
    }

    /// DECISIONS 241: `src` stored on function token `t` under `key`, kept
    /// apart from what its field holds from the join, so the unit publishes
    /// only its own stores. `prototype` is DECISIONS 210(1)'s.
    fn note_fn_store(&mut self, t: Tok, key: Key, src: NodeId) {
        let n = match key {
            Key::Named(a) if self.atoms[a as usize] == "prototype" => return,
            Key::Named(a) => match self.fn_stored.get(&(t, a)) {
                Some(&n) => n,
                None => {
                    let n = self.node_named("fnstored", || format!("fnstored({t},{a})"));
                    self.fn_stored.insert((t, a), n);
                    n
                }
            },
            Key::Any(_) => match self.fn_stored_any.get(&t) {
                Some(&n) => n,
                None => {
                    let n = self.node_named("fnstored", || format!("fnstoredany({t})"));
                    self.fn_stored_any.insert(t, n);
                    n
                }
            },
        };
        self.edge(src, n);
    }

    pub(crate) fn store_into(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) {
        // A call binding its rest array's argument at a known position
        // (`Model::rest_elem_node`), or a store under one constant index
        // (`Cons::StoreIndexed`): an element store, into index slot `i` of
        // a rest array.
        let slot = match key {
            Key::Named(a) => self.index_slot_atom(a),
            _ => None,
        };
        let key = if slot.is_some() { self.elem_key() } else { key };
        let slot = slot.filter(|_| self.is_rest_arr(t));
        // A class private field: what its class's code stores (`private_field`);
        // on an object of this unit, its own field too.
        if let Key::Named(a) = key {
            if self.is_private_atom(a) {
                let p = self.private_field(a);
                self.edge(src, p);
                if !self.owns_private(t) {
                    return;
                }
            }
        }
        // An element store (`push`, `unshift`, `splice`: the element key) is
        // an index store: on an Array it writes its elements only, which no
        // named non-index key reads (`is_elem_array`); on another holder
        // it writes the numeric range, never an arbitrary string key.
        // ECMA-262 10.4.5.5 with 7.1.19: a Number-or-BigInt key (`DIGITS`)
        // on a typed array is a CanonicalNumericIndexString (an element
        // store, below) or a BigInt's digits that are none: an
        // OrdinarySet of a digit-only name, the numeric range (`#num`,
        // `numeric_field`), never another name. On any other holder
        // (10.4.2.1 an Array, 10.1.9 an ordinary object) it names an index
        // or another Number::toString / BigInt::toString form: the
        // numeric range of that holder (`num_only`, below), never a
        // non-numeric name. Which index it is stays unknown: the length,
        // the hole and the chain's setters are the unknown key's.
        let digits_key = self.is_digits_key(key);
        let digits = digits_key && self.typed_array_elements(t);
        if digits {
            self.typed_array_element_store(t, src, site);
        }
        let digits_other = digits_key && !digits;
        // Rule C1 (`ta_length_getter`) under the runtime lock: a BufferHeader
        // view (a Node Buffer, a plain Uint8Array) takes a [[Set]] of a
        // non-index name as an own data property (`buffer_set_own_prop`, from
        // `js_object_set_field_by_name` and the computed-key path, without
        // the inherited accessor that ECMA-262 10.1.9.2 finds), and a typed
        // array's template does not keep its kind: a [[Set]] that may name
        // `length` may make an own `length`. A Number-or-BigInt key never
        // names `length` (its ToString is digits, NaN or Infinity).
        if !self.is_elem_key(key)
            && !digits_key
            && match key {
                Key::Named(a) => self.atoms[a as usize] == "length",
                Key::Any(_) => true,
            }
        {
            self.ta_len_open(t);
        }
        let key = if digits_key { Key::any() } else { key };
        let elem = self.is_elem_key(key) || digits;
        // The key names a number only: the numeric range's field.
        let num_only = elem || digits_other;
        let key = if elem { Key::any() } else { key };
        // ECMA-262 10.4.5.5 / 10.4.5.16: an integer-indexed exotic object's
        // [[Set]] under a numeric key (an element-key store is one: a Number
        // key, an index string, an append) runs TypedArraySetElement, which
        // stores ToNumber(v), or ToBigInt(v) on a BigInt64/BigUint64 array,
        // never `v`; no prototype setter is met.
        if elem && !digits && self.typed_array_elements(t) {
            self.typed_array_element_store(t, src, site);
            return;
        }
        // A computed store may be a runtime helper's write of a Map's
        // contents (`solve/map_vals.rs`).
        if matches!(key, Key::Any(_)) {
            self.map_open(t);
        }
        // DECISIONS 242(2): a [[Set]] of a key the object does not own
        // definitely meets the chain's setters (and may add the key).
        if t != TOP {
            self.note_chain_miss(t, key);
        }
        // DECISIONS 231(c): a named store is the value of that key on the
        // receivers of the classes `t` is or stands for (`virt_store`); a
        // computed one opens them. A store into ⊤ may be a store into an
        // escaped instance (`top_store`).
        match key {
            Key::Named(a) => self.virt_store(t, a, src),
            Key::Any(_) => self.virt_revoke(t, None),
        }
        if t == TOP {
            self.top_store(match key {
                Key::Named(a) => Some(a),
                Key::Any(_) => None,
            });
        }
        // DECISIONS 210: a store into another unit's function (a stand-in)
        // under `prototype` (or a key that may be it) replaces the chain its
        // owner's layout has: the census reports it. Its other properties
        // left in the owner (`escape_props`).
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            let proto = match key {
                Key::Named(a) => self.atoms[a as usize] == "prototype",
                Key::Any(_) => true,
            };
            if proto && self.foreign_ctor.contains_key(&f) {
                self.fn_touched.insert(t);
            }
        }
        // The boundary's store for other units' code is theirs to publish.
        let theirs = self.store_at.is_some() && self.store_at == self.other_units_store;
        if let Some(s) = self.shared_of(t).filter(|_| !theirs) {
            let k = match key {
                Key::Named(a) => Some(self.atoms[a as usize].clone()),
                Key::Any(_) if num_only => Some(NUM_KEY.to_string()),
                Key::Any(_) => None,
            };
            self.note_shared(SharedWrite::Store(s, k));
        }
        // A program-owned global: an ordinary field of the global object
        // (one field per key, whichever token stored it).
        if let Key::Named(a) = key {
            if builtins::global_object(self, t)
                && builtins::global_user_key(self, &self.atoms[a as usize])
                && !builtins::native_written(self, t, a)
            {
                let g = self.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let f = builtins::global_field(self, g, a);
                self.edge(src, f);
                return;
            }
        }
        // A [[Set]] that meets the realm's accessor without a setter
        // (`navigator` on the global object) writes nothing; in strict code
        // it throws the runtime's TypeError (`Model::thrown`). Until a
        // native write may have replaced the accessor: then the store runs
        // again on the ordinary path (`NativeMiss::Store`).
        if let Key::Named(a) = key {
            if !elem
                && !theirs
                && !self.unit_mode
                && builtins::getter_only(self, t, &self.atoms[a as usize])
                && !builtins::native_written(self, t, a)
            {
                self.native_missing
                    .push((t, a, NativeMiss::Store(src, site)));
                return;
            }
        }
        // Native function values use ordinary fields below. Constructors
        // and namespaces retain the realm-declared boundary: the runtime
        // itself consumes some declared statics (Error.prepareStackTrace),
        // even when the program never hands the constructor to an operand
        // protocol. Only their undeclared keys are program-owned (R1).
        if !theirs {
            if let Some(f) = self.builtin_field(t, key) {
                self.edge(src, f);
                self.accessor_walk(t, t, key, ChainAct::Store(src));
                let cp = self.cp(t);
                if self.callthis_done.insert((cp, u32::MAX, t)) {
                    self.cons(cp, Cons::StoreTopProto { origin: t, src });
                }
                return;
            }
        }
        // An element store writes an index of a builtin prototype at most.
        let wkey = match num_only {
            true => Key::Named(self.atom(NUM_KEY)),
            false => key,
        };
        // The boundary's store for other units' code is theirs: they note
        // (and publish) what they write, as for `note_native_write` below.
        if !theirs {
            self.note_fnproto_write(t, Some(wkey), "store");
            self.note_array_member_write(t, Some(wkey), Some(site), "store");
        }
        // Explain: the first store into ⊤ under a key names its site and why
        // the receiver is ⊤.
        let what = match (self.explain && t == TOP, self.store_at) {
            (true, Some(n)) => {
                let (desc, func) = self
                    .stores
                    .get(site as usize)
                    .map_or(("(no site)", u32::MAX), |s| (s.desc.as_str(), s.func));
                let f = self
                    .funcs
                    .get(func as usize)
                    .map_or("(no function)", |f| f.name.as_str());
                format!(
                    "store `{desc}` in {f} (receiver: {}; path {})",
                    self.top_origin(n),
                    self.top_path(n, 24)
                )
            }
            _ => "store".to_string(),
        };
        // Not the boundary's store for other units' code: what they store
        // into ⊤ they publish (`SharedWrite::Store(Top, _)`; a unit past the
        // bound publishes every key), and every unit replays it under its key.
        if !theirs {
            self.census_store_ctx(Some((site, self.store_at)));
            if !builtins::builtin_function(self, t) {
                builtins::note_native_data_write(
                    self,
                    t,
                    Some(wkey),
                    &what,
                    builtins::WriteValue::Value(src),
                );
            }
            self.census_store_ctx(None);
        }
        // OPTIMISTIC (`json_key_optimistic`): the key names no built-in
        // member, so no `__proto__`: the store writes an own property.
        let json_key = self.json_key_optimistic(t, key);
        if json_key {
            self.json_key_fired.push((true, src));
        }
        // An element store's key is a number: it never names `__proto__`.
        if key.is_any() && !num_only && !json_key && self.key_may_name_str(key, "__proto__")
            || matches!(key, Key::Named(a) if self.atoms[a as usize] == "__proto__")
        {
            self.super_relink.insert(t);
            if !self.unit_mode && self.is_builtin_proto(t) {
                self.given_prototype(t, src);
            }
            if let TokKind::ClassObj(tm) = self.toks[t as usize] {
                let objs = self.proto_objects(src);
                self.cons(objs, Cons::ParentGiven { of: t });
                let parent = self.proto_node(t);
                self.edge(objs, parent);
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
            if key.is_any() {
                self.mark_escape_why(t, &|_| "store: under a computed key".into());
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
                    Key::Any(_) => {
                        self.this_open.insert(f);
                    }
                }
                self.store_into_own_instances(t, key, src, site);
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
                    Key::Any(_) => {
                        self.param_open.insert((f, i));
                    }
                }
                let r = self.reason("stored into another unit's value");
                self.sink_node(src, r);
                return;
            }
        }
        self.mark_escape_why(t, &|_| "store: into a marker".into());
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
                let private = self.is_private_atom(a);
                match facts.kind {
                    "litclass" | "lit" => {
                        k != "__proto__"
                            && !facts
                                .fields
                                .iter()
                                .any(|f| !f.data_field && f.is(k, private))
                    }
                    "class" => facts.fields.iter().any(|f| {
                        f.data_field && f.is(k, private) && (f.definite || f.declared || f.reserved)
                    }),
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
            (Some(facts), Key::Any(_)) if matches!(self.toks[t as usize], TokKind::Obj(..)) => {
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
                Key::Any(_) if effect => {
                    let a = self.atom(crate::STAR_KEY);
                    self.foreign_stores.insert((tm, a, src));
                }
                _ => {
                    self.foreign_written.insert(tm);
                }
            }
        }
        if self.is_arr(t) {
            self.arr_len_store(t, key, elem, site);
            // A store under one fixed index of a rest array writes that
            // index only (`index_slot`) and moves no other element.
            let fixed = match key {
                Key::Named(a) if self.is_rest_arr(t) => {
                    self.atoms[a as usize].parse::<u32>().ok().filter(|i| i.to_string() == self.atoms[a as usize])
                }
                _ => None,
            };
            if let Some(i) = fixed {
                let s = self.index_slot(t, i);
                self.edge(src, s);
            }
            // DEADFN15: the front (`rest_front`) gets index 0's stores and
            // the stores at an index not known; a `length` store may move
            // index 0 (C1).
            if self.is_rest_arr(t) {
                if self.is_array_length(t, key) {
                    self.rest_front_widen(t);
                } else if matches!((slot, fixed, key), (Some(0), _, _) | (_, Some(0), _) | (None, None, Key::Any(_))) {
                    let n = self.rest_front(t);
                    self.edge(src, n);
                }
            }
            match slot.is_some() || fixed.is_some() {
                true => self.arr_written_at(t),
                false => self.arr_written(t),
            }
        }
        // An index past the end, a computed key or a `length` store may
        // leave a hole; an append (`elem`) never does (`hole`).
        let index = match key {
            Key::Named(a) => numeric_key(&self.atoms[a as usize]) || self.is_array_length(t, key),
            Key::Any(_) => !elem,
        };
        if index {
            self.mark_hole(t);
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
        // DECISIONS 241: what this unit stores on a function value's own
        // property is published to every holder of it (`fn_stored`).
        if self.unit_mode && matches!(self.toks[t as usize], TokKind::Func(..)) {
            self.note_fn_store(t, key, src);
        }
        match key {
            Key::Named(a) => {
                let f = match slot {
                    Some(i) => self.index_slot(t, i),
                    None => self.field(t, a),
                };
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
                        *self.adds_named.entry((tm, a)).or_default() += 1;
                    }
                }
            }
            Key::Any(_) => {
                // A rest array's argument at its position (`slot`): its index
                // slot, which flows into its elements (`index_slot`).
                let f = if let Some(i) = slot {
                    self.index_slot(t, i)
                } else if num_only && self.is_elem_array(t) {
                    self.field_any(t)
                } else if num_only {
                    self.numeric_field(t)
                } else {
                    self.key_store_node(t, key)
                };
                // Relations: a store under a key not known here writes the
                // object's any-key (or numeric) field node.
                let (sf, sm) = (self.stores[site as usize].func, self.stores[site as usize].module);
                self.rel(|| format!("anystore {t} {src} {f} site={site} fn={sf} m={sm}"));
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
        // Setters on the chain (and own accessors), including an accessor
        // installed through another token that may denote this native.
        self.builtin_field_use(t, key, ChainAct::Store(src));
        // A setter a declaration made the receiver with (`onabort`).
        self.with_b(|m| builtins::declared_setter_store(m, t, key, src));
        self.accessor_walk(t, t, key, ChainAct::Store(src));
        if !self.is_array_like(t) {
            let cp = self.cp(t);
            if self.callthis_done.insert((cp, u32::MAX, t)) {
                self.cons(cp, Cons::StoreTopProto { origin: t, src });
            }
        }
        // A callable namespace's own property is the realm's only under a
        // static the realm itself reads (`realm_reads_store`). Every other
        // key is a field of the function graph (above), which program
        // reads and the protocols run on it read.
        let namespace = matches!(self.toks[t as usize], TokKind::Builtin(ns)
            if !matches!(ns, BuiltinNs::ObjectProtoOwn(_) | BuiltinNs::ObjectProtoMethod));
        // A number key names no declared static (the realm declares no
        // index) and the runtime reads no index of a built-in function or
        // namespace: an element store there writes that object's own
        // numeric field (above), which its reads see; no E20 sink. Not the
        // global object's (its indices are globals unmodeled code reads),
        // not a prototype's (the lock's, `note_native_write`).
        let own_index = elem
            && match self.toks[t as usize] {
                _ if builtins::global_object(self, t) => false,
                TokKind::Builtin(_) | TokKind::BuiltinFn(_) => true,
                _ => false,
            };
        if (!builtins::builtin_function(self, t)
            || (namespace && builtins::realm_reads_store(self, t, key)))
            && !own_index
        {
            self.with_b(|m| builtins::tok_store(m, t, key, src));
        }
    }
}
