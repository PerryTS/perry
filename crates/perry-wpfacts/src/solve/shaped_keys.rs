//! Own fields under a shaped computed key (`model/key_shape.rs`): a read
//! sees only the fields whose name the shape admits, plus every write under
//! a computed key; a store writes only those fields, and computed-key
//! reads. Named fields made later join through `field_shaped`, as numeric
//! fields join `num_fields`.
//!
//! Only a program object of the plain kind is split this way: a marker's
//! fields are its reach markers, an array-like's elements and named keys
//! have their own nodes, and other values keep `all_fields`. Per unit
//! (`unit_mode`) nothing is split: a literal leaving its unit key by key
//! (`fields_leave`) sinks its named fields and `field_any`, which a shaped
//! store under a name no field has yet would not reach.

use super::*;

impl Model {
    fn shape_splits(&self, t: Tok, key: Key) -> Option<KeyShapeId> {
        match key {
            Key::Any(s)
                if s != 0
                    && !self.unit_mode
                    && matches!(self.toks[t as usize], TokKind::Obj(..))
                    && !self.is_marker(t)
                    && !self.is_array_like(t) =>
            {
                Some(s)
            }
            _ => None,
        }
    }

    /// The own-field values a computed-key read of `t` sees.
    pub fn key_fields(&mut self, t: Tok, key: Key) -> NodeId {
        let Some(s) = self.shape_splits(t, key) else {
            return match key {
                Key::Named(a) => self.field(t, a),
                Key::Any(_) => self.all_fields(t),
            };
        };
        if let Some(&n) = self.shaped_reads.get(&(t, s)) {
            return n;
        }
        let n = self.node_named("shapedread", || format!("shapedread({t},{s})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.shaped_reads.insert((t, s), n);
        let any = self.field_any(t);
        self.edge(any, n);
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            if self.key_may_name(key, a) {
                let f = self.field(t, a);
                self.edge(f, n);
            }
        }
        // Another shaped store may write a name this shape admits.
        for (_, w, store) in self.shaped_of.get(&t).cloned().unwrap_or_default() {
            if store {
                self.edge(w, n);
            }
        }
        self.shaped_of.entry(t).or_default().push((s, n, false));
        n
    }

    /// Where a computed-key store into `t` writes.
    pub fn key_store_node(&mut self, t: Tok, key: Key) -> NodeId {
        let Some(s) = self.shape_splits(t, key) else {
            return self.any_key_node(t);
        };
        if let Some(&n) = self.shaped_stores.get(&(t, s)) {
            return n;
        }
        let n = self.node_named("shapedstore", || format!("shapedstore({t},{s})"));
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.shaped_stores.insert((t, s), n);
        let all = self.all_fields(t);
        self.edge(n, all);
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            if self.key_may_name(key, a) {
                let f = self.field(t, a);
                self.edge(n, f);
            }
        }
        for (_, r, store) in self.shaped_of.get(&t).cloned().unwrap_or_default() {
            if !store {
                self.edge(n, r);
            }
        }
        self.shaped_of.entry(t).or_default().push((s, n, true));
        n
    }

    /// A field `n` of `t` named `a`, made after shaped reads or stores of
    /// `t`: it joins those whose shape admits `a`.
    pub(super) fn field_shaped(&mut self, t: Tok, a: Atom, n: NodeId) {
        let Some(v) = self.shaped_of.get(&t).cloned() else {
            return;
        };
        for (s, w, store) in v {
            if self.key_may_name(Key::Any(s), a) {
                match store {
                    true => self.edge(w, n),
                    false => self.edge(n, w),
                }
            }
        }
    }
}
