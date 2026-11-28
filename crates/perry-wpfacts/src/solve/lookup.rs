//! Property reads: `load_from` and the `__proto__`, plain-key and index
//! reads it routes, the accessor walk, and the chain predicates they share.

use crate::builtins;
use crate::model::*;

impl Model {
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

    pub fn is_proto_key(&self, key: Key) -> bool {
        matches!(key, Key::Named(a) if self.atoms[a as usize] == "__proto__")
    }

    /// dst ⊇ `t`'s prototypes as values (DECISIONS 217): what
    /// `Object.getPrototypeOf(t)` and `t.__proto__` return. A literal's
    /// class prototype is its layout, no object (`Cons::ProtoValues`);
    /// `ObjectProto` is %Object.prototype%, whose prototype is null (rule
    /// OPROTO); a module record's (`module.exports` or a namespace) may be null.
    pub fn proto_value(&mut self, t: Tok, dst: NodeId) {
        // A bound value's [[Prototype]] is its root target's
        // (BoundFunctionCreate, ECMA-262 10.4.1.3 step 1); its prototype
        // node holds the root's (`proto_node`).
        let r = builtins::bound_root(self, t);
        if !self.unit_mode && builtins::builtin_function(self, r) {
            let key = Key::Named(self.atom("__proto__"));
            self.builtin_field_use(r, key, ChainAct::Load(dst, self.thrown));
        }
        self.proto_value_at(t, dst);
        // A program function born with another intrinsic prototype
        // (`%AsyncGeneratorFunction.prototype%`, a generator object's
        // `%GeneratorPrototype%` chain) has one the model does not
        // describe: not `Function.prototype`, whose reads differ.
        if let TokKind::Func(f, _) = self.toks[r as usize] {
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
            // Rule OPROTO: %Object.prototype%.[[Prototype]] is null (ECMA-262
            // 20.1.3); `ObjectProto` is no other prototype.
            TokKind::ObjectProto => self.add_prim(dst, prim::NULL),
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
    /// (`enum_obj`), or the collapsed `ObjectProto` (or an intrinsic
    /// prototype `from` was born with, `Model::collapsed_end`) where the seed of
    /// `from` makes it such an intrinsic prototype
    /// (`builtins::enum_free_seed`), or a built-in instance whose own
    /// enumerable properties are its program-written fields and, for a
    /// typed array, its elements (`builtins::enum_free_link_obj`).
    pub(crate) fn enum_link(&self, from: Tok, q: Tok) -> bool {
        self.enum_free(q)
            || self.enum_obj(q)
            || builtins::enum_free_link_obj(self, q)
            || self.collapsed_end(from, q) && builtins::enum_free_seed(self, from)
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
}
