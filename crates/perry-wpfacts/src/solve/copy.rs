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
        // Dedup against short successor lists only: a duplicate edge costs
        // a little propagation, a global edge set costs gigabytes.
        // This value can never change again. Deliver the initial literal
        // below, but do not retain an edge waiting for impossible updates.
        if !(self.nodes[a as usize].widened && self.nodes[a as usize].prim == prim::ALL) {
            let b_widened = self.nodes[b as usize].widened;
            {
                let node = &mut self.nodes[a as usize];
                match &mut node.succ_set {
                    Some(set) => {
                        if !std::rc::Rc::make_mut(set).insert(b) {
                            return;
                        }
                    }
                    None => {
                        if node.succ.contains(&b) || (node.widened && node.prim_succ.contains(&b)) {
                            return;
                        }
                        if node.succ.len()
                            + if node.widened {
                                node.prim_succ.len()
                            } else {
                                0
                            }
                            >= 16
                        {
                            let mut set: crate::pts::FxHashSet<NodeId> =
                                node.succ.iter().copied().collect();
                            if node.widened {
                                set.extend(node.prim_succ.iter().copied());
                            }
                            set.insert(b);
                            node.succ_set = Some(std::rc::Rc::new(set));
                        }
                    }
                }
                if (!node.widened && !b_widened) || (node.widened && !node.succ.is_empty()) {
                    node.succ.push(b);
                } else {
                    // A widened source has only TOP forever; the initial copy below
                    // delivers it. A widened target escapes values at the source.
                    // Both kinds of edge carry only future primitive changes.
                    node.esc_out = true;
                    node.prim_succ.push(b);
                }
            }
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
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = a;
        self.cur_why = u32::MAX;
        self.copy_to(a, b, Tokens::Slice(&toks), prim);
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    pub(super) fn copy_to(&mut self, src: NodeId, dst: NodeId, toks: Tokens<'_>, prim: u16) -> u64 {
        self.add_tokens(dst, toks, prim);
        if !self.nodes[dst as usize].widened
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
        if count == 0
            || source.widened
            || target.widened
            || len.checked_sub(count) != Some(target.pts.len())
            || (!target.internal && self.widen_k != 0 && len > self.widen_k)
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
        let top = self.explain
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
        if top {
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
