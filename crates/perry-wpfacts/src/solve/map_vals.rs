//! The values of a Map, apart from its keys.
//!
//! A collection's contents are one node (`Model::field_any`): `set(k, v)`
//! puts the key and the value there, and every reader (iteration, `keys`,
//! `values`, `entries`, `forEach`, an escape) reads both. That is all a
//! Set needs. For a Map it makes `get` return the keys too: with string
//! keys, `m.get(k)` may be a String, and a field read of the result
//! (`entry.move`) may be the `undefined` a String's missing property reads,
//! however the program guards the miss.
//!
//! `get` reads the Map's **value node** instead (`map_vals`), which holds
//! what `set` stored as a value. That is every value the Map holds only
//! while each write of its contents is a `set` this model runs: the Map is
//! **closed**. A Map is closed from its birth when the program's own
//! `new Map()` (no iterable) bears it in this unit (`map_born`), and it is
//! **opened** for good, its value node then holding all its contents as
//! before, by anything else that may write them:
//! - an escape or a HARD marker (unknown code may call `set`);
//! - a `set` whose arguments are spread (their positions are not known);
//! - a store under a computed key and a copy of another object's
//!   properties into it (no contents by the language, but the runtime's
//!   own helpers store through them: `js_weakmap_set`);
//! - a birth from an iterable, or by anything but `new Map()`.
//!
//! Every other Map (another unit's, a marker, a subclass instance, a
//! WeakMap, a per-unit solve) is never closed. The contents node is
//! unchanged, so no other reader sees a difference.
//!
//! `PERRY_FACTS_MAP_VALUES=0` turns it off (`get` reads the contents).
//! Probe: one `# map_values` line per Map birth (`probe`).

use super::*;

/// `PERRY_FACTS_MAP_VALUES=0` (`off`, `false`): `Map.prototype.get` reads
/// the Map's whole contents, its keys included.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_MAP_VALUES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

impl Model {
    /// The program's `new Map(...)` bore `t` (`empty`: without an
    /// iterable). The first empty birth of a Map of this unit closes it; a
    /// birth from an iterable opens it.
    pub(crate) fn map_born(&mut self, t: Tok, empty: bool) {
        if !self.map_values {
            return;
        }
        let first = self.map_seen.insert(t);
        if !empty {
            self.map_open(t);
            return;
        }
        let own = match self.toks[t as usize] {
            TokKind::Obj(tm, _) => !self.foreign.contains_key(&tm),
            _ => false,
        };
        if first && own && !self.unit_mode && !self.is_marker(t) && !self.is_escaped(t) {
            self.map_closed.insert(t);
        }
    }

    /// Something other than a modelled `set` may write Map `t`'s contents
    /// from now on: its value node holds all of them.
    pub(crate) fn map_open(&mut self, t: Tok) {
        if self.map_closed.is_empty() || !self.map_closed.remove(&t) {
            return;
        }
        if let Some(&v) = self.map_vals.get(&t) {
            let fa = self.field_any(t);
            self.edge(fa, v);
        }
    }

    /// What `get` of collection `t` reads: the value node of a Map, which
    /// is all its contents unless the Map is closed.
    pub(crate) fn map_vals(&mut self, t: Tok) -> NodeId {
        if !self.map_values {
            return self.field_any(t);
        }
        if let Some(&n) = self.map_vals.get(&t) {
            return n;
        }
        let n = self.node_named("map-vals", || format!("map-vals({t})"));
        self.map_vals.insert(t, n);
        if !self.map_closed.contains(&t) {
            let fa = self.field_any(t);
            self.edge(fa, n);
        }
        n
    }

    /// `t.set(args...)` of a Map: the second argument is the value
    /// (`undefined` when absent). Spread arguments may land anywhere.
    /// Negative control `PERRY_FACTS_SABOTAGE=mapvals`: the key is taken
    /// for the value.
    pub(crate) fn map_set(&mut self, t: Tok, args: &[NodeId], spread: bool) {
        if !self.map_values {
            return;
        }
        if spread {
            self.map_open(t);
            return;
        }
        let v = self.map_vals(t);
        let at = if self.map_values_sabotage { 0 } else { 1 };
        match args.get(at) {
            Some(&a) => self.edge(a, v),
            None => self.add_prim(v, prim::UNDEF),
        }
    }

    /// The probe (`PERRY_FACTS_DUMP`): one `# map_values` line per Map the
    /// program's `new Map` bore: whether it is still closed, and what its
    /// value node and its contents node hold (`prims` as the prim bits,
    /// `toks` the number of object tokens).
    pub(crate) fn map_values_probe(&self) -> Vec<String> {
        if !self.map_values {
            return Vec::new();
        }
        let mut seen: Vec<Tok> = self.map_seen.iter().copied().collect();
        seen.sort_unstable();
        let side = |n: Option<&NodeId>| match n {
            Some(&n) => {
                let d = &self.nodes[n as usize];
                format!("prims=0x{:x} toks={}", d.prim, d.pts.len())
            }
            None => "none".to_string(),
        };
        seen.into_iter()
            .map(|t| {
                let name = match self.toks[t as usize] {
                    TokKind::Obj(tm, _) => self.tmpls[tm as usize].display.clone(),
                    _ => String::new(),
                };
                format!(
                    "# map_values map={} tok={t} closed={} values=({}) contents=({})",
                    name.replace(' ', "_"),
                    self.map_closed.contains(&t) as u8,
                    side(self.map_vals.get(&t)),
                    side(self.field_any.get(&t)),
                )
            })
            .collect()
    }
}
