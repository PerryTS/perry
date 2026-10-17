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
                if t != TOP && !self.is_escaped(t) {
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
            if self.widen_k != 0 && node.pts.len() > self.widen_k && !node.internal {
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
        let rss_mb = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|t| t.split_whitespace().nth(1)?.parse::<u64>().ok())
            .map(|p| p * 4096 / (1 << 20))
            .unwrap_or(0);
        let succ_cap: usize = self.nodes.iter().map(|n| n.succ.capacity()).sum();
        let delta_cap: usize = self.nodes.iter().map(|n| n.delta.capacity()).sum();
        let big: usize = self
            .nodes
            .iter()
            .filter(|n| matches!(n.pts, crate::pts::Pts::Big(_)))
            .count();
        let mut hubs: Vec<(usize, usize)> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.succ.len(), i))
            .filter(|x| x.0 > 100_000)
            .collect();
        hubs.sort_unstable_by(|a, b| b.cmp(a));
        let max_succ: Vec<String> = hubs
            .iter()
            .take(6)
            .map(|&(l, i)| {
                let n = &self.nodes[i];
                format!(
                    "{}#{i}:{l}:w{}:p{}:n{}:top{}",
                    self.labels[i],
                    n.widened as u8,
                    n.prim,
                    n.pts.len(),
                    n.pts.contains_tok(TOP) as u8
                )
            })
            .collect();
        let max_succ = max_succ.join(",");
        let stopped: usize = self.stopped.values().map(|v| v.len()).sum();
        let csites: usize = self.callee_sites.values().map(|v| v.len()).sum();
        format!(
            "rss_mb={rss_mb} succ_cap={succ_cap} delta_cap={delta_cap} big={big} max_succ={max_succ} stopped={stopped} csites={csites} walks={} relays={} pops={} wl={} nodes={} cons={} succ={} pts={} delta={} calls={} stores={} toks={} tmpls={} bind={} chain={} callthis={} visits={} anywalks={} cv={} cp={} widenq={} widened={} escaped={} entries={}",
            self.walk_nodes.len(),
            self.relays.len(),
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
        self.edge_impl(a, b, true);
    }

    /// `edge` without a relation line: internal relays the checker derives
    /// from a logged rule (`bind_via`).
    fn edge_quiet(&mut self, a: NodeId, b: NodeId) {
        self.edge_impl(a, b, false);
    }

    fn edge_impl(&mut self, a: NodeId, b: NodeId, log: bool) {
        if a == b {
            return;
        }
        // Dedup against short successor lists only: a duplicate edge costs
        // a little propagation, a global edge set costs gigabytes.
        let b_widened = self.nodes[b as usize].widened;
        {
            let node = &mut self.nodes[a as usize];
            match &mut node.succ_set {
                Some(set) => {
                    if !set.insert(b) {
                        return;
                    }
                }
                None => {
                    if node.succ.contains(&b) {
                        return;
                    }
                    if node.succ.len() >= 16 {
                        let mut set: crate::pts::FxHashSet<NodeId> =
                            node.succ.iter().copied().collect();
                        set.insert(b);
                        node.succ_set = Some(Box::new(set));
                    }
                }
            }
            if node.widened || !b_widened {
                node.succ.push(b);
            } else {
                // Into a widened node: escape at the source, carry prims.
                node.esc_out = true;
                node.prim_succ.push(b);
            }
        }
        if log {
            self.rel(|| format!("copy {a} {b}"));
        }
        // Over a bound the program gets no facts: nothing more to propagate.
        if self.aborted {
            return;
        }
        let prim = self.nodes[a as usize].prim;
        if self.nodes[a as usize].pts.is_empty() && prim == 0 {
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[a as usize].pts.sorted_into(&mut toks);
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = a;
        self.cur_why = u32::MAX;
        self.add(b, &toks, prim);
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    /// Attach a complex constraint and apply it to what the node holds now.
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        self.stats_cons += 1;
        if self.relations.is_some()
            && !matches!(
                c,
                Cons::ThisTo { .. }
                    | Cons::CallArgs { .. }
                    | Cons::BuiltinCallee
                    | Cons::HolderStep { .. }
                    | Cons::VisitHolders { .. }
                    | Cons::VisitStop { .. }
            )
        {
            let line = self.cons_line(n, &c);
            self.rel(|| line);
        }
        self.nodes[n as usize].cons.push(c);
        if self.aborted {
            return;
        }
        let prim = self.nodes[n as usize].prim;
        if self.nodes[n as usize].pts.is_empty() && prim == 0 {
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[n as usize].pts.sorted_into(&mut toks);
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = n;
        self.cur_why = u32::MAX;
        self.with_s(|m| m.apply(n, &c, &toks, prim));
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
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
            if self.profile.is_some() {
                let e = self.label_work.entry(self.labels[n as usize]).or_default();
                e.0 += delta.len() as u64 * nsucc as u64;
                e.1 += delta.len() as u64 * ncons0 as u64;
                e.2 += 1;
            }
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            if self.nodes[n as usize].esc_out {
                for &t in &delta {
                    if t != TOP && !self.is_escaped(t) {
                        self.widen_queue.push(t);
                    }
                }
                if dprim != 0 {
                    for i in 0..self.nodes[n as usize].prim_succ.len() {
                        let s = self.nodes[n as usize].prim_succ[i];
                        self.add(s, &[], dprim);
                    }
                }
            }
            let mut i = 0;
            while i < self.nodes[n as usize].succ.len() {
                let s = self.nodes[n as usize].succ[i];
                if self.nodes[s as usize].widened && !self.nodes[n as usize].widened {
                    // A widened successor holds ⊤ and escapes what reaches
                    // it: this node escapes its tokens itself from now on,
                    // and the edge carries only primitive kinds.
                    self.add(s, &delta, dprim);
                    let node = &mut self.nodes[n as usize];
                    node.succ.swap_remove(i);
                    if let Some(set) = node.succ_set.as_mut() {
                        set.remove(&s);
                    }
                    node.esc_out = true;
                    node.prim_succ.push(s);
                    continue;
                }
                self.add(s, &delta, dprim);
                i += 1;
            }
            let ncons = self.nodes[n as usize].cons.len();
            for i in 0..ncons {
                let c = self.nodes[n as usize].cons[i];
                self.cur_src = n;
                self.cur_why = u32::MAX;
                self.apply(n, &c, &delta, dprim);
            }
            // Hand the delta's storage back to the node (no reallocation).
            let mut delta = delta;
            delta.clear();
            let node = &mut self.nodes[n as usize];
            if node.delta.is_empty() && delta.capacity() <= 256 {
                node.delta = delta;
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
                    let objproto = matches!(self.toks[t as usize], TokKind::ObjectProto);
                    if objproto {
                        // Every lookup reaches Object.prototype: rather than
                        // replay them all, the derivation turns every fact
                        // off (an accessor on Object.prototype is E23).
                        self.objproto_accessor = true;
                    }
                    self.accessor_installed(t, key, f, setter, !objproto);
                }
            }
            Cons::ProtoFrom { src } => {
                for &t in toks {
                    // `%Object.prototype%` is an immutable prototype exotic
                    // object: setting its [[Prototype]] to anything but its
                    // own (null) throws.
                    if t == TOP || matches!(self.toks[t as usize], TokKind::ObjectProto) {
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
            Cons::ThisTo { origin } => {
                for &t in toks {
                    self.this_to(t, origin);
                }
            }
            Cons::CallArgs { call } => {
                for &t in toks {
                    self.call_args(call, t);
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
    fn shared_act(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) -> Option<ChainAct> {
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
                // A call walk relays getter results to calls: never widened.
                // A load walk's members all reach every site's result, and a
                // store walk's every setter's parameter, which would widen
                // past the bound anyway: those widen like any value node.
                match kind {
                    1 => {
                        self.walk_store_pending.insert(n, Vec::new());
                    }
                    2 => self.nodes[n as usize].internal = true,
                    _ => {}
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
    fn holders(&mut self, t: Tok, key: Key) -> (NodeId, NodeId) {
        if let Some(&x) = self.holder_nodes.get(&(t, key)) {
            return x;
        }
        let p = self.node_l("holders");
        let s = self.node_l("holders-stop");
        self.nodes[p as usize].internal = true;
        self.nodes[s as usize].internal = true;
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
    fn holder_walk(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct, at_start: bool) {
        let (p, s) = if at_start {
            let p = self.node_l("holders");
            let s = self.node_l("holders-stop");
            self.nodes[p as usize].internal = true;
            self.nodes[s as usize].internal = true;
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
    fn store_live(&mut self, n: NodeId) {
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
    fn resume_stopped(&mut self, p: Tok) {
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
    fn group_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
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
    fn acc_group(&mut self, holder: Tok, key: Key) {
        if self.acc_groups.contains_key(&(holder, key)) {
            return;
        }
        let gt = self.node_l("acc-this");
        let ar = self.node_l("acc-ret");
        let st = self.node_l("acc-setthis");
        // The setter parameter source widens like the parameter it feeds.
        let sp = self.node_l("acc-setarg");
        for n in [gt, ar, st] {
            self.nodes[n as usize].internal = true;
        }
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
    fn group_link(&mut self, holder: Tok, a: Atom) {
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
    fn group_connect(&mut self, holder: Tok, key: Key, f: FnIdx, setter: bool) {
        let g = self.acc_groups[&(holder, key)].clone();
        // An accessor with a lexical `this` (arrow) never sees the
        // receiver, as for a call (`call_tok`).
        let this = (!self.funcs[f as usize].lexical_this).then(|| self.funcs[f as usize].this);
        if setter {
            if let Some(this) = this {
                self.edge_quiet(g.st, this);
            }
            if let Some(&p0) = self.funcs[f as usize].params.first() {
                self.edge_quiet(g.sp, p0);
            }
            self.group_live(holder, key);
            if self.acc_groups.contains_key(&(holder, Key::Any)) {
                self.group_live(holder, Key::Any);
            }
        } else {
            if let Some(this) = this {
                self.edge_quiet(g.gt, this);
            }
            let ret = self.funcs[f as usize].ret;
            self.edge_quiet(ret, g.ar);
        }
    }

    /// A group has a setter: its waiting store sources flow from now on.
    fn group_live(&mut self, holder: Tok, key: Key) {
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
    fn accessor_installed(&mut self, holder: Tok, key: Atom, f: FnIdx, setter: bool, replay: bool) {
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
    fn bind_via(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        if !self.callthis_done.insert((callee, call, origin)) {
            return;
        }
        self.rel(|| format!("callthis {callee} {call} {origin}"));
        if self.this_done.insert((callee, origin)) {
            self.cons(callee, Cons::ThisTo { origin });
        }
        let relay = match self.relays.get(&call) {
            Some(&r) => r,
            None => {
                let r = self.node_l("callee-relay");
                self.nodes[r as usize].internal = true;
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
            for &t in &toks {
                if matches!(self.toks[t as usize], TokKind::Builtin(_)) {
                    self.call_tok(call, t, Some(origin));
                }
            }
            self.bufs.push(toks);
        }
    }

    /// The `this` half of `call_tok(_, t, Some(origin))`.
    fn this_to(&mut self, t: Tok, origin: Tok) {
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => {
                let r = self.reason("receiver of a call of an unknown function value");
                self.escape(origin, r);
            }
            TokKind::Func(f) => {
                if !self.funcs[f as usize].lexical_this {
                    let this = self.funcs[f as usize].this;
                    self.add_tok(this, origin);
                }
            }
            _ => {}
        }
    }

    /// The argument half of `call_tok(call, t, Some(_))` (builtin namespace
    /// values excepted: they run per receiver, `BuiltinCallee`).
    fn call_args(&mut self, call: CallId, t: Tok) {
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => {
                if self.bind_done.insert((call, t, u32::MAX - 2)) {
                    self.unknown_callee_args(call, t, false);
                }
            }
            TokKind::Func(_) => self.bind_args(call, t),
            TokKind::Bound(_) => {
                if self.bind_done.insert((call, t, u32::MAX)) {
                    self.call_bound(call, t);
                }
            }
            _ => {}
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
            self.unknown_callee_args(call, t, this_tok.is_none());
            return;
        }
        let key_this = this_tok.unwrap_or(u32::MAX);
        if !self.bind_done.insert((call, t, key_this)) {
            return;
        }
        match self.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn => unreachable!(),
            TokKind::Func(f) => {
                if !self.funcs[f as usize].lexical_this {
                    let fthis = self.funcs[f as usize].this;
                    match (this_tok, &self.calls[call as usize].this) {
                        (Some(x), _) => self.add_tok(fthis, x),
                        (None, &ThisArg::Tok(x)) => self.add_tok(fthis, x),
                        (None, &ThisArg::Node(n)) => self.edge(n, fthis),
                        (None, ThisArg::Undef) => self.add_prim(fthis, prim::UNDEF),
                    }
                }
                self.bind_args(call, t);
            }
            TokKind::Bound(_) => self.call_bound(call, t),
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

    /// Unknown callee (⊤ or an opaque builtin function value): arguments
    /// escape, the result is ⊤; with `sink_this` (no receiver token given),
    /// the call's own `this` escapes too.
    fn unknown_callee_args(&mut self, call: CallId, t: Tok, sink_this: bool) {
        if t == TOP {
            self.top_calls += 1;
        }
        let r = if t == TOP {
            self.reason("passed to a call of an unknown function value")
        } else {
            self.reason("passed to an opaque builtin function value")
        };
        let na = self.calls[call as usize].args.len();
        for i in 0..na {
            let a = self.calls[call as usize].args[i];
            self.sink_node(a, r);
        }
        let ns = self.calls[call as usize].spread.len();
        for i in 0..ns {
            let a = self.calls[call as usize].spread[i];
            self.sink_node(a, r);
        }
        if sink_this {
            match self.calls[call as usize].this {
                ThisArg::Node(n) => self.sink_node(n, r),
                ThisArg::Tok(x) => self.escape(x, r),
                ThisArg::Undef => {}
            }
        }
        let ret = self.calls[call as usize].ret;
        if t == TOP {
            self.add_tok(ret, TOP);
        } else {
            self.add_top(ret, |_| "result of an opaque builtin function value".into());
        }
        self.add_prim(ret, prim::ALL);
    }

    /// Arguments and result of `call` into function token `t`: once per
    /// (call, function); only `this` differs between receivers.
    fn bind_args(&mut self, call: CallId, t: Tok) {
        let TokKind::Func(f) = self.toks[t as usize] else {
            return;
        };
        if !self.bind_done.insert((call, t, u32::MAX - 1)) {
            return;
        }
        let f = f as usize;
        let c = call as usize;
        let rest = self.funcs[f].rest;
        let args_all = self.funcs[f].args_all;
        let nparams = self.funcs[f].params.len();
        let nargs = self.calls[c].args.len();
        for i in 0..nargs {
            let a = self.calls[c].args[i];
            if i < nparams {
                let p = self.funcs[f].params[i];
                self.edge(a, p);
            } else if let Some(rest) = rest {
                let fa = self.field_any_node_of_node(rest);
                self.edge(a, fa);
            }
            self.edge(a, args_all);
        }
        // Missing arguments are undefined.
        for i in nargs..nparams {
            let p = self.funcs[f].params[i];
            self.add_prim(p, prim::UNDEF);
        }
        let nspread = self.calls[c].spread.len();
        for j in 0..nspread {
            let s = self.calls[c].spread[j];
            // Spread arrays: their elements may be any parameter.
            let elems = self.node_l("spread-elems");
            self.cons(
                s,
                Cons::Load {
                    key: Key::Any,
                    dst: elems,
                },
            );
            for i in 0..nparams {
                let p = self.funcs[f].params[i];
                self.edge(elems, p);
            }
            if let Some(rest) = rest {
                let fa = self.field_any_node_of_node(rest);
                self.edge(elems, fa);
            }
            self.edge(elems, args_all);
        }
        let fret = self.funcs[f].ret;
        let cret = self.calls[c].ret;
        self.edge(fret, cret);
    }

    /// Calling a bound function: its target with the bound this; bound +
    /// call args land in any parameter (positions shift).
    fn call_bound(&mut self, call: CallId, t: Tok) {
        let TokKind::Bound(b) = self.toks[t as usize] else {
            return;
        };
        let c = self.calls[call as usize].clone();
        let (target, bthis, bargs) = self.bound[b as usize].clone();
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
        let holder = self.tok(TokKind::Bound(b));
        let fa = self.field_any(holder);
        self.edge(all_args, fa);
        self.add_tok(arr, holder);
        self.calls[call2 as usize].spread.push(arr);
        self.cons(target, Cons::Call { call: call2 });
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
        if t == TOP || self.is_escaped(t) {
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

    fn mark_escaped(&mut self, t: Tok, reason: u32) {
        self.escaped.insert(t, reason);
        let w = (t >> 6) as usize;
        if self.esc_bits.len() <= w {
            self.esc_bits.resize(w + 1, 0);
        }
        self.esc_bits[w] |= 1 << (t & 63);
    }

    fn escape_inner(&mut self, t: Tok, reason: u32) {
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.mark_escaped(t, reason);
            self.resume_stopped(t);
        }
        self.mark_escaped(t, reason);
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
        // Unknown code can also write any key of it: every setter on its
        // chain (its own accessors included) runs with `this` = it and an
        // unknown value (a direct eval writes the caller's locals exactly so,
        // through the setters of its bindings object); and read any key:
        // every getter runs and its result reaches unknown code.
        let w = self.node_l("escaped-store");
        self.add_top(w, |m| {
            format!(
                "stored into an escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(w, prim::ALL);
        self.accessor_walk(t, t, Key::Any, ChainAct::Store(w));
        let r = self.node_l("escaped-load");
        self.sink_node(r, reason);
        self.accessor_walk(t, t, Key::Any, ChainAct::Load(r));
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
        // `Object.setPrototypeOf(Object.prototype, p)` throws unless `p` is
        // null (its [[Prototype]] already): nothing changes (immutable
        // prototype exotic object). A ⊤ target that may be it is no E6 on it.
        if kind == Hard::E6SetPrototypeOf && matches!(self.toks[t as usize], TokKind::ObjectProto) {
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
            Cons::ThisTo { origin } => format!("thisto {n} {origin}"),
            Cons::CallArgs { call } => format!("callargs {n} {call}"),
            Cons::BuiltinCallee => format!("builtincallee {n}"),
            Cons::HolderStep { key, pass, stop } => {
                format!("holderstep {n} {} {pass} {stop}", self.key_name(*key))
            }
            Cons::VisitHolders { origin, key, act } => {
                format!("visitholders {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::VisitStop { origin, key, act } => {
                format!("visitstop {n} {origin} {} {act:?}", self.key_name(*key))
            }
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
