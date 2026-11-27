//! Rule 3 prototype-first reads, realm accessors a lookup is sure of, and
//! the inert keys a class construction reads present.

use crate::builtins;
use crate::model::*;

impl Model {
    /// Rule 3 (`gen::proto_first`): object `t` is an instance of a program
    /// class whose chain (ECMA-262 10.1.8.1 OrdinaryGet), followed from the
    /// class up, reaches a prototype that a rule 3 store statement gives `a`
    /// (`proto_first_keys`) and that no other writer of `a` stores into. Every link is
    /// the static one (no `setPrototypeOf`, `delete`, `defineProperty` on
    /// it: `hard`; not reachable by unknown code: `escaped`, `inst_escaped`;
    /// its prototype node holds only the parent's prototype), no link
    /// defines an accessor under `a` and nothing redefined `a` through ⊤ or
    /// a builtin prototype (`reshaped_keys`). Links below the holder may
    /// hold `a` too: their values join through the chain.
    pub(crate) fn proto_first_holds(&self, t: Tok, a: Atom) -> bool {
        if t == TOP
            || self.is_marker(t)
            || self.is_proto_key(Key::Named(a))
            || self.is_private_atom(a)
        {
            return false;
        }
        // R9READ4: a read on the prototype object itself (`X.prototype.m`)
        // of a class-body method `m` of `X`: ClassDefinitionEvaluation
        // (ECMA-262 15.7.14) defines `m` on it as an own writable data
        // property (10.2.8 DefineMethodProperty) before `X.prototype` can be
        // reached (the class binding is in its TDZ until then, 9.1.1.1.6),
        // so OrdinaryGet (10.1.8.1) finds it there; only `delete` /
        // `defineProperty` (HARD) or unknown code (escaped) remove or
        // reshape it, and a stored `undefined` ends the rule as below.
        if let TokKind::Proto(c) = self.toks[t as usize] {
            let tp = &self.tmpls[c as usize];
            let undef = |n: Option<&NodeId>| {
                n.is_some_and(|&n| self.nodes[n as usize].prim & prim::UNDEF != 0)
            };
            return !self.reshaped_any
                && !self.reshaped_keys.contains(&a)
                && !tp.per_evaluation
                && matches!(
                    tp.kind,
                    TmplKind::Class {
                        literal_shape: false,
                        ..
                    }
                )
                && !self.inst_escaped.contains(&c)
                && !tp.accessor_keys.contains(&a)
                && tp.proto_members.contains(&a)
                && !self.hard.contains_key(&t)
                && !self.escaped.contains_key(&t)
                && !self.accessor_keys.get(&t).is_some_and(|ks| ks.contains(&a))
                && !undef(self.fields.get(&(t, a)))
                && !undef(self.field_any.get(&t));
        }
        let TokKind::Obj(tm0, _) = self.toks[t as usize] else {
            return false;
        };
        if self.reshaped_any
            || self.reshaped_keys.contains(&a)
            || self.hard.contains_key(&t)
            || self.escaped.contains_key(&t)
        {
            return false;
        }
        let only = |m: &Self, o: Tok, p: Tok| {
            m.proto_node
                .get(&o)
                .is_none_or(|&n| m.nodes[n as usize].pts.to_sorted().iter().all(|&q| q == p))
        };
        let (mut c, mut below) = (tm0, t);
        loop {
            let tp = &self.tmpls[c as usize];
            if tp.per_evaluation
                || !matches!(
                    tp.kind,
                    TmplKind::Class {
                        literal_shape: false,
                        ..
                    }
                )
                || self.inst_escaped.contains(&c)
                || tp.accessor_keys.contains(&a)
            {
                return false;
            }
            let Some(&p) = self.tok_index.get(&TokKind::Proto(c)) else {
                return false;
            };
            // A link's field holding `undefined` (a stored one) would be
            // stripped with the absences: the rule ends.
            let stored_undef = self
                .fields
                .get(&(p, a))
                .is_some_and(|&n| self.nodes[n as usize].prim & prim::UNDEF != 0);
            if !only(self, below, p)
                || stored_undef
                || self.hard.contains_key(&p)
                || self.escaped.contains_key(&p)
                || self.accessor_keys.get(&p).is_some_and(|ks| ks.contains(&a))
            {
                return false;
            }
            // R9READ1: a class-body method (a non-accessor `proto_members`
            // key) is defined on the prototype in ClassDefinitionEvaluation
            // (ECMA-262 15.7.14) before any instance exists, as a writable
            // data property; only `delete` / `defineProperty` (HARD, above)
            // remove or reshape it, and a store keeps it a data property
            // whose values join the field (a stored `undefined`, named or
            // computed, ends the rule above).
            if tp.proto_members.contains(&a)
                && !self
                    .field_any
                    .get(&p)
                    .is_some_and(|&n| self.nodes[n as usize].prim & prim::UNDEF != 0)
            {
                return true;
            }
            // The store statement runs when the class is evaluated, before
            // any instance exists; every other writer the solve finds
            // (later: `settle_proto_first`) ends the rule.
            if self.proto_first_keys.contains(&(c, a)) {
                return self.proto_writers.get(&(c, a)).is_none_or(|w| {
                    w.ctor.is_empty()
                        && w.post
                            .iter()
                            .all(|&s| self.stores[s as usize].proto_first == Some(c))
                });
            }
            match (tp.parent, &tp.native_parent) {
                (Some(q), None) => (c, below) = (q, p),
                _ => return false,
            }
        }
    }

    /// OrdinarySet (ECMA-262 10.1.9.2) of `a` on `t` finds a realm accessor:
    /// `t` is an instance of program classes whose static chain (each link
    /// the class's prototype only: no `setPrototypeOf`, `delete`,
    /// `defineProperty` (`hard`), no unknown code (`escaped`,
    /// `inst_escaped`)) holds no `a` of its own (no own definition of `t`,
    /// `realm_set_open`; no method, accessor or stored field on a link) and
    /// ends at realm prototypes that each own an accessor under `a`, which
    /// no native write replaced. The store's own data definition then never
    /// runs; the setter (or, getter-only, nothing) does.
    pub(crate) fn realm_accessor_sure(&self, t: Tok, a: Atom) -> bool {
        if self.unit_mode
            || t == TOP
            || self.is_marker(t)
            || self.is_proto_key(Key::Named(a))
            || self.is_private_atom(a)
            || self.reshaped_any
            || self.reshaped_keys.contains(&a)
            || self.realm_set_open.contains(&(t, a))
        {
            return false;
        }
        let TokKind::Obj(tm0, _) = self.toks[t as usize] else {
            return false;
        };
        let clean = |m: &Self, o: Tok| {
            !m.hard.contains_key(&o)
                && !m.escaped.contains_key(&o)
                && !m.accessor_keys.get(&o).is_some_and(|ks| ks.contains(&a))
        };
        // A link's field node exists once a lookup asked for it; it holds a
        // stored value only after a store (which a later check sees).
        let held = |n: Option<&NodeId>| {
            n.is_some_and(|&n| {
                let n = &self.nodes[n as usize];
                n.prim != 0 || !n.pts.is_empty()
            })
        };
        if !clean(self, t) {
            return false;
        }
        let (mut c, mut below) = (tm0, t);
        loop {
            let tp = &self.tmpls[c as usize];
            if tp.per_evaluation
                || !matches!(
                    tp.kind,
                    TmplKind::Class {
                        literal_shape: false,
                        ..
                    }
                )
                || self.inst_escaped.contains(&c)
                || tp.accessor_keys.contains(&a)
                || tp.proto_members.contains(&a)
            {
                return false;
            }
            let Some(&p) = self.tok_index.get(&TokKind::Proto(c)) else {
                return false;
            };
            let only = self.proto_node.get(&below).is_none_or(|&n| {
                self.nodes[n as usize]
                    .pts
                    .to_sorted()
                    .iter()
                    .all(|&q| q == p)
            });
            if !only
                || !clean(self, p)
                || held(self.fields.get(&(p, a)))
                || held(self.field_any.get(&p))
            {
                return false;
            }
            match (tp.parent, &tp.native_parent) {
                (Some(q), None) => (c, below) = (q, p),
                // The linked realm parent (`Model::realm_parent`): the
                // evaluated `extends` is that constructor (else E11 marks
                // the instances hard), so the link's prototype is its
                // declared prototype; what the solve gave the link so far
                // agrees (a later value re-checks: `settle_realm_set`).
                (None, _) if self.realm_parent.contains_key(&c) => {
                    let name = &self.atoms[a as usize];
                    if builtins::realm_ctor_prototype(self.realm_parent[&c])
                        .and_then(|born| builtins::realm_born_setter(born, name))
                        .is_none()
                    {
                        return false;
                    }
                    let rs = self
                        .proto_node
                        .get(&p)
                        .map(|&n| self.nodes[n as usize].pts.to_sorted())
                        .unwrap_or_default();
                    return rs.iter().all(|&r| {
                        r != TOP
                            && builtins::realm_proto_setter(self, r, a).is_some()
                            && !builtins::native_written(self, r, a)
                    });
                }
                _ => return false,
            }
        }
    }

    /// Once the worklist is empty: the rule 3 reads whose chain no longer
    /// holds read `undefined`. True when that made new work.
    pub(crate) fn settle_proto_first(&mut self) -> bool {
        if self.proto_first_reads.is_empty() {
            return false;
        }
        let reads = std::mem::take(&mut self.proto_first_reads);
        let mut kept = Vec::with_capacity(reads.len());
        for (t, a, dst) in reads {
            if self.proto_first_holds(t, a) {
                kept.push((t, a, dst));
            } else {
                self.add_prim(dst, prim::UNDEF);
            }
        }
        self.proto_first_reads = kept;
        !self.wl.is_empty()
    }

    /// R9READ2 rule I: `a` is present on `tm`'s objects from birth on: the
    /// construction phase stores it before `this` is observed once its
    /// reads of keys not yet own run no code, i.e. no accessor may be
    /// defined under any of them (`needs_accessor_walk`: the test every
    /// lookup takes for whether it can run a getter). A later accessor
    /// under one of them flushes the reads (`add_accessor_atom`).
    pub(crate) fn inert_sure(&self, tm: TmplId, a: Atom) -> bool {
        let t = &self.tmpls[tm as usize];
        t.inert_definite.contains(&a)
            && t.inert_reads
                .iter()
                .all(|&k| !self.needs_accessor_walk(Key::Named(k)))
    }

    /// Rule I: the birth value `v` of reservation `a` of class `tm` holds
    /// `undefined` unless `inert_sure`; filed for a later accessor key.
    pub(crate) fn inert_undef(&mut self, tm: TmplId, a: Atom, v: NodeId) {
        if self.inert_sure(tm, a) {
            self.inert_births.push((tm, a, v));
        } else {
            self.add_prim(v, prim::UNDEF);
        }
    }

    /// The reads of `tm` that found a key no longer sure read `undefined`.
    pub(crate) fn flush_present_reads(&mut self, tm: TmplId) {
        let reads = self.present_reads.remove(&tm).unwrap_or_default();
        let mut kept = Vec::new();
        for (a, dst) in reads {
            let sure = !self.keys_unsure.contains(&tm)
                && (self.tmpls[tm as usize].definite.contains(&a)
                    || self.spread_sure(tm, a)
                    || self.inert_sure(tm, a));
            if sure {
                kept.push((a, dst));
            } else {
                self.add_prim(dst, prim::UNDEF);
                let open = !crate::builtins::object_final_read(&self.atoms[a as usize]);
                let tpl = &mut self.tmpls[tm as usize];
                tpl.chain_miss = true;
                tpl.chain_miss_open |= open;
            }
        }
        if !kept.is_empty() {
            self.present_reads.insert(tm, kept);
        }
        // A key of `tm` no longer sure no longer shadows on its objects nor
        // on the receivers of class `tm` (`keys_unsure`, `spread_sure`).
        self.flush_shadowed_classes(&[tm]);
    }
}
