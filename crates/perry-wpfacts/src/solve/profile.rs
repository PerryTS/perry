//! Solver diagnostics: the work profile, structure sizes, and the relation
//! and filter checks.

use super::*;

impl Model {
    /// Charge the work since the last mark to what it was spent for, and
    /// from now on to `n` (a popped node) or, with `NO_NODE`, to `phase`
    /// (PERRY_FACTS_PROFILE only).
    pub fn prof_mark(&mut self, n: NodeId, phase: &'static str) {
        if let Some(s) = self.stop_top.as_mut() {
            s.phase = phase;
        }
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
                *at.entry(format!("{} {name}", if st { "S" } else { "L" }))
                    .or_default() += 1;
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
    pub(super) fn rel_generic_fn_arrival(&mut self, n: NodeId, toks: &Tokens<'_>) {
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
    pub(super) fn filter_check(&mut self, n: NodeId, toks: Tokens<'_>) {
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
}
