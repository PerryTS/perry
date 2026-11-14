//! The solver: inclusion constraints with difference propagation over a
//! worklist. Copy edges are plain successor lists; every other rule is a
//! complex constraint (`Cons`) re-applied to each node's new tokens.
//!
//! Soundness rests on two things: every rule over-approximates the concrete
//! transfer function of its construct, and every unknown value or operation
//! goes to ⊤ (`TOP` token for values coming from unmodeled code, `TopSink`
//! for values leaving into it). The rules here are the executable form of
//! design section 3.2; `scripts/wpfacts_recompute.py` re-implements them.

mod accessor_lookup;
mod copy;
mod define;
mod lookup;
pub(crate) mod map_vals;
mod own_read;

use crate::builtins;
mod call_and_construct;
mod native_value;
mod reflect_construct;
mod shared;
use crate::delta::Tokens;
use crate::model::*;

#[path = "solve/apply.rs"]
mod apply;
mod arr_len;
mod builtin_fields;

#[path = "solve/store.rs"]
mod store;

mod escape;
mod fields;
mod json_pos;
mod shaped_keys;
pub(crate) mod strs;
mod virt;

// Property-key ranges come from value deltas, not source annotations. Keep
// computed reads and calls on the same partition of ToPropertyKey's inputs.
/// An escaped value (unknown code holds it) that is no parameter or
/// argument marker: beside ⊤ it is that ⊤, since every rule does to ⊤ at
/// least what it does to the value. The one predicate of the arrival rule
/// (`add_tokens_s`, with the markers that read as ⊤, DECISIONS 42), of ⊤'s
/// arrival at a node that lists the value, and of a ⊤ node's pop.
pub(crate) fn escaped_beside_top<V>(
    esc_bits: &[u64],
    param_marker: &crate::pts::FxHashMap<Tok, V>,
    arg_bits: &[u64],
    t: Tok,
) -> bool {
    t != TOP
        && marker_top_bit(esc_bits, t)
        && !(param_marker.contains_key(&t) || marker_top_bit(arg_bits, t))
}

fn index_needs_general_key(toks: Tokens<'_>, prim: u16) -> bool {
    prim & (prim::STR | prim::SYMBOL) != 0 || !toks.is_empty()
}

fn index_has_numeric_key(prim: u16) -> bool {
    prim & (prim::NUM | prim::BIGINT) != 0
}

/// How many times the work bound a widened solve may use before it stops.
const WIDENED_WORK: u64 = 1000;

impl Model {
    // ---------------------------------------------------------------- nodes

    /// Add tokens/prims to a node; schedules it when anything is new.
    #[track_caller]
    pub fn add(&mut self, n: NodeId, toks: &[Tok], prim: u16) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Seed(
            n,
            toks.to_vec(),
            prim,
        )) {
            return;
        }
        self.add_tokens(n, Tokens::Slice(toks), prim);
    }

    /// `add` with the string constants `strs` for a string in `prim` (a
    /// copy of another node's value: `Node::strs`).
    pub(crate) fn add_s(&mut self, n: NodeId, toks: &[Tok], prim: u16, strs: StrSet) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Seed(
            n,
            toks.to_vec(),
            prim,
        )) {
            return;
        }
        self.add_tokens_s(n, Tokens::Slice(toks), prim, strs);
    }
    /// The work bound (`FactsConfig::max_work`, 0 = none). Past it a
    /// whole-program solve (`Model::widen`) is widened (`bind_layout`: no
    /// new calling contexts) and goes on to its fixpoint; past `WIDENED_WORK` times it, or past it
    /// at all per unit, the solve stops and claims nothing.
    pub(crate) fn past_work_bound(&mut self) -> bool {
        if self.max_work == 0 || self.work.get() <= self.max_work {
            return false;
        }
        self.widened = self.widen;
        if !self.widen {
            self.aborted = true;
        } else if self.work.get() > self.max_work.saturating_mul(WIDENED_WORK) {
            self.aborted = true;
            self.widened_stop = true;
        }
        self.aborted
    }

    /// A string that is no copy of another node's is any string
    /// (`Node::strs`).
    #[track_caller]
    fn add_tokens(&mut self, n: NodeId, toks: Tokens<'_>, prim: u16) {
        self.add_tokens_s(n, toks, prim, STRS_TOP);
    }

    #[track_caller]
    fn add_tokens_s(&mut self, n: NodeId, toks: Tokens<'_>, prim: u16, strs: StrSet) {
        if self.ft_check.is_some() {
            self.filter_check(n, toks);
        }
        if self.relations.is_some() {
            self.rel_generic_fn_arrival(n, &toks);
        }
        if let Some(c) = self.diagnostic.as_mut().and_then(|d| d.census.as_mut()) {
            if !c.injecting
                && self.cur_src != NO_NODE
                && self.cur_src != n
                && toks.iter().any(|&t| t == TOP)
            {
                c.top_edges.insert((self.cur_src, n));
            }
        }
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
        let boundary = self.nodes[n as usize].succ.len() as u32;
        // Delta tokens are distinct; a slice may repeat one.
        let distinct =
            crossed.is_none() && matches!(toks, Tokens::Pending(_) | Tokens::Delivery(..));
        let mut fresh = std::mem::take(&mut self.fresh);
        let mut seen = std::mem::take(&mut self.fresh_seen);
        fresh.clear();
        seen.clear();
        // Is `t` already among the new members?
        let is_fresh = |fresh: &[Tok], seen: &mut crate::pts::FxHashSet<Tok>, t: Tok| {
            if distinct {
                return false;
            }
            if fresh.len() <= 16 {
                return fresh.contains(&t);
            }
            if seen.len() < fresh.len() {
                seen.extend(fresh[seen.len()..].iter().copied());
            }
            seen.contains(&t)
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
            if marker_top_bit(&self.marker_top, t)
                || escaped_beside_top(&self.esc_bits, &self.param_marker, &self.arg_bits, t)
            {
                later.push(t);
                continue;
            }
            if !node.pts.contains_tok(t) && !is_fresh(&fresh, &mut seen, t) {
                fresh.push(t);
                self.pending.push_to(&mut node.delta, t, boundary);
                changed = true;
                self.pts_entries += 1;
                top_new |= t == TOP;
            }
        }
        // Beside ⊤ it is that ⊤: every rule does to ⊤ at least what it does
        // to the marker, and the readers after the solve read it as ⊤. Left
        // out, it never floods the unit. Without ⊤ it is delivered as before.
        if !later.is_empty() {
            if top_new || node.pts.contains_tok(TOP) {
                node.dropped = true;
            } else {
                for t in later {
                    if !node.pts.contains_tok(t) && !is_fresh(&fresh, &mut seen, t) {
                        fresh.push(t);
                        self.pending.push_to(&mut node.delta, t, boundary);
                        changed = true;
                        self.pts_entries += 1;
                    }
                }
            }
        }
        // One step from the old set to the new one: a shared set is never
        // copied in place (`SetTable::extend`).
        if !fresh.is_empty() {
            let old = std::mem::take(&mut node.pts);
            node.pts = self.sets.extend(old, &fresh);
        }
        // ⊤ arrived: a member listed before it and escaped since is that ⊤
        // now, and leaves (the set is rebuilt). One still in the node's
        // delta leaves at the node's pop, which follows (top_new queued it).
        if top_new {
            let (esc, pm, ab) = (&self.esc_bits, &self.param_marker, &self.arg_bits);
            let old = std::mem::take(&mut node.pts);
            let before = old.len();
            node.pts = self
                .sets
                .retain(old, |t| !escaped_beside_top(esc, pm, ab, t));
            if node.pts.len() != before {
                node.dropped = true;
                self.charge_work(before as u64);
            }
        }
        if self.prov.is_some() && !fresh.is_empty() {
            let at = std::panic::Location::caller();
            self.prov_arrivals(n, &mut fresh.iter().copied(), false, at);
        }
        self.fresh = fresh;
        self.fresh_seen = seen;
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
        let iv = self.cur_range.unwrap_or(crate::model::Rg::TOP);
        let node = &mut self.nodes[n as usize];
        let newp = prim & !node.prim;
        if newp != 0 {
            node.prim |= newp;
            node.dprim |= newp;
            changed = true;
        }
        // A Number brings its interval: the delivering edge's, else ⊤. A
        // rule's result takes its interval from the rule only.
        if prim & prim::NUM != 0 && !node.ruled {
            changed |= grow_range(node, iv.num);
            if iv.frac && !node.frac {
                node.frac = true;
                node.drange = true;
                changed = true;
            }
        }
        // A String brings its code interval the same way.
        if prim & prim::STR != 0 && !node.cruled {
            changed |= grow_crange(node, iv.chr);
        }
        if prim & prim::STR != 0 {
            let strs = if strs == STRS_NONE { STRS_TOP } else { strs };
            changed |= self.join_strs(n, strs);
        }
        let node = &mut self.nodes[n as usize];
        if changed && !node.in_wl {
            node.in_wl = true;
            self.wl.push_back(n);
        }
    }

    /// Whether operation `c` on a ⊤ operand may run unknown code (DESIGN
    /// SEEDE section 8): every kind but tests, escapes and call plumbing.
    fn may_run_code(c: &Cons) -> bool {
        !matches!(
            c,
            Cons::Raise { .. }
                | Cons::Identity { .. }
                | Cons::Guard { .. }
                | Cons::Defined { .. }
                | Cons::NotNullish { .. }
                | Cons::FieldTruthy { .. }
                | Cons::Fulfil { .. }
                | Cons::Falsy { .. }
                | Cons::NotBuffer { .. }
                | Cons::ProtoObjects { .. }
                | Cons::NotUndefined { .. }
                | Cons::TopSink { .. }
                | Cons::RuntimeStore { .. }
                | Cons::BirthField { .. }
                | Cons::ThisTo { .. }
                | Cons::CallArgs { .. }
                | Cons::RetInto { .. }
                | Cons::ArgInto { .. }
                | Cons::ThisInst { .. }
                | Cons::NativeThis { .. }
                | Cons::HolderStep { .. }
                | Cons::VisitHolders { .. }
                | Cons::VisitStop { .. }
        )
    }

    /// The throw region of function `f`'s body (DESIGN SEEDE section 8),
    /// made on first use. Per unit any function may be called by another
    /// unit's code, whose `catch` reads the shared cell: its region feeds
    /// `thrown` (the cell the unit boundary publishes).
    pub fn raise_of(&mut self, f: FnIdx) -> NodeId {
        let r = self.funcs[f as usize].raise;
        if r != u32::MAX {
            return r;
        }
        let r = self.node_l("RAISE");
        self.funcs[f as usize].raise = r;
        if self.unit_mode {
            let t = self.thrown;
            self.edge(r, t);
        }
        r
    }

    /// Unknown code may run in `region` (a ⊤ or opaque callee, an operation
    /// on ⊤, a native handing a value to unknown code): it may throw anything
    /// it holds there.
    pub fn raise_top(&mut self, region: NodeId, from: NodeId, why: &'static str) {
        if region == u32::MAX || self.nodes[region as usize].pts.contains_tok(TOP) {
            return;
        }
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = from;
        self.cur_why = self.reason(why);
        self.add_tokens(region, Tokens::Slice(&[TOP]), prim::ALL);
        (self.cur_src, self.cur_why) = saved;
    }

    /// Charge the work since the last mark to what it was spent for, and
    /// from now on to `n` (a popped node) or, with `NO_NODE`, to `phase`
    /// (PERRY_FACTS_PROFILE only).
    pub fn prof_mark(&mut self, n: NodeId, phase: &'static str) {
        let Some(p) = self.prof.as_mut() else { return };
        let (at, w0, ph) = p.at;
        let w = self.work.get() - w0;
        if w != 0 {
            if at == NO_NODE {
                *p.phase.entry(ph).or_default() += w;
            } else {
                *p.node.entry(at).or_default() += w;
            }
        }
        p.at = (n, self.work.get(), phase);
        if self.work.get() >= p.next_curve {
            p.next_curve = (self.work.get() / WorkProfile::STEP + 1) * WorkProfile::STEP;
            let cons: usize = self.nodes.iter().map(|n| n.cons.len()).sum();
            let succ: usize = self.nodes.iter().map(|n| n.succ.len()).sum();
            let stored_entries = self
                .native_writes
                .values()
                .filter(|w| !w.stored.is_empty())
                .count();
            let stored_values: usize = self.native_writes.values().map(|w| w.stored.len()).sum();
            eprintln!(
                "perry: wpfacts curve work={} pops={} nodes={} pts={} cons={} toks={} insts={} wl={} succ={succ} answered={} answer_keys={} stored_entries={stored_entries} stored_values={stored_values} replays={} fed={}",
                self.work.get(),
                self.pops,
                self.nodes.len(),
                self.pts_entries,
                cons,
                self.toks.len(),
                self.sum.insts.len(),
                self.wl.len(),
                self.native_answered.len(),
                self.native_answer_nodes.len(),
                self.native_replays,
                self.native_fed
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
            self.work.get(), p.step[0], p.step[1], p.step[2], p.step[3]
        );
        let [sites, shaped, no_ctor] = self.key_shape_census;
        eprintln!(
            "perry: wpfacts key shapes sites={sites} shaped={shaped} no_constructor={no_ctor} distinct={}",
            self.key_shapes.len().saturating_sub(1)
        );
        let [own, gated, obj_key] = self.own_read_census;
        eprintln!("perry: wpfacts own reads generated={own} gated={gated} object_key={obj_key}");
        let (logical, stored, distinct) = self.pts_storage();
        eprintln!(
            "perry: wpfacts sets logical={logical} stored={stored} distinct={distinct} copies={}",
            self.sets.copies
        );
        let mut natives: Vec<_> = p.natives.iter().collect();
        natives.sort_by(|a, b| a.0.cmp(b.0));
        for (name, (runs, sites)) in natives {
            eprintln!(
                "perry: wpfacts native-effects {name} runs={runs} sites={}",
                sites.len()
            );
        }
        if self.optimistic_json_key {
            let stores = self.json_key_fired.iter().filter(|f| f.0).count();
            eprintln!(
                "perry: wpfacts # optimistic json-key sites={} reads={} stores={stores}",
                self.json_key_fired.len(),
                self.json_key_fired.len() - stores
            );
            let mut at: std::collections::BTreeMap<String, usize> = Default::default();
            for &(st, n) in &self.json_key_fired {
                let name = self
                    .node_names
                    .get(n as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("{}#{n}", self.labels[n as usize]));
                *at.entry(format!("{} {name}", if st { "S" } else { "L" })).or_default() += 1;
            }
            let mut at: Vec<_> = at.into_iter().collect();
            at.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (k, c) in at.iter().take(40) {
                eprintln!("perry: wpfacts # optimistic json-key at {c} {k}");
            }
        }
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
        let delta_cap = self.pending.retained_bytes() / std::mem::size_of::<u32>();
        let big: usize = self.nodes.iter().filter(|n| n.pts.is_hashed()).count();
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

    /// Provenance of the generic builtin function value (`builtin-fn:Any`,
    /// the one token `builtin_fn` interns) under `PERRY_FACTS_RELATIONS`:
    /// each new arrival writes `A addbf <node> <tok> src=<cur_src>
    /// @<file>:<line>` (a propagation names its source node; diagnostic only).
    #[track_caller]
    fn rel_generic_fn_arrival(&mut self, n: NodeId, toks: &Tokens<'_>) {
        let Some(&bf) = self.tok_index.get(&TokKind::BuiltinFn(Native::Any)) else {
            return;
        };
        if !toks.iter().any(|&t| t == bf) || self.nodes[n as usize].pts.contains_tok(bf) {
            return;
        }
        let at = std::panic::Location::caller();
        let f = at.file();
        let f = f.rsplit_once("src/").map_or(f, |(_, x)| x);
        let line = format!("A addbf {n} {bf} src={} @{f}:{}", self.cur_src, at.line());
        if let Some(r) = self.relations.as_mut() {
            r.push(line);
        }
    }

    /// `PERRY_WPF_FILTER_CHECK`: a token new at the output of a
    /// `Cons::FieldTruthy` (not ⊤, not a marker) must be in one of its
    /// hosts' sets now; the first 5 that are not are printed.
    fn filter_check(&mut self, n: NodeId, toks: Tokens<'_>) {
        let Some(hosts) = self.ft_check.as_ref().and_then(|m| m.get(&n)) else {
            return;
        };
        let mut bad = Vec::new();
        for &t in toks.iter() {
            if t == TOP || self.is_marker(t) || self.nodes[n as usize].pts.contains_tok(t) {
                continue;
            }
            if !hosts
                .iter()
                .any(|&h| self.nodes[h as usize].pts.contains_tok(t))
            {
                bad.push((t, hosts.clone()));
            }
        }
        for (t, hosts) in bad {
            self.ft_violations += 1;
            if self.ft_violations > 5 {
                continue;
            }
            let body = |m: &Self, x: NodeId| {
                let f = m.node_fn.get(x as usize).copied().unwrap_or(u32::MAX);
                m.funcs
                    .get(f as usize)
                    .map_or("-".to_string(), |fi| fi.name.clone())
            };
            let mut rules = Vec::new();
            for &h in &hosts {
                for c in &self.nodes[h as usize].cons {
                    if let Cons::FieldTruthy { key, test, dst } = *c {
                        if dst == n {
                            rules.push(format!(
                                "fieldtruthy host {h} key {} test {test} (body {})",
                                self.key_name(key),
                                body(self, h)
                            ));
                        }
                    }
                }
            }
            eprintln!(
                "FILTER-CHECK violation #{}: dst {n} (body {}) tok {t} kind {} src {} rules [{}]",
                self.ft_violations,
                body(self, n),
                self.tok_kind_word(t),
                self.cur_src,
                rules.join("; ")
            );
        }
    }

    /// Seed a token (an input fact outside the solver rules: logged).
    #[track_caller]
    pub fn add_tok(&mut self, n: NodeId, t: Tok) {
        self.add_tok_arm(n, t, "")
    }

    /// `add_tok` naming the code arm: under `PERRY_FACTS_RELATIONS`, a
    /// solver-phase add (which writes no `addr` relation) writes the
    /// provenance line `A addtok <node> <tok> <arm>@<file>:<line>` (the
    /// caller's location; diagnostic only).
    #[track_caller]
    pub fn add_tok_arm(&mut self, n: NodeId, t: Tok, arm: &'static str) {
        if t == TOP
            && self.diagnostic.as_ref().is_some_and(|d| d.census.is_some())
            && !self
                .nodes
                .get(self.cur_src as usize)
                .is_some_and(|v| v.pts.contains_tok(TOP))
        {
            if let Some(d) = &mut self.diagnostic {
                if let Some(c) = &mut d.census {
                    if !c.injecting {
                        let at = std::panic::Location::caller();
                        let gate = c.node_gate(n);
                        c.inject_at = Some((n, gate));
                        c.injections.insert((
                            n,
                            at.file().into(),
                            at.line(),
                            "direct TOP token".into(),
                            d.cause_node.unwrap_or(NO_NODE),
                            d.cur_call.unwrap_or(u32::MAX),
                            d.escaping.last().copied().unwrap_or(u32::MAX),
                            self.cur_fn,
                            gate,
                        ));
                        if c.suppress.as_deref()
                            == Some(format!("{}:{}:{n}", at.file(), at.line()).as_str())
                        {
                            c.suppressed += 1;
                            return;
                        }
                    }
                }
            }
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("addr {n} {t}"));
        } else if let Some(r) = self.relations.as_mut() {
            let at = std::panic::Location::caller();
            let f = at.file();
            let f = f.rsplit_once("src/").map_or(f, |(_, x)| x);
            r.push(format!("A addtok {n} {t} {arm}@{f}:{}", at.line()));
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
    #[track_caller]
    pub fn add_top(&mut self, n: NodeId, why: impl FnOnce(&Self) -> String) {
        if self.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
            let w = why(self);
            let at = std::panic::Location::caller();
            let d = self.diagnostic.as_mut().unwrap();
            let c = d.census.as_mut().unwrap();
            let gate = c.node_gate(n);
            c.inject_at = Some((n, gate));
            c.injections.insert((
                n,
                at.file().into(),
                at.line(),
                w.clone(),
                d.cause_node.unwrap_or(NO_NODE),
                d.cur_call.unwrap_or(u32::MAX),
                d.escaping.last().copied().unwrap_or(u32::MAX),
                self.cur_fn,
                gate,
            ));
            let key = format!("{}:{}:{n}", at.file(), at.line());
            if c.suppress.as_deref() == Some(key.as_str()) {
                c.suppressed += 1;
                return;
            }
            let saved = std::mem::replace(&mut c.injecting, true);
            self.add_top_inner(n, |_| w);
            self.diagnostic
                .as_mut()
                .unwrap()
                .census
                .as_mut()
                .unwrap()
                .injecting = saved;
            return;
        }
        self.add_top_inner(n, why);
    }
    fn add_top_inner(&mut self, n: NodeId, why: impl FnOnce(&Self) -> String) {
        if self.explain || self.diagnostic.is_some() {
            let w = why(self);
            if !self.nodes[n as usize].pts.contains_tok(TOP) {
                if let Some(d) = &mut self.diagnostic {
                    d.seeds.entry(n).or_insert(w.clone());
                    if let Some(&b) = d
                        .seed_holder
                        .last()
                        .or(d.seed_by.last())
                        .or(d.escaping.last())
                    {
                        d.top_by.entry(n).or_insert(b);
                    }
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
    #[track_caller]
    pub fn top_val(&mut self, why: &str) -> NodeId {
        if crate::unksrc::on() {
            crate::unksrc::note("topval", why, u32::MAX, self.cur_fn as u32);
        }
        // Explain and the dump name each origin: its own node.
        if !self.explain && self.diagnostic.is_none() {
            return self.top_tok_node;
        }
        if let Some(&n) = self.top_vals.get(why) {
            return n;
        }
        let n = self.node_l("TOPVAL");
        #[cfg(test)]
        {
            self.nodes[n as usize].intrinsic_top = Some(why.to_owned());
        }
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
    /// Inside a clause of a switch on a parameter the rules it adds to `n`
    /// record the clause's reach mask (rule M1, `Model::cons_reach`).
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        let tag = match self.clause {
            Some((f, m)) if self.cur_fn == f => Some(m),
            _ => None,
        };
        let len0 = self.nodes[n as usize].cons.len();
        self.cons_untagged(n, c);
        if let Some(m) = tag {
            for i in len0..self.nodes[n as usize].cons.len() {
                self.cons_reach.insert((n, i as u32), m);
            }
        }
    }

    fn cons_untagged(&mut self, n: NodeId, c: Cons) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Rule(n, c)) {
            return;
        }
        self.stats_cons += 1;
        if let Cons::FieldTruthy { dst, .. } = c {
            if self.ft_check.is_none() && std::env::var_os("PERRY_WPF_FILTER_CHECK").is_some() {
                self.ft_check = Some(Default::default());
            }
            if let Some(m) = self.ft_check.as_mut() {
                m.entry(dst).or_default().push(n);
            }
        }
        if matches!(c, Cons::IdxOwnLoad { .. }) {
            self.own_read_census[0] += 1;
        }
        if self.diagnostic.is_some() {
            if let Cons::TopSink { reason } | Cons::RuntimeStore { reason, .. } = c {
                self.diagnostic_sink(n, reason);
            }
        }
        if self.relations.is_some()
            && !matches!(
                c,
                // No flow: the recomputation has nothing to check.
                Cons::Identity { .. }
                    | Cons::ThisTo { .. }
                    | Cons::CallArgs { .. }
                    | Cons::RetInto { .. }
                    | Cons::ArgInto { .. }
                    | Cons::ThisInst { .. }
                    | Cons::NativeThis { .. }
                    | Cons::BuiltinCallee
                    | Cons::HolderStep { .. }
                    | Cons::VisitHolders { .. }
                    | Cons::VisitStop { .. }
            )
        {
            let line = self.cons_line(n, &c);
            self.rel(|| line);
        }
        if let Cons::Range { dst, op, .. } = c {
            // The rule's result is every Number (String, for a code rule)
            // the node can hold: what reached it before (an instance's
            // seeds) is subsumed.
            let d = &mut self.nodes[dst as usize];
            if matches!(op, RangeOp::SClip(_)) {
                if !d.cruled {
                    d.cruled = true;
                    d.crange = Iv::EMPTY;
                    d.csteps = 0;
                }
            } else if !d.ruled {
                d.ruled = true;
                d.range = Iv::EMPTY;
                d.rsteps = 0;
            }
        }
        self.nodes[n as usize].cons.push(c);
        if let Some(region) = self.gen_region.filter(|_| Self::may_run_code(&c)) {
            let cs = &self.nodes[n as usize].cons;
            let seen = cs
                .iter()
                .rev()
                .take(4)
                .any(|x| matches!(x, Cons::Raise { region: r } if *r == region));
            if !seen {
                self.cons(n, Cons::Raise { region });
            }
        }
        if !self.cell_watch.is_empty() && self.cell_watch.remove(&n) {
            self.cell_woken.push(n);
        }
        if self.aborted {
            return;
        }
        self.own_refuted_cons(n, c);
        let prim = self.nodes[n as usize].prim;
        if self.nodes[n as usize].pts.is_empty() && prim == 0 {
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[n as usize].pts.sorted_into(&mut toks);
        // The replay is solve work: the bound covers it.
        self.charge_work(toks.len() as u64 + 1);
        if let Some(p) = self.prof.as_mut() {
            let e = p.kind.entry(c.kind_name()).or_default();
            e.2 += 1;
            e.3 += toks.len() as u64 + 1;
        }
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = n;
        self.cur_why = u32::MAX;
        let pv = (self.prov_mode, self.prov_rule);
        if self.prov.is_some() {
            (self.prov_mode, self.prov_rule) = (crate::provenance::PM_RULE, Some((n, c)));
        }
        self.with_s(|m| m.apply(n, &c, &toks, prim));
        (self.prov_mode, self.prov_rule) = pv;
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
        self.solve_fixpoint();
        // The interned sets stay with their nodes; their index is the solve's.
        self.sets.clear();
    }

    fn solve_fixpoint(&mut self) {
        self.rel_phase = b'S';
        self.mark_params();
        self.members_settled.get_mut().clear();
        let mut delta = crate::delta::Delta::default();
        loop {
            self.prof_mark(NO_NODE, "instances");
            self.fill_instances();
            self.prof_mark(NO_NODE, "marker links");
            self.replay_marker_links();
            self.prof_mark(NO_NODE, "settle");
            let Some(n) = self.wl.pop_front() else {
                if self.sum.pending.is_empty()
                    && self.marker_links.is_empty()
                    && !self.settle_for_in()
                    && !self.settle_val_runs()
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
            let (dprim, dstrs, drange) = {
                let node = &mut self.nodes[n as usize];
                node.in_wl = false;
                self.pending.take(&mut node.delta, &mut delta);
                (
                    std::mem::take(&mut node.dprim),
                    std::mem::take(&mut node.dstrs),
                    std::mem::take(&mut node.drange) && node.prim & (prim::NUM | prim::STR) != 0,
                )
            };
            // A node that holds ⊤ delivers no member that escaped after it
            // was listed: beside ⊤ it is that ⊤, which the node delivered.
            // It leaves the delta and the set (both rebuilt, not changed in
            // shared storage).
            if !delta.is_empty() && self.nodes[n as usize].pts.contains_tok(TOP) {
                let (esc, pm, ab) = (&self.esc_bits, &self.param_marker, &self.arg_bits);
                let cover = |t: Tok| escaped_beside_top(esc, pm, ab, t);
                if delta.iter().any(|&t| cover(t)) {
                    delta.retain(|t| !cover(t));
                    let node = &mut self.nodes[n as usize];
                    let old = std::mem::take(&mut node.pts);
                    let before = old.len() as u64;
                    node.pts = self.sets.retain(old, |t| !cover(t));
                    node.dropped = true;
                    self.charge_work(before);
                }
            }
            if delta.is_empty() && dprim == 0 && !dstrs && !drange {
                continue;
            }
            // Only a node's string constants (`Node::strs`) and/or its
            // intervals (`model::range`: a Number's, a String's code) grew:
            // re-deliver them along the edges (STR with the constants, the
            // interval's kinds with it) and re-run only the rules that read
            // them (`Cons::carries_strs`, `Cons::carries_range`). Each lattice
            // keeps its own predicate.
            let meta_only = delta.is_empty() && dprim == 0;
            let rprim = if drange {
                self.nodes[n as usize].prim & (prim::NUM | prim::STR)
            } else {
                0
            };
            let sprim = dprim | rprim | if dstrs { prim::STR } else { 0 };
            let sstrs = self.nodes[n as usize].strs;
            // Sets change interned (`SetTable::extend`); one made outside
            // this solve (an earlier solve, whose index is gone) is interned
            // here, so covering below stays a pointer test.
            let own = std::mem::take(&mut self.nodes[n as usize].pts);
            self.nodes[n as usize].pts = self.sets.intern(own);
            delta.prepare();
            let delivery_overhead = delta.delivery_overhead();
            // Index loops (no clone of the lists): successors and
            // constraints appended while this runs already saw the full set.
            self.cur_src = n;
            self.cur_why = u32::MAX;
            let ncons0 = self.nodes[n as usize].cons.len();
            self.charge_work((delta.len() as u64 + 1) * ncons0 as u64);
            if let Some(p) = self.prof.as_mut() {
                p.step[0] += (delta.len() as u64 + 1) * ncons0 as u64;
            }
            if self.profile.is_some() {
                let e = self.label_work.entry(self.labels[n as usize]).or_default();
                e.1 += delta.len() as u64 * ncons0 as u64;
                e.2 += 1;
            }
            if self.past_work_bound() {
                return;
            }
            // A value-copy edge delivers the source's Numbers with their
            // interval (`add_tokens`).
            self.cur_range = Some(self.rg(n));
            if self.prov.is_some() {
                (self.prov_mode, self.prov_rule) = (crate::provenance::PM_COPY, None);
            }
            let mut i = 0;
            while i < self.nodes[n as usize].succ.len() {
                let s = self.nodes[n as usize].succ[i];
                if self.try_copy_delivery(n, s, i as u32, &delta, sprim, sstrs) {
                    self.charge_work(1 + delivery_overhead);
                    if let Some(p) = self.prof.as_mut() {
                        p.step[1] += 1 + delivery_overhead;
                    }
                    if self.past_work_bound() {
                        return;
                    }
                    i += 1;
                    continue;
                }
                let (covered, checks) = if delta.is_empty() {
                    (true, 1)
                } else {
                    self.copy_coverage(n, s, Tokens::Delivery(&delta, i as u32))
                };
                let toks = if covered {
                    Tokens::Slice(&[])
                } else {
                    Tokens::Delivery(&delta, i as u32)
                };
                let visits = toks.len() as u64;
                self.charge_work(visits + checks + delivery_overhead);
                if let Some(p) = self.prof.as_mut() {
                    p.step[2] += visits + checks + delivery_overhead;
                }
                if self.profile.is_some() {
                    self.label_work
                        .entry(self.labels[n as usize])
                        .or_default()
                        .0 += visits;
                }
                if self.past_work_bound() {
                    return;
                }
                if covered {
                    self.add_tokens_s(s, toks, sprim, sstrs);
                } else {
                    let extra = self.copy_to(n, s, toks, sprim, sstrs);
                    self.charge_work(extra);
                    if let Some(p) = self.prof.as_mut() {
                        p.step[3] += extra;
                    }
                    if self.past_work_bound() {
                        return;
                    }
                }
                i += 1;
            }
            self.cur_range = None;
            let ncons = self.nodes[n as usize].cons.len();
            for i in 0..ncons {
                let c = self.nodes[n as usize].cons[i];
                let mut p = dprim;
                if !meta_only || c.carries_range() {
                    p |= rprim;
                }
                if dstrs && c.carries_strs() {
                    p |= prim::STR;
                }
                if meta_only && p == 0 {
                    continue;
                }
                self.cur_src = n;
                self.cur_why = u32::MAX;
                if self.prov.is_some() {
                    (self.prov_mode, self.prov_rule) = (crate::provenance::PM_RULE, Some((n, c)));
                }
                self.apply_tokens(n, &c, Tokens::Pending(&delta), p);
            }
            delta.clear();
        }
        self.cur_src = NO_NODE;
        self.cur_why = u32::MAX;
        (self.prov_mode, self.prov_rule) = (crate::provenance::PM_ADD, None);
        #[cfg(debug_assertions)]
        self.assert_shadowed_complete();
    }

    // --------------------------------------------------------------- fields

    /// The node holding the prototype objects of `t`.
    pub fn proto_node(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.proto_node(t));
        }
        if let Some(&n) = self.proto_node.get(&t) {
            return n;
        }
        let n = self.node_named("protos", || format!("protos({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::Prototype,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.proto_node.insert(t, n);
        self.rel(|| format!("protonode {n} {t}"));
        // Seed the static part of the chain.
        let objproto = self.tok(TokKind::ObjectProto);
        match self.toks[t as usize].clone() {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind.clone() {
                // A specialization's instances have its origin's prototype
                // object (`js_register_class_generic_origin`: one object;
                // `class_id_holders` holds its class-id members).
                TmplKind::Class { .. } => {
                    let p = self.tok(TokKind::Proto(self.constructor_origin(tm)));
                    self.add_tok(n, p);
                }
                TmplKind::FnInst { func } => {
                    // `new f()` takes its prototype from `f.prototype`,
                    // which holds `f`'s own prototype object from `f`'s
                    // creation, read or not (as a `f.prototype` read mints
                    // it, `builtins::loads`), unless a store replaced it.
                    let ftok = self.funcs[func as usize].token;
                    let a = self.atom("prototype");
                    let fp = self.field(ftok, a);
                    let p = self.tok(TokKind::Proto(tm));
                    self.add_tok(fp, p);
                    self.edge(fp, n);
                }
                // Object.create / literal prototypes are added by gen. A
                // receiver token or marker has its classes' (DECISIONS 59).
                _ => match self.virt_protos.get(&t).cloned() {
                    Some(ps) => {
                        for p in ps {
                            let pt = self.tok(TokKind::Proto(self.constructor_origin(p)));
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
                    let pp = self.tok(TokKind::Proto(self.constructor_origin(p)));
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
            TokKind::NativeProto(..) => self.add_tok(n, objproto),
            // A builtin namespace's prototype is `Function.prototype` (a
            // constructor's) or `Object.prototype` (`Math`, `JSON`, the
            // global object's chain): `ObjectProto` stands for both.
            // A generator object (its step closure) inherits from what its
            // generator function's `prototype` holds (`FuncInfo::gen_proto`:
            // the original object or a replacement), whose chain the
            // runtime's generator prototypes continue.
            TokKind::Func(f, _) if self.funcs[f as usize].gen_step() => {
                if let Some(p) = self.funcs[f as usize].gen_proto {
                    self.edge(p, n);
                }
                self.add_tok(n, objproto);
            }
            // A builtin constructor whose [[Prototype]] is another one
            // (`TypeError`'s is `Error`): that namespace.
            TokKind::Builtin(ns) if crate::builtins::ns_parent(ns).is_some() => {
                let p = self.tok(TokKind::Builtin(crate::builtins::ns_parent(ns).unwrap()));
                self.add_tok(n, p);
            }
            // A realm constructor whose declared [[Prototype]] is another
            // realm constructor (`Readable` -> `Stream` -> `EventEmitter`,
            // node's legacy stream chain): that constructor's value, so a
            // lookup on it reaches what the program gave or stored up the
            // chain (`Object.setPrototypeOf(EventEmitter, p)` then
            // `Readable.apply`).
            TokKind::BuiltinFn(Native::Member {
                identity,
                holder: None,
            }) if crate::builtins::realm_parent_constructor(identity).is_some() => {
                let parent = crate::builtins::realm_parent_constructor(identity).unwrap();
                let p = self.tok(TokKind::BuiltinFn(Native::Member {
                    identity: parent,
                    holder: None,
                }));
                self.add_tok(n, p);
            }
            TokKind::Func(_, _)
            | TokKind::Bound(_)
            | TokKind::BuiltinFn(_)
            | TokKind::Builtin(_) => self.add_tok(n, objproto),
            TokKind::ClassObj(tm) => match self.tmpls[tm as usize].parent {
                Some(p) => {
                    let pc = self.tok(TokKind::ClassObj(p));
                    self.add_tok(n, pc);
                }
                None => match self.tmpls[tm as usize].parent_value {
                    // `S.__proto__` is the value of the `extends` expression.
                    Some((pv, _)) => self.edge(pv, n),
                    None => match self.tmpls[tm as usize].native_parent.clone() {
                        // `class C extends EventEmitter` over a named import
                        // of a node module: the heritage reads the import's
                        // binding, which holds what key `EventEmitter` of the
                        // module object reads (the named ESM export value,
                        // `js_native_module_named_esm_export_value`).
                        Some((module, name)) if !self.unit_mode => {
                            match crate::builtins::node_module_ns(&module) {
                                Some(ns) => {
                                    let mt = self.tok(TokKind::Builtin(ns));
                                    let mn = self.node_l("native-parent-module");
                                    self.add_tok(mn, mt);
                                    let key = Key::Named(self.atom(&name));
                                    self.cons(mn, Cons::Load { key, dst: n });
                                }
                                None => self.add_tok(n, objproto),
                            }
                        }
                        _ => self.add_tok(n, objproto),
                    },
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
            let w0 = self.work.get();
            self.apply_inner(n, c, toks, prim);
            let name = c.kind_name();
            if let Some(p) = self.prof.as_mut() {
                let e = p.kind.entry(name).or_default();
                e.0 += 1;
                e.1 += self.work.get() - w0;
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
                    self.parent_unsettled.insert(tm);
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
                self.parent_unsettled.insert(tm);
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
            self.charge_work(1);
            if self.past_work_bound() {
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
        if let Some(d) = &mut self.diagnostic {
            if let Some(c) = d.cause_node.filter(|&c| c != n) {
                d.store_top.entry(n).or_insert(c);
            }
        }
        if let Some(c) = self.cur_native_call {
            let region = self.calls[c as usize].region;
            self.raise_top(
                region,
                n,
                "a native hands a value to unknown code, which may throw",
            );
        }
        if self.sunk_nodes.insert(n) {
            self.cons(n, Cons::TopSink { reason });
        }
    }

    // ---------------------------------------------------------------- calls

    /// InstanceofOperator on `t` with left-hand side `lhs`: the protocol
    /// touch, its method called with `lhs`. Where the method is not
    /// modeled (⊤, a marker's run, a bound function's target), `lhs` is
    /// passed to unknown code: it escapes.
    pub fn has_instance(&mut self, t: Tok, reason: u32, lhs: NodeId) {
        let unknown =
            t == TOP || self.is_marker(t) || matches!(self.toks[t as usize], TokKind::Bound(_));
        if unknown {
            self.sink_node(lhs, reason);
            match self.toks[t as usize] {
                // `instanceof` a bound function runs its target's.
                TokKind::Bound(_) => self.escape(t, reason),
                _ => self.implicit(t, reason, Protocol::HasInstance),
            }
            return;
        }
        let arg = self.has_instance_node(t);
        self.edge(lhs, arg);
        self.implicit(t, reason, Protocol::HasInstance);
    }

    /// The argument node of the `@@hasInstance` call on `t`, one per token.
    fn has_instance_node(&mut self, t: Tok) -> NodeId {
        if let Some(&a) = self.has_instance_arg.get(&t) {
            return a;
        }
        let a = self.node_l("hasinstance-arg");
        self.nodes[a as usize].frame = self.fam_home(t);
        self.has_instance_arg.insert(t, a);
        a
    }

    /// Builtin protocol touch on `t` (ToPrimitive, iteration, toJSON, then,
    /// inspect …): every function reachable as a property of `t` or its
    /// chain runs with `this` = `t` and unknown primitive arguments, or for
    /// `@@hasInstance` the `instanceof` left-hand sides (`has_instance`).
    pub fn implicit(&mut self, t: Tok, reason: u32, proto: Protocol) {
        // I32 at rest (PR B): a protocol touch reads elements undecoded.
        self.rest_used(t);
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
        if proto.reads_all() {
            reads.push(Key::any());
        }
        let calls = keys.as_ref().is_none_or(|k| !k.is_empty());
        if !calls && reads.is_empty() {
            return;
        }
        // `PERRY_WPF_IMPLSRC=1`: one stderr line per protocol request on `t`
        // (`keys` `None`: any method may run), naming the requesting rule
        // (the escape ledger's reason), the protocol, the token and its
        // kind; a second line names the implicit call the request opens.
        if implsrc_on() {
            let kind = format!("{:?}", self.toks[t as usize]);
            eprintln!(
                "perry: wpfacts implsrc: tok={t} marker={} done={} proto={proto:?} reason={:?} name={:?} kind={}",
                self.is_marker(t),
                self.implicit_done.contains(&(t, proto)),
                self.reasons[reason as usize],
                self.tok_name(t),
                &kind[..kind.len().min(80)]
            );
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
                    let inst: Vec<Atom> = proto
                        .instance_of_reads()
                        .iter()
                        .map(|k| self.atom(k))
                        .collect();
                    for &a in ks.iter().chain(&inst) {
                        let m = self.marker_load(t, Some(a));
                        self.mark_escape_why(m, &|_| "protocol: on a marker".into());
                    }
                }
            }
            if proto.reads_all() {
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
            for &proto in proto.read_touches() {
                self.cons(dst, Cons::Implicit { reason, proto });
            }
            self.add_tok(recv, t);
        }
        // `t instanceof C` for the constructors on its chain: `C`'s
        // `@@hasInstance` runs with `t` (`has_instance`).
        for k in proto.instance_of_reads() {
            let key = Key::Named(self.atom(k));
            let recv = self.node_l("implicit-recv");
            let dst = self.node_l("implicit-ctor");
            let lhs = self.node_l("implicit-lhs");
            for n in [recv, dst, lhs] {
                self.nodes[n as usize].frame = home;
            }
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::Load { key, dst });
            self.cons(dst, Cons::HasInstance { reason, arg: lhs });
            self.add_tok(lhs, t);
            self.add_tok(recv, t);
        }
        if !calls {
            return;
        }
        let ret = self.node_named("implicit-ret", || format!("implicit-ret({t})"));
        // Protocol results go back to the runtime only (touched below);
        // an awaited one is adopted first (below).
        self.nodes[ret as usize].sink = !proto.awaits_result();
        self.nodes[ret as usize].frame = home;
        // Protocol arguments are primitives (hints, keys), except
        // `@@hasInstance`'s, the `instanceof` left-hand sides. The
        // one protocol that hands user code runtime functions,
        // `then(resolve, reject)`, is modeled where it happens
        // (`builtins::adopt`): the resolvers there are synthetic functions
        // whose argument reaches the result.
        let args = match proto {
            Protocol::HasInstance => vec![self.has_instance_node(t)],
            _ => {
                let arg = self.node_l("implicit-arg");
                self.nodes[arg as usize].frame = home;
                self.add_prim(arg, prim::ALL);
                vec![arg; 4]
            }
        };
        let arg = args[0];
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Tok(t),
            args,
            spread: vec![],
            ret,
            site: 0,
            region: self.thrown,
        });
        self.protocol_calls.insert(call);
        self.rel(|| format!("implicitcall {call} {t} {ret} {arg}"));
        if implsrc_on() {
            eprintln!(
                "perry: wpfacts implsrc-call: call={call} tok={t} proto={proto:?} reason={:?}",
                self.reasons[reason as usize]
            );
        }
        match keys {
            None => self.method_on(t, Key::any(), call),
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
            let settled = if proto.awaits_result() {
                let dst = self.node_l("implicit-settled");
                self.nodes[dst as usize].sink = true;
                self.nodes[dst as usize].frame = home;
                crate::builtins::adopt(self, call, ret, dst);
                dst
            } else {
                ret
            };
            self.cons(
                settled,
                Cons::Implicit {
                    reason,
                    proto: next,
                },
            );
        }
    }

    pub fn cons_line(&self, n: NodeId, c: &Cons) -> String {
        match c {
            Cons::Identity { route } => format!("identity {n} {}", route.name()),
            Cons::Load { key, dst } => format!("load {n} {} {dst}", self.key_name(*key)),
            Cons::ProtoKey { key, dst } => {
                format!("protokey {n} {} {dst}", self.key_name(Key::Named(*key)))
            }
            Cons::OwnValues { dst } => format!("ownvalues {n} {dst}"),
            Cons::IndexHole => format!("indexhole {n}"),
            Cons::IndexKept => format!("indexkept {n}"),
            Cons::LenIndex { lo, hi } => format!("lenindex {n} {lo} {hi}"),
            Cons::LenPermuted { t } => format!("lenpermuted {n} {t}"),
            Cons::CjsPublish { dst } => format!("cjspublish {n} {dst}"),
            Cons::Store { key, src, site } => {
                format!("store {n} {} {src} {site}", self.key_name(*key))
            }
            Cons::Call { call } => format!("call {n} {call}"),
            Cons::CallThis { call, this } => format!("callthis {n} {call} {this}"),
            Cons::New { call } => format!("new {n} {call}"),
            Cons::NewProgram { call } => format!("newprogram {n} {call}"),
            Cons::FromArrayLike { dst } => format!("fromarraylike {n} {dst}"),
            Cons::ReflectNew { call, new_target } => format!("reflectnew {n} {call} {new_target}"),
            Cons::ReflectNewTarget { call, target } => {
                format!("reflectnewtarget {n} {call} {target}")
            }
            Cons::NewNamed { name, call } => format!(
                "newnamed {n} {} {call}",
                esc_key(&self.atoms[*name as usize])
            ),
            Cons::MCall { key, call } => format!("mcall {n} {} {call}", self.key_name(*key)),
            Cons::Chain { origin, key, act } => {
                format!("chain {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::TopSink { reason } => format!("top {n} {reason}"),
            Cons::Raise { region } => format!("raise {n} {region}"),
            Cons::RuntimeStore { reason, retained } => {
                format!("runtimestore {n} {retained} {reason}")
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
            Cons::FieldTruthy { key, test, dst } => {
                format!("fieldtruthy {n} {} {test} {dst}", self.key_name(*key))
            }
            Cons::Fulfil { dst } => format!("fulfil {n} {dst}"),
            Cons::Falsy { dst } => format!("falsy {n} {dst}"),
            Cons::NotUndefined { dst } => format!("notundefined {n} {dst}"),
            Cons::NotBuffer { shared, dst } => format!("notbuffer {n} {dst} shared={shared}"),
            Cons::ProtoObjects { dst } => format!("protoobjects {n} {dst}"),
            Cons::TopIfAny { dst, from, reason } => format!(
                "topifany {n} {dst} {from} {}",
                reason.map_or(-1, |r| r as i64)
            ),
            Cons::RequireSpec { call } => format!("requirespec {n} {call}"),
            Cons::TopIfTop { dst, reason } => format!("topiftop {n} {dst} {reason}"),
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
            Cons::HasInstance { reason, arg } => format!("hasinstance {n} {reason} {arg}"),
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
            Cons::UnnamedSet { target } => format!("unnamedset {n} {target}"),
            Cons::StoreTopProto { origin, src } => format!("storetopproto {n} {origin} {src}"),
            Cons::Super { start, key, act } => {
                format!("super {n} {start} {} {act:?}", self.key_name(*key))
            }
            Cons::SuperFrom { recv, key, act } => {
                format!("superfrom {n} {recv} {} {act:?}", self.key_name(*key))
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
            Cons::AnnexBDefine { call, setter } => format!("annexbdefine {n} {call} {setter}"),
            Cons::AnnexBLookup { call, setter } => format!("annexblookup {n} {call} {setter}"),
            Cons::AccessorInstall { key, f, setter, .. } => {
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
            Cons::NativeParent {
                tm,
                expected,
                reason,
            } => format!("nativeparent {n} {tm} {expected} {reason}"),
            Cons::ShareBuffer { ta } => format!("sharebuffer {n} {ta}"),
            Cons::Captures { rec, pat } => format!("captures {n} {rec} {pat}"),
            Cons::StrReplace { dst, .. } => format!("strreplace {n} {dst}"),
            Cons::ClassReplace { dst, .. } => format!("classreplace {n} {dst}"),
            Cons::RegExpPattern { tok } => format!("regexppattern {n} {tok}"),
            Cons::ViewedBuffer { view, fresh, share } => {
                format!("viewedbuffer {n} {view} {fresh:?} {share}")
            }
            Cons::BoundCall { reason, bound } => format!("boundcall {n} {reason} {bound}"),
            Cons::Arith { dst, .. } => format!("arith {n} {dst}"),
            Cons::Range {
                op,
                other,
                dst,
                right,
            } => format!("range {n} {} {other} {dst} {}", op.name(), *right as u8),
            Cons::Plus { str, num, .. } => format!("plus {n} {str} {num}"),
            Cons::ArrayLenArg { dst } => format!("arraylenarg {n} {dst}"),
            Cons::CaseMap { dst, upper } => format!("casemap {n} {dst} {upper}"),
            Cons::ConsoleFormat { reason, agg } => format!("consolefmt {n} {reason} {agg}"),
            Cons::IdxLoad { idx, dst, .. } => format!("idxload {n} {idx} {dst}"),
            Cons::EnumChain { dst, of, .. } => format!("enumchain {n} {of} {dst}"),
            Cons::IdxKey { obj, dst, .. } => format!("idxkey {n} {obj} {dst}"),
            Cons::ElemUndef { obj, key, dst } => format!("elemundef {n} {obj} {key} {dst}"),
            Cons::IdxOwnLoad { idx, dst, .. } => format!("idxownload {n} {idx} {dst}"),
            Cons::IdxOwnKey { obj, dst, .. } => format!("idxownkey {n} {obj} {dst}"),
            Cons::OwnTest { dst, at } => format!("owntest {n} {dst} {at:?}"),
            Cons::OwnGate { obj, idx, dst, .. } => {
                format!("owngate {n} {obj} {idx} {dst}")
            }
            Cons::IdxMCall { idx, call, .. } => format!("idxmcall {n} {idx} {call}"),
            Cons::IdxKeyCall {
                obj, prim, call, ..
            } => format!("idxkeycall {n} {obj} {prim} {call}"),
            Cons::ForInBody { obj, call } => format!("forinbody {n} {obj} {call}"),
            Cons::ForInKeys { ft, call } => format!("forinkeys {n} {ft} {call}"),
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => format!("definekeyed {n} {o} {reason} {}", *accessor as u8),
            Cons::StoreKeyed { o, src, site, subj } => format!("storekeyed {n} {o} {src} {site} {subj}"),
            Cons::EnumKeyStore { o, src, from, .. } => format!("enumkeystore {n} {o} {src} {from:?}"),
            Cons::EnumKeyAny { o, src, .. } => format!("enumkeyany {n} {o} {src}"),
            Cons::StoreIndexed { o, src, site, .. } => format!("storeindexed {n} {o} {src} {site}"),
            Cons::AccessorKeyed { holder, f, setter } => {
                format!("accessorkeyed {n} {holder} {f} {}", *setter as u8)
            }
            Cons::ThisTo { origin } => format!("thisto {n} {origin}"),
            Cons::CallArgs { call } => format!("callargs {n} {call}"),
            Cons::RetInto { call, run } => format!("retinto {n} {call} {run}"),
            Cons::ArgInto { call, run, i } => format!("arginto {n} {call} {run} {i}"),
            Cons::Spread { into, site } => format!("spread {n} {into} {site}"),
            Cons::SpreadFrom { src, site, .. } => format!("spreadfrom {n} {src} {site}"),
            Cons::ThisInst { call, ft } => format!("thisinst {n} {call} {ft}"),
            Cons::NativeThis { call, f } => format!("nativethis {n} {call} {f}"),
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

impl Model {
    /// An operand's interval for `range::eval`: its own while it holds only
    /// Numbers (nothing to coerce), empty while it holds nothing, else ⊤.
    /// What a value-copy delivery from `n` carries beside its values.
    pub(crate) fn rg(&self, n: NodeId) -> crate::model::Rg {
        let nd = &self.nodes[n as usize];
        crate::model::Rg {
            num: nd.range,
            chr: nd.crange,
            frac: nd.frac,
        }
    }

    /// The code interval of `x`'s Strings when it holds only Strings
    /// (else ⊤; nothing: empty).
    pub(crate) fn crange_input(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            Iv::EMPTY
        } else if nd.pts.is_empty() && nd.prim == prim::STR {
            nd.crange
        } else {
            Iv::TOP
        }
    }

    /// `Number(x)` over everything `x` holds (`RangeOp::ToNumber`).
    pub(crate) fn to_number_range(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if !nd.pts.is_empty() || nd.prim & (prim::UNDEF | prim::BIGINT) != 0 {
            return Iv::TOP;
        }
        let mut out = Iv::EMPTY;
        if nd.prim & prim::NUM != 0 {
            out = out.join(nd.range);
        }
        if nd.prim & prim::STR != 0 {
            out = out.join(crate::model::range::chars_to_number(nd.crange));
        }
        if nd.prim & prim::BOOL != 0 {
            out = out.join(Iv::int(0, 1));
        }
        if nd.prim & prim::NULL != 0 {
            out = out.join(Iv::int(0, 0));
        }
        let known = prim::NUM | prim::STR | prim::BOOL | prim::NULL;
        if nd.prim & !known != 0 {
            return Iv::TOP;
        }
        out
    }

    pub(crate) fn range_input(&self, x: NodeId) -> Iv {
        let nd = &self.nodes[x as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            Iv::EMPTY
        } else if nd.pts.is_empty() && nd.prim == prim::NUM {
            nd.range
        } else {
            Iv::TOP
        }
    }
}

/// Join `iv` into the node's interval; widen to ⊤ past `WIDEN_STEPS`
/// growths. Whether it grew.
/// Join `iv` into the node's code interval; past `WIDEN_STEPS` growths it
/// is every one-unit String (`CHAR_ANY`), unless ⊤.
pub(crate) fn grow_crange(node: &mut Node, iv: Iv) -> bool {
    let joined = node.crange.join(iv);
    if joined == node.crange {
        return false;
    }
    node.csteps = node.csteps.saturating_add(1);
    node.crange = if joined.is_top() || node.csteps <= crate::model::range::WIDEN_STEPS {
        joined
    } else if joined.within(crate::model::range::CHAR_ANY) {
        crate::model::range::CHAR_ANY
    } else {
        Iv::TOP
    };
    node.drange = true;
    true
}

pub(crate) fn grow_range(node: &mut Node, iv: Iv) -> bool {
    let joined = node.range.join(iv);
    if joined == node.range {
        return false;
    }
    use crate::model::range::{widen, WIDEN_LIMIT, WIDEN_STEPS};
    node.rsteps = node.rsteps.saturating_add(1);
    node.range = if node.rsteps > WIDEN_LIMIT {
        Iv::TOP
    } else if node.rsteps > WIDEN_STEPS {
        widen(node.range, joined)
    } else {
        joined
    };
    node.drange = true;
    true
}

/// `PERRY_WPF_IMPLSRC=1` (`Model::implicit`).
fn implsrc_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_WPF_IMPLSRC").is_some())
}

/// Join `iv` into the result of a [`Cons::Range`] rule (R1, sum widening).
///
/// The rule's result is `op` over its operands' intervals at each
/// evaluation, and every operand only grows, so the join is the rule over
/// the operands' last intervals: a rule result needs no widening of its own.
/// The operands are what widen: a loop counter (a copy target, `grow_range`)
/// climbs the ladder and the loop test clips it back (`gen::narrow`), so
/// `r * 16 + f` over `r, f` in `[0, 7]` is `[0, 119]`, where widening the
/// sum after `WIDEN_STEPS` of its operands' growths jumped to a rung (127,
/// then 255).
///
/// Termination: the rule results form no cycle among themselves (each
/// rule's operands exist before its result node is ruled, and a result has
/// one rule), so every cycle passes a copy target, which widens, and a
/// result grows at most once per operand growth. Past `WIDEN_LIMIT` growths
/// it still climbs the ladder (a backstop that bounds even a cycle the
/// argument missed).
pub(crate) fn grow_rule_range(node: &mut Node, iv: Iv) -> bool {
    use crate::model::range::{widen, WIDEN_LIMIT};
    let joined = node.range.join(iv);
    if joined == node.range {
        return false;
    }
    node.rsteps = node.rsteps.saturating_add(1);
    node.range = if node.rsteps > WIDEN_LIMIT {
        widen(node.range, joined)
    } else {
        joined
    };
    node.drange = true;
    true
}
