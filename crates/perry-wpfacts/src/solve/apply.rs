use super::*;

impl Model {
    pub(super) fn apply_inner(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        match *c {
            Cons::Load { key, dst } => {
                if prim != 0 {
                    self.with_b(|m| builtins::prim_load(m, prim, key, dst));
                }
                for &t in toks {
                    self.load_from(t, t, key, dst);
                }
            }
            Cons::CjsPublish { dst } => {
                self.add_prim(dst, prim);
                let marker = self.atom("__perry_cjs_record");
                let ea = self.atom("exports");
                for &t in toks {
                    let record = matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                        if self.tmpls[tm as usize].birth_keys.contains(&marker));
                    if record {
                        self.load_from(t, t, Key::Named(ea), dst);
                    } else {
                        self.add_tok(dst, t);
                    }
                }
            }
            Cons::OwnValues { dst } => {
                self.add_prim(dst, prim & prim::STR);
                for &t in toks {
                    self.own_values_of(t, dst);
                }
            }
            Cons::Store { key, src, site } => {
                self.store_at = Some(n);
                for &t in toks {
                    self.store_into(t, key, src, site);
                }
                self.store_at = None;
            }
            Cons::Call { call } => {
                for &t in toks {
                    self.call_tok(call, t, None);
                }
            }
            Cons::CallThis { call, this } => {
                for &t in toks {
                    self.call_tok(call, t, Some(this));
                }
            }
            Cons::New { call } => {
                for &t in toks {
                    self.construct(call, t);
                }
            }
            Cons::NewNamed { name, call } => {
                if !toks.is_empty() {
                    self.new_named(name, call);
                }
            }
            Cons::MCall { key, call } => {
                // The primitive kinds beside ⊤ add nothing to the call: the
                // call of the unknown value escapes the arguments and returns
                // ⊤ with every kind, which covers the primitive method's touch,
                // its primitive result and its container of primitives
                // (DECISIONS 42's reason, for a receiver's kinds). Made anyway,
                // the container is a value of its own at every ⊤ site the
                // result reaches: one per call site, each with field storage.
                if prim != 0 && !self.nodes[n as usize].pts.contains_tok(TOP) {
                    self.with_b(|m| builtins::prim_method(m, prim, key, call));
                }
                for &t in toks {
                    self.method_on(t, key, call);
                }
            }
            Cons::Chain { origin, key, act } => {
                for &p in toks {
                    self.chain_step(n, origin, p, key, act);
                }
            }
            Cons::TopSink { reason } => {
                let chain = self.chain_sinks.contains(&n);
                let saved = std::mem::replace(&mut self.chain_escape, chain);
                for &t in toks {
                    self.escape(t, reason);
                }
                self.chain_escape = saved;
            }
            Cons::OwnOnly { f, dst } => {
                self.add_prim(dst, prim);
                for &t in toks {
                    let flow = self.is_arg_marker(t)
                        || self
                            .param_marker
                            .get(&t)
                            .is_some_and(|&(g, j)| g == f && j != u32::MAX);
                    if !flow {
                        self.add_tok(dst, t);
                    }
                }
            }
            Cons::HandOver { reason, ctor, arg } => {
                for &t in toks {
                    if self.is_marker(t) {
                        self.handed.insert((t, ctor, arg));
                        self.hand_over_marker(t, reason);
                        continue;
                    }
                    let stand_in = matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                        if self.foreign.contains_key(&tm));
                    // DECISIONS 149e: an array leaves by its element cell,
                    // a primitive method's container too (184).
                    let arr = self.is_cell_arr(t);
                    if !stand_in && !arr {
                        self.escape(t, reason);
                    }
                }
            }
            Cons::StoreOut {
                f,
                dst,
                reach,
                via,
                key,
                reason,
                at,
                to,
            } => {
                let mut from = Vec::new();
                let mut from_reach = Vec::new();
                let mut top = false;
                let mut vals = Vec::new();
                for &t in toks {
                    // The argument marker of `f`'s parameter node `j` is
                    // argument `j`; its other members (the markers this
                    // unit's own calls pass) escape, as they link in.
                    let arg = match self.reach_marker.contains(&t) || !self.is_arg_marker(t) {
                        true => None,
                        false => self.arg_param_of(t, f),
                    };
                    if let Some(j) = arg {
                        from.push(j);
                        let w = self.arg_src_watch.entry(t).or_default();
                        if !w.contains(&(f, j, reason)) {
                            w.push((f, j, reason));
                            let own = self.marker_of.get(&(f, j)).copied();
                            for m in self.arg_members.get(&t).cloned().unwrap_or_default() {
                                if Some(m) != own {
                                    self.escape(m, reason);
                                }
                            }
                        }
                        continue;
                    }
                    match self.param_marker.get(&t) {
                        Some(&(g, j))
                            if g == f && j != u32::MAX && !self.reach_marker.contains(&t) =>
                        {
                            from.push(j)
                        }
                        // DECISIONS 136: what the parameter's argument
                        // reaches (`y.left = y.left.right`) is a value of
                        // the reader too: it stores its own, as the whole
                        // program does, not a value of this unit.
                        Some(&(g, j)) if g == f && j != u32::MAX => {
                            from_reach.push((j, self.param_reach_key(f, j, t)));
                        }
                        // DECISIONS 179: the receiver, and what it
                        // reaches, stored into the receiver (or into what
                        // it reaches) is the reader's own value too: the
                        // reader replays the store on its own receiver.
                        // An own reach flow keeps DECISIONS 149 (below).
                        Some(&(g, u32::MAX)) if g == f && dst == u32::MAX && !reach => {
                            match self.reach_marker.contains(&t) {
                                true => from_reach
                                    .push((u32::MAX, self.param_reach_key(f, u32::MAX, t))),
                                false => from.push(u32::MAX),
                            }
                        }
                        // DECISIONS 136: an argument marker, and what it
                        // reaches, is its members and what they reach
                        // (DECISIONS 44); each one is a source of the flow,
                        // as it links in.
                        _ if self.is_arg_marker(t) => {
                            let own = (dst == u32::MAX && to == at).then_some(at);
                            self.store_out_arg_reach(
                                t,
                                (f, dst, reach, via, key),
                                reason,
                                own,
                                None,
                            );
                        }
                        // DECISIONS 149: only the arguments are the
                        // reader's own values in what its receiver reaches;
                        // this unit's value stored there takes the path of
                        // a store into the receiver's reach marker (its
                        // reach escapes, the value goes where the
                        // receiver's beside values take it).
                        // DECISIONS 149b: a value of this unit a rooted store
                        // put into another function's marker or ⊤ goes there
                        // as before: ⊤ to the reader, and it escapes (below).
                        _ if dst == u32::MAX && reach && to == at => {
                            self.mark_escape_why(at, &|_| {
                                "store: into the receiver's reach".into()
                            });
                        }
                        // Another function's marker: another unit's value
                        // the reader cannot follow (DECISIONS 149b).
                        _ if self.is_marker(t) => {
                            top = true;
                            self.escape(t, reason);
                        }
                        // DECISIONS 176: a value of this unit goes to the
                        // reader's object as a result goes to the caller
                        // (DECISIONS 140): checked after the solve
                        // (`Gen::contract_check`), described by the flow.
                        _ => vals.push(t),
                    }
                }
                if !vals.is_empty() {
                    let k = (f, dst, reach, via, key);
                    let v = match self.store_flows.get(&k).and_then(|fl| fl.vals) {
                        Some(v) => v,
                        None => {
                            let v = self.node_named("stored", || "stored-vals".into());
                            self.node_fn[v as usize] = f;
                            self.store_flows.entry(k).or_default().vals = Some(v);
                            v
                        }
                    };
                    for t in vals {
                        self.add_tok(v, t);
                    }
                }
                let fl = self
                    .store_flows
                    .entry((f, dst, reach, via, key))
                    .or_default();
                for j in from {
                    if !fl.from.contains(&j) {
                        fl.from.push(j);
                    }
                }
                for r in from_reach {
                    if !fl.from_reach.contains(&r) {
                        fl.from_reach.push(r);
                    }
                }
                fl.top |= top;
                fl.prim |= prim;
            }
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
                this,
            } => {
                for &t in toks {
                    let own_class = statics && self.toks[t as usize] == TokKind::ClassObj(tmpl);
                    if self.tmpl_of(t) == Some(tmpl) || own_class {
                        // DECISIONS 107: what this unit runs the method on.
                        if !statics && this != u32::MAX {
                            self.this_admitted.push((this, t));
                        }
                        continue;
                    }
                    let admitted = self.admit_this(t, tmpl, this, reason);
                    if admitted {
                        self.this_admitted.push((this, t));
                    }
                    if !admitted {
                        self.escape(t, reason);
                        // A receiver the method's contract does not admit:
                        // its unit runs it on a value it never saw, so the
                        // contract is reported (census, DECISIONS 85).
                        if let Some(&ft) = self.this_contract_fns.get(this as usize) {
                            self.escape(ft, reason);
                        }
                    }
                }
            }
            Cons::OverrideThis { tmpl, key, reason } => {
                for &t in toks {
                    if self.recv_class.get(&t) == Some(&tmpl) {
                        continue;
                    }
                    match self.param_marker.get(&t) {
                        Some(&(f, u32::MAX)) if !self.reach_marker.contains(&t) => {
                            let ks = self.this_calls.entry(f).or_default();
                            if !ks.contains(&key) {
                                ks.push(key);
                            }
                        }
                        _ => self.escape(t, reason),
                    }
                }
            }
            Cons::Guard { test, from, dst } => {
                let admits = prim & guard::prims(test) != 0
                    || toks
                        .into_iter()
                        .any(|&t| t == TOP || guard::objects(test) || self.is_marker(t));
                if admits {
                    self.edge(from, dst);
                }
            }
            Cons::Defined { dst } => {
                for &t in toks {
                    self.add_tok(dst, t);
                }
                if prim & !prim::UNDEF != 0 {
                    self.add_prim(dst, prim & !prim::UNDEF);
                }
            }
            Cons::TopIfAny { dst, from, reason } => {
                if !toks.is_empty() || prim != 0 {
                    if let Some(r) = reason {
                        self.add_top(dst, |m| m.reasons[r as usize].clone());
                        self.add_prim(dst, prim::ALL);
                    }
                    self.edge(from, dst);
                }
            }
            Cons::DeleteKey { key, reason } => {
                for &t in toks {
                    if t == TOP {
                        // Deleting `key` of an unknown object changes that
                        // key of whatever it is, a builtin prototype
                        // included, and no other key.
                        let k = Some(Key::Named(key));
                        self.note_fnproto_write(TOP, k, Hard::E7Delete.code());
                        builtins::note_native_write(self, TOP, k, "E7 delete");
                    } else {
                        self.mark_hard(t, Hard::E7Delete, reason);
                    }
                }
            }
            Cons::Hard { kind, reason } => {
                for &t in toks {
                    if self.explain && t == TOP {
                        eprintln!(
                            "perry: wpfacts explain: {} on ⊤: {} (receiver: {})",
                            kind.code(),
                            self.reasons[reason as usize],
                            self.top_origin(n)
                        );
                    }
                    self.mark_hard(t, kind, reason);
                }
            }
            Cons::DefineKey { key, data, reason } => {
                for &t in toks {
                    // A key the layout already holds as data may move into an
                    // accessor pair in its slot, and a class instance's layout
                    // is the class's: E5 HARD, as any defineProperty was. (On
                    // another unit's layout the accessor is beyond its contract
                    // in any case: the census breaks it.)
                    match self.toks[t as usize] {
                        TokKind::Obj(tm, _) if !data => {
                            let tpl = &self.tmpls[tm as usize];
                            let new_key = matches!(
                                tpl.kind,
                                TmplKind::Lit { .. }
                                    | TmplKind::Class {
                                        literal_shape: true,
                                        ..
                                    }
                            ) && !tpl.birth_keys.contains(&key);
                            if new_key {
                                *self.adds_keys.entry(tm).or_default() += 1;
                            } else {
                                self.mark_hard(t, Hard::E5DefineProperty, reason);
                            }
                        }
                        // A prototype holds no instance's slots: a key it has
                        // no method under is a key of the chain that is no
                        // plain data slot (its getter, or its value under the
                        // descriptor's attributes), as a class getter is: the
                        // instances' layouts and lookups treat it as an
                        // accessor key (derive), not the whole object as HARD.
                        TokKind::Proto(tm)
                            if !self.tmpls[tm as usize].proto_members.contains(&key) =>
                        {
                            self.virt_revoke(t, Some(key));
                            let ks = self.accessor_keys.entry(t).or_default();
                            if !ks.contains(&key) {
                                ks.push(key);
                            }
                        }
                        // Defining `key` on an unknown object writes that key
                        // of whatever it is, a builtin prototype included,
                        // and no other key.
                        _ if t == TOP => {
                            let k = Some(Key::Named(key));
                            self.note_fnproto_write(TOP, k, Hard::E5DefineProperty.code());
                            builtins::note_native_write(self, TOP, k, "E5 Object.defineProperty");
                        }
                        _ => self.mark_hard(t, Hard::E5DefineProperty, reason),
                    }
                }
            }
            Cons::DefineAll {
                o,
                descs,
                value,
                site,
                reason,
            } => {
                // A string's own values are its characters.
                if prim & prim::STR != 0 {
                    self.add_prim(descs, prim::STR);
                    self.define_any_on(o, descs, value, site, reason);
                }
                for &t in toks {
                    self.define_all(t, o, descs, value, site, reason);
                }
            }
            Cons::DefineDesc {
                o,
                key,
                site,
                reason,
            } => {
                for &d in toks {
                    self.define_desc(d, o, key, site, reason);
                }
            }
            Cons::AddsKeys => {
                for &t in toks {
                    let t = self
                        .contract_recv
                        .get(&t)
                        .and_then(|v| v.first())
                        .copied()
                        .unwrap_or(t);
                    if let Some(tm) = self.tmpl_of(t) {
                        *self.adds_keys.entry(tm).or_default() += 1;
                    }
                }
            }
            Cons::Implicit { reason, proto } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        // A function handed to a reflecting runtime helper:
                        // it may be called back with anything.
                        TokKind::Func(_, _) | TokKind::Bound(_) if proto == Protocol::Any => {
                            self.escape(t, reason)
                        }
                        // `instanceof` a bound function runs its target's.
                        TokKind::Bound(_) if proto == Protocol::HasInstance => {
                            self.escape(t, reason)
                        }
                        _ => self.implicit(t, reason, proto),
                    }
                }
            }
            Cons::CreateProto { reason } => {
                for &t in toks {
                    // E10: every template whose prototype this is can now
                    // have instances born without its constructor.
                    if let TokKind::Proto(tm) = self.toks[t as usize] {
                        let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                        self.mark_hard(inst, Hard::E10ObjectCreate, reason);
                    }
                }
            }
            Cons::ChainUnion { key, dst, of } => {
                for &q in toks {
                    if q == TOP {
                        self.add_tok(dst, TOP);
                        if self.is_constructor_key(key) {
                            let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                            self.add_tok(dst, f);
                        }
                        continue;
                    }
                    if self.is_array_length(q, key) {
                        self.add_prim(dst, prim::NUM);
                        continue;
                    }
                    let f = match key {
                        Key::Named(a) => self.field(q, a),
                        Key::Any => self.all_fields(q),
                    };
                    self.edge(f, dst);
                    self.with_b(|m| builtins::chain_load(m, of, q, key, dst));
                    if self.stops_at(q, key) {
                        self.stopped_shared.entry(q).or_default().push((key, dst));
                    } else {
                        let up = self.cv(q, key);
                        self.edge(up, dst);
                    }
                }
            }
            Cons::ProtoClosure { dst } => {
                for &q in toks {
                    self.add_tok(dst, q);
                    if q != TOP {
                        let up = self.cp(q);
                        self.edge(up, dst);
                    }
                }
            }
            Cons::AnyAccessors { origin, act } => {
                for &q in toks {
                    if q != TOP {
                        self.any_walk_holders
                            .entry(q)
                            .or_default()
                            .push((origin, act));
                    }
                    if q != TOP && self.accessor_keys.get(&q).is_some_and(|k| !k.is_empty()) {
                        self.accessor_hit(origin, q, Key::Any, act);
                    }
                }
            }
            Cons::ProtoMethods { origin, key, call } => {
                for &q in toks {
                    if matches!(
                        self.toks[q as usize],
                        TokKind::ObjectProto | TokKind::NativeProto(_)
                    ) {
                        self.with_b(|m| builtins::proto_method(m, q, origin, key, call));
                    }
                }
            }
            Cons::StoreTopProto { origin, src } => {
                if toks.contains(&TOP) {
                    let r = self.reason("store through an unknown prototype chain");
                    self.sink_node(src, r);
                    self.escape(origin, r);
                }
            }
            Cons::Super { start, key, act } => {
                for &t in toks {
                    self.super_lookup(t, start, key, act);
                }
            }
            Cons::SuperCtor { class, call } => {
                for &t in toks {
                    let owner_class = self.constructor_origin(class);
                    let owner = self.tok(TokKind::ClassObj(owner_class));
                    let parent = self.proto_node(owner);
                    self.cons(
                        parent,
                        Cons::SuperCtorParent {
                            class,
                            call,
                            origin: t,
                        },
                    );
                    if self.tmpls[class as usize].native_parent.is_some() {
                        self.with_b(|m| crate::builtins::native_parent_ctor(m, class, call, t));
                    }
                }
            }
            Cons::SuperCtorParent {
                class,
                call,
                origin,
            } => {
                let owner_class = self.constructor_origin(class);
                let owner = self.tok(TokKind::ClassObj(owner_class));
                if self.super_relink.contains(&owner) || self.is_escaped(owner) {
                    // The instance under construction follows the live
                    // superclass. An unknown one (⊤: another unit's
                    // subclass instance) is a fresh object or whatever the
                    // superclass returned, whose prototype construction
                    // never changes: no write of a builtin prototype.
                    if origin == TOP {
                        self.super_relink.insert(TOP);
                    } else {
                        let reason = self.reason("E6 constructor follows live superclass");
                        self.mark_hard(origin, Hard::E6SetPrototypeOf, reason);
                    }
                }
                for &parent in toks {
                    match self.toks[parent as usize] {
                        TokKind::ClassObj(tm) => {
                            // Record the link before the parent's stores
                            // reach the instance (§17.2 item 2).
                            let oc = &mut self.tmpls[owner_class as usize];
                            if oc.parent.is_none() && !oc.parent_links.contains(&tm) {
                                oc.parent_links.push(tm);
                            }
                            self.run_ctor_chain_this(call, tm, origin)
                        }
                        TokKind::Func(_, _) => self.call_tok(call, parent, Some(origin)),
                        TokKind::Bound(_) => self.construct(call, parent),
                        TokKind::Top | TokKind::BuiltinFn | TokKind::Builtin(_) => {
                            self.call_tok(call, parent, Some(origin));
                            let ret = self.calls[call as usize].ret;
                            self.add_tok(ret, TOP);
                        }
                        _ => {}
                    }
                }
            }
            Cons::BirthField { key, src } => {
                for &t in toks {
                    if t == TOP {
                        continue;
                    }
                    let f = match key {
                        Key::Named(a) => self.field(t, a),
                        Key::Any => self.any_key_node(t),
                    };
                    self.edge(src, f);
                }
            }
            Cons::ThisFrom { recv } => {
                for &t in toks {
                    if let TokKind::Func(f, _) = self.toks[t as usize] {
                        if !self.funcs[f as usize].lexical_this {
                            let this = self.this_any(t);
                            self.edge(recv, this);
                        }
                    }
                }
            }
            Cons::Accessor {
                holder,
                key,
                setter,
            } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        TokKind::Func(_, _) => {
                            self.cons(holder, Cons::AccessorInstall { key, f: t, setter });
                        }
                        TokKind::Top | TokKind::BuiltinFn | TokKind::Bound(_) => {
                            // An unknown accessor: the key it is installed
                            // under (any key, when computed) reads ⊤ on the
                            // holder, and the holder escapes to the code that
                            // runs. It defines that one key: on a holder from
                            // unmodeled code, a builtin prototype may hold it
                            // under that key only.
                            let r = self.reason("accessor of unknown function value");
                            let fa = self.node_l("acc-unknown");
                            self.add_top(fa, |_| "accessor of unknown function value".into());
                            let site = self.anon_post_site("unknown accessor");
                            let k = if self.atoms[key as usize] == crate::STAR_KEY {
                                Key::Any
                            } else {
                                Key::Named(key)
                            };
                            self.cons(
                                holder,
                                Cons::Store {
                                    key: k,
                                    src: fa,
                                    site,
                                },
                            );
                            self.cons(holder, Cons::TopSink { reason: r });
                        }
                        _ => {}
                    }
                }
            }
            Cons::AccessorInstall { key, f, setter } => {
                let star = self.atom(crate::STAR_KEY);
                for &t in toks {
                    if let Some(s) = self.shared_of(t) {
                        let k = self.atoms[key as usize].clone();
                        if self.explain && !self.shared_replay && s != Shared::Top {
                            eprintln!(
                                "perry: wpfacts explain: accessor {} on shared {} key {k:?}",
                                self.tok_name(f),
                                self.tok_name(t)
                            );
                        }
                        self.note_shared(SharedWrite::Accessor(s, k));
                    }
                    self.note_fnproto_write(t, Some(Key::Named(key)), "accessor");
                    builtins::note_native_write(self, t, Some(Key::Named(key)), "accessor");
                    if t == TOP {
                        continue;
                    }
                    if key == star {
                        // A computed-key accessor: it may answer any key, so
                        // it runs as unknown code and every key read through
                        // this holder may yield anything (derive kills the
                        // templates that reach it).
                        let r = if self.explain {
                            let why = format!(
                                "accessor under a computed key ({} on {})",
                                self.tok_name(f),
                                self.tok_name(t)
                            );
                            self.reason(&why)
                        } else {
                            self.reason("accessor under a computed key")
                        };
                        self.escape(f, r);
                        let fa = self.any_key_node(t);
                        self.add_top(fa, |_| "accessor under a computed key".into());
                        self.add_prim(fa, prim::ALL);
                        let ks = self.accessor_keys.entry(t).or_default();
                        if !ks.contains(&star) {
                            ks.push(star);
                        }
                        continue;
                    }
                    let entry = self.accessors.entry((t, key)).or_default();
                    let list = if setter { &mut entry.1 } else { &mut entry.0 };
                    if list.contains(&f) {
                        continue;
                    }
                    list.push(f);
                    let ks = self.accessor_keys.entry(t).or_default();
                    if !ks.contains(&key) {
                        ks.push(key);
                    }
                    let objproto = matches!(self.toks[t as usize], TokKind::ObjectProto);
                    if objproto {
                        // Every lookup reaches Object.prototype: rather than
                        // replay them all, the derivation turns every fact
                        // off (an accessor on Object.prototype is E23).
                        if self.explain && !self.objproto_accessor {
                            eprintln!(
                                "perry: wpfacts explain: E23 accessor {} on Object.prototype key {}",
                                self.tok_name(f),
                                self.atoms[key as usize]
                            );
                        }
                        self.objproto_accessor = true;
                    }
                    self.accessor_installed(t, key, f, setter, !objproto);
                }
            }
            Cons::ProtoFrom { src } => {
                for &t in toks {
                    if t == TOP || matches!(self.toks[t as usize], TokKind::ClassObj(_)) {
                        self.cons(src, Cons::ParentGiven { of: t });
                    }
                    // `%Object.prototype%` is an immutable prototype exotic
                    // object: setting its [[Prototype]] to anything but its
                    // own (null) throws.
                    if t == TOP || matches!(self.toks[t as usize], TokKind::ObjectProto) {
                        continue;
                    }
                    let pn = self.proto_node(t);
                    self.edge(src, pn);
                    if self.given_proto.insert((src, pn)) {
                        self.cons(src, Cons::ProtoGiven { protos: pn });
                    }
                    if let TokKind::ClassObj(tm) = self.toks[t as usize] {
                        let instance = self.tok(TokKind::Obj(tm, NO_CTX));
                        let reason =
                            self.reason("E6 live superclass may change constructor birth fields");
                        self.mark_hard(instance, Hard::E6SetPrototypeOf, reason);
                    }
                }
            }
            // `ObjectProto` given as a prototype may be any builtin
            // prototype, `Function.prototype` included: `seed_proto` no
            // longer says what the chain's `ObjectProto` is.
            Cons::ProtoGiven { protos } => {
                if toks
                    .iter()
                    .any(|&t| matches!(self.toks[t as usize], TokKind::ObjectProto))
                {
                    self.add_tok(protos, TOP);
                }
            }
            // A class given as the parent of class object `of`: `of`'s
            // `super()` runs its constructor on `of`'s instances. A class of
            // this unit given to a class it constructs runs there
            // (`Cons::SuperCall` follows the live superclass); given to
            // another unit's class or to ⊤, the code running `super()` is
            // unknown here, so the class escapes to it. Another unit's class
            // is that unit's to run on the receiver: the census reports it
            // (`extends <class>`), and its unit publishes its constructor
            // as it does for an `extends` naming it (DECISIONS 57).
            Cons::ParentGiven { of } => {
                let theirs = |m: &Self, t: Tok| match m.toks[t as usize] {
                    TokKind::ClassObj(tm) => m.foreign.contains_key(&tm),
                    _ => t == TOP,
                };
                for &p in toks {
                    let TokKind::ClassObj(tm) = self.toks[p as usize] else {
                        continue;
                    };
                    if self.foreign.contains_key(&tm) {
                        self.relinked_parents.insert(tm);
                    } else if theirs(self, of) {
                        let reason = self
                            .reason("class given as the parent of a class unknown code constructs");
                        self.escape(p, reason);
                    }
                }
            }
            Cons::ProtoOf { dst } => {
                let pk = self.atom("__proto__");
                for &t in toks {
                    // A parameter marker has no prototype here (DECISIONS
                    // 119): the read is a `__proto__` step its reader
                    // replays on the argument.
                    if self.is_marker(t) && !self.virt_protos.contains_key(&t) {
                        self.load_from(t, t, Key::Named(pk), dst);
                        continue;
                    }
                    self.proto_value(t, dst);
                }
            }
            Cons::ProtoValues { dst } => {
                self.add_prim(dst, prim & prim::NULL);
                for &p in toks {
                    match self.toks[p as usize] {
                        TokKind::Proto(tm)
                            if matches!(
                                self.tmpls[tm as usize].kind,
                                TmplKind::Class {
                                    literal_shape: true,
                                    ..
                                }
                            ) =>
                        {
                            self.proto_value(p, dst)
                        }
                        _ => self.add_tok(dst, p),
                    }
                }
            }
            Cons::PlainKey { origin, act } => {
                for &q in toks {
                    match self.toks[q as usize] {
                        TokKind::Top => match act {
                            ChainAct::Load(dst) => self.add_tok(dst, TOP),
                            ChainAct::Call(call) => self.call_tok(call, TOP, Some(origin)),
                            ChainAct::Store(_) => {}
                        },
                        TokKind::ObjectProto
                        | TokKind::NativeProto(_)
                        | TokKind::Builtin(_)
                        | TokKind::BuiltinFn => {
                            self.with_b(|m| builtins::plain_key(m, origin, act));
                        }
                        _ => {
                            let f = self.num_fields(q);
                            match act {
                                ChainAct::Load(dst) => self.edge(f, dst),
                                ChainAct::Call(call) => self.bind_via(f, call, origin),
                                ChainAct::Store(_) => {}
                            }
                        }
                    }
                }
            }
            Cons::IterNext { dst } => {
                for &t in toks {
                    match self.toks[t as usize].clone() {
                        TokKind::Top => self.add_tok(dst, TOP),
                        TokKind::Func(_, _) | TokKind::Bound(_) => self.add_tok(dst, t),
                        TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind {
                            _ if self.is_array_like(t) => {}
                            _ => {
                                let a = self.atom("next");
                                self.load_from(t, t, Key::Named(a), dst);
                            }
                        },
                        _ => {
                            let a = self.atom("next");
                            self.load_from(t, t, Key::Named(a), dst);
                        }
                    }
                }
            }
            // A for-in run's key (FVA 4.3b): `o[k]` reads that one key.
            Cons::IdxLoad { idx, dst } if self.key_nodes.contains_key(&idx) => {
                let a = self.key_nodes[&idx];
                for &t in toks {
                    self.load_from(t, t, Key::Named(a), dst);
                }
            }
            Cons::IdxLoad { idx, dst } => {
                for &t in toks {
                    if t == TOP {
                        self.add_tok(dst, TOP);
                        continue;
                    }
                    let all = self.all_fields(t);
                    self.edge(all, dst);
                    self.add_prim(dst, prim::UNDEF);
                    if self.idx_done.insert((idx, t, dst, 0)) {
                        self.plain_key(t, ChainAct::Load(dst));
                        self.cons(idx, Cons::IdxKey { obj: t, dst });
                    }
                }
            }
            Cons::IdxKey { obj, dst } => {
                let named = prim & (prim::STR | prim::SYMBOL) != 0 || !toks.is_empty();
                if named && self.idx_done.insert((n, obj, dst, 1)) {
                    self.load_from(obj, obj, Key::Any, dst);
                }
                // ToPropertyKey of `true`, `null`, `undefined` is a named key.
                if !named {
                    for (bit, keys) in [
                        (prim::BOOL, &["true", "false"][..]),
                        (prim::NULL, &["null"][..]),
                        (prim::UNDEF, &["undefined"][..]),
                    ] {
                        if prim & bit != 0 && self.idx_done.insert((n, obj, dst, 2 + bit as u8)) {
                            for k in keys {
                                let a = self.atom(k);
                                self.load_program_key(obj, a, dst);
                            }
                        }
                    }
                }
            }
            Cons::IdxMCall { idx, call } => {
                // Primitive receivers (`MCall`'s rule): a number key calls
                // a key a store into ⊤ wrote on their prototype.
                if prim != 0 && !self.nodes[n as usize].pts.contains_tok(TOP) {
                    self.with_b(|m| builtins::prim_plain_key(m, prim, call));
                    let p = prim & !(prim::NULL | prim::UNDEF);
                    if p != 0 && self.idx_done.insert((idx, TOP, call, 128 | p as u8)) {
                        self.cons(
                            idx,
                            Cons::IdxKeyCall {
                                obj: TOP,
                                prim: p,
                                call,
                            },
                        );
                    }
                }
                for &t in toks {
                    // ⊤ and a parameter marker take any key (the call
                    // escapes; the marker's reader replays the call).
                    if t == TOP || (self.is_marker(t) && !self.virt_protos.contains_key(&t)) {
                        self.method_on(t, Key::Any, call);
                        continue;
                    }
                    if self.idx_done.insert((idx, t, call, 0)) {
                        self.mark_use(t, crate::model::MARK_ESCAPE);
                        let own = self.all_fields(t);
                        self.bind_via(own, call, t);
                        self.plain_key(t, ChainAct::Call(call));
                        self.cons(
                            idx,
                            Cons::IdxKeyCall {
                                obj: t,
                                prim: 0,
                                call,
                            },
                        );
                    }
                }
            }
            Cons::IdxKeyCall {
                obj,
                prim: recv_prim,
                call,
            } => {
                let named = prim & (prim::STR | prim::SYMBOL) != 0 || !toks.is_empty();
                // One rule per receiver: a token, or (⊤) primitive kinds.
                let rule = match obj {
                    TOP => 128 | recv_prim as u8,
                    _ => IDX_CALL_ANY,
                };
                if named && self.idx_done.insert((n, obj, call, rule)) {
                    match obj {
                        TOP => self.with_b(|m| builtins::prim_method(m, recv_prim, Key::Any, call)),
                        _ => self.method_on(obj, Key::Any, call),
                    }
                }
                // ToPropertyKey of `true`, `null`, `undefined` is a named key.
                if !named && obj != TOP {
                    for (bit, keys) in [
                        (prim::BOOL, &["true", "false"][..]),
                        (prim::NULL, &["null"][..]),
                        (prim::UNDEF, &["undefined"][..]),
                    ] {
                        if prim & bit != 0
                            && self
                                .idx_done
                                .insert((n, obj, call, IDX_CALL_NAMED + bit as u8))
                        {
                            for k in keys {
                                let a = self.atom(k);
                                self.method_on(obj, Key::Named(a), call);
                            }
                        }
                    }
                }
            }
            Cons::ForInBody { obj, call } => {
                for &t in toks {
                    if matches!(self.toks[t as usize], TokKind::Func(..))
                        && self.for_in_on.insert((obj, t, call))
                    {
                        self.cons(obj, Cons::ForInKeys { ft: t, call });
                    }
                }
            }
            Cons::ForInKeys { ft, call } => {
                // A primitive's keys (a string's indices) are no literal's.
                if prim != 0 {
                    self.for_in_every_key(call, ft);
                }
                for &t in toks {
                    self.for_in_tok(call, ft, t);
                }
            }
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => {
                if let Some(k) = self.keyed_once(n, o, 0) {
                    match k {
                        Some(key) => self.cons(
                            o,
                            Cons::DefineKey {
                                key,
                                data: !accessor,
                                reason,
                            },
                        ),
                        None => self.cons(
                            o,
                            Cons::Hard {
                                kind: Hard::E5DefineProperty,
                                reason,
                            },
                        ),
                    }
                }
            }
            Cons::StoreKeyed { o, src, site } => {
                if let Some(k) = self.keyed_once(n, src, 1) {
                    let key = k.map_or(Key::Any, Key::Named);
                    self.cons(o, Cons::Store { key, src, site });
                }
            }
            Cons::AccessorKeyed { holder, f, setter } => {
                if let Some(k) = self.keyed_once(n, f, 2 + setter as u8) {
                    let key = k.unwrap_or_else(|| self.atom(crate::STAR_KEY));
                    self.cons(
                        f,
                        Cons::Accessor {
                            holder,
                            key,
                            setter,
                        },
                    );
                }
            }
            Cons::Arith { dst, add } => {
                let mut out = 0u16;
                // Objects go through ToPrimitive (user valueOf/toString).
                let objs = prim::NUM | prim::BIGINT | if add { prim::STR } else { 0 };
                // A described parameter's marker is no value (DECISIONS 74);
                // an argument marker is none while it stands for such
                // markers alone, and adds the kinds if it leaves (75).
                if toks.iter().any(|&t| !self.beside_values(t)) {
                    out |= objs;
                } else {
                    for &t in toks {
                        if self.beside_markers.contains(&t) {
                            self.beside_arith.entry(t).or_default().push((dst, objs));
                        }
                    }
                }
                if prim & prim::BIGINT != 0 {
                    out |= prim::BIGINT;
                }
                if prim & (prim::NUM | prim::BOOL | prim::NULL | prim::UNDEF) != 0 {
                    out |= prim::NUM;
                }
                if prim & prim::STR != 0 {
                    out |= if add { prim::STR } else { prim::NUM };
                }
                if out != 0 {
                    self.add_prim(dst, out);
                }
            }
            Cons::ArrayLenArg { dst } => {
                self.add_tokens(dst, toks, prim & !prim::NUM);
            }
            Cons::ShareBuffer { ta } => {
                for &t in toks {
                    let shared = t == TOP
                        || matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                            if matches!(self.tmpls[tm as usize].kind, TmplKind::BObj { what: "TypedArray", .. }));
                    if shared {
                        let r = self.reason("B1 typed array over a shared buffer");
                        self.mark_hard(ta, Hard::B1Exposed, r);
                    }
                }
            }
            Cons::ThisTo { origin } => {
                let calls = self
                    .via_calls
                    .get(&(n, origin))
                    .cloned()
                    .unwrap_or_default();
                for &t in toks {
                    // Each receiver binds every call looked up through
                    // this node: solve work per call.
                    self.work += calls.len() as u64;
                    self.this_to(t, origin, &calls);
                    self.this_calls(t, origin, &calls);
                }
            }
            Cons::CallArgs { call } => {
                for &t in toks {
                    self.call_args(call, t);
                }
            }
            Cons::RetInto { call, run } => self.ret_into(call, run, toks, prim),
            Cons::ArgInto { call, run, i } => self.arg_into(call, run, i, toks),
            Cons::Spread { into, site } => {
                // A primitive source copies no key surely (`null`,
                // `undefined`: none at all).
                if prim != 0 {
                    if let Some(&tm) = self.lit_spread_site.get(&site) {
                        self.spread_source(tm, site, Vec::new());
                    }
                }
                for &s in toks {
                    self.cons(into, Cons::SpreadFrom { src: s, site });
                }
            }
            Cons::SpreadFrom { src, site } => {
                for &d in toks {
                    self.spread_into(src, d, site);
                }
            }
            Cons::ThisInst { call, ft } => {
                for &t in toks {
                    self.bind_layout(call, ft, Some(t), 0);
                }
                if prim != 0 {
                    self.bind_layout(call, ft, None, prim);
                }
            }
            Cons::HolderStep { key, pass, stop } => {
                for &q in toks {
                    if q == TOP {
                        self.add(pass, &[TOP], 0);
                    } else if self.stops_at(q, key) {
                        self.add(stop, &[q], 0);
                        self.stopped_holders
                            .entry(q)
                            .or_default()
                            .push((key, pass, stop));
                    } else {
                        let (p2, s2) = self.holders(q, key);
                        self.edge_quiet(p2, pass);
                        self.edge_quiet(s2, stop);
                    }
                }
            }
            Cons::VisitHolders { origin, key, act } => {
                for &h in toks {
                    if h == TOP {
                        self.chain_step(0, origin, TOP, key, act);
                        continue;
                    }
                    if !matches!(self.toks[h as usize], TokKind::ObjectProto) {
                        self.visits.entry(h).or_default().insert((origin, key, act));
                    }
                    self.accessor_hit(origin, h, key, act);
                }
            }
            Cons::VisitStop { origin, key, act } => {
                for &h in toks {
                    self.accessor_hit(origin, h, key, act);
                }
            }
            Cons::BuiltinCallee => {
                for &t in toks {
                    if matches!(self.toks[t as usize], TokKind::Builtin(_)) {
                        self.builtin_callee.insert(n);
                        let sites = self.callee_sites.get(&n).cloned().unwrap_or_default();
                        for (call, origin) in sites {
                            self.call_tok(call, t, Some(origin));
                        }
                    }
                }
            }
            Cons::NewTargetHard { reason } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        TokKind::ClassObj(tm) => {
                            let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                            self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                        }
                        // DECISIONS 210: another unit's constructor function:
                        // the hard mark lands on its owner's layout (a census
                        // report); unpublished, the function escapes.
                        TokKind::Func(f, _) if self.foreign_ctor.contains_key(&f) => {
                            match self.foreign_ctor[&f] {
                                Some((_, tm)) => {
                                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                                    self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                                }
                                None => self.escape(t, reason),
                            }
                        }
                        TokKind::Func(f, _) => {
                            let tm = self.fn_inst_template(f);
                            let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                            self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
