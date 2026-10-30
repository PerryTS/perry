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
            if self.explain {
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
        self.with_s(|m| m.apply(n, &c, &toks, prim));
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
            // An array's named non-index key is its own key: its elements
            // (`field_any`) are read under an index only (`named_any`). A
            // module namespace's named key is resolved by name (DECISIONS
            // 215): it never reads the namespace's other keys.
            let named = matches!(self.toks[t as usize], TokKind::ModNs(_))
                || self.is_elem_array(t) && !numeric_key(&self.atoms[a as usize]);
            let any = match named {
                true => self.named_any(t),
                false => self.field_any(t),
            };
            self.edge(any, n);
            if let Some(srcs) = self.star_from.get(&t).cloned() {
                for s in srcs {
                    self.star_key(t, s, a, n);
                }
            }
        }
        let all = self.all_fields(t);
        self.edge(n, all);
        if let Some(&nf) = self.num_fields.get(&t) {
            if self.is_num_atom(a) {
                self.edge(n, nf);
            }
        }
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
            TokKind::Obj(tm, _) => matches!(self.tmpls[*tm as usize].kind, TmplKind::BObj { .. }),
            _ => false,
        };
        let fa_d = self.any_key_node(d);
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

    /// An Array of the program: its elements (`field_any`: what its
    /// construction, `push` and the like put at an index, and every store
    /// under a computed key) are no value of a named non-index key, which
    /// reads `named_any` instead. A marker's fields are its reach markers.
    pub fn is_elem_array(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
            && !self.is_marker(t)
    }

    /// The values of `t` under a key no store named: for an Array the ones
    /// its named non-index keys read (`named_any`, flowing on into its
    /// elements: the key may be an index too), else `field_any`. Every
    /// write under an unknown key goes here: a computed-key store, unknown
    /// code's writes (escape, hard), a spread, a computed-key accessor.
    pub fn any_key_node(&mut self, t: Tok) -> NodeId {
        match self.is_elem_array(t) {
            true => self.named_any(t),
            false => self.field_any(t),
        }
    }

    /// DECISIONS 215 (ECMAScript ResolveExport): `export * from` the module
    /// of namespace `src` (`TOP`: a source outside the program) makes each
    /// name of `src` a name of namespace `ns`, unless `ns`'s module exports
    /// that name itself; `default` is never one. A computed key reads every
    /// key (`field_any`); a named key reads only that name of each source.
    pub(crate) fn export_star(&mut self, ns: Tok, src: Tok) {
        let fa = self.field_any(ns);
        if src == TOP {
            self.add_top(fa, |_| "unresolved export * source".into());
        } else {
            let all = self.all_fields(src);
            self.edge(all, fa);
        }
        let v = self.star_from.entry(ns).or_default();
        if v.contains(&src) {
            return;
        }
        v.push(src);
        // The names asked of `ns` before this source was known.
        let keys = self.field_keys.get(&ns).cloned().unwrap_or_default();
        for a in keys {
            let n = self.fields[&(ns, a)];
            self.star_key(ns, src, a, n);
        }
    }

    /// Name `a` of namespace `ns` (node `n`) through its `export *` source
    /// `src` (`export_star`).
    fn star_key(&mut self, ns: Tok, src: Tok, a: Atom, n: NodeId) {
        if self.ns_own.contains(&(ns, a)) || &*self.atoms[a as usize] == "default" {
            return;
        }
        if src == TOP {
            self.add_top(n, |_| "unresolved export * source".into());
        } else {
            let f = self.field(src, a);
            self.edge(f, n);
        }
    }

    /// An Array's values under a computed key that may be no index
    /// (`is_elem_array`): its named non-index keys read them, and they are
    /// among its elements as well.
    pub fn named_any(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.named_any.get(&t) {
            return n;
        }
        let n = self.node_named("namedany", || format!("namedany({t})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.named_any.insert(t, n);
        self.rel(|| format!("namedany {n} {t}"));
        let any = self.field_any(t);
        self.edge(n, any);
        n
    }

    /// dst ⊇ the values of `t`'s own properties (`Cons::OwnValues`).
    pub(crate) fn own_values_of(&mut self, t: Tok, dst: NodeId) {
        let opaque = match &self.toks[t as usize] {
            TokKind::Top | TokKind::Builtin(_) | TokKind::BuiltinFn | TokKind::NativeProto(_) => {
                true
            }
            TokKind::Obj(tm, _) => matches!(self.tmpls[*tm as usize].kind, TmplKind::BObj { .. }),
            _ => false,
        };
        if opaque {
            self.add_top(dst, |_| "own values of an unmodeled object".into());
            self.add_prim(dst, prim::ALL);
        }
        if t == TOP {
            return;
        }
        let fields = self.all_fields(t);
        self.edge(fields, dst);
        let act = ChainAct::Load(dst);
        // Any-key lookups subscribe through the holder relation;
        // named-key replay in `visits` does not cover them.
        self.any_walk_holders.entry(t).or_default().push((t, act));
        self.accessor_hit(t, t, Key::Any, act);
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

    /// The fields of `t` a number key may name (DECISIONS 217): those under
    /// a numeric key or the element key, and what it holds under a
    /// computed key. ToPropertyKey of a number names no other.
    pub fn num_fields(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.num_fields.get(&t) {
            return n;
        }
        let n = self.node_named("numfields", || format!("numfields({t})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.num_fields.insert(t, n);
        let fa = self.field_any(t);
        self.edge(fa, n);
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            if self.is_num_atom(a) {
                let f = self.field(t, a);
                self.edge(f, n);
            }
        }
        n
    }

    fn is_num_atom(&self, a: Atom) -> bool {
        let s = &self.atoms[a as usize];
        numeric_key(s) || s == ELEM || s == NUM_KEY
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
                self.cons(
                    q,
                    Cons::Implicit {
                        reason,
                        proto: Protocol::Any,
                    },
                );
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
        self.mark_use(t, crate::model::MARK_TOUCH);
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
            Cons::DeleteKey { key, reason } => {
                format!(
                    "deletekey {n} {} {reason}",
                    esc_key(&self.atoms[*key as usize])
                )
            }
            Cons::AddsKeys => format!("addskeys {n}"),
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
            Cons::ChainUnion { key, dst, of } => {
                format!("chainunion {n} {} {dst} {of}", self.key_name(*key))
            }
            Cons::ProtoGiven { protos } => format!("protogiven {n} {protos}"),
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
            Cons::Arith { dst, add } => format!("arith {n} {dst} {}", *add as u8),
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
