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
            },
        );
        self.define_accessors(d, o, key);
    }

    fn define_accessors(&mut self, d: Tok, o: NodeId, key: Atom) {
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
                key: Key::Any,
                src: value,
                site,
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
