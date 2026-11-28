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
    EnumFiled, IDX_OWN_ANY, IDX_OWN_GATE, IDX_OWN_KEY, IDX_OWN_NUM, IDX_OWN_PRIM, IDX_OWN_STR,
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
    /// The key of an index read `o[k]` on a primitive `o`: the for-in run's
    /// one key (FVA 4.3b), else every key of the site's shape.
    pub(super) fn prim_index_key(&self, idx: NodeId, shape: KeyShapeId) -> Key {
        match self.key_nodes.get(&idx) {
            Some(&a) => Key::Named(a),
            None => Key::Any(shape),
        }
    }

    /// An own-only read of a primitive under `key` (10.4.3.5
    /// StringGetOwnProperty; a Number, Boolean, Symbol or BigInt wrapper has
    /// no own property): `undefined`, and on a String a one-character
    /// String under an index key, its length under `length`.
    pub(super) fn prim_own_load(&mut self, prim: u16, key: Key, dst: NodeId) {
        if prim & (prim::NULL | prim::UNDEF) == prim {
            return; // TypeError
        }
        self.add_prim(dst, prim::UNDEF);
        if prim & prim::STR == 0 {
            return;
        }
        let (index, length) = match key {
            Key::Named(a) => {
                let s = self.atoms[a as usize].as_str();
                (crate::builtins::members::is_index_name(s), s == "length")
            }
            k => (true, self.key_may_name_str(k, "length")),
        };
        if index {
            self.add_prim(dst, prim::STR);
        }
        if length {
            self.add_prim(dst, prim::NUM);
        }
    }

    /// `Cons::IdxLoad` for one object token `t`.
    pub(super) fn idx_load_tok(
        &mut self,
        idx: NodeId,
        t: Tok,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
        kept: bool,
    ) {
        self.idx_load_from(t, idx, t, dst, shape, enumerable, kept)
    }

    /// `idx_load_tok` with receiver `origin` (rule PXRECV: a Proxy's
    /// forward reads its target with the original receiver): the getters
    /// the read runs have `this` = `origin`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn idx_load_from(
        &mut self,
        origin: Tok,
        idx: NodeId,
        t: Tok,
        dst: NodeId,
        shape: KeyShapeId,
        enumerable: bool,
        kept: bool,
    ) {
        if enumerable
            && t != TOP
            && (self.enum_free(t)
                || self.enum_obj(t)
                || builtins::enum_free_instance(self, t)
                || builtins::enum_free_buffer(self, t))
        {
            let e = EnumFiled::Idx {
                origin,
                idx,
                dst,
                shape,
                kept,
            };
            if self.enum_kept(t) {
                self.enum_file(t, e);
                return self.enum_read_tok(idx, t, dst, shape);
            }
            // E7DEL2: `o[k]` of the walk on an object a delete may have
            // taken keys from: OrdinaryGet under the keys of the walk.
            if self.enum_lost_exact(t) {
                if let Some(&a) = self.key_nodes.get(&idx) {
                    return self.load_from(origin, t, Key::Named(a), dst);
                }
                self.enum_lost_read(t, e);
                return self.enum_read_tok(idx, t, dst, shape);
            }
        }
        // A for-in run's key (FVA 4.3b): `o[k]` reads that one key.
        if let Some(&a) = self.key_nodes.get(&idx) {
            self.load_from(origin, t, Key::Named(a), dst);
            return;
        }
        if t == TOP {
            self.add_tok(dst, TOP);
            return;
        }
        // A String wrapper's index keys are own properties whose values are
        // its one-character Strings (10.4.3.5 StringGetOwnProperty).
        if self.may_wrap_string(t) {
            self.add_prim(dst, prim::STR);
        }
        // An object's own fields: by the key's values (`IdxKey`: a number
        // key names only its numeric fields). A marker's fields are its
        // reach markers under any key; another value (a namespace) keeps
        // every field. The global object under a key of known shape keeps
        // the fields the shape can spell (`global_read_split`).
        if self.index_needs_all_own_fields(t) {
            let own = self.key_fields(t, Key::Any(shape));
            self.edge(own, dst);
        }
        // PR-C1: a program Array with a length interval reads `undefined`
        // past its end only (`Cons::ElemUndef`, watching the key and the
        // length); below it, an element or a hole (`hole`).
        let watched = self.elem_undef_watched(t);
        // The in-bounds read (`gen/in_bounds.rs`): the key was below the
        // length of an Array that only grows, so the read is past its end
        // only under a key that is no index (`Model::kept_read`).
        let kept = (kept || self.exec_index_present(t, idx)) && self.kept_read(t);
        // A key of String constants only (`const_str_key`) reads, per
        // constant, the named [[Get]] (`IdxKey`), which decides the absent
        // key's `undefined` itself; any other key may name a key `t` lacks.
        // A key that holds nothing yet is decided by `IdxKey` when it does.
        let key_decides = !self.is_arr(t) && {
            let k = &self.nodes[idx as usize];
            (k.prim == 0 && k.pts.is_empty()) || self.const_str_key(idx)
        };
        if !watched && !kept && !key_decides {
            self.add_prim(dst, prim::UNDEF);
        }
        if self.idx_done.insert((idx, t, dst, 0)) {
            if watched || kept {
                // A typed array has no holes (rule TA-LEN): an index below
                // its length always names an element (10.4.5.15).
                if self.is_arr(t) {
                    let h = self.hole(t);
                    self.edge(h, dst);
                }
                let c = Cons::ElemUndef {
                    obj: t,
                    key: idx,
                    dst,
                };
                if kept {
                    self.kept_reads.entry(t).or_default().push((idx, dst));
                }
                self.cons(idx, c);
                if watched {
                    let len = self.arr_len(t);
                    self.cons(len, c);
                }
            }
            if self.shape_may_be_numeric(shape) {
                self.plain_key_from(origin, t, ChainAct::Load(dst, self.thrown));
            }
            self.cons(
                idx,
                Cons::IdxKey {
                    obj: t,
                    origin,
                    prim: 0,
                    dst,
                    shape,
                },
            );
        }
    }

    /// ENUM1/ENUM2: `o[k]` of `for (k in o)` on built-in object `t` none of
    /// whose built-in properties is enumerable, or on a program object
    /// (`enum_obj`): `k` is a key the program wrote on `t` or on its chain
    /// (`Cons::EnumChain`, up to the first prototype with enumerable
    /// built-in members), while no delete may have taken a key of `t`
    /// (`enum_kept`). No built-in
    /// member of a link the rule covers, no generic member.
    pub(super) fn enum_read_tok(&mut self, idx: NodeId, t: Tok, dst: NodeId, shape: KeyShapeId) {
        let key = match self.key_nodes.get(&idx) {
            Some(&a) => Key::Named(a),
            None => Key::Any(shape),
        };
        self.enum_read_key_kept(idx, t, key, dst);
    }

    /// F-A on a primitive subject: `s[k]` inside `for (k in s)` reads an
    /// enumerable String key of ToObject(s) (ECMA-262 14.7.5.6
    /// ForIn/OfHeadEvaluation step 6: `null` / `undefined` run no
    /// iteration, any other value is ToObject'ed; 14.7.5.9
    /// EnumerateObjectProperties: own, then inherited, enumerable String
    /// keys). A String object's own enumerable keys are its indices, each
    /// a one-character String (10.4.3.1, 10.4.3.5 StringGetOwnProperty);
    /// its `length` is not enumerable (10.4.3.4 StringCreate). A Number,
    /// Boolean, BigInt or Symbol wrapper has no own property (21.1.1.1,
    /// 20.3.1.1, 21.2.1.1, 20.4.1.1). On the chain (the kind's prototype,
    /// then Object.prototype) every built-in member is non-enumerable
    /// (clause 18), so an inherited key is one the program made enumerable
    /// there: the enumerable read of the kind's prototype (`enum_read_key`),
    /// the rule of an object subject's inherited keys.
    pub(super) fn enum_read_prim(&mut self, idx: NodeId, prim: u16, key: Key, dst: NodeId) {
        let p = prim & !(prim::NULL | prim::UNDEF);
        if p & prim::STR != 0 {
            self.add_prim(dst, prim::STR);
        }
        for (b, ns) in [
            (prim::STR, BuiltinNs::String),
            (prim::NUM, BuiltinNs::Number),
            (prim::BOOL, BuiltinNs::Boolean),
            (prim::BIGINT, BuiltinNs::BigInt),
            (prim::SYMBOL, BuiltinNs::Symbol),
        ] {
            if p & b == 0 {
                continue;
            }
            let pt = self.with_b(|m| builtins::realm_proto_tok(m, &format!("{ns:?}")));
            self.enum_read_key(idx, pt, key, dst);
        }
    }

    /// The enumerable read of `key` on `t` (`Cons::EnumLoad`, a primitive
    /// subject's prototype) while no delete may have taken a key of `t`
    /// (`enum_kept`); the ordinary read once one may.
    pub(super) fn enum_read_key(&mut self, idx: NodeId, t: Tok, key: Key, dst: NodeId) {
        if !self.enum_kept(t) {
            return self.load_from(t, t, key, dst);
        }
        self.enum_file(t, EnumFiled::Key { key, dst });
        self.enum_read_key_kept(idx, t, key, dst)
    }

    /// B-del: does every object `t` denotes keep the own keys it had when a
    /// for-in or an `Object.keys` walk listed them? A key the walk yields
    /// may be deleted before `o[k]` reads it (by the body, a callee, an
    /// alias: ECMA-262 14.7.5.9 skips a key deleted before it is processed,
    /// not after; `Object.keys` snapshots the keys first, 20.1.2.18); the
    /// read is then OrdinaryGet (10.1.8.1), which goes on to the prototype
    /// and finds its NON-enumerable builtin members. So the enumerable read
    /// holds only for an object no delete (`Hard::E7Delete`) or `with`
    /// (`E15With`) met, and, once a delete went through ⊤, that ⊤ does not
    /// denote: not escaped (DECISIONS 234/150) and not a builtin object (a
    /// delete through ⊤ "changes that key of whatever it is, a builtin
    /// prototype included", `Cons::DeleteKey`). Only turns false: the reads
    /// it allowed are filed (`enum_file`) and run again (`enum_flush`).
    pub(crate) fn enum_kept(&self, t: Tok) -> bool {
        !self.enum_lost.contains(&t) && !(self.enum_top_lost && self.enum_top_reach(t))
    }

    fn enum_top_reach(&self, t: Tok) -> bool {
        self.escaped.contains_key(&t) || self.is_builtin_proto(t)
    }

    /// Files enumerable read `e` of `t` for `enum_flush`.
    pub(crate) fn enum_file(&mut self, t: Tok, e: EnumFiled) {
        if self.enum_filed_seen.insert((t, e)) {
            self.enum_filed.entry(t).or_default().push(e);
        }
    }

    /// A delete or `with` met `t`: its enumerable reads are ordinary reads.
    pub(crate) fn enum_lose(&mut self, t: Tok) {
        if t != TOP && self.enum_lost.insert(t) {
            self.enum_flush(t);
        }
    }

    /// A delete went through ⊤: so on every token ⊤ may denote.
    pub(crate) fn enum_top_lose(&mut self) {
        if std::mem::replace(&mut self.enum_top_lost, true) {
            return;
        }
        if std::env::var_os("PERRY_FACTS_ENUMDEL_DIAG").is_some() {
            eprintln!("perry: wpfacts enumdel top-lost");
        }
        let mut ts: Vec<Tok> = self
            .enum_filed
            .keys()
            .copied()
            .filter(|&t| self.enum_top_reach(t))
            .collect();
        ts.sort_unstable();
        for t in ts {
            self.enum_flush(t);
        }
    }

    /// `t` escaped: after a delete through ⊤ it is one ⊤ denotes.
    pub(crate) fn enum_note_escape(&mut self, t: Tok) {
        if self.enum_top_lost {
            self.enum_flush(t);
        }
    }

    /// The enumerable reads filed on `t` run again as the ordinary read.
    fn enum_flush(&mut self, t: Tok) {
        let Some(reads) = self.enum_filed.remove(&t) else {
            return;
        };
        if std::env::var_os("PERRY_FACTS_ENUMDEL_DIAG").is_some() {
            let why = match self.escaped.get(&t) {
                Some(&r) if !self.enum_lost.contains(&t) => {
                    format!("top-delete+escaped: {}", self.reasons[r as usize])
                }
                _ => "delete".to_string(),
            };
            eprintln!("perry: wpfacts enumdel tok={} reads={} {why}", self.tok_name(t), reads.len());
        }
        for e in reads {
            match e {
                EnumFiled::Idx {
                    origin,
                    idx,
                    dst,
                    shape,
                    kept,
                } => self.idx_load_from(origin, idx, t, dst, shape, true, kept),
                EnumFiled::Chain { of, key, dst } => self.enum_chain_lost(of, t, key, dst),
                EnumFiled::Key { key, dst } => self.load_from(t, t, key, dst),
            }
        }
    }

    /// E7DEL2 (B-del, exact): does the walk of `t` still yield only the
    /// keys the program wrote on it? `t` is a program object whose own
    /// properties are its fields (`enum_obj`, not an array), not escaped
    /// (unknown code may make any own key enumerable, `enum_own`), not
    /// joined with other units' keys, and met no HARD operation but a
    /// delete (a delete stores no key, ECMA-262 10.1.10 OrdinaryDelete;
    /// a definition or `with` may). A delete through ⊤ never denotes it
    /// (`enum_top_reach`: it did not escape).
    pub(crate) fn enum_lost_exact(&self, t: Tok) -> bool {
        t != TOP
            && self.enum_obj(t)
            && !self.is_array_like(t)
            && !self.is_escaped(t)
            && self.joined_of(t).is_none()
            && self
                .hard
                .get(&t)
                .is_none_or(|&(k, _)| k == crate::model::Hard::E7Delete)
    }

    /// E7DEL2 (B-del, exact): enumerable read `e` of `t` once a delete may
    /// have taken a key of `t`. The key `k` the walk gives is one of the
    /// enumerable String keys of `t` and its chain when the walk lists it
    /// (ECMA-262 14.7.5.9 EnumerateObjectProperties; a key added later is
    /// one the program writes on `t` too); a delete may take it before
    /// `o[k]` reads it, and the read is then OrdinaryGet(o, k) (10.1.8.1):
    /// `undefined`, or the member named `k` further up the chain, a
    /// non-enumerable builtin one included. So per key the walk can give:
    /// a named key reads that name on `t` and its chain; the numeric keys
    /// (a Number-keyed store, an index) read the numeric fields and the
    /// chain under a numeric name (`plain_key_from`, which names no
    /// builtin member); a key no name lists (a store under a key of
    /// unknown content, `*`) may be any String: the ordinary read. The
    /// keys of the chain's links are their own walk reads (`EnumChain`).
    /// Keys `t` gains later are read as they appear (`enum_key_gained`,
    /// `enum_index_field`).
    pub(super) fn enum_lost_read(&mut self, t: Tok, e: EnumFiled) {
        if !self.enum_lost_seen.insert((t, e)) {
            return;
        }
        self.enum_lost_reads.entry(t).or_default().push(e);
        let dst = match e {
            EnumFiled::Idx { dst, .. } | EnumFiled::Chain { dst, .. } | EnumFiled::Key { dst, .. } => dst,
        };
        self.add_prim(dst, prim::UNDEF);
        let mut keys: Vec<Atom> = Vec::new();
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            let f = self.fields[&(t, a)];
            if self.enum_own(t, f, a) {
                keys.push(a);
            }
        }
        let num = self.atom(crate::model::NUM_KEY);
        if self.fields.contains_key(&(t, num)) && !keys.contains(&num) {
            keys.push(num);
        }
        for a in keys {
            self.enum_lost_key(t, e, a);
        }
    }

    /// E7DEL2: key `a` of the walk of `t` for lost read `e` (`enum_lost_read`).
    fn enum_lost_key(&mut self, t: Tok, e: EnumFiled, a: Atom) {
        if !self.enum_lost_keys.insert((t, e, a)) {
            return;
        }
        let star = self.atoms[a as usize] == crate::STAR_KEY;
        let num = self.atoms[a as usize] == crate::model::NUM_KEY;
        match e {
            EnumFiled::Idx {
                origin,
                idx,
                dst,
                shape,
                kept,
            } => {
                if star {
                    self.idx_load_from(origin, idx, t, dst, shape, false, kept);
                } else if num {
                    let f = self.num_fields(t);
                    self.edge(f, dst);
                    self.plain_key_from(origin, t, ChainAct::Load(dst, self.thrown));
                } else {
                    self.load_from(origin, t, Key::Named(a), dst);
                }
            }
            EnumFiled::Chain { of, key, dst } => {
                if star {
                    self.load_from(of, t, key, dst);
                } else if num {
                    let f = self.num_fields(t);
                    self.edge(f, dst);
                    self.plain_key_from(of, t, ChainAct::Load(dst, self.thrown));
                } else {
                    self.load_from(of, t, Key::Named(a), dst);
                }
            }
            EnumFiled::Key { key, dst } => self.load_from(t, t, key, dst),
        }
    }

    /// E7DEL2: key `a` (`NUM_KEY`: a numeric one) joined the walk of `t`.
    pub(super) fn enum_lost_gained(&mut self, t: Tok, a: Atom) {
        if let Some(v) = self.enum_lost_reads.get(&t).cloned() {
            for e in v {
                self.enum_lost_key(t, e, a);
            }
        }
    }

    /// E7DEL2: `t` stopped holding only its walk's keys (`enum_lost_exact`:
    /// it escaped, or met a definition or `with`): its lost reads are the
    /// ordinary read.
    pub(crate) fn enum_lost_widen(&mut self, t: Tok) {
        let Some(v) = self.enum_lost_reads.remove(&t) else {
            return;
        };
        for e in v {
            match e {
                EnumFiled::Idx {
                    origin,
                    idx,
                    dst,
                    shape,
                    kept,
                } => self.idx_load_from(origin, idx, t, dst, shape, false, kept),
                EnumFiled::Chain { of, key, dst } => self.load_from(of, t, key, dst),
                EnumFiled::Key { key, dst } => self.load_from(t, t, key, dst),
            }
        }
    }

    /// B-del on chain link `q` of `of` (`Cons::EnumChain`) a delete may have
    /// taken a key from: OrdinaryGet under `key`; under a computed key, per
    /// key of `q`'s walk (`enum_lost_read`) beside its enumerable read.
    pub(super) fn enum_chain_lost(&mut self, of: Tok, q: Tok, key: Key, dst: NodeId) {
        if matches!(key, Key::Any(_)) && self.enum_lost_exact(q) {
            self.enum_lost_read(q, EnumFiled::Chain { of, key, dst });
            self.enum_fields(of, q, key, dst);
            self.accessor_walk(of, q, key, ChainAct::Load(dst, self.thrown));
            let up = self.proto_node(q);
            self.cons(
                up,
                Cons::EnumChain {
                    key,
                    dst,
                    of,
                    from: q,
                },
            );
            return;
        }
        self.load_from(of, q, key, dst);
    }

    /// `enum_read_key` once `enum_kept(t)` holds; `idx` keys its one-time
    /// chain walk.
    fn enum_read_key_kept(&mut self, idx: NodeId, t: Tok, key: Key, dst: NodeId) {
        if self.enum_obj(t) {
            self.mark_use(t, crate::model::MARK_READ);
        }
        self.add_prim(dst, prim::UNDEF);
        self.enum_fields(t, t, key, dst);
        if self
            .idx_done
            .insert((idx, t, dst, crate::model::IDX_ENUM_READ))
        {
            self.accessor_walk(t, t, key, ChainAct::Load(dst, self.thrown));
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
                    if t == TOP {
                        "TOP".to_string()
                    } else {
                        self.tok_name(t)
                    }
                );
            }
            if !covered {
                self.enum_key_any(o, src, site);
                continue;
            }
            if !self.enum_key_done.insert((n, t, o, src)) {
                continue;
            }
            self.enum_key_stores
                .entry(t)
                .or_default()
                .push((o, src, site));
            let star = self.atom(crate::STAR_KEY);
            if self
                .accessor_keys
                .get(&t)
                .is_some_and(|ks| ks.contains(&star))
            {
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
            && (self.enum_free(t)
                || self.enum_obj(t)
                || builtins::enum_free_instance(self, t)
                || builtins::enum_free_buffer(self, t))
            && !self.is_elem_array(t)
            && !self
                .tmpl_of(t)
                .is_some_and(|tm| matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    }

    /// ENUM3: does what `enum_key_watch(t)` collects hold a value now: was
    /// `t` written under a key no name lists (its any-key field, a shaped
    /// computed-key store)? Read without making the watch node.
    pub(crate) fn enum_any_written(&self, t: Tok) -> bool {
        // The numeric range (`numeric_field`): a Number or BigInt key's
        // store names a numeric string no name lists (7.1.19).
        let numeric = self
            .atom_index
            .get(crate::model::NUM_KEY)
            .and_then(|&a| self.fields.get(&(t, a)));
        self.any_key_written(t) || numeric.is_some_and(|&n| self.node_held(n))
    }

    /// Was `t` written under a String or Symbol key no name lists: its
    /// any-key field or a shaped computed-key store (`enum_any_written`
    /// without the numeric range)?
    pub(crate) fn any_key_written(&self, t: Tok) -> bool {
        let any = match self.is_elem_array(t) {
            true => self.named_any.get(&t),
            false => self.field_any.get(&t),
        };
        any.is_some_and(|&n| self.node_held(n))
            || self
                .shaped_of
                .get(&t)
                .is_some_and(|v| v.iter().any(|&(_, n, store)| store && self.node_held(n)))
    }

    pub(crate) fn node_held(&self, n: NodeId) -> bool {
        let nd = &self.nodes[n as usize];
        !nd.pts.is_empty() || nd.prim != 0
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
    pub(super) fn enum_key_watch(&mut self, t: Tok) -> NodeId {
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
        if self.enum_lost_reads.contains_key(&t) {
            let num = self.atom(crate::model::NUM_KEY);
            self.enum_lost_gained(t, num);
        }
    }

    /// ENUM3: key `a` appeared on token `t`: the keyed stores `t`'s
    /// enumerated keys decide store under it too.
    pub(super) fn enum_key_gained(&mut self, t: Tok, a: Atom) {
        self.enum_lost_gained(t, a);
        if let Some(v) = self.enum_key_stores.get(&t).cloned() {
            for (o, src, site) in v {
                self.enum_key_named(o, src, site, a);
            }
        }
        // ENUM-DEL: a for-in delete of the global object deletes it too.
        if let Some(v) = self.enum_key_deletes.get(&t).cloned() {
            for reason in v {
                self.delete_enum_named(t, a, reason);
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
                region: self.thrown,
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
            self.cons(
                o,
                Cons::Store {
                    key,
                    src,
                    site,
                    region: self.thrown,
                },
            );
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
            return self.idx_load_tok(idx, t, dst, shape, enumerable, false);
        }
        self.mark_use(t, crate::model::MARK_READ);
        self.add_prim(dst, prim::UNDEF);
        if let Some(&a) = self.key_nodes.get(&idx) {
            let f = self.field_r(t, a);
            self.edge(f, dst);
            self.accessor_walk(t, t, Key::Named(a), ChainAct::Load(dst, self.thrown));
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
            if self.shape_may_be_numeric(shape) {
                self.plain_key(obj, ChainAct::Load(dst, self.thrown));
            }
            self.load_from(obj, obj, Key::Any(shape), dst);
        }
        let key = Key::Any(shape);
        if index_needs_general_key(Tokens::Slice(&[]), prim)
            && self.idx_done.insert((n, obj, dst, IDX_OWN_STR))
        {
            let f = self.key_fields(obj, key);
            self.edge(f, dst);
            self.accessor_walk(obj, obj, key, ChainAct::Load(dst, self.thrown));
        }
        // ToPropertyKey of a Number or BigInt is a numeric string: the own
        // numeric fields, and an accessor under a numeric name.
        if index_has_numeric_key(prim) && self.idx_done.insert((n, obj, dst, IDX_OWN_NUM)) {
            let f = self.num_fields(obj);
            self.edge(f, dst);
            if self.numeric_accessor {
                self.accessor_walk(obj, obj, Key::any(), ChainAct::Load(dst, self.thrown));
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
                    self.accessor_walk(obj, obj, Key::Named(a), ChainAct::Load(dst, self.thrown));
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
                    kept: false,
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
            t.map(|t| if t == TOP {
                "TOP".to_string()
            } else {
                self.tok_name(t)
            })
            .unwrap_or_else(|| "prim".into())
        );
    }
}
