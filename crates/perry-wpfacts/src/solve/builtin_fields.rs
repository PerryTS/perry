//! Program-owned fields on runtime objects use the ordinary field graph.
//! Native token overlap is read in one direction; field storage is never
//! merged merely because the anonymous native may denote two distinct values.

use super::*;

impl Model {
    /// A non-callable namespace's program-owned property (rule R1).
    pub(crate) fn builtin_field(&mut self, t: Tok, key: Key) -> Option<NodeId> {
        let Key::Named(a) = key else { return None };
        if builtins::builtin_function(self, t) || !builtins::undeclared(self, t, a) {
            return None;
        }
        let n = self.field(t, a);
        if self.r1_holders.insert(t) && self.interp_runs {
            self.builtin_fields_sink(t, None);
        } else if self.realm_held {
            self.realm_hold_tok(t, None);
        }
        Some(n)
    }

    /// Join the fields of each token that may denote this native. `visits`
    /// keeps the ordinary read/call dependencies for native tokens minted
    /// later; every edge ends at the use, never another token's storage.
    pub(crate) fn builtin_field_use(&mut self, t: Tok, key: Key, act: ChainAct) {
        if !builtins::builtin_function(self, t) {
            return;
        }
        if !self.visits.entry(t).or_default().insert((t, key, act)) {
            return;
        }
        let holders = self.native_holders(t);
        for h in holders {
            self.native_field_at(t, h, key, act);
        }
    }

    fn native_field_at(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        if matches!(act, ChainAct::Lookup { .. }) {
            self.accessor_walk(origin, holder, key, act);
            return;
        }
        match act {
            ChainAct::Load(dst) => {
                let n = if self.is_elem_key(key) {
                    self.num_fields(holder)
                } else {
                    match key {
                        Key::Named(a) => self.field(holder, a),
                        Key::Any(_) => self.key_fields(holder, key),
                    }
                };
                self.edge(n, dst);
            }
            ChainAct::Call(call) => {
                let n = if self.is_elem_key(key) {
                    self.num_fields(holder)
                } else {
                    match key {
                        Key::Named(a) => self.field(holder, a),
                        Key::Any(_) => self.key_fields(holder, key),
                    }
                };
                self.bind_via(n, call, origin);
            }
            ChainAct::Store(_) | ChainAct::Lookup { .. } => {}
        }
        if holder != origin && !self.is_elem_key(key) {
            match act {
                ChainAct::Load(dst) => {
                    self.with_b(|m| builtins::native_own_load(m, origin, holder, key, dst));
                }
                ChainAct::Call(call) => {
                    let value = self.node_l("native-own-value");
                    self.with_b(|m| builtins::native_own_load(m, origin, holder, key, value));
                    self.bind_via(value, call, origin);
                }
                ChainAct::Store(_) | ChainAct::Lookup { .. } => {}
            }
        }
        // A function value's inherited fields are observations of the same
        // denotation as its own fields. Keep edges at the use: sharing the
        // prototype storage would make Any an equivalence relation. A key
        // the namespace `origin` owns ends the lookup at `origin` (ECMA-262
        // 10.1.8.1 OrdinaryGet; `Promise.all`, 27.2.4.1): where `holder`
        // denotes `origin`, its chain is not read.
        let owned = matches!(key, Key::Named(a) if builtins::namespace_owns(self, origin, a));
        if !self.unit_mode && holder != origin && !self.is_elem_key(key) && !owned {
            if self.is_proto_key(key) {
                if let ChainAct::Load(dst) = act {
                    self.proto_value_at(holder, dst);
                }
            }
            let cv = self.cv(holder, key);
            match act {
                ChainAct::Load(dst) => self.edge(cv, dst),
                ChainAct::Call(call) => self.bind_via(cv, call, origin),
                ChainAct::Store(_) | ChainAct::Lookup { .. } => {}
            }
        }
        self.accessor_walk(origin, holder, key, act);
    }

    /// A newly minted native joins the reads already registered on tokens
    /// whose denotations overlap it. Reuse the ordinary holder visits rather
    /// than recording a separate native-field registry.
    pub(crate) fn builtin_token_created(&mut self, t: Tok) {
        if !builtins::builtin_function(self, t) {
            if self.interp_runs && matches!(self.toks[t as usize], TokKind::Builtin(_)) {
                self.builtin_fields_sink(t, None);
            } else if self.realm_held {
                self.realm_hold_tok(t, None);
            }
            return;
        }
        let mut uses = Vec::new();
        for holder in self.native_holders(t) {
            if let Some(registered) = self.visits.get(&holder) {
                for &(origin, key, act) in registered {
                    self.charge_work(1);
                    if origin == holder {
                        uses.push((origin, key, act));
                    }
                }
            }
        }
        for (origin, key, act) in uses {
            self.native_field_at(origin, t, key, act);
        }
        if self.realm_held {
            let reason = self.reason(if self.interp_runs {
                "built-in function object interpreted code may read"
            } else {
                "an intrinsic of the realm: unknown code holds the realm"
            });
            self.escape(t, reason);
        }
    }

    /// Exact values can overlap only themselves and the existing wildcard
    /// values. A wildcard walks the interning index's builtin values. Every
    /// candidate is charged, including values the overlap predicate rejects.
    fn native_holders(&self, t: Tok) -> Vec<Tok> {
        if matches!(
            self.toks[t as usize],
            TokKind::BuiltinFn(Native::Any) | TokKind::Builtin(BuiltinNs::ObjectProtoMethod)
        ) {
            return self
                .tok_index
                .builtin_values()
                .copied()
                .filter(|&h| {
                    self.charge_work(1);
                    builtins::native_may_be(self, t, h)
                })
                .collect();
        }
        let mut candidates = vec![t];
        for kind in [
            TokKind::BuiltinFn(Native::Any),
            TokKind::Builtin(BuiltinNs::ObjectProtoMethod),
        ] {
            if let Some(&h) = self.tok_index.get(&kind) {
                candidates.push(h);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates.retain(|&h| {
            self.charge_work(1);
            builtins::native_may_be(self, t, h)
        });
        candidates
    }

    /// Non-callable runtime objects retain the namespace escape rule.
    /// Callable objects enter `escape_inner`, just like program objects.
    pub(crate) fn builtin_fields_sink(&mut self, t: Tok, reason: Option<u32>) {
        if self.unit_mode || t == TOP {
            return;
        }
        if builtins::builtin_function(self, t) {
            let r = reason.unwrap_or_else(|| {
                self.reason("built-in function object interpreted code may read")
            });
            self.escape(t, r);
            return;
        }
        if builtins::global_object(self, t) {
            return;
        }
        let holder = match self.toks[t as usize] {
            TokKind::Builtin(_) => t,
            _ => return,
        };
        self.r1_sinks[0] += 1;
        if self.r1_holders.contains(&holder) {
            self.r1_sinks[1] += 1;
        }
        // Unknown code can install an own member even when the intrinsic
        // prototype refuses additions. Ordinary reads replay that write.
        builtins::note_native_write(self, holder, None, "built-in object given to unknown code");
        let r = reason
            .unwrap_or_else(|| self.reason("field of a built-in object interpreted code may read"));
        let all = self.all_fields(holder);
        self.sink_node(all, r);
    }

    /// Unknown code exists: it holds the realm (ECMA-262 9.3: every
    /// intrinsic is reachable from the global object it can name), not only
    /// what the program hands it. One seed, once (`realm_held`, monotone):
    /// each intrinsic token the model has is held (`realm_hold_tok`); each
    /// one minted later is held when minted (`builtin_token_created`,
    /// `builtin_field`).
    pub(crate) fn hold_realm(&mut self, reason: u32) {
        if self.realm_held {
            return;
        }
        self.realm_held = true;
        self.realm_widen_strs();
        let mut hs: Vec<Tok> = self.r1_holders.iter().copied().collect();
        hs.extend((0..self.toks.len() as Tok).filter(|&t| {
            matches!(self.toks[t as usize], TokKind::Builtin(_))
                || builtins::builtin_function(self, t)
        }));
        hs.sort_unstable();
        hs.dedup();
        for h in hs {
            self.realm_hold_tok(h, Some(reason));
        }
    }

    /// Unknown code exists: every String form that held only while it did
    /// not (rule M9's exclusion entries; `realm_str_dsts`) is any string.
    pub(crate) fn realm_widen_strs(&mut self) {
        for d in std::mem::take(&mut self.realm_str_dsts) {
            self.add_s(d, &[], prim::STR, STRS_TOP);
        }
    }

    /// Unknown code holds intrinsic `t`: a built-in function as one handed
    /// to it (`escape`: it may store any key on it, call it with anything,
    /// and the program's reads of its fields see what it stored); a
    /// namespace's program fields (rule R1) are read there. It never
    /// replaces an intrinsic's own members: a held function stays that
    /// native, `console.log` stays the realm's (unlike interpreted code,
    /// `builtin_fields_sink`, whose writes the model replays).
    pub(crate) fn realm_hold_tok(&mut self, t: Tok, reason: Option<u32>) {
        if t == TOP {
            return;
        }
        let r = match reason {
            Some(r) => r,
            None => self.reason("an intrinsic of the realm: unknown code holds the realm"),
        };
        if builtins::builtin_function(self, t) {
            return self.escape(t, r);
        }
        if self.unit_mode
            || builtins::global_object(self, t)
            || !matches!(self.toks[t as usize], TokKind::Builtin(_))
        {
            return;
        }
        let all = self.all_fields(t);
        self.sink_node(all, r);
    }

    /// Interpreted code can obtain native functions by their realm paths.
    pub(crate) fn builtin_fields_interp(&mut self) {
        if crate::unksrc::on() {
            crate::unksrc::note("interp", "interpreted code runs", u32::MAX, u32::MAX);
            if !self.realm_held {
                crate::unksrc::held("interp", "interpreted code runs", u32::MAX, u32::MAX);
            }
        }
        if self.keysnap.is_some() && !self.realm_held {
            self.keysnap_at(true, "interpreted code (builtin_fields_interp)", None, u32::MAX, None);
        }
        self.realm_held = true;
        self.realm_widen_strs();
        let mut hs: Vec<Tok> = self.r1_holders.iter().copied().collect();
        hs.extend((0..self.toks.len() as Tok).filter(|&t| {
            matches!(self.toks[t as usize], TokKind::Builtin(_))
                || builtins::builtin_function(self, t)
        }));
        hs.sort_unstable();
        hs.dedup();
        for h in hs {
            self.builtin_fields_sink(h, None);
        }
    }
}
