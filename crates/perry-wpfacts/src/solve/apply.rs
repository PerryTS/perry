use super::*;

impl Model {
    pub(super) fn apply_inner(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        let pv = self.prov.is_some();
        match *c {
            // An observation for the value-record derive: no flow.
            Cons::Identity { .. } | Cons::Detached => {}
            // `k in o`: no flow on an ordinary object; OrdinaryHasProperty
            // (10.1.7.1 step 3) goes on with parent.[[HasProperty]]: the same
            // rule on the object's prototype closure (once per object, key
            // and key node), where a Proxy runs its `has` trap (rule
            // PXCHAIN, `solve/proxy.rs`). The closure holds every link, so
            // its ordinary links add nothing.
            Cons::HasProp { key, idx } => {
                if !self.chain_has_nodes.contains(&n) {
                    for &t in toks {
                        if t != TOP && self.chain_has_done.insert((t, key, idx)) {
                            let cp = self.cp(t);
                            self.chain_has_nodes.insert(cp);
                            self.cons(cp, Cons::HasProp { key, idx });
                        }
                    }
                }
            }
            // Rule PXCHAIN: only a Proxy in the closure acts (`proxy_apply`).
            Cons::ChainSet { .. } | Cons::ChainRecv { .. } => {}
            // Rule PXDEFINE: an ordinary object's definition is the store
            // itself; only a Proxy runs a trap (`proxy_apply`).
            Cons::DefineOwn { .. } => {}
            // Rule PXRECV: a Proxy's forward to the ordinary objects of its
            // target, with the original receiver (a Proxy target: `proxy_apply`).
            Cons::FwdLoad {
                key,
                idx,
                dst,
                recv,
            } => {
                for &q in toks {
                    self.prov_tok(pv, q);
                    if idx == NO_NODE {
                        self.load_from(recv, q, key, dst);
                    } else {
                        self.idx_load_from(recv, idx, q, dst, 0, false, false);
                    }
                }
            }
            Cons::FwdStore {
                key,
                src,
                site,
                region,
                recv,
            } => {
                self.store_at = Some(n);
                self.store_region = Some(region);
                self.store_recv = Some(recv);
                for &q in toks {
                    self.prov_tok(pv, q);
                    self.store_into(q, key, src, site);
                }
                self.store_at = None;
                self.store_region = None;
                self.store_recv = None;
            }
            // `Object.hasOwn(o, k)` and kin: no flow on an ordinary object (a
            // Proxy's `getOwnPropertyDescriptor` trap: `solve/proxy.rs`).
            Cons::OwnProp { .. } => {}
            // Rule PXINV: only a Proxy acts (`proxy_apply`).
            Cons::DeleteIdx { .. } | Cons::Meta { .. } => {}
            Cons::Load { key, dst } => {
                // The token loop (⊤ first) runs before the primitive load, so a ⊤
                // receiver's read keeps the receiver's own root cause instead of being
                // booked to the String/Number/Boolean.prototype chain it also reads.
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.load_from(t, t, key, dst);
                }
                if prim != 0 {
                    self.with_b(|m| builtins::prim_load(m, prim, key, dst));
                }
            }
            Cons::ProtoKey { key, dst } => {
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.proto_key(t, key, dst);
                }
                if prim != 0 {
                    let r = self.proto_key_relay(key, dst);
                    let pa = self.atom("prototype");
                    self.with_b(|m| builtins::prim_load(m, prim, Key::Named(pa), r));
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
            Cons::LenIndex { lo, hi } => {
                // An index store's rule comes with its `Cons::Store`, which
                // records the event; `pop` and a generic permutation have
                // only this rule (`held_lengths`).
                let at = match self.len_index_sites.get(&n) {
                    _ if self.len_index_stores.contains(&n) => None,
                    Some(&site) => Some(site),
                    None => Some(u32::MAX),
                };
                for &t in toks {
                    if let Some(at) = at {
                        self.len_event_through(t, at, false, n);
                    }
                    self.arr_len_index(t, Iv { lo, hi, nz: false });
                }
            }
            Cons::LenPermuted { t } => self.arr_len_permuted_check(n, t),
            Cons::IndexHole => {
                for &t in toks {
                    self.mark_hole(t);
                }
            }
            Cons::IndexHoleIn { key } => {
                for &t in toks {
                    self.index_hole_in(t, key);
                }
            }
            Cons::IndexKept => {
                for &t in toks {
                    self.index_kept(t);
                }
            }
            Cons::OwnValues { dst } => {
                self.add_prim(dst, prim & prim::STR);
                for &t in toks {
                    self.own_values_of(t, dst);
                }
            }
            Cons::OwnKeys { dst } => {
                // A String's own keys are its index names: any String.
                if prim & prim::STR != 0 {
                    self.add_prim(dst, prim::STR);
                }
                for &t in toks {
                    self.own_keys_of(t, dst);
                }
            }
            Cons::OwnNames { dst } => {
                // A String's own names: its indices and `length`.
                if prim & prim::STR != 0 {
                    self.add_prim(dst, prim::STR);
                }
                for &t in toks {
                    self.own_names_of(t, dst);
                }
            }
            Cons::OwnDescValues { dst } => {
                // A String's own properties: its characters and `length`.
                if prim & prim::STR != 0 {
                    self.add_prim(dst, prim::STR | prim::NUM);
                }
                for &t in toks {
                    self.own_values_of(t, dst);
                    let fun = matches!(
                        self.toks[t as usize],
                        TokKind::Func(..)
                            | TokKind::ClassObj(_)
                            | TokKind::Bound(_)
                            | TokKind::BuiltinFn(_)
                    );
                    if fun {
                        // `name` (a String) and `length` (a Number).
                        self.add_prim(dst, prim::STR | prim::NUM);
                    }
                    if fun
                        || matches!(
                            self.toks[t as usize],
                            TokKind::ObjectProto | TokKind::Builtin(_)
                        )
                    {
                        self.load_from(t, t, Key::any(), dst);
                    }
                    // A RegExp's own data property `lastIndex` (22.2.3.1
                    // RegExpAlloc, a Number from birth; `regexp_last_index`)
                    // and whatever the program stored there.
                    let li = self.atom("lastIndex");
                    if self.regexp_last_index(t, li) {
                        self.add_prim(dst, prim::NUM);
                        let f = self.field_r(t, li);
                        self.edge(f, dst);
                    }
                }
            }
            Cons::Store {
                key,
                src,
                site,
                region,
            } => {
                self.store_at = Some(n);
                self.store_region = Some(region);
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.store_into(t, key, src, site);
                }
                self.store_at = None;
                self.store_region = None;
            }
            Cons::Call { call } => {
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.call_tok(call, t, None);
                }
            }
            Cons::CallThis { call, this } => {
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.call_tok(call, t, Some(this));
                }
            }
            Cons::New { call } => {
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.construct(call, t);
                }
            }
            Cons::NewProgram { call } => {
                for &t in toks {
                    self.construct_program(call, t);
                }
            }
            Cons::FromArrayLike { dst } => {
                for &t in toks {
                    self.from_array_like(t, dst);
                }
            }
            Cons::ReflectNew { call, new_target } => {
                for &target in toks {
                    self.cons(new_target, Cons::ReflectNewTarget { call, target });
                    // An unknown target may be any escaped class: each one,
                    // now or later, arrives here as an ordinary target.
                    if target == TOP && !self.sum.templates.contains(&call) {
                        let esc = self.escaped_classes;
                        self.cons(esc, Cons::ReflectNew { call, new_target });
                    }
                }
            }
            Cons::ReflectNewTarget { call, target } => {
                // HIR uses undefined for the omitted third argument, as the
                // runtime does: the target then also selects the allocation.
                if prim & prim::UNDEF != 0 {
                    self.construct(call, target);
                }
                for &new_target in toks {
                    self.reflect_construct(call, target, new_target);
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
                    self.with_native(call, |m, call| builtins::prim_method(m, prim, key, call));
                }
                for &t in toks {
                    self.prov_tok(pv, t);
                    self.method_on(t, key, call);
                }
            }
            Cons::Chain { origin, key, act } => {
                for &p in toks {
                    self.prov_tok(pv, p);
                    self.chain_step(n, origin, p, key, act);
                }
            }
            Cons::Raise { region } => {
                if toks.contains(&TOP) {
                    self.raise_top(
                        region,
                        n,
                        "unknown code may throw in the region (an operation on ⊤)",
                    );
                }
            }
            Cons::HandRaise { region } => {
                if !toks.is_empty() {
                    self.raise_top(
                        region,
                        n,
                        "a native hands a value to unknown code, which may throw",
                    );
                }
            }
            Cons::TopSink { reason } | Cons::RuntimeStore { reason, .. } => {
                let retained = if let Cons::RuntimeStore { retained, .. } = c {
                    Some(*retained)
                } else {
                    None
                };
                if self.rfam.top_held.contains(&n) {
                    let ts: Vec<Tok> = toks.into_iter().copied().collect();
                    self.rfam_top_held(&ts);
                }
                let chain = self.chain_sinks.contains(&n);
                let saved = std::mem::replace(&mut self.chain_escape, chain);
                let root = self.ledger.as_mut().map(|l| l.enter_sink(n));
                for &t in toks {
                    if retained != Some(t) {
                        self.escape(t, reason);
                    }
                }
                if let (Some(l), Some(r)) = (self.ledger.as_mut(), root) {
                    l.leave(r);
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
            Cons::CallerValue { dst, reason } => {
                self.add_prim(dst, prim);
                for &t in toks {
                    if self.is_marker(t) {
                        self.add_top(dst, |m| m.reasons[reason as usize].clone());
                        self.add_prim(dst, prim::ALL);
                    } else {
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
                    // A literal layout of this unit is handed over as one
                    // handed to a closed function is: the argument contract
                    // describes it key by key (`ctor_args_out`); it escapes
                    // once a census report broke its layout
                    // (`Gen::closed_ctor_literals`).
                    let lit = !stand_in
                        && matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                            if matches!(self.tmpls[tm as usize].kind,
                                TmplKind::Class { literal_shape: true, .. }));
                    if !stand_in && !arr && !lit {
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
                            self.set_owner(v, f);
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
                // The intrinsic buffer constructor a failed `instanceof`
                // narrows by (`Cons::NotBuffer`): any other value admits.
                let ctor = match test {
                    guard::NOT_BUFFER_CTOR => {
                        Some(self.tok(TokKind::Builtin(BuiltinNs::ArrayBuffer)))
                    }
                    guard::NOT_SHARED_BUFFER_CTOR => {
                        Some(self.tok(TokKind::Builtin(BuiltinNs::SharedArrayBuffer)))
                    }
                    guard::NOT_OBJECT_CTOR => Some(self.tok(TokKind::Builtin(BuiltinNs::Object))),
                    guard::NOT_ERROR_CTOR => Some(self.tok(TokKind::Builtin(BuiltinNs::Error))),
                    _ => None,
                };
                // A marker beside its values is no value (DECISIONS 74):
                // the test waits for them, and fires if the marker leaves
                // (75). A marker with none beside it holds anything.
                let admits = prim & guard::prims(test) != 0
                    || toks.iter().any(|&t| {
                        t == TOP
                            || if self.is_marker(t) {
                                !self.beside_values(t)
                            } else if let Some(c) = ctor {
                                t != c
                            } else if test == guard::NOT_CALLABLE {
                                !crate::builtins::surely_callable(self, t)
                            } else {
                                guard::objects(test)
                            }
                    });
                if admits {
                    self.edge(from, dst);
                } else {
                    for &t in toks {
                        if self.beside_markers.contains(&t) {
                            self.beside_guard.entry(t).or_default().push((from, dst));
                        }
                    }
                }
            }
            // Rule RP2: an ordinary Array the runtime takes the array arm
            // for is held back (`solve/record_entry.rs`).
            Cons::RecordEntry { dst } => {
                for &t in toks {
                    if self.record_entry_holds(t) {
                        if !self.record_entry_held.contains(&(t, dst)) {
                            self.record_entry_held.push((t, dst));
                        }
                    } else {
                        self.add_tok(dst, t);
                    }
                }
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
            }
            Cons::Defined { dst }
            | Cons::NotNullish { dst }
            | Cons::NotUndefined { dst }
            | Cons::NotNull { dst } => {
                let strip = match c {
                    Cons::Defined { .. } | Cons::NotUndefined { .. } => prim::UNDEF,
                    Cons::NotNull { .. } => prim::NULL,
                    _ => prim::NULL | prim::UNDEF,
                };
                for &t in toks {
                    self.add_tok(dst, t);
                }
                if prim & !strip != 0 {
                    // A filter of the host's values: its Numbers keep the
                    // host's interval, its strings the host's constants.
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim & !strip, strs);
                    self.cur_range = saved;
                }
            }
            // Rule T6: the primitives of `mask`; ⊤ and markers (values
            // of other units, which may be of that type) pass, no other
            // token (an object's typeof is "object" or "function").
            Cons::TypeIs { mask, dst } => {
                for &t in toks {
                    if t == TOP || self.is_marker(t) {
                        self.add_tok(dst, t);
                    }
                }
                if prim & mask != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim & mask, strs);
                    self.cur_range = saved;
                }
            }
            Cons::ProtoObjects { dst } => {
                for &t in toks {
                    self.add_tok(dst, t);
                }
                if prim & prim::NULL != 0 {
                    self.add_prim(dst, prim::NULL);
                }
            }
            Cons::NotBuffer { shared, dst } => {
                let kind = if shared {
                    "SharedArrayBuffer"
                } else {
                    "ArrayBuffer"
                };
                for &t in toks {
                    let buffer = match self.toks[t as usize] {
                        TokKind::Obj(tm, _) => matches!(
                            self.tmpls[tm as usize].kind,
                            TmplKind::BObj { what, .. } if what == kind
                        ),
                        _ => false,
                    };
                    if !buffer {
                        self.add_tok(dst, t);
                    }
                }
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
            }
            // Rule T7: every value but `%Object.prototype%`.
            Cons::NotObjectProto { dst } => {
                for &t in toks {
                    if !matches!(self.toks[t as usize], TokKind::ObjectProto) {
                        self.add_tok(dst, t);
                    }
                }
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
            }
            // Rule NOT-ERROR (`solve/not_error.rs`): every value but the
            // ones whose chain surely reaches `%Error.prototype%`; a dropped
            // token is held for `settle_not_error`.
            Cons::NotError { hook, dst } => {
                for &t in toks {
                    if self.not_error_drops(t, hook) {
                        if self.not_error_seen.insert((t, hook, dst)) {
                            self.not_error_held.push((t, hook, dst));
                        }
                    } else {
                        self.add_tok(dst, t);
                    }
                }
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
            }
            Cons::NotRegExp { dst } => {
                for &t in toks {
                    if !self.is_regexp_tok(t) {
                        self.add_tok(dst, t);
                    }
                }
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
            }
            Cons::FieldTruthy { key, test, dst } => {
                // Rule G2: `x` where `x.key` was truthy; rule G1: where
                // `x.key === b` (a Boolean constant) was decided. Reading `key` on
                // `null`/`undefined` throws, so neither reaches `dst`. Every
                // other primitive kind is judged like a token, per kind: its
                // read of `key` is GetV (7.3.2: ToObject, then the wrapper
                // prototype's chain; a String's own index and `length` keys),
                // the solver's primitive load (`builtins::prim_load`: builtin
                // members, program stores on the chain, a computed-key store,
                // replayed on a later store). The kind, with the host's
                // interval and constants, passes once that read may pass `test`.
                let strip = prim::NULL | prim::UNDEF;
                let mut kinds = prim & !strip;
                while kinds != 0 {
                    let k = kinds & kinds.wrapping_neg();
                    kinds &= !k;
                    let one = self.node_l("field-truthy-prim");
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(one, &[], k, strs);
                    self.cur_range = saved;
                    let read = self.node_l("field-truthy-prim-read");
                    self.cons(
                        read,
                        Cons::Guard {
                            test,
                            from: one,
                            dst,
                        },
                    );
                    self.cons(one, Cons::Load { key, dst: read });
                }
                for &t in toks {
                    if t == TOP || self.is_marker(t) {
                        self.add_tok(dst, t);
                        continue;
                    }
                    // The token's own read of `key` (the test's `Load`, on
                    // this token alone): it passes once that read may pass
                    // `test` (`guard::TRUTHY`: an object, function, marker,
                    // ⊤, or any primitive kind but `undefined`/`null`/`false`;
                    // `IS_FALSE`: the `false` kind, a marker or ⊤; ...).
                    // The read depends on (token, key) alone: one pair per
                    // (token, key) for every test and output
                    // (`Model::ft_reads`); each test adds its own guard,
                    // which `cons` replays on what the read already holds.
                    let (one, read, fresh) = match self.ft_reads.get(&(t, key)) {
                        Some(&(one, read)) => (one, read, false),
                        None => {
                            let one = self.node_l("field-truthy-token");
                            self.add_tok(one, t);
                            let read = self.node_l("field-truthy-read");
                            self.ft_reads.insert((t, key), (one, read));
                            (one, read, true)
                        }
                    };
                    self.cons(
                        read,
                        Cons::Guard {
                            test,
                            from: one,
                            dst,
                        },
                    );
                    if fresh {
                        // A rule, not a one-off lookup: a later store on the
                        // chain (an `Object.prototype` member) re-runs it.
                        self.cons(one, Cons::Load { key, dst: read });
                    }
                }
            }
            Cons::Objects { dst } => {
                // ToObject of an object is the object; a primitive's
                // wrapper is the caller's one `Guard` per kind (`to_object_into`).
                for &t in toks {
                    self.add_tok(dst, t);
                }
            }
            Cons::Fulfil { dst } => {
                // Nodes per (constraint, token); per-token read node is a planned follow-up (CWN).
                // A primitive has no `then` (27.2.1.3.2 step 8): it fulfils.
                if prim != 0 {
                    let strs = self.nodes[n as usize].strs;
                    let saved = self.cur_range.replace(self.rg(n));
                    self.add_s(dst, &[], prim, strs);
                    self.cur_range = saved;
                }
                for &t in toks {
                    if t == TOP || self.is_marker(t) {
                        self.add_tok(dst, t);
                        continue;
                    }
                    // The token's own read of `then` (steps 9-13): it fulfils
                    // as itself once that read may hold a value no call
                    // takes; a callable `then` adopts it (`builtins::adopt`).
                    let then_a = self.atom("then");
                    let then = Key::Named(then_a);
                    let one = self.node_l("fulfil-token");
                    self.add_tok(one, t);
                    let read = self.node_l("fulfil-then");
                    self.cons(
                        read,
                        Cons::Guard {
                            test: guard::NOT_CALLABLE,
                            from: one,
                            dst,
                        },
                    );
                    if crate::builtins::locked_chain_defines(self, t, then_a) {
                        // A native promise: its chain ends at a locked
                        // `then` (Promise.prototype.then), so the read is
                        // never the chain end's `undefined`; its own values
                        // (a stored `undefined` among them) count as they are.
                        let chain = self.node_l("fulfil-then-chain");
                        self.cons(chain, Cons::Defined { dst: read });
                        self.cons(
                            one,
                            Cons::Load {
                                key: then,
                                dst: chain,
                            },
                        );
                        let own = self.field_r(t, then_a);
                        self.edge(own, read);
                    } else {
                        self.cons(
                            one,
                            Cons::Load {
                                key: then,
                                dst: read,
                            },
                        );
                    }
                }
            }
            Cons::Falsy { dst } => {
                // ES ToBoolean: every object is truthy, so no object or
                // function token is the result; ⊤ is, as its falsy
                // primitives. A marker stands for a caller's value.
                let falsy = guard::prims(guard::FALSY);
                let mut p = prim & falsy;
                for &t in toks {
                    if t == TOP {
                        p |= falsy;
                    } else if self.is_marker(t) {
                        self.add_tok(dst, t);
                    }
                }
                // Rule KEY2c, ECMA-262 7.1.2 ToBoolean: the one falsy String
                // is "": a host of constants without "" gives no String,
                // any other host "" alone.
                let str_falsy = p & prim::STR != 0;
                p &= !prim::STR;
                if p != 0 {
                    // The host's falsy Numbers: within its interval unless ⊤
                    // or a marker adds falsy values of its own.
                    let own = !toks.iter().any(|&t| t == TOP || self.is_marker(t));
                    let host = own.then(|| self.rg(n));
                    let saved = std::mem::replace(&mut self.cur_range, host);
                    self.add_prim(dst, p);
                    self.cur_range = saved;
                }
                if str_falsy {
                    let x = &self.nodes[n as usize];
                    let plain = !x.pts.contains_tok(TOP) && x.strs >= STRS_SET0;
                    let may_empty = !plain
                        || self
                            .str_atoms(x.strs)
                            .iter()
                            .any(|&a| self.atom_is_mark(a) || self.atoms[a as usize].is_empty());
                    if may_empty {
                        let e = self.atom("");
                        let s = self.str_set(&[e]);
                        let s = self.run_born(s);
                        self.add_s(dst, &[], prim::STR, s);
                    }
                }
            }
            Cons::TopIfAny { dst, from, reason } => {
                if !toks.is_empty() || prim != 0 {
                    if let Some(r) = reason {
                        self.add_top(dst, |m| m.reasons[r as usize].clone());
                        self.add_prim(dst, prim::ALL);
                    }
                    if from != dst {
                        self.edge(from, dst);
                    }
                }
            }
            Cons::RequireSpec { call } => {
                // R-SPEC: each string constant resolves once, as that
                // literal does; any string, a token or ⊤ once as a value
                // no constant spells.
                let strs = self.nodes[n as usize].strs;
                let any_str = prim & prim::STR != 0 && self.strs_any(strs);
                if prim & prim::STR != 0 && !any_str {
                    let atoms = self.str_atoms(strs).to_vec();
                    for a in atoms {
                        if self.require_spec_done.insert((call, Some(a))) {
                            self.with_b(|m| builtins::require_by_spec(m, call, Some(a)));
                        }
                    }
                }
                if (any_str || !toks.is_empty()) && self.require_spec_done.insert((call, None)) {
                    self.with_b(|m| builtins::require_by_spec(m, call, None));
                }
            }
            Cons::TopIfTop { dst, reason } => {
                if toks.contains(&TOP) {
                    self.add_top(dst, |m| m.reasons[reason as usize].clone());
                    self.add_prim(dst, prim::ALL);
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
                        builtins::note_native_data_write(
                            self,
                            TOP,
                            k,
                            "E7 delete",
                            builtins::WriteValue::Absent,
                        );
                        self.top_reshape(Some(key));
                        let k = self.atoms[key as usize].clone();
                        self.note_shared(SharedWrite::Removed(Some(k)));
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
                    // DECISIONS 242 (contract half): a defineProperty under
                    // a computed key on another unit's argument: the caller
                    // runs it there (`FnContract::param_defines`).
                    if kind == Hard::E5DefineProperty && self.is_marker(t) {
                        self.note_marker_define(t, None, false);
                    }
                    self.mark_hard(t, kind, reason);
                }
            }
            Cons::Miss { key } => {
                for &t in toks {
                    if t != TOP {
                        self.note_chain_miss(t, key);
                    }
                }
            }
            Cons::DefineKey { key, data, reason } => {
                for &t in toks {
                    // Defining an index or `length` may leave a hole (`hole`).
                    let k = &self.atoms[key as usize];
                    if crate::solve::numeric_key(k) || k == "length" {
                        self.mark_hole(t);
                        self.len_event_through(t, u32::MAX, false, NO_NODE);
                        self.arr_len_top(t);
                    }
                    if self.is_marker(t) {
                        self.note_inst(t, true);
                        self.note_marker_define(t, Some(key), data);
                    }
                    // A key the layout already holds as data may move into an
                    // accessor pair in its slot, and a class instance's layout
                    // is the class's: E5 HARD, as any defineProperty was. (On
                    // another unit's layout the accessor is beyond its contract
                    // in any case: the census breaks it.)
                    match self.toks[t as usize] {
                        //
                        // Rule D2: a DATA descriptor under a named key the
                        // template neither holds at birth nor has on its
                        // prototype (ECMA-262 20.1.2.4 -> 7.3.8 -> 10.1.6.3
                        // step 2: `current` undefined) creates that key
                        // alone, with the descriptor's value (stored by
                        // `define_by_data`) and attributes, on a class
                        // instance or a literal: a key added after birth
                        // (`adds_keys`: no exact key list, no fixed layout),
                        // no other own key created, changed or removed, no
                        // accessor installed. Reads and stores of the key
                        // itself see an accessor atom (`add_accessor_atom`),
                        // never a plain slot (`defined_keys`, key-scoped);
                        // the key does not enter `accessor_keys`, which
                        // would refuse every key. A
                        // non-extensible target makes the define throw
                        // (10.1.6.3 step 2.a) and creates nothing.
                        TokKind::Obj(tm, _) => {
                            let tpl = &self.tmpls[tm as usize];
                            let birth = tpl.birth_keys.contains(&key);
                            let new_key = !birth
                                && if data {
                                    matches!(
                                        tpl.kind,
                                        TmplKind::Lit { .. } | TmplKind::Class { .. }
                                    ) && !tpl.proto_members.contains(&key)
                                } else {
                                    matches!(
                                        tpl.kind,
                                        TmplKind::Lit { .. }
                                            | TmplKind::Class {
                                                literal_shape: true,
                                                ..
                                            }
                                    )
                                };
                            if new_key {
                                *self.adds_keys.entry(tm).or_default() += 1;
                                if data {
                                    self.virt_revoke(t, Some(key));
                                    let ks = self.defined_keys.entry(tm).or_default();
                                    if !ks.contains(&key) {
                                        ks.push(key);
                                    }
                                }
                            } else if data {
                                self.mark_hard_as(t, Hard::E5DefineProperty, reason, false);
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
                        // Defining NAMED `key` on a realm prototype
                        // (`builtins::realm_proto_value`) writes that key of
                        // that one object and no other (rule D1): the
                        // lookups passing it see the write under its
                        // identity (`intrinsic_miss_on`); its get/set are
                        // installed by `Cons::Accessor`.
                        _ if builtins::realm_proto_of(self, t).is_some() => {
                            let k = Some(Key::Named(key));
                            builtins::note_native_data_write(
                                self,
                                t,
                                k,
                                "E5 Object.defineProperty",
                                builtins::WriteValue::Opaque,
                            );
                            self.virt_revoke(t, Some(key));
                            self.reshape(Some(key));
                        }
                        // Defining `key` on an unknown object writes that key
                        // of whatever it is, a builtin prototype included,
                        // and no other key.
                        _ if t == TOP => {
                            let k = Some(Key::Named(key));
                            self.note_fnproto_write(TOP, k, Hard::E5DefineProperty.code());
                            // The definition installs exactly what its
                            // descriptor says (ECMA-262 6.2.6.5, 10.1.6.3):
                            // its value as data here, its get/set functions
                            // by the producer's `Cons::Accessor` (an unknown
                            // one as an unknown accessor: `AccessorInstall`).
                            builtins::note_native_data_write(
                                self,
                                TOP,
                                k,
                                "E5 Object.defineProperty",
                                builtins::WriteValue::Opaque,
                            );
                            self.reshape(Some(key));
                            // DECISIONS 234: in every unit, as an accessor
                            // through ⊤ under the key (a superset: what a
                            // descriptor installs, a getter included).
                            self.top_reshape(Some(key));
                            let k = self.atoms[key as usize].clone();
                            self.note_shared(SharedWrite::Accessor(Shared::Top, k.clone()));
                            self.note_shared(SharedWrite::Removed(Some(k)));
                        }
                        // A NAMED key on `Object.prototype` defines that key
                        // there and no other (node: `Object.defineProperty(
                        // Object.prototype, "k", d)`): an accessor key of
                        // every chain through it, as on a class prototype.
                        // DECISIONS 242(1) (an accessor under any key) is a
                        // computed key: `Cons::Hard` E5 (`mark_hard`).
                        TokKind::ObjectProto => {
                            let k = Some(Key::Named(key));
                            builtins::note_native_write(self, t, k, "E5 Object.defineProperty");
                            self.virt_revoke(t, Some(key));
                            self.reshape(Some(key));
                            if let Some(s) = self.shared_of(t) {
                                let name = self.atoms[key as usize].clone();
                                self.note_shared(SharedWrite::Accessor(s, name));
                            }
                            let ks = self.accessor_keys.entry(t).or_default();
                            if !ks.contains(&key) {
                                ks.push(key);
                            }
                        }
                        // A NAMED key on the global object (9.1.1.4: an
                        // ordinary object whose own properties are the
                        // global bindings) creates or replaces that one own
                        // property (20.1.2.4 -> 10.1.6.3) and writes no other
                        // key: a data descriptor's value is the named store
                        // `define_by_data` emits (the binding's global field,
                        // `global_field`), an accessor's get / set are
                        // installed under that key by `Cons::Accessor`. No
                        // any-key write of the global object.
                        _ if builtins::global_object(self, t) && !self.unit_mode => {
                            self.virt_revoke(t, Some(key));
                        }
                        // A data descriptor under this NAMED key writes that
                        // key alone (rule D1): HARD, no unknown write of any
                        // other field (`mark_hard_as`).
                        _ => self.mark_hard_as(t, Hard::E5DefineProperty, reason, !data),
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
            Cons::Implicit {
                reason,
                proto,
                region,
            } => {
                for &t in toks {
                    // The uncaught report of a native Error (perry-runtime
                    // exception.rs `print_uncaught`: `ptr_is_native_error`,
                    // then `uncaught_native_error_report`) reads the stored
                    // name, message and stack and returns: no property
                    // lookup, no ToPrimitive, no program code. Every other
                    // value and every other ToPrimitive keeps the request.
                    if proto == Protocol::ToPrimitive
                        && n == self.host_report
                        && self.native_error_tok(t)
                    {
                        continue;
                    }
                    // JSON.stringify reads `toJSON` and the values itself
                    // (json/stringify.rs: native_get, native_call_value_this):
                    // the request is no route; each call it makes is bound
                    // by `Model::implicit` and keeps its callee's.
                    if proto != Protocol::Json {
                        crate::family_reach::dispatch_route(self, |m| {
                            format!("the runtime's {proto:?} protocol on {}", m.tok_name(t))
                        });
                    }
                    match self.toks[t as usize] {
                        // A function handed to a reflecting runtime helper:
                        // it may be called back with anything.
                        TokKind::Func(_, _) if proto == Protocol::Any => self.escape(t, reason),
                        // A bound function: the call runs its target's
                        // [[Call]] (`Cons::BoundCall` on the target node);
                        // its other keys are the touch of the bound value.
                        TokKind::Bound(b) if proto == Protocol::Any => {
                            let target = self.bound[b as usize].0;
                            self.bound_call(target, t, reason);
                            self.implicit(t, reason, proto, region);
                        }
                        // `instanceof` a bound function runs its target's.
                        TokKind::Bound(_) if proto == Protocol::HasInstance => {
                            self.escape(t, reason)
                        }
                        _ => self.implicit(t, reason, proto, region),
                    }
                }
            }
            Cons::BoundCall { reason, bound } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        // 10.2.1 step 2: a class constructor called
                        // without `new` throws before its body runs.
                        TokKind::ClassObj(tm)
                            if matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. }) => {}
                        // 10.4.1.1: a bound target forwards the call.
                        TokKind::Bound(b) => {
                            let target = self.bound[b as usize].0;
                            self.bound_call(target, bound, reason);
                        }
                        _ => self.escape(bound, reason),
                    }
                }
            }
            Cons::HasInstance { reason, arg } => {
                for &t in toks {
                    self.has_instance(t, reason, arg);
                }
            }
            // Rule PXASSIGN: on a Proxy target the copy's [[Set]] of some
            // key is the Proxy's (10.5.9, `store_into`), once a value of an
            // unnamed key is there.
            Cons::UnnamedSet { target, site }
                if matches!(self.toks[target as usize], TokKind::Proxy(_)) =>
            {
                if !toks.is_empty() || prim != 0 {
                    self.store_into(target, Key::any(), n, site);
                }
            }
            Cons::UnnamedSet { target, .. } => {
                let walked = self
                    .walk_nodes
                    .contains_key(&(target, target, Key::any(), 1));
                if !walked && (!toks.is_empty() || prim != 0) {
                    self.accessor_walk(target, target, Key::any(), ChainAct::Store(n, self.thrown));
                    self.note_unnamed_set(target);
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
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            } => {
                for &q in toks {
                    self.prov_tok(pv, q);
                    if !builtin && self.is_builtin_proto(q) {
                        continue;
                    }
                    if q == TOP {
                        self.add_tok(dst, TOP);
                        if self.is_constructor_key(key) {
                            let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                            self.add_tok(dst, f);
                        }
                        continue;
                    }
                    if self.is_array_length(q, key) {
                        self.arr_len_into(q, dst);
                        continue;
                    }
                    // The same final-read decision as chain_load: the
                    // collapsed token also holds stores onto add-open
                    // prototypes, which this receiver's chain never sees.
                    if let Key::Named(a) = key {
                        if matches!(self.toks[q as usize], TokKind::ObjectProto)
                            && builtins::locked_chain_read(self, of, a)
                        {
                            self.with_b(|m| builtins::chain_load(m, of, q, key, dst));
                            continue;
                        }
                    }
                    let f = match key {
                        Key::Named(a) => self.field_r(q, a),
                        Key::Any(_) => self.key_fields(q, key),
                    };
                    self.edge(f, dst);
                    for x in self.class_id_holders(of, q) {
                        let f = match key {
                            Key::Named(a) => self.field_r(x, a),
                            Key::Any(_) => self.key_fields(x, key),
                        };
                        self.edge(f, dst);
                    }
                    if self.json_key_optimistic(of, key) {
                        // OPTIMISTIC (`json_key_optimistic`): no built-in
                        // member of the link, only what a store into ⊤ may
                        // have written there.
                        self.with_b(|m| builtins::chain_written_only(m, q, dst));
                    } else {
                        self.with_b(|m| builtins::chain_load(m, of, q, key, dst));
                    }
                    if self.stops_at(q, key) {
                        let v = self.stopped_shared.entry(q).or_default();
                        v.push((key, dst, builtin));
                    } else {
                        let up = self.cv_up(q, key, builtin);
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
                    // A realm prototype's setters run for a computed-key
                    // store the key may name (`realm_setter_hit`).
                    let realm_store = matches!(act, ChainAct::Store(..))
                        && builtins::realm_proto_of(self, q).is_some();
                    if q != TOP
                        && (realm_store
                            || self.accessor_keys.get(&q).is_some_and(|k| !k.is_empty()))
                    {
                        self.accessor_hit(origin, q, Key::any(), act);
                    }
                }
            }
            Cons::ProtoMethods { origin, key, call } => {
                for &q in toks {
                    if matches!(
                        self.toks[q as usize],
                        TokKind::ObjectProto | TokKind::NativeProto(..)
                    ) {
                        self.with_native(call, |m, call| {
                            builtins::proto_method(m, q, origin, key, call)
                        });
                    }
                }
            }
            Cons::StoreTopProto { origin, src } => {
                if toks.contains(&TOP) {
                    if let Some(d) = &mut self.diagnostic {
                        if let Some(pn) = d.cause_node {
                            d.store_top.entry(src).or_insert(pn);
                        }
                    }
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
            Cons::SuperFrom { recv, key, act } => {
                for &start in toks {
                    self.cons(recv, Cons::Super { start, key, act });
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
                        self.with_native(call, |m, call| {
                            crate::builtins::native_parent_ctor(m, class, call, t)
                        });
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
                // The realm's constructor the built-in parent names runs as
                // the native summary (`Cons::SuperCtor`); any other value of
                // the class's [[Prototype]] runs as itself.
                let native = crate::builtins::native_parent_value(self, class);
                for &parent in toks {
                    match self.toks[parent as usize] {
                        _ if Some(parent) == native => {}
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
                        // ECMA-262 13.3.7.1 SuperCall step 6: Construct(func,
                        // argList, NewTarget): a realm constructor runs its
                        // [[Construct]] (the Promise executor, 27.2.3.1 step
                        // 9), never its [[Call]] (which throws for Promise,
                        // 27.2.3.1 step 1). The object it makes has
                        // NewTarget's prototype, which no builtin model
                        // gives: the model's object stays apart from the
                        // instance (the result is ⊤) and leaves, with what
                        // the construction put into it.
                        // A realm constructor that declares what it does
                        // (ECMA-262 10.1.13 OrdinaryCreateFromConstructor):
                        // its declaration runs on the instance under
                        // construction, whose prototype chain is NewTarget's.
                        TokKind::BuiltinFn(Native::Member { identity, .. })
                            if origin != TOP
                                && !self.unit_mode
                                && builtins::declared_super_construct(identity).is_some() =>
                        {
                            let e = builtins::declared_super_construct(identity).expect("guarded");
                            self.with_native(call, |m, call| {
                                builtins::run_super_construct(m, e, call, origin)
                            });
                        }
                        TokKind::BuiltinFn(_) | TokKind::Builtin(_) => {
                            if origin != TOP {
                                let what = match &self.toks[parent as usize] {
                                    TokKind::BuiltinFn(Native::Member { identity, .. }) => {
                                        identity.to_string()
                                    }
                                    _ => self.tok_name(parent),
                                };
                                let r = self.reason(&format!(
                                    "E21 realm parent constructor {what} declares no construct effects"
                                ));
                                self.mark_hard(origin, Hard::E21NativeParent, r);
                            }
                            let c = self.calls[call as usize].clone();
                            let made = self.node_l("super-builtin-construct");
                            let key = (crate::summary::MK_SUPER_BUILTIN, parent, 0);
                            if let Some(call2) = self.solver_call_keyed(
                                call,
                                key,
                                None,
                                CallSite {
                                    ret: made,
                                    ..c.clone()
                                },
                            ) {
                                self.construct(call2, parent);
                                let r = self.reason(
                                    "the object a builtin parent constructor makes for super()",
                                );
                                self.sink_node(made, r);
                            }
                            self.add_tok(c.ret, TOP);
                        }
                        TokKind::Top => {
                            self.call_tok(call, parent, Some(origin));
                            let ret = self.calls[call as usize].ret;
                            self.add_tok(ret, TOP);
                        }
                        _ => {}
                    }
                }
            }
            Cons::BirthField { key, src, tm } => {
                for &t in toks {
                    // Only the literal's own tokens: the holder node may
                    // also hold values from an outer source.
                    if !matches!(self.toks[t as usize], TokKind::Obj(b, _) if b == tm) {
                        continue;
                    }
                    let f = match key {
                        Key::Named(a) => self.field(t, a),
                        // A birth's `Key::Any` is the literal's elements
                        // (summary/build.rs): the element slot, never an
                        // array's named-key node.
                        Key::Any(_) => self.field_any(t),
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
                    if matches!(self.toks[t as usize], TokKind::Func(..))
                        || builtins::accessor_callable(self, t)
                    {
                        self.cons(
                            holder,
                            Cons::AccessorInstall {
                                key,
                                f: t,
                                setter,
                                from: n,
                            },
                        );
                    }
                }
            }
            Cons::AnnexBDefine { call, setter } => {
                for &t in toks {
                    self.with_native(call, |m, call| builtins::annexb_define(m, t, call, setter));
                }
            }
            Cons::AnnexBLookup { call, setter } => {
                for &t in toks {
                    self.with_native(call, |m, call| {
                        builtins::annexb_lookup(m, Some(t), call, setter)
                    });
                }
                if prim & !(prim::NULL | prim::UNDEF) != 0 {
                    self.with_native(call, |m, call| {
                        builtins::annexb_lookup_prim(m, prim, call, setter)
                    });
                }
            }
            Cons::AccessorInstall {
                key,
                f,
                setter,
                from,
            } => {
                let star = self.atom(crate::STAR_KEY);
                for &t in toks {
                    if f == TOP {
                        // An unknown function value is an opaque accessor:
                        // it shares the holder/key's existing accessor
                        // result. Repeated installs do not allocate another
                        // unknown value for the same accessor group. A known
                        // class, builtin or bound function is installed as a
                        // program function is, and runs as a call of that
                        // value (`group_call`).
                        let group = Key::Named(key);
                        self.acc_group(t, group);
                        let fa = self.acc_groups[&(t, group)].ar;
                        // Census only: the accessor's ⊤ and the holder's
                        // sink propagate the ⊤ its function value holds, so
                        // they are attributed to the node that held it, not
                        // to the holder whose rule runs.
                        let saved = match self.diagnostic.as_mut() {
                            Some(dg) if from != NO_NODE => Some(dg.cause_node.replace(from)),
                            _ => None,
                        };
                        self.add_top(fa, |_| "accessor of unknown function value".into());
                        if t == TOP {
                            // An unknown function installed on whatever ⊤
                            // is, a builtin prototype included.
                            builtins::note_native_write(
                                self,
                                TOP,
                                Some(Key::Named(key)),
                                "accessor of unknown function value",
                            );
                        }
                        let site = self.anon_post_site("unknown accessor");
                        let k = if self.atoms[key as usize] == crate::STAR_KEY {
                            Key::any()
                        } else {
                            group
                        };
                        self.cons(
                            n,
                            Cons::Store {
                                key: k,
                                src: fa,
                                site,
                                region: self.thrown,
                            },
                        );
                        let reason = self.reason("accessor of unknown function value");
                        self.cons(n, Cons::TopSink { reason });
                        if let (Some(dg), Some(saved)) = (self.diagnostic.as_mut(), saved) {
                            dg.cause_node = saved;
                        }
                        continue;
                    }
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
                    self.note_array_member_write(t, Some(Key::Named(key)), None, "accessor");
                    let realm_proto = builtins::realm_proto_of(self, t).is_some();
                    if t == TOP || realm_proto {
                        // A known function: an accessor query that may
                        // reach the install returns it (10.1.6.3).
                        builtins::note_native_accessor(self, t, Some(Key::Named(key)), f, setter);
                    } else {
                        builtins::note_native_write(self, t, Some(Key::Named(key)), "accessor");
                    }
                    if t == TOP || self.is_chain_proto(t) {
                        self.reshape(Some(key));
                    }
                    if t == TOP {
                        // An accessor installed on a value from unmodeled
                        // code (a native object, a built-in prototype the
                        // model gives as ⊤) is that code's to run, as a
                        // value stored there is (`store_into`): every read
                        // of the key there, by compiled code or by the
                        // runtime, may call it with any receiver.
                        let r = self.reason("accessor installed on a value from unmodeled code");
                        self.escape(f, r);
                        continue;
                    }
                    if realm_proto {
                        // On a realm prototype the runtime's own reads of
                        // the objects whose chain passes it (an emitter's,
                        // a stream's) may run it with such a receiver: it
                        // is the runtime's to call. The program lookups
                        // that walk the prototype run it too (`accessors`).
                        let r = self.reason("accessor installed on a realm prototype");
                        self.escape(f, r);
                    }
                    if key == star && !builtins::accessor_receiver(self, t, key) {
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
                        self.rfam_holding(f, t, crate::solve::recv_family::NO_HOME_KEY);
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
                    self.rfam_holding(f, t, key);
                    let ks = self.accessor_keys.entry(t).or_default();
                    if !ks.contains(&key) {
                        ks.push(key);
                        // ENUM3: an accessor a literal or a definition
                        // installs may be enumerable (not a class body's:
                        // `gen.rs` puts those into `accessor_keys` itself).
                        let n = self.field_r(t, key);
                        self.enum_key_mark(n, t, key);
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
                        self.note_objproto_accessor();
                        // `ObjectProto` stands for every built-in prototype:
                        // the lookups of a primitive (`prim_load`) and of an
                        // unknown value (⊤) end there too, and no walk runs
                        // for them. Any of them may call the accessor, with
                        // any receiver.
                        if self.reach_proto_accessors {
                            let r = self.reason("accessor on a built-in prototype");
                            self.escape(f, r);
                        }
                    }
                    self.accessor_installed(t, key, f, setter, !objproto);
                }
            }
            Cons::ProtoFrom { src } => {
                for &t in toks {
                    self.given_prototype(t, src);
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
                            ChainAct::Load(dst, _) | ChainAct::Lookup { dst, .. } => {
                                self.add_tok(dst, TOP)
                            }
                            ChainAct::Call(call) => self.call_tok(call, TOP, Some(origin)),
                            ChainAct::Store(_, _) => {}
                        },
                        TokKind::ObjectProto
                        | TokKind::NativeProto(..)
                        | TokKind::Builtin(_)
                        | TokKind::BuiltinFn(_) => {
                            self.with_b(|m| builtins::plain_key(m, origin, act));
                        }
                        _ => {
                            let f = self.num_fields(q);
                            match act {
                                ChainAct::Load(dst, _) => self.edge(f, dst),
                                ChainAct::Call(call) => self.bind_via(f, call, origin),
                                ChainAct::Store(_, _) | ChainAct::Lookup { .. } => {}
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
                        // A realm value is not its own iterator here: GetIterator
                        // (ECMA-262 7.4.3) makes the record's [[Iterator]] the
                        // result of `Call(v[@@iterator], v)`, and its `next` is
                        // read off that result, which the method calls of
                        // `@@iterator` and of `next` on its result step (a realm
                        // `@@iterator` that returns `this`, a written one).
                        // Absent (Object.prototype, a function), GetIterator
                        // throws TypeError before any `next` is read (7.4.3,
                        // 7.3.14 Call step 2); Array.prototype's is `values`, a
                        // fresh ArrayIterator (23.1.3.40). Read off `v` itself,
                        // `next` would be the realm's, called on a receiver with
                        // no iterator slots (it throws, 23.1.5.2.1, 27.5.3.3),
                        // yet stepped as an opaque call that escapes `v`.
                        TokKind::ObjectProto
                        | TokKind::NativeProto(..)
                        | TokKind::Builtin(_)
                        | TokKind::BuiltinFn(_) => {}
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
            Cons::IdxLoad {
                idx,
                dst,
                shape,
                enumerable,
                kept,
            } => {
                for &t in toks {
                    self.idx_load_tok(idx, t, dst, shape, enumerable, kept);
                }
                // A primitive receiver (7.3.2 GetV): ToObject, then the read
                // under every key the key may be (the rule of `Cons::Load`):
                // a String's index keys are its one-character Strings, its
                // `length` a Number (10.4.3.5), any other key a member of
                // the prototype chain of the kind. A for-in run's key is
                // that one name; any other key is decided by its values
                // (`Cons::IdxKey` with `obj` ⊤, rule NUM-KEY-PRIM). Under
                // F-A (`enumerable`) the key is an enumerable key of the
                // ToObject'ed subject (`enum_read_prim`).
                if prim != 0 && enumerable {
                    let key = match self.key_nodes.get(&idx) {
                        Some(&a) => Key::Named(a),
                        None => Key::Any(shape),
                    };
                    self.enum_read_prim(idx, prim, key, dst);
                } else if prim != 0 {
                    match self.key_nodes.get(&idx) {
                        Some(&a) => {
                            self.with_b(|m| builtins::prim_load(m, prim, Key::Named(a), dst))
                        }
                        None => {
                            let p = prim & !(prim::NULL | prim::UNDEF);
                            if p != 0
                                && self.idx_done.insert((
                                    idx,
                                    TOP,
                                    dst,
                                    crate::model::IDX_PRIM_KEY | p,
                                ))
                            {
                                self.cons(
                                    idx,
                                    Cons::IdxKey {
                                        obj: TOP,
                                        origin: TOP,
                                        prim: p,
                                        dst,
                                        shape,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            Cons::EnumLoad { key, dst } => {
                for &t in toks {
                    if t != TOP
                        && (self.enum_free(t)
                            || self.enum_obj(t)
                            || builtins::enum_free_instance(self, t)
                            // SKINROOT20: a built-in instance whose
                            // enumerable own properties are the program's
                            // (plus a typed array's index elements, never
                            // "__proto__") and whose built-in chain holds no
                            // enumerable built-in property, the
                            // `Object.prototype.__proto__` accessor among
                            // them (clause 18, 20.1.3.8, 10.4.5.6): for-in
                            // yields "__proto__" only from a property the
                            // program made enumerable, so the read is the
                            // enumerable read, never the accessor.
                            || builtins::enum_free_link_obj(self, t))
                    {
                        self.enum_read_key(n, t, key, dst);
                    } else {
                        self.load_from(t, t, key, dst);
                    }
                }
            }
            Cons::EnumChain { key, dst, of, from } => {
                for &q in toks {
                    if q == TOP {
                        self.add_tok(dst, TOP);
                        continue;
                    }
                    // A prototype with enumerable built-in members (a host
                    // one, or one the seed of `from` does not name): its
                    // keys in full.
                    if !self.enum_link(from, q) {
                        self.load_from(of, q, key, dst);
                        continue;
                    }
                    // SKINROOT7: a typed-array link's enumerable own
                    // properties are its elements (10.4.5.1): a Number or
                    // BigInt under an index key.
                    if self.typed_array_elements(q)
                        && match key {
                            Key::Any(_) => true,
                            Key::Named(a) => {
                                let s = self.atoms[a as usize].as_str();
                                !s.is_empty()
                                    && s.len() <= 10
                                    && s.bytes().all(|b| b.is_ascii_digit())
                                    && (s == "0" || !s.starts_with('0'))
                            }
                        }
                    {
                        let p = self.typed_array_elem_prim(q);
                        self.add_prim(dst, p);
                    }
                    self.enum_fields(of, q, key, dst);
                    if self
                        .idx_done
                        .insert((n, q, dst, crate::model::IDX_ENUM_CHAIN))
                    {
                        self.accessor_walk(of, q, key, ChainAct::Load(dst, self.thrown));
                        let up = self.proto_node(q);
                        let link = Cons::EnumChain {
                            key,
                            dst,
                            of,
                            from: q,
                        };
                        // SKINROOT27: one prototype level per queue entry, not
                        // per Rust frame group (55k levels overflowed r8).
                        if self.chain_active {
                            self.chain_defer = true;
                            self.cons(up, link);
                        } else {
                            self.chain_active = true;
                            self.cons(up, link);
                            while let Some((n2, c2, i2)) = self.chain_q.pop() {
                                if !self.aborted {
                                    self.replay_cons(n2, c2, i2);
                                }
                            }
                            self.chain_active = false;
                        }
                    }
                }
            }
            Cons::ForInAcc { key, setter, probe } => {
                for &t in toks {
                    if t == TOP {
                        self.add_tok(probe, TOP);
                        continue;
                    }
                    self.accessor_walk(t, t, key, ChainAct::Lookup { dst: probe, setter });
                }
            }
            Cons::OwnDescAcc { setter, dst } => {
                for &t in toks {
                    if t == TOP {
                        // The object is unknown: so are its accessors.
                        self.add_tok(dst, TOP);
                        continue;
                    }
                    self.own_accessor_lookup(t, Key::any(), dst, setter);
                }
            }
            Cons::ForInMeet { other, src, gate } => {
                let hit = toks.iter().any(|&t| {
                    t == TOP
                        || if other == NO_NODE {
                            !matches!(self.toks[t as usize], TokKind::BuiltinFn(..))
                        } else {
                            let p = &self.nodes[other as usize].pts;
                            p.contains_tok(t) || p.contains_tok(TOP)
                        }
                });
                if hit && !self.nodes[src as usize].succ.contains(&gate) {
                    if std::env::var_os("PERRY_FACTS_FORIN_PATH_COUNT").is_some() {
                        eprintln!("forin-path-open: gate {gate} at node {n}");
                    }
                    self.edge(src, gate);
                }
            }
            Cons::ElemUndef { obj, key, dst } => self.elem_undef(obj, key, dst),
            Cons::IdxOwnLoad {
                idx,
                dst,
                shape,
                enumerable,
            } => {
                for &t in toks {
                    self.idx_own_load_tok(idx, t, dst, shape, enumerable);
                }
                // A primitive receiver: the own properties of its wrapper
                // (10.4.3.5): a String's index keys (one-character Strings)
                // and `length` (a Number); no own property else.
                if prim != 0 {
                    let key = self.prim_index_key(idx, shape);
                    self.prim_own_load(prim, key, dst);
                }
            }
            Cons::IdxOwnKey { obj, dst, shape } => self.idx_own_key(n, obj, dst, shape, toks, prim),
            // Refutation markers have their own replay path, not value deltas.
            Cons::OwnGate { .. } => {}
            Cons::OwnTest { dst, at } => self.own_test(dst, at, toks),
            Cons::IdxKey {
                obj: TOP,
                prim: recv,
                dst,
                shape,
                ..
            } => {
                // Rule NUM-KEY-PRIM (ECMA-262 7.3.2 GetV, 7.1.19
                // ToPropertyKey, 6.1.6.1.20 Number::toString): `p[k]` of a
                // primitive `p`. A String, Symbol or object key (⊤ among
                // them) may spell any name: the read under the site's shape.
                // A Number or BigInt key is ToString of one, a numeric
                // name ("12", "-0.5", "1e+21", "NaN", "Infinity"), which no
                // member of an intrinsic prototype has: a String's
                // one-character Strings or `undefined`, and only a number
                // key a store into ⊤ wrote on the prototype (`NUM_KEY`,
                // as `Cons::IdxMCall`'s `prim_plain_key`). `true`, `false`,
                // `null`, `undefined` are their names (rule K8).
                if index_needs_general_key(toks, prim)
                    && self
                        .idx_done
                        .insert((n, TOP, dst, crate::model::IDX_PRIM_GENERAL | recv))
                {
                    self.with_b(|m| builtins::prim_load(m, recv, Key::Any(shape), dst));
                }
                if index_has_numeric_key(prim)
                    && self
                        .idx_done
                        .insert((n, TOP, dst, crate::model::IDX_PRIM_NUM | recv))
                {
                    // A String's index is one of its characters (10.4.3.5).
                    if recv & prim::STR != 0 {
                        self.add_prim(dst, prim::STR | prim::UNDEF);
                    }
                    let num = self.atom(crate::model::NUM_KEY);
                    self.with_b(|m| builtins::prim_load(m, recv, Key::Named(num), dst));
                }
                for (bit, keys) in [
                    (prim::BOOL, &["true", "false"][..]),
                    (prim::NULL, &["null"][..]),
                    (prim::UNDEF, &["undefined"][..]),
                ] {
                    if prim & bit != 0 {
                        for k in keys {
                            let a = self.atom(k);
                            self.with_b(|m| builtins::prim_load(m, recv, Key::Named(a), dst));
                        }
                    }
                }
            }
            Cons::IdxKey {
                obj,
                origin,
                dst,
                shape,
                ..
            } => {
                let named = index_needs_general_key(toks, prim);
                // Rule M3: a key of string constants and known prefixes
                // reads, per entry, the names that entry admits; any other
                // key reads under the site's shape.
                // ECMA-262 13.3.2.1, 7.1.19: `o[k]` of a String constant
                // `k` is the named [[Get]] of `k` (OrdinaryGet 10.1.8.1):
                // the named read, which decides presence (`load_from`). A
                // non-Array object read under such a key got no `undefined`
                // up front (`idx_load_tok`); once the key holds anything
                // else, it does. Rule TA-LEN: a Number key of a typed array
                // whose length rule watches the read names an element or
                // no property at all (10.4.5.4); its `Cons::ElemUndef`
                // decides `undefined`. A key that grows past a Number runs
                // this again.
                let ta_floor = toks.is_empty()
                    && prim == prim::NUM
                    && self.typed_array_number_key(obj, prim, named)
                    && self.idx_done.contains(&(n, obj, dst, 0))
                    && self.elem_undef_watched(obj);
                if !self.is_arr(obj) && !self.const_str_key(n) && !ta_floor {
                    self.add_prim(dst, prim::UNDEF);
                }
                match named.then(|| self.key_entry_shapes(n)).flatten() {
                    Some(entries) => {
                        for (a, s) in entries {
                            if self.idx_atom_done.insert((n, obj, dst, a)) {
                                let key = match self.atom_is_mark(a) {
                                    true => Key::Any(s),
                                    // Rule NUM-CONST-KEY: on an array-like
                                    // receiver (an Array 10.4.2, arguments,
                                    // a typed array 10.4.5, a builtin with
                                    // internal entries) a constant that
                                    // spells a Number key reads as that
                                    // Number does: the element read.
                                    false
                                        if self.is_array_like(obj)
                                            && crate::solve::numeric_key(
                                                &self.atoms[a as usize],
                                            ) =>
                                    {
                                        self.elem_key()
                                    }
                                    // An ordinary object's "1" is its named
                                    // property "1" (10.1.8.1 OrdinaryGet).
                                    false => Key::Named(a),
                                };
                                self.load_from(origin, obj, key, dst);
                            }
                        }
                    }
                    None => {
                        if named && self.idx_done.insert((n, obj, dst, 1)) {
                            self.load_from(origin, obj, Key::Any(shape), dst);
                        }
                    }
                }
                // ToPropertyKey of a Number or BigInt is a numeric string: the
                // own fields a number key may name (DECISIONS 217); the chain
                // is `plain_key`'s. A marker has no own fields (`Model::field`:
                // its fields are its reach markers): what a number key reads
                // from it is its `[]` path step, which `IdxLoad` edged in
                // under the path's loop collapse (DECISIONS 158). An own-field
                // node of a marker stands outside that collapse: in
                // `x = x.next[0]` per unit it made a marker per iteration
                // (CHB11: 27k markers and 25 GB on tssrc).
                let exact = match !self.is_marker(obj) && index_has_numeric_key(prim) {
                    true => self.exact_index_slot(n, obj),
                    false => None,
                };
                if let Some(slot) = exact {
                    // An index-exact object read under the constant key `i`:
                    // its index `i` (`Model::index_slot`) and the chain's.
                    if self
                        .idx_done
                        .insert((n, obj, dst, crate::model::IDX_NUM_EXACT))
                    {
                        let key = self.elem_key();
                        self.builtin_field_use(obj, key, ChainAct::Load(dst, self.thrown));
                        self.edge(slot, dst);
                    }
                } else if !self.is_marker(obj)
                    && index_has_numeric_key(prim)
                    && self
                        .idx_done
                        .insert((n, obj, dst, crate::model::IDX_NUM_OWN))
                {
                    let key = self.elem_key();
                    self.builtin_field_use(obj, key, ChainAct::Load(dst, self.thrown));
                    if self.typed_array_number_key(obj, prim, named) {
                        // Rule TYPED-ELEM (`typed_array_number_key`): the
                        // element, a Number (BigInt) or `undefined`. Under
                        // rule TA-LEN (`elem_undef_watched`) the read's
                        // `Cons::ElemUndef` decides `undefined` (`idx_load_tok`
                        // registered it, or added `undefined` itself).
                        let p = self.typed_array_elem_prim(obj);
                        let floor = self.idx_done.contains(&(n, obj, dst, 0))
                            && self.elem_undef_watched(obj);
                        let undef = if floor { 0 } else { prim::UNDEF };
                        self.add_prim(dst, p | undef);
                    } else {
                        let f = self.num_fields(obj);
                        self.edge(f, dst);
                    }
                }
                // ToPropertyKey of `true`, `null`, `undefined` is a named key,
                // also beside Strings read per entry (rule K8).
                for (bit, keys) in [
                    (prim::BOOL, &["true", "false"][..]),
                    (prim::NULL, &["null"][..]),
                    (prim::UNDEF, &["undefined"][..]),
                ] {
                    if prim & bit != 0 && self.idx_done.insert((n, obj, dst, 2 + bit)) {
                        for k in keys {
                            let a = self.atom(k);
                            self.load_program_key_from(origin, obj, a, dst);
                        }
                    }
                }
            }
            Cons::IdxMCall { idx, call, shape } => {
                crate::family_reach::dispatch_route(self, |_| "a call under a computed key".into());
                // Primitive receivers (`MCall`'s rule): a number key calls
                // a key a store into ⊤ wrote on their prototype.
                if prim != 0 && !self.nodes[n as usize].pts.contains_tok(TOP) {
                    self.with_native(call, |m, call| builtins::prim_plain_key(m, prim, call));
                    let p = prim & !(prim::NULL | prim::UNDEF);
                    if p != 0 && self.idx_done.insert((idx, TOP, call, IDX_MCALL_PRIM | p)) {
                        self.cons(
                            idx,
                            Cons::IdxKeyCall {
                                obj: TOP,
                                prim: p,
                                call,
                                shape,
                            },
                        );
                    }
                }
                for &t in toks {
                    // ⊤ and a parameter marker take any key (the call
                    // escapes; the marker's reader replays the call).
                    if t == TOP || (self.is_marker(t) && !self.virt_protos.contains_key(&t)) {
                        self.method_on(t, Key::any(), call);
                        continue;
                    }
                    if self.idx_done.insert((idx, t, call, IDX_MCALL_TOKEN)) {
                        self.mark_use(t, crate::model::MARK_ESCAPE);
                        if self.index_needs_all_own_fields(t) {
                            let own = self.key_fields(t, Key::Any(shape));
                            self.bind_via(own, call, t);
                        }
                        self.plain_key(t, ChainAct::Call(call));
                        self.cons(
                            idx,
                            Cons::IdxKeyCall {
                                obj: t,
                                prim: 0,
                                call,
                                shape,
                            },
                        );
                    }
                }
            }
            Cons::IdxKeyCall {
                obj,
                prim: recv_prim,
                call,
                shape,
            } => {
                let named = index_needs_general_key(toks, prim);
                // One rule per receiver: a token, or (⊤) primitive kinds.
                let rule = match obj {
                    TOP => 128 | recv_prim,
                    _ => IDX_CALL_ANY,
                };
                if named && self.idx_done.insert((n, obj, call, rule)) {
                    match obj {
                        TOP => self.with_native(call, |m, call| {
                            builtins::prim_method(m, recv_prim, Key::Any(shape), call)
                        }),
                        _ => self.method_on(obj, Key::Any(shape), call),
                    }
                }
                if obj != TOP
                    && !self.is_marker(obj)
                    && index_has_numeric_key(prim)
                    && self.idx_done.insert((n, obj, call, IDX_CALL_NUM_OWN))
                {
                    let key = self.elem_key();
                    self.builtin_field_use(obj, key, ChainAct::Call(call));
                    // Rule TYPED-ELEM: the callee is an element, a Number
                    // (BigInt) or `undefined`, no function.
                    if !self.typed_array_number_key(obj, prim, named) {
                        let own = self.num_fields(obj);
                        self.bind_via(own, call, obj);
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
                            && self.idx_done.insert((n, obj, call, IDX_CALL_NAMED + bit))
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
                // `undefined` and `null` run no iteration (ECMA-262 14.7.5.6
                // ForIn/OfHeadEvaluation step 6.a).
                if prim & !(prim::NULL | prim::UNDEF) != 0 {
                    self.for_in_every_key(call, ft);
                }
                for &t in toks {
                    self.for_in_tok(call, ft, t);
                }
            }
            Cons::DefineAnyKey {
                o,
                key,
                value,
                get,
                set,
                site,
                reason,
                accessor,
                desc,
            } => {
                let d = crate::model::DefineAny {
                    o,
                    key,
                    value,
                    get,
                    set,
                    site,
                    reason,
                    accessor,
                    desc,
                };
                self.define_computed_key(d);
            }
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => {
                if let Some(k) = self.keyed_once(n, o, 0) {
                    let dk = k.unwrap_or_else(|| self.atom(crate::STAR_KEY));
                    self.define_on.insert((o, dk, !accessor));
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
            Cons::StoreKeyed {
                o,
                src,
                site,
                subj,
                region,
            } => {
                // FORIN1: a key that holds String constants only (a shared
                // for-in run over the keys `for_in_keys` lists,
                // `for_in_named_run`) is one of them: a named store per
                // constant, made as constants arrive. A key that turns any
                // String later takes the rule below.
                let (kp, ks) = (self.nodes[n as usize].prim, self.nodes[n as usize].strs);
                if !self.key_nodes.contains_key(&n)
                    && kp == prim::STR
                    && ks >= crate::model::STRS_SET0
                {
                    for i in 0..self.str_atoms(ks).len() {
                        let a = self.str_atoms(ks)[i];
                        if self.keyed_named.insert((n, src, a)) {
                            self.cons(
                                o,
                                Cons::Store {
                                    key: Key::Named(a),
                                    src,
                                    site,
                                    region,
                                },
                            );
                        }
                    }
                } else if let Some(k) = self.keyed_once(n, src, 1) {
                    // The key of a for-in run is a String: EnumerateObjectProperties
                    // never returns a Symbol key (ECMA-262 14.7.5.9, 14.7.5.10).
                    // An unknown one stores under the keys the subject
                    // enumerates (ENUM3), or else under the any-String shape,
                    // which never names a symbol-keyed property (`key_may_name_str`).
                    match k {
                        Some(a) => self.cons(
                            o,
                            Cons::Store {
                                key: Key::Named(a),
                                src,
                                site,
                                region,
                            },
                        ),
                        None if subj != NO_NODE => self.cons(
                            subj,
                            Cons::EnumKeyStore {
                                o,
                                src,
                                site,
                                from: None,
                            },
                        ),
                        None => self.enum_key_any(o, src, site),
                    }
                }
            }
            Cons::EnumKeyStore { o, src, site, from } => {
                self.enum_key_store(n, o, src, site, from, toks, prim)
            }
            Cons::EnumKeyAny { o, src, site } => {
                if !toks.is_empty() || prim != 0 {
                    if super::own_read::enum_diag() {
                        self.enum_diag_any(n, site, toks);
                    }
                    self.enum_key_any(o, src, site);
                }
            }
            Cons::StoreIndexed {
                o,
                src,
                site,
                shape,
                region,
            } => {
                // DECISIONS 217(c) for stores: a Number key (ToPropertyKey:
                // its ToString, a numeric name, `numeric_key`) writes an
                // holder's numeric properties, never a nonnumeric name.
                // A key's string constants (`Node::strs`, DESIGN SEEDKV)
                // and `true`, `false`, `null`, `undefined` are the names
                // they spell, an index string the numeric properties;
                // any string, a symbol, a BigInt or an object retains an
                // Any store.
                let strs = self.nodes[n as usize].strs;
                let mut index = prim & prim::NUM != 0;
                let mut names: Vec<Atom> = Vec::new();
                let mut index_strs: Vec<i64> = Vec::new();
                // Rule M3: a known prefix writes the names it admits
                // (`prefix_key_shape`); one that may spell a Number key is
                // any string.
                let mut prefixed: Vec<(Atom, KeyShapeId)> = Vec::new();
                let mut any_prefix = false;
                if prim & prim::STR != 0 {
                    for i in 0..self.str_atoms(strs).len() {
                        let a = self.str_atoms(strs)[i];
                        if self.atom_is_mark(a) {
                            // Rule STORE-ENTRY: an entry writes the names it
                            // may spell, as its read reads them.
                            match self.entry_key_shape(a) {
                                0 => any_prefix = true,
                                s => prefixed.push((a, s)),
                            }
                        } else if super::strs::index_string(&self.atoms[a as usize]) {
                            index = true;
                            index_strs.push(self.atoms[a as usize].parse::<i64>().unwrap_or(-1));
                        } else {
                            names.push(a);
                        }
                    }
                }
                for (bit, keys) in [
                    (prim::BOOL, &["true", "false"][..]),
                    (prim::NULL, &["null"][..]),
                    (prim::UNDEF, &["undefined"][..]),
                ] {
                    if prim & bit != 0 {
                        names.extend(keys.iter().map(|k| self.atom(k)));
                    }
                }
                // Every index store names its index interval before the
                // element store below reaches the holders (`Cons::LenIndex`,
                // `arr_len`): a Number key its interval, an index string
                // its value.
                let mut len_ivs: Vec<Iv> = Vec::new();
                if prim & prim::NUM != 0 {
                    len_ivs.push(self.range_input(n));
                }
                len_ivs.extend(index_strs.iter().map(|&v| Iv::int(v, v)));
                for iv in len_ivs {
                    self.indexed_store_sites.insert(site);
                    self.len_index_stores.insert(o);
                    if self.len_index_done.insert((site, o, iv.lo, iv.hi)) {
                        self.cons(
                            o,
                            Cons::LenIndex {
                                lo: iv.lo,
                                hi: iv.hi,
                            },
                        );
                    }
                }
                if index && self.index_store_done.insert((n, o, src, site, 1)) {
                    // A key that is one numeric literal and nothing else
                    // writes that index alone: a rest array's index slot
                    // (`store_into`), any other holder's elements.
                    let one = self
                        .num_literal_index(n)
                        .filter(|_| prim == prim::NUM && index_strs.is_empty());
                    let ek = match one {
                        Some(i) => Key::Named(self.atom(&format!("[[Index{i}]]"))),
                        None => self.elem_key(),
                    };
                    self.cons(
                        o,
                        Cons::Store {
                            key: ek,
                            src,
                            site,
                            region,
                        },
                    );
                }
                if index {
                    // `a[i] = v` may write past the end; `push` appends. At
                    // a site whose key is at most the length
                    // (`inbounds_sites`) it does not, while the key is a
                    // Number and nothing else (a key that may be anything
                    // else was not what gen compared).
                    let node = &self.nodes[n as usize];
                    let kept = self.inbounds_store
                        && (self.inbounds_sabotage || self.inbounds_sites.contains(&site))
                        && node.prim == prim::NUM
                        && node.pts.is_empty()
                        && index_strs.is_empty();
                    // A key of Numbers only: its interval against the
                    // length decides the hole (`index_hole_in`).
                    let num =
                        node.prim == prim::NUM && node.pts.is_empty() && index_strs.is_empty();
                    let (kind, c) = match (kept, num) {
                        (true, _) => (3, Cons::IndexKept),
                        (false, true) => (5, Cons::IndexHoleIn { key: n }),
                        (false, false) => (4, Cons::IndexHole),
                    };
                    if self.index_store_done.insert((n, o, src, site, kind)) {
                        self.cons(o, c);
                    }
                }
                // A key that may be no String (an object to ToPrimitive, a
                // symbol, a BigInt) keeps the any-key store; any String
                // stores under the any-String shape, which never names a
                // symbol-keyed property (`key_may_name_str`).
                // ECMA-262 7.1.19 ToPropertyKey: an object key is
                // ToPrimitive(key, string) (7.1.1): what its `@@toPrimitive` /
                // `toString` / `valueOf` returns (`to_prim_ret`; an object
                // result is no key: TypeError or the next method). It
                // stores under that result's key (this rule on that node).
                // ⊤, a marker or a token without the protocol's result keep
                // the any-key store. For every receiver (R-INSPKEY): a
                // String result never writes a symbol-keyed property such as
                // the inspect hook. In a result node, ⊤ or a marker (no
                // primitive bits: `add_top_inner`) may be a Symbol and keeps
                // the any-key store.
                let ret_node = self.to_prim_ret_nodes.contains(&n);
                let mut obj_key = false;
                if ret_node {
                    obj_key = toks.iter().any(|&t| t == TOP || self.is_marker(t));
                } else {
                    let r = self.reason("ToPropertyKey");
                    for &t in toks {
                        let ret = match t != TOP && !self.is_marker(t) {
                            true => {
                                self.implicit(
                                    t,
                                    r,
                                    crate::model::Protocol::ToPrimitive,
                                    self.thrown,
                                );
                                self.to_prim_ret.get(&t).copied()
                            }
                            false => None,
                        };
                        match ret {
                            Some(ret) => {
                                if self.to_prim_key_done.insert((ret, o, site)) {
                                    self.cons(
                                        ret,
                                        Cons::StoreIndexed {
                                            o,
                                            src,
                                            site,
                                            shape,
                                            region,
                                        },
                                    );
                                }
                            }
                            None => obj_key = true,
                        }
                    }
                }
                let not_str = obj_key || prim & (prim::SYMBOL | prim::BIGINT) != 0;
                // ECMA-262 7.1.19: a key that is a Number or a BigInt and
                // nothing else (no String, no Symbol, no object, not ⊤) is
                // ToString of one: decimal digits, never a non-digit name.
                // Its store is the `DIGITS` key's (`store_into`); a key
                // that later holds more stores the Any key as well.
                let digits = (toks.is_empty() || ret_node)
                    && prim & prim::BIGINT != 0
                    && prim & !(prim::NUM | prim::BIGINT) == 0;
                let any = not_str || (prim & prim::STR != 0 && strs == STRS_TOP) || any_prefix;
                // An Any store under no shape or the any-String shape (this
                // pop's or an earlier one's) is every name's.
                if shape == 0
                    && (any
                        || self.index_store_done.contains(&(n, o, src, site, 0))
                        || self.index_store_done.contains(&(n, o, src, site, 2)))
                {
                    names.clear();
                    prefixed.clear();
                }
                for (a, s) in prefixed {
                    if self.str_store_done.insert((n, o, src, site, a)) {
                        self.cons(
                            o,
                            Cons::Store {
                                key: Key::Any(s),
                                src,
                                site,
                                region,
                            },
                        );
                    }
                }
                for a in names {
                    if self.str_store_done.insert((n, o, src, site, a)) {
                        self.cons(
                            o,
                            Cons::Store {
                                key: Key::Named(a),
                                src,
                                site,
                                region,
                            },
                        );
                    }
                }
                let (kind, shape) = match shape == 0 && !not_str {
                    true => (2, self.any_string_shape()),
                    false => (0, shape),
                };
                if digits {
                    if self.index_store_done.insert((n, o, src, site, 5)) {
                        let key = self.digits_key();
                        self.cons(
                            o,
                            Cons::Store {
                                key,
                                src,
                                site,
                                region,
                            },
                        );
                    }
                } else if any && self.index_store_done.insert((n, o, src, site, kind)) {
                    // Diagnostic (off: one flag read): why a store keeps the any key.
                    if shape == 0 && crate::derive::why_indexed() {
                        let f = self
                            .stores
                            .get(site as usize)
                            .and_then(|x| self.funcs.get(x.func as usize))
                            .map_or("?", |f| f.name.as_str());
                        let nd = &self.nodes[n as usize];
                        eprintln!(
                            "WHY_ANYKEY {f} site={site} key={n} prim={:#x} strs_top={} dropped={} toks={:?}",
                            prim, strs == STRS_TOP, nd.dropped,
                            toks.iter().take(4).map(|&t| self.tok_name(t)).collect::<Vec<_>>()
                        );
                    }
                    self.cons(
                        o,
                        Cons::Store {
                            key: Key::Any(shape),
                            src,
                            site,
                            region,
                        },
                    );
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
            Cons::Range {
                op,
                other,
                dst,
                right,
            } => {
                // A code rule clips the operand's Strings by the literal's
                // code; its result is a code interval (`Node::crange`).
                if let RangeOp::SClip(k) = op {
                    let me = self.crange_input(n);
                    let ot = if other == NO_NODE {
                        Iv::TOP
                    } else {
                        self.crange_input(other)
                    };
                    let (l, r) = if right { (ot, me) } else { (me, ot) };
                    let iv = crate::model::range::eval(RangeOp::Clip(k), l, r);
                    if crate::solve::grow_crange(&mut self.nodes[dst as usize], iv) {
                        let d = &mut self.nodes[dst as usize];
                        if !d.in_wl {
                            d.in_wl = true;
                            self.wl.push_back(dst);
                        }
                    }
                    return;
                }
                let iv = if op == RangeOp::ToNumber {
                    self.to_number_range(n)
                } else {
                    // Rule NUM-PART: of an operand holding Numbers and
                    // Strings only, its Numbers alone take part (a) in a
                    // Number sum (ECMA-262 13.15.3 step 1.c: a pair with a
                    // String is a concatenation) and (b) as the guarded
                    // value of a clip (the clip bounds the guarded value's
                    // Numbers; its Strings pass as they are). A clip's bound
                    // operand converts (7.2.13, 7.2.14), so it keeps
                    // `range_input`.
                    let clip = matches!(op, RangeOp::Clip(_));
                    let input = |m: &Self, x: NodeId, part: bool| {
                        let nd = &m.nodes[x as usize];
                        if part
                            && nd.pts.is_empty()
                            && nd.prim & prim::NUM != 0
                            && nd.prim & !(prim::NUM | prim::STR) == 0
                        {
                            nd.range
                        } else {
                            m.range_input(x)
                        }
                    };
                    let me = input(self, n, op == RangeOp::Add || (clip && !right));
                    let ot = if other == NO_NODE {
                        Iv::EMPTY
                    } else {
                        input(self, other, op == RangeOp::Add || (clip && right))
                    };
                    let (l, r) = if right { (ot, me) } else { (me, ot) };
                    if self.range_sabotage && op == RangeOp::Add {
                        l
                    } else if self.exact_rule_sabotage && op == RangeOp::Add {
                        // Negative control (`exactrule`): a sum is the hull
                        // of its operands, not their sum.
                        l.join(r)
                    } else {
                        self.const_numbers
                            .then(|| crate::model::range::eval_exact(op, l, r))
                            .flatten()
                            .unwrap_or_else(|| {
                                crate::model::range::eval_with(op, l, r, self.zero_sign)
                            })
                    }
                };
                // Rule INT-OR-NAN: ToNumber/ToNumeric of an operand with no
                // fraction (NaN, an infinity, an integer within 2^53) has
                // none; `x + 1`, `x - 1` and `++`/`--` of one keep it (past
                // 2^53 a unit step rounds back to 2^53; NaN and the
                // infinities stay).
                let frac = !iv.within(Iv::int(-(1i64 << 53), 1i64 << 53))
                    && match op {
                        RangeOp::ToNumber => !self.no_frac_number(n, false),
                        RangeOp::Step(_) => !self.no_frac_number(n, true),
                        RangeOp::Add | RangeOp::Sub if other != NO_NODE => {
                            let one = Iv::int(-1, 1);
                            let (a, b) = if right { (other, n) } else { (n, other) };
                            let (ia, ib) = (self.range_input(a), self.range_input(b));
                            !((ib.within(one) && self.no_frac_number(a, true))
                                || (op == RangeOp::Add
                                    && ia.within(one)
                                    && self.no_frac_number(b, true)))
                        }
                        _ => true,
                    };
                let d = &mut self.nodes[dst as usize];
                let mut grew = if self.exact_rules {
                    crate::solve::grow_rule_range(d, iv)
                } else {
                    crate::solve::grow_range(d, iv)
                };
                if frac && !d.frac {
                    d.frac = true;
                    d.drange = true;
                    grew = true;
                }
                if grew {
                    let d = &mut self.nodes[dst as usize];
                    if !d.in_wl {
                        d.in_wl = true;
                        self.wl.push_back(dst);
                    }
                }
            }
            Cons::Arith { dst, other } => {
                let mut out = 0u16;
                // Objects go through ToPrimitive (user valueOf/toString).
                // ECMA-262 13.15.3: a BigInt beside a Number throws, so the
                // BigInt reaches the result only when `other` may be one.
                let big = if self.may_be_bigint(other) {
                    prim::BIGINT
                } else {
                    0
                };
                let objs = prim::NUM | big;
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
                    out |= big;
                }
                if prim & (prim::NUM | prim::STR | prim::BOOL | prim::NULL | prim::UNDEF) != 0 {
                    out |= prim::NUM;
                }
                if out != 0 {
                    self.add_prim(dst, out);
                }
            }
            // Rule K7: the case map of the receiver keeps its infix entries.
            // Rule CONSOLE-PCT: a first argument that may hold `%` lets the
            // specifiers consume the later arguments.
            Cons::ConsoleFormat {
                reason,
                agg,
                region,
            } => {
                // Rule R2: `%o` formats its argument with depth 4
                // (util_format.rs:415), every other specifier under the
                // call's limit.
                let floor = if self.may_hold_pct_o(n) {
                    INSPECT_PCT_O_LIMIT
                } else {
                    INSPECT_DEFAULT_LIMIT
                };
                let fired = self.nodes[agg as usize]
                    .cons
                    .iter()
                    .filter_map(|c| match c {
                        Cons::Implicit {
                            proto: Protocol::Inspect(at),
                            ..
                        } => Some(at.limit),
                        _ => None,
                    })
                    .max()
                    .is_some_and(|l| l >= floor.max(self.inspect_raised));
                if !fired && self.may_hold_percent(n) {
                    // `PERRY_WPF_IMPLSRC=1`: the first argument that lets
                    // the specifiers consume (its tokens and String entries).
                    if crate::solve::implsrc_on() {
                        self.console_pct_why(n);
                    }
                    self.console_inspect(agg, reason, false, region, floor, true);
                }
            }
            Cons::ObjTagProbe { t, key } => {
                // What each [[Get]] may give without another string: 7.1.1
                // GetMethod(@@toPrimitive) is undefined for undefined and
                // null; 7.1.1.1 OrdinaryToPrimitive skips a `toString` or
                // `valueOf` that is not callable (every primitive: with
                // neither left it throws, no string), so only the
                // intrinsic functions may be objects there; 20.1.3.6 step
                // 15 uses the builtin tag for a @@toStringTag that is not
                // a String. The read gives `undefined` beside the inherited
                // member while the own key is not surely present.
                let prim_ok = match key {
                    0 => prim::UNDEF | prim::NULL,
                    3 => !prim::STR,
                    _ => !0,
                };
                let name = if key == 1 { "toString" } else { "valueOf" };
                let ok = prim & !prim_ok == 0
                    && toks
                        .iter()
                        .all(|&f| (key == 1 || key == 2) && self.object_proto_native(f, name));
                if !ok {
                    self.objtag_widen(t);
                }
            }
            Cons::CaseMap { dst, upper } => {
                let strs = self.case_map_strs(n, upper);
                let strs = self.run_born(strs);
                self.add_s(dst, &[], prim::STR, strs);
            }
            Cons::Plus {
                str,
                num,
                other,
                right,
            } => {
                let mut s = 0u16;
                let mut k = 0u16;
                // Objects go through ToPrimitive (user valueOf/toString): a
                // String or a numeric value. Markers as in `Arith`; a BigInt
                // sum needs both operands to be BigInts (as in `Arith`).
                let big = if self.may_be_bigint(other) {
                    prim::BIGINT
                } else {
                    0
                };
                if toks.iter().any(|&t| !self.beside_values(t)) {
                    s |= prim::STR;
                    k |= prim::NUM | big;
                } else {
                    for &t in toks {
                        if self.beside_markers.contains(&t) {
                            let later = self.beside_arith.entry(t).or_default();
                            later.push((str, prim::STR));
                            later.push((num, prim::NUM | prim::BIGINT));
                        }
                    }
                }
                if prim & prim::STR != 0 {
                    s |= prim::STR;
                }
                if prim & prim::BIGINT != 0 {
                    k |= big;
                }
                if prim & (prim::NUM | prim::BOOL | prim::NULL | prim::UNDEF) != 0 {
                    k |= prim::NUM;
                }
                if s != 0 {
                    // Rule M3: the result's strings are the pair's
                    // (`plus_strs`); nothing while one operand holds no
                    // value yet (its own rule adds them when it does).
                    let (strs, gated) = match (other, right) {
                        (NO_NODE, _) => (STRS_TOP, false),
                        (o, false) => {
                            self.objtag_sink = Some(str);
                            let r = self.plus_strs(n, o);
                            self.objtag_sink = None;
                            r
                        }
                        (o, true) => {
                            self.objtag_sink = Some(str);
                            let r = self.plus_strs(o, n);
                            self.objtag_sink = None;
                            r
                        }
                    };
                    if gated && strs != STRS_TOP {
                        self.realm_str_dsts.push(str);
                    }
                    if strs != STRS_NONE {
                        let strs = self.run_born(strs);
                        self.add_s(str, &[], s, strs);
                    }
                }
                if k != 0 {
                    self.add_prim(num, k);
                }
            }
            Cons::ArrayLenArg { dst } => {
                self.add_tokens(dst, toks, prim & !prim::NUM);
            }
            Cons::ViewedBuffer { view, fresh, share } => {
                let a = self.atom(VIEWED_BUFFER);
                let f = self.field(view, a);
                let mut made = prim != 0;
                for &t in toks {
                    if t == TOP {
                        self.add_top(f, |_| {
                            "buffer of a view over a value of unknown code".into()
                        });
                        continue;
                    }
                    let what = match self.toks[t as usize] {
                        TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind {
                            TmplKind::BObj { what, .. } => what,
                            _ => "",
                        },
                        _ => "",
                    };
                    match (share, what) {
                        (true, "TypedArray" | "Buffer" | "DataView") => {
                            let src = self.field(t, a);
                            self.edge(src, f);
                        }
                        (false, "ArrayBuffer" | "SharedArrayBuffer") => self.add_tok(f, t),
                        _ => made = true,
                    }
                }
                if let (true, false, Some(fresh)) = (made, share, fresh) {
                    self.add_tok(f, fresh);
                }
            }
            // Rule RT: an `exec` record's derived captures.
            // Rule M6: String.prototype.replace of constants.
            Cons::StrReplace {
                dst,
                recv,
                search,
                repl,
            } => {
                if let Some(s) = self.replace_strs(recv, search, repl) {
                    let s = self.run_born(s);
                    self.add_s(dst, &[], prim::STR, s);
                }
            }
            // Rules KEY2a / KEY2b: indexOf / lastIndexOf and substring of
            // constants.
            Cons::StrIndex {
                dst,
                recv,
                search,
                last,
            } => self.str_index(dst, recv, search, last),
            Cons::StrSubstring {
                dst,
                recv,
                start,
                end,
            } => self.str_substring(dst, recv, start, end),
            // Rule M9: RegExp.prototype[@@replace] of a global one-class
            // pattern.
            Cons::ClassReplace { dst, tok, repl } => {
                if let Some(s) = self.class_replace_strs(tok, repl) {
                    if s != STRS_TOP {
                        self.realm_str_dsts.push(dst);
                    }
                    let s = self.run_born(s);
                    self.add_s(dst, &[], prim::STR, s);
                }
            }
            // Rule M7: the pattern of `new RegExp(P)`.
            Cons::RegExpPattern { tok } => self.regexp_pattern(n, tok),
            Cons::Captures { rec, pat } => {
                for (g, p, s) in self.capture_strs(n, pat) {
                    if p != 0 {
                        let slot = self.index_slot(rec, g);
                        let s = self.run_born(s);
                        self.add_s(slot, &[], p, s);
                    }
                }
            }
            Cons::ShareBuffer { ta } => {
                for &t in toks {
                    let shared = t == TOP
                        || matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                            if self.tmpls[tm as usize].kind.typed_storage());
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
                    self.charge_work(calls.len() as u64);
                    self.this_to(t, origin, &calls);
                    self.this_calls(t, origin, &calls);
                }
            }
            Cons::CallArgs { call } => {
                for &t in toks {
                    self.call_args(call, t);
                }
            }
            Cons::RetInto { call, run } => {
                // The run's result is the call's: its Numbers keep their
                // interval.
                let saved = self.cur_range.replace(self.rg(n));
                self.ret_into(call, run, toks, prim);
                self.cur_range = saved;
            }
            Cons::ArgInto { call, run, i } => self.arg_into(call, run, i, toks),
            Cons::Spread { into, site } => {
                // A primitive source copies no key surely (`null`,
                // `undefined`: none at all).
                if prim != 0 {
                    if let Some(&tm) = self.lit_spread_site.get(&site) {
                        self.spread_source(tm, site, Vec::new());
                    }
                }
                // A String source is ToObject'd (20.1.2.1 step 3.a.i, 13.2.5.5
                // CopyDataProperties): its own enumerable keys are its indices,
                // their values its one-character Strings (10.4.3.5); each
                // target gets a String under an index key.
                if prim & prim::STR != 0
                    && self
                        .idx_done
                        .insert((n, TOP, into, crate::model::IDX_SPREAD_STR))
                {
                    let v = self.node_l("spread-string-chars");
                    self.add_prim(v, prim::STR);
                    let key = self.elem_key();
                    self.cons(
                        into,
                        Cons::Store {
                            key,
                            src: v,
                            site,
                            region: self.thrown,
                        },
                    );
                }
                for &s in toks {
                    self.cons(
                        into,
                        Cons::SpreadFrom {
                            src: s,
                            site,
                            from: n,
                        },
                    );
                }
            }
            Cons::SpreadFrom { src, site, from } => {
                for &d in toks {
                    // A copy into an Array (`Object.assign`) may write any
                    // index, past its end too.
                    self.len_event(d, site, false);
                    self.arr_len_top(d);
                    self.spread_into(src, d, site, from);
                }
            }
            Cons::OwnComputedKey { key, val } => {
                for &t in toks {
                    if t == TOP {
                        continue;
                    }
                    let e = self.own_computed.entry(t).or_default();
                    if e.contains(&(key, val)) {
                        continue;
                    }
                    e.push((key, val));
                    for (d, site) in self.assign_into.get(&t).cloned().unwrap_or_default() {
                        self.cons(
                            key,
                            Cons::ProtoKeySet {
                                target: d,
                                val,
                                site,
                            },
                        );
                    }
                }
            }
            Cons::ProtoKeySet { target, val, site } => {
                if self.key_may_be_proto(n) && self.proto_key_set.insert((n, target, site)) {
                    let pk = Key::Named(self.atom("__proto__"));
                    self.store_into(target, pk, val, site);
                }
            }
            Cons::NativeThis { call, f } => self.native_this(call, f, toks, prim),
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
            // Rule PXCALL: a value of Proxy `p`'s target that may be
            // callable (not an ordinary object without [[Call]],
            // `no_call_tok`; a marker, ⊤ or a builtin may be one) gives `p`
            // its [[Call]]. A Proxy here reaches `proxy_apply` instead.
            Cons::ProxyCallee { p } => {
                if toks.iter().any(|&t| !self.no_call_tok(t)) {
                    self.proxy_callable(p);
                }
            }
            Cons::BuiltinCallee => {
                for &t in toks {
                    if self.per_receiver_callee(t) {
                        self.builtin_callee.insert(n);
                        let sites = self.callee_sites.get(&n).cloned().unwrap_or_default();
                        for (call, origin) in sites {
                            self.call_tok(call, t, Some(origin));
                        }
                    }
                }
            }
            Cons::NativeParent {
                tm,
                expected,
                reason,
            } => {
                let mut other = prim & prim::NULL != 0;
                let mut class_parent = false;
                for &t in toks {
                    if t == expected {
                        continue;
                    }
                    match self.toks[t as usize] {
                        // A program class (`g[k] = P` lets the global name
                        // `Error` read `P`) is modeled code, not unknown
                        // code: `super()` follows the live edge and runs its
                        // constructor, the class object's [[Prototype]]
                        // receives it (`feed_parent`), and the instances'
                        // chain continues at its prototype object
                        // (ClassDefinitionEvaluation: protoParent is the
                        // value's `prototype`, which a class keeps
                        // non-writable). As for a name-linked program parent
                        // whose evaluated value is not one class
                        // (`settle_parent_values`), the instance is hard E11
                        // and its parent unsettled; nothing escapes. A class
                        // with `tm` on its own chain does not exist yet when
                        // the `extends` of `tm` is evaluated: never its value.
                        TokKind::ClassObj(p) => {
                            class_parent = true;
                            if !self.has_ancestor(p, tm) {
                                let own = self.tok(TokKind::Proto(self.constructor_origin(tm)));
                                let pn = self.proto_node(own);
                                let pp = self.tok(TokKind::Proto(self.constructor_origin(p)));
                                self.add_tok(pn, pp);
                            }
                        }
                        _ => {
                            self.escape(t, reason);
                            other = true;
                        }
                    }
                }
                if other || class_parent {
                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                    if other {
                        self.escape(inst, reason);
                    }
                    self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
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
    /// What of `src` a prototype change gives: its objects (⊤ included) and
    /// `null`, never another primitive ([`Cons::ProtoObjects`]).
    pub(super) fn proto_objects(&mut self, src: NodeId) -> NodeId {
        let objs = self.node_l("given-prototype-objects");
        self.cons(src, Cons::ProtoObjects { dst: objs });
        objs
    }

    /// GP1: `t` is one built-in object (not ⊤, not a built-in prototype
    /// every object of a family inherits from): a prototype given to it
    /// is read through its lookups exactly.
    pub(crate) fn gives_exactly(&self, t: Tok) -> bool {
        t != TOP && matches!(self.toks[t as usize], TokKind::Builtin(_))
    }

    /// Both prototype-change spellings contribute the same chain relation.
    /// Only an object (or `null`) is given: `src` is filtered first.
    pub(crate) fn given_prototype(&mut self, t: Tok, given: NodeId) {
        let src = self.proto_objects(given);
        if t == TOP || matches!(self.toks[t as usize], TokKind::ClassObj(_)) {
            self.cons(src, Cons::ParentGiven { of: t });
        }
        // A runtime object (⊤ may be one; `ObjectProto` stands for
        // every builtin prototype) takes `src` as its prototype:
        // its lookups may reach the keys of `src`'s chain.
        // Such a lookup of a key `src`'s chain holds reads the opaque member
        // (`settle_native_given` writes every key of that chain on `t`): the
        // value it stands for, what `src`'s chain holds under the key, is
        // handed to code the model does not follow, so each member escapes.
        // `src` itself does not: a lookup on `t` finds what `src` holds,
        // never `src`. A builtin function value keeps the `proto_node` link
        // below, which the model walks.
        if (t == TOP || self.shared_of(t).is_some_and(|s| s != Shared::BuiltinFn))
            && !self.native_given.contains(&(t, given))
        {
            self.native_given.push((t, given));
            self.census_given(t, given);
            // GP1: setting a built-in's [[Prototype]] reads and calls
            // nothing (ECMA-262 10.1.2); a later [[Get]] on it continues at
            // `src` (10.1.8.1). Program lookups read `src`'s chain
            // (`settle_native_given` writes those values on `t`), and only a
            // key of that chain the runtime's own protocol lookups name hands
            // a member to code the model does not follow (sunk there). ⊤ and
            // the built-in prototypes keep every member.
            if !self.gives_exactly(t) {
                let members = self.node_l("given-prototype-members");
                self.cons(
                    src,
                    Cons::Load {
                        key: Key::Any(0),
                        dst: members,
                    },
                );
                let r = self.reason("E6 member of a prototype given to a runtime object");
                self.sink_node(members, r);
            }
        }
        // `%Object.prototype%` is an immutable prototype exotic
        // object: setting its [[Prototype]] to anything but its
        // own (null) throws.
        if t == TOP || matches!(self.toks[t as usize], TokKind::ObjectProto) {
            return;
        }
        let pn = self.proto_node(t);
        self.edge(src, pn);
        if self.given_proto.insert((src, pn)) {
            self.cons(src, Cons::ProtoGiven { protos: pn });
        }
        if let TokKind::ClassObj(tm) = self.toks[t as usize] {
            let instance = self.tok(TokKind::Obj(tm, NO_CTX));
            let reason = self.reason("E6 live superclass may change constructor birth fields");
            self.mark_hard(instance, Hard::E6SetPrototypeOf, reason);
        }
    }
}

impl Model {
    /// `bound` may be called with anything by the runtime: its target node
    /// decides (`Cons::BoundCall`), once per bound value.
    pub(crate) fn bound_call(&mut self, target: NodeId, bound: Tok, reason: u32) {
        let seen = self.nodes[target as usize]
            .cons
            .iter()
            .any(|c| matches!(c, Cons::BoundCall { bound: b, .. } if *b == bound));
        if !seen {
            self.cons(target, Cons::BoundCall { reason, bound });
        }
    }
}

impl Model {
    /// ECMA-262 13.15.3 ApplyStringOrNumericBinaryOperator: ToNumeric of
    /// both operands, and a TypeError when one is a BigInt and the other a
    /// Number. `other` (a binary operator's other operand) may be a BigInt
    /// after ToNumeric when it holds a BigInt or any value whose
    /// ToPrimitive may return one (an object, ⊤, a marker); `NO_NODE` (a
    /// unary operator) puts no condition.
    pub(super) fn may_be_bigint(&self, other: NodeId) -> bool {
        if other == NO_NODE {
            return true;
        }
        let o = &self.nodes[other as usize];
        o.prim & prim::BIGINT != 0 || !o.pts.is_empty()
    }
}
