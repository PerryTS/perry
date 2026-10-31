//! Prototype value lookup, holder traversal and shared accessor/callee flow.

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

    /// The reads of `tm` that found a key no longer sure read `undefined`.
    pub(crate) fn flush_present_reads(&mut self, tm: TmplId) {
        let reads = self.present_reads.remove(&tm).unwrap_or_default();
        let mut kept = Vec::new();
        for (a, dst) in reads {
            let sure = !self.keys_unsure.contains(&tm)
                && (self.tmpls[tm as usize].definite.contains(&a) || self.spread_sure(tm, a));
            if sure {
                kept.push((a, dst));
            } else {
                self.add_prim(dst, prim::UNDEF);
            }
        }
        if !kept.is_empty() {
            self.present_reads.insert(tm, kept);
        }
    }

    /// dst ⊇ value of `key` read on object `t` (receiver `origin`).
    pub fn load_from(&mut self, origin: Tok, t: Tok, key: Key, dst: NodeId) {
        self.mark_use(t, crate::model::MARK_READ);
        if self.is_iter_key(key) {
            match self.toks[t as usize] {
                TokKind::Top
                | TokKind::Builtin(_)
                | TokKind::BuiltinFn
                | TokKind::NativeProto(_) => {}
                TokKind::Obj(_, _) if self.is_array_like(t) => {}
                // A program object/function iterates only through its
                // protocol, which `Gen::iterate` follows separately.
                _ => return,
            }
            let ek = self.elem_key();
            return self.load_from(origin, t, ek, dst);
        }
        if self.is_elem_key(key) {
            if t != TOP && self.is_array_like(t) {
                let all = self.all_fields(t);
                self.edge(all, dst);
                self.add_prim(dst, prim::UNDEF);
                return;
            }
            return self.load_from(origin, t, Key::Any, dst);
        }
        if t == TOP {
            self.add_tok(dst, TOP);
            // Any function's `.constructor` is the Function constructor (E14).
            if self.is_constructor_key(key) {
                let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                self.add_tok(dst, f);
            }
            return;
        }
        if matches!(self.toks[t as usize], TokKind::BuiltinFn) {
            // Opaque (no field storage, `store_into`): its members are
            // builtin functions and primitives (`tok_load`), or ⊤ once any
            // builtin function value has been written.
            if self.bf_written.is_some() {
                self.add_top(dst, |_| {
                    "property of a written builtin function value".into()
                });
            } else {
                self.bf_reads.push(dst);
            }
            self.with_b(|m| builtins::tok_load(m, t, key, dst));
            return;
        }
        if self.is_array_length(t, key) {
            self.add_prim(dst, prim::NUM);
            return;
        }
        // An absent key reads `undefined`: only a definite birth key of a
        // fact unit is known to be present.
        let present = match (key, self.tmpl_of(t)) {
            (Key::Named(a), Some(tm)) => {
                let sure = self.tmpls[tm as usize].kind.is_fact_unit()
                    && !self.keys_unsure.contains(&tm)
                    && (self.tmpls[tm as usize].definite.contains(&a) || self.spread_sure(tm, a));
                if sure && self.present_seen.insert((tm, a, dst)) {
                    self.present_reads.entry(tm).or_default().push((a, dst));
                }
                sure
            }
            _ => false,
        };
        if !present {
            self.add_prim(dst, prim::UNDEF);
        }
        match key {
            // A method key on a published receiver finds the class chain's
            // value, not the receiver's unknown own field (DECISIONS 59).
            Key::Named(a) => match self.virt_load(t, a) {
                Some(v) => {
                    self.edge(v, dst);
                    // A key another unit's subclass defines: a read finds an
                    // unknown own value (only a call through the receiver
                    // is that subclass's method, `virt_call`, DECISIONS 97).
                    if !self.virt_overriders(t, a).is_empty() {
                        let f = self.field(t, a);
                        self.edge(f, dst);
                    }
                }
                None => {
                    let f = self.field(t, a);
                    self.edge(f, dst);
                }
            },
            Key::Any => {
                let all = self.all_fields(t);
                self.edge(all, dst);
            }
        }
        // A marker is use tracking, not a value (DECISIONS 111): a load from
        // it reads its fields only. A parameter marker has no prototype here
        // (DECISIONS 119): what
        // its argument inherits is the reader's, which replays the load on
        // it (the keyed reach above); values beside a marker walk their own
        // chains. A receiver marker's chain is its classes' (`virt_protos`).
        if self.is_marker(t) && !self.virt_protos.contains_key(&t) {
            return;
        }
        // DECISIONS 217: `__proto__` is Object.prototype's accessor, found on
        // every chain: it reads the receiver's prototype, as a value.
        if self.is_proto_key(key) {
            self.proto_value(t, dst);
        }
        // DECISIONS 124: a receiver marker with values beside it stands for
        // instances of its classes, whose builtin members are the chain's
        // (`cv` below reaches Object.prototype): no builtin object's.
        // An own data key the object surely holds stops the lookup at the
        // object: the chain's end, where a key no link defines reads
        // `undefined`, is never reached. Its values stay (a superset).
        let chain = if present && !self.is_proto_key(key) {
            self.defined_into(dst)
        } else {
            dst
        };
        if !(self.is_marker(t) && self.beside_values(t)) {
            self.with_b(|m| builtins::tok_load(m, t, key, chain));
        }
        // Element reads of array-likes stop at their own elements: their
        // builtin prototype members are approximated by `tok_load`.
        if key == Key::Any && self.is_array_like(t) {
            return;
        }
        // The prototype chain's values, shared by every receiver of `t`.
        let cv = self.cv(t, key);
        self.edge(cv, chain);
        // Accessors run with the receiver as `this`: walk per receiver, only
        // for keys an accessor may be defined under.
        self.accessor_walk(origin, t, key, ChainAct::Load(dst));
    }

    /// A key generation names an accessor under.
    pub(crate) fn add_accessor_atom(&mut self, a: Atom) {
        self.numeric_accessor |= self.atoms[a as usize].parse::<f64>().is_ok();
        self.accessor_atoms.insert(a);
    }

    pub fn is_proto_key(&self, key: Key) -> bool {
        matches!(key, Key::Named(a) if self.atoms[a as usize] == "__proto__")
    }

    /// dst ⊇ `t`'s prototypes as values (DECISIONS 217): what
    /// `Object.getPrototypeOf(t)` and `t.__proto__` return. A literal's
    /// class prototype is its layout, no object (`Cons::ProtoValues`);
    /// `ObjectProto` stands for every builtin prototype, so its own
    /// prototype may be itself (Array.prototype's is Object.prototype) or
    /// null; a module record's (`module.exports` or a namespace) may be null.
    pub fn proto_value(&mut self, t: Tok, dst: NodeId) {
        match self.toks[t as usize] {
            TokKind::Top => self.add_tok(dst, TOP),
            TokKind::ObjectProto => {
                self.add_tok(dst, t);
                self.add_prim(dst, prim::NULL);
            }
            _ => {
                if matches!(self.toks[t as usize], TokKind::ModNs(_)) {
                    let op = self.tok(TokKind::ObjectProto);
                    self.add_tok(dst, op);
                    self.add_prim(dst, prim::NULL);
                }
                let pn = self.proto_node(t);
                if self.idx_done.insert((pn, t, dst, IDX_PROTO_VALUE)) {
                    self.cons(pn, Cons::ProtoValues { dst });
                }
            }
        }
    }

    /// `t` read (called) under a key that names no member of an intrinsic
    /// prototype and is not `__proto__`: a number (DECISIONS 217; its own
    /// fields under any key are the caller's). Its program prototypes'
    /// members under any key, a key a store into ⊤ wrote on an intrinsic
    /// prototype (`Cons::PlainKey`), and accessors that key may name.
    pub(crate) fn plain_key(&mut self, t: Tok, act: ChainAct) {
        if t == TOP || (self.is_marker(t) && !self.virt_protos.contains_key(&t)) {
            return;
        }
        let cp = self.cp(t);
        let at = match act {
            ChainAct::Load(n) | ChainAct::Store(n) => n,
            ChainAct::Call(c) => c,
        };
        let tag = match act {
            ChainAct::Load(_) => IDX_PLAIN_LOAD,
            ChainAct::Store(_) => return,
            ChainAct::Call(_) => IDX_PLAIN_CALL,
        };
        if self.idx_done.insert((cp, t, at, tag)) {
            self.cons(cp, Cons::PlainKey { origin: t, act });
            // An accessor under a numeric name (one under a computed key
            // made the object's fields ⊤ already).
            if self.numeric_accessor {
                self.accessor_walk(t, t, Key::Any, act);
            }
        }
    }

    /// A key no builtin prototype defines (ToPropertyKey of `true`, `false`,
    /// `null`, `undefined`; ECMA-262 has no such member): the receiver's own
    /// field and its program prototype chain, else `undefined`. The builtin
    /// member approximation (`tok_load`) has nothing under it.
    pub fn load_program_key(&mut self, t: Tok, a: Atom, dst: NodeId) {
        if t == TOP || matches!(self.toks[t as usize], TokKind::BuiltinFn) {
            return self.load_from(t, t, Key::Named(a), dst);
        }
        self.mark_use(t, crate::model::MARK_READ);
        self.add_prim(dst, prim::UNDEF);
        let f = self.field(t, a);
        self.edge(f, dst);
        let cv = self.cv(t, Key::Named(a));
        self.edge(cv, dst);
        self.accessor_walk(t, t, Key::Named(a), ChainAct::Load(dst));
    }

    /// Accessors run with the receiver as `this`. Named keys an accessor
    /// may be defined under walk the chain per receiver; computed keys
    /// visit, through the prototype closure, only holders with accessors.
    pub(crate) fn accessor_walk(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
        if !self.needs_accessor_walk(key) {
            return;
        }
        let Some(act) = self.shared_act(origin, t, key, act) else {
            return;
        };
        match key {
            Key::Named(_) => self.holder_walk(origin, t, key, act, false),
            Key::Any => {
                self.accessor_hit(origin, t, Key::Any, act);
                let cp = self.cp(t);
                if self.any_walk_done.insert((cp, origin, act)) {
                    self.any_walks.push((cp, origin, t, act));
                    self.any_walk_holders
                        .entry(t)
                        .or_default()
                        .push((origin, act));
                    self.cons(cp, Cons::AnyAccessors { origin, act });
                }
            }
        }
    }

    /// Walks are per (receiver, start, key), not per site: the first site
    /// walks with a shared act (a node the accessors feed, or that feeds
    /// the setters), every site connects to it. `None`: already walked.
    /// The same accessors run with the same `this`, so the union each site
    /// sees is unchanged.
    pub(super) fn shared_act(
        &mut self,
        origin: Tok,
        t: Tok,
        key: Key,
        act: ChainAct,
    ) -> Option<ChainAct> {
        let store = matches!(act, ChainAct::Store(_));
        let kind: u8 = match act {
            ChainAct::Load(_) => 0,
            ChainAct::Store(_) => 1,
            ChainAct::Call(_) => 2,
        };
        let (n, fresh) = match self.walk_nodes.get(&(origin, t, key, kind)) {
            Some(&n) => (n, false),
            None => {
                let n = self.node_l(["walk-load", "walk-store", "walk-call"][kind as usize]);
                if kind == 1 {
                    self.walk_store_pending.insert(n, Vec::new());
                }
                self.walk_nodes.insert((origin, t, key, kind), n);
                (n, true)
            }
        };
        match act {
            ChainAct::Load(dst) => self.edge(n, dst),
            ChainAct::Store(src) => match self.walk_store_pending.get_mut(&n) {
                Some(v) => v.push(src),
                None => self.edge(src, n),
            },
            ChainAct::Call(call) => self.bind_via(n, call, origin),
        }
        // A call walk collects getter results as a load does.
        fresh.then_some(if store {
            ChainAct::Store(n)
        } else {
            ChainAct::Load(n)
        })
    }

    /// The holder sets of a named-key lookup that starts at `t` itself
    /// (`t` is passed through, then its prototypes).
    pub(super) fn holders(&mut self, t: Tok, key: Key) -> (NodeId, NodeId) {
        if let Some(&x) = self.holder_nodes.get(&(t, key)) {
            return x;
        }
        let p = self.node_l("holders");
        let s = self.node_l("holders-stop");
        self.holder_nodes.insert((t, key), (p, s));
        self.add(p, &[t], 0);
        let pn = self.proto_node(t);
        self.cons(
            pn,
            Cons::HolderStep {
                key,
                pass: p,
                stop: s,
            },
        );
        (p, s)
    }

    /// A named-key accessor lookup for receiver `origin` from `t` (from
    /// `t`'s prototypes, or, `at_start`, from prototype `t` itself with a
    /// stop check there: `super.key`). Accessors on the pass holders run
    /// and replay; those on stop holders run.
    pub(super) fn holder_walk(
        &mut self,
        origin: Tok,
        t: Tok,
        key: Key,
        act: ChainAct,
        at_start: bool,
    ) {
        let (p, s) = if at_start {
            let p = self.node_l("holders");
            let s = self.node_l("holders-stop");
            let x = self.node_l("holders-start");
            self.add(x, &[t], 0);
            self.cons(
                x,
                Cons::HolderStep {
                    key,
                    pass: p,
                    stop: s,
                },
            );
            (p, s)
        } else {
            self.holders(t, key)
        };
        self.cons(p, Cons::VisitHolders { origin, key, act });
        self.cons(s, Cons::VisitStop { origin, key, act });
    }

    /// A store walk found a consumer: its sites' sources flow from now on.
    pub(super) fn store_live(&mut self, n: NodeId) {
        if let Some(srcs) = self.walk_store_pending.remove(&n) {
            for src in srcs {
                self.edge(src, n);
            }
        }
    }

    pub fn needs_accessor_walk(&self, key: Key) -> bool {
        match key {
            Key::Named(a) => self.accessor_atoms.contains(&a),
            Key::Any => !self.accessor_atoms.is_empty(),
        }
    }

    /// The values `key` takes on `t`'s prototypes (transitively).
    pub fn cv(&mut self, t: Tok, key: Key) -> NodeId {
        if let Some(&n) = self.cv_nodes.get(&(t, key)) {
            return n;
        }
        let n = self.node_l("chainval");
        self.cv_nodes.insert((t, key), n);
        let pn = self.proto_node(t);
        self.cons(pn, Cons::ChainUnion { key, dst: n, of: t });
        n
    }

    /// The node whose values reach `dst` without `undefined` (`Cons::Defined`).
    fn defined_into(&mut self, dst: NodeId) -> NodeId {
        if let Some(&n) = self.defined_via.get(&dst) {
            return n;
        }
        let n = self.node_l("defined");
        self.defined_via.insert(dst, n);
        self.cons(n, Cons::Defined { dst });
        n
    }

    /// Every prototype of `t`, transitively.
    pub fn cp(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.cp_nodes.get(&t) {
            return n;
        }
        let n = self.node_l("protoclosure");
        self.cp_nodes.insert(t, n);
        let pn = self.proto_node(t);
        self.cons(pn, Cons::ProtoClosure { dst: n });
        n
    }

    /// Does a lookup of `key` stop at prototype `q` (a declared member)?
    pub(super) fn stops_at(&self, q: Tok, key: Key) -> bool {
        match (key, &self.toks[q as usize]) {
            (Key::Named(a), TokKind::Proto(tm)) => {
                self.tmpls[*tm as usize].proto_members.contains(&a)
                    && !self.hard.contains_key(&q)
                    && !self.escaped.contains_key(&q)
            }
            _ => false,
        }
    }

    pub fn is_array_like(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind {
                TmplKind::Arr { .. } => true,
                TmplKind::BObj { .. } => true,
                _ => false,
            },
            _ => false,
        }
    }

    /// An Array's own `length` (design §17.8, DECISIONS 54): a data property
    /// that always holds a Number and is never an element. A read is that
    /// Number; a store converts its value with ToNumber and only moves the
    /// end of the element range (a new slot reads `undefined`, which every
    /// element read already has). Neither touches a field, so `length` never
    /// joins the elements (`all_fields`) nor reads them (`field_any`).
    pub fn is_array_length(&self, t: Tok, key: Key) -> bool {
        matches!(key, Key::Named(a) if self.atoms[a as usize] == "length")
            && matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                if matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    }

    pub fn is_constructor_key(&self, key: Key) -> bool {
        match key {
            Key::Any => true,
            Key::Named(a) => self.atoms[a as usize] == "constructor",
        }
    }

    /// Start or continue a chain lookup at `t`'s prototypes.
    pub(super) fn chain_from(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
        // Own accessors of `t` itself (object-literal getters/setters).
        if !matches!(self.toks[t as usize], TokKind::ObjectProto) {
            self.visits.entry(t).or_default().insert((origin, key, act));
        }
        self.accessor_hit(origin, t, key, act);
        let pn = self.proto_node(t);
        if self.chain_done.insert((pn, origin, key, act)) {
            self.cons(pn, Cons::Chain { origin, key, act });
        }
    }

    /// One step of a per-receiver accessor walk: prototype `p` of `origin`.
    /// (Values and methods come from the shared chain nodes `cv`/`cp`.)
    pub fn chain_step(&mut self, _pn: NodeId, origin: Tok, p: Tok, key: Key, act: ChainAct) {
        if p == TOP {
            match act {
                ChainAct::Load(dst) => self.add_tok(dst, TOP),
                ChainAct::Call(call) => self.call_tok(call, TOP, Some(origin)),
                ChainAct::Store(src) => {
                    self.store_live(src);
                    let r = self.reason("store through an unknown prototype chain");
                    self.sink_node(src, r);
                }
            }
            return;
        }
        // A class prototype defines its declared members at class creation:
        // the lookup ends there (resumed if the prototype is later mutated).
        if self.stops_at(p, key) {
            self.accessor_hit(origin, p, key, act);
            self.stopped.entry(p).or_default().push((origin, key, act));
            return;
        }
        self.chain_from(origin, p, key, act);
    }

    /// `super.key` from receiver `origin`: the lookup starts at `start`.
    pub(super) fn super_lookup(&mut self, origin: Tok, start: Tok, key: Key, act: ChainAct) {
        if start == TOP {
            self.chain_step(0, origin, TOP, key, act);
            return;
        }
        match act {
            ChainAct::Load(dst) if self.is_array_length(start, key) => {
                self.add_prim(dst, prim::NUM);
            }
            ChainAct::Load(dst) => {
                let f = match key {
                    Key::Named(a) => self.field(start, a),
                    Key::Any => self.all_fields(start),
                };
                self.edge(f, dst);
                // DECISIONS 217: `super.__proto__` runs Object.prototype's
                // getter on the receiver.
                if self.is_proto_key(key) {
                    let pk = self.atom("__proto__");
                    self.load_from(origin, origin, Key::Named(pk), dst);
                }
                self.with_b(|m| builtins::tok_load(m, start, key, dst));
                if !self.stops_at(start, key) {
                    let up = self.cv(start, key);
                    self.edge(up, dst);
                }
            }
            ChainAct::Call(call) => {
                let f = match key {
                    Key::Named(a) => self.field(start, a),
                    Key::Any => self.all_fields(start),
                };
                self.bind_via(f, call, origin);
                self.with_b(|m| builtins::proto_method(m, start, origin, key, call));
                if !self.stops_at(start, key) {
                    let up = self.cv(start, key);
                    self.bind_via(up, call, origin);
                    let cp = self.cp(start);
                    self.cons(cp, Cons::ProtoMethods { origin, key, call });
                }
            }
            ChainAct::Store(_) => {}
        }
        match key {
            Key::Named(_) if self.needs_accessor_walk(key) => {
                // `start` itself is a prototype: a lookup can stop there.
                if let Some(act) = self.shared_act(origin, start | SUPER_BIT, key, act) {
                    self.holder_walk(origin, start, key, act, true)
                }
            }
            Key::Any => self.accessor_walk(origin, start, key, act),
            _ => {}
        }
    }

    /// A prototype became HARD or escaped: lookups that stopped at it go on.
    pub(super) fn resume_stopped(&mut self, p: Tok) {
        if let Some(v) = self.stopped.remove(&p) {
            for (origin, key, act) in v {
                self.chain_from(origin, p, key, act);
            }
        }
        if let Some(v) = self.stopped_holders.remove(&p) {
            for (key, pass, stop) in v {
                let (p2, s2) = self.holders(p, key);
                self.edge_quiet(p2, pass);
                self.edge_quiet(s2, stop);
            }
        }
        if let Some(v) = self.stopped_shared.remove(&p) {
            for (key, dst) in v {
                let up = self.cv(p, key);
                self.edge(up, dst);
            }
        }
    }

    /// Accessors named `key` (any key: `Key::Any`) declared on `holder`:
    /// they run with `origin` as `this`, through the holder's group.
    pub fn accessor_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        let has = match key {
            Key::Named(a) => self.accessors.contains_key(&(holder, a)),
            Key::Any => self
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

    /// One lookup from `origin` reaches group (holder, key).
    pub(super) fn group_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        let g = &self.acc_groups[&(holder, key)];
        let (gt, ar, st, sp, live) = (g.gt, g.ar, g.st, g.sp, g.has_setter);
        match act {
            ChainAct::Load(dst) => {
                self.add_tok(gt, origin);
                self.edge(ar, dst);
            }
            ChainAct::Call(call) => {
                // `o.k()` where k is a getter: the getter runs, then its
                // result is called with `o` as `this`.
                self.add_tok(gt, origin);
                self.bind_via(ar, call, origin);
            }
            ChainAct::Store(src) => {
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
        self.acc_groups.insert(
            (holder, key),
            AccGroup {
                gt,
                ar,
                st,
                sp,
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
                if self.acc_groups.contains_key(&(holder, Key::Any)) {
                    self.group_link(holder, a);
                }
            }
            Key::Any => {
                let star = self.atom(crate::STAR_KEY);
                let n = self.accessor_keys.get(&holder).map_or(0, |v| v.len());
                for i in 0..n {
                    let a = self.accessor_keys[&holder][i];
                    if a == star || !self.accessors.contains_key(&(holder, a)) {
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
        let any = self.acc_groups[&(holder, Key::Any)].clone();
        let g = self.acc_groups[&(holder, Key::Named(a))].clone();
        self.edge_quiet(any.gt, g.gt);
        self.edge_quiet(g.ar, any.ar);
        self.edge_quiet(any.st, g.st);
        self.edge_quiet(any.sp, g.sp);
        if g.has_setter {
            self.group_live(holder, Key::Any);
        }
    }

    /// Accessor `f` hangs off group (holder, key).
    pub(super) fn group_connect(&mut self, holder: Tok, key: Key, f: Tok, setter: bool) {
        // DECISIONS 225: a lookup reached the accessor's group.
        if self.act.is_some() {
            let g = self.tok_fn(f);
            self.activate(g, "accessor");
        }
        let g = self.acc_groups[&(holder, key)].clone();
        // An accessor with a lexical `this` (arrow) never sees the
        // receiver, as for a call (`call_tok`).
        let this = (!self.funcs[self.tok_fn(f) as usize].lexical_this).then(|| self.this_any(f));
        if setter {
            if let Some(this) = this {
                self.edge_quiet(g.st, this);
            }
            self.group_bind(holder, key, f, true);
            self.group_live(holder, key);
            if self.acc_groups.contains_key(&(holder, Key::Any)) {
                self.group_live(holder, Key::Any);
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
        let f = self.tok_fn(ft);
        self.activate(f, "accessor");
        if self.diagnostic.is_some() {
            self.diagnostic_reach(f, "accessor", u32::MAX);
        }
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::Group(holder, key, ft, setter));
            return;
        }
        let (sp, ar) = {
            let g = &self.acc_groups[&(holder, key)];
            (g.sp, g.ar)
        };
        if self.summarized(f) {
            self.group_summarized(ft, sp, ar, setter);
            return;
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
        if self.acc_groups.contains_key(&(holder, k)) {
            self.group_connect(holder, k, f, setter);
        } else {
            let any_exists = self.acc_groups.contains_key(&(holder, Key::Any));
            if replay || any_exists {
                // Creating the group picks up every accessor there (this
                // one too) and links it under an existing Any group.
                self.acc_group(holder, k);
            }
            if replay {
                let visits: Vec<_> = self
                    .visits
                    .get(&holder)
                    .map(|v| v.iter().copied().filter(|&(_, kk, _)| kk == k).collect())
                    .unwrap_or_default();
                for (origin, _, act) in visits {
                    self.group_hit(origin, holder, k, act);
                }
            }
        }
        if replay && !self.acc_groups.contains_key(&(holder, Key::Any)) {
            let walks = self
                .any_walk_holders
                .get(&holder)
                .cloned()
                .unwrap_or_default();
            if !walks.is_empty() {
                self.acc_group(holder, Key::Any);
                for (origin, act) in walks {
                    self.group_hit(origin, holder, Key::Any, act);
                }
            }
        }
    }

    /// Bind `call` to every function that shows up in `callee`, with `this`
    /// fixed to `origin` (the relation `callthis callee call origin`).
    ///
    /// The rule splits in two so a megamorphic site does not pay
    /// receivers × methods per call: `this` binds once per (method node,
    /// receiver), shared by every site that looks the same key up on that
    /// receiver (`ThisTo`), and arguments/result bind once per (call,
    /// function) through the call's relay node (`CallArgs`). A builtin
    /// namespace value runs per (call, receiver) as before (`BuiltinCallee`).
    /// The result is the same as one `CallThis` per (callee, call, origin).
    pub(super) fn bind_via(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        if !self.callthis_done.insert((callee, call, origin)) {
            return;
        }
        self.rel(|| format!("callthis {callee} {call} {origin}"));
        self.via_calls
            .entry((callee, origin))
            .or_default()
            .push(call);
        if self.this_done.insert((callee, origin)) {
            self.cons(callee, Cons::ThisTo { origin });
        } else {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.work += toks.len() as u64;
            for &t in &toks {
                self.this_calls(t, origin, &[call]);
            }
            self.bufs.push(toks);
        }
        let relay = match self.relays.get(&call) {
            Some(&r) => r,
            None => {
                let r = self.node_l("callee-relay");
                self.nodes[r as usize].frame = self.call_run(call);
                self.relays.insert(call, r);
                self.cons(r, Cons::CallArgs { call });
                r
            }
        };
        if self.relay_done.insert((call, callee)) {
            self.edge_quiet(callee, relay);
        }
        let sites = self.callee_sites.entry(callee).or_default();
        let first = sites.is_empty();
        sites.push((call, origin));
        if first {
            self.cons(callee, Cons::BuiltinCallee);
        } else if self.builtin_callee.contains(&callee) {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.work += toks.len() as u64;
            for &t in &toks {
                if matches!(self.toks[t as usize], TokKind::Builtin(_)) {
                    self.call_tok(call, t, Some(origin));
                }
            }
            self.bufs.push(toks);
        }
    }

    /// The `this` half of `call_tok(_, t, Some(origin))`.
    pub(super) fn this_to(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => {
                let r = if self.explain {
                    let fs: Vec<String> = calls
                        .iter()
                        .map(|&c| {
                            let f = self.node_fn[self.calls[c as usize].ret as usize];
                            let p = if self.protocol_calls.contains(&c) {
                                " protocol"
                            } else {
                                ""
                            };
                            let name = self
                                .funcs
                                .get(f as usize)
                                .map_or("(no function)", |f| f.name.as_str());
                            format!("{name}#{c}{p}")
                        })
                        .collect();
                    let why = format!(
                        "receiver of a call of an unknown function value ({} callee, receiver {}, calls [{}])",
                        if t == TOP { "⊤" } else { "builtin-fn" },
                        self.tok_name(origin),
                        fs.join(", ")
                    );
                    self.reason(&why)
                } else {
                    self.reason("receiver of a call of an unknown function value")
                };
                self.escape(origin, r);
            }
            TokKind::Func(f, _) => {
                if !self.funcs[f as usize].lexical_this {
                    self.this_lookup(t, origin);
                }
            }
            _ => {}
        }
    }

    /// The instance half of `this_to`: each call bound through the method
    /// node binds its instance of `t` with this receiver.
    pub(super) fn this_calls(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            if !self.funcs[f as usize].lexical_this && self.summarized(f) {
                for &call in calls {
                    self.this_into(call, t, Some(origin));
                }
            }
        }
    }

    /// The argument half of `call_tok(call, t, Some(_))` (builtin namespace
    /// values excepted: they run per receiver, `BuiltinCallee`).
    pub(super) fn call_args(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => {
                if self.bind_done.insert((call, t, u32::MAX - 2)) {
                    self.unknown_callee_args(call, t, false);
                }
            }
            TokKind::Func(_, _) => self.bind_args(call, t),
            TokKind::Bound(_) => {
                if self.bind_done.insert((call, t, u32::MAX)) {
                    self.call_bound(call, t);
                }
            }
            _ => {}
        }
    }
}
