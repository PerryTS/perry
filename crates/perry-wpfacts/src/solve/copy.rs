//! Copy edges subscribe to future deliveries; registration sends current values.
use crate::delta::{Batch, Delta, Targets, Tokens};
use crate::model::*;
impl Model {
    /// Copy edge a → b (pts(a) ⊆ pts(b)).
    pub fn edge(&mut self, a: NodeId, b: NodeId) {
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
        if self.copy_coverage(a, b).0 {
            self.add(b, &[], prim);
            return;
        }
        if self.nodes[a as usize].pts.is_empty() && prim == 0 {
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[a as usize].pts.sorted_into(&mut toks);
        // Registration sends what `a` holds: solve work.
        self.work += toks.len() as u64;
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = a;
        self.cur_why = u32::MAX;
        self.copy_to(a, b, Tokens::Slice(&toks), prim);
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    pub(super) fn copy_to(&mut self, src: NodeId, dst: NodeId, toks: Tokens<'_>, prim: u16) -> u64 {
        self.add_tokens(dst, toks, prim);
        // Equal sizes prove equal sets only while `dst` holds all of `src`.
        if !self.nodes[dst as usize].dropped
            && self.nodes[src as usize].pts.len() > 1
            && self.nodes[src as usize].pts.len() == self.nodes[dst as usize].pts.len()
        {
            let a = self.nodes[src as usize].pts.shared_owners();
            let b = self.nodes[dst as usize].pts.shared_owners();
            if a > b || (a == b && src < dst) {
                self.nodes[dst as usize].pts = self.nodes[src as usize].pts.shared_clone();
            } else {
                self.nodes[src as usize].pts = self.nodes[dst as usize].pts.shared_clone();
            }
        }
        0
    }
    pub(super) fn copy_coverage(&self, src: NodeId, dst: NodeId) -> (bool, u64) {
        (
            self.nodes[src as usize]
                .pts
                .covered_by(&self.nodes[dst as usize].pts),
            1,
        )
    }
    /// Every member not addressed by this pending delivery reached the
    /// destination at registration or in an older delivery. Cardinality
    /// proves that these are all its old members, so only handles need move.
    pub(super) fn try_copy_delivery(
        &mut self,
        src: NodeId,
        dst: NodeId,
        delta: &Delta,
        prim: u16,
    ) -> bool {
        let count = delta.delivery_len(dst);
        let source = &self.nodes[src as usize];
        let target = &self.nodes[dst as usize];
        let len = source.pts.len();
        if count == 0 || len.checked_sub(count) != Some(target.pts.len()) || target.dropped {
            return false;
        }
        // A marker that reads as ⊤ is delivered by `add_tokens` (DECISIONS 42),
        // a marker crossing into the node too (MRK, `Model::crosses`).
        if !self.param_marker.is_empty()
            && Tokens::Delivery(delta, dst)
                .iter()
                .any(|&t| self.crosses(dst, t))
        {
            return false;
        }
        if (!self.marker_top.is_empty() || !self.escaped.is_empty())
            && Tokens::Delivery(delta, dst)
                .iter()
                .any(|&t| self.marker_is_top(t) || self.top_beside(t))
        {
            return false;
        }
        let batch = delta.batch_for(dst);
        let pts = self.nodes[src as usize].pts.shared_clone();
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
        let targets = Targets::capture(
            &self.nodes[dst as usize].succ,
            self.nodes[dst as usize].succ_set.as_ref(),
        );
        self.nodes[dst as usize].pts = pts;
        self.work += self.nodes[dst as usize].delta.append_to(batch, &targets);
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
