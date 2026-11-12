//! Field nodes of a token (named, any-key, numeric, private) and the spreads and export-star copies that feed them.

use super::*;

impl Model {
    /// ECMA-262 22.2.3.1 RegExpAlloc: every RegExp object holds `lastIndex`
    /// as an own data property, `[[Configurable]]: false`, from birth: no
    /// delete or defineProperty removes it or makes it an accessor, so a
    /// lookup of it never reaches a prototype, whatever is written there.
    pub(crate) fn regexp_last_index(&self, t: Tok, a: Atom) -> bool {
        self.atoms[a as usize] == "lastIndex"
            && matches!(&self.toks[t as usize], TokKind::Obj(tm, _)
                if matches!(self.tmpls[*tm as usize].kind, TmplKind::BObj { what: "RegExp", .. }))
    }

    pub fn field(&mut self, t: Tok, a: Atom) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.field(t, a));
        }
        if let Some(&n) = self.fields.get(&(t, a)) {
            return n;
        }
        let n = self.node_named("field", || format!("field({t},{a})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::Named(self.atoms[a as usize].clone()),
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
        self.fields.insert((t, a), n);
        self.field_keys.entry(t).or_default().push(a);
        // A RegExp object's `lastIndex` is the Number 0 from birth
        // (`regexp_last_index`).
        if self.regexp_last_index(t, a) {
            self.add_prim(n, prim::NUM);
        }
        self.rel(|| format!("fieldnode {n} {t} {a}"));
        // A marker has no fields (`store_into`): its any-key field holds
        // only its reach marker. Under a named key it holds that key's
        // reach marker instead (design §18.4).
        // A path marker's named field holds its step (DECISIONS 158).
        // A class private field (`Model::private_atom`) is reached by its
        // name only, by its class's code: no computed key writes or reads
        // it, no spread copies it, no reflection lists it.
        if self.is_private_atom(a) {
            return n;
        }
        if self.is_marker(t) && (!self.reach_marker.contains(&t) || self.is_path_marker(t)) {
            let r = self.keyed_reach_of(t, a);
            self.add_tok(n, r);
        } else {
            // An array's named non-index key is its own key: its elements
            // (`field_any`) are read under an index only (`named_any`). A
            // module namespace's named key is resolved by name (DECISIONS
            // 215): it never reads the namespace's other keys.
            let named = matches!(self.toks[t as usize], TokKind::ModNs(_))
                || self.is_elem_array(t) && !numeric_key(&self.atoms[a as usize]);
            let any = match named {
                true => self.named_any(t),
                false => self.field_any(t),
            };
            self.edge(any, n);
            // A Number-valued key can name this field, including NaN,
            // Infinity and exponent forms. Other named fields never read
            // numeric-only writes. The range is a value fact, not a type.
            if self.is_num_atom(a) && self.atoms[a as usize] != NUM_KEY {
                let numeric = self.numeric_field(t);
                self.edge(numeric, n);
            }
            if let Some(srcs) = self.star_from.get(&t).cloned() {
                for s in srcs {
                    self.star_key(t, s, a, n);
                }
            }
        }
        let all = self.all_fields(t);
        self.edge(n, all);
        self.field_shaped(t, a, n);
        if let Some(&nf) = self.num_fields.get(&t) {
            if self.is_num_atom(a) {
                self.edge(n, nf);
            }
        }
        // A key gained after a spread of `t`: the targets get it too.
        if let Some(v) = self.spread_to.get(&t).cloned() {
            for (d, site) in v {
                self.store_into(d, Key::Named(a), n, site);
            }
        }
        // DECISIONS 94: a named field of an array whose elements are a cell.
        if let Some(&seed) = self.cell_arrs.get(&t) {
            self.edge(seed, n);
        }
        // DECISIONS 241: what the units stored under this key.
        if let Some(j) = self.joined_of(t) {
            if let Some(&(_, v)) = j.keys.iter().find(|(k, _)| *k == a) {
                self.edge(v, n);
            }
        }
        n
    }

    /// Object spread of source `s` into target `d` (`{...s}`,
    /// `Object.assign(d, s)`): every own key of `s`, now and later, is
    /// stored under the same key on `d`; computed keys and getter results
    /// of `s` reach `d`'s any-key field. An unmodeled source makes `d`'s
    /// keys unknown; an unmodeled target takes `s`'s values.
    pub(super) fn spread_into(&mut self, s: Tok, d: Tok, site: SiteId, from: NodeId) {
        self.rest_used(s);
        if !self.spread_done.insert((s, d)) {
            return;
        }
        if let Some(&tm) = self.lit_spread_site.get(&site) {
            if self.tmpl_of(d) == Some(tm) {
                let keys = self.spread_source_keys(s);
                self.spread_source(tm, site, keys);
            }
        }
        if d == TOP {
            let r = self.reason("spread into a value from unmodeled code");
            let all = if s == TOP {
                self.top_tok_node
            } else if builtins::builtin_function(self, s) {
                let read = self.node_l("spread-native-fields");
                self.builtin_field_use(s, Key::any(), ChainAct::Load(read));
                read
            } else {
                self.all_fields(s)
            };
            self.sink_node(all, r);
            return;
        }
        let opaque = !self.own_props_modelled(s);
        // Any key of the source may be an index of an Array target.
        self.mark_hole(d);
        self.map_open(d);
        let fa_d = self.any_key_node(d);
        self.builtin_field_use(s, Key::any(), ChainAct::Load(fa_d));
        if opaque {
            // Census only: a spread of ⊤ is propagation of the ⊤ its source value holds, so the
            // injection is attributed to that source node, not to the target node whose rule runs.
            let saved = match self.diagnostic.as_mut() {
                Some(dg) if s == TOP && from != NO_NODE => Some(dg.cause_node.replace(from)),
                _ => None,
            };
            self.add_top(fa_d, |_| "spread of an unmodeled object".into());
            if let (Some(dg), Some(saved)) = (self.diagnostic.as_mut(), saved) {
                dg.cause_node = saved;
            }
            self.add_prim(fa_d, prim::ALL);
            if s == TOP {
                return;
            }
        }
        self.spread_to.entry(s).or_default().push((d, site));
        for a in self.field_keys.get(&s).cloned().unwrap_or_default() {
            if self.is_private_atom(a) {
                continue;
            }
            let f = self.field(s, a);
            self.store_into(d, Key::Named(a), f, site);
        }
        let fa_s = self.field_any(s);
        self.edge(fa_s, fa_d);
        // A key of the source the model does not name (computed, symbol) is
        // a [[Set]] of some key on the target, as a named one is
        // (`store_into`): the setters on the target's chain run with the
        // target as `this` (ECMA-262 20.1.2.1 step 4.b.iii.2), once the
        // source has such a key (an unmodeled source may have any).
        if opaque {
            self.accessor_walk(d, d, Key::any(), ChainAct::Store(fa_s));
        } else {
            self.cons(fa_s, Cons::UnnamedSet { target: d });
        }
        // Numeric-only source properties keep their range through a copy.
        let numeric_s = self.numeric_field(s);
        let numeric_d = self.numeric_field(d);
        self.edge(numeric_s, numeric_d);
        // Getters of `s` (own and on the chain) run; their results have no
        // known key here.
        let act = ChainAct::Load(fa_d);
        self.any_walk_holders.entry(s).or_default().push((s, act));
        self.accessor_hit(s, s, Key::any(), act);
    }

    /// DECISIONS 241: the join's values of function token `t`'s own
    /// properties (`fn_joined`).
    fn joined_of(&self, t: Tok) -> Option<crate::model::FnJoined> {
        if self.fn_joined.is_empty() {
            return None;
        }
        match self.toks.get(t as usize) {
            Some(TokKind::Func(f, _)) => self.fn_joined.get(f).cloned(),
            _ => None,
        }
    }

    /// Values stored under an unknown Number key, plus arbitrary-key
    /// writes that may name a number. This is the NUM_KEY range in the
    /// ordinary field space, not a concrete own key: do not publish it as
    /// a layout key or a definite key copied by a spread.
    pub(super) fn numeric_field(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.numeric_field(t));
        }
        let a = self.atom(NUM_KEY);
        let n = match self.fields.get(&(t, a)).copied() {
            Some(n) => n,
            None => {
                let n = self.node_named("numericfield", || format!("numericfield({t})"));
                #[cfg(test)]
                {
                    self.nodes[n as usize].field_origin =
                        Some(crate::summary::packet::FieldOrigin {
                            object: Some(t),
                            key: crate::summary::packet::FieldPort::Numeric,
                        });
                }
                self.nodes[n as usize].obs = self.obs_toks.contains(&t);
                self.nodes[n as usize].frame = self.fam_home(t);
                self.nodes[n as usize].of_marker = self.is_marker(t);
                self.fields.insert((t, a), n);
                self.rel(|| format!("fieldnode {n} {t} {a}"));
                n
            }
        };
        let any = self.field_any(t);
        self.edge(any, n);
        if self.is_elem_array(t) {
            // Array algorithms and iteration consume these properties too.
            self.edge(n, any);
        }
        let all = self.all_fields(t);
        self.edge(n, all);
        if let Some(&numeric) = self.num_fields.get(&t) {
            self.edge(n, numeric);
        }
        n
    }

    pub fn field_any(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.field_any(t));
        }
        if let Some(&n) = self.field_any.get(&t) {
            return n;
        }
        let n = self.node_named("fieldany", || format!("fieldany({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::Any,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
        self.field_any.insert(t, n);
        self.rel(|| format!("fieldany {n} {t}"));
        let all = self.all_fields(t);
        self.edge(n, all);
        // DECISIONS 241: what the units stored under a computed key.
        if let Some(v) = self.joined_of(t).and_then(|j| j.any) {
            self.edge(v, n);
        }
        // Arrays and primitive boxes hold primitives at unknown indices.
        n
    }

    /// An Array of the program: its elements (`field_any`: what its
    /// construction, `push` and the like put at an index, and every store
    /// under a computed key) are no value of a named non-index key, which
    /// reads `named_any` instead. A marker's fields are its reach markers.
    ///
    /// A Map, Set, WeakMap or WeakSet (`builtins::is_collection`) likewise:
    /// its entries (`field_any`) sit in internal slots and are no property
    /// at all, so a named key (`map.get`) never reads them; a store under a
    /// computed key is a property and reaches both.
    pub fn is_elem_array(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
        if match self.tmpls[tm as usize].kind {
            TmplKind::Arr { .. } | TmplKind::Args { .. } => true,
            TmplKind::BObj { what, .. } => builtins::is_collection(what),
            _ => false,
        }) && !self.is_marker(t)
    }

    /// The values of `t` under a key no store named: for an Array the ones
    /// its named non-index keys read (`named_any`, flowing on into its
    /// elements: the key may be an index too), else `field_any`. Every
    /// write under an unknown key goes here: a computed-key store, unknown
    /// code's writes (escape, hard), a spread, a computed-key accessor.
    pub fn any_key_node(&mut self, t: Tok) -> NodeId {
        match self.is_elem_array(t) {
            true => self.named_any(t),
            false => self.field_any(t),
        }
    }

    /// DECISIONS 215 (ECMAScript ResolveExport): `export * from` the module
    /// of namespace `src` (`TOP`: a source outside the program) makes each
    /// name of `src` a name of namespace `ns`, unless `ns`'s module exports
    /// that name itself; `default` is never one. A computed key reads every
    /// key (`field_any`); a named key reads only that name of each source.
    pub(crate) fn export_star(&mut self, ns: Tok, src: Tok) {
        let fa = self.field_any(ns);
        if src == TOP {
            self.add_top(fa, |_| "unresolved export * source".into());
        } else {
            let all = self.all_fields(src);
            self.edge(all, fa);
        }
        let v = self.star_from.entry(ns).or_default();
        if v.contains(&src) {
            return;
        }
        v.push(src);
        // The names asked of `ns` before this source was known.
        let keys = self.field_keys.get(&ns).cloned().unwrap_or_default();
        for a in keys {
            let n = self.fields[&(ns, a)];
            self.star_key(ns, src, a, n);
        }
    }

    /// Name `a` of namespace `ns` (node `n`) through its `export *` source
    /// `src` (`export_star`).
    fn star_key(&mut self, ns: Tok, src: Tok, a: Atom, n: NodeId) {
        if self.ns_own.contains(&(ns, a)) || &*self.atoms[a as usize] == "default" {
            return;
        }
        if src == TOP {
            self.add_top(n, |_| "unresolved export * source".into());
        } else {
            let f = self.field(src, a);
            self.edge(f, n);
        }
    }

    /// An Array's values under a computed key that may be no index
    /// (`is_elem_array`): its named non-index keys read them, and they are
    /// among its elements as well.
    pub fn named_any(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.named_any(t));
        }
        if let Some(&n) = self.named_any.get(&t) {
            return n;
        }
        let n = self.node_named("namedany", || format!("namedany({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::NamedAny,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.named_any.insert(t, n);
        self.rel(|| format!("namedany {n} {t}"));
        let any = self.field_any(t);
        self.edge(n, any);
        n
    }

    /// Are the own properties of every object `t` denotes in the model, so
    /// that a copy of them (`spread_into`, `own_values_of`) reads its
    /// fields: the named fields, the any-key field and the numeric field,
    /// each fed by everything that defines a property on it? A program
    /// object's are: every store, `defineProperty` and accessor goes through
    /// the field funnel. A builtin object's are when its template is a
    /// container the model fills (`builtins::own_values_filled`): its birth
    /// puts every value the runtime gives it under its any key, and the
    /// runtime gives it no own accessor. A builtin namespace or constructor
    /// the realm gives no enumerable own property
    /// (`builtins::realm_own_props_unenumerable`) holds, for a copy, only
    /// what the program stored on it: its fields. Every other builtin object
    /// (Date, RegExp, typed arrays, collections, markers …), a host
    /// namespace, a builtin function, a native prototype and ⊤ hold own
    /// properties the model does not list. A stand-in (`site: u32::MAX`, another unit's array as this
    /// unit sees it, DECISIONS 94) holds what other units wrote in a cell,
    /// not in these fields.
    pub(crate) fn own_props_modelled(&self, t: Tok) -> bool {
        match &self.toks[t as usize] {
            TokKind::Builtin(ns) => builtins::realm_own_props_unenumerable(*ns),
            TokKind::Top | TokKind::BuiltinFn(_) | TokKind::NativeProto(_) => false,
            TokKind::Obj(tm, _) => match self.tmpls[*tm as usize].kind {
                TmplKind::BObj { what, site, .. } => {
                    site != u32::MAX && builtins::own_values_filled(what)
                }
                _ => true,
            },
            _ => true,
        }
    }

    /// dst ⊇ the values of `t`'s own properties (`Cons::OwnValues`).
    pub(crate) fn own_values_of(&mut self, t: Tok, dst: NodeId) {
        self.rest_used(t);
        let opaque = !self.own_props_modelled(t);
        if opaque {
            self.add_top(dst, |_| "own values of an unmodeled object".into());
            self.add_prim(dst, prim::ALL);
        }
        if t == TOP {
            return;
        }
        self.builtin_field_use(t, Key::any(), ChainAct::Load(dst));
        let fields = self.all_fields(t);
        self.edge(fields, dst);
        let act = ChainAct::Load(dst);
        // Any-key lookups subscribe through the holder relation;
        // named-key replay in `visits` does not cover them.
        self.any_walk_holders.entry(t).or_default().push((t, act));
        self.accessor_hit(t, t, Key::any(), act);
    }

    /// What class private field `a` (`Model::private_atom`) holds on any
    /// object: every store of the class's code under it lands here
    /// (`store_into`). Only that code names it, all of it in this unit, and
    /// an object holds it only once the class's own initializer ran on it
    /// (the brand), so a read on a receiver this unit does not own (a
    /// marker, ⊤, another unit's instance) finds what this holds: the
    /// value never leaves through that receiver.
    pub fn private_field(&mut self, a: Atom) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.private_field(a));
        }
        if let Some(&n) = self.private_fields.get(&a) {
            return n;
        }
        let n = self.node_named("private", || format!("private({a})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: None,
                key: crate::summary::packet::FieldPort::Private(self.atoms[a as usize].clone()),
            });
        }
        self.private_fields.insert(a, n);
        n
    }

    /// `t` is an object of this unit (no marker, ⊤ or stand-in): its
    /// private fields are its own field nodes.
    pub(crate) fn owns_private(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) => {
                !self.is_marker(t) && !self.foreign.contains_key(&tm)
            }
            _ => false,
        }
    }

    /// What Array `t`'s iteration reads at a hole: `undefined` once `t`
    /// may hold one. A birth may (`TmplKind::Arr` `hole_free` false; another
    /// unit's array, whose stores this unit does not see); so does a store
    /// under an index or a computed key, or of `length` (`mark_hole`). An
    /// append (`push`, `unshift`, `splice`: the element key) fills the next
    /// index and leaves none. Unknown code's writes (escape, HARD) put
    /// every primitive among the elements already (`any_key_node`).
    /// Negative control `PERRY_FACTS_SABOTAGE=nohole`: no Array has one.
    pub fn hole(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.hole(t));
        }
        if let Some(&n) = self.holes.get(&t) {
            return n;
        }
        let n = self.node_named("hole", || format!("hole({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::Hole,
            });
        }
        self.nodes[n as usize].frame = self.fam_home(t);
        self.holes.insert(t, n);
        let born_dense = match self.toks[t as usize] {
            TokKind::Obj(tm, _) => {
                matches!(
                    self.tmpls[tm as usize].kind,
                    TmplKind::Arr {
                        hole_free: true,
                        ..
                    }
                ) && !self.foreign.contains_key(&tm)
            }
            _ => false,
        };
        if !born_dense && !self.no_hole_sabotage {
            self.add_prim(n, prim::UNDEF);
        }
        n
    }

    /// A write that may leave Array `t` with a hole (`hole`).
    pub fn mark_hole(&mut self, t: Tok) {
        if t != TOP && self.is_arr(t) && !self.no_hole_sabotage {
            let h = self.hole(t);
            self.add_prim(h, prim::UNDEF);
        }
    }

    pub fn all_fields(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.all_fields(t));
        }
        if let Some(&n) = self.all_fields.get(&t) {
            return n;
        }
        let n = self.node_named("allfields", || format!("allfields({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::All,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.nodes[n as usize].of_marker = self.is_marker(t);
        self.all_fields.insert(t, n);
        self.rel(|| format!("allfields {n} {t}"));
        // DECISIONS 241: every key the units stored, read under any key.
        if let Some(j) = self.joined_of(t) {
            for (_, v) in j.keys {
                self.edge(v, n);
            }
        }
        // A path marker read under a computed key: its `[]` step
        // (DECISIONS 158).
        if self.is_path_marker(t) {
            let s = self.path_step_of(t, None);
            self.add_tok(n, s);
        }
        n
    }

    /// The fields of `t` a number key may name (DECISIONS 217): those under
    /// a numeric key or the element key, and what it holds under a
    /// computed key. ToPropertyKey of a number names no other.
    pub fn num_fields(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.num_fields(t));
        }
        if let Some(&n) = self.num_fields.get(&t) {
            return n;
        }
        let n = self.node_named("numfields", || format!("numfields({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::NumberKeys,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.nodes[n as usize].frame = self.fam_home(t);
        self.num_fields.insert(t, n);
        let fa = self.field_any(t);
        self.edge(fa, n);
        let numeric = self.numeric_field(t);
        self.edge(numeric, n);
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            if self.is_num_atom(a) {
                let f = self.field(t, a);
                self.edge(f, n);
            }
        }
        n
    }

    fn is_num_atom(&self, a: Atom) -> bool {
        let s = &self.atoms[a as usize];
        numeric_key(s) || s == ELEM || s == NUM_KEY
    }
}
