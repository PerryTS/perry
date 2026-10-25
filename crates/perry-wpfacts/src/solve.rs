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
mod lookup;

use crate::builtins;
mod call_and_construct;
use crate::delta::{Targets, Tokens};
use crate::model::*;

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
        if top_new && self.explain {
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

    /// Attach a complex constraint and apply it to what the node holds now.
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        self.stats_cons += 1;
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
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = n;
        self.cur_why = u32::MAX;
        self.with_s(|m| m.apply(n, &c, &toks, prim));
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    pub fn solve(&mut self) {
        self.rel_phase = b'S';
        self.mark_params();
        self.members_settled.get_mut().clear();
        loop {
            self.fill_instances();
            self.replay_marker_links();
            let Some(n) = self.wl.pop_front() else {
                if self.sum.pending.is_empty() && self.marker_links.is_empty() && !self.settle_bf_written() {
                    break;
                }
                continue;
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

    pub fn field(&mut self, t: Tok, a: Atom) -> NodeId {
        if let Some(&n) = self.fields.get(&(t, a)) {
            return n;
        }
        let n = self.node_named("field", || format!("field({t},{a})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
        self.fields.insert((t, a), n);
        self.field_keys.entry(t).or_default().push(a);
        self.rel(|| format!("fieldnode {n} {t} {a}"));
        // A marker has no fields (`store_into`): its any-key field holds
        // only its reach marker. Under a named key it holds that key's
        // reach marker instead (design §18.4).
        // A path marker's named field holds its step (DECISIONS 158).
        if self.is_marker(t) && (!self.reach_marker.contains(&t) || self.is_path_marker(t)) {
            let r = self.keyed_reach_of(t, a);
            self.add_tok(n, r);
        } else {
            let any = self.field_any(t);
            self.edge(any, n);
        }
        let all = self.all_fields(t);
        self.edge(n, all);
        // A key gained after a spread of `t`: the targets get it too.
        if let Some(v) = self.spread_to.get(&t).cloned() {
            for (d, site) in v {
                self.store_into(d, Key::Named(a), n, site);
            }
        }
        // DECISIONS 94: a named field of an array whose elements are a cell.
        if let Some(&seed) = self.cell_arrs.get(&t) {
            self.edge(seed, n);
        }
        n
    }

    /// Object spread of source `s` into target `d` (`{...s}`,
    /// `Object.assign(d, s)`): every own key of `s`, now and later, is
    /// stored under the same key on `d`; computed keys and getter results
    /// of `s` reach `d`'s any-key field. An unmodeled source makes `d`'s
    /// keys unknown; an unmodeled target takes `s`'s values.
    fn spread_into(&mut self, s: Tok, d: Tok, site: SiteId) {
        if !self.spread_done.insert((s, d)) {
            return;
        }
        if d == TOP || matches!(self.toks[d as usize], TokKind::BuiltinFn) {
            let r = self.reason("spread into a value from unmodeled code");
            let all = if s == TOP {
                self.top_tok_node
            } else {
                self.all_fields(s)
            };
            self.sink_node(all, r);
            return;
        }
        let opaque = match &self.toks[s as usize] {
            TokKind::Top | TokKind::Builtin(_) | TokKind::BuiltinFn | TokKind::NativeProto(_) => {
                true
            }
            TokKind::Obj(tm, _) => matches!(self.tmpls[*tm as usize].kind,
                TmplKind::BObj { .. }),
            _ => false,
        };
        let fa_d = self.field_any(d);
        if opaque {
            self.add_top(fa_d, |_| "spread of an unmodeled object".into());
            self.add_prim(fa_d, prim::ALL);
            if s == TOP {
                return;
            }
        }
        self.spread_to.entry(s).or_default().push((d, site));
        for a in self.field_keys.get(&s).cloned().unwrap_or_default() {
            let f = self.field(s, a);
            self.store_into(d, Key::Named(a), f, site);
        }
        let fa_s = self.field_any(s);
        self.edge(fa_s, fa_d);
        // Getters of `s` (own and on the chain) run; their results have no
        // known key here.
        let act = ChainAct::Load(fa_d);
        self.any_walk_holders.entry(s).or_default().push((s, act));
        self.accessor_hit(s, s, Key::Any, act);
    }

    pub fn field_any(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.field_any.get(&t) {
            return n;
        }
        let n = self.node_named("fieldany", || format!("fieldany({t})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
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
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
        self.all_fields.insert(t, n);
        self.rel(|| format!("allfields {n} {t}"));
        // A path marker read under a computed key: its `[]` step
        // (DECISIONS 158).
        if self.is_path_marker(t) {
            let s = self.path_step_of(t, None);
            self.add_tok(n, s);
        }
        n
    }

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
                    None => self.add_tok(n, objproto),
                },
            },
            TokKind::NativeProto(_) => self.add_tok(n, objproto),
            TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn => {
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
        if self.sum_skip(n, c) {
            return;
        }
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

    fn apply_inner(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
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
                    let opaque = match &self.toks[t as usize] {
                        TokKind::Top
                        | TokKind::Builtin(_)
                        | TokKind::BuiltinFn
                        | TokKind::NativeProto(_) => true,
                        TokKind::Obj(tm, _) => matches!(self.tmpls[*tm as usize].kind,
                            TmplKind::BObj { .. }),
                        _ => false,
                    };
                    if opaque {
                        self.add_top(dst, |_| "own values of an unmodeled object".into());
                        self.add_prim(dst, prim::ALL);
                    }
                    if t == TOP {
                        continue;
                    }
                    let fields = self.all_fields(t);
                    self.edge(fields, dst);
                    let act = ChainAct::Load(dst);
                    // Any-key lookups subscribe through the holder relation;
                    // named-key replay in `visits` does not cover them.
                    self.any_walk_holders.entry(t).or_default().push((t, act));
                    self.accessor_hit(t, t, Key::Any, act);
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
                for &t in toks {
                    self.escape(t, reason);
                }
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
                            self.mark_use(at, crate::model::MARK_ESCAPE);
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
            Cons::TopIfAny { dst, from, reason } => {
                if !toks.is_empty() || prim != 0 {
                    if let Some(r) = reason {
                        self.add_top(dst, |m| m.reasons[r as usize].clone());
                        self.add_prim(dst, prim::ALL);
                    }
                    self.edge(from, dst);
                }
            }
            Cons::Hard { kind, reason } => {
                for &t in toks {
                    self.mark_hard(t, kind, reason);
                }
            }
            Cons::DefineAccessor { key, reason } => {
                for &t in toks {
                    // A key the layout already holds as data may move into an
                    // accessor pair in its slot, and a class instance's layout
                    // is the class's: E5 HARD, as any defineProperty was. (On
                    // another unit's layout the accessor is beyond its contract
                    // in any case: the census breaks it.)
                    let new_key = match self.toks[t as usize] {
                        TokKind::Obj(tm, _) => {
                            let tpl = &self.tmpls[tm as usize];
                            matches!(
                                tpl.kind,
                                TmplKind::Lit { .. }
                                    | TmplKind::Class {
                                        literal_shape: true,
                                        ..
                                    }
                            ) && !tpl.birth_keys.contains(&key)
                        }
                        _ => false,
                    };
                    if !new_key {
                        self.mark_hard(t, Hard::E5DefineProperty, reason);
                    } else if let Some(tm) = self.tmpl_of(t) {
                        *self.adds_keys.entry(tm).or_default() += 1;
                    }
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
            Cons::Implicit { reason } => {
                for &t in toks {
                    match self.toks[t as usize] {
                        // A function handed to the runtime: it may be
                        // called back with anything.
                        TokKind::Func(_, _) | TokKind::Bound(_) => self.escape(t, reason),
                        _ => self.implicit(t, reason),
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
                    if self.is_array_length(q, key) {
                        self.add_prim(dst, prim::NUM);
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
                    let reason = self.reason("E6 constructor follows live superclass");
                    self.mark_hard(origin, Hard::E6SetPrototypeOf, reason);
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
                        Key::Any => self.field_any(t),
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
                        let r = self.reason("accessor under a computed key");
                        self.escape(f, r);
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
                    if let TokKind::ClassObj(tm) = self.toks[t as usize] {
                        let instance = self.tok(TokKind::Obj(tm, NO_CTX));
                        let reason =
                            self.reason("E6 live superclass may change constructor birth fields");
                        self.mark_hard(instance, Hard::E6SetPrototypeOf, reason);
                    }
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
                    self.this_to(t, origin);
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

    /// DECISIONS 118/149: `src` stored under `key` into parameter `i`'s
    /// argument of contract function `f` (or what it reaches, marker `t`)
    /// is a flow of `f`'s contract (`Cons::StoreOut`).
    #[allow(clippy::too_many_arguments)]
    fn store_flow_out(
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
    fn rooted_store(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) -> bool {
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
    fn rooted_own_store(&mut self, t: Tok, src: NodeId, site: SiteId) -> NodeId {
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
    fn write_builtin_fns(&mut self, r: u32) {
        if self.bf_written.is_some() {
            return;
        }
        self.bf_written = Some(r);
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
    fn settle_bf_written(&mut self) -> bool {
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

    /// DECISIONS 187: stores `ss`, made on an argument marker, reach what
    /// each marker of `from` stands for under its key (`None`: the marker
    /// itself; `Some(k)`: its reach under `k`, DECISIONS 121). An argument marker stands for
    /// its members; its keyed reach under `k` for its parent's members'
    /// reaches under `k`, and has no members of its own (`link_marker` links
    /// only into a parameter's argument marker and its reach). Each (marker,
    /// key) is visited once and each visit is solve work (XUC-1), so a
    /// member several argument markers stand for takes the stores once and
    /// no argument marker on the way keeps a copy: a store costs the
    /// markers below it, not the paths to them.
    pub(crate) fn arg_store_fan(
        &mut self,
        from: Vec<(Tok, Option<Atom>)>,
        ss: &[(Atom, NodeId, SiteId)],
    ) {
        let mut st = from;
        let mut seen = crate::pts::FxHashSet::default();
        while let Some((x, k)) = st.pop() {
            if !seen.insert((x, k)) {
                continue;
            }
            self.work += 1;
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return;
            }
            if !self.is_arg_marker(x) {
                let y = match k {
                    Some(k) => self.keyed_reach_of(x, k),
                    None => x,
                };
                for &(a, src, site) in ss {
                    self.store_into(y, Key::Named(a), src, site);
                }
                continue;
            }
            match (k, self.keyed_parent.get(&x).copied()) {
                (None, Some((p, k))) => {
                    for m in self.arg_members.get(&p).cloned().unwrap_or_default() {
                        st.push((m, Some(k)));
                    }
                }
                // A keyed reach's own reach: no member stands under it.
                (Some(_), Some(_)) => {}
                (k, None) => {
                    if k.is_none() {
                        for &(a, src, site) in ss {
                            self.arg_store_on(x, a, src, site);
                        }
                    }
                    for m in self.arg_members.get(&x).cloned().unwrap_or_default() {
                        st.push((m, k));
                    }
                }
            }
        }
    }

    /// What a store under named key `a` does to argument marker `t` itself:
    /// the published classes it stands for gain the key (`virt_revoke`),
    /// and a contract receiver takes it (`contract_recv`).
    fn arg_store_on(&mut self, t: Tok, a: Atom, src: NodeId, site: SiteId) {
        if self.virt_protos.contains_key(&t) {
            self.virt_revoke(t, Some(a));
        }
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Store(Key::Named(a), src, site));
            for i in is {
                self.store_into(i, Key::Named(a), src, site);
            }
        }
    }

    /// DECISIONS 187: the argument markers `t` stands in and itself.
    fn arg_markers_above(&mut self, t: Tok) -> Vec<Tok> {
        let mut out = Vec::new();
        let mut st = vec![t];
        let mut seen = crate::pts::FxHashSet::default();
        while let Some(x) = st.pop() {
            if !seen.insert(x) {
                continue;
            }
            self.work += 1;
            out.push(x);
            st.extend(self.arg_parents.get(&x).into_iter().flatten().copied());
        }
        out
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
        // alone, not into all its members reach (DECISIONS 121).
        if let (Key::Named(a), true) = (key, self.is_arg_marker(t)) {
            if self.aborted {
                return;
            }
            if self.arg_store_done.insert((t, a, src)) {
                // DECISIONS 187: kept on `t` alone, where it was made (the
                // links replay it from there), and walked once to what `t`
                // stands for.
                self.arg_stores.entry(t).or_default().push((a, src, site));
                let from: Vec<(Tok, Option<Atom>)> = match self.keyed_parent.get(&t).copied() {
                    Some((p, k)) => self
                        .arg_members
                        .get(&p)
                        .into_iter()
                        .flatten()
                        .map(|&m| (m, Some(k)))
                        .collect(),
                    None => self
                        .arg_members
                        .get(&t)
                        .into_iter()
                        .flatten()
                        .map(|&m| (m, None))
                        .collect(),
                };
                self.arg_store_fan(from, &[(a, src, site)]);
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
        // (definite): the store adds no key (CW2 C, binarytrees_arena).
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
                        .any(|f| f.data_field && &f.key == k && f.definite),
                    _ => false,
                }
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
                _ => {
                    self.foreign_written.insert(tm);
                }
            }
        }
        if self.is_array_length(t, key) {
            let r = self.reason("ToNumber of an array length store");
            self.cons(src, Cons::Implicit { reason: r });
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
                    let w = self.writers.entry((tm, a)).or_default();
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

    /// `t`, a receiver token or a receiver marker, stands for instances of
    /// `classes` (DECISIONS 59): its prototypes are theirs, and a named load
    /// on it finds the chain's value; the own field (unknown for these
    /// tokens) only once the key may be an own key of such an instance
    /// (`virt_load`).
    pub(crate) fn register_virt(&mut self, t: Tok, classes: &[TmplId]) {
        let ps = self.virt_protos.entry(t).or_default();
        let fresh: Vec<TmplId> = classes
            .iter()
            .copied()
            .filter(|c| !ps.contains(c))
            .collect();
        ps.extend(fresh.iter().copied());
        for &c in &fresh {
            self.virt_by_class.entry(c).or_default().push(t);
        }
        if let Some(&pn) = self.proto_node.get(&t) {
            for p in fresh {
                let pt = self.tok(TokKind::Proto(p));
                self.add_tok(pn, pt);
            }
        }
    }

    /// The load node of key `a` on receiver token / marker `t` (`None`: `t`
    /// is neither). It holds the chain's values, and the own field once `a`
    /// may be an own key of an instance `t` stands for: the class code
    /// stores `a` (or a computed key) into one, uses a hard operation on one
    /// or lets one escape; a subclass of another unit defines `a` (census,
    /// `override_keys`); an accessor on the chain may answer it.
    pub(crate) fn virt_load(&mut self, t: Tok, a: Atom) -> Option<NodeId> {
        let classes = self.virt_protos.get(&t)?.clone();
        if let Some(&n) = self.virt_nodes.get(&(t, a)) {
            return Some(n);
        }
        let n = self.node_l("virt-load");
        self.virt_nodes.insert((t, a), n);
        self.virt_tok_keys.entry(t).or_default().push(a);
        let cv = self.cv(t, Key::Named(a));
        self.edge(cv, n);
        let own = self.virt_own(&classes, a);
        if own {
            let f = self.field(t, a);
            self.edge(f, n);
        }
        Some(n)
    }

    /// Key `a` may be an own key of the instances a receiver of `classes`
    /// stands for in this unit's view: the class code stores it, uses a
    /// hard operation on one or lets one escape, or an accessor on the
    /// chain may answer it.
    fn virt_own(&self, classes: &[TmplId], a: Atom) -> bool {
        classes.iter().any(|&p| {
            self.virt_open.contains(&p)
                || self.virt_revoked.contains(&(p, a))
                || self.chain_accessor_key(p, a)
        })
    }

    /// The classes of receiver token / marker `t` that another unit's
    /// subclass overrides `a` of (census, `override_keys`).
    pub(crate) fn virt_overriders(&mut self, t: Tok, a: Atom) -> Vec<TmplId> {
        let star = self.atom(crate::STAR_KEY);
        let Some(classes) = self.virt_protos.get(&t) else {
            return Vec::new();
        };
        classes
            .iter()
            .copied()
            .filter(|p| {
                self.override_keys
                    .get(p)
                    .is_some_and(|v| v.contains(&a) || v.contains(&star))
            })
            .collect()
    }

    /// The callee of `t.a(...)` beyond the chain, on receiver token /
    /// marker `t` (DECISIONS 97): when `a` is own only because another
    /// unit's subclass defines it, the call runs that subclass's method on
    /// a subclass instance, i.e. the override token of each such class
    /// (`override_node`); else nothing (`virt_load` holds the own field).
    pub(crate) fn virt_call(&mut self, t: Tok, a: Atom) -> Vec<NodeId> {
        let classes = self.virt_protos.get(&t).cloned().unwrap_or_default();
        if self.virt_own(&classes, a) {
            return Vec::new();
        }
        let ps = self.virt_overriders(t, a);
        ps.into_iter().map(|p| self.override_node(p, a)).collect()
    }

    /// The node holding the override token of key `a` of published class
    /// `tm` (DECISIONS 97): the function another unit's subclass defines
    /// under `a`, as a call through a receiver of `tm` sees it. That unit
    /// runs it unseen: the arguments escape, the result is unknown, and its
    /// receiver is checked (`Cons::OverrideThis`).
    fn override_node(&mut self, tm: TmplId, a: Atom) -> NodeId {
        if let Some(&n) = self.override_fns.get(&(tm, a)) {
            return n;
        }
        let fx = self.funcs.len() as FnIdx;
        let tok = self.tok(TokKind::Func(fx, NO_CTX));
        let this = self.node_l("this(override)");
        let ret = self.node_l("ret(override)");
        let args_all = self.node_l("args(override)");
        let name = format!(
            "<override {} of {}>",
            self.atoms[a as usize], self.tmpls[tm as usize].display
        );
        self.funcs.push(FuncInfo {
            module: 0,
            func_id: u32::MAX,
            name,
            params: Vec::new(),
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: true,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_step: false,
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded: false,
        });
        let r = self.reason("passed to an unknown function");
        self.sink_node(args_all, r);
        self.add_top(ret, |_| "result of an unknown function".to_string());
        self.add_prim(ret, prim::ALL);
        let rt = self.reason("receiver of a call of an unknown function value");
        self.cons(
            this,
            Cons::OverrideThis {
                tmpl: tm,
                key: a,
                reason: rt,
            },
        );
        let n = self.node_l("override-callee");
        self.add_tok(n, tok);
        self.override_fns.insert((tm, a), n);
        n
    }

    fn chain_accessor_key(&self, tm: TmplId, a: Atom) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            let t = &self.tmpls[c as usize];
            if t.accessor_keys.contains(&a) || t.has_computed_members {
                return true;
            }
            cur = t.parent;
        }
        false
    }

    /// `t` gains own key `a` (`None`: any key): for each published class
    /// whose instances `t` is or stands for, the key is open on its
    /// receiver token and markers from now on (`virt_load`).
    pub(crate) fn virt_revoke(&mut self, t: Tok, key: Option<Atom>) {
        if self.virt_by_class.is_empty() {
            return;
        }
        let classes: Vec<TmplId> = match self.virt_protos.get(&t) {
            Some(ps) => ps.clone(),
            None => {
                let Some(tm) = self.tmpl_of(t) else { return };
                let mut out = Vec::new();
                let mut work = vec![tm];
                let mut seen: Vec<TmplId> = Vec::new();
                while let Some(c) = work.pop() {
                    if seen.contains(&c) {
                        continue;
                    }
                    seen.push(c);
                    if self.virt_by_class.contains_key(&c) {
                        out.push(c);
                    }
                    let tp = &self.tmpls[c as usize];
                    work.extend(tp.parent);
                    work.extend(tp.parent_links.iter().copied());
                }
                out
            }
        };
        for p in classes {
            if self.virt_open.contains(&p) {
                continue;
            }
            let opened: Option<Atom> = match key {
                Some(a) => {
                    if !self.virt_revoked.insert((p, a)) {
                        continue;
                    }
                    Some(a)
                }
                None => {
                    self.virt_open.insert(p);
                    None
                }
            };
            for vt in self.virt_by_class.get(&p).cloned().unwrap_or_default() {
                let keys: Vec<Atom> = match opened {
                    Some(a) => vec![a],
                    None => self.virt_tok_keys.get(&vt).cloned().unwrap_or_default(),
                };
                for a in keys {
                    if let Some(&n) = self.virt_nodes.get(&(vt, a)) {
                        let f = self.field(vt, a);
                        self.edge(f, n);
                    }
                }
            }
        }
    }

    /// Is `owner` `tm` or an ancestor of it: the static parent, then the
    /// parents `super()` ran (`parent_links`, §17.2).
    pub(crate) fn has_ancestor(&self, tm: TmplId, owner: TmplId) -> bool {
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if c == owner {
                return true;
            }
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let t = &self.tmpls[c as usize];
            work.extend(t.parent);
            work.extend(t.parent_links.iter().copied());
        }
        false
    }

    /// `t` reaches `this` of another unit's method of class `tmpl` whose
    /// receiver contract is `this` (DECISIONS 57): admitted when `t` is an
    /// instance of a subclass of `tmpl` and the method neither uses its
    /// receiver beyond reading it nor reads a key a subclass accessor may
    /// answer; then the method's published effects happen to `t` (the
    /// stored keys hold unknown values, what it reaches is escaped or
    /// touched as published). False: `t` escapes.
    pub(crate) fn admit_this(&mut self, t: Tok, tmpl: TmplId, this: u32, reason: u32) -> bool {
        use crate::model::{MARK_ESCAPE, MARK_READ, MARK_TOUCH};
        let Some(c) = self.this_contracts.get(this as usize).cloned() else {
            return false;
        };
        if c.uses & (MARK_ESCAPE | MARK_TOUCH) != 0 {
            return false;
        }
        // A published receiver token stands for its class's instances; a
        // receiver marker for its classes' (DECISIONS 59): the method runs
        // on them, so its published effects on its receiver are the
        // marker's own (they compose, as arguments do).
        let marker = matches!(self.param_marker.get(&t), Some(&(_, u32::MAX)))
            && !self.reach_marker.contains(&t);
        let classes: Vec<TmplId> = match self.toks[t as usize] {
            _ if marker => self.virt_protos.get(&t).cloned().unwrap_or_default(),
            _ if self.recv_class.contains_key(&t) => vec![self.recv_class[&t]],
            TokKind::Obj(tm, _) if tm != tmpl => vec![tm],
            _ => Vec::new(),
        };
        if classes.is_empty()
            || classes
                .iter()
                .any(|&tm| !self.has_ancestor(tm, tmpl) || (!marker && tm == tmpl))
        {
            return false;
        }
        // A read may run a subclass getter unseen by the method's unit.
        if c.uses & MARK_READ != 0 && classes.iter().any(|&tm| self.chain_has_accessors(tm, tmpl)) {
            return false;
        }
        if marker {
            let (f, _) = self.param_marker[&t];
            self.mark_use(t, c.uses);
            if let Some(&r) = self.marker_reach.get(&t) {
                self.mark_use(r, c.reach);
            }
            match &c.stores {
                Some(ks) => {
                    for k in ks {
                        let a = self.atom(k);
                        self.virt_revoke(t, Some(a));
                        let ks = self.this_stores.entry(f).or_default();
                        if !ks.contains(&a) {
                            ks.push(a);
                        }
                    }
                }
                None => {
                    self.virt_revoke(t, None);
                    self.this_open.insert(f);
                }
            }
            return true;
        }
        let why = |_: &Self| "field of another unit's class".to_string();
        let top = self.node_l("admitted-this-store");
        self.add_top(top, why);
        self.add_prim(top, prim::ALL);
        let site = self.anon_post_site("store by another unit's method");
        match &c.stores {
            Some(ks) => {
                // DECISIONS 179: what the method's flow stores there.
                for (i, k) in ks.iter().enumerate() {
                    let a = self.atom(k);
                    let v = c.values.get(i).copied().unwrap_or(top);
                    self.store_into(t, Key::Named(a), v, site);
                }
            }
            None => self.store_into(t, Key::Any, top, site),
        }
        // What the receiver reaches (its fields, transitively), as for an
        // argument (gen.rs `stand_in_fn_of`).
        if c.reach & (MARK_ESCAPE | MARK_TOUCH) != 0 {
            let h = self.node_l("admitted-this");
            let q = self.node_l("admitted-this-reach");
            self.cons(
                h,
                Cons::Load {
                    key: Key::Any,
                    dst: q,
                },
            );
            self.cons(
                q,
                Cons::Load {
                    key: Key::Any,
                    dst: q,
                },
            );
            if c.reach & MARK_ESCAPE != 0 {
                self.sink_node(q, reason);
            } else {
                self.cons(q, Cons::Implicit { reason });
            }
            self.add_tok(h, t);
        }
        true
    }

    /// A template from `tm` up to (not including) ancestor `owner` defines
    /// an accessor or computed members.
    fn chain_has_accessors(&self, tm: TmplId, owner: TmplId) -> bool {
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if c == owner || seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let t = &self.tmpls[c as usize];
            if !t.accessor_keys.is_empty() || t.has_computed_members {
                return true;
            }
            work.extend(t.parent);
            work.extend(t.parent_links.iter().copied());
        }
        false
    }

    /// DECISIONS 118: what an argument marker's stores and `StoreOut`s do
    /// to each marker that linked in since (`link_marker`).
    fn replay_marker_links(&mut self) {
        while let Some((m, r)) = self.marker_links.pop() {
            if self.aborted {
                self.marker_links.clear();
                return;
            }
            // DECISIONS 187: a store stays on the marker it was made on. What
            // reaches `r` is made on `r` or an argument marker above it; what
            // reaches `r`'s reach under `k`, on such a marker's keyed reach
            // under `k` or on an argument marker that keyed reach is a member
            // of, or above that. `m` takes them as `r` would.
            let above = self.arg_markers_above(r);
            let mut plain: Vec<(Atom, NodeId, SiteId)> = Vec::new();
            let mut keyed: Vec<(Atom, Vec<(Atom, NodeId, SiteId)>)> = Vec::new();
            for &x in &above {
                plain.extend(self.arg_stores.get(&x).into_iter().flatten().copied());
                for (k, kr) in self.keyed_reach.get(&x).cloned().unwrap_or_default() {
                    let mut ss = Vec::new();
                    for y in self.arg_markers_above(kr) {
                        ss.extend(self.arg_stores.get(&y).into_iter().flatten().copied());
                    }
                    if !ss.is_empty() {
                        keyed.push((k, ss));
                    }
                }
            }
            let dedup = |v: &mut Vec<(Atom, NodeId, SiteId)>| {
                v.sort_unstable();
                v.dedup_by_key(|&mut (a, src, _)| (a, src));
            };
            dedup(&mut plain);
            if !plain.is_empty() {
                self.arg_store_fan(vec![(m, None)], &plain);
            }
            keyed.sort_by_key(|&(k, _)| k);
            let mut i = 0;
            while i < keyed.len() {
                let k = keyed[i].0;
                let mut ss = Vec::new();
                while i < keyed.len() && keyed[i].0 == k {
                    ss.append(&mut keyed[i].1);
                    i += 1;
                }
                dedup(&mut ss);
                self.arg_store_fan(vec![(m, Some(k))], &ss);
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
                    Some(at) => self.mark_use(at, crate::model::MARK_ESCAPE),
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

    // ---------------------------------------------------------------- calls

    /// Builtin protocol touch on `t` (ToPrimitive, iteration, toJSON, then,
    /// inspect …): every function reachable as a property of `t` or its
    /// chain runs with `this` = `t` and unknown primitive arguments.
    pub fn implicit(&mut self, t: Tok, reason: u32) {
        self.mark_use(t, crate::model::MARK_TOUCH);
        // A marker: the protocol runs on the values beside it; on the
        // marker it calls what the marker reaches, an escape of that
        // (`method_on`, DECISIONS 111).
        if self.is_marker(t) {
            let r = self.reach_of(t);
            self.mark_use(r, crate::model::MARK_ESCAPE);
            return;
        }
        if t == TOP || !self.implicit_done.insert(t) {
            return;
        }
        let _ = reason;
        let ret = self.node_named("implicit-ret", || format!("implicit-ret({t})"));
        // Protocol results go back to the runtime only (touched below).
        self.nodes[ret as usize].sink = true;
        let arg = self.node_l("implicit-arg");
        // The protocol calls on a marker family's token are of its run
        // (`call_run`): what they find on it is the family's.
        let home = self.fam_home(t);
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
        self.method_on(t, Key::Any, call);
        // Results of protocol methods flow back to the runtime (iterator
        // results, ToPrimitive values, then-ables): treat as touched too.
        self.cons(ret, Cons::Implicit { reason });
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
            Cons::HandOver { reason, ctor, arg } => format!("handover {n} {reason} {ctor} {arg}"),
            Cons::OwnOnly { f, dst } => format!("ownonly {n} {f} {dst}"),
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
            Cons::TopIfAny { dst, from, reason } => format!(
                "topifany {n} {dst} {from} {}",
                reason.map_or(-1, |r| r as i64)
            ),
            Cons::Hard { kind, reason } => format!("hard {n} {} {reason}", kind.code()),
            Cons::AddsKeys => format!("addskeys {n}"),
            Cons::DefineAccessor { key, reason } => {
                format!(
                    "defineaccessor {n} {} {reason}",
                    self.key_name(Key::Named(*key))
                )
            }
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
            Cons::IterNext { dst } => format!("iternext {n} {dst}"),
            Cons::NewTargetHard { reason } => format!("newtargethard {n} {reason}"),
            Cons::ShareBuffer { ta } => format!("sharebuffer {n} {ta}"),
            Cons::Arith { dst, add } => format!("arith {n} {dst} {}", *add as u8),
            Cons::ArrayLenArg { dst } => format!("arraylenarg {n} {dst}"),
            Cons::IdxLoad { idx, dst } => format!("idxload {n} {idx} {dst}"),
            Cons::IdxKey { obj, dst } => format!("idxkey {n} {obj} {dst}"),
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
