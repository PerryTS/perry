//! Prototype value lookup, holder traversal and shared accessor/callee flow.

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

    /// A read that rule 3 answers: filed, so that a later writer, escape or
    /// re-link gives it `undefined` after all (`settle_proto_first`).
    fn proto_first_read(&mut self, t: Tok, a: Atom, dst: NodeId) -> bool {
        if !self.proto_first_holds(t, a) {
            return false;
        }
        if self.proto_first_seen.insert((t, a, dst)) {
            self.proto_first_reads.push((t, a, dst));
        }
        true
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

    /// EvaluateCall's GetV then Call (ECMA-262 13.3.6.1, 7.3.20) of the
    /// method call `call` on object `t` under `key`: it calls what [[Get]]
    /// of the key on `t` reads, with `t` as `this`.
    pub(crate) fn call_read_member(&mut self, t: Tok, key: Key, call: CallId) {
        let callee = self.node_l("builtin-function-read");
        self.load_from(t, t, key, callee);
        self.bind_via(callee, call, t);
    }

    /// dst ⊇ value of `key` read on object `t` (receiver `origin`).
    pub fn load_from(&mut self, origin: Tok, t: Tok, key: Key, dst: NodeId) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::LoadFrom(
            origin, t, key, dst,
        )) {
            return;
        }
        self.mark_use(t, crate::model::MARK_READ);
        // A class private field on an object this unit does not own: what
        // the class's code stored (`private_field`).
        if let Key::Named(a) = key {
            if self.is_private_atom(a) && !self.owns_private(t) {
                let p = self.private_field(a);
                self.edge(p, dst);
                return;
            }
        }
        if self.is_yield_key(key) {
            let builtin_result = self.tmpl_of(t).is_some_and(|tm| {
                matches!(
                    self.tmpls[tm as usize].kind,
                    TmplKind::BObj {
                        what: "IterResult",
                        ..
                    }
                )
            });
            if !builtin_result {
                let va = self.atom("value");
                return self.load_from(origin, t, Key::Named(va), dst);
            }
            let Key::Named(ya) = key else { unreachable!() };
            let y = self.field_r(t, ya);
            self.edge(y, dst);
            return;
        }
        if self.is_iter_key(key) {
            match self.toks[t as usize] {
                TokKind::Top | TokKind::Builtin(_) | TokKind::NativeProto(..) => {}
                TokKind::Obj(_, _) if self.is_array_like(t) => {}
                // A program object/function iterates only through its
                // protocol, which `Gen::iterate` follows separately. So does
                // a builtin function: it has no own `@@iterator` and no list
                // of entries, nor has `%Function.prototype%` (ECMA-262
                // 20.2.3, 20.2.4); an `@@iterator` the program installs on
                // its chain is that protocol call.
                _ => return,
            }
            // An Array's iteration reads each index below its length: an
            // element, or `undefined` at a hole (`hole`); never past the end.
            if t != TOP && self.is_arr(t) {
                let all = self.all_fields(t);
                self.edge(all, dst);
                let h = self.hole(t);
                self.edge(h, dst);
                self.index_accessors(origin, t, ChainAct::Load(dst, self.thrown));
                return;
            }
            // A Map's iteration yields a fresh `[key, value]` array per entry
            // (ECMA-262 24.1.3.12 Map.prototype[@@iterator] is entries,
            // 24.1.5.1 CreateArrayFromList), never a key or a value itself:
            // the pairs come from its `@@iterator` call, which the iteration
            // makes beside this read (`builtins::iterate`, `Gen::iterate`). A
            // WeakMap is not iterable (24.3.3: no @@iterator).
            if self.keyed_collection(t) {
                return;
            }
            // A builtin object other than an array the runtime made walks
            // its own internal entries (a Set's list, an iterator's steps, a
            // string's code points): no [[Get]] of an index, so no index
            // accessor runs.
            if self.iterates_internal(t) {
                let all = self.all_fields(t);
                self.edge(all, dst);
                self.add_prim(dst, prim::UNDEF);
                return;
            }
            let ek = self.elem_key();
            return self.load_from(origin, t, ek, dst);
        }
        if self.is_elem_key(key) {
            // An element read is [[Get]] of an index: an accessor under a
            // numeric name runs (an HIR array operation's read, as the
            // native's in `native_index_access`).
            if t != TOP && self.is_array_like(t) {
                let all = self.all_fields(t);
                self.edge(all, dst);
                self.add_prim(dst, prim::UNDEF);
                self.index_accessors(origin, t, ChainAct::Load(dst, self.thrown));
                return;
            }
            if builtins::builtin_function(self, t) {
                self.builtin_field_use(t, key, ChainAct::Load(dst, self.thrown));
                self.add_prim(dst, prim::UNDEF);
                self.accessor_walk(t, t, Key::any(), ChainAct::Load(dst, self.thrown));
                return;
            }
            // An element of ⊤ is ⊤. Its key is an index, never
            // `constructor`: no Function constructor (DECISIONS 235).
            if t == TOP {
                self.add_tok(dst, TOP);
                return;
            }
            // ToPropertyKey of an index is a canonical numeric String
            // (ECMA-262 7.1.19, 10.1.8.1 [[Get]]): an ordinary object's
            // number-named own fields (`num_fields`: numeric keys, the
            // element key, what a computed store wrote) and, along its
            // chain, only number-named members and accessors (`plain_key`),
            // never an identifier-named method. A marker's fields are its
            // reach markers, and another value (the global object, a
            // namespace, a function) keeps the computed read.
            if origin == t
                && matches!(self.toks[t as usize], TokKind::Obj(..))
                && !self.is_marker(t)
            {
                self.builtin_field_use(t, key, ChainAct::Load(dst, self.thrown));
                let f = self.num_fields(t);
                self.edge(f, dst);
                self.add_prim(dst, prim::UNDEF);
                self.plain_key(t, ChainAct::Load(dst, self.thrown));
                return;
            }
            return self.load_from(origin, t, Key::any(), dst);
        }
        if t == TOP {
            self.add_tok(dst, TOP);
            // Any function's `.constructor` is the Function constructor (E14).
            if self.is_constructor_key(key) {
                let f = self.tok(TokKind::Builtin(BuiltinNs::Function));
                self.add_tok(dst, f);
            }
            return;
        }
        self.builtin_field_use(t, key, ChainAct::Load(dst, self.thrown));
        if self.is_array_length(t, key) {
            self.arr_len_into(t, dst);
            return;
        }
        // Rule C1 (`ta_length_getter`): the getter's Number, and no own field
        // until one may exist (`ta_len_open`).
        if matches!(key, Key::Named(a) if self.atoms[a as usize] == "length")
            && self.ta_length_getter(t)
        {
            self.add_prim(dst, prim::NUM);
            let g = self.ta_len_gate(t);
            self.edge(g, dst);
            return;
        }
        // An absent key reads `undefined`: only a definite birth key of a
        // fact unit is known to be present. A closed marker holds no value
        // of its own: every value its parameter can hold is beside it, and
        // each of those reads its own absence (CHB3), so the marker adds
        // none (as `judge` skips it). Only a parameter marker's own
        // closedness (`closed_param`): it settles nothing and never turns
        // off, while an argument marker's depends on members that may still
        // join after this load (CHB5).
        self.note_chain_miss(t, key);
        let closed = self.closed_param(t);
        let present = closed
            || match (key, self.tmpl_of(t)) {
                (Key::Named(a), Some(tm)) => {
                    let sure = (self.tmpls[tm as usize].kind.is_fact_unit()
                        || self.realm_born.contains(&tm))
                        && !self.keys_unsure.contains(&tm)
                        && (self.tmpls[tm as usize].definite.contains(&a)
                            || self.spread_sure(tm, a)
                            || self.inert_sure(tm, a));
                    if sure && self.present_seen.insert((tm, a, dst)) {
                        self.present_reads.entry(tm).or_default().push((a, dst));
                    }
                    sure
                }
                _ => false,
            };
        // A class private field (`Model::private_atom`) is never read
        // absent: its class's guard throws on an object without it first
        // (ECMA-262 PrivateGet).
        let private = matches!(key, Key::Named(a) if self.is_private_atom(a));
        // REALM-PRESENT (`builtins::declared_present`): a declared realm
        // global or a builtin namespace's own native member is present; a
        // write or delete that can reach it gives the read every primitive
        // (`undefined` too) where `tok_load` answers it. Not `present`: what
        // reaches the read through the chain keeps its `undefined`.
        let realm = matches!(key, Key::Named(a) if builtins::declared_present(self, t, a));
        // ECMA-262 10.1.8.1 OrdinaryGet: a sure own data property is the
        // read's value; the lookup ends at the object.
        let stop = match key {
            Key::Named(a) => self.surely_own_data(t, a),
            Key::Any(_) => false,
        };
        // Rule 3 (`proto_first_holds`): the chain holds the key from every
        // receiver's birth on; what the chain holds (a stored `undefined`
        // too) reaches the read as it is.
        let first = !present
            && !closed
            && match key {
                Key::Named(a) => self.proto_first_read(t, a, dst),
                Key::Any(_) => false,
            };
        // R9BIRTH4: a class constructor's own `prototype` (`surely_own_data`)
        // is present for life: the read is the field's value alone.
        let ctor_proto = stop && matches!(self.toks[t as usize], TokKind::ClassObj(_));
        // A final chain read whose intrinsic chain defines the key
        // (`builtins::locked_chain_defines`: the locked member can be
        // neither replaced nor deleted) finds that member past every own
        // key; what an own key holds reaches the read through its field.
        let chain_defined =
            matches!(key, Key::Named(a) if builtins::locked_chain_defines(self, t, a));
        if !present && !private && !realm && !first && !ctor_proto && !chain_defined {
            self.add_prim(dst, prim::UNDEF);
        }
        // What a closed marker's own load gives (its classes' chain through
        // `virt_load`, its keyed reach) carries no `undefined` either: the
        // values beside it read their own (CHB3).
        let own = |m: &mut Self, src: NodeId| match closed {
            true => m.defined_from(src, dst),
            false => m.edge(src, dst),
        };
        match key {
            Key::Named(a) if stop => {
                let f = self.field_r(t, a);
                own(self, f);
                self.note_own_stopped(origin, t, a, ChainAct::Load(dst, self.thrown));
                return;
            }
            // A method key on a published receiver finds the class chain's
            // value, not the receiver's unknown own field (DECISIONS 59).
            Key::Named(a) => match self.virt_load(t, a) {
                Some(v) => {
                    own(self, v);
                    // A key another unit's subclass defines: a read finds an
                    // unknown own value (only a call through the receiver
                    // is that subclass's method, `virt_call`, DECISIONS 97).
                    if !self.virt_overriders(t, a).is_empty() {
                        let f = self.field_r(t, a);
                        own(self, f);
                    }
                }
                None => {
                    let f = self.field_r(t, a);
                    own(self, f);
                }
            },
            Key::Any(_) => {
                let all = self.key_fields(t, key);
                own(self, all);
            }
        }
        // A private field is the object's own: its read consults no
        // prototype and no accessor (ECMA-262 PrivateGet).
        if private {
            return;
        }
        // A marker is use tracking, not a value (DECISIONS 111): a load from
        // it reads its fields only. A parameter marker has no prototype here
        // (DECISIONS 119): what
        // its argument inherits is the reader's, which replays the load on
        // it (the keyed reach above); values beside a marker walk their own
        // chains. A receiver marker's chain is its classes' (`virt_protos`).
        if self.is_marker(t) && !self.virt_protos.contains_key(&t) {
            return;
        }
        // DECISIONS 217: `__proto__` is Object.prototype's accessor, found on
        // every chain: it reads the receiver's prototype, as a value.
        if self.is_proto_key(key) {
            self.proto_value(t, dst);
        }
        // DECISIONS 124: a receiver marker with values beside it stands for
        // instances of its classes, whose builtin members are the chain's
        // (`cv` below reaches Object.prototype): no builtin object's.
        // An own data key the object surely holds stops the lookup at the
        // object: the chain's end, where a key no link defines reads
        // `undefined`, is never reached. Its values stay (a superset).
        let defined = present && !self.is_proto_key(key);
        // Rule 3: every link holds the key or passes the lookup on to the
        // holder, which holds it: no link's absence and no chain end gives
        // `undefined` (a stored `undefined` ends the rule:
        // `proto_first_holds`).
        let chain = if defined || first {
            self.defined_into(dst)
        } else {
            dst
        };
        // DECISIONS 234: a key every object `t` denotes holds as an own data
        // property never reaches the builtin prototypes, nor the builtin
        // members of `t`'s own kind.
        let shadow_key = self.shadow_key(t, key);
        let shadow = shadow_key.is_some();
        let own_members = !(self.is_marker(t) && self.beside_values(t));
        if own_members && !shadow {
            if self.json_key_optimistic(t, key) {
                // OPTIMISTIC (`json_key_optimistic`): no built-in member,
                // only what a store into ⊤ may have written on the chain.
                self.json_key_fired.push((false, dst));
                self.with_b(|m| builtins::chain_written_only(m, t, chain));
            } else {
                self.with_b(|m| builtins::tok_load(m, t, key, chain));
            }
        }
        if let Some(a) = shadow_key {
            self.note_shadowed(t, a, ChainAct::Load(dst, self.thrown), own_members);
        }
        if matches!(self.toks[t as usize], TokKind::ObjectProto) && !self.is_proto_key(key) {
            self.with_b(|m| builtins::intrinsic_proto_own(m, key, dst));
        }
        // Element reads of array-likes stop at their own elements: their
        // builtin prototype members are approximated by `tok_load`. An
        // `arguments` object's chain is an ordinary one (`Object.prototype`).
        // Its own properties are ordinary ones, though: an accessor the
        // program defines on it after its birth (`defineProperty` on a
        // `JSON.parse` record) runs with the receiver as `this`, now or
        // when it is defined later in the solve (as in `own_values_of`).
        if key == Key::any() && self.is_array_like(t) && !self.is_args(t) {
            let act = ChainAct::Load(dst, self.thrown);
            self.any_walk_holders
                .entry(t)
                .or_default()
                .push((origin, Key::any(), act));
            self.accessor_hit(origin, t, Key::any(), act);
            // An ordinary object a builtin made finds `Object.prototype`'s
            // members in `tok_load`; a prototype the program gave it
            // (`Object.setPrototypeOf`) is on its chain: its values.
            if crate::builtins::ordinary_box(self, t) || crate::builtins::array_chain(self, t) {
                let cv = self.cv_recv(origin, t, key, false);
                self.edge(cv, dst);
            }
            return;
        }
        // ECMA-262 10.1.8.1 OrdinaryGet: an own property ends the lookup.
        // A namespace object's own static (`Promise.all`) never reads its
        // chain: what a store wrote onto `Function.prototype` or
        // `Object.prototype` under that key, a data value or an accessor,
        // is not its value. A store or definition that may replace or
        // delete the static is the namespace's own written key (`tok_load`
        // reads the opaque member, or replays the read). So is a declared
        // realm global (`realm`, REALM-PRESENT): the global object's own
        // property, which `tok_load` read with every write that can reach it.
        if let Key::Named(a) = key {
            if builtins::namespace_owns(self, t, a) || realm {
                return;
            }
        }
        // The prototype chain's values, shared by every receiver of `t`.
        let cv = self.cv_recv(origin, t, key, !shadow);
        if defined || first {
            // A rule on the chain node, not an edge into a relay: an
            // observation node feeds only observation nodes (`edge`), and
            // `sum_skip` applies that to this rule by its `dst`.
            self.defined_from(cv, dst);
        } else {
            self.edge(cv, dst);
        }
        // Accessors run with the receiver as `this`: walk per receiver, only
        // for keys an accessor may be defined under.
        self.accessor_walk(origin, t, key, ChainAct::Load(dst, self.thrown));
    }

    fn numeric_accessor_key(&self, a: Atom) -> bool {
        self.atoms[a as usize] == crate::STAR_KEY || self.atoms[a as usize].parse::<f64>().is_ok()
    }

    /// A key becomes an accessor key: generation names an accessor under it,
    /// or the solve installs one. The first time, the reads and writes that
    /// arrived before it start the walks they deferred, whichever side named
    /// the key first: in the activation pass generation and the solve
    /// interleave, so a read of an inherited literal getter (`Object.create`,
    /// `setPrototypeOf`) can be solved before generation walks the literal.
    pub(crate) fn add_accessor_atom(&mut self, a: Atom) {
        if !self.accessor_atoms.insert(a) {
            return;
        }
        // Rule I: a class whose construction read `a` (or any key: STAR)
        // through the chain no longer knows its inert keys present.
        let star = self.atoms[a as usize] == crate::STAR_KEY;
        let hit: Vec<TmplId> = (0..self.tmpls.len() as TmplId)
            .filter(|&tm| {
                let r = &self.tmpls[tm as usize].inert_reads;
                !r.is_empty() && (star || r.contains(&a))
            })
            .collect();
        for &tm in &hit {
            self.flush_present_reads(tm);
        }
        if !hit.is_empty() {
            let births = std::mem::take(&mut self.inert_births);
            for (tm, b, v) in births {
                if self.inert_sure(tm, b) {
                    self.inert_births.push((tm, b, v));
                } else {
                    self.add_prim(v, prim::UNDEF);
                }
            }
        }
        self.numeric_index_walks(a);
        self.deferred_walks(a);
    }

    /// Numeric reads registered before the first numeric accessor start
    /// their deferred walks: installing a descriptor does not activate its
    /// body on their behalf.
    fn numeric_index_walks(&mut self, a: Atom) {
        if self.numeric_accessor || !self.numeric_accessor_key(a) {
            return;
        }
        self.numeric_accessor = true;
        let num = self.atom(NUM_KEY);
        let mut pending: Vec<_> = self
            .visits
            .iter()
            .flat_map(|(&t, visits)| {
                visits
                    .iter()
                    .copied()
                    .filter_map(move |(origin, key, act)| {
                        (key == Key::Named(num)).then_some((origin, t, act))
                    })
            })
            .collect();
        pending.sort_unstable_by_key(|&(origin, t, act)| {
            let (kind, n) = match act {
                ChainAct::Load(n, _) => (0, n),
                ChainAct::Call(n) => (1, n),
                ChainAct::Store(n, _) => (2, n),
                ChainAct::Lookup { dst, setter } => (3 + u8::from(setter), dst),
            };
            (origin, t, kind, n)
        });
        for (origin, t, act) in pending {
            self.accessor_walk(origin, t, Key::any(), act);
        }
    }

    pub fn is_proto_key(&self, key: Key) -> bool {
        matches!(key, Key::Named(a) if self.atoms[a as usize] == "__proto__")
    }

    /// dst ⊇ `t`'s prototypes as values (DECISIONS 217): what
    /// `Object.getPrototypeOf(t)` and `t.__proto__` return. A literal's
    /// class prototype is its layout, no object (`Cons::ProtoValues`);
    /// `ObjectProto` stands for every builtin prototype, so its own
    /// prototype may be itself (Array.prototype's is Object.prototype) or
    /// null; a module record's (`module.exports` or a namespace) may be null.
    pub fn proto_value(&mut self, t: Tok, dst: NodeId) {
        if !self.unit_mode && builtins::builtin_function(self, t) {
            let key = Key::Named(self.atom("__proto__"));
            self.builtin_field_use(t, key, ChainAct::Load(dst, self.thrown));
        }
        self.proto_value_at(t, dst);
        // A program function born with another intrinsic prototype
        // (`%AsyncGeneratorFunction.prototype%`, a generator object's
        // `%GeneratorPrototype%` chain) has one the model does not
        // describe: not `Function.prototype`, whose reads differ.
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            if !builtins::born_with_function_proto(self, f) {
                self.add_tok(dst, TOP);
                return;
            }
        }
        if let Some(ns) = builtins::chain_ns(self, t) {
            self.with_b(|m| builtins::builtin_proto_value(m, Some(ns), dst));
        }
    }

    pub(super) fn proto_value_at(&mut self, t: Tok, dst: NodeId) {
        match self.toks[t as usize] {
            TokKind::Top => self.add_tok(dst, TOP),
            TokKind::ObjectProto => {
                self.add_tok(dst, t);
                self.add_prim(dst, prim::NULL);
            }
            _ => {
                if matches!(self.toks[t as usize], TokKind::ModNs(_)) {
                    let op = self.tok(TokKind::ObjectProto);
                    self.add_tok(dst, op);
                    self.add_prim(dst, prim::NULL);
                }
                let pn = self.proto_node(t);
                if self.idx_done.insert((pn, t, dst, IDX_PROTO_VALUE)) {
                    self.cons(pn, Cons::ProtoValues { dst });
                }
            }
        }
    }

    /// `t` read (called) under a key that names no member of an intrinsic
    /// prototype and is not `__proto__`: a number (DECISIONS 217; its own
    /// fields under any key are the caller's). Its program prototypes'
    /// members under any key, a key a store into ⊤ wrote on an intrinsic
    /// prototype (`Cons::PlainKey`), and accessors that key may name.
    pub(crate) fn plain_key(&mut self, t: Tok, act: ChainAct) {
        self.plain_key_from(t, t, act)
    }

    /// `plain_key` with receiver `origin` (rule PXRECV): `this` of the
    /// accessors it runs, the receiver passed to a Proxy on `t`'s chain.
    pub(crate) fn plain_key_from(&mut self, origin: Tok, t: Tok, act: ChainAct) {
        if t == TOP || (self.is_marker(t) && !self.virt_protos.contains_key(&t)) {
            return;
        }
        let cp = self.cp(t);
        let at = match act {
            ChainAct::Load(n, _) | ChainAct::Store(n, _) | ChainAct::Lookup { dst: n, .. } => n,
            ChainAct::Call(c) => c,
        };
        let tag = match act {
            ChainAct::Load(_, _) => IDX_PLAIN_LOAD,
            ChainAct::Store(_, _) | ChainAct::Lookup { .. } => return,
            ChainAct::Call(_) => IDX_PLAIN_CALL,
        };
        if self.idx_done.insert((cp, t, at, tag)) {
            self.cons(cp, Cons::PlainKey { origin, act });
            // An accessor under a numeric name (one under a computed key
            // made the object's fields ⊤ already).
            if self.numeric_accessor {
                self.accessor_walk(origin, t, Key::any(), act);
            } else {
                let num = self.atom(NUM_KEY);
                self.visits
                    .entry(t)
                    .or_default()
                    .insert((origin, Key::Named(num), act));
            }
        }
    }

    /// A native's [[Get]] and [[Set]] of `t`'s indices (an Array.prototype
    /// method on its receiver) are the program's element read and write: an
    /// accessor under a numeric name on `t` or its chain runs with `t` as
    /// `this`. A getter's result is an element the native reads and a setter
    /// takes the elements it writes, so both go through `elems`, the
    /// receiver's element node. Registered before the first numeric
    /// accessor, the visit waits for it (`add_accessor_atom`), as
    /// `plain_key`'s does.
    pub(crate) fn native_index_access(&mut self, t: Tok, elems: NodeId) {
        if t == TOP || (self.is_marker(t) && !self.virt_protos.contains_key(&t)) {
            return;
        }
        for act in [
            ChainAct::Load(elems, self.thrown),
            ChainAct::Store(elems, self.thrown),
        ] {
            self.index_accessors(t, t, act);
        }
    }

    /// An index read or write of `t` with `origin` as `this`: an accessor
    /// under a numeric name runs, once one exists (until then the visit
    /// waits for it).
    pub(crate) fn index_accessors(&mut self, origin: Tok, t: Tok, act: ChainAct) {
        if self.numeric_accessor {
            self.accessor_walk(origin, t, Key::any(), act);
        } else {
            let num = self.atom(NUM_KEY);
            self.visits
                .entry(t)
                .or_default()
                .insert((origin, Key::Named(num), act));
        }
    }

    /// A key no builtin prototype defines (ToPropertyKey of `true`, `false`,
    /// `null`, `undefined`; ECMA-262 has no such member): the receiver's own
    /// field and its program prototype chain, else `undefined`. The builtin
    /// member approximation (`tok_load`) has nothing under it.
    pub fn load_program_key(&mut self, t: Tok, a: Atom, dst: NodeId) {
        self.load_program_key_from(t, t, a, dst)
    }

    /// `load_program_key` with receiver `origin` (rule PXRECV).
    pub fn load_program_key_from(&mut self, origin: Tok, t: Tok, a: Atom, dst: NodeId) {
        if t == TOP || matches!(self.toks[t as usize], TokKind::BuiltinFn(_)) {
            return self.load_from(origin, t, Key::Named(a), dst);
        }
        self.mark_use(t, crate::model::MARK_READ);
        self.add_prim(dst, prim::UNDEF);
        let f = self.field_r(t, a);
        self.edge(f, dst);
        let cv = self.cv_recv(origin, t, Key::Named(a), true);
        self.edge(cv, dst);
        self.accessor_walk(origin, t, Key::Named(a), ChainAct::Load(dst, self.thrown));
    }

    /// Accessors run with the receiver as `this`. Named keys an accessor
    /// may be defined under walk the chain per receiver; computed keys
    /// visit, through the prototype closure, only holders with accessors.
    pub(crate) fn accessor_walk(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
        // A computed key keeps its shape: a [[Get]]/[[Set]] of key P runs
        // only an accessor defined under P (ECMA-262 10.1.8.1 OrdinaryGet,
        // 10.1.9.2 OrdinarySetWithOwnDescriptor), so on a holder the shape
        // splits (`shape_splits`) only the accessors it names run
        // (`accessor_hit`); the walk itself is per shape (`shared_act`).
        if matches!(act, ChainAct::Lookup { .. }) {
            if let Some(act) = self.shared_act(origin, t, key, act) {
                self.chain_step(0, origin, t, key, act);
            }
            return;
        }
        if !self.needs_accessor_walk(key) {
            // An Annex B call may install this key later. Remember the
            // lookup in the existing visit relation, without creating a
            // walk or charging solver work before an install exists.
            self.visits.entry(t).or_default().insert((origin, key, act));
            return;
        }
        let Some(act) = self.shared_act(origin, t, key, act) else {
            return;
        };
        match key {
            Key::Named(_) => self.holder_walk(origin, t, key, act, false),
            Key::Any(_) => {
                self.accessor_hit(origin, t, key, act);
                let cp = self.cp(t);
                if self.any_walk_done.insert((cp, origin, act)) {
                    self.any_walks.push((cp, origin, t, act));
                    self.any_walk_holders
                        .entry(t)
                        .or_default()
                        .push((origin, key, act));
                    self.cons(cp, Cons::AnyAccessors { origin, key, act });
                }
            }
        }
    }

    /// Walks are per (receiver, start, key), not per site: the first site
    /// walks with a shared act (a node the accessors feed, or that feeds
    /// the setters), every site connects to it. `None`: already walked.
    /// The same accessors run with the same `this`, so the union each site
    /// sees is unchanged.
    pub(super) fn shared_act(
        &mut self,
        origin: Tok,
        t: Tok,
        key: Key,
        act: ChainAct,
    ) -> Option<ChainAct> {
        let store = matches!(act, ChainAct::Store(_, _));
        let kind: u8 = match act {
            ChainAct::Load(_, _) => 0,
            ChainAct::Store(_, _) => 1,
            ChainAct::Call(_) => 2,
            ChainAct::Lookup { setter, .. } => 3 + u8::from(setter),
        };
        let ((n, rn), fresh) = match self.walk_nodes.get(&(origin, t, key, kind)) {
            Some(&w) => (w, false),
            None => {
                let n = self.node_l(
                    [
                        "walk-load",
                        "walk-store",
                        "walk-call",
                        "walk-getter",
                        "walk-setter",
                    ][kind as usize],
                );
                if kind == 1 {
                    self.walk_store_pending.insert(n, Vec::new());
                }
                // What the walk's accessors throw reaches each site's region.
                let rn = self.node_l("walk-raise");
                self.walk_nodes.insert((origin, t, key, kind), (n, rn));
                ((n, rn), true)
            }
        };
        let region = match act {
            ChainAct::Load(_, r) | ChainAct::Store(_, r) => r,
            ChainAct::Call(call) => self.calls[call as usize].region,
            ChainAct::Lookup { .. } => NO_NODE,
        };
        if region != NO_NODE {
            self.edge(rn, region);
        }
        match act {
            ChainAct::Load(dst, _) | ChainAct::Lookup { dst, .. } => {
                if dst == self.esc_accessors {
                    if let Some(d) = &mut self.diagnostic {
                        d.acc_src.entry(n).or_insert(origin);
                    }
                }
                self.edge(n, dst)
            }
            ChainAct::Store(src, _) => match self.walk_store_pending.get_mut(&n) {
                Some(v) => v.push(src),
                None => self.edge(src, n),
            },
            ChainAct::Call(call) => self.bind_via(n, call, origin),
        }
        // A call walk collects getter results as a load does.
        fresh.then_some(match act {
            ChainAct::Lookup { setter, .. } => ChainAct::Lookup { dst: n, setter },
            _ if store => ChainAct::Store(n, rn),
            _ => ChainAct::Load(n, rn),
        })
    }

    /// The holder sets of a named-key lookup that starts at `t` itself
    /// (`t` is passed through, then its prototypes).
    pub(super) fn holders(&mut self, t: Tok, key: Key) -> (NodeId, NodeId) {
        if let Some(&x) = self.holder_nodes.get(&(t, key)) {
            return x;
        }
        let p = self.node_l("holders");
        let s = self.node_l("holders-stop");
        self.holder_nodes.insert((t, key), (p, s));
        self.add(p, &[t], 0);
        let pn = self.proto_node(t);
        self.cons(
            pn,
            Cons::HolderStep {
                key,
                pass: p,
                stop: s,
            },
        );
        (p, s)
    }

    /// A named-key accessor lookup for receiver `origin` from `t` (from
    /// `t`'s prototypes, or, `at_start`, from prototype `t` itself with a
    /// stop check there: `super.key`). Accessors on the pass holders run
    /// and replay; those on stop holders run.
    pub(super) fn holder_walk(
        &mut self,
        origin: Tok,
        t: Tok,
        key: Key,
        act: ChainAct,
        at_start: bool,
    ) {
        let (p, s) = if at_start {
            let p = self.node_l("holders");
            let s = self.node_l("holders-stop");
            let x = self.node_l("holders-start");
            self.add(x, &[t], 0);
            self.cons(
                x,
                Cons::HolderStep {
                    key,
                    pass: p,
                    stop: s,
                },
            );
            (p, s)
        } else {
            self.holders(t, key)
        };
        self.cons(p, Cons::VisitHolders { origin, key, act });
        self.cons(s, Cons::VisitStop { origin, key, act });
    }

    /// A store walk found a consumer: its sites' sources flow from now on.
    pub(super) fn store_live(&mut self, n: NodeId) {
        if let Some(srcs) = self.walk_store_pending.remove(&n) {
            for src in srcs {
                self.edge(src, n);
            }
        }
    }

    pub fn needs_accessor_walk(&self, key: Key) -> bool {
        match key {
            Key::Named(a) => {
                self.accessor_atoms.contains(&a)
                    || self
                        .atom_index
                        .get(crate::STAR_KEY)
                        .is_some_and(|a| self.accessor_atoms.contains(a))
            }
            Key::Any(_) => !self.accessor_atoms.is_empty(),
        }
    }

    /// The values `key` takes on `t`'s prototypes (transitively).
    pub fn cv(&mut self, t: Tok, key: Key) -> NodeId {
        self.cv_of(t, key, true)
    }

    /// `cv`, with the builtin prototypes' members or (`builtin` false)
    /// with the program prototypes' values only.
    pub fn cv_of(&mut self, t: Tok, key: Key, builtin: bool) -> NodeId {
        self.cv_recv(t, t, key, builtin)
    }

    /// `cv_of` of `t` read with receiver `origin` (rule PXRECV: a Proxy's
    /// forward): a Proxy on `t`'s chain is passed `origin`.
    pub fn cv_recv(&mut self, origin: Tok, t: Tok, key: Key, builtin: bool) -> NodeId {
        self.note_chain_recv_at(t, origin);
        self.cv_up(t, key, builtin)
    }

    /// `cv_of` of a prototype `t` on a receiver's chain (the next link of a
    /// `ChainUnion`): `t` is no receiver of it.
    pub(super) fn cv_up(&mut self, t: Tok, key: Key, builtin: bool) -> NodeId {
        if let Some(&n) = self.cv_nodes.get(&(t, key, builtin)) {
            return n;
        }
        let n = self.node_l("chainval");
        self.cv_nodes.insert((t, key, builtin), n);
        let pn = self.proto_node(t);
        self.cons(
            pn,
            Cons::ChainUnion {
                key,
                dst: n,
                of: t,
                builtin,
            },
        );
        n
    }

    /// A prototype of the runtime an instance's chain can reach
    /// (`ObjectProto` stands for each of them): redefining a key there may
    /// intercept the store that makes an own key (a setter, a read-only
    /// value). A namespace object or a builtin function is no instance's
    /// prototype.
    pub(crate) fn is_chain_proto(&self, q: Tok) -> bool {
        matches!(
            self.toks[q as usize],
            TokKind::ObjectProto | TokKind::NativeProto(..)
        )
    }

    /// ENUM1: a built-in object a for-in visits only program-written keys
    /// of (`builtins::enum_free_ns`): a language namespace or constructor,
    /// or its prototype. Not `ObjectProto` as a receiver (it stands for
    /// every builtin prototype, a host one whose methods are enumerable
    /// among them). A builtin function value only when it is the realm's
    /// (`builtins::enum_free_fn`, F-B); a host function may hold enumerable
    /// data, `fs.realpath.native`.
    pub(crate) fn enum_free(&self, t: Tok) -> bool {
        match &self.toks[t as usize] {
            TokKind::Builtin(ns) => builtins::enum_free_ns(*ns),
            TokKind::NativeProto(_, ns) => ns.is_some_and(builtins::enum_free_ns),
            TokKind::BuiltinFn(crate::model::Native::Member { identity, .. }) => {
                builtins::enum_free_fn(identity)
            }
            _ => false,
        }
    }

    /// ENUM2: a value whose properties are its program-written fields as a
    /// for-in sees them (its built-in own properties, a function's `name`,
    /// `length`, `prototype`, an array's `length`, are not enumerable): a
    /// program object, array, function, class object or class prototype,
    /// or a `JSON.parse` object (ECMA-262 25.5.1 ParseJSON /
    /// InternalizeJSONProperty: an ordinary object whose own properties are
    /// all CreateDataProperty-made, so enumerable; its arrays are `Arr`).
    /// Not a marker, not another builtin-made object (`BObj`), not a
    /// generator's step closure (it stands for the generator object).
    pub(crate) fn enum_obj(&self, t: Tok) -> bool {
        if t == TOP || self.is_marker(t) {
            return false;
        }
        match &self.toks[t as usize] {
            TokKind::Obj(tm, _) => matches!(
                self.tmpls[*tm as usize].kind,
                TmplKind::Lit { .. }
                    | TmplKind::Class { .. }
                    | TmplKind::FnInst { .. }
                    | TmplKind::ObjCreate { .. }
                    | TmplKind::Arr { .. }
                    | TmplKind::BObj { what: "json", .. }
            ),
            TokKind::Proto(_) | TokKind::ClassObj(_) => true,
            TokKind::Func(f, _) => !self.funcs[*f as usize].gen_step(),
            _ => false,
        }
    }

    /// ENUM2: prototype `q` of chain link `from` has no enumerable built-in
    /// member: a built-in object of `enum_free`, a program value
    /// (`enum_obj`), or the collapsed `ObjectProto` where the seed of
    /// `from` makes it such an intrinsic prototype
    /// (`builtins::enum_free_seed`), or a built-in instance whose own
    /// enumerable properties are its program-written fields and, for a
    /// typed array, its elements (`builtins::enum_free_link_obj`).
    pub(crate) fn enum_link(&self, from: Tok, q: Tok) -> bool {
        self.enum_free(q)
            || self.enum_obj(q)
            || builtins::enum_free_link_obj(self, q)
            || matches!(self.toks[q as usize], TokKind::ObjectProto)
                && builtins::enum_free_seed(self, from)
    }

    /// A builtin prototype on a chain (`ObjectProto` stands for each of
    /// them): its members are the runtime's, and what stores into ⊤ wrote.
    pub(crate) fn is_builtin_proto(&self, q: Tok) -> bool {
        matches!(
            self.toks[q as usize],
            TokKind::ObjectProto
                | TokKind::NativeProto(..)
                | TokKind::Builtin(_)
                | TokKind::BuiltinFn(_)
        )
    }

    /// dst ⊇ `src`'s values without `undefined`: a rule on `src`, not an
    /// edge into a relay (an observation node feeds only observation nodes,
    /// `edge`; `sum_skip` applies that to this rule by its `dst`).
    pub(crate) fn defined_from(&mut self, src: NodeId, dst: NodeId) {
        if self.defined_done.insert((src, dst)) {
            self.cons(src, Cons::Defined { dst });
        }
    }

    /// The node whose values reach `dst` without `undefined` (`Cons::Defined`):
    /// `tok_load` writes into it (no edge, so an observation `dst` is fine).
    fn defined_into(&mut self, dst: NodeId) -> NodeId {
        if let Some(&n) = self.defined_via.get(&dst) {
            return n;
        }
        let n = self.node_l("defined");
        self.defined_via.insert(dst, n);
        self.cons(n, Cons::Defined { dst });
        n
    }

    /// Every prototype of `t`, transitively.
    pub fn cp(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.cp_nodes.get(&t) {
            return n;
        }
        let n = self.node_l("protoclosure");
        self.cp_nodes.insert(t, n);
        let pn = self.proto_node(t);
        self.cons(pn, Cons::ProtoClosure { dst: n });
        n
    }

    /// Does a lookup of `key` stop at prototype `q` (a declared member)?
    pub(super) fn stops_at(&self, q: Tok, key: Key) -> bool {
        match (key, &self.toks[q as usize]) {
            (Key::Named(a), TokKind::Proto(tm)) => {
                self.tmpls[*tm as usize].proto_members.contains(&a)
                    && !self.hard.contains_key(&q)
                    && !self.escaped.contains_key(&q)
            }
            _ => false,
        }
    }

    /// A builtin object whose iteration is not Array iteration: every
    /// builtin object template but the arrays the runtime makes
    /// (`builtins::iterates_indices`).
    fn iterates_internal(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(self.tmpls[tm as usize].kind,
                TmplKind::BObj { what, .. } if !builtins::iterates_indices(what)))
    }

    pub fn is_array_like(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind {
                TmplKind::Arr { .. } | TmplKind::Args { .. } => true,
                TmplKind::BObj { .. } => true,
                _ => false,
            },
            _ => false,
        }
    }

    /// An Array's own `length` (design §17.8, DECISIONS 54): a data property
    /// that always holds a Number and is never an element. A read is that
    /// Number; a store converts its value with ToNumber and only moves the
    /// end of the element range (a new slot reads `undefined`, which every
    /// element read already has). Neither touches a field, so `length` never
    /// joins the elements (`all_fields`) nor reads them (`field_any`).
    /// A function's `arguments` object (`TmplKind::Args`).
    pub fn is_args(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(self.tmpls[tm as usize].kind, TmplKind::Args { .. }))
    }

    /// An Array exotic object of the program (`TmplKind::Arr`).
    pub fn is_arr(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    }

    pub fn is_array_length(&self, t: Tok, key: Key) -> bool {
        matches!(key, Key::Named(a) if self.atoms[a as usize] == "length")
            && matches!(self.toks[t as usize], TokKind::Obj(tm, _)
                if matches!(self.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    }

    pub fn is_constructor_key(&self, key: Key) -> bool {
        self.key_may_name_str(key, "constructor")
    }

    /// Start or continue a chain lookup at `t`'s prototypes.
    pub(super) fn chain_from(&mut self, origin: Tok, t: Tok, key: Key, act: ChainAct) {
        // Own accessors of `t` itself (object-literal getters/setters).
        if !matches!(self.toks[t as usize], TokKind::ObjectProto) {
            self.visits.entry(t).or_default().insert((origin, key, act));
        }
        self.accessor_hit(origin, t, key, act);
        let pn = self.proto_node(t);
        if self.chain_done.insert((pn, origin, key, act)) {
            self.cons(pn, Cons::Chain { origin, key, act });
        }
    }

    /// One step of a per-receiver accessor walk: prototype `p` of `origin`.
    /// (Values and methods come from the shared chain nodes `cv`/`cp`.)
    pub fn chain_step(&mut self, pn: NodeId, origin: Tok, p: Tok, key: Key, act: ChainAct) {
        if let ChainAct::Lookup { dst, setter } = act {
            self.lookup_holder(origin, p, key, dst, setter);
            return;
        }
        if p == TOP {
            match act {
                ChainAct::Load(dst, _) | ChainAct::Lookup { dst, .. } => self.add_tok(dst, TOP),
                ChainAct::Call(call) => self.call_tok(call, TOP, Some(origin)),
                ChainAct::Store(src, _) => {
                    self.store_live(src);
                    if let Some(d) = &mut self.diagnostic {
                        d.store_top.entry(src).or_insert(pn);
                    }
                    let r = self.reason("store through an unknown prototype chain");
                    self.sink_node(src, r);
                }
            }
            return;
        }
        for x in self.class_id_holders(origin, p) {
            self.accessor_hit(origin, x, key, act);
        }
        // A class prototype defines its declared members at class creation:
        // the lookup ends there (resumed if the prototype is later mutated).
        if self.stops_at(p, key) {
            self.accessor_hit(origin, p, key, act);
            self.stopped.entry(p).or_default().push((origin, key, act));
            return;
        }
        self.chain_from(origin, p, key, act);
    }

    /// The class-id holders of a lookup on `q` for `of` (the receiver, or
    /// the prototype the lookup came from): a specialization's instances
    /// have its origin's prototype object `q` as [[Prototype]], one object
    /// (`js_register_class_generic_origin`), and a call or accessor access
    /// there for an instance of the specialization (or of a class derived
    /// from one) runs the specialization's member by class id (codegen
    /// `static_prototype::chain`): `Proto` of each such specialization,
    /// which holds its declared members and is no object's prototype.
    pub(crate) fn class_id_holders(&mut self, of: Tok, q: Tok) -> Vec<Tok> {
        let mut out = Vec::new();
        if self.constructor_origins.is_empty() {
            return out;
        }
        let TokKind::Proto(o) = self.toks[q as usize] else {
            return out;
        };
        let starts: Vec<TmplId> = match self.toks[of as usize] {
            TokKind::Obj(c, _) if matches!(self.tmpls[c as usize].kind, TmplKind::Class { .. }) => {
                vec![c]
            }
            TokKind::Proto(c) => vec![c],
            _ => self.virt_protos.get(&of).cloned().unwrap_or_default(),
        };
        for c in starts {
            let mut cur = Some(c);
            for _ in 0..self.tmpls.len() {
                let Some(x) = cur else { break };
                if x != o && self.constructor_origin(x) == o {
                    let t = self.tok(TokKind::Proto(x));
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
                cur = self.tmpls[x as usize].parent;
            }
        }
        out
    }

    /// `super.key` from receiver `origin`: the lookup starts at `start`.
    pub(super) fn super_lookup(&mut self, origin: Tok, start: Tok, key: Key, act: ChainAct) {
        // The Receiver of `super.key` is `this` (13.3.7.3 MakeSuperPropertyReference).
        self.note_chain_recv(origin);
        if start == TOP {
            self.chain_step(0, origin, TOP, key, act);
            return;
        }
        // Rule PXSUPER: a Proxy home prototype runs its [[Get]] / [[Set]]
        // with Receiver `this` (`proxy_super`).
        if let TokKind::Proxy(p) = self.toks[start as usize] {
            return self.proxy_super(p, origin, key, act);
        }
        match act {
            ChainAct::Load(dst, _) if self.is_array_length(start, key) => {
                self.add_prim(dst, prim::NUM);
            }
            ChainAct::Load(dst, _) => {
                let f = match key {
                    Key::Named(a) => self.field_r(start, a),
                    Key::Any(_) => self.all_fields(start),
                };
                self.edge(f, dst);
                // DECISIONS 217: `super.__proto__` runs Object.prototype's
                // getter on the receiver.
                if self.is_proto_key(key) {
                    let pk = self.atom("__proto__");
                    self.load_from(origin, origin, Key::Named(pk), dst);
                }
                self.with_b(|m| builtins::tok_load(m, start, key, dst));
                if !self.stops_at(start, key) {
                    let up = self.cv(start, key);
                    self.edge(up, dst);
                }
            }
            ChainAct::Call(call) => {
                let f = match key {
                    Key::Named(a) => self.field_r(start, a),
                    Key::Any(_) => self.all_fields(start),
                };
                self.bind_via(f, call, origin);
                self.with_native(call, |m, call| {
                    builtins::proto_method(m, start, origin, key, call)
                });
                if !self.stops_at(start, key) {
                    let up = self.cv(start, key);
                    self.bind_via(up, call, origin);
                    let cp = self.cp(start);
                    self.cons(cp, Cons::ProtoMethods { origin, key, call });
                }
            }
            ChainAct::Lookup { .. } => {
                self.chain_step(0, origin, start, key, act);
                return;
            }
            ChainAct::Store(_, _) => {}
        }
        if !self.needs_accessor_walk(key) {
            self.visits
                .entry(start)
                .or_default()
                .insert((origin, key, act));
        }
        match key {
            Key::Named(_) if self.needs_accessor_walk(key) => {
                // `start` itself is a prototype: a lookup can stop there.
                if let Some(act) = self.shared_act(origin, start | SUPER_BIT, key, act) {
                    self.holder_walk(origin, start, key, act, true)
                }
            }
            Key::Any(_) => self.accessor_walk(origin, start, key, act),
            _ => {}
        }
    }

    /// A prototype became HARD or escaped: lookups that stopped at it go on.
    pub(super) fn resume_stopped(&mut self, p: Tok) {
        if let Some(v) = self.stopped.remove(&p) {
            for (origin, key, act) in v {
                self.chain_from(origin, p, key, act);
            }
        }
        if let Some(v) = self.stopped_holders.remove(&p) {
            for (key, pass, stop) in v {
                let (p2, s2) = self.holders(p, key);
                self.edge_quiet(p2, pass);
                self.edge_quiet(s2, stop);
            }
        }
        if let Some(v) = self.stopped_shared.remove(&p) {
            for (key, dst, builtin) in v {
                let up = self.cv_up(p, key, builtin);
                self.edge(up, dst);
            }
        }
    }

    /// Accessors named `key` (any key: `Key::Any`) declared on `holder`:
    /// invocation actions run them with `origin` as `this` through the holder's
    /// group; a descriptor query returns their function tokens directly.
    pub fn accessor_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        if let ChainAct::Lookup { dst, setter } = act {
            self.accessor_functions(holder, key, dst, setter);
            return;
        }
        // A getter the realm declares on a realm prototype of the chain
        // (`builtins::realm_proto_getter`) runs with the receiver as `this`.
        if let Key::Named(a) = key {
            if let Some(e) = builtins::realm_proto_getter(self, holder, a) {
                match act {
                    ChainAct::Load(dst, _) => {
                        self.with_b(|m| builtins::realm_getter_load(m, e, origin, a, dst))
                    }
                    ChainAct::Call(call) => {
                        let v = self.node_l("realm-getter-callee");
                        self.with_b(|m| builtins::realm_getter_load(m, e, origin, a, v));
                        self.bind_via(v, call, origin);
                    }
                    ChainAct::Store(..) | ChainAct::Lookup { .. } => {}
                }
            }
            if let ChainAct::Store(src, region) = act {
                self.realm_setter_hit(origin, holder, a, src, region);
            }
        }
        if let (Key::Any(_), ChainAct::Store(src, region)) = (key, act) {
            for a in self.with_b(|m| builtins::realm_proto_setter_keys(m, holder)) {
                if self.key_may_name(key, a) {
                    self.realm_setter_hit(origin, holder, a, src, region);
                }
            }
        }
        // A key whose shape splits the holder (`shape_splits`) runs only the
        // accessors named by a name it admits, and the computed-name ones.
        if self.shape_splits(holder, key).is_some() {
            let star = self.atom(crate::STAR_KEY);
            let names = self.accessor_keys.get(&holder).cloned().unwrap_or_default();
            for a in names {
                if (a == star || self.key_may_name(key, a))
                    && self.accessors.contains_key(&(holder, a))
                {
                    let k = Key::Named(a);
                    self.acc_group(holder, k);
                    self.group_hit(origin, holder, k, act);
                }
            }
            return;
        }
        // Accessor groups are keyed by the computed key itself.
        let key = key.unshaped();
        if let Key::Named(a) = key {
            let star = self.atom(crate::STAR_KEY);
            if a != star && self.accessors.contains_key(&(holder, star)) {
                let wildcard = Key::Named(star);
                self.acc_group(holder, wildcard);
                self.group_hit(origin, holder, wildcard, act);
            }
        }
        let has = match key {
            Key::Named(a) => self.accessors.contains_key(&(holder, a)),
            Key::Any(_) => self
                .accessor_keys
                .get(&holder)
                .is_some_and(|ks| !ks.is_empty()),
        };
        if !has {
            return;
        }
        self.acc_group(holder, key);
        self.group_hit(origin, holder, key, act);
    }

    /// A store from `origin` under `a` finds a realm accessor on the chain
    /// of realm prototype `holder`: its declared setter runs with `origin`
    /// as `this` (ECMA-262 10.1.9.2 step 7). A getter-only accessor runs
    /// nothing and keeps nothing (`RealmSetter::GetterOnly`). A setter the
    /// realm declares no effects for is an explicit stop: the seed names the
    /// accessor, the stored value escapes and what the setter does to the
    /// receiver is unknown.
    fn realm_setter_hit(&mut self, origin: Tok, holder: Tok, a: Atom, src: NodeId, region: NodeId) {
        match builtins::realm_proto_setter(self, holder, a) {
            None | Some(builtins::RealmSetter::GetterOnly) => {}
            Some(builtins::RealmSetter::Declared(e)) => {
                self.with_b(|m| builtins::realm_setter_store(m, e, origin, a, src))
            }
            Some(builtins::RealmSetter::Undeclared(what)) => {
                let reason = self.reason(&format!(
                    "stored through realm accessor {what} whose setter the realm does not declare"
                ));
                self.cons(src, Cons::TopSink { reason });
                let v = self.node_l("realm-setter-undeclared");
                self.add_top(v, |_| {
                    format!("store through realm accessor {what}: setter effects undeclared")
                });
                let n = self.node_l("realm-setter-this");
                self.add_tok(n, origin);
                let site = self.anon_post_site("undeclared realm setter");
                self.cons(
                    n,
                    Cons::Store {
                        key: Key::any(),
                        src: v,
                        site,
                        region,
                    },
                );
            }
        }
    }

    /// One lookup from `origin` reaches group (holder, key).
    pub(super) fn group_hit(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        let g = &self.acc_groups[&(holder, key)];
        let (gt, ar, st, sp, rg, rs, live) = (g.gt, g.ar, g.st, g.sp, g.rg, g.rs, g.has_setter);
        // ACCREGION: an accessor runs inside the [[Get]]/[[Set]] of the
        // access (ECMA-262 10.1.8.1 step 7, 10.1.9.2 step 7): what it
        // throws completes that access, in the access's region.
        let (raise, region) = match act {
            ChainAct::Load(_, r) => (rg, r),
            ChainAct::Store(_, r) => (rs, r),
            ChainAct::Call(call) => (rg, self.calls[call as usize].region),
            ChainAct::Lookup { .. } => unreachable!("descriptor queries never invoke a group"),
        };
        self.edge(raise, region);
        match act {
            ChainAct::Lookup { .. } => unreachable!("descriptor queries never invoke a group"),
            ChainAct::Load(dst, _) => {
                self.add_tok(gt, origin);
                self.edge(ar, dst);
            }
            ChainAct::Call(call) => {
                // `o.k()` where k is a getter: the getter runs, then its
                // result is called with `o` as `this`.
                self.add_tok(gt, origin);
                self.bind_via(ar, call, origin);
            }
            ChainAct::Store(src, _) => {
                self.add_tok(st, origin);
                if live {
                    self.store_live(src);
                    self.edge(src, sp);
                } else {
                    self.acc_groups
                        .get_mut(&(holder, key))
                        .unwrap()
                        .pending
                        .push(src);
                }
            }
        }
    }

    /// The group of (holder, key), created with every accessor declared
    /// there so far (an Any group: linked to every named group of the
    /// holder).
    pub(super) fn acc_group(&mut self, holder: Tok, key: Key) {
        if self.acc_groups.contains_key(&(holder, key)) {
            return;
        }
        let gt = self.node_l("acc-this");
        let ar = self.node_l("acc-ret");
        let st = self.node_l("acc-setthis");
        let sp = self.node_l("acc-setarg");
        let rg = self.node_l("acc-get-raise");
        let rs = self.node_l("acc-set-raise");
        self.acc_groups.insert(
            (holder, key),
            AccGroup {
                gt,
                ar,
                st,
                sp,
                rg,
                rs,
                has_setter: false,
                pending: Vec::new(),
            },
        );
        match key {
            Key::Named(a) => {
                let (ng, ns) = self
                    .accessors
                    .get(&(holder, a))
                    .map_or((0, 0), |(g, s)| (g.len(), s.len()));
                for i in 0..ng {
                    let f = self.accessors[&(holder, a)].0[i];
                    self.group_connect(holder, key, f, false);
                }
                for i in 0..ns {
                    let f = self.accessors[&(holder, a)].1[i];
                    self.group_connect(holder, key, f, true);
                }
                if self.acc_groups.contains_key(&(holder, Key::any())) {
                    self.group_link(holder, a);
                }
            }
            Key::Any(_) => {
                let n = self.accessor_keys.get(&holder).map_or(0, |v| v.len());
                for i in 0..n {
                    let a = self.accessor_keys[&holder][i];
                    if !self.accessors.contains_key(&(holder, a)) {
                        continue;
                    }
                    self.acc_group(holder, Key::Named(a));
                    self.group_link(holder, a);
                }
            }
        }
    }

    /// Link named group (holder, a) under the holder's Any group.
    pub(super) fn group_link(&mut self, holder: Tok, a: Atom) {
        let any = self.acc_groups[&(holder, Key::any())].clone();
        let g = self.acc_groups[&(holder, Key::Named(a))].clone();
        self.edge_quiet(any.gt, g.gt);
        self.edge_quiet(g.ar, any.ar);
        self.edge_quiet(g.rg, any.rg);
        self.edge_quiet(g.rs, any.rs);
        self.edge_quiet(any.st, g.st);
        self.edge_quiet(any.sp, g.sp);
        if g.has_setter {
            self.group_live(holder, Key::any());
        }
    }

    /// Accessor `f` hangs off group (holder, key).
    pub(super) fn group_connect(&mut self, holder: Tok, key: Key, f: Tok, setter: bool) {
        let func = matches!(self.toks[f as usize], TokKind::Func(..));
        // DECISIONS 225: a lookup reached the accessor's group.
        if self.act.is_some() && func {
            let g = self.tok_fn(f);
            self.activate(g, "accessor");
        }
        let g = self.acc_groups[&(holder, key)].clone();
        // An accessor with a lexical `this` (arrow) never sees the
        // receiver, as for a call (`call_tok`). A class or builtin
        // accessor gets its receivers through its call (`group_call`).
        let this =
            (func && !self.funcs[self.tok_fn(f) as usize].lexical_this).then(|| self.this_any(f));
        if setter {
            if let Some(this) = this {
                self.edge_quiet(g.st, this);
            }
            self.group_bind(holder, key, f, true);
            self.group_live(holder, key);
            if self.acc_groups.contains_key(&(holder, Key::any())) {
                self.group_live(holder, Key::any());
            }
        } else {
            if let Some(this) = this {
                self.edge_quiet(g.gt, this);
            }
            self.group_bind(holder, key, f, false);
        }
    }

    /// The parameter/result half of `group_connect`.
    pub(crate) fn group_bind(&mut self, holder: Tok, key: Key, ft: Tok, setter: bool) {
        if !matches!(self.toks[ft as usize], TokKind::Func(..)) {
            if !self.sum.ready {
                self.defer(crate::summary::Deferred::Group(holder, key, ft, setter));
                return;
            }
            return self.group_call(holder, key, ft, setter);
        }
        let f = self.tok_fn(ft);
        self.activate(f, "accessor");
        if self.diagnostic.is_some() {
            self.diagnostic_reach(f, "accessor", u32::MAX);
        }
        // What the accessor throws reaches the group's getter or setter
        // raise node, which feeds the region of every access that runs it
        // (`group_hit`).
        let raise = self.raise_of(f);
        let g = &self.acc_groups[&(holder, key)];
        let rs = if setter { g.rs } else { g.rg };
        self.edge(raise, rs);
        if !self.sum.ready {
            self.defer(crate::summary::Deferred::Group(holder, key, ft, setter));
            return;
        }
        let (sp, ar, this) = {
            let g = &self.acc_groups[&(holder, key)];
            (g.sp, g.ar, if setter { g.st } else { g.gt })
        };
        if self.summarized(f) {
            // The group's receivers are the instance's `this`, as they are
            // the shared `this` in `group_connect` (none for a lexical one).
            let this = (!self.funcs[f as usize].lexical_this).then_some(this);
            self.group_summarized(ft, this, sp, ar, setter);
            return;
        }
        // A getter has no arguments; a setter has exactly its stored value.
        // Missing arguments are undefined, including for default parameters.
        for i in usize::from(setter)..self.funcs[f as usize].params.len() {
            let p = self.funcs[f as usize].params[i];
            self.add_prim(p, prim::UNDEF);
        }
        if setter {
            if let Some(&p0) = self.funcs[f as usize].params.first() {
                self.edge_quiet(sp, p0);
            }
        } else {
            let ret = self.funcs[f as usize].ret;
            self.edge_quiet(ret, ar);
        }
    }

    /// An accessor whose function is a class, a builtin or a bound function
    /// (B.2.2.2-3 `__defineGetter__(k, Object)`, a descriptor `get: C`):
    /// [[Get]] runs `Call(getter, receiver)`, [[Set]] runs
    /// `Call(setter, receiver, « v »)`, as for a program accessor. The
    /// group's call of `f`, with each receiver of the group as `this`
    /// (`Cons::NativeThis`), runs that value's ordinary call: a class
    /// throws a TypeError before it reads `this` (nothing flows), `Object()`
    /// is a fresh object, an `Object.prototype` method runs on the receiver,
    /// a bound function calls its target. What it throws reaches the
    /// groups raise node, as a program accessors does (`group_bind`).
    fn group_call(&mut self, holder: Tok, key: Key, f: Tok, setter: bool) {
        let g = self.acc_groups[&(holder, key)].clone();
        let ret = self.node_l(if setter {
            "acc-call-setret"
        } else {
            "acc-call-ret"
        });
        if !setter {
            self.edge(ret, g.ar);
        }
        let (recv, args) = if setter {
            (g.st, vec![g.sp])
        } else {
            (g.gt, vec![])
        };
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Node(recv),
            args,
            spread: vec![],
            ret,
            site: 0,
            region: if setter { g.rs } else { g.rg },
        });
        self.why_mint(call, None, u32::MAX);
        self.rel(|| format!("accessorcall {call} {holder} {f} {}", setter as u8));
        self.cons(recv, Cons::NativeThis { call, f });
    }

    /// A group has a setter: its waiting store sources flow from now on.
    pub(super) fn group_live(&mut self, holder: Tok, key: Key) {
        let g = self.acc_groups.get_mut(&(holder, key)).unwrap();
        if g.has_setter {
            return;
        }
        g.has_setter = true;
        let pending = std::mem::take(&mut g.pending);
        let sp = g.sp;
        for src in pending {
            self.store_live(src);
            self.edge(src, sp);
        }
    }

    /// The one getter under own accessor key `a` of `h` that is a contract
    /// function of this unit (`Gen::note_own_getters`): no setter, every
    /// getter token one function of this unit, in `contract_fns`. Other
    /// units run it through its contract, which checks its result; the
    /// literal's leave walk does not load it (`leave_walk`).
    pub(crate) fn contract_getter(&self, h: Tok, a: Atom) -> Option<FnIdx> {
        if !self.getter_owners.contains(&h) {
            return None;
        }
        let (gs, ss) = self.accessors.get(&(h, a))?;
        if !ss.is_empty() {
            return None;
        }
        let mut one = None;
        for &g in gs {
            let f = self.tok_fn(g);
            if f == NO_FN
                || self.funcs[f as usize].func_id == u32::MAX
                || !self.contract_fns.contains(&f)
                || one.is_some_and(|o| o != f)
            {
                return None;
            }
            one = Some(f);
        }
        // Its readers run the stub once a census broke its contract.
        one.filter(|&f| {
            !self
                .broken_fns
                .contains(&format!("fn:{}", self.funcs[f as usize].name))
        })
    }

    /// What other units read from published literal `t` through its
    /// accessors loads into `dst` (`Gen::fields_leave`): every accessor on
    /// its prototypes, as a computed-key walk finds them, and its own
    /// accessors except its contract getters (`contract_getter`), whose
    /// results leave through their contracts. An accessor installed on `t`
    /// later joins (`accessor_installed`).
    pub(crate) fn leave_walk(&mut self, t: Tok, dst: NodeId) {
        let seen = self.leave_loads.insert(t, dst).is_some();
        if !self.needs_accessor_walk(Key::any()) {
            return;
        }
        let act = ChainAct::Load(dst, self.thrown);
        let cp = self.cp(t);
        if self.any_walk_done.insert((cp, t, act)) {
            self.any_walks.push((cp, t, t, act));
            self.cons(
                cp,
                Cons::AnyAccessors {
                    origin: t,
                    key: Key::any(),
                    act,
                },
            );
        }
        if seen {
            return;
        }
        let keys = self.accessor_keys.get(&t).cloned().unwrap_or_default();
        for a in keys {
            self.leave_key(t, a);
        }
    }

    /// Own accessor key `a` of a leaving literal `t` (`leave_walk`): unless
    /// it holds a contract getter, what it returns loads into the leave
    /// node. The computed-key group contributes its possible getter results too.
    fn leave_key(&mut self, t: Tok, a: Atom) {
        let Some(&dst) = self.leave_loads.get(&t) else {
            return;
        };
        if !self.accessors.contains_key(&(t, a)) {
            return;
        }
        // Other units run a contract getter on the literal: so does its
        // contract function here (as `note_field_fns` for a method).
        if let Some(f) = self.contract_getter(t, a) {
            if !self.funcs[f as usize].lexical_this {
                for g in self.accessors[&(t, a)].0.clone() {
                    let this = self.this_any(g);
                    self.add_tok(this, t);
                }
            }
            return;
        }
        self.acc_group(t, Key::Named(a));
        self.group_hit(t, t, Key::Named(a), ChainAct::Load(dst, self.thrown));
    }

    /// Accessor `f` was installed on (holder, key) while solving. With
    /// `replay`, lookups that reached the holder before it had a group for
    /// this key run it now; later ones find it through the group.
    pub(super) fn accessor_installed(
        &mut self,
        holder: Tok,
        key: Atom,
        f: Tok,
        setter: bool,
        replay: bool,
    ) {
        let k = Key::Named(key);
        let wildcard = self.atoms[key as usize] == crate::STAR_KEY;
        let visits: Vec<_> = self
            .visits
            .get(&holder)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|&(_, kk, _)| {
                        wildcard || kk == k || (kk.is_any() && self.shape_admits(holder, kk, key))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for &(_, _, act) in &visits {
            if let ChainAct::Lookup {
                dst,
                setter: wanted,
            } = act
            {
                if wanted == setter {
                    self.add_tok(dst, f);
                }
            }
        }
        let visits: Vec<_> = visits
            .into_iter()
            .filter(|&(_, _, act)| !matches!(act, ChainAct::Lookup { .. }))
            .collect();
        if self.acc_groups.contains_key(&(holder, k)) {
            self.group_connect(holder, k, f, setter);
        } else {
            let any_exists = self.acc_groups.contains_key(&(holder, Key::any()));
            if (replay && !visits.is_empty()) || any_exists {
                // Creating the group picks up every accessor there (this
                // one too) and links it under an existing Any group.
                self.acc_group(holder, k);
            }
            if replay {
                for (origin, _, act) in visits {
                    self.group_hit(origin, holder, k, act);
                }
            }
        }
        self.leave_key(holder, key);
        self.add_accessor_atom(key);
        if replay && !self.acc_groups.contains_key(&(holder, Key::any())) {
            let walks = self
                .any_walk_holders
                .get(&holder)
                .cloned()
                .unwrap_or_default();
            for (origin, wk, act) in walks {
                if self.shape_splits(holder, wk).is_some() {
                    if wildcard || self.key_may_name(wk, key) {
                        self.acc_group(holder, k);
                        self.group_hit(origin, holder, k, act);
                    }
                } else {
                    self.acc_group(holder, Key::any());
                    self.group_hit(origin, holder, Key::any(), act);
                }
            }
        }
    }

    /// A computed-key lookup `kk` that reached `holder` may name accessor
    /// key `a`: always, unless its shape splits the holder and excludes `a`.
    fn shape_admits(&self, holder: Tok, kk: Key, a: Atom) -> bool {
        self.shape_splits(holder, kk).is_none() || self.key_may_name(kk, a)
    }

    /// Reuse recorded visits and leave nodes to start the walks that were
    /// not needed when those reads/stores first arrived (`add_accessor_atom`).
    fn deferred_walks(&mut self, key: Atom) {
        let wildcard = self.atoms[key as usize] == crate::STAR_KEY;
        let numeric = self.numeric_accessor_key(key);
        let num = self.atom(NUM_KEY);
        let mut pending = Vec::new();
        for (&t, visits) in &self.visits {
            for &(origin, k, act) in visits {
                let numeric_read = k == Key::Named(num) && numeric;
                if !matches!(act, ChainAct::Lookup { .. })
                    && (wildcard || k.is_any() || k == Key::Named(key) || numeric_read)
                {
                    pending.push((origin, t, if numeric_read { Key::any() } else { k }, act));
                }
            }
        }
        // Hash-map order must not choose the worklist's first-arrival path.
        pending.sort_unstable_by_key(|&(origin, t, k, act)| {
            let k = match k {
                Key::Named(a) => (0, a),
                Key::Any(s) => (1, s),
            };
            let act = match act {
                ChainAct::Load(n, _) => (0, n),
                ChainAct::Store(n, _) => (1, n),
                ChainAct::Call(c) => (2, c),
                ChainAct::Lookup { dst, setter } => (3 + u8::from(setter), dst),
            };
            (origin, t, k, act)
        });
        for (origin, t, k, act) in pending {
            self.accessor_walk(origin, t, k, act);
        }
        let mut leaves: Vec<_> = self.leave_loads.iter().map(|(&t, &n)| (t, n)).collect();
        leaves.sort_unstable();
        for (t, n) in leaves {
            self.leave_walk(t, n);
        }
    }

    /// Bind `call` to every function that shows up in `callee`, with `this`
    /// fixed to `origin` (the relation `callthis callee call origin`).
    ///
    /// The rule splits in two so a megamorphic site does not pay
    /// receivers × methods per call: `this` binds once per (method node,
    /// receiver), shared by every site that looks the same key up on that
    /// receiver (`ThisTo`), and arguments/result bind once per (call,
    /// function) through the call's relay node (`CallArgs`). A builtin
    /// namespace value runs per (call, receiver) as before (`BuiltinCallee`).
    /// The result is the same as one `CallThis` per (callee, call, origin).
    pub(super) fn bind_via(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        let pc = self.prov_call;
        if self.prov.is_some() {
            self.prov_call = call + 1;
        }
        self.bind_via_bound(callee, call, origin);
        self.prov_call = pc;
    }

    pub(super) fn bind_via_bound(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        if !self.callthis_done.insert((callee, call, origin)) {
            return;
        }
        self.rel(|| format!("callthis {callee} {call} {origin}"));
        self.via_calls
            .entry((callee, origin))
            .or_default()
            .push(call);
        if self.this_done.insert((callee, origin)) {
            self.cons(callee, Cons::ThisTo { origin });
        } else {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.charge_work(toks.len() as u64);
            for &t in &toks {
                // Each token's binding replays rules: the bound holds here.
                if self.past_work_bound() {
                    break;
                }
                self.this_calls(t, origin, &[call]);
            }
            self.bufs.push(toks);
        }
        let relay = match self.relays.get(&call) {
            Some(&r) => r,
            None => {
                let r = self.node_l("callee-relay");
                self.nodes[r as usize].frame = self.call_run(call);
                self.relays.insert(call, r);
                self.cons(r, Cons::CallArgs { call });
                r
            }
        };
        if self.relay_done.insert((call, callee)) {
            self.edge_quiet(callee, relay);
        }
        let sites = self.callee_sites.entry(callee).or_default();
        let first = sites.is_empty();
        sites.push((call, origin));
        if first {
            self.cons(callee, Cons::BuiltinCallee);
        } else if self.builtin_callee.contains(&callee) {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.charge_work(toks.len() as u64);
            for &t in &toks {
                if self.past_work_bound() {
                    break;
                }
                if self.per_receiver_callee(t) {
                    self.call_tok(call, t, Some(origin));
                }
            }
            self.bufs.push(toks);
        }
    }

    /// The `this` half of `call_tok(_, t, Some(origin))`.
    pub(super) fn this_to(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        match self.toks[t as usize] {
            // It runs per (call, receiver) (`BuiltinCallee`).
            TokKind::BuiltinFn(Native::Member { .. }) => {}
            TokKind::Top | TokKind::BuiltinFn(_) => {
                if let Some(d) = self.diagnostic.as_mut() {
                    let n = d.cause_node;
                    for &c in calls {
                        d.unknown_calls.entry(c).or_insert((n, t == TOP));
                        d.unknown_callee.entry(c).or_insert(t);
                    }
                }
                let r = if self.explain {
                    let fs: Vec<String> = calls
                        .iter()
                        .map(|&c| {
                            let f = self.node_fn[self.calls[c as usize].ret as usize];
                            let p = if self.protocol_calls.contains(&c) {
                                " protocol"
                            } else {
                                ""
                            };
                            let name = self
                                .funcs
                                .get(f as usize)
                                .map_or("(no function)", |f| f.name.as_str());
                            format!("{name}#{c}{p}")
                        })
                        .collect();
                    let why = format!(
                        "receiver of a call of an unknown function value ({} callee, receiver {}, calls [{}])",
                        self.callee_name(t),
                        self.tok_name(origin),
                        fs.join(", ")
                    );
                    self.reason(&why)
                } else if self.diagnostic.is_some() {
                    let cs: Vec<String> = calls.iter().map(|c| c.to_string()).collect();
                    self.reason(&format!(
                        "receiver of a call of an unknown function value [call {}]",
                        cs.join(",")
                    ))
                } else {
                    self.reason("receiver of a call of an unknown function value")
                };
                if self.keysnap.is_some() {
                    if let Some(&c) = calls.first() {
                        self.keysnap_note(c, t, origin);
                    }
                }
                self.escape(origin, r);
            }
            TokKind::Func(f, _) => {
                if !self.funcs[f as usize].lexical_this {
                    self.this_lookup(t, origin);
                }
            }
            _ => {}
        }
    }

    /// The instance half of `this_to`: each call bound through the method
    /// node binds its instance of `t` with this receiver.
    pub(super) fn this_calls(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            if !self.funcs[f as usize].lexical_this && self.summarized(f) {
                for &call in calls {
                    self.this_into(call, t, Some(origin));
                }
            }
        }
    }

    /// The argument half of `call_tok(call, t, Some(_))` (builtin namespace
    /// values excepted: they run per receiver, `BuiltinCallee`).
    pub(super) fn call_args(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        match self.toks[t as usize] {
            TokKind::BuiltinFn(Native::Member { .. }) => {}
            TokKind::Top | TokKind::BuiltinFn(_) => {
                if self.bind_done.insert((call, t, u32::MAX - 2)) {
                    self.unknown_callee_args(call, t, false);
                }
            }
            TokKind::Func(_, _) => self.bind_args(call, t),
            TokKind::Bound(_) => {
                if self.bind_done.insert((call, t, u32::MAX)) {
                    self.call_bound(call, t);
                }
            }
            // A bare marker under a key of this unit's object (`o.k()` with
            // `k` holding a parameter's own marker, DECISIONS 119): both
            // halves at once. `call_tok` records the call for the caller's
            // unit with the receiver described as its `this` (DECISIONS
            // 140), or escapes marker, receiver and arguments as a call of
            // an unknown function value, as whole does. (`this_to` has no
            // arm for a marker.)
            _ if self.bare_marker(t) => self.call_tok(call, t, None),
            _ => {}
        }
    }
}
