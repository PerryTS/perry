//! Copy edges subscribe to future deliveries; registration sends current values.
use crate::delta::{Batch, Delta, Tokens};
use crate::model::*;
impl Model {
    pub(crate) fn prune_successors(&mut self, n: NodeId, keep: &[bool]) {
        let node = &mut self.nodes[n as usize];
        self.pending.remap(&node.delta, keep);
        let mut it = keep.iter();
        node.succ.retain(|_| *it.next().unwrap());
        if node.succ_set.is_some() {
            node.succ_set = Some(std::rc::Rc::new(node.succ.iter().copied().collect()));
        }
    }

    /// Copy edge a → b (pts(a) ⊆ pts(b)).
    pub fn edge(&mut self, a: NodeId, b: NodeId) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Edge(a, b)) {
            return;
        }
        self.edge_impl(a, b, true);
    }

    /// `edge` without a relation line: internal relays the checker derives
    /// from a logged rule (`bind_via`).
    pub(super) fn edge_quiet(&mut self, a: NodeId, b: NodeId) {
        self.edge_impl(a, b, false);
    }

    fn edge_impl(&mut self, a: NodeId, b: NodeId, log: bool) {
        if a == b {
            return;
        }
        debug_assert!(!self.nodes[a as usize].sink, "an edge out of a sink node");
        // An observation node feeds only observation nodes.
        if self.nodes[a as usize].obs && !self.nodes[b as usize].obs {
            return;
        }
        // Dedup against short successor lists only: a duplicate edge costs
        // a little propagation, a global edge set costs gigabytes.
        {
            let node = &mut self.nodes[a as usize];
            // Pending batches retain long-set mutation-attempt boundaries,
            // including duplicate insertions, so delivery work stays exact.
            if node.succ.len() > 16 {
                self.pending.split_tail(&node.delta);
            }
            match &mut node.succ_set {
                Some(set) => {
                    if !std::rc::Rc::make_mut(set).insert(b) {
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
                        node.succ_set = Some(std::rc::Rc::new(set));
                    }
                }
            }
            node.succ.push(b);
        }
        if !self.cell_watch.is_empty() && self.cell_watch.remove(&a) {
            self.cell_woken.push(a);
        }
        if log {
            self.rel(|| format!("copy {a} {b}"));
        }
        // Over a bound the program gets no facts: nothing more to propagate.
        if self.aborted {
            return;
        }
        let prim = self.nodes[a as usize].prim;
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        if self.nodes[a as usize]
            .pts
            .same_storage(&self.nodes[b as usize].pts)
        {
            self.nodes[a as usize].pts.sorted_into(&mut toks);
        }
        let covered = self.copy_coverage(a, b, Tokens::Slice(&toks)).0;
        self.bufs.push(toks);
        // Registration is a value-copy delivery: `a`'s Numbers arrive with
        // `a`'s interval.
        let saved_range = self.cur_range.replace(self.nodes[a as usize].range);
        if covered {
            self.add(b, &[], prim);
            self.cur_range = saved_range;
            return;
        }
        if self.nodes[a as usize].pts.is_empty() && prim == 0 {
            self.cur_range = saved_range;
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[a as usize].pts.sorted_into(&mut toks);
        // Registration sends what `a` holds: solve work.
        self.charge_work(toks.len() as u64);
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = a;
        self.cur_why = u32::MAX;
        self.copy_to(a, b, Tokens::Slice(&toks), prim);
        (self.cur_src, self.cur_why) = saved;
        self.cur_range = saved_range;
        self.bufs.push(toks);
    }

    /// Equal sets share storage through the solve's set table
    /// (`SetTable::extend`, `SetTable::intern`), not here.
    pub(super) fn copy_to(
        &mut self,
        _src: NodeId,
        dst: NodeId,
        toks: Tokens<'_>,
        prim: u16,
    ) -> u64 {
        self.add_tokens(dst, toks, prim);
        0
    }
    /// Points-to storage: (logical entries summed over nodes, entries
    /// stored, distinct stored sets of two or more members).
    pub fn pts_storage(&self) -> (u64, u64, u64) {
        let mut seen: crate::pts::FxHashSet<usize> = Default::default();
        let (mut logical, mut stored, mut distinct) = (0u64, 0u64, 0u64);
        for n in &self.nodes {
            let len = n.pts.len() as u64;
            logical += len;
            match n.pts.storage_id() {
                Some(id) if !seen.insert(id) => {}
                _ => {
                    stored += len;
                    distinct += (len >= 2) as u64;
                }
            }
        }
        (logical, stored, distinct)
    }
    /// `dst` holds every member of `src` (a singleton it holds, or the same
    /// interned set), so delivering `toks` (members of `src`) changes
    /// nothing there: skipped. Equal storage also needs that no token
    /// would cross into `dst` as its marker (`add_tokens`), the one effect a
    /// held token can still have.
    pub(super) fn copy_coverage(&self, src: NodeId, dst: NodeId, toks: Tokens<'_>) -> (bool, u64) {
        let (a, b) = (&self.nodes[src as usize].pts, &self.nodes[dst as usize].pts);
        let covered = a.covered_by(b)
            && (!a.same_storage(b)
                || self.param_marker.is_empty()
                || !toks.iter().any(|&t| self.crosses(dst, t)));
        (covered, 1)
    }
    /// Every member not addressed by this pending delivery reached the
    /// destination at registration or in an older delivery. Cardinality
    /// proves that these are all its old members, so only handles need move.
    pub(super) fn try_copy_delivery(
        &mut self,
        src: NodeId,
        dst: NodeId,
        successor: u32,
        delta: &Delta,
        prim: u16,
    ) -> bool {
        let count = delta.delivery_len(successor);
        let source = &self.nodes[src as usize];
        let target = &self.nodes[dst as usize];
        let len = source.pts.len();
        if count == 0 || len.checked_sub(count) != Some(target.pts.len()) || target.dropped {
            return false;
        }
        // A marker that reads as ⊤ is delivered by `add_tokens` (DECISIONS 42),
        // a marker crossing into the node too (MRK, `Model::crosses`).
        if !self.param_marker.is_empty()
            && Tokens::Delivery(delta, successor)
                .iter()
                .any(|&t| self.crosses(dst, t))
        {
            return false;
        }
        if (!self.marker_top.is_empty() || !self.escaped.is_empty())
            && Tokens::Delivery(delta, successor)
                .iter()
                .any(|&t| self.marker_is_top(t) || self.top_beside(t))
        {
            return false;
        }
        let batch = delta.batch_for(successor);
        let pts = self.nodes[src as usize].pts.clone();
        self.adopt_delivery(src, dst, pts, &batch, prim);
        true
    }
    fn adopt_delivery(
        &mut self,
        src: NodeId,
        dst: NodeId,
        pts: crate::pts::Pts,
        batch: &Batch,
        prim: u16,
    ) {
        let top = (self.explain || self.diagnostic.is_some())
            && self.nodes[src as usize].pts.contains_tok(TOP)
            && !self.nodes[dst as usize].pts.contains_tok(TOP);
        let boundary = self.nodes[dst as usize].succ.len() as u32;
        let old = std::mem::replace(&mut self.nodes[dst as usize].pts, pts);
        self.sets.release(old);
        let extra = self
            .pending
            .append_to(&mut self.nodes[dst as usize].delta, batch, boundary);
        self.charge_work(extra);
        let before = self.pts_entries;
        self.pts_entries += batch.len() as u64;
        if top && self.diagnostic.is_some() {
            self.diagnostic_predecessor(dst, src, self.cur_why);
        }
        if top && self.explain {
            self.top_pred
                .entry(dst)
                .or_insert((self.cur_src, self.cur_why));
        }
        if before >> 20 != self.pts_entries >> 20 {
            if self
                .budget
                .is_some_and(|(start, limit)| start.elapsed() > limit)
                || (self.max_pts != 0 && self.pts_entries > self.max_pts)
            {
                self.aborted = true;
            }
        }
        self.add(dst, &[], prim);
        let node = &mut self.nodes[dst as usize];
        if !node.in_wl {
            node.in_wl = true;
            self.wl.push_back(dst);
        }
    }
}
