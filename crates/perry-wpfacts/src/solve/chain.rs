//! The prototype-chain walk: shared walk acts and holders, chain steps
//! from a receiver and from `super`, and resuming walks a stop held.

use crate::builtins;
use crate::model::*;

impl Model {
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
        let store = matches!(act, ChainAct::Store(_, _));
        let kind: u8 = match act {
            ChainAct::Load(_, _) => 0,
            ChainAct::Store(_, _) => 1,
            ChainAct::Call(_) => 2,
            ChainAct::Lookup { setter, .. } => 3 + u8::from(setter),
        };
        let ((n, rn), fresh) = match self.walk_nodes.get(&(origin, t, key, kind)) {
            Some(&w) => (w, false),
            None => {
                let n = self.node_l(
                    [
                        "walk-load",
                        "walk-store",
                        "walk-call",
                        "walk-getter",
                        "walk-setter",
                    ][kind as usize],
                );
                if kind == 1 {
                    self.walk_store_pending.insert(n, Vec::new());
                }
                // What the walk's accessors throw reaches each site's region.
                let rn = self.node_l("walk-raise");
                self.walk_nodes.insert((origin, t, key, kind), (n, rn));
                ((n, rn), true)
            }
        };
        let region = match act {
            ChainAct::Load(_, r) | ChainAct::Store(_, r) => r,
            ChainAct::Call(call) => self.calls[call as usize].region,
            ChainAct::Lookup { .. } => NO_NODE,
        };
        if region != NO_NODE {
            self.edge(rn, region);
        }
        match act {
            ChainAct::Load(dst, _) | ChainAct::Lookup { dst, .. } => {
                if dst == self.esc_accessors {
                    if let Some(d) = &mut self.diagnostic {
                        d.acc_src.entry(n).or_insert(origin);
                    }
                }
                self.edge(n, dst)
            }
            ChainAct::Store(src, _) => match self.walk_store_pending.get_mut(&n) {
                Some(v) => v.push(src),
                None => self.edge(src, n),
            },
            ChainAct::Call(call) => self.bind_via(n, call, origin),
        }
        // A call walk collects getter results as a load does.
        fresh.then_some(match act {
            ChainAct::Lookup { setter, .. } => ChainAct::Lookup { dst: n, setter },
            _ if store => ChainAct::Store(n, rn),
            _ => ChainAct::Load(n, rn),
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
            Key::Named(a) => {
                self.accessor_atoms.contains(&a)
                    || self
                        .atom_index
                        .get(crate::STAR_KEY)
                        .is_some_and(|a| self.accessor_atoms.contains(a))
            }
            Key::Any(_) => !self.accessor_atoms.is_empty(),
        }
    }

    /// The values `key` takes on `t`'s prototypes (transitively).
    pub fn cv(&mut self, t: Tok, key: Key) -> NodeId {
        self.cv_of(t, key, true)
    }

    /// `cv`, with the builtin prototypes' members or (`builtin` false)
    /// with the program prototypes' values only.
    pub fn cv_of(&mut self, t: Tok, key: Key, builtin: bool) -> NodeId {
        self.cv_recv(t, t, key, builtin)
    }

    /// `cv_of` of `t` read with receiver `origin` (rule PXRECV: a Proxy's
    /// forward): a Proxy on `t`'s chain is passed `origin`.
    pub fn cv_recv(&mut self, origin: Tok, t: Tok, key: Key, builtin: bool) -> NodeId {
        self.note_chain_recv_at(t, origin);
        self.cv_up(t, key, builtin)
    }

    /// `cv_of` of a prototype `t` on a receiver's chain (the next link of a
    /// `ChainUnion`): `t` is no receiver of it.
    pub(super) fn cv_up(&mut self, t: Tok, key: Key, builtin: bool) -> NodeId {
        if let Some(&n) = self.cv_nodes.get(&(t, key, builtin)) {
            return n;
        }
        let n = self.node_l("chainval");
        self.cv_nodes.insert((t, key, builtin), n);
        let pn = self.proto_node(t);
        self.cons(
            pn,
            Cons::ChainUnion {
                key,
                dst: n,
                of: t,
                builtin,
            },
        );
        n
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
    pub fn chain_step(&mut self, pn: NodeId, origin: Tok, p: Tok, key: Key, act: ChainAct) {
        if let ChainAct::Lookup { dst, setter } = act {
            self.lookup_holder(origin, p, key, dst, setter);
            return;
        }
        if p == TOP {
            match act {
                ChainAct::Load(dst, _) | ChainAct::Lookup { dst, .. } => self.add_tok(dst, TOP),
                ChainAct::Call(call) => self.call_tok(call, TOP, Some(origin)),
                ChainAct::Store(src, _) => {
                    self.store_live(src);
                    if let Some(d) = &mut self.diagnostic {
                        d.store_top.entry(src).or_insert(pn);
                    }
                    let r = self.reason("store through an unknown prototype chain");
                    self.sink_node(src, r);
                }
            }
            return;
        }
        for x in self.class_id_holders(origin, p) {
            self.accessor_hit(origin, x, key, act);
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

    /// The class-id holders of a lookup on `q` for `of` (the receiver, or
    /// the prototype the lookup came from): a specialization's instances
    /// have its origin's prototype object `q` as [[Prototype]], one object
    /// (`js_register_class_generic_origin`), and a call or accessor access
    /// there for an instance of the specialization (or of a class derived
    /// from one) runs the specialization's member by class id (codegen
    /// `static_prototype::chain`): `Proto` of each such specialization,
    /// which holds its declared members and is no object's prototype.
    pub(crate) fn class_id_holders(&mut self, of: Tok, q: Tok) -> Vec<Tok> {
        let mut out = Vec::new();
        if self.constructor_origins.is_empty() {
            return out;
        }
        let TokKind::Proto(o) = self.toks[q as usize] else {
            return out;
        };
        let starts: Vec<TmplId> = match self.toks[of as usize] {
            TokKind::Obj(c, _) if matches!(self.tmpls[c as usize].kind, TmplKind::Class { .. }) => {
                vec![c]
            }
            TokKind::Proto(c) => vec![c],
            _ => self.virt_protos.get(&of).cloned().unwrap_or_default(),
        };
        for c in starts {
            let mut cur = Some(c);
            for _ in 0..self.tmpls.len() {
                let Some(x) = cur else { break };
                if x != o && self.constructor_origin(x) == o {
                    let t = self.tok(TokKind::Proto(x));
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
                cur = self.tmpls[x as usize].parent;
            }
        }
        out
    }

    /// `super.key` from receiver `origin`: the lookup starts at `start`.
    pub(super) fn super_lookup(&mut self, origin: Tok, start: Tok, key: Key, act: ChainAct) {
        // The Receiver of `super.key` is `this` (13.3.7.3 MakeSuperPropertyReference).
        self.note_chain_recv(origin);
        if start == TOP {
            self.chain_step(0, origin, TOP, key, act);
            return;
        }
        // Rule PXSUPER: a Proxy home prototype runs its [[Get]] / [[Set]]
        // with Receiver `this` (`proxy_super`).
        if let TokKind::Proxy(p) = self.toks[start as usize] {
            return self.proxy_super(p, origin, key, act);
        }
        match act {
            ChainAct::Load(dst, _) if self.is_array_length(start, key) => {
                self.add_prim(dst, prim::NUM);
            }
            ChainAct::Load(dst, _) => {
                let f = match key {
                    Key::Named(a) => self.field_r(start, a),
                    Key::Any(_) => self.all_fields(start),
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
                    Key::Named(a) => self.field_r(start, a),
                    Key::Any(_) => self.all_fields(start),
                };
                self.bind_via(f, call, origin);
                // The method found on `start` runs in its native frame, as
                // the dispatch of the value `bind_via` hands the call does
                // (`Model::native_frame`): whichever of the two runs first,
                // what it hands to unknown code raises into the call's region.
                let native = builtins::proto_native(self, start, key);
                self.with_native(call, |m, call| {
                    m.native_frame(call, native, |m| {
                        builtins::proto_method(m, start, origin, key, call)
                    })
                });
                if !self.stops_at(start, key) {
                    let up = self.cv(start, key);
                    self.bind_via(up, call, origin);
                    let cp = self.cp(start);
                    self.cons(cp, Cons::ProtoMethods { origin, key, call });
                }
            }
            ChainAct::Lookup { .. } => {
                self.chain_step(0, origin, start, key, act);
                return;
            }
            ChainAct::Store(_, _) => {}
        }
        if !self.needs_accessor_walk(key) {
            self.visits
                .entry(start)
                .or_default()
                .insert((origin, key, act));
        }
        match key {
            Key::Named(_) if self.needs_accessor_walk(key) => {
                // `start` itself is a prototype: a lookup can stop there.
                if let Some(act) = self.shared_act(origin, start | SUPER_BIT, key, act) {
                    self.holder_walk(origin, start, key, act, true)
                }
            }
            Key::Any(_) => self.accessor_walk(origin, start, key, act),
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
            for (key, dst, builtin) in v {
                let up = self.cv_up(p, key, builtin);
                self.edge(up, dst);
            }
        }
    }
}
