//! The solver: inclusion constraints with difference propagation over a
//! worklist. Copy edges are plain successor lists; every other rule is a
//! complex constraint (`Cons`) re-applied to each node's new tokens.
//!
//! Soundness rests on two things: every rule over-approximates the concrete
//! transfer function of its construct, and every unknown value or operation
//! goes to ⊤ (`TOP` token for values coming from unmodeled code, `TopSink`
//! for values leaving into it). The rules here are the executable form of
//! design section 3.2; `scripts/wpfacts_recompute.py` re-implements them.

mod copy;
mod define;
mod lookup;

use crate::builtins;
mod call_and_construct;
mod shared;
use crate::delta::{Targets, Tokens};
use crate::model::*;

#[path = "solve/apply.rs"]
mod apply;

#[path = "solve/store.rs"]
mod store;

mod escape;
mod fields;
mod virt;

impl Model {
    // ---------------------------------------------------------------- nodes

    /// Add tokens/prims to a node; schedules it when anything is new.
    pub fn add(&mut self, n: NodeId, toks: &[Tok], prim: u16) {
        self.add_tokens(n, Tokens::Slice(toks), prim);
    }
    fn add_tokens(&mut self, n: NodeId, toks: Tokens<'_>, prim: u16) {
        let mut later: Vec<Tok> = Vec::new();
        // A marker crossing into the node arrives as a member of the node's
        // own marker (MRK, `Model::crosses`; DECISIONS 44 for parameters):
        // the node no longer holds every value of its sources.
        let crossed: Option<Vec<Tok>> =
            match !self.param_marker.is_empty() && toks.iter().any(|&t| self.crosses(n, t)) {
                true => {
                    let r = self.arg_marker(n);
                    let mut v = Vec::with_capacity(toks.len());
                    for &t in toks {
                        if self.crosses(n, t) {
                            self.link_marker(t, r);
                            v.push(r);
                        } else {
                            v.push(t);
                        }
                    }
                    self.nodes[n as usize].dropped = true;
                    Some(v)
                }
                false => None,
            };
        let targets = if toks.is_empty() {
            Targets::Empty
        } else {
            Targets::capture(
                &self.nodes[n as usize].succ,
                self.nodes[n as usize].succ_set.as_ref(),
            )
        };
        let node = &mut self.nodes[n as usize];
        let mut changed = false;
        let mut top_new = false;
        let toks = match &crossed {
            Some(v) => Tokens::Slice(v),
            None => toks,
        };
        for &t in toks {
            // A marker that reads as ⊤ (DECISIONS 42) is decided after the
            // other tokens, once it is known whether ⊤ arrived with them. So
            // is an escaped value (`Model::top_beside`): unknown code holds
            // it, so beside ⊤ it is that ⊤.
            let escaped_value = marker_top_bit(&self.esc_bits, t)
                && !(self.param_marker.contains_key(&t) || marker_top_bit(&self.arg_bits, t));
            if marker_top_bit(&self.marker_top, t) || escaped_value {
                later.push(t);
                continue;
            }
            if node.pts.insert(t) {
                node.delta.push_to(t, &targets);
                changed = true;
                self.pts_entries += 1;
                top_new |= t == TOP;
            }
        }
        // Beside ⊤ it is that ⊤: every rule does to ⊤ at least what it does
        // to the marker, and the readers after the solve read it as ⊤. Left
        // out, it never floods the unit. Without ⊤ it is delivered as before.
        if !later.is_empty() {
            if node.pts.contains_tok(TOP) {
                node.dropped = true;
            } else {
                for t in later {
                    if node.pts.insert(t) {
                        node.delta.push_to(t, &targets);
                        changed = true;
                        self.pts_entries += 1;
                    }
                }
            }
        }
        if top_new && self.diagnostic.is_some() {
            self.diagnostic_predecessor(n, self.cur_src, self.cur_why);
        }
        if top_new && (self.explain || self.top_trace) {
            self.top_pred
                .entry(n)
                .or_insert((self.cur_src, self.cur_why));
        }
        if self.pts_entries & 0xfffff == 0 && self.pts_entries != 0 {
            if self
                .budget
                .is_some_and(|(start, limit)| start.elapsed() > limit)
                || (self.max_pts != 0 && self.pts_entries > self.max_pts)
            {
                self.aborted = true;
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

    /// Charge the work since the last mark to what it was spent for, and
    /// from now on to `n` (a popped node) or, with `NO_NODE`, to `phase`
    /// (PERRY_FACTS_PROFILE only).
    pub fn prof_mark(&mut self, n: NodeId, phase: &'static str) {
        let Some(p) = self.prof.as_mut() else { return };
        let (at, w0, ph) = p.at;
        let w = self.work - w0;
        if w != 0 {
            if at == NO_NODE {
                *p.phase.entry(ph).or_default() += w;
            } else {
                *p.node.entry(at).or_default() += w;
            }
        }
        p.at = (n, self.work, phase);
        if self.work >= p.next_curve {
            p.next_curve = (self.work / WorkProfile::STEP + 1) * WorkProfile::STEP;
            let cons: usize = self.nodes.iter().map(|n| n.cons.len()).sum();
            eprintln!(
                "perry: wpfacts curve work={} pops={} nodes={} pts={} cons={} toks={} insts={} wl={}",
                self.work,
                self.pops,
                self.nodes.len(),
                self.pts_entries,
                cons,
                self.toks.len(),
                self.sum.insts.len(),
                self.wl.len()
            );
        }
    }

    /// The work profile (PERRY_FACTS_PROFILE): phases, steps, rule kinds and
    /// the 20 nodes the most work was spent for.
    pub fn prof_report(&mut self) {
        let Some(p) = self.prof.take() else { return };
        let mut ph: Vec<_> = p.phase.iter().collect();
        ph.sort_by(|a, b| b.1.cmp(a.1));
        let in_pops: u64 = p.node.values().sum();
        eprintln!(
            "perry: wpfacts work total={} in pops={in_pops} phases={ph:?} steps pop-charge={} fast-copy={} copy={} copy-extra={}",
            self.work, p.step[0], p.step[1], p.step[2], p.step[3]
        );
        let mut ks: Vec<_> = p.kind.iter().collect();
        ks.sort_by(|a, b| (b.1 .1 + b.1 .3).cmp(&(a.1 .1 + a.1 .3)));
        for (k, (n, w, rn, rw)) in ks.iter().take(25) {
            eprintln!(
                "perry: wpfacts kindwork {k} applies={n} work={w} attach={rn} attachwork={rw}"
            );
        }
        let mut ns: Vec<_> = p.node.iter().map(|(n, w)| (*w, *n)).collect();
        ns.sort_unstable_by(|a, b| b.cmp(a));
        for (w, n) in ns.into_iter().take(20) {
            let node = &self.nodes[n as usize];
            let mut kinds: crate::pts::FxHashMap<&'static str, u32> = Default::default();
            for c in &node.cons {
                *kinds.entry(c.kind_name()).or_default() += 1;
            }
            let mut kinds: Vec<_> = kinds.into_iter().collect();
            kinds.sort_by(|a, b| b.1.cmp(&a.1));
            kinds.truncate(6);
            let some: Vec<String> = node
                .pts
                .to_sorted()
                .iter()
                .take(8)
                .map(|&t| self.tok_name(t))
                .collect();
            eprintln!(
                "perry: wpfacts nodework #{n} {} work={w} pts={} top={} succ={} cons={} kinds={kinds:?} some={some:?}",
                self.labels[n as usize],
                node.pts.len(),
                node.pts.contains_tok(TOP) as u8,
                node.succ.len(),
                node.cons.len()
            );
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
                    "{}#{i}:{l}:p{}:n{}:top{}",
                    self.labels[i],
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
            "rss_mb={rss_mb} succ_cap={succ_cap} delta_cap={delta_cap} big={big} max_succ={max_succ} stopped={stopped} csites={csites} walks={} relays={} pops={} wl={} nodes={} cons={} succ={} pts={} delta={} calls={} stores={} toks={} tmpls={} bind={} chain={} callthis={} visits={} anywalks={} cv={} cp={} escaped={} entries={}",
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
            self.escaped.len(),
            self.pts_entries
        )
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
        if self.explain || self.diagnostic.is_some() {
            let w = why(self);
            if !self.nodes[n as usize].pts.contains_tok(TOP) {
                if let Some(d) = &mut self.diagnostic {
                    d.seeds.entry(n).or_insert(w.clone());
                }
            }
            if self.explain || self.top_trace {
                let r = self.reason(&w);
                let saved = (self.cur_src, self.cur_why);
                self.cur_src = NO_NODE;
                self.cur_why = r;
                self.add_tok(n, TOP);
                (self.cur_src, self.cur_why) = saved;
            } else {
                self.add_tok(n, TOP);
            }
        } else {
            self.add_tok(n, TOP);
        }
    }

    /// A node holding only ⊤ (all kinds) for one named unmodeled origin.
    pub fn top_val(&mut self, why: &str) -> NodeId {
        // Explain and the dump name each origin: its own node.
        if !self.explain && self.diagnostic.is_none() {
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

    /// Attach a complex constraint and apply it to what the node holds now.
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        self.stats_cons += 1;
        if self.diagnostic.is_some() && matches!(c, Cons::TopSink { .. }) {
            self.diagnostic_sink(n);
        }
        if self.relations.is_some()
            && !matches!(
                c,
                Cons::ThisTo { .. }
                    | Cons::CallArgs { .. }
                    | Cons::RetInto { .. }
                    | Cons::ArgInto { .. }
                    | Cons::ThisInst { .. }
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
        if !self.cell_watch.is_empty() && self.cell_watch.remove(&n) {
            self.cell_woken.push(n);
        }
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
        // The replay is solve work: the bound covers it.
        self.work += toks.len() as u64 + 1;
        if let Some(p) = self.prof.as_mut() {
            let e = p.kind.entry(c.kind_name()).or_default();
            e.2 += 1;
            e.3 += toks.len() as u64 + 1;
        }
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = n;
        self.cur_why = u32::MAX;
        let tsaved = self.trace_apply_node;
        if self.top_trace {
            self.trace_apply_node = n;
        }
        self.with_s(|m| m.apply(n, &c, &toks, prim));
        if self.top_trace {
            self.trace_apply_node = tsaved;
        }
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    /// Work no solve has run yet: what was added since the last one (a
    /// contract check's escapes and the instances they make) is part of
    /// the unit's facts only once solved.
    pub(crate) fn unsolved(&self) -> bool {
        !self.wl.is_empty() || !self.sum.pending.is_empty() || !self.marker_links.is_empty()
    }

    pub fn solve(&mut self) {
        self.rel_phase = b'S';
        self.mark_params();
        self.members_settled.get_mut().clear();
        loop {
            self.prof_mark(NO_NODE, "instances");
            self.fill_instances();
            self.prof_mark(NO_NODE, "marker links");
            self.replay_marker_links();
            self.prof_mark(NO_NODE, "settle");
            let Some(n) = self.wl.pop_front() else {
                if self.sum.pending.is_empty()
                    && self.marker_links.is_empty()
                    && !self.settle_bf_written()
                    && !self.settle_for_in()
                    && !self.settle_define()
                    && !crate::builtins::settle_native_given(self)
                {
                    break;
                }
                continue;
            };
            self.pops += 1;
            self.prof_mark(n, "");
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
            let (mut delta, dprim) = {
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
            delta.prepare();
            let delivery_overhead = delta.delivery_overhead();
            // Index loops (no clone of the lists): successors and
            // constraints appended while this runs already saw the full set.
            self.cur_src = n;
            self.cur_why = u32::MAX;
            let ncons0 = self.nodes[n as usize].cons.len();
            self.work += (delta.len() as u64 + 1) * ncons0 as u64;
            if let Some(p) = self.prof.as_mut() {
                p.step[0] += (delta.len() as u64 + 1) * ncons0 as u64;
            }
            if self.profile.is_some() {
                let e = self.label_work.entry(self.labels[n as usize]).or_default();
                e.1 += delta.len() as u64 * ncons0 as u64;
                e.2 += 1;
            }
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            let mut i = 0;
            while i < self.nodes[n as usize].succ.len() {
                let s = self.nodes[n as usize].succ[i];
                if self.try_copy_delivery(n, s, &delta, dprim) {
                    self.work += 1 + delivery_overhead;
                    if let Some(p) = self.prof.as_mut() {
                        p.step[1] += 1 + delivery_overhead;
                    }
                    if self.max_work != 0 && self.work > self.max_work {
                        self.aborted = true;
                        return;
                    }
                    i += 1;
                    continue;
                }
                let (covered, checks) = if delta.is_empty() {
                    (true, 1)
                } else {
                    self.copy_coverage(n, s)
                };
                let toks = if covered {
                    Tokens::Slice(&[])
                } else {
                    Tokens::Delivery(&delta, s)
                };
                let visits = toks.len() as u64;
                self.work += visits + checks + delivery_overhead;
                if let Some(p) = self.prof.as_mut() {
                    p.step[2] += visits + checks + delivery_overhead;
                }
                if self.profile.is_some() {
                    self.label_work
                        .entry(self.labels[n as usize])
                        .or_default()
                        .0 += visits;
                }
                if self.max_work != 0 && self.work > self.max_work {
                    self.aborted = true;
                    return;
                }
                if covered {
                    self.add_tokens(s, toks, dprim);
                } else {
                    let extra = self.copy_to(n, s, toks, dprim);
                    self.work += extra;
                    if let Some(p) = self.prof.as_mut() {
                        p.step[3] += extra;
                    }
                    if self.max_work != 0 && self.work > self.max_work {
                        self.aborted = true;
                        return;
                    }
                }
                i += 1;
            }
            let ncons = self.nodes[n as usize].cons.len();
            for i in 0..ncons {
                let c = self.nodes[n as usize].cons[i];
                self.cur_src = n;
                self.cur_why = u32::MAX;
                self.apply_tokens(n, &c, Tokens::Pending(&delta), dprim);
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

    /// The node holding the prototype objects of `t`.
    pub fn proto_node(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.proto_node.get(&t) {
            return n;
        }
        let n = self.node_named("protos", || format!("protos({t})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.proto_node.insert(t, n);
        self.rel(|| format!("protonode {n} {t}"));
        // Seed the static part of the chain.
        let objproto = self.tok(TokKind::ObjectProto);
        match self.toks[t as usize].clone() {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind.clone() {
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
                // Object.create / literal prototypes are added by gen. A
                // receiver token or marker has its classes' (DECISIONS 59).
                _ => match self.virt_protos.get(&t).cloned() {
                    Some(ps) => {
                        for p in ps {
                            let pt = self.tok(TokKind::Proto(p));
                            self.add_tok(n, pt);
                        }
                    }
                    None => self.add_tok(n, objproto),
                },
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
                (None, None) => match self.tmpls[tm as usize].parent_value {
                    // `S.prototype.__proto__` is the parent's `prototype`.
                    Some((_, pp)) => self.edge(pp, n),
                    // A null-prototype record: the chain ends here.
                    None if self.tmpls[tm as usize].null_proto => {}
                    None => self.add_tok(n, objproto),
                },
            },
            TokKind::NativeProto(_) => self.add_tok(n, objproto),
            // A builtin namespace's prototype is `Function.prototype` (a
            // constructor's) or `Object.prototype` (`Math`, `JSON`, the
            // global object's chain): `ObjectProto` stands for both.
            TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn | TokKind::Builtin(_) => {
                self.add_tok(n, objproto)
            }
            TokKind::ClassObj(tm) => match self.tmpls[tm as usize].parent {
                Some(p) => {
                    let pc = self.tok(TokKind::ClassObj(p));
                    self.add_tok(n, pc);
                }
                None => match self.tmpls[tm as usize].parent_value {
                    // `S.__proto__` is the value of the `extends` expression.
                    Some((pv, _)) => self.edge(pv, n),
                    None => self.add_tok(n, objproto),
                },
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
        self.apply_tokens(n, c, Tokens::Slice(toks), prim);
    }
    fn apply_tokens(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if let Some(d) = self.diagnostic.as_mut() {
            let saved = (d.cur_call.take(), d.cause_node.replace(n));
            self.apply_tokens_in(n, c, toks, prim);
            if let Some(d) = self.diagnostic.as_mut() {
                (d.cur_call, d.cause_node) = saved;
            }
            return;
        }
        self.apply_tokens_in(n, c, toks, prim);
    }
    fn apply_tokens_in(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if let Some(mk) = self.top_chain_mark {
            if toks.iter().any(|&t| t == mk) {
                return self.apply_chain_marker(n, c, toks, prim, mk);
            }
        }
        self.apply_tokens_body(n, c, toks, prim);
    }

    /// DECISIONS 254 (rule P): `toks` holds the chain marker, always beside
    /// ⊤: "⊤, which may be any prototype in `top_chain`". Every constraint
    /// kind is named (a new kind is a compile error here, not a silent
    /// hole):
    /// - `Mark`: a write through it (a member added, replaced, deleted or
    ///   defined, an accessor installed, the [[Prototype]] relinked, the
    ///   receiver handed to a writer) marks every prototype in `top_chain`,
    ///   now and as the set grows, as written with ⊤ under the write's
    ///   constant key, or under an unknown key where it names none
    ///   (`Cons::ChainMark`): the opaque mark, never a value flow. DEC 250
    ///   makes adds the only effect, so only a missing-member read of that
    ///   key sees it. A delete marks nothing: deleting a described member is
    ///   a TypeError at the store (DEC 250), and an added key is opaque
    ///   already. A `setPrototypeOf` writes no own key: the prototypes take
    ///   the source as their [[Prototype]] (`ProtoFrom` on `top_chain`);
    /// - `Call`: a method or `super` call, or a receiver binding, with the
    ///   prototypes as `this`, for its effect only (`effect_only`): the
    ///   member resolves against each prototype's described members;
    /// - `Pass`: the value itself flows on (a copy, a guard, a sink, a
    ///   run's result): the marker stays with it;
    /// - `Drop`: a read, call, key, chain walk or descriptor through it is
    ///   ⊤'s, which `toks` holds.
    fn apply_chain_marker(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16, mk: Tok) {
        enum Mk {
            Mark,
            Call,
            Pass,
            Drop,
        }
        let mode = match c {
            // Writes onto the node's objects.
            Cons::Store { .. } => Mk::Mark,
            Cons::DefineKey { .. } => Mk::Mark,
            Cons::AddsKeys => Mk::Mark,
            Cons::Hard { .. } => Mk::Mark,
            Cons::DeleteKey { .. } => Mk::Mark,
            Cons::AccessorInstall { .. } => Mk::Mark,
            // `Object.assign`'s target.
            Cons::SpreadFrom { .. } => Mk::Mark,
            // A literal's birth puts the field on every token of the node:
            // on a prototype that is a write.
            Cons::BirthField { .. } => Mk::Mark,
            Cons::ProtoFrom { .. } => Mk::Mark,
            Cons::StoreTopProto { .. } => Mk::Mark,
            Cons::NewTargetHard { .. } => Mk::Mark,
            // The node's objects handed to a writer.
            Cons::StoreOut { .. } => Mk::Mark,
            Cons::HandOver { .. } => Mk::Mark,
            Cons::ContractThis { .. } => Mk::Mark,
            Cons::OverrideThis { .. } => Mk::Mark,
            // A method call or a receiver binding with the node's objects as
            // `this`.
            Cons::MCall { .. } => Mk::Call,
            Cons::IdxMCall { .. } => Mk::Call,
            Cons::ThisInst { .. } => Mk::Call,
            Cons::SuperCtor { .. } => Mk::Call,
            Cons::SuperCtorParent { .. } => Mk::Call,
            // `super.key` on `this`: a read is ⊤'s; a call is on the
            // receiver; a store writes it.
            Cons::Super {
                act: ChainAct::Load(_),
                ..
            } => Mk::Drop, // read
            Cons::Super {
                act: ChainAct::Call(_),
                ..
            } => Mk::Call,
            Cons::Super { .. } => Mk::Mark,
            // The value itself flows on.
            Cons::Guard { .. } => Mk::Pass,
            Cons::Defined { .. } => Mk::Pass,
            Cons::NotNullish { .. } => Mk::Pass,
            Cons::TopIfAny { .. } => Mk::Pass,
            Cons::CjsPublish { .. } => Mk::Pass,
            Cons::TopSink { .. } => Mk::Pass,
            Cons::OwnOnly { .. } => Mk::Pass,
            Cons::CallerValue { .. } => Mk::Pass,
            Cons::RetInto { .. } => Mk::Pass,
            Cons::ArrayLenArg { .. } => Mk::Pass,
            // An undescribed builtin's argument: its own arm reads the
            // marker.
            Cons::ChainMark { .. } => Mk::Pass,
            // Reads: ⊤'s result (a chain key on ⊤ marks its own result).
            Cons::Load { .. } => Mk::Drop,      // read
            Cons::OwnValues { .. } => Mk::Drop, // read
            Cons::IdxLoad { .. } => Mk::Drop,   // read
            Cons::Miss { .. } => Mk::Drop,      // read
            Cons::Implicit { .. } => Mk::Drop,  // read
            Cons::IterNext { .. } => Mk::Drop,  // read
            Cons::ForInKeys { .. } => Mk::Drop, // read
            Cons::Spread { .. } => Mk::Drop,    // read
            Cons::ProtoOf { .. } => Mk::Drop,   // read
            // The node is a callee: ⊤'s call.
            Cons::Call { .. } => Mk::Drop,        // call
            Cons::CallThis { .. } => Mk::Drop,    // call
            Cons::New { .. } => Mk::Drop,         // call
            Cons::NewNamed { .. } => Mk::Drop,    // call
            Cons::ThisTo { .. } => Mk::Drop,      // call
            Cons::CallArgs { .. } => Mk::Drop,    // call
            Cons::BuiltinCallee => Mk::Drop,      // call
            Cons::ForInBody { .. } => Mk::Drop,   // call
            Cons::ThisFrom { .. } => Mk::Drop,    // call
            Cons::ParentGiven { .. } => Mk::Drop, // call
            // The node is a key; the receiver gets its own constraint.
            Cons::StoreKeyed { .. } => Mk::Drop,    // key
            Cons::StoreIndexed { .. } => Mk::Drop,  // key
            Cons::DefineKeyed { .. } => Mk::Drop,   // key
            Cons::AccessorKeyed { .. } => Mk::Drop, // key
            Cons::IdxKey { .. } => Mk::Drop,        // key
            Cons::IdxKeyCall { .. } => Mk::Drop,    // key
            // The node is a descriptor map, a descriptor or an accessor
            // function; the target gets its own constraint.
            Cons::DefineAll { .. } => Mk::Drop,  // map
            Cons::DefineDesc { .. } => Mk::Drop, // desc
            Cons::Accessor { .. } => Mk::Drop,   // fn
            // Chain walks: ⊤ is an unknown prototype there.
            Cons::Chain { .. } => Mk::Drop,         // chain
            Cons::ChainUnion { .. } => Mk::Drop,    // chain
            Cons::ProtoGiven { .. } => Mk::Drop,    // chain
            Cons::ProtoClosure { .. } => Mk::Drop,  // chain
            Cons::ProtoMethods { .. } => Mk::Drop,  // chain
            Cons::AnyAccessors { .. } => Mk::Drop,  // chain
            Cons::ProtoValues { .. } => Mk::Drop,   // chain
            Cons::PlainKey { .. } => Mk::Drop,      // chain
            Cons::HolderStep { .. } => Mk::Drop,    // chain
            Cons::VisitHolders { .. } => Mk::Drop,  // chain
            Cons::VisitStop { .. } => Mk::Drop,     // chain
            // Primitive results only.
            Cons::Arith { .. } => Mk::Drop, // prim
            Cons::Plus { .. } => Mk::Drop,  // prim
            // Program prototypes only (E10), never a builtin one.
            Cons::CreateProto { .. } => Mk::Drop, // program
            // Parameter markers only (DECISIONS 44 (b)).
            Cons::ArgInto { .. } => Mk::Drop, // marker
            // ⊤ beside it is a shared buffer already (B1).
            Cons::ShareBuffer { .. } => Mk::Drop, // top
        };
        let tc = self.top_chain();
        let here = n != tc && !self.sum_skip(n, c);
        match mode {
            Mk::Pass => return self.apply_tokens_body(n, c, toks, prim),
            Mk::Mark if here => {
                let key = match c {
                    Cons::Store { key, .. } | Cons::BirthField { key, .. } => Some(*key),
                    Cons::Super {
                        key,
                        act: ChainAct::Store(_),
                        ..
                    } => Some(*key),
                    Cons::DefineKey { key, .. } | Cons::AccessorInstall { key, .. } => {
                        Some(Key::Named(*key))
                    }
                    Cons::DeleteKey { .. }
                    | Cons::Hard {
                        kind: Hard::E7Delete,
                        ..
                    } => None,
                    // `setPrototypeOf` writes no own key: each prototype
                    // takes the source as its [[Prototype]], as ⊤ does
                    // (`native_given`), so its lookups reach the keys of the
                    // source's chain and no other.
                    Cons::Hard {
                        kind: Hard::E6SetPrototypeOf,
                        ..
                    } => None,
                    Cons::ProtoFrom { .. } => {
                        self.cons(tc, *c);
                        None
                    }
                    _ => Some(Key::Any),
                };
                if let Some(key) = key {
                    let r = self.reason(&format!("a {} through the chain marker", c.kind_name()));
                    self.chain_mark(r, false, Some(key));
                }
            }
            Mk::Call if here => {
                // DEC 250 (narrowed): only a prototype's own members other
                // than `constructor` are locked. `constructor` and a computed
                // member may be any value, written where the solve does not
                // see it: the call may write the prototypes, the mark.
                let open = match c {
                    Cons::MCall { key: Key::Named(a), .. } => {
                        self.atoms[*a as usize] == "constructor"
                    }
                    Cons::MCall { key: Key::Any, .. } | Cons::IdxMCall { .. } => true,
                    _ => false,
                };
                if open {
                    let r = self.reason("a call of an unlocked member through the chain marker");
                    self.chain_mark(r, false, Some(Key::Any));
                }
                let e = self.effect_only(c);
                self.cons(tc, e);
            }
            Mk::Mark | Mk::Call | Mk::Drop => {}
        }
        let rest: Vec<Tok> = toks.iter().copied().filter(|&t| t != mk).collect();
        self.apply_tokens_body(n, c, Tokens::Slice(&rest), prim);
    }

    /// DECISIONS 254: the opaque mark on every prototype in `top_chain`
    /// (`Cons::ChainMark`), installed once per key (`join`: once more, for
    /// the actor set): idempotent per prototype, one constraint per constant
    /// key, not one per write.
    pub(crate) fn chain_mark(&mut self, reason: u32, join: bool, key: Option<Key>) {
        if self.unit_mode || !self.top_chain_marks.insert((key, join)) {
            return;
        }
        let tc = self.top_chain();
        self.cons(tc, Cons::ChainMark { reason, join, key });
    }

    /// DECISIONS 254: `c` copied onto `top_chain` for its effect only: a
    /// call it makes (a method or `super` call with the prototypes as
    /// `this`, a receiver binding) is a solver call of the same site whose
    /// result no node reads. The prototypes reach that call's `this` and
    /// what the method writes there; never a value (`sort`, `valueOf`,
    /// `Set.prototype.add` return `this`), where ⊤ stands already.
    fn effect_only(&mut self, c: &Cons) -> Cons {
        let mut c = *c;
        let call = match &mut c {
            Cons::MCall { call, .. }
            | Cons::IdxMCall { call, .. }
            | Cons::ThisInst { call, .. }
            | Cons::SuperCtor { call, .. }
            | Cons::SuperCtorParent { call, .. }
            | Cons::Super {
                act: ChainAct::Call(call),
                ..
            } => call,
            _ => return c,
        };
        let mut cs = self.calls[*call as usize].clone();
        cs.ret = self.node_l("TOPCHAIN-ret");
        *call = self.solver_call(*call, cs);
        c
    }

    fn apply_tokens_body(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if self.top_trace {
            self.trace_apply_node = n;
        }
        if self.sum_skip(n, c) {
            return;
        }
        if self.profile.is_some() {
            let started = std::time::Instant::now();
            let w0 = self.work;
            self.apply_inner(n, c, toks, prim);
            let name = c.kind_name();
            if let Some(p) = self.prof.as_mut() {
                let e = p.kind.entry(name).or_default();
                e.0 += 1;
                e.1 += self.work - w0;
            }
            let e = self.profile.as_mut().unwrap().entry(name).or_default();
            e.0 += 1;
            e.1 += started.elapsed();
            return;
        }
        self.apply_inner(n, c, toks, prim);
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
        self.has_ancestor(tm, owner)
    }

    /// After the solve (§17.2 item 3): a parent value that holds exactly one
    /// class and nothing else is the class's parent (one chain for derive,
    /// the contract export and `ancestors`); anything else — ⊤, two classes,
    /// a non-class — leaves no single layout chain: hard E11.
    pub fn settle_parent_values(&mut self) {
        for tm in 0..self.tmpls.len() as TmplId {
            let Some((pv, _)) = self.tmpls[tm as usize].parent_value else {
                continue;
            };
            let toks = self.nodes[pv as usize].pts.to_sorted();
            // `extends v` with v a primitive other than null throws a
            // TypeError when the class is defined: no instance of it exists
            // with that parent. `extends null` is a parent of its own.
            let null = self.nodes[pv as usize].prim & prim::NULL != 0;
            let one = match toks.as_slice() {
                [t] if !null => match self.toks[*t as usize] {
                    TokKind::ClassObj(p) if !self.has_ancestor(p, tm) => Some(p),
                    _ => None,
                },
                _ => None,
            };
            match one {
                Some(p) => self.tmpls[tm as usize].parent = Some(p),
                None => {
                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                    let what = format!(
                        "E11 class whose `extends` value is not one class ({} tokens{}{})",
                        toks.len(),
                        if toks.contains(&TOP) { ", ⊤" } else { "" },
                        if null { ", null" } else { "" }
                    );
                    let r = self.reason(&what);
                    self.mark_hard(inst, Hard::E11ReflectConstruct, r);
                }
            }
        }
        // A parent linked by name: the evaluated `extends` value is that
        // class (the same primitive rule as above), else the chain the
        // solve modeled is not the chain of the instances.
        for (tm, chk) in std::mem::take(&mut self.parent_checks) {
            let Some(p) = self.tmpls[tm as usize].parent else {
                continue;
            };
            let toks = self.nodes[chk as usize].pts.to_sorted();
            let null = self.nodes[chk as usize].prim & prim::NULL != 0;
            let linked = !null
                && matches!(toks.as_slice(), [t] if matches!(self.toks[*t as usize], TokKind::ClassObj(q) if q == p));
            if !linked {
                let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                let what = format!(
                    "E11 class whose evaluated `extends` is not its linked parent ({} tokens{}{})",
                    toks.len(),
                    if toks.contains(&TOP) { ", ⊤" } else { "" },
                    if null { ", null" } else { "" }
                );
                let r = self.reason(&what);
                self.mark_hard(inst, Hard::E11ReflectConstruct, r);
            }
        }
    }

    /// DECISIONS 118: what an argument marker's stores and `StoreOut`s do
    /// to each marker that linked in since (`link_marker`).
    fn replay_marker_links(&mut self) {
        while let Some((m, r)) = self.marker_links.pop() {
            if self.aborted {
                self.marker_links.clear();
                return;
            }
            // DECISIONS 193: `r` and the argument markers above it stand for
            // the readers `m` brings; the stores made on them reach those.
            let zs = self.arg_readers_of(m);
            if !zs.is_empty() {
                self.arg_readers_add(r, zs);
            }
            // DECISIONS 136: a flow whose stored value `r`'s members stand for.
            // Only `m` is new below `r`: the walk from `r`'s base has seen
            // the rest, and what it found is recorded (XUC-3).
            for (t, flow, reason, own) in self.arg_src_flows.get(&r).cloned().unwrap_or_default() {
                self.store_out_arg_reach(t, flow, reason, own, Some(m));
            }
            for (f, j, reason) in self.arg_src_watch.get(&r).cloned().unwrap_or_default() {
                if self.marker_of.get(&(f, j)) != Some(&m) {
                    self.escape(m, reason);
                }
            }
            for reason in self.handover_watch.get(&r).cloned().unwrap_or_default() {
                self.hand_over_marker(m, reason);
            }
        }
    }

    /// DECISIONS 152: marker `t` handed to a closed class's constructor
    /// (`Cons::HandOver`). A parameter's own marker is its function's: that
    /// function's contract hands it over, and each caller's unit passes
    /// what it holds to the constructor itself. What a parameter reaches
    /// is not the parameter (its keyed reach marker shares the parameter's
    /// entry, §18.4). An argument marker stands
    /// for its members, also those that link in later
    /// (`replay_marker_links`). DECISIONS 155: a reach with values beside
    /// it (a published method's receivers, DECISIONS 107; a closed
    /// function's described parameter, 74) stands for no value of its own:
    /// the values beside it reach the constructor themselves, path by path,
    /// and its argument contract reads it as them (142). Any other marker
    /// (the receiver itself, a bare reach, an argument marker's reach)
    /// escapes, as a value no contract describes does.
    fn hand_over_marker(&mut self, t: Tok, reason: u32) {
        if self.is_arg_marker(t) && !self.reach_marker.contains(&t) {
            let w = self.handover_watch.entry(t).or_default();
            if w.contains(&reason) {
                return;
            }
            w.push(reason);
            for m in self.arg_members.get(&t).cloned().unwrap_or_default() {
                self.hand_over_marker(m, reason);
            }
            return;
        }
        let own = self
            .param_marker
            .get(&t)
            .is_some_and(|&(f, j)| j != u32::MAX && self.marker_of.get(&(f, j)) == Some(&t));
        let beside = self.reach_marker.contains(&t) && self.beside_values(t);
        if !own && !beside {
            self.escape(t, reason);
        }
    }

    /// The key `t` is the keyed reach of parameter `j` of `f` under, if
    /// it is one (`None`: its reach through any key).
    fn param_reach_key(&self, f: FnIdx, j: u32, t: Tok) -> Option<Atom> {
        self.marker_of
            .get(&(f, j))
            .and_then(|mk| self.keyed_reach.get(mk))
            .and_then(|v| v.iter().find(|&&(_, r)| r == self.path_root(t)))
            .map(|&(k, _)| k)
    }

    /// DECISIONS 136: stored value `t`, an argument marker or its reach
    /// (under a key, or any), stands for each member or that reach of it:
    /// `f`'s own parameter, or its reach, is a source of the flow (the
    /// reader stores its own value), any other member's escapes. The
    /// members that link in later are replayed (`replay_marker_links`).
    ///
    /// `from`: a member that linked in under a marker this flow already
    /// walked (`replay_marker_links`); the walk covers what is new, `from`
    /// and below (XUC-3). Each marker visited is solve work (113-CWL3).
    /// DECISIONS 151: a member is a source as a value stored directly is:
    /// in a store into the receiver's reach marker it met (`own`), a value
    /// of this unit takes the path of that store (`Cons::StoreOut`,
    /// DECISIONS 149), not an escape.
    fn store_out_arg_reach(
        &mut self,
        t: Tok,
        flow: StoreFlowKey,
        reason: u32,
        own: Option<Tok>,
        from: Option<Tok>,
    ) {
        let (f, dst, reach, ..) = flow;
        // `None`: the members themselves; `Some(k)`: their reach under key
        // `k`, or any key.
        let (base, k) = match self.keyed_parent.get(&t).copied() {
            Some((p, k)) => (p, Some(Some(k))),
            None => match self.arg_reach_base.get(&t).copied() {
                Some(p) => (p, Some(None)),
                None => (t, None),
            },
        };
        let mut st = vec![from.unwrap_or(base)];
        let mut seen = crate::pts::FxHashSet::default();
        let mut srcs = Vec::new();
        while let Some(x) = st.pop() {
            if !seen.insert(x) {
                continue;
            }
            self.work += 1;
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            match self.arg_members.get(&x).cloned() {
                Some(ms) => {
                    if self.arg_src_flow_done.insert((x, t, flow, reason, own)) {
                        self.arg_src_flows
                            .entry(x)
                            .or_default()
                            .push((t, flow, reason, own));
                    }
                    st.extend(ms);
                }
                // A reach marker reaches itself under every key.
                None if self.reach_marker.contains(&x) => srcs.push(x),
                None => srcs.push(match k {
                    None => x,
                    Some(Some(k)) => self.keyed_reach_of(x, k),
                    Some(None) => match self.marker_reach.get(&x) {
                        Some(&r) => r,
                        None => x,
                    },
                }),
            }
        }
        let (mut from, mut from_reach) = (Vec::new(), Vec::new());
        let mut top = false;
        for s in srcs {
            match self.param_marker.get(&s).copied() {
                // DECISIONS 179: as in `Cons::StoreOut`.
                Some((g, u32::MAX)) if g == f && dst == u32::MAX && !reach => {
                    match self.reach_marker.contains(&s) {
                        true => from_reach.push((u32::MAX, self.param_reach_key(f, u32::MAX, s))),
                        false => from.push(u32::MAX),
                    }
                }
                Some((g, j)) if g == f && j != u32::MAX && self.reach_marker.contains(&s) => {
                    from_reach.push((j, self.param_reach_key(f, j, s)));
                }
                Some((g, j)) if g == f && j != u32::MAX => from.push(j),
                _ => match own {
                    Some(at) => {
                        self.mark_escape_why(at, &|_| "store: flow of another value".into())
                    }
                    None => {
                        top = true;
                        self.escape(s, reason);
                    }
                },
            }
        }
        let fl = self.store_flows.entry(flow).or_default();
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
    }

    /// The parameter of `f` whose node argument marker `t` is the marker of.
    fn arg_param_of(&self, t: Tok, f: FnIdx) -> Option<u32> {
        let n = *self.arg_marker_node.get(&t)?;
        match self.param_at.get(&n) {
            Some(&(g, j)) if g == f => Some(j),
            _ => None,
        }
    }

    /// Every function's parameter nodes hold their argument marker
    /// (DECISIONS 44), from before the first binding reaches them: the
    /// queued bindings run when the summaries exist, before the solve.
    pub(crate) fn mark_params(&mut self) {
        for f in 0..self.funcs.len() {
            for i in 0..self.funcs[f].params.len() {
                let p = self.funcs[f].params[i];
                if p != NO_NODE {
                    self.nodes[p as usize].param = true;
                    self.param_at.insert(p, (f as FnIdx, i as u32));
                }
            }
        }
    }

    /// Everything that reaches `n` escapes (once per node: no sink nodes).
    pub fn sink_node(&mut self, n: NodeId, reason: u32) {
        if self.sunk_nodes.insert(n) {
            self.cons(n, Cons::TopSink { reason });
        }
    }

    /// DECISIONS 254: a value given to undescribed code may be the chain
    /// marker; the node then marks every prototype in `top_chain` as written
    /// under every key and joins the actor set (`Cons::ChainMark`). Once per
    /// node (`Node::opaque_marked`), as `sink_node`: `chain_mark` installs one
    /// mark per (key, join), so a second identical mark on the node only
    /// re-scans what the node holds, and the first mark is always the first
    /// to fire there (the same delivery, or an earlier one), reason included.
    pub fn opaque_mark(&mut self, n: NodeId, reason: u32) {
        if !std::mem::replace(&mut self.nodes[n as usize].opaque_marked, true) {
            self.cons(n, Cons::ChainMark { reason, join: true, key: Some(Key::Any) });
        }
    }

    // ---------------------------------------------------------------- calls

    /// Builtin protocol touch on `t` (ToPrimitive, iteration, toJSON, then,
    /// inspect …): every function reachable as a property of `t` or its
    /// chain runs with `this` = `t` and unknown primitive arguments.
    pub fn implicit(&mut self, t: Tok, reason: u32, proto: Protocol) {
        // The touch looks up and calls the protocol's keys and reads its
        // values, nothing else.
        let keys: Option<Vec<Atom>> = proto
            .keys()
            .map(|ks| ks.iter().map(|k| self.atom(k)).collect());
        let mut reads: Vec<Key> = proto
            .reads()
            .iter()
            .map(|k| Key::Named(self.atom(k)))
            .collect();
        if proto == Protocol::Json {
            reads.push(Key::Any);
        }
        let calls = keys.as_ref().is_none_or(|k| !k.is_empty());
        if !calls && reads.is_empty() {
            return;
        }
        self.mark_use(t, proto.touch_bit());
        // A marker: the protocol runs on the values beside it; on the
        // marker it calls what the marker reaches under the protocol's
        // keys, an escape of that (`method_on`, DECISIONS 111). It has no
        // accessors, so its reads run nothing; what JSON serializes in turn
        // is what it reaches, touched (DECISIONS 45).
        if self.is_marker(t) {
            match &keys {
                None => {
                    let r = self.reach_of(t);
                    self.mark_escape_why(r, &|_| "protocol: on a marker".into());
                }
                Some(ks) => {
                    for &a in ks {
                        let m = self.marker_load(t, Some(a));
                        self.mark_escape_why(m, &|_| "protocol: on a marker".into());
                    }
                }
            }
            if proto == Protocol::Json {
                let r = self.reach_of(t);
                self.mark_use(r, crate::model::MARK_TOUCH);
            }
            return;
        }
        if t == TOP || !self.implicit_done.insert((t, proto)) {
            return;
        }
        // The protocol calls on a marker family's token are of its run
        // (`call_run`): what they find on it is the family's.
        let home = self.fam_home(t);
        // The values the protocol reads (getters run in the load) and, for
        // JSON, serializes in turn.
        for key in reads {
            let recv = self.node_l("implicit-recv");
            let dst = self.node_l("implicit-read");
            self.nodes[recv as usize].frame = home;
            self.nodes[dst as usize].frame = home;
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::Load { key, dst });
            if proto == Protocol::Json {
                self.cons(dst, Cons::Implicit { reason, proto });
            }
            self.add_tok(recv, t);
        }
        if !calls {
            return;
        }
        let ret = self.node_named("implicit-ret", || format!("implicit-ret({t})"));
        // Protocol results go back to the runtime only (touched below).
        self.nodes[ret as usize].sink = true;
        let arg = self.node_l("implicit-arg");
        self.nodes[ret as usize].frame = home;
        self.nodes[arg as usize].frame = home;
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
        match keys {
            None => self.method_on(t, Key::Any, call),
            Some(ks) => {
                for a in ks {
                    self.method_on(t, Key::Named(a), call);
                }
            }
        }
        // Results of protocol methods flow back to the runtime: what the
        // protocol runs on them (an iterator's `next`, a result's `value`,
        // what `toJSON` returns) is a touch of its own.
        if let Some(next) = proto.of_result() {
            self.cons(
                ret,
                Cons::Implicit {
                    reason,
                    proto: next,
                },
            );
        }
    }

    pub fn cons_line(&self, n: NodeId, c: &Cons) -> String {
        match c {
            Cons::Load { key, dst } => format!("load {n} {} {dst}", self.key_name(*key)),
            Cons::OwnValues { dst } => format!("ownvalues {n} {dst}"),
            Cons::CjsPublish { dst } => format!("cjspublish {n} {dst}"),
            Cons::Store { key, src, site } => {
                format!("store {n} {} {src} {site}", self.key_name(*key))
            }
            Cons::Call { call } => format!("call {n} {call}"),
            Cons::CallThis { call, this } => format!("callthis {n} {call} {this}"),
            Cons::New { call } => format!("new {n} {call}"),
            Cons::NewNamed { name, call } => format!(
                "newnamed {n} {} {call}",
                esc_key(&self.atoms[*name as usize])
            ),
            Cons::MCall { key, call } => format!("mcall {n} {} {call}", self.key_name(*key)),
            Cons::Chain { origin, key, act } => {
                format!("chain {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::TopSink { reason } => format!("top {n} {reason}"),
            Cons::ChainMark { reason, join, key } => {
                let k = key.map_or("-".to_string(), |k| self.key_name(k));
                format!("chainmark {n} {reason} {join} {k}")
            }
            Cons::HandOver { reason, ctor, arg } => format!("handover {n} {reason} {ctor} {arg}"),
            Cons::OwnOnly { f, dst } => format!("ownonly {n} {f} {dst}"),
            Cons::CallerValue { dst, reason } => format!("callervalue {n} {dst} {reason}"),
            Cons::StoreOut {
                f,
                dst,
                reach,
                via,
                key,
                reason,
                ..
            } => format!(
                "storeout {n} {f} {dst} {reach} {} {} {reason}",
                via.map_or("-".to_string(), |v| esc_key(&self.atoms[v as usize])),
                self.key_name(*key)
            ),
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
                this,
            } => format!(
                "contractthis {n} {tmpl} {reason} {} {}",
                *statics as u8, *this as i64
            ),
            Cons::OverrideThis { tmpl, key, reason } => format!(
                "overridethis {n} {tmpl} {} {reason}",
                self.key_name(Key::Named(*key))
            ),
            Cons::Guard { test, from, dst } => format!("guard {n} {test} {from} {dst}"),
            Cons::Defined { dst } => format!("defined {n} {dst}"),
            Cons::NotNullish { dst } => format!("notnullish {n} {dst}"),
            Cons::TopIfAny { dst, from, reason } => format!(
                "topifany {n} {dst} {from} {}",
                reason.map_or(-1, |r| r as i64)
            ),
            Cons::Hard { kind, reason } => format!("hard {n} {} {reason}", kind.code()),
            Cons::DeleteKey { key, reason } => {
                format!(
                    "deletekey {n} {} {reason}",
                    esc_key(&self.atoms[*key as usize])
                )
            }
            Cons::AddsKeys => format!("addskeys {n}"),
            Cons::Miss { key } => format!("miss {n} {}", self.key_name(*key)),
            Cons::DefineKey { key, data, reason } => {
                format!(
                    "definekey {n} {} {} {reason}",
                    self.key_name(Key::Named(*key)),
                    *data as u8
                )
            }
            Cons::DefineAll {
                o,
                descs,
                value,
                site,
                reason,
            } => format!("defineall {n} {o} {descs} {value} {site} {reason}"),
            Cons::DefineDesc {
                o,
                key,
                site,
                reason,
            } => format!(
                "definedesc {n} {o} {} {site} {reason}",
                self.key_name(Key::Named(*key))
            ),
            Cons::Implicit { reason, proto } => {
                format!("implicit {n} {reason} {}", proto.name())
            }
            Cons::CreateProto { reason } => format!("createproto {n} {reason}"),
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            } => {
                format!(
                    "chainunion {n} {} {dst} {of} {builtin}",
                    self.key_name(*key)
                )
            }
            Cons::ProtoGiven { protos } => format!("protogiven {n} {protos}"),
            Cons::ParentGiven { of } => format!("parentgiven {n} {of}"),
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
            Cons::SuperCtorParent {
                class,
                call,
                origin,
            } => format!("superctorparent {n} {class} {call} {origin}"),
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
                    "accinstall {n} {} {} {}",
                    self.key_name(Key::Named(*key)),
                    self.tok_fn(*f),
                    *setter as u8
                )
            }
            Cons::ProtoFrom { src } => format!("protofrom {n} {src}"),
            Cons::BirthField { key, src } => {
                format!("birthfield {n} {} {src}", self.key_name(*key))
            }
            Cons::ProtoOf { dst } => format!("protoof {n} {dst}"),
            Cons::ProtoValues { dst } => format!("protovalues {n} {dst}"),
            Cons::PlainKey { origin, act } => format!("plainkey {n} {origin} {act:?}"),
            Cons::IterNext { dst } => format!("iternext {n} {dst}"),
            Cons::NewTargetHard { reason } => format!("newtargethard {n} {reason}"),
            Cons::ShareBuffer { ta } => format!("sharebuffer {n} {ta}"),
            Cons::Arith { dst } => format!("arith {n} {dst}"),
            Cons::Plus { str, num } => format!("plus {n} {str} {num}"),
            Cons::ArrayLenArg { dst } => format!("arraylenarg {n} {dst}"),
            Cons::IdxLoad { idx, dst } => format!("idxload {n} {idx} {dst}"),
            Cons::IdxKey { obj, dst } => format!("idxkey {n} {obj} {dst}"),
            Cons::IdxMCall { idx, call } => format!("idxmcall {n} {idx} {call}"),
            Cons::IdxKeyCall { obj, prim, call } => format!("idxkeycall {n} {obj} {prim} {call}"),
            Cons::ForInBody { obj, call } => format!("forinbody {n} {obj} {call}"),
            Cons::ForInKeys { ft, call } => format!("forinkeys {n} {ft} {call}"),
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => format!("definekeyed {n} {o} {reason} {}", *accessor as u8),
            Cons::StoreKeyed { o, src, site } => format!("storekeyed {n} {o} {src} {site}"),
            Cons::StoreIndexed { o, src, site } => format!("storeindexed {n} {o} {src} {site}"),
            Cons::AccessorKeyed { holder, f, setter } => {
                format!("accessorkeyed {n} {holder} {f} {}", *setter as u8)
            }
            Cons::ThisTo { origin } => format!("thisto {n} {origin}"),
            Cons::CallArgs { call } => format!("callargs {n} {call}"),
            Cons::RetInto { call, run } => format!("retinto {n} {call} {run}"),
            Cons::ArgInto { call, run, i } => format!("arginto {n} {call} {run} {i}"),
            Cons::Spread { into, site } => format!("spread {n} {into} {site}"),
            Cons::SpreadFrom { src, site } => format!("spreadfrom {n} {src} {site}"),
            Cons::ThisInst { call, ft } => format!("thisinst {n} {call} {ft}"),
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
            TokKind::Obj(tm, _) => match self.tmpls[*tm as usize].kind {
                TmplKind::Class { .. } => "class instance",
                TmplKind::Lit { .. } => "literal",
                TmplKind::FnInst { .. } => "fn instance",
                TmplKind::Arr { .. } => "array",
                TmplKind::Args { .. } => "arguments object",
                TmplKind::ObjCreate { .. } => "Object.create object",
                TmplKind::BObj { .. } => "builtin object",
            },
            TokKind::Func(_, _) => "function",
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

/// May key `name` be ToString of a Number (an index, `1.5`, `-1`, `1e+21`,
/// `NaN`, `Infinity`)? A store under a Number key lands there. A superset
/// is safe: such a key reads the elements as well.
pub(crate) fn numeric_key(name: &str) -> bool {
    name.parse::<f64>().is_ok() || matches!(name, "NaN" | "Infinity" | "-Infinity")
}
