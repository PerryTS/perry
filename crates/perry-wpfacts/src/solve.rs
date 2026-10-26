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
mod shared;
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
            let Some(n) = self.wl.pop_front() else {
                if self.sum.pending.is_empty() && !self.settle_bf_written() && !self.settle_for_in()
                {
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
        if self.is_marker(t) && !self.reach_marker.contains(&t) {
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
                        TokKind::Obj(tm, _) => {
                            matches!(self.tmpls[*tm as usize].kind, TmplKind::BObj { .. })
                        }
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
                let chain = self.chain_sinks.contains(&n);
                let saved = std::mem::replace(&mut self.chain_escape, chain);
                for &t in toks {
                    self.escape(t, reason);
                }
                self.chain_escape = saved;
            }
            Cons::HandOver { reason } => {
                for &t in toks {
                    let stand_in = matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                        if self.foreign.contains_key(&tm));
                    if !stand_in {
                        self.escape(t, reason);
                    }
                }
            }
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
            } => {
                for &t in toks {
                    let own_class = statics && self.toks[t as usize] == TokKind::ClassObj(tmpl);
                    if self.tmpl_of(t) != Some(tmpl) && !own_class {
                        self.escape(t, reason);
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
                    if let Some(s) = self.shared_of(t) {
                        let k = self.atoms[key as usize].clone();
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
                        Some(key) if accessor => self.cons(o, Cons::DefineAccessor { key, reason }),
                        _ => self.cons(
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

    /// Every builtin function value may have been written: every property
    /// read of one is ⊤ from now on.
    fn write_builtin_fns(&mut self, r: u32) {
        if self.bf_written.is_some() {
            return;
        }
        self.bf_written = Some(r);
        self.note_shared(SharedWrite::Store(Shared::BuiltinFn, None));
        self.bf_stores.clear();
        for dst in std::mem::take(&mut self.bf_reads) {
            self.add_top(dst, |_| {
                "property of a written builtin function value".into()
            });
        }
    }

    /// The stores into a builtin function value, once the worklist is
    /// empty: one at a node without ⊤ wrote one (true: solve on). A node
    /// that holds ⊤ keeps it, so its stores are stores into ⊤ for good.
    fn settle_bf_written(&mut self) -> bool {
        if self.bf_written.is_some() || self.bf_stores.is_empty() {
            return false;
        }
        let stores = std::mem::take(&mut self.bf_stores);
        match stores
            .iter()
            .find(|&&(n, _)| !self.nodes[n as usize].pts.contains_tok(TOP))
        {
            Some(&(_, r)) => {
                self.write_builtin_fns(r);
                true
            }
            None => false,
        }
    }

    /// A keyed rule on key node `n` (FVA 4.3b) applies once per operand:
    /// `Some(Some(k))` for a for-in run's one key, `Some(None)` for any
    /// other key, `None` when it already applied.
    fn keyed_once(&mut self, n: NodeId, x: NodeId, kind: u8) -> Option<Option<Atom>> {
        if !self.keyed_done.insert((n, x, kind)) {
            return None;
        }
        Some(self.key_nodes.get(&n).copied())
    }

    pub(crate) fn store_into(&mut self, t: Tok, key: Key, src: NodeId, site: SiteId) {
        // A store into a builtin function value is decided once the
        // worklist is empty (`settle_bf_written`): `write_builtin_fns` notes it.
        // The boundary's store for other units' code is theirs to publish.
        let theirs = self.store_at.is_some() && self.store_at == self.other_units_store;
        if let Some(s) = self
            .shared_of(t)
            .filter(|s| *s != Shared::BuiltinFn && !theirs)
        {
            let k = match key {
                Key::Named(a) => Some(self.atoms[a as usize].clone()),
                Key::Any => None,
            };
            self.note_shared(SharedWrite::Store(s, k));
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
            self.cons(
                src,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            return;
        }
        match key {
            Key::Named(a) => {
                let f = self.field(t, a);
                self.edge(src, f);
                if let Some(tm) = tm {
                    let ctor = self.stores[site as usize].birth || self.is_ctor_phase(site, tm);
                    // Writers per object: an instance's, the class object's,
                    // the prototype's.
                    let w = match self.toks[t as usize] {
                        TokKind::ClassObj(_) => self.class_writers.entry((tm, a)),
                        TokKind::Proto(_) => self.proto_writers.entry((tm, a)),
                        _ => self.writers.entry((tm, a)),
                    }
                    .or_default();
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

    /// Every function's parameter nodes hold their argument marker
    /// (DECISIONS 44), from before the first binding reaches them: the
    /// queued bindings run when the summaries exist, before the solve.
    pub(crate) fn mark_params(&mut self) {
        for f in 0..self.funcs.len() {
            for i in 0..self.funcs[f].params.len() {
                let p = self.funcs[f].params[i];
                if p != NO_NODE {
                    self.nodes[p as usize].param = true;
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
                    self.mark_use(r, crate::model::MARK_ESCAPE);
                }
                Some(ks) => {
                    for &a in ks {
                        let m = self.marker_load(t, Some(a));
                        self.mark_use(m, crate::model::MARK_ESCAPE);
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
            Cons::HandOver { reason } => format!("handover {n} {reason}"),
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
            } => format!("contractthis {n} {tmpl} {reason} {}", *statics as u8),
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
            Cons::Implicit { reason, proto } => {
                format!("implicit {n} {reason} {}", proto.name())
            }
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
