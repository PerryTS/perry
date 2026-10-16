//! The solver: inclusion constraints with difference propagation over a
//! worklist. Copy edges are plain successor lists; every other rule is a
//! complex constraint (`Cons`) re-applied to each node's new tokens.
//!
//! Soundness rests on two things: every rule over-approximates the concrete
//! transfer function of its construct, and every unknown value or operation
//! goes to ⊤ (`TOP` token for values coming from unmodeled code, `TopSink`
//! for values leaving into it). The rules here are the executable form of
//! design section 3.2; `scripts/wpfacts_recompute.py` re-implements them.

use crate::builtins;
use crate::model::*;

impl Model {
    // ---------------------------------------------------------------- nodes

    /// Add tokens/prims to a node; schedules it when anything is new.
    pub fn add(&mut self, n: NodeId, toks: &[Tok], prim: u16) {
        let node = &mut self.nodes[n as usize];
        let mut changed = false;
        if node.widened {
            // A widened set is {⊤}: every token reaching it escapes.
            for &t in toks {
                if t != TOP {
                    self.widen_queue.push(t);
                }
            }
        } else {
            let mut top_new = false;
            for &t in toks {
                if node.pts.insert(t) {
                    node.delta.push(t);
                    changed = true;
                    self.pts_entries += 1;
                    top_new |= t == TOP;
                }
            }
            if top_new && self.explain {
                self.top_pred
                    .entry(n)
                    .or_insert((self.cur_src, self.cur_why));
            }
            let node = &mut self.nodes[n as usize];
            if self.pts_entries & 0xfffff == 0 && self.pts_entries != 0 {
                if self
                    .budget
                    .is_some_and(|(start, limit)| start.elapsed() > limit)
                    || (self.max_pts != 0 && self.pts_entries > self.max_pts)
                {
                    self.aborted = true;
                }
            }
            if self.widen_k != 0 && node.pts.len() > self.widen_k {
                // Sound widening: the members escape (unknown code may do
                // anything with them) and the set becomes ⊤.
                node.widened = true;
                for t in node.pts.to_sorted() {
                    if t != TOP {
                        self.widen_queue.push(t);
                    }
                }
                node.pts = crate::pts::Pts::default();
                node.pts.insert(TOP);
                if self.explain {
                    let r = self.reason("widened (context-insensitive merge)");
                    self.top_pred.entry(n).or_insert((NO_NODE, r));
                }
                let node = &mut self.nodes[n as usize];
                node.delta.clear();
                node.delta.push(TOP);
                changed = true;
                self.widened_nodes += 1;
                if let Some(r) = self.relations.as_mut() {
                    r.push(format!("W widen {n}"));
                }
            }
        }
        let node = &mut self.nodes[n as usize];
        let newp = prim & !node.prim;
        if newp != 0 {
            node.prim |= newp;
            node.dprim |= newp;
            changed = true;
        }
        if changed && !node.in_wl {
            node.in_wl = true;
            self.wl.push_back(n);
        }
    }

    /// Sizes of the solver's structures (progress diagnostics).
    pub fn size_line(&self) -> String {
        let visits: usize = self.visits.values().map(|v| v.len()).sum();
        let cons: usize = self.nodes.iter().map(|n| n.cons.len()).sum();
        let succ: usize = self.nodes.iter().map(|n| n.succ.len()).sum();
        let pts: usize = self.nodes.iter().map(|n| n.pts.len()).sum();
        let delta: usize = self.nodes.iter().map(|n| n.delta.len()).sum();
        format!(
            "pops={} wl={} nodes={} cons={} succ={} pts={} delta={} calls={} stores={} toks={} tmpls={} bind={} chain={} callthis={} visits={} anywalks={} cv={} cp={} widenq={} widened={} escaped={} entries={}",
            self.pops,
            self.wl.len(),
            self.nodes.len(),
            cons,
            succ,
            pts,
            delta,
            self.calls.len(),
            self.stores.len(),
            self.toks.len(),
            self.tmpls.len(),
            self.bind_done.len(),
            self.chain_done.len(),
            self.callthis_done.len(),
            visits,
            self.any_walks.len(),
            self.cv_nodes.len(),
            self.cp_nodes.len(),
            self.widen_queue.len(),
            self.widened_nodes,
            self.escaped.len(),
            self.pts_entries
        )
    }

    /// Escape the tokens that reached widened sets.
    pub fn drain_widened(&mut self) {
        if self.widen_queue.is_empty() {
            return;
        }
        let r =
            self.reason("context-insensitive merge: a points-to set grew past the widening bound");
        while let Some(t) = self.widen_queue.pop() {
            self.escape(t, r);
        }
    }

    /// Seed a token (an input fact outside the solver rules: logged).
    pub fn add_tok(&mut self, n: NodeId, t: Tok) {
        if self.rel_phase != b'S' {
            self.rel(|| format!("addr {n} {t}"));
        }
        self.add(n, &[t], 0);
    }

    pub fn add_prim(&mut self, n: NodeId, p: u16) {
        if self.rel_phase != b'S' {
            self.rel(|| format!("prim {n} {p}"));
        }
        self.add(n, &[], p);
    }

    /// Seed ⊤ into `n` as an ORIGIN (not a propagation): `why` names the
    /// unmodeled thing; only interned in explain mode (diagnostics).
    pub fn add_top(&mut self, n: NodeId, why: impl FnOnce(&Self) -> String) {
        if self.explain {
            let w = why(self);
            let r = self.reason(&w);
            let saved = (self.cur_src, self.cur_why);
            self.cur_src = NO_NODE;
            self.cur_why = r;
            self.add_tok(n, TOP);
            (self.cur_src, self.cur_why) = saved;
        } else {
            self.add_tok(n, TOP);
        }
    }

    /// A node holding only ⊤ (all kinds) for one named unmodeled origin.
    pub fn top_val(&mut self, why: &str) -> NodeId {
        if !self.explain {
            return self.top_tok_node;
        }
        if let Some(&n) = self.top_vals.get(why) {
            return n;
        }
        let n = self.node_l("TOPVAL");
        self.add_top(n, |_| why.to_string());
        self.add_prim(n, prim::ALL);
        self.top_vals.insert(why.to_string(), n);
        n
    }

    /// Run `f` as a builtin summary: its relations are axioms for the
    /// independent checker.
    pub fn with_b<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let p = self.rel_phase;
        self.rel_phase = b'B';
        let r = f(self);
        self.rel_phase = p;
        r
    }

    fn with_s<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let p = self.rel_phase;
        self.rel_phase = b'S';
        let r = f(self);
        self.rel_phase = p;
        r
    }

    /// Copy edge a → b (pts(a) ⊆ pts(b)).
    pub fn edge(&mut self, a: NodeId, b: NodeId) {
        if a == b {
            return;
        }
        // Dedup against short successor lists only: a duplicate edge costs
        // a little propagation, a global edge set costs gigabytes.
        {
            let succ = &self.nodes[a as usize].succ;
            if succ.len() <= 16 && succ.contains(&b) {
                return;
            }
        }
        self.nodes[a as usize].succ.push(b);
        self.rel(|| format!("copy {a} {b}"));
        // Over a bound the program gets no facts: nothing more to propagate.
        if self.aborted {
            return;
        }
        let toks = self.nodes[a as usize].pts.to_sorted();
        let prim = self.nodes[a as usize].prim;
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = a;
        self.cur_why = u32::MAX;
        self.add(b, &toks, prim);
        (self.cur_src, self.cur_why) = saved;
    }

    /// Attach a complex constraint and apply it to what the node holds now.
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        self.stats_cons += 1;
        if self.relations.is_some() {
            let line = self.cons_line(n, &c);
            self.rel(|| line);
        }
        self.nodes[n as usize].cons.push(c.clone());
        if self.aborted {
            return;
        }
        let toks = self.nodes[n as usize].pts.to_sorted();
        let prim = self.nodes[n as usize].prim;
        if !toks.is_empty() || prim != 0 {
            let saved = (self.cur_src, self.cur_why);
            self.cur_src = n;
            self.cur_why = u32::MAX;
            self.with_s(|m| m.apply(n, &c, &toks, prim));
            (self.cur_src, self.cur_why) = saved;
        }
    }

    pub fn solve(&mut self) {
        self.rel_phase = b'S';
        loop {
            self.drain_widened();
            let Some(n) = self.wl.pop_front() else {
                break;
            };
            self.pops += 1;
            if self.aborted {
                return;
            }
            if self.profile.is_some() && self.pops & 0x3ffff == 0 {
                eprintln!("perry: wpfacts progress {}", self.size_line());
            }
            if self.pops & 0x3ff == 0 {
                let over_time = self
                    .budget
                    .is_some_and(|(start, limit)| start.elapsed() > limit);
                if over_time || (self.max_pts != 0 && self.pts_entries > self.max_pts) {
                    self.aborted = true;
                    return;
                }
            }
            let (delta, dprim) = {
                let node = &mut self.nodes[n as usize];
                node.in_wl = false;
                (
                    std::mem::take(&mut node.delta),
                    std::mem::take(&mut node.dprim),
                )
            };
            if delta.is_empty() && dprim == 0 {
                continue;
            }
            // Index loops (no clone of the lists): successors and
            // constraints appended while this runs already saw the full set.
            self.cur_src = n;
            self.cur_why = u32::MAX;
            let nsucc = self.nodes[n as usize].succ.len();
            let ncons0 = self.nodes[n as usize].cons.len();
            self.work += (delta.len() as u64 + 1) * (nsucc + ncons0) as u64;
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            for i in 0..nsucc {
                let s = self.nodes[n as usize].succ[i];
                self.add(s, &delta, dprim);
            }
            let ncons = self.nodes[n as usize].cons.len();
            for i in 0..ncons {
                let c = self.nodes[n as usize].cons[i].clone();
                self.cur_src = n;
                self.cur_why = u32::MAX;
                self.apply(n, &c, &delta, dprim);
            }
        }
        self.cur_src = NO_NODE;
        self.cur_why = u32::MAX;
    }

    // --------------------------------------------------------------- fields

    pub fn field(&mut self, t: Tok, a: Atom) -> NodeId {
        if let Some(&n) = self.fields.get(&(t, a)) {
            return n;
        }
        let n = self.node_named("field", || format!("field({t},{a})"));
        self.fields.insert((t, a), n);
        self.field_keys.entry(t).or_default().push(a);
        self.rel(|| format!("fieldnode {n} {t} {a}"));
        let any = self.field_any(t);
        self.edge(any, n);
        let all = self.all_fields(t);
        self.edge(n, all);
        n
    }

    pub fn field_any(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.field_any.get(&t) {
            return n;
        }
        let n = self.node_named("fieldany", || format!("fieldany({t})"));
        self.field_any.insert(t, n);
        self.rel(|| format!("fieldany {n} {t}"));
        let all = self.all_fields(t);
        self.edge(n, all);
        // Arrays and primitive boxes hold primitives at unknown indices.
        n
    }

    pub fn all_fields(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.all_fields.get(&t) {
            return n;
        }
        let n = self.node_named("allfields", || format!("allfields({t})"));
        self.all_fields.insert(t, n);
        self.rel(|| format!("allfields {n} {t}"));
        n
    }

    /// The node holding the prototype objects of `t`.
    pub fn proto_node(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.proto_node.get(&t) {
            return n;
        }
        let n = self.node_named("protos", || format!("protos({t})"));
        self.proto_node.insert(t, n);
        self.rel(|| format!("protonode {n} {t}"));
        // Seed the static part of the chain.
        let objproto = self.tok(TokKind::ObjectProto);
        match self.toks[t as usize].clone() {
            TokKind::Obj(tm) => match self.tmpls[tm as usize].kind.clone() {
                TmplKind::Class { .. } => {
                    let p = self.tok(TokKind::Proto(tm));
                    self.add_tok(n, p);
                }
                TmplKind::FnInst { func } => {
                    // `new f()` takes its prototype from `f.prototype`.
                    let ftok = self.funcs[func as usize].token;
                    let a = self.atom("prototype");
                    let fp = self.field(ftok, a);
                    self.edge(fp, n);
                }
                // Object.create / literal prototypes are added by gen.
                _ => self.add_tok(n, objproto),
            },
            TokKind::Proto(tm) => match (
                self.tmpls[tm as usize].parent,
                self.tmpls[tm as usize].native_parent.clone(),
            ) {
                (Some(p), _) => {
                    let pp = self.tok(TokKind::Proto(p));
                    self.add_tok(n, pp);
                }
                (None, Some((_, name))) => {
                    let np = crate::builtins::native_proto(self, &name);
                    self.add_tok(n, np);
                }
                (None, None) => self.add_tok(n, objproto),
            },
            TokKind::NativeProto(_) => self.add_tok(n, objproto),
            TokKind::Func(_) | TokKind::Bound(_) | TokKind::BuiltinFn => self.add_tok(n, objproto),
            TokKind::ClassObj(tm) => match self.tmpls[tm as usize].parent {
                Some(p) => {
                    let pc = self.tok(TokKind::ClassObj(p));
                    self.add_tok(n, pc);
                }
                None => self.add_tok(n, objproto),
            },
            _ => {}
        }
        n
    }

    fn fns_of(&self, n: NodeId) -> Vec<Tok> {
        self.nodes[n as usize].pts.to_sorted()
    }

    // ---------------------------------------------------------------- rules

    pub fn apply(&mut self, n: NodeId, c: &Cons, toks: &[Tok], prim: u16) {
        if self.profile.is_some() {
            let started = std::time::Instant::now();
            self.apply_inner(n, c, toks, prim);
            let name = c.kind_name();
            let e = self.profile.as_mut().unwrap().entry(name).or_default();
            e.0 += 1;
            e.1 += started.elapsed();
            return;
        }
        self.apply_inner(n, c, toks, prim);
    }

    fn apply_inner(&mut self, n: NodeId, c: &Cons, toks: &[Tok], prim: u16) {
        match *c {
            Cons::Load { key, dst } => {
                if prim != 0 {
                    self.with_b(|m| builtins::prim_load(m, prim, key, dst));
                }
                for &t in toks {
                    self.load_from(t, t, key, dst);
                }
            }
            Cons::Store { key, src, site } => {
                for &t in toks {
                    self.store_into(t, key, src, site);
                }
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
            Cons::MCall { key, call } => {
                if prim != 0 {
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
                for &t in toks {
                    self.escape(t, reason);
                }
            }
            Cons::Hard { kind, reason } => {
                for &t in toks {
                    self.mark_hard(t, kind, reason);
                }
            }
            Cons::AddsKeys => {
                for &t in toks {
                    if let Some(tm) = self.tmpl_of(t) {
                        *self.adds_keys.entry(tm).or_default() += 1;
                    }
                }
            }
            Cons::Implicit { reason } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        // A function handed to the runtime: it may be
                        // called back with anything.
                        TokKind::Func(_) | TokKind::Bound(_) => self.escape(t, reason),
                        _ => self.implicit(t, reason),
                    }
                }
            }
            Cons::CreateProto { reason } => {
                for &t in toks {
                    // E10: every template whose prototype this is can now
                    // have instances born without its constructor.
                    if let TokKind::Proto(tm) = self.toks[t as usize] {
                        let inst = self.tok(TokKind::Obj(tm));
                        self.mark_hard(inst, Hard::E10ObjectCreate, reason);
                    }
                }
            }
            Cons::ChainUnion { key, dst } => {
                for &q in toks {
                    if q == TOP {
                        self.add_tok(dst, TOP);
                        if self.is_constructor_key(key) {
                            let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                            self.add_tok(dst, f);
                        }
                        continue;
                    }
                    let f = match key {
                        Key::Named(a) => self.field(q, a),
                        Key::Any => self.all_fields(q),
                    };
                    self.edge(f, dst);
                    self.with_b(|m| builtins::tok_load(m, q, key, dst));
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
                    if t == TOP {
                        continue;
                    }
                    let parent = self.tmpls[class as usize].parent;
                    let native = self.tmpls[class as usize].native_parent.is_some();
                    match parent {
                        Some(p) => self.run_ctor_chain_this(call, p, t),
                        None if native => {
                            self.with_b(|m| crate::builtins::native_parent_ctor(m, class, call, t))
                        }
                        None => {}
                    }
                }
            }
            Cons::ThisFrom { recv } => {
                for &t in toks {
                    if let TokKind::Func(f) = self.toks[t as usize] {
                        if !self.funcs[f as usize].lexical_this {
                            let this = self.funcs[f as usize].this;
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
                        TokKind::Func(f) => {
                            self.cons(holder, Cons::AccessorInstall { key, f, setter });
                        }
                        TokKind::Top | TokKind::BuiltinFn | TokKind::Bound(_) => {
                            // An unknown accessor: reads of any key on the
                            // holder return ⊤ and stores escape.
                            let r = self.reason("accessor of unknown function value");
                            let fa = self.node_l("acc-unknown");
                            self.add_top(fa, |_| "accessor of unknown function value".into());
                            let site = self.anon_post_site("unknown accessor");
                            self.cons(
                                holder,
                                Cons::Store {
                                    key: Key::Any,
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
                    if t == TOP {
                        continue;
                    }
                    if key == star {
                        // A computed-key accessor: it may answer any key, so
                        // it runs as unknown code and every key read through
                        // this holder may yield anything (derive kills the
                        // templates that reach it).
                        let r = self.reason("accessor under a computed key");
                        let ft = self.funcs[f as usize].token;
                        self.escape(ft, r);
                        let fa = self.field_any(t);
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
                    if matches!(self.toks[t as usize], TokKind::ObjectProto) {
                        // Every lookup reaches Object.prototype: rather than
                        // replay them all, the derivation turns every fact
                        // off (an accessor on Object.prototype is E23).
                        self.objproto_accessor = true;
                        continue;
                    }
                    // Replay computed-key walks that reached this holder.
                    let walks: Vec<(NodeId, Tok, Tok, ChainAct)> = self
                        .any_walks
                        .iter()
                        .copied()
                        .filter(|&(cp, _, own, _)| {
                            own == t || self.nodes[cp as usize].pts.contains_tok(t)
                        })
                        .collect();
                    for (_, origin, _, act) in walks {
                        self.accessor_hit(origin, t, Key::Any, act);
                    }
                    // Replay lookups that already visited this holder.
                    let visits: Vec<_> = self
                        .visits
                        .get(&t)
                        .map(|v| v.iter().copied().collect())
                        .unwrap_or_default();
                    for (origin, k, act) in visits {
                        self.accessor_hit(origin, t, k, act);
                    }
                }
            }
            Cons::ProtoFrom { src } => {
                for &t in toks {
                    if t == TOP {
                        continue;
                    }
                    let pn = self.proto_node(t);
                    self.edge(src, pn);
                }
            }
            Cons::ProtoOf { dst } => {
                for &t in toks {
                    if t == TOP {
                        self.add_tok(dst, TOP);
                        continue;
                    }
                    let pn = self.proto_node(t);
                    self.edge(pn, dst);
                }
            }
            Cons::IterNext { dst } => {
                for &t in toks {
                    match self.toks[t as usize].clone() {
                        TokKind::Top => self.add_tok(dst, TOP),
                        TokKind::Func(_) | TokKind::Bound(_) => self.add_tok(dst, t),
                        TokKind::Obj(tm) => match self.tmpls[tm as usize].kind {
                            TmplKind::Arr { .. } | TmplKind::BObj { .. } => {}
                            _ => {
                                let a = self.atom("next");
                                self.load_from(t, t, Key::Named(a), dst);
                            }
                        },
                        TokKind::PrimBox => {}
                        _ => {
                            let a = self.atom("next");
                            self.load_from(t, t, Key::Named(a), dst);
                        }
                    }
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
                        self.cons(idx, Cons::IdxKey { obj: t, dst });
                    }
                }
            }
            Cons::IdxKey { obj, dst } => {
                let named = prim & (prim::STR | prim::SYMBOL) != 0 || !toks.is_empty();
                if named && self.idx_done.insert((n, obj, dst, 1)) {
                    self.load_from(obj, obj, Key::Any, dst);
                }
            }
            Cons::Arith { dst, add } => {
                let mut out = 0u16;
                if !toks.is_empty() {
                    // Objects go through ToPrimitive (user valueOf/toString).
                    out |= prim::NUM | prim::BIGINT | if add { prim::STR } else { 0 };
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
            Cons::ShareBuffer { ta } => {
                for &t in toks {
                    let shared = t == TOP
                        || matches!(self.toks[t as usize], TokKind::Obj(tm)
                            if matches!(self.tmpls[tm as usize].kind, TmplKind::BObj { what: "TypedArray", .. }));
                    if shared {
                        let r = self.reason("B1 typed array over a shared buffer");
                        self.mark_hard(ta, Hard::B1Exposed, r);
                    }
                }
            }
            Cons::NewTargetHard { reason } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        TokKind::ClassObj(tm) => {
                            let inst = self.tok(TokKind::Obj(tm));
                            self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                        }
                        TokKind::Func(f) => {
                            let tm = self.fn_inst_template(f);
                            let inst = self.tok(TokKind::Obj(tm));
                            self.mark_hard(inst, Hard::E11ReflectConstruct, reason);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// dst ⊇ value of `key` read on object `t` (receiver `origin`).
    pub fn load_from(&mut self, origin: Tok, t: Tok, key: Key, dst: NodeId) {
        if t == TOP {
            self.add_tok(dst, TOP);
            // Any function's `.constructor` is the Function constructor (E14).
            if self.is_constructor_key(key) {
                let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                self.add_tok(dst, f);
            }
            return;
        }
        // An absent key reads `undefined`: only a definite birth key of a
        // fact unit is known to be present.
        let present = match (key, self.tmpl_of(t)) {
            (Key::Named(a), Some(tm)) => {
                self.tmpls[tm as usize].kind.is_fact_unit()
                    && self.tmpls[tm as usize].definite.contains(&a)
            }
            _ => false,
        };
        if !present {
            self.add_prim(dst, prim::UNDEF);
        }
        match key {
            Key::Named(a) => {
                let f = self.field(t, a);
                self.edge(f, dst);
            }
            Key::Any => {
                let all = self.all_fields(t);
                self.edge(all, dst);
            }
        }
        self.with_b(|m| builtins::tok_load(m, t, key, dst));
        // Element reads of array-likes stop at their own elements: their
        // builtin prototype members are approximated by `tok_load`.
        if key == Key::Any && self.is_array_like(t) {
            return;
        }
        // The prototype chain's values, shared by every receiver of `t`.
        let cv = self.cv(t, key);
        self.edge(cv, dst);
        // Accessors run with the receiver as `this`: walk per receiver, only
        // for keys an accessor may be defined under.
        self.accessor_walk(origin, t, key, ChainAct::Load(dst));
    }

    /// Accessors run with the receiver as `this`. Named keys an accessor
    /// may be defined under walk the chain per receiver; computed keys
    /// visit, through the prototype closure, only holders with accessors.
    fn accessor_walk(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
        if !self.needs_accessor_walk(key) {
            return;
        }
        match key {
            Key::Named(_) => self.chain_from(origin, t, key, act),
            Key::Any => {
                self.accessor_hit(origin, t, Key::Any, act);
                let cp = self.cp(t);
                if self.any_walk_done.insert((cp, origin, act)) {
                    self.any_walks.push((cp, origin, t, act));
                    self.cons(cp, Cons::AnyAccessors { origin, act });
                }
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
        self.cons(pn, Cons::ChainUnion { key, dst: n });
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
    fn stops_at(&self, q: Tok, key: Key) -> bool {
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
            TokKind::PrimBox => true,
            TokKind::Obj(tm) => matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Arr { .. } | TmplKind::BObj { .. }
            ),
            _ => false,
        }
    }

    pub fn is_constructor_key(&self, key: Key) -> bool {
        match key {
            Key::Any => true,
            Key::Named(a) => self.atoms[a as usize] == "constructor",
        }
    }

    /// Start or continue a chain lookup at `t`'s prototypes.
    fn chain_from(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
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
    fn super_lookup(&mut self, origin: Tok, start: Tok, key: Key, act: ChainAct) {
        if start == TOP {
            self.chain_step(0, origin, TOP, key, act);
            return;
        }
        match act {
            ChainAct::Load(dst) => {
                let f = match key {
                    Key::Named(a) => self.field(start, a),
                    Key::Any => self.all_fields(start),
                };
                self.edge(f, dst);
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
                self.chain_step(0, origin, start, key, act)
            }
            Key::Any => self.accessor_walk(origin, start, key, act),
            _ => {}
        }
    }

    /// A prototype became HARD or escaped: lookups that stopped at it go on.
    fn resume_stopped(&mut self, p: Tok) {
        if let Some(v) = self.stopped.remove(&p) {
            for (origin, key, act) in v {
                self.chain_from(origin, p, key, act);
            }
        }
        if let Some(v) = self.stopped_shared.remove(&p) {
            for (key, dst) in v {
                let up = self.cv(p, key);
                self.edge(up, dst);
            }
        }
    }

    /// Accessors named `key` declared on `holder`: call them with `origin`.
    pub fn accessor_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        let keys: Vec<Atom> = match key {
            Key::Named(a) => vec![a],
            Key::Any => self.accessor_keys.get(&holder).cloned().unwrap_or_default(),
        };
        for a in keys {
            let Some((getters, setters)) = self.accessors.get(&(holder, a)).cloned() else {
                continue;
            };
            match act {
                ChainAct::Load(dst) => {
                    for g in getters {
                        let ret = self.funcs[g as usize].ret;
                        let this = self.funcs[g as usize].this;
                        self.add_tok(this, origin);
                        self.edge(ret, dst);
                    }
                }
                ChainAct::Store(src) => {
                    for s in setters {
                        let this = self.funcs[s as usize].this;
                        self.add_tok(this, origin);
                        if let Some(&p0) = self.funcs[s as usize].params.first() {
                            self.edge(src, p0);
                        }
                    }
                }
                ChainAct::Call(call) => {
                    // `o.k()` where k is a getter: the getter runs, then its
                    // result is called with `o` as `this`.
                    for g in getters {
                        let ret = self.funcs[g as usize].ret;
                        let this = self.funcs[g as usize].this;
                        self.add_tok(this, origin);
                        self.bind_via(ret, call, origin);
                    }
                }
            }
        }
    }

    /// Bind `call` to every function that shows up in `callee`, with `this`
    /// fixed to `origin`.
    fn bind_via(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        if self.callthis_done.insert((callee, call, origin)) {
            self.cons(callee, Cons::CallThis { call, this: origin });
        }
    }

    fn store_into(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) {
        if t == TOP {
            let r = self.reason("stored into a value from unmodeled code");
            self.sink_node(src, r);
            return;
        }
        let tm = self.tmpl_of(t);
        match key {
            Key::Named(a) => {
                let f = self.field(t, a);
                self.edge(src, f);
                if let Some(tm) = tm {
                    let ctor = self.stores[site as usize].birth || self.is_ctor_phase(site, tm);
                    let w = self.writers.entry((tm, a)).or_default();
                    if ctor {
                        if !w.ctor.contains(&site) {
                            w.ctor.push(site);
                        }
                    } else if !w.post.contains(&site) {
                        w.post.push(site);
                    }
                    if !self.tmpls[tm as usize].birth_keys.contains(&a) && !ctor {
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
                        *self.adds_keys.entry(tm).or_default() += 1;
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

    /// Is `site` a construction-phase writer for instances of `tm`?
    fn is_ctor_phase(&self, site: SiteId, tm: TmplId) -> bool {
        if let TmplKind::FnInst { func } = self.tmpls[tm as usize].kind {
            return self.stores[site as usize].ctor_fn == Some(func);
        }
        let Some(owner) = self.stores[site as usize].ctor_phase_of else {
            return false;
        };
        // The store sits in the constructor of `owner`; instances of `tm`
        // run it during construction when `owner` is `tm` or an ancestor.
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if c == owner {
                return true;
            }
            cur = self.tmpls[c as usize].parent;
        }
        false
    }

    /// Everything that reaches `n` escapes (once per node: no sink nodes).
    pub fn sink_node(&mut self, n: NodeId, reason: u32) {
        if self.sunk_nodes.insert(n) {
            self.cons(n, Cons::TopSink { reason });
        }
    }

    // ---------------------------------------------------------------- calls

    /// Bind `call` to callee token `t`. `this_tok` overrides the call's this.
    pub fn call_tok(&mut self, call: CallId, t: Tok, this_tok: Option<Tok>) {
        if matches!(self.toks[t as usize], TokKind::Top | TokKind::BuiltinFn) {
            // An unknown callee sees only its own receiver token: that one
            // escapes; arguments and result are per call.
            if let Some(x) = this_tok {
                let r = self.reason("receiver of a call of an unknown function value");
                self.escape(x, r);
            }
            if !self.bind_done.insert((call, t, u32::MAX - 2)) {
                return;
            }
            let c = self.calls[call as usize].clone();
            if t == TOP {
                self.top_calls += 1;
            }
            let r = if t == TOP {
                self.reason("passed to a call of an unknown function value")
            } else {
                self.reason("passed to an opaque builtin function value")
            };
            for &a in c.args.iter().chain(c.spread.iter()) {
                self.sink_node(a, r);
            }
            if this_tok.is_none() {
                match &c.this {
                    ThisArg::Node(n) => self.sink_node(*n, r),
                    ThisArg::Tok(x) => self.escape(*x, r),
                    ThisArg::Undef => {}
                }
            }
            if t == TOP {
                self.add_tok(c.ret, TOP);
            } else {
                self.add_top(c.ret, |_| {
                    "result of an opaque builtin function value".into()
                });
            }
            self.add_prim(c.ret, prim::ALL);
            return;
        }
        let key_this = this_tok.unwrap_or(u32::MAX);
        if !self.bind_done.insert((call, t, key_this)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        match self.toks[t as usize].clone() {
            TokKind::Top | TokKind::BuiltinFn => {
                if t == TOP {
                    self.top_calls += 1;
                }
                // Unknown callee: arguments and receiver escape, result ⊤.
                let r = if t == TOP {
                    self.reason("passed to a call of an unknown function value")
                } else {
                    self.reason("passed to an opaque builtin function value")
                };
                for &a in c.args.iter().chain(c.spread.iter()) {
                    self.sink_node(a, r);
                }
                match (this_tok, &c.this) {
                    (Some(x), _) => self.escape(x, r),
                    (None, ThisArg::Node(n)) => self.sink_node(*n, r),
                    (None, ThisArg::Tok(x)) => self.escape(*x, r),
                    (None, ThisArg::Undef) => {}
                }
                self.add_tok(c.ret, TOP);
                self.add_prim(c.ret, prim::ALL);
            }
            TokKind::Func(f) => {
                let fi = self.funcs[f as usize].clone();
                if !fi.lexical_this {
                    match (this_tok, &c.this) {
                        (Some(x), _) => self.add_tok(fi.this, x),
                        (None, ThisArg::Tok(x)) => self.add_tok(fi.this, *x),
                        (None, ThisArg::Node(n)) => self.edge(*n, fi.this),
                        (None, ThisArg::Undef) => self.add_prim(fi.this, prim::UNDEF),
                    }
                }
                // Arguments and result bind once per (call, function); only
                // `this` differs between receivers.
                if !self.bind_done.insert((call, t, u32::MAX - 1)) {
                    return;
                }
                for (i, &a) in c.args.iter().enumerate() {
                    if let Some(&p) = fi.params.get(i) {
                        self.edge(a, p);
                    } else if let Some(rest) = fi.rest {
                        let fa = self.field_any_node_of_node(rest);
                        self.edge(a, fa);
                    }
                    self.edge(a, fi.args_all);
                }
                // Missing arguments are undefined.
                for &p in fi.params.iter().skip(c.args.len()) {
                    self.add_prim(p, prim::UNDEF);
                }
                for &s in &c.spread {
                    // Spread arrays: their elements may be any parameter.
                    let elems = self.node_l("spread-elems");
                    self.cons(
                        s,
                        Cons::Load {
                            key: Key::Any,
                            dst: elems,
                        },
                    );
                    for &p in &fi.params {
                        self.edge(elems, p);
                    }
                    if let Some(rest) = fi.rest {
                        let fa = self.field_any_node_of_node(rest);
                        self.edge(elems, fa);
                    }
                    self.edge(elems, fi.args_all);
                }
                self.edge(fi.ret, c.ret);
            }
            TokKind::Bound(b) => {
                let (target, bthis, bargs) = self.bound[b as usize].clone();
                // Calling a bound function: its target with the bound this;
                // bound + call args land in any parameter (positions shift).
                let all_args = self.node_l("bound-args");
                for &a in bargs.iter().chain(c.args.iter()).chain(c.spread.iter()) {
                    self.edge(a, all_args);
                }
                let call2 = self.calls.len() as CallId;
                let mut args: Vec<NodeId> = bargs.clone();
                args.extend(c.args.iter().copied());
                self.calls.push(CallSite {
                    this: ThisArg::Node(bthis),
                    args,
                    spread: c.spread.clone(),
                    ret: c.ret,
                    site: c.site,
                });
                self.rel(|| format!("boundcall {call2} {call} {b}"));
                // Positions shift when the bound args are not a prefix of the
                // target's params; the conservative cover: every arg to every
                // param, done by spreading `all_args` as an array.
                let arr = self.node_l("bound-arr");
                let primbox = self.tok(TokKind::PrimBox);
                let _ = primbox;
                let holder = self.tok(TokKind::Bound(b));
                let fa = self.field_any(holder);
                self.edge(all_args, fa);
                self.add_tok(arr, holder);
                self.calls[call2 as usize].spread.push(arr);
                self.cons(target, Cons::Call { call: call2 });
            }
            TokKind::ClassObj(_) => {
                // Calling a class without `new` throws; nothing flows.
            }
            TokKind::Builtin(_) if self.protocol_calls.contains(&call) => {
                // The runtime's protocols (ToPrimitive, iteration, toJSON,
                // then) never call a constructor found under `constructor`.
            }
            TokKind::Builtin(ns) => self.with_b(|m| {
                let prev = std::mem::replace(&mut m.cur_this, this_tok);
                builtins::call_builtin_ns(m, ns, call);
                m.cur_this = prev;
            }),
            TokKind::Obj(_)
            | TokKind::Proto(_)
            | TokKind::ObjectProto
            | TokKind::PrimBox
            | TokKind::NativeProto(_)
            | TokKind::ModNs(_) => {
                // Not callable (TypeError) — unless it is a builtin object
                // that is a function (Proxy over a function etc., ⊤ anyway).
            }
        }
    }

    fn field_any_node_of_node(&mut self, arr_node: NodeId) -> NodeId {
        // Elements of whatever array the node holds: a store relay.
        let relay = self.node_named("elems-into", || format!("elems-into({arr_node})"));
        let site = self.anon_store_site();
        self.cons(
            arr_node,
            Cons::Store {
                key: Key::Any,
                src: relay,
                site,
            },
        );
        relay
    }

    pub fn anon_store_site(&mut self) -> SiteId {
        // A store performed by call binding (rest arrays): never a template
        // writer, because rest arrays are Arr templates.
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: 0,
            func: 0,
            desc: "binding".into(),
            ctor_phase_of: None,
            birth: true,
            ctor_fn: None,
        });
        id
    }

    /// `new t(args)`.
    pub fn construct(&mut self, call: CallId, t: Tok) {
        if !self.construct_done.insert((call, t)) {
            return;
        }
        let c = self.calls[call as usize].clone();
        match self.toks[t as usize].clone() {
            TokKind::ClassObj(tm) => {
                let inst = self.tok(TokKind::Obj(tm));
                self.add_tok(c.ret, inst);
                self.run_ctor_chain(call, tm, inst);
            }
            TokKind::Func(f) => {
                let tm = self.fn_inst_template(f);
                let inst = self.tok(TokKind::Obj(tm));
                self.add_tok(c.ret, inst);
                let fi = self.funcs[f as usize].clone();
                self.call_tok(call, t, Some(inst));
                // E12: a constructor function returning an object.
                self.ret_objects(fi.ret, c.ret);
            }
            TokKind::Top | TokKind::BuiltinFn => {
                if t == TOP {
                    self.top_calls += 1;
                }
                let r = self.reason("passed to `new` of an unknown constructor");
                for &a in c.args.iter().chain(c.spread.iter()) {
                    self.sink_node(a, r);
                }
                if t == TOP {
                    self.add_tok(c.ret, TOP);
                } else {
                    self.add_top(c.ret, |_| {
                        "`new` of an opaque builtin function value".into()
                    });
                }
            }
            TokKind::Bound(b) => {
                let (target, _bthis, bargs) = self.bound[b as usize].clone();
                let call2 = self.calls.len() as CallId;
                let mut args = bargs;
                args.extend(c.args.iter().copied());
                self.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args,
                    spread: c.spread.clone(),
                    ret: c.ret,
                    site: c.site,
                });
                self.cons(target, Cons::New { call: call2 });
            }
            TokKind::Builtin(ns) => self.with_b(|m| builtins::new_builtin_ns(m, ns, call)),
            _ => {}
        }
    }

    /// `super(...)` from a subclass constructor: the parent chain runs on
    /// the same instance; the `new` result is the subclass's.
    pub fn run_ctor_chain_this(&mut self, call: CallId, tm: TmplId, inst: Tok) {
        self.run_ctor_chain(call, tm, inst);
    }

    /// Run the constructor chain of class template `tm` for instance `inst`.
    pub fn run_ctor_chain(&mut self, call: CallId, tm: TmplId, inst: Tok) {
        let c = self.calls[call as usize].clone();
        if let Some(fi) = self.tmpls[tm as usize].field_init {
            let this = self.funcs[fi as usize].this;
            self.add_tok(this, inst);
        }
        let t = &self.tmpls[tm as usize];
        match (t.ctor, t.parent, t.native_parent.clone()) {
            (Some(ctor), _, _) => {
                let ftok = self.funcs[ctor as usize].token;
                self.call_tok(call, ftok, Some(inst));
                let ret = self.funcs[ctor as usize].ret;
                self.ret_objects(ret, c.ret);
            }
            (None, Some(p), _) => {
                // Implicit `constructor(...a) { super(...a) }`.
                self.run_ctor_chain(call, p, inst);
            }
            (None, None, Some(_)) => {
                self.with_b(|m| builtins::native_parent_ctor(m, tm, call, inst))
            }
            (None, None, None) => {}
        }
    }

    /// Objects a constructor returns replace `this` as the `new` result.
    pub fn ret_objects(&mut self, ret: NodeId, dst: NodeId) {
        // Object tokens only (a primitive return is ignored by `new`).
        let filt = self.node_named("objret", || format!("objret({ret})"));
        self.edge(ret, filt);
        // `filt` keeps prims; dst gets them too, which is a harmless
        // over-approximation for kinds only when the ctor returns
        // primitives — but it would pollute `new` results, so relay
        // tokens only through a Load-free copy: we accept the prims.
        self.edge(filt, dst);
    }

    pub fn fn_inst_template(&mut self, f: FnIdx) -> TmplId {
        if let Some(&tm) = self.fn_inst.get(&f) {
            return tm;
        }
        let name = self.funcs[f as usize].name.clone();
        let id = self.tmpls.len() as TmplId;
        let (birth_keys, definite) = self.fn_birth.get(&f).cloned().unwrap_or_default();
        self.tmpls.push(Template {
            kind: TmplKind::FnInst { func: f },
            display: format!("new {name}"),
            birth_keys,
            definite,
            parent: None,
            ctor: Some(f),
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
        });
        self.fn_inst.insert(f, id);
        id
    }

    /// `recv.key(args)` on token `t`.
    fn method_on(&mut self, t: Tok, key: Key, call: CallId) {
        if t == TOP {
            self.call_tok(call, TOP, Some(TOP));
            return;
        }
        // Own function-valued fields, then the prototype chain.
        let own = match key {
            Key::Named(a) => self.field(t, a),
            Key::Any => self.all_fields(t),
        };
        self.bind_via(own, call, t);
        self.with_b(|m| builtins::own_method(m, t, key, call));
        // Methods on the chain (shared), builtin prototype methods, and
        // getters returning functions (per receiver, accessor keys only).
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
        self.accessor_walk(t, t, key, ChainAct::Call(call));
    }

    // --------------------------------------------------------------- escape

    pub fn escape(&mut self, t: Tok, reason: u32) {
        if t == TOP || self.escaped.contains_key(&t) {
            return;
        }
        // Runtime-owned objects do not "escape": unknown code already holds
        // them. Object.prototype's unmodeled mutations are E5/E6 (modeled
        // operations) or E14 (interpreted code), never a side effect of some
        // value reaching unknown code; its user-stored members escape where
        // they are stored (`builtins::tok_store`).
        if matches!(
            self.toks[t as usize],
            TokKind::ObjectProto
                | TokKind::BuiltinFn
                | TokKind::Builtin(_)
                | TokKind::NativeProto(_)
        ) {
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("escapeseed {t} {reason}"));
        }
        self.with_s(|m| m.escape_inner(t, reason));
    }

    fn escape_inner(&mut self, t: Tok, reason: u32) {
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.escaped.insert(t, reason);
            self.resume_stopped(t);
        }
        self.escaped.insert(t, reason);
        self.rel(|| format!("escaped {t}"));
        // Unknown code reads every field and can write any key.
        let all = self.all_fields(t);
        self.cons(all, Cons::TopSink { reason });
        let any = self.field_any(t);
        self.add_top(any, |m| {
            format!(
                "field of escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        // Its prototypes are reachable (`Object.getPrototypeOf`).
        let pn = self.proto_node(t);
        self.cons(pn, Cons::TopSink { reason });
        // Own accessors run with `this` = ⊤-reached object: already modeled
        // by the accessor rule when unknown code reads; call them now.
        self.implicit(t, reason);
        match self.toks[t as usize].clone() {
            TokKind::Func(f) => {
                let fi = self.funcs[f as usize].clone();
                let why =
                    |m: &Self| format!("param of escaped function: {}", m.reasons[reason as usize]);
                for &p in fi.params.iter().chain(fi.rest.iter()) {
                    self.add_top(p, why);
                    self.add_prim(p, prim::ALL);
                }
                if let Some(rest) = fi.rest {
                    let r = self.field_any_node_of_node(rest);
                    self.add_top(r, why);
                }
                self.add_top(fi.args_all, why);
                if !fi.lexical_this {
                    self.add_top(fi.this, |m| {
                        format!("this of escaped function: {}", m.reasons[reason as usize])
                    });
                    self.add_prim(fi.this, prim::ALL);
                }
                self.sink_node(fi.ret, reason);
            }
            TokKind::ClassObj(tm) => {
                let p = self.tok(TokKind::Proto(tm));
                self.escape(p, reason);
                // Unknown code may construct it with unknown arguments.
                let inst = self.tok(TokKind::Obj(tm));
                let ret = self.node_l("top-new");
                let args_n = self.node_l("top-args");
                self.add_top(args_n, |_| "constructor args from unknown code".into());
                self.add_prim(args_n, prim::ALL);
                let call = self.calls.len() as CallId;
                self.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![args_n; 8],
                    spread: vec![],
                    ret,
                    site: 0,
                });
                self.construct(call, t);
                self.sink_node(ret, reason);
                self.escape(inst, reason);
            }
            TokKind::Obj(tm) => {
                // `.constructor` reaches the class value.
                if let TmplKind::Class { .. } = self.tmpls[tm as usize].kind {
                    let c = self.tok(TokKind::ClassObj(tm));
                    self.escape(c, reason);
                }
                if let TmplKind::FnInst { func } = self.tmpls[tm as usize].kind {
                    let ft = self.funcs[func as usize].token;
                    self.escape(ft, reason);
                }
            }
            TokKind::Proto(tm) => {
                let c = self.tok(TokKind::ClassObj(tm));
                // `.constructor` on the prototype.
                if matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
                    self.escape(c, reason);
                }
            }
            TokKind::Bound(b) => {
                let (target, bthis, bargs) = self.bound[b as usize].clone();
                for n in [target, bthis].into_iter().chain(bargs) {
                    self.sink_node(n, reason);
                }
            }
            _ => {}
        }
    }

    pub fn mark_hard(&mut self, t: Tok, kind: Hard, reason: u32) {
        if t == TOP {
            return;
        }
        if self.hard.contains_key(&t) {
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("hardseed {t} {} {reason}", kind.code()));
        }
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.hard.insert(t, (kind, reason));
            self.with_s(|m| m.resume_stopped(t));
        }
        self.hard.insert(t, (kind, reason));
        self.rel(|| format!("hardmark {t} {}", kind.code()));
        // Deleting / redefining can move values or install accessors: the
        // object's fields are now written by unknown code as well.
        let any = self.field_any(t);
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
    }

    /// Builtin protocol touch on `t` (ToPrimitive, iteration, toJSON, then,
    /// inspect …): every function reachable as a property of `t` or its
    /// chain runs with `this` = `t` and unknown primitive arguments.
    pub fn implicit(&mut self, t: Tok, reason: u32) {
        if t == TOP || !self.implicit_done.insert(t) {
            return;
        }
        let _ = reason;
        let ret = self.node_named("implicit-ret", || format!("implicit-ret({t})"));
        let arg = self.node_l("implicit-arg");
        self.add_prim(arg, prim::ALL);
        // Protocol arguments are primitives (hints, keys). The one protocol
        // that hands user code runtime functions, `then(resolve, reject)`,
        // is modeled where it happens (`builtins::adopt`): the resolvers
        // there are synthetic functions whose argument reaches the result.
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Tok(t),
            args: vec![arg; 4],
            spread: vec![],
            ret,
            site: 0,
        });
        self.protocol_calls.insert(call);
        self.rel(|| format!("implicitcall {call} {t} {ret} {arg}"));
        self.method_on(t, Key::Any, call);
        // Results of protocol methods flow back to the runtime (iterator
        // results, ToPrimitive values, then-ables): treat as touched too.
        self.cons(ret, Cons::Implicit { reason });
    }

    pub fn cons_line(&self, n: NodeId, c: &Cons) -> String {
        match c {
            Cons::Load { key, dst } => format!("load {n} {} {dst}", self.key_name(*key)),
            Cons::Store { key, src, site } => {
                format!("store {n} {} {src} {site}", self.key_name(*key))
            }
            Cons::Call { call } => format!("call {n} {call}"),
            Cons::CallThis { call, this } => format!("callthis {n} {call} {this}"),
            Cons::New { call } => format!("new {n} {call}"),
            Cons::MCall { key, call } => format!("mcall {n} {} {call}", self.key_name(*key)),
            Cons::Chain { origin, key, act } => {
                format!("chain {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::TopSink { reason } => format!("top {n} {reason}"),
            Cons::Hard { kind, reason } => format!("hard {n} {} {reason}", kind.code()),
            Cons::AddsKeys => format!("addskeys {n}"),
            Cons::Implicit { reason } => format!("implicit {n} {reason}"),
            Cons::CreateProto { reason } => format!("createproto {n} {reason}"),
            Cons::ChainUnion { key, dst } => {
                format!("chainunion {n} {} {dst}", self.key_name(*key))
            }
            Cons::ProtoClosure { dst } => format!("protoclosure {n} {dst}"),
            Cons::ProtoMethods { origin, key, call } => {
                format!("protomethods {n} {origin} {} {call}", self.key_name(*key))
            }
            Cons::AnyAccessors { origin, act } => format!("anyaccessors {n} {origin} {act:?}"),
            Cons::StoreTopProto { origin, src } => format!("storetopproto {n} {origin} {src}"),
            Cons::Super { start, key, act } => {
                format!("super {n} {start} {} {act:?}", self.key_name(*key))
            }
            Cons::SuperCtor { class, call } => format!("superctor {n} {class} {call}"),
            Cons::ThisFrom { recv } => format!("thisfrom {n} {recv}"),
            Cons::Accessor {
                holder,
                key,
                setter,
            } => {
                format!(
                    "accessor {n} {holder} {} {}",
                    self.key_name(Key::Named(*key)),
                    *setter as u8
                )
            }
            Cons::AccessorInstall { key, f, setter } => {
                format!(
                    "accinstall {n} {} {f} {}",
                    self.key_name(Key::Named(*key)),
                    *setter as u8
                )
            }
            Cons::ProtoFrom { src } => format!("protofrom {n} {src}"),
            Cons::ProtoOf { dst } => format!("protoof {n} {dst}"),
            Cons::IterNext { dst } => format!("iternext {n} {dst}"),
            Cons::NewTargetHard { reason } => format!("newtargethard {n} {reason}"),
            Cons::ShareBuffer { ta } => format!("sharebuffer {n} {ta}"),
            Cons::Arith { dst, add } => format!("arith {n} {dst} {}", *add as u8),
            Cons::IdxLoad { idx, dst } => format!("idxload {n} {idx} {dst}"),
            Cons::IdxKey { obj, dst } => format!("idxkey {n} {obj} {dst}"),
        }
    }

    /// One word for a token's kind (diagnostics).
    pub fn tok_kind_word(&self, t: Tok) -> &'static str {
        match &self.toks[t as usize] {
            TokKind::Obj(tm) => match self.tmpls[*tm as usize].kind {
                TmplKind::Class { .. } => "class instance",
                TmplKind::Lit { .. } => "literal",
                TmplKind::FnInst { .. } => "fn instance",
                TmplKind::Arr { .. } => "array",
                TmplKind::ObjCreate { .. } => "Object.create object",
                TmplKind::BObj { .. } => "builtin object",
            },
            TokKind::Func(_) => "function",
            TokKind::Bound(_) => "bound function",
            TokKind::ClassObj(_) => "class",
            TokKind::Proto(_) => "prototype",
            TokKind::ModNs(_) => "module namespace",
            _ => "value",
        }
    }

    pub fn fn_tokens(&self, n: NodeId) -> Vec<Tok> {
        self.fns_of(n)
    }
}
