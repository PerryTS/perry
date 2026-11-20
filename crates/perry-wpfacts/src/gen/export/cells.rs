//! Cells shared with other units: seeding, waking and the flows they carry.

use super::*;

impl<'a> Gen<'a> {
    /// Design §17.16 (b): cell `key` as this unit reads it: node `n` also
    /// holds what the other units write (their contracts' stand-ins), and
    /// an unknown value when a unit solve went over the bound.
    /// DECISIONS 171: a unit reads a cell where its solve consumes the
    /// cell's node (a successor edge or a constraint on it). One nothing
    /// consumes yet is neither seeded nor read: the other units' writes
    /// cannot reach its facts, and what it publishes there is its own
    /// write. The first consumer wakes it (`Model::cell_watch`) and
    /// `wake_cells` seeds it before the unit solves again. Every unit
    /// names the registry union `cjs:*`; on AWFY 18 of 471 consume it.
    pub(crate) fn seed_cell(&mut self, key: &str, n: NodeId) {
        let node = &self.m.nodes[n as usize];
        if self.unit_in.is_some() && node.succ.is_empty() && node.cons.is_empty() {
            self.cells_deferred.push((key.to_string(), n));
            self.m.cell_watch.insert(n);
            return;
        }
        self.seed_cell_now(key, n);
    }

    /// The deferred cells a consumer reached in the last solve, seeded now.
    /// True when there were any: the unit solves again.
    pub(crate) fn wake_cells(&mut self) -> bool {
        let woken = std::mem::take(&mut self.m.cell_woken);
        if woken.is_empty() {
            return false;
        }
        let mut now: Vec<(String, NodeId)> = Vec::new();
        self.cells_deferred.retain(|(k, n)| {
            let w = woken.contains(n);
            if w {
                now.push((k.clone(), *n));
            }
            !w
        });
        for (k, n) in now {
            self.seed_cell_now(&k, n);
        }
        true
    }

    fn seed_cell_now(&mut self, key: &str, n: NodeId) {
        self.cells_read.push(key.to_string());
        // A unit whose globals unmodeled code may write publishes `global:*`
        // unknown: every global cell reads it.
        let wide = key.starts_with("global:").then_some("global:*");
        if let Some(w) = wide {
            self.cells_read.push(w.to_string());
        }
        let Some(u) = self.unit_in else { return };
        let me = self.view.map(|v| v.key.clone()).unwrap_or_default();
        let seed = self.m.node_named("cell", || format!("cell({key})"));
        self.cell_seeds.insert(n, seed);
        if u.cells_top {
            self.m
                .add_top(seed, |_| "cell written by a unit over the bound".into());
            self.m.add_prim(seed, prim::ALL);
        }
        let writes = u.cells.get(key).into_iter().flatten();
        let wide = wide.and_then(|w| u.cells.get(w)).into_iter().flatten();
        // One stand-in per distinct write (DECISIONS 113); this unit's own
        // write is in its own solve (DECISIONS 89).
        for w in writes.chain(wide) {
            if w.writers.len() == 1 && w.writers[0] == me {
                continue;
            }
            let v = self.stand_in_value(0, key, &w.c);
            if n == self.thrown {
                let held = self.m.nodes[v as usize].pts.to_sorted();
                self.m.thrown_in.extend(held);
            }
            self.m.edge(v, seed);
        }
        self.m.edge(seed, n);
    }

    /// DECISIONS 162: array cell `cells[i]` holding a marker that stands
    /// for own parameters of this unit's contract functions, none escaped
    /// (`Model::own_param_members`): the body stored the callers' argument
    /// into a shared array, which their units' solves hold, not this one.
    /// The marker is described by a flow (`FnContract::param_cells`): each
    /// caller's unit stores its argument into its view of the cell, as the
    /// whole program runs that store. A marker that later turns ⊤ is no
    /// longer described: the cell check sinks it then. Only the elements of
    /// an array: a named field of a shared array, a global or a program
    /// object cell keep the check. True when it recorded a new flow.
    pub(super) fn cell_flows_of(&mut self, i: usize) -> bool {
        let (key, n) = (&self.cells[i].0, self.cells[i].1);
        if !(key.starts_with("arr:") || key.starts_with("box:")) || key.ends_with("#named") {
            return false;
        }
        let key = key.clone();
        let mut changed = false;
        for t in self.m.nodes[n as usize].pts.to_sorted() {
            if !self.m.is_marker(t) || self.m.beside_values(t) {
                continue;
            }
            let Some(own) = self.m.own_param_members(t) else {
                self.cell_flow_toks.remove(&t);
                continue;
            };
            self.cell_flow_toks.insert(t);
            for (f, p) in own {
                changed |= self.cell_flows.insert((f, p, key.clone()));
            }
        }
        changed
    }
}
