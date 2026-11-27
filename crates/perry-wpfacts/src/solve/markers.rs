//! Argument markers and sink nodes: marker links and hand-overs, the
//! parameter reach of a store through a marker, and `sink_node`.

use super::*;

impl Model {
    /// DECISIONS 118: what an argument marker's stores and `StoreOut`s do
    /// to each marker that linked in since (`link_marker`).
    pub(super) fn replay_marker_links(&mut self) {
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
    pub(super) fn hand_over_marker(&mut self, t: Tok, reason: u32) {
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
    pub(super) fn param_reach_key(&self, f: FnIdx, j: u32, t: Tok) -> Option<Atom> {
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
    pub(super) fn store_out_arg_reach(
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
    pub(super) fn arg_param_of(&self, t: Tok, f: FnIdx) -> Option<u32> {
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
    #[track_caller]
    pub fn sink_node(&mut self, n: NodeId, reason: u32) {
        if let Some(l) = self.ledger.as_mut() {
            if l.dropped(std::panic::Location::caller(), n, false) {
                return;
            }
            l.sink(n, reason, std::panic::Location::caller());
        }
        if let Some(d) = &mut self.diagnostic {
            if let Some(c) = d.cause_node.filter(|&c| c != n) {
                d.store_top.entry(n).or_insert(c);
            }
        }
        // The native's region gets what that code may throw once a value
        // reaches `n` (HandRaise), not when the sink is attached: an empty
        // slot (`fetch(url)`: no `init`, no `init.body`) hands nothing.
        if let Some(c) = self.cur_native_call {
            let region = self.calls[c as usize].region;
            let seen = self.nodes[n as usize]
                .cons
                .iter()
                .any(|x| matches!(x, Cons::HandRaise { region: r } if *r == region));
            if region != u32::MAX && !seen {
                self.cons(n, Cons::HandRaise { region });
            }
        }
        // Per level: a node sunk with a contract reason is sunk again with
        // a writer reason (`Model::writes_any`), never the other way.
        let first = if self.writes_any(reason) {
            self.sunk_nodes.insert(n);
            self.sunk_writer_nodes.insert(n)
        } else {
            !self.sunk_writer_nodes.contains(&n) && self.sunk_nodes.insert(n)
        };
        if first {
            self.cons(n, Cons::TopSink { reason });
        }
    }
}
