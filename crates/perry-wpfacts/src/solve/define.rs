//! `Object.defineProperties(o, map)` key by key: the keys of a map are the
//! keys of its literal templates. A literal whose keys are exactly its birth
//! keys (and its methods) defines each of them by the descriptor under it, a
//! named definition; only a map with a computed key, a key added after birth,
//! or an unknown value defines under a computed key, as every map did before.
//! Whether a literal stays exact is known once the worklist is empty
//! (`settle_define`): one that did not defines under a computed key too.

use crate::model::*;
use crate::pts::FxHashMap;

/// Keys stored into each template after birth (`settle_define`).
type Late = FxHashMap<TmplId, Vec<Atom>>;

impl Model {
    /// dst ⊇ the value of `key` read on `t` (getters run with `this` = `t`).
    fn tok_load(&mut self, t: Tok, key: Key) -> NodeId {
        let dst = self.node_l("define-key");
        self.load_from(t, t, key, dst);
        dst
    }

    /// The own keys of `t` when they are exactly a literal's: not escaped,
    /// not HARD, not another unit's value, with no key added after birth or
    /// stored under a computed key, no computed member and no `__proto__`;
    /// each a key generation named (`Model::accessor_atoms`: lookups walk
    /// accessors only for those). `late`: the keys stored into each template.
    fn exact_keys(&self, t: Tok, late: Option<&Late>) -> Option<Vec<Atom>> {
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return None;
        };
        let tpl = &self.tmpls[tm as usize];
        let literal = matches!(
            tpl.kind,
            TmplKind::Lit { .. }
                | TmplKind::Class {
                    literal_shape: true,
                    ..
                }
        );
        if !literal
            || tpl.has_computed_members
            || self.is_marker(t)
            || self.is_escaped(t)
            || self.hard.contains_key(&t)
            || self.adds_keys.get(&tm).is_some_and(|&n| n > 0)
            || self.any_writers.contains_key(&tm)
        {
            return None;
        }
        let mut keys: Vec<Atom> = Vec::new();
        // The literal's methods are its own keys, not its prototype's
        // `constructor`.
        let methods = tpl
            .proto_members
            .iter()
            .filter(|&&a| self.atoms[a as usize] != "constructor");
        for &a in tpl
            .birth_keys
            .iter()
            .chain(&tpl.accessor_keys)
            .chain(methods)
        {
            if self.atoms[a as usize] == "__proto__" || !self.accessor_atoms.contains(&a) {
                return None;
            }
            if !keys.contains(&a) {
                keys.push(a);
            }
        }
        if late.is_some_and(|l| {
            l.get(&tm)
                .is_some_and(|ws| ws.iter().any(|a| !keys.contains(a)))
        }) {
            return None;
        }
        Some(keys)
    }

    /// `d` is an accessor descriptor literal (FVA 4.3a): its exact keys are
    /// within get/set/enumerable/configurable and name a getter or a setter
    /// (no value, no writable), with no own getter.
    fn accessor_desc(&self, d: Tok, late: Option<&Late>) -> bool {
        let Some(keys) = self.exact_keys(d, late) else {
            return false;
        };
        let TokKind::Obj(tm, _) = self.toks[d as usize] else {
            return false;
        };
        let names: Vec<&str> = keys
            .iter()
            .map(|&a| self.atoms[a as usize].as_str())
            .collect();
        self.tmpls[tm as usize].accessor_keys.is_empty()
            && names
                .iter()
                .all(|k| matches!(*k, "get" | "set" | "enumerable" | "configurable"))
            && names.iter().any(|k| matches!(*k, "get" | "set"))
    }

    /// Define `key` of `o` by accessor descriptor `d`.
    fn define_by_accessor(&mut self, d: Tok, o: NodeId, key: Atom, reason: u32) {
        if self.define_on.insert((o, key, false)) {
            self.cons(
                o,
                Cons::DefineKey {
                    key,
                    data: false,
                    reason,
                },
            );
        }
        let va = self.atom("value");
        self.tok_load(d, Key::Named(va));
        self.define_accessors(d, o, key);
    }

    /// Define `key` of `o` by descriptor `d` as data: its value stored under
    /// `key` (a descriptor of any other shape may still name get/set).
    fn define_by_data(&mut self, d: Tok, o: NodeId, key: Atom, site: SiteId, reason: u32) {
        if !self.desc_data.insert((o, key, d)) {
            return;
        }
        if self.define_on.insert((o, key, true)) {
            self.cons(
                o,
                Cons::DefineKey {
                    key,
                    data: true,
                    reason,
                },
            );
        }
        let va = self.atom("value");
        let v = self.tok_load(d, Key::Named(va));
        self.cons(
            o,
            Cons::Store {
                key: Key::Named(key),
                src: v,
                site,
                region: self.thrown,
            },
        );
        self.define_accessors(d, o, key);
    }

    fn define_accessors(&mut self, d: Tok, o: NodeId, key: Atom) {
        // DECISIONS 242(2): ToPropertyDescriptor reads all six descriptor
        // keys, which no descriptor owns at once (get and value exclude
        // each other): some read reaches its chain. Each read is a [[Get]]
        // (`DESCRIPTOR_KEYS`): `value` is read by the caller, get and set
        // below, the three flags here.
        for name in DESCRIPTOR_KEYS {
            let a = self.atom(name);
            self.note_chain_miss(d, Key::Named(a));
            if matches!(name, "enumerable" | "configurable" | "writable") {
                self.tok_load(d, Key::Named(a));
            }
        }
        for (name, setter) in [("get", false), ("set", true)] {
            let a = self.atom(name);
            let f = self.tok_load(d, Key::Named(a));
            self.cons(
                f,
                Cons::Accessor {
                    holder: o,
                    key,
                    setter,
                },
            );
        }
    }

    /// GLOBK3 (`Cons::DefineAnyKey`): `Object.defineProperty(o, k, d)` under
    /// a computed key, decided again as `o`, `k` or the setter grows.
    /// On the global object (ECMA-262 20.1.2.4 -> 7.3.8 -> 10.1.6.3
    /// ValidateAndApplyPropertyDescriptor; 9.1.1.4: the global object is
    /// an ordinary object whose properties are the global bindings), with a
    /// key proven never a Symbol (ToPropertyKey 7.1.19 of a primitive other
    /// than a Symbol is a String; an object key keeps the old path) and no
    /// setter: the define creates or replaces one string-named own
    /// property of the global object and writes nothing else. Its `value`,
    /// and what a call of its `get` with `this` = the global object
    /// returns (a [[Get]] of an accessor, 10.1.8.1 OrdinaryGet step 7),
    /// join the global's define field (`global_def_field`), which every
    /// global binding read takes beside its original (a define that fails
    /// or comes later keeps the original). `undefined` / `NaN` / `Infinity`
    /// are non-configurable, non-writable (19.1): the define throws a
    /// TypeError (`global_any_names` leaves them out). `__proto__` is no
    /// exception: [[DefineOwnProperty]] never runs the `__proto__` setter.
    /// Any other receiver, a key that may be a Symbol or an object, a
    /// setter, unit mode, or G2 off: E5 HARD on `o`, `value` under any key
    /// and the computed-key accessors (`define_any_old`), as before.
    pub(crate) fn define_computed_key(&mut self, d: DefineAny) {
        let kn = &self.nodes[d.key as usize];
        // A key node with no value yet: no define has run (the rule runs
        // again when it grows). Deciding now would take the computed path
        // before the first String constant arrives.
        if kn.prim == 0 && kn.pts.is_empty() {
            return;
        }
        // A value call (`desc`): a key of String constants defines each by
        // name, as the direct call does; any other key the computed-key path
        // on every receiver, as before (the named defines kept beside it).
        if d.desc != NO_NODE {
            match self.const_str_keys(d.key) {
                Some(atoms) => {
                    for a in atoms {
                        if self.global_key_def_done.insert((d.id(), a)) {
                            let c = Cons::DefineDesc {
                                o: d.o,
                                key: a,
                                site: d.site,
                                reason: d.reason,
                            };
                            self.cons(d.desc, c);
                        }
                    }
                }
                None => {
                    let star = self.atom(crate::STAR_KEY);
                    self.define_on.insert((d.o, star, true));
                    for t in self.nodes[d.o as usize].pts.to_sorted() {
                        self.define_any_old(d, t);
                    }
                }
            }
            return;
        }
        let str_key = kn.pts.is_empty() && kn.prim & prim::SYMBOL == 0;
        // A `set` that is not callable (nor undefined) makes the define
        // throw (ToPropertyDescriptor 6.2.6.5 step 15.b): no define.
        let no_set = !self.nodes[d.set as usize]
            .pts
            .to_sorted()
            .into_iter()
            .any(|t| {
                t == TOP
                    || matches!(self.toks[t as usize], TokKind::Func(..))
                    || crate::builtins::accessor_callable(self, t)
            });
        let ok = str_key && no_set && !self.unit_mode && !self.global_any_off;
        // E5 exact key set: a key that is one of finitely many String
        // constants defines exactly those keys (each as a named define does).
        let exact = match no_set && !self.unit_mode {
            true => self.exact_global_keys(d.key),
            false => None,
        };
        // Per receiver: `o` may also hold what a read of `globalThis`
        // yields after such a define (`globalThis` is configurable).
        let mut glob = None;
        for t in self.nodes[d.o as usize].pts.to_sorted() {
            match crate::builtins::global_object(self, t) && (ok || exact.is_some()) {
                true => glob = Some(t),
                false => self.define_any_old(d, t),
            }
        }
        let Some(g) = glob else {
            return;
        };
        if let Some(atoms) = exact {
            self.define_global_keys(d, g, &atoms);
            return;
        }
        if !self.global_def_done.insert(d.id()) {
            return;
        }
        self.global_defs.push((d, g));
        let gd = crate::builtins::global_def_field(self);
        if !d.accessor {
            self.edge(d.value, gd);
        }
        // An accessor without `get` reads `undefined` (10.1.8.1 step 6).
        self.add_prim(gd, prim::UNDEF);
        let ret = self.node_l("global-define-get-ret");
        self.edge(ret, gd);
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Tok(g),
            args: vec![],
            spread: vec![],
            ret,
            site: call,
            region: self.thrown,
        });
        self.why_mint(call, Some(g), u32::MAX);
        self.cons(d.get, Cons::Call { call });
    }

    /// E5 exact key set: the keys a define's key node `k` may be, when it
    /// holds only String constants, finitely many (`Node::strs`, exact:
    /// `STRS_K` is unbounded). ToPropertyKey of a String is that String
    /// (ECMA-262 7.1.19), so the define names one of them (20.1.2.4 step 2).
    /// `navigator` keeps the computed path (its reads take the define field,
    /// `global_def_nav_read`); any other node: `None`, a computed key. The
    /// node only grows: a later non-constant makes the define computed
    /// (`define_computed_key` runs again), the named writes kept beside it.
    fn exact_global_keys(&self, k: NodeId) -> Option<Vec<Atom>> {
        let atoms = self.const_str_keys(k)?;
        let named = |a: &Atom| {
            let n = &self.atoms[*a as usize];
            n != "navigator" && n != crate::STAR_KEY
        };
        atoms.iter().all(named).then_some(atoms)
    }

    /// The keys key node `k` may be when it holds only String constants,
    /// finitely many (`Node::strs`, exact: `STRS_K` is unbounded); else `None`.
    pub(super) fn const_str_keys(&self, k: NodeId) -> Option<Vec<Atom>> {
        let kn = &self.nodes[k as usize];
        if !kn.pts.is_empty() || kn.prim != prim::STR {
            return None;
        }
        if kn.strs & !crate::model::STRS_RUN < crate::model::STRS_SET0 {
            return None;
        }
        Some(self.str_atoms(kn.strs).to_vec())
    }

    /// E5 exact key set (`Cons::DefineAnyKey` whose key is one of finitely
    /// many String constants, no setter, whole program): on the global
    /// object (9.1.1.4) the define creates or replaces the own property
    /// under one of `atoms` and writes no other key (10.1.6.3
    /// ValidateAndApplyPropertyDescriptor). Each key's global field takes
    /// the descriptor's `value` and what a call of its `get` with `this` =
    /// the global object returns, as a named define does; no any-key write.
    /// `undefined` / `NaN` / `Infinity` are non-configurable (19.1): the
    /// define throws a TypeError and writes nothing there.
    fn define_global_keys(&mut self, d: DefineAny, g: Tok, atoms: &[Atom]) {
        let ret = match self.global_key_def_ret.get(&d.id()) {
            Some(&r) => r,
            None => {
                let ret = self.node_l("global-define-named-get-ret");
                self.global_key_def_ret.insert(d.id(), ret);
                let call = self.calls.len() as CallId;
                self.calls.push(CallSite {
                    first_arg_fixed: false,
                    this: ThisArg::Tok(g),
                    args: vec![],
                    spread: vec![],
                    ret,
                    site: call,
                    region: self.thrown,
                });
                self.why_mint(call, Some(g), u32::MAX);
                self.cons(d.get, Cons::Call { call });
                ret
            }
        };
        for &a in atoms {
            if matches!(
                self.atoms[a as usize].as_str(),
                "undefined" | "NaN" | "Infinity"
            ) {
                continue;
            }
            if !self.global_key_def_done.insert((d.id(), a)) {
                continue;
            }
            let f = crate::builtins::global_field(self, g, a);
            if !d.accessor {
                self.edge(d.value, f);
            }
            // An accessor without `get` reads `undefined` (10.1.8.1 step 6).
            self.add_prim(f, prim::UNDEF);
            self.edge(ret, f);
        }
    }

    /// GLOBK3: the old path of a computed-key define for receiver `t`: the
    /// define's holder node (one per define) takes `t`; on it E5 HARD, the
    /// `value` under any key, the get / set installed under the
    /// computed-key star, as `o` had before.
    pub(crate) fn define_any_old(&mut self, d: DefineAny, t: Tok) {
        if let Some(&h) = self.define_any_old.get(&d.id()) {
            self.add_tok(h, t);
            return;
        }
        let h = self.node_l("define-any-holder");
        self.define_any_old.insert(d.id(), h);
        self.add_tok(h, t);
        self.cons(
            h,
            Cons::Hard {
                kind: Hard::E5DefineProperty,
                reason: d.reason,
            },
        );
        if !d.accessor {
            self.cons(
                h,
                Cons::Store {
                    key: Key::any(),
                    src: d.value,
                    site: d.site,
                    region: self.thrown,
                },
            );
        }
        let star = self.atom(crate::STAR_KEY);
        for (f, setter) in [(d.get, false), (d.set, true)] {
            self.cons(
                f,
                Cons::Accessor {
                    holder: h,
                    key: star,
                    setter,
                },
            );
        }
    }

    /// Map `t` defines its own values under a computed key: they join
    /// `descs` (whose get/set are installed under `*` and whose value is
    /// `value`, by generation), and `o` is E5 HARD with `value` stored under
    /// any key (once per `descs`).
    pub(super) fn define_any_key(
        &mut self,
        t: Tok,
        o: NodeId,
        descs: NodeId,
        value: NodeId,
        site: SiteId,
        reason: u32,
    ) {
        if !self.define_any.insert((descs, t)) {
            return;
        }
        self.own_values_of(t, descs);
        self.define_any_on(o, descs, value, site, reason);
    }

    pub(super) fn define_any_on(
        &mut self,
        o: NodeId,
        descs: NodeId,
        value: NodeId,
        site: SiteId,
        reason: u32,
    ) {
        if !self.define_any_on.insert(descs) {
            return;
        }
        self.cons(
            o,
            Cons::Hard {
                kind: Hard::E5DefineProperty,
                reason,
            },
        );
        self.cons(
            o,
            Cons::Store {
                key: Key::any(),
                src: value,
                site,
                region: self.thrown,
            },
        );
    }

    /// `Cons::DefineAll` on map `t`: each key by its descriptor, or every
    /// own value under a computed key.
    pub(super) fn define_all(
        &mut self,
        t: Tok,
        o: NodeId,
        descs: NodeId,
        value: NodeId,
        site: SiteId,
        reason: u32,
    ) {
        let Some(keys) = self.exact_keys(t, None) else {
            return self.define_any_key(t, o, descs, value, site, reason);
        };
        for k in keys {
            let d = self.tok_load(t, Key::Named(k));
            self.cons(
                d,
                Cons::DefineDesc {
                    o,
                    key: k,
                    site,
                    reason,
                },
            );
        }
        self.define_runs.push((t, o, descs, value, site, reason));
    }

    /// `Cons::DefineDesc`: define `key` of `o` by descriptor `d`.
    pub(super) fn define_desc(&mut self, d: Tok, o: NodeId, key: Atom, site: SiteId, reason: u32) {
        if self.accessor_desc(d, None) {
            self.define_by_accessor(d, o, key, reason);
            self.desc_runs.push((d, o, key, site, reason));
        } else {
            self.define_by_data(d, o, key, site, reason);
        }
    }

    /// Once the worklist is empty: a map defined key by key, or an accessor
    /// descriptor, that gained a key after birth (or escaped, or turned HARD)
    /// defines under a computed key, or as data, from now on too. True when
    /// one did.
    pub(crate) fn settle_define(&mut self) -> bool {
        if self.define_runs.is_empty() && self.desc_runs.is_empty() {
            return false;
        }
        let tms: crate::pts::FxHashSet<TmplId> = self
            .define_runs
            .iter()
            .map(|r| r.0)
            .chain(self.desc_runs.iter().map(|r| r.0))
            .filter_map(|t| self.tmpl_of(t))
            .collect();
        let mut late: Late = FxHashMap::default();
        for &(tm, a) in self.writers.keys().chain(self.proto_writers.keys()) {
            if tms.contains(&tm) {
                late.entry(tm).or_default().push(a);
            }
        }
        let mut more = false;
        for (t, o, descs, value, site, reason) in self.define_runs.clone() {
            if !self.define_any.contains(&(descs, t)) && self.exact_keys(t, Some(&late)).is_none() {
                self.define_any_key(t, o, descs, value, site, reason);
                more = true;
            }
        }
        for (d, o, key, site, reason) in self.desc_runs.clone() {
            if !self.desc_data.contains(&(o, key, d)) && !self.accessor_desc(d, Some(&late)) {
                self.define_by_data(d, o, key, site, reason);
                more = true;
            }
        }
        more
    }
}
