//! Running the solve: the worklist fixpoint, the late-escape prune, and
//! the inert bodies' detach and wake.

use super::*;

/// Work between two checks of the time budget (beside every 1024 pops).
const BUDGET_WORK_STEP: u64 = 1 << 22;

impl Model {
    /// Work no solve has run yet: what was added since the last one (a
    /// contract check's escapes and the instances they make) is part of
    /// the unit's facts only once solved.
    pub(crate) fn unsolved(&self) -> bool {
        !self.wl.is_empty()
            || !self.sum.pending.is_empty()
            || !self.marker_links.is_empty()
            || self.act.as_ref().is_some_and(|a| !a.woken.is_empty())
    }

    /// D2: once the summaries are built, the rules inert bodies attached
    /// to nodes they do not own leave the graph (an observation holder's
    /// stay: a summary template, delivering nothing outside instances).
    pub(crate) fn detach_foreign(&mut self) {
        let Some(a) = self.act.as_mut().filter(|a| a.inert) else {
            return;
        };
        let mut foreign = std::mem::take(&mut a.foreign);
        let mut edges = Vec::new();
        for (&f, list) in foreign.iter_mut() {
            if !self.fn_inert(f) {
                continue;
            }
            for e in list.iter_mut() {
                match e {
                    crate::model::Foreign::Cons(h, ci, _, det) => {
                        if !self.nodes[*h as usize].obs {
                            if let Cons::HandRaise { region } =
                                self.nodes[*h as usize].cons[*ci as usize]
                            {
                                if let Some(k) = self.hand_raise.get_mut(&(*h, region)) {
                                    *k -= 1;
                                }
                            }
                            self.nodes[*h as usize].cons[*ci as usize] = Cons::Detached;
                            *det = true;
                        }
                    }
                    crate::model::Foreign::Edge(x, y) => edges.push((*x, *y)),
                    crate::model::Foreign::Effect(_) => {}
                }
            }
        }
        for (x, y) in edges {
            let set_aside = self
                .act
                .as_ref()
                .is_some_and(|a| a.foreign_edges.contains(&(x, y)));
            if set_aside && !self.nodes[x as usize].obs {
                self.succ_remove(x, y);
            }
        }
        self.act.as_mut().unwrap().foreign = foreign;
    }

    /// D2: the inert bodies activated since the last fill: each node they
    /// own sends what it holds along its edges and through its rules (as
    /// registration and attachment would have, had the body been active),
    /// the rules they set aside are attached again, and a summarized one's
    /// generation-fed `this` gets its shared instance.
    pub(crate) fn wake_activated(&mut self) {
        loop {
            let q = match self.act.as_mut() {
                Some(a) if !a.woken.is_empty() => std::mem::take(&mut a.woken),
                _ => return,
            };
            for f in q {
                if self.aborted {
                    return;
                }
                let a = self.act.as_mut().unwrap();
                let nodes = a
                    .fn_nodes
                    .get_mut(f as usize)
                    .map(std::mem::take)
                    .unwrap_or_default();
                let foreign = a.foreign.remove(&f).unwrap_or_default();
                for &n in &nodes {
                    let mut i = 0;
                    while i < self.nodes[n as usize].succ.len() {
                        let b = self.nodes[n as usize].succ[i];
                        self.register_copy(n, b);
                        i += 1;
                    }
                    let mut ci = 0;
                    while ci < self.nodes[n as usize].cons.len() {
                        let c = self.nodes[n as usize].cons[ci];
                        self.replay_cons(n, c, ci as u32);
                        ci += 1;
                    }
                    let node = &mut self.nodes[n as usize];
                    if !node.in_wl {
                        node.in_wl = true;
                        self.wl.push_back(n);
                    }
                }
                let saved = self.cur_fn;
                self.cur_fn = f;
                for e in foreign {
                    match e {
                        crate::model::Foreign::Cons(h, ci, c, det) => {
                            if det {
                                if let Cons::HandRaise { region } = c {
                                    *self.hand_raise.entry((h, region)).or_default() += 1;
                                }
                                self.nodes[h as usize].cons[ci as usize] = c;
                            }
                            self.own_refuted_cons(h, c);
                            self.replay_cons(h, c, ci);
                        }
                        crate::model::Foreign::Edge(x, y) => self.edge_quiet(x, y),
                        crate::model::Foreign::Effect(e) => self.apply_effect(e),
                    }
                }
                self.cur_fn = saved;
                if self.sum.of.contains_key(&f) {
                    self.eager_this(f);
                }
            }
        }
    }

    pub fn solve(&mut self) {
        self.solve_fixpoint();
        // The interned sets stay with their nodes; their index is the solve's.
        self.sets.clear();
    }

    /// R-ESCLATE (DECISIONS 42, `Model::top_beside`): an escaped value
    /// beside ⊤ is that ⊤. A node that held ⊤ and a value before the value
    /// escaped drops the value, as it does when ⊤ arrives after the escape
    /// (`add_tokens_s`) or the escaped value after ⊤. The prune visits every
    /// node, so it runs once the work since the last one reaches the node
    /// count: it costs at most the work it follows. A dropped member leaves
    /// the node's delta too. One that escapes between two prunes may still
    /// be delivered: the node delivered ⊤ beside it, so the receiver holds
    /// ⊤ and drops it on arrival (`add_tokens_s`).
    pub(super) fn prune_late_escaped(&mut self) {
        let late = std::mem::take(&mut self.late_esc);
        self.late_esc_at = self.work.get();
        let mut members = Vec::new();
        for n in 0..self.nodes.len() {
            let (esc, pm, ab) = (&self.esc_writer_bits, &self.param_marker, &self.arg_bits);
            let cover = |t: Tok| escaped_beside_top(esc, pm, ab, t);
            let node = &mut self.nodes[n];
            if !node.pts.contains_tok(TOP) {
                continue;
            }
            // Every other member that escaped before this prune left at an
            // earlier one, or when it or ⊤ arrived.
            let holds = if late.len() < node.pts.len() {
                late.iter().any(|&t| cover(t) && node.pts.contains_tok(t))
            } else {
                members.clear();
                node.pts.sorted_into(&mut members);
                members.iter().any(|&t| cover(t))
            };
            if !holds {
                continue;
            }
            if self.ledger.is_some() || self.k3_held.is_some() {
                members.clear();
                node.pts.sorted_into(&mut members);
                let gone: Vec<Tok> = members.iter().copied().filter(|&t| cover(t)).collect();
                self.ledger_again_beside_top(n as NodeId, &gone);
            }
            let (esc, pm, ab) = (&self.esc_writer_bits, &self.param_marker, &self.arg_bits);
            let cover = |t: Tok| escaped_beside_top(esc, pm, ab, t);
            let node = &mut self.nodes[n];
            let old = std::mem::take(&mut node.pts);
            let before = old.len() as u64;
            node.pts = self.sets.retain(old, |t| !cover(t));
            let pending = node.delta.len() as u64;
            self.pending.retain(&mut node.delta, |t| !cover(t));
            node.dropped = true;
            self.charge_work(before + pending);
        }
    }

    pub(super) fn solve_fixpoint(&mut self) {
        self.rel_phase = b'S';
        self.spelled_atoms.get_or_insert(self.atoms.len());
        self.mark_params();
        self.members_settled.get_mut().clear();
        let mut delta = crate::delta::Delta::default();
        loop {
            self.prof_mark(NO_NODE, "activations");
            self.wake_activated();
            self.prof_mark(NO_NODE, "instances");
            self.fill_instances();
            self.prof_mark(NO_NODE, "marker links");
            self.replay_marker_links();
            self.prof_mark(NO_NODE, "settle");
            if !self.late_esc.is_empty()
                && self.work.get() >= self.late_esc_at.saturating_add(self.nodes.len() as u64)
            {
                self.prof_mark(NO_NODE, "late escapes");
                self.prune_late_escaped();
            }
            let Some(n) = self.wl.pop_front() else {
                if self.sum.pending.is_empty()
                    && self.marker_links.is_empty()
                    && self.act.as_ref().is_none_or(|a| a.woken.is_empty())
                    && !self.settle_for_in()
                    && !self.settle_val_runs()
                    && !self.settle_define()
                    && !self.settle_proto_first()
                    && !self.settle_realm_set()
                    && !self.settle_record_entry()
                    && !self.settle_not_error()
                    && !self.settle_ta_len()
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
            if self.inert_node(n) {
                // D2: an inert body's node keeps what it holds; its
                // activation re-delivers it (`wake_activated`).
                self.nodes[n as usize].in_wl = false;
                continue;
            }
            if self.profile.is_some() && self.pops & 0x3ffff == 0 {
                eprintln!("perry: wpfacts progress {}", self.size_line());
            }
            // The time budget is checked every 1024 pops, and also once the
            // work counter has advanced by `BUDGET_WORK_STEP` since the last
            // check: a pop can cost millions of work units, and then 1024
            // pops outlast any budget.
            if self.pops & 0x3ff == 0
                || self.work.get() >= self.budget_work_at.saturating_add(BUDGET_WORK_STEP)
            {
                self.budget_work_at = self.work.get();
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
            if delta.is_empty() && dprim == 0 && !dstrs && !drange {
                continue;
            }
            if self.nodes[n as usize].front_watch && !delta.is_empty() {
                let ts: Vec<Tok> = delta.iter().copied().collect();
                self.rest_front_arrived(n, &ts);
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
                self.prov_in_set(0);
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
                    self.prov_in_set(0);
                }
                let rule0 = self.stop_top.as_mut().map(|s| s.enter_rule(n, c, i as u32));
                self.apply_tokens(n, &c, Tokens::Pending(&delta), p);
                if let (Some(r0), Some(s)) = (rule0, self.stop_top.as_mut()) {
                    s.leave_rule(r0);
                }
            }
            delta.clear();
        }
        self.cur_src = NO_NODE;
        self.cur_why = u32::MAX;
        (self.prov_mode, self.prov_rule) = (crate::provenance::PM_ADD, None);
        if self.prov.is_some() {
            self.prov_in_set(0);
        }
        #[cfg(debug_assertions)]
        self.assert_shadowed_complete();
    }
}
