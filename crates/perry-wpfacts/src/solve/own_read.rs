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
    /// `Cons::IdxLoad` for one object token `t`.
    pub(super) fn idx_load_tok(&mut self, idx: NodeId, t: Tok, dst: NodeId, shape: KeyShapeId) {
        // A for-in run's key (FVA 4.3b): `o[k]` reads that one key.
        if let Some(&a) = self.key_nodes.get(&idx) {
            self.load_from(t, t, Key::Named(a), dst);
            return;
        }
        if t == TOP {
            self.refute_own_test(dst);
            return;
        }
        // An object's own fields: by the key's values (`IdxKey`: a number
        // key names only its numeric fields). A marker's fields are its
        // reach markers under any key; another value (the global object, a
        // namespace) keeps every field.
        if self.is_marker(t) || !matches!(self.toks[t as usize], TokKind::Obj(..)) {
            let all = self.all_fields(t);
            self.edge(all, dst);
        }
        self.add_prim(dst, prim::UNDEF);
        if self.idx_done.insert((idx, t, dst, 0)) {
            self.plain_key(t, ChainAct::Load(dst));
            self.cons(idx, Cons::IdxKey { obj: t, dst, shape });
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
    pub(super) fn idx_own_load_tok(&mut self, idx: NodeId, t: Tok, dst: NodeId, shape: KeyShapeId) {
        if !self.own_read_obj(t) {
            return self.idx_load_tok(idx, t, dst, shape);
        }
        self.mark_use(t, crate::model::MARK_READ);
        self.add_prim(dst, prim::UNDEF);
        if let Some(&a) = self.key_nodes.get(&idx) {
            let f = self.field(t, a);
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
        if prim & (prim::STR | prim::SYMBOL) != 0
            && self.idx_done.insert((n, obj, dst, IDX_OWN_STR))
        {
            let f = self.key_fields(obj, key);
            self.edge(f, dst);
            self.accessor_walk(obj, obj, key, ChainAct::Load(dst));
        }
        // ToPropertyKey of a Number or BigInt is a numeric string: the own
        // numeric fields, and an accessor under a numeric name.
        if prim & (prim::NUM | prim::BIGINT) != 0
            && self.idx_done.insert((n, obj, dst, IDX_OWN_NUM))
        {
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
            if prim & bit != 0 && self.idx_done.insert((n, obj, dst, IDX_OWN_PRIM + i as u8)) {
                for k in keys {
                    let a = self.atom(k);
                    let f = self.field(obj, a);
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
            let f = self.field(t, a);
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
                    } => self.own_gate(obj, idx, dst, shape),
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
            } => self.own_gate(obj, idx, dst, shape),
            _ => {}
        }
    }

    /// Refuted evidence restores the ordinary read, once.
    fn own_gate(&mut self, obj: NodeId, idx: NodeId, dst: NodeId, shape: KeyShapeId) {
        if self.idx_done.insert((obj, TOP, dst, IDX_OWN_GATE)) {
            self.own_read_census[1] += 1;
            self.cons(obj, Cons::IdxLoad { idx, dst, shape });
        }
    }
}
