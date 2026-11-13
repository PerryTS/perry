//! OWN1: a computed read behind an own-property test of the same object and
//! key (`gen/own_guard.rs`) reads own properties only.
//!
//! `hasOwnProperty.call(o, k) ? o[k] : …`: the read runs only when `o` holds
//! `k` as an own property, and nothing runs between the test and the read,
//! so it never reaches `o`'s prototype chain (the inherited `constructor`,
//! the `Object.prototype` methods). It reads the own fields the key may name
//! and runs an own getter as any read does (`accessor_walk`; a getter on
//! the chain stays a superset). The rule holds for program objects whose
//! properties the model holds (literals, instances, `Object.create`
//! results); any other value, an object or ⊤ key (its ToPropertyKey runs
//! code at the test and again at the read), and a test whose callee may be
//! anything but the own-property builtin or a body-derived own-test
//! helper read as `IdxLoad` does.

use super::*;
use crate::model::{
    IDX_OWN_ANY, IDX_OWN_GATE, IDX_OWN_KEY, IDX_OWN_NUM, IDX_OWN_PRIM, IDX_OWN_STR,
};

impl Model {
    /// Markers expose reach fields rather than value fields. Other token
    /// kinds use a general view; concrete object tokens and function values
    /// use key ranges. A function value (program function, bound function,
    /// class, built-in function, built-in constructor such as `Object`
    /// (IsCallable, `surely_callable`), `Object.prototype` method) holds
    /// every property the program wrote in its field table: a number key
    /// names one numeric string (ECMA-262 7.1.19), so `IdxKey` reads its
    /// numeric fields and the chain, never a named field. A program
    /// prototype (`C.prototype`) is an ordinary object: `O[P]` is
    /// ToPropertyKey, then OrdinaryGet (ECMA-262 13.3.2.1, 7.3.2, 10.1.8.1),
    /// so it yields property P and its chain only; `IdxKey` reads the names
    /// the key admits (every field for a key of unknown content), never all
    /// methods and the `constructor` under a known key.
    pub(super) fn index_needs_all_own_fields(&self, t: Tok) -> bool {
        self.is_marker(t)
            || !(matches!(
                self.toks[t as usize],
                TokKind::Obj(..)
                    | TokKind::Proto(_)
                    | TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_))
            ) || crate::builtins::surely_callable(self, t))
    }
    /// `Cons::IdxLoad` for one object token `t`.
    pub(super) fn idx_load_tok(
        &mut self,
        idx: NodeId,
        t: Tok,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
    ) {
        if enumerable
            && t != TOP
            && (self.enum_free(t) || self.enum_obj(t) || builtins::enum_free_instance(self, t))
        {
            return self.enum_read_tok(idx, t, dst, shape);
        }
        // A for-in run's key (FVA 4.3b): `o[k]` reads that one key.
        if let Some(&a) = self.key_nodes.get(&idx) {
            self.load_from(t, t, Key::Named(a), dst);
            return;
        }
        if t == TOP {
            self.add_tok(dst, TOP);
            return;
        }
        // An object's own fields: by the key's values (`IdxKey`: a number
        // key names only its numeric fields). A marker's fields are its
        // reach markers under any key; another value (the global object, a
        // namespace) keeps every field.
        if self.index_needs_all_own_fields(t) {
            let all = self.all_fields(t);
            self.edge(all, dst);
        }
        // PR-C1: a program Array with a length interval reads `undefined`
        // past its end only (`Cons::ElemUndef`, watching the key and the
        // length); below it, an element or a hole (`hole`).
        let watched = self.elem_undef_watched(t);
        if !watched {
            self.add_prim(dst, prim::UNDEF);
        }
        if self.idx_done.insert((idx, t, dst, 0)) {
            if watched {
                let h = self.hole(t);
                self.edge(h, dst);
                let len = self.arr_len(t);
                let c = Cons::ElemUndef {
                    obj: t,
                    key: idx,
                    dst,
                };
                self.cons(idx, c);
                self.cons(len, c);
            }
            self.plain_key(t, ChainAct::Load(dst));
            self.cons(idx, Cons::IdxKey { obj: t, dst, shape });
        }
    }

    /// ENUM1/ENUM2: `o[k]` of `for (k in o)` on built-in object `t` none of
    /// whose built-in properties is enumerable, or on a program object
    /// (`enum_obj`): `k` is a key the program wrote on `t` or on its chain
    /// (`Cons::EnumChain`, up to the first prototype with enumerable
    /// built-in members); a deleted key reads `undefined`. No built-in
    /// member of a link the rule covers, no generic member.
    pub(super) fn enum_read_tok(&mut self, idx: NodeId, t: Tok, dst: NodeId, shape: KeyShapeId) {
        if self.enum_obj(t) {
            self.mark_use(t, crate::model::MARK_READ);
        }
        self.add_prim(dst, prim::UNDEF);
        let key = match self.key_nodes.get(&idx) {
            Some(&a) => Key::Named(a),
            None => Key::Any(shape),
        };
        self.enum_fields(t, t, key, dst);
        if self
            .idx_done
            .insert((idx, t, dst, crate::model::IDX_ENUM_READ))
        {
            self.accessor_walk(t, t, key, ChainAct::Load(dst));
            let pn = self.proto_node(t);
            self.cons(
                pn,
                Cons::EnumChain {
                    key,
                    dst,
                    of: t,
                    from: t,
                },
            );
        }
    }

    /// ENUM3: `o[k] = src` where `k` is the key of `for (k in s)` over a
    /// local `s` the ENUM1/ENUM2 read rule scopes (`Gen::enum_scope`): `k`
    /// is one of the enumerable String keys of `s` and its chain (ECMA-262
    /// 14.7.5.9 EnumerateObjectProperties, 14.7.5.10 %ForInIteratorPrototype%.next:
    /// own keys whose [[Enumerable]] is true, then the prototype's), so on
    /// a link the read rule covers (`enum_free`, `enum_obj`,
    /// `enum_free_instance` for `s`, `enum_link` up the chain) the store
    /// writes only the names the program wrote there, never a built-in
    /// member such as `Object.prototype.valueOf` (non-enumerable, clause
    /// 18, 20.1.3). Names gained later are stored too (`enum_key_gained`).
    /// Any String key for: ⊤, a primitive subject (a String's indices), an
    /// array (its index keys), a link the read rule does not cover, a link
    /// written under an unknown key (its any-key field holds a value: a
    /// computed store, a `JSON.parse` object, unknown code's write) and a
    /// built-in link a store into ⊤ may have written (`enum_top_written_any`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn enum_key_store(
        &mut self,
        n: NodeId,
        o: NodeId,
        src: NodeId,
        site: SiteId,
        from: Option<Tok>,
        toks: crate::delta::Tokens<'_>,
        prim: u16,
    ) {
        if from.is_none() && prim & !(prim::NULL | prim::UNDEF) != 0 {
            if enum_diag() {
                eprintln!("perry: wpfacts enumdiag site={site} reason=prim prim={prim}");
            }
            self.enum_key_any(o, src, site);
        }
        for &t in toks {
            // The any-String store of this (o, src) holds every key a name
            // could add (`enum_key_named`): nothing more to store.
            if self.enum_key_any_done(o, src, site) {
                return;
            }
            let covered = match from {
                None => self.enum_subject(t),
                Some(f) => t != TOP && self.enum_link(f, t),
            };
            if enum_diag() {
                let r = if t == TOP {
                    "top".to_string()
                } else if covered {
                    "exact".to_string()
                } else if from.is_none() {
                    "uncovered-subject".to_string()
                } else {
                    format!("uncovered-link from={}", self.tok_name(from.unwrap()))
                };
                eprintln!(
                    "perry: wpfacts enumdiag site={site} reason={r} depth={} tok={}",
                    from.is_some() as u8,
                    if t == TOP { "TOP".to_string() } else { self.tok_name(t) }
                );
            }
            if !covered {
                self.enum_key_any(o, src, site);
                continue;
            }
            if !self.enum_key_done.insert((n, t, o, src)) {
                continue;
            }
            self.enum_key_stores.entry(t).or_default().push((o, src, site));
            let star = self.atom(crate::STAR_KEY);
            if self.accessor_keys.get(&t).is_some_and(|ks| ks.contains(&star)) {
                self.enum_key_any(o, src, site);
            }
            let names = self.field_keys.get(&t).cloned().unwrap_or_default();
            for a in names {
                if self.nodes[self.fields[&(t, a)] as usize].enum_key {
                    self.enum_key_named(o, src, site, a);
                }
            }
            let w = self.enum_key_watch(t);
            self.cons(w, Cons::EnumKeyAny { o, src, site });
            if !self.enum_obj(t) {
                let w = self.node_l("enum-key-top-written");
                builtins::enum_top_written_any(self, t, w);
                self.cons(w, Cons::EnumKeyAny { o, src, site });
            }
            let up = self.proto_node(t);
            self.cons(
                up,
                Cons::EnumKeyStore {
                    o,
                    src,
                    site,
                    from: Some(t),
                },
            );
        }
    }

    /// ENUM3: a for-in subject whose enumerable String keys are the names
    /// the program wrote on it (`Node::enum_key` fields) and what
    /// `enum_key_watch` collects: a built-in object of `enum_free`, a
    /// program value (`enum_obj`) or a built-in instance
    /// (`builtins::enum_free_instance`); not ⊤, not an array (its index
    /// keys). The one predicate of the keyed store (`enum_key_store`) and
    /// of the per-key for-in runs (`Model::for_in_keys`).
    pub(crate) fn enum_subject(&self, t: Tok) -> bool {
        t != TOP
            && (self.enum_free(t) || self.enum_obj(t) || builtins::enum_free_instance(self, t))
            && !self.is_elem_array(t)
            && !self
                .tmpl_of(t)
                .is_some_and(|tm| matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    }

    /// ENUM3: does what `enum_key_watch(t)` collects hold a value now: was
    /// `t` written under a key no name lists (its any-key field, a shaped
    /// computed-key store)? Read without making the watch node.
    pub(crate) fn enum_any_written(&self, t: Tok) -> bool {
        let held = |n: NodeId| {
            let nd = &self.nodes[n as usize];
            !nd.pts.is_empty() || nd.prim != 0
        };
        let any = match self.is_elem_array(t) {
            true => self.named_any.get(&t),
            false => self.field_any.get(&t),
        };
        // The numeric range (`numeric_field`): a Number or BigInt key's
        // store names a numeric string no name lists (7.1.19).
        let numeric = self
            .atom_index
            .get(crate::model::NUM_KEY)
            .and_then(|&a| self.fields.get(&(t, a)));
        any.is_some_and(|&n| held(n))
            || numeric.is_some_and(|&n| held(n))
            || self
                .shaped_of
                .get(&t)
                .is_some_and(|v| v.iter().any(|&(_, n, store)| store && held(n)))
    }

    /// ENUM3: is `a` a key a for-in may yield? String keys only (ECMA-262
    /// 14.7.5.9 EnumerateObjectProperties): never a private name, a symbol
    /// (a well-known symbol's `@@` key, `util.inspect.custom`'s) nor an
    /// internal slot.
    pub(crate) fn enum_string_key(&self, a: Atom) -> bool {
        let name = &self.atoms[a as usize];
        !(self.is_private_atom(a)
            || name.starts_with("@@")
            || name == crate::model::INSPECT_CUSTOM_METHOD
            || builtins::is_internal_key(name))
    }

    /// ENUM3: what `t` holds under keys no name lists: its any-key field and
    /// every shaped computed-key store into it (`key_store_node`), now and
    /// later. A node of its own: a field node of an observed token runs no
    /// rule (`sum_skip`).
    fn enum_key_watch(&mut self, t: Tok) -> NodeId {
        if let Some(&w) = self.enum_key_watch.get(&t) {
            return w;
        }
        let w = self.node_l("enum-key-any");
        self.enum_key_watch.insert(t, w);
        let fa = self.any_key_node(t);
        self.edge(fa, w);
        for (_, n, store) in self.shaped_of.get(&t).cloned().unwrap_or_default() {
            if store {
                self.edge(n, w);
            }
        }
        w
    }

    /// ENUM2: what `t` holds under the keys a for-in over it yields: its
    /// enumerable own fields (`Node::enum_key`, the one predicate the
    /// keyed store of ENUM3 uses too; a key marked later joins,
    /// `enum_key_mark`), its index keys (an element: every write of one,
    /// [[Set]] or CreateDataProperty, makes an enumerable data property,
    /// 10.1.9.2, 7.3.5; `enum_index_field`) and what it holds under keys
    /// no name lists (`enum_key_watch`). Not a non-enumerable own property: a class
    /// prototype's `constructor` and methods (ECMA-262 15.7.14
    /// ClassDefinitionEvaluation, MakeConstructor 10.2.5; 10.2.8
    /// DefineMethodProperty, enumerable false), which `all_fields` holds.
    pub(crate) fn enum_key_fields(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.enum_key_fields.get(&t) {
            return n;
        }
        let n = self.node_l("enum-key-fields");
        self.enum_key_fields.insert(t, n);
        let w = self.enum_key_watch(t);
        self.edge(w, n);
        // An element array's (a typed array's) elements are its any-key
        // field (its non-index keys, `named_any`, are in `w`).
        if self.elems_apart(t) {
            let fa = self.field_any(t);
            self.edge(fa, n);
        }
        // The numeric range (`numeric_field`: values stored under a Number
        // or BigInt key, ToPropertyKey 7.1.19, made by OrdinarySet as
        // enumerable data properties 10.1.9.2) made before this node; a
        // later one links itself (`enum_index_field`). It is no
        // `field_keys` entry.
        let num = self.atom(crate::model::NUM_KEY);
        if let Some(&nf) = self.fields.get(&(t, num)) {
            self.edge(nf, n);
        }
        // DECISIONS 241: what the units stored, by key (enumerability not
        // carried): every key, as `all_fields` reads it.
        if let Some(j) = self.joined_of(t) {
            for (_, v) in j.keys {
                self.edge(v, n);
            }
        }
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            let f = self.fields[&(t, a)];
            if self.enum_own(t, f, a) {
                self.edge(f, n);
            }
        }
        n
    }

    /// ENUM2: index field `f` of `t` (an element key, or the element node)
    /// made after `enum_key_fields(t)`: one of its enumerable keys.
    pub(super) fn enum_index_field(&mut self, t: Tok, f: NodeId) {
        if let Some(&e) = self.enum_key_fields.get(&t) {
            self.edge(f, e);
        }
    }

    /// ENUM3: key `a` appeared on token `t`: the keyed stores `t`'s
    /// enumerated keys decide store under it too.
    pub(super) fn enum_key_gained(&mut self, t: Tok, a: Atom) {
        if let Some(v) = self.enum_key_stores.get(&t).cloned() {
            for (o, src, site) in v {
                self.enum_key_named(o, src, site, a);
            }
        }
    }

    fn enum_key_named(&mut self, o: NodeId, src: NodeId, site: SiteId, a: Atom) {
        if !self.enum_string_key(a) || self.enum_key_any_done(o, src, site) {
            return;
        }
        if self.atoms[a as usize] == crate::STAR_KEY {
            return self.enum_key_any(o, src, site);
        }
        self.cons(
            o,
            Cons::Store {
                key: Key::Named(a),
                src,
                site,
            },
        );
    }

    /// Did the any-String store of `src` into `o` at `site` run? It writes
    /// under every String key a for-in yields (ECMA-262 14.7.5.10: String
    /// keys only), so it holds what any named store of the same value
    /// would add.
    fn enum_key_any_done(&self, o: NodeId, src: NodeId, site: SiteId) -> bool {
        self.enum_key_done.contains(&(o, TOP, src, site as NodeId))
    }

    /// The store of a for-in key no rule names: under any String key.
    pub(super) fn enum_key_any(&mut self, o: NodeId, src: NodeId, site: SiteId) {
        if self.enum_key_done.insert((o, TOP, src, site as NodeId)) {
            let key = Key::Any(self.any_string_shape());
            self.cons(o, Cons::Store { key, src, site });
        }
    }

    /// What the program wrote under `key` on link `t` of the chain of `of`:
    /// its fields, and on a built-in object what a store into ⊤ wrote there
    /// (under a named key that key, under any key every key such a store
    /// wrote, as for a computed read of the builtin chain, `any_written`).
    pub(super) fn enum_fields(&mut self, of: Tok, t: Tok, key: Key, dst: NodeId) {
        let f = match key {
            Key::Named(a) => self.field_r(t, a),
            Key::Any(_) => self.enum_key_fields(t),
        };
        self.edge(f, dst);
        if !self.is_builtin_proto(t) {
            return;
        }
        match key {
            Key::Named(a) => self.with_b(|m| builtins::enum_top_written(m, a, dst)),
            Key::Any(_) => self.with_b(|m| builtins::enum_top_written_any(m, of, dst)),
        }
    }

    /// A program object whose properties are its fields here: an own-only
    /// read of it is its fields. Not a marker (use tracking), not an
    /// array-like (its `length` and elements are the builtin model's), not
    /// a builtin object (its members are natives, a Proxy's are code).
    fn own_read_obj(&self, t: Tok) -> bool {
        if t == TOP || self.is_marker(t) || self.is_array_like(t) {
            return false;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return false;
        };
        matches!(
            self.tmpls[tm as usize].kind,
            TmplKind::Lit { .. }
                | TmplKind::Class { .. }
                | TmplKind::FnInst { .. }
                | TmplKind::ObjCreate { .. }
        )
    }

    /// `Cons::IdxOwnLoad` for one object token `t`.
    pub(super) fn idx_own_load_tok(
        &mut self,
        idx: NodeId,
        t: Tok,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
    ) {
        if !self.own_read_obj(t) {
            return self.idx_load_tok(idx, t, dst, shape, enumerable);
        }
        self.mark_use(t, crate::model::MARK_READ);
        self.add_prim(dst, prim::UNDEF);
        if let Some(&a) = self.key_nodes.get(&idx) {
            let f = self.field_r(t, a);
            self.edge(f, dst);
            self.accessor_walk(t, t, Key::Named(a), ChainAct::Load(dst));
            return;
        }
        if self.idx_done.insert((idx, t, dst, IDX_OWN_KEY)) {
            self.cons(idx, Cons::IdxOwnKey { obj: t, dst, shape });
        }
    }

    /// `Cons::IdxOwnKey` on key node `n` of the own-only read of `obj`.
    pub(super) fn idx_own_key(
        &mut self,
        n: NodeId,
        obj: Tok,
        dst: NodeId,
        shape: KeyShapeId,
        toks: Tokens<'_>,
        prim: u16,
    ) {
        // An object key converts by running its own code, at the test and
        // again here: the two keys may differ. ⊤ may be such an object.
        if !toks.is_empty() && self.idx_done.insert((n, obj, dst, IDX_OWN_ANY)) {
            self.own_read_census[2] += 1;
            self.plain_key(obj, ChainAct::Load(dst));
            self.load_from(obj, obj, Key::Any(shape), dst);
        }
        let key = Key::Any(shape);
        if index_needs_general_key(Tokens::Slice(&[]), prim)
            && self.idx_done.insert((n, obj, dst, IDX_OWN_STR))
        {
            let f = self.key_fields(obj, key);
            self.edge(f, dst);
            self.accessor_walk(obj, obj, key, ChainAct::Load(dst));
        }
        // ToPropertyKey of a Number or BigInt is a numeric string: the own
        // numeric fields, and an accessor under a numeric name.
        if index_has_numeric_key(prim) && self.idx_done.insert((n, obj, dst, IDX_OWN_NUM)) {
            let f = self.num_fields(obj);
            self.edge(f, dst);
            if self.numeric_accessor {
                self.accessor_walk(obj, obj, Key::any(), ChainAct::Load(dst));
            }
        }
        // ToPropertyKey of `true`, `false`, `null`, `undefined` is a named key.
        for (i, (bit, keys)) in [
            (prim::BOOL, &["true", "false"][..]),
            (prim::NULL, &["null"][..]),
            (prim::UNDEF, &["undefined"][..]),
        ]
        .into_iter()
        .enumerate()
        {
            if prim & bit != 0 && self.idx_done.insert((n, obj, dst, IDX_OWN_PRIM + i as u16)) {
                for k in keys {
                    let a = self.atom(k);
                    let f = self.field_r(obj, a);
                    self.edge(f, dst);
                    self.accessor_walk(obj, obj, Key::Named(a), ChainAct::Load(dst));
                }
            }
        }
    }

    /// The one own-test predicate. Every nonmatching value invalidates
    /// `dst`; passing builtins watch their replaceable fields, and passing
    /// helpers share their body evidence. Later writes invalidate already
    /// generated guards through the same monotone edges.
    pub(super) fn own_test(&mut self, dst: NodeId, at: OwnGateAt, toks: Tokens<'_>) {
        // Predicate markers are replayed independently of value deltas.
        if at == OwnGateAt::Refuted {
            return;
        }
        for &t in toks {
            let key = match (at, &self.toks[t as usize]) {
                (
                    OwnGateAt::OwnFn,
                    TokKind::Builtin(BuiltinNs::ObjectProtoOwn(
                        ProtoOwn::HasOwnProperty | ProtoOwn::PropertyIsEnumerable,
                    )),
                ) => "call",
                (OwnGateAt::ObjectCtor, TokKind::Builtin(BuiltinNs::Object)) => "hasOwn",
                (OwnGateAt::Helper, TokKind::Func(f, _)) => {
                    match self.funcs[*f as usize].own_test {
                        OwnTestBody::Pending(bad) | OwnTestBody::Checked(bad) => self.cons(
                            bad,
                            Cons::OwnTest {
                                dst,
                                at: OwnGateAt::Refuted,
                            },
                        ),
                        OwnTestBody::Unknown => self.refute_own_test(dst),
                    }
                    continue;
                }
                _ => {
                    self.refute_own_test(dst);
                    continue;
                }
            };
            let a = self.atom(key);
            let f = self.field_r(t, a);
            self.cons(
                f,
                Cons::OwnTest {
                    dst,
                    at: OwnGateAt::Value,
                },
            );
        }
    }

    /// Refutation is control evidence. It never enters points-to sets,
    /// primitive masks, ordinary copy edges or the unknown-value census.
    pub(crate) fn refute_own_test(&mut self, n: NodeId) {
        let mut pending = vec![n];
        while let Some(n) = pending.pop() {
            if self.nodes[n as usize].own_refuted {
                continue;
            }
            self.nodes[n as usize].own_refuted = true;
            let cs = self.nodes[n as usize].cons.clone();
            for c in cs {
                if self.sum_skip(n, &c) {
                    continue;
                }
                match c {
                    Cons::OwnTest {
                        dst,
                        at: OwnGateAt::Refuted,
                    } => pending.push(dst),
                    Cons::OwnGate {
                        obj,
                        idx,
                        dst,
                        shape,
                        enumerable,
                    } => self.own_gate(obj, idx, dst, shape, enumerable),
                    _ => {}
                }
            }
        }
    }

    /// A constraint attached after its predicate was refuted sees that
    /// marker too, including constraints instantiated from summaries.
    pub(super) fn own_refuted_cons(&mut self, n: NodeId, c: Cons) {
        if !self.nodes[n as usize].own_refuted || self.sum_skip(n, &c) {
            return;
        }
        match c {
            Cons::OwnTest {
                dst,
                at: OwnGateAt::Refuted,
            } => self.refute_own_test(dst),
            Cons::OwnGate {
                obj,
                idx,
                dst,
                shape,
                enumerable,
            } => self.own_gate(obj, idx, dst, shape, enumerable),
            _ => {}
        }
    }

    /// Refuted evidence restores the ordinary read, once.
    fn own_gate(
        &mut self,
        obj: NodeId,
        idx: NodeId,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
    ) {
        if self.idx_done.insert((obj, TOP, dst, IDX_OWN_GATE)) {
            self.own_read_census[1] += 1;
            self.cons(
                obj,
                Cons::IdxLoad {
                    idx,
                    dst,
                    shape,
                    enumerable,
                },
            );
        }
    }
}

/// `PERRY_WPF_ENUMDIAG=1`: one stderr line per ENUM3 key-store decision.
pub(super) fn enum_diag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_WPF_ENUMDIAG").is_some())
}

impl Model {
    pub(super) fn enum_diag_any(&self, n: NodeId, site: SiteId, toks: crate::delta::Tokens<'_>) {
        let t = toks.into_iter().next().copied();
        eprintln!(
            "perry: wpfacts enumdiag-any site={site} via={} tok={}",
            self.labels[n as usize],
            t.map(|t| if t == TOP { "TOP".to_string() } else { self.tok_name(t) })
                .unwrap_or_else(|| "prim".into())
        );
    }
}
