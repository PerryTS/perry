//! JSON-RT: `JSON.parse(JSON.stringify(x))` with no replacer function and
//! no reviver (`gen_rest/builtin_objects.rs`, `json_rt_operands`) is a
//! fresh tree mirroring `x`. ECMA-262 25.5.2.2 SerializeJSONProperty: the
//! value after `toJSON(key)` (step 2); `null`, Booleans, Strings as they
//! are; a Number as itself or `null` when not finite (step 9); a BigInt
//! throws (step 10); an object that is not callable is serialized by
//! SerializeJSONArray (IsArray) or SerializeJSONObject; anything else
//! (`undefined`, a symbol, a function) is `undefined`: an object member
//! omitted (25.5.2.5 step 5-8), an array element `"null"` (25.5.2.6 step
//! 8.b), and at the root a text `JSON.parse` rejects. SerializeJSONObject
//! lists EnumerableOwnProperties(value, key) (own enumerable String keys);
//! SerializeJSONArray the indices below `length`. 25.5.1 ParseJSON then
//! creates exactly those properties on fresh objects and arrays
//! (CreateDataProperty, 25.5.1.1).
//!
//! The model: per (parse site, source token) one copy token (a clone of the
//! site's `json` object or Array template), whose key `a` exists iff the
//! source holds an own enumerable key `a` (`field_keys` with `enum_own`;
//! keys gained later replay through `spread_replay`), and whose values are
//! the rule applied to the source's values there (`Cons::JsonCopy`, per
//! arriving value: monotone). The source's values under keys no store
//! named (`field_any`, shaped computed stores) go to the copy's any-key
//! node. A value whose own properties the model does not hold (⊤, a
//! marker, a Proxy, an opaque builtin object, an escaped object's unknown
//! writes) has unknown content: the site's any-JSON value (the tree
//! `gen/glue.rs` `json_value` makes for a text of unknown origin).
//! No positions: this is not the JSON position model (`json_pos`).
use super::*;

/// The position of a serialized value (`Cons::JsonCopy::at`).
pub(crate) const JSON_AT_ROOT: u8 = 0;
pub(crate) const JSON_AT_PROP: u8 = 1;
pub(crate) const JSON_AT_ELEM: u8 = 2;

/// The primitives `JSON.parse` gives back for primitives `p` serialized at
/// `at` (25.5.2.2 steps 5-10).
fn json_rt_prims(p: u16, at: u8) -> u16 {
    let mut out = p & (prim::STR | prim::BOOL | prim::NULL);
    if p & prim::NUM != 0 {
        out |= prim::NUM | prim::NULL;
    }
    if at == JSON_AT_ELEM && p & (prim::UNDEF | prim::SYMBOL) != 0 {
        out |= prim::NULL;
    }
    out
}

impl Model {
    /// The node of what `toJSON` returns on `t` (the `Protocol::Json`
    /// touch's result, `Model::implicit`): one per token.
    pub(crate) fn json_rt_ret(&mut self, t: Tok) -> NodeId {
        if let Some(&n) = self.json_rt_ret.get(&t) {
            return n;
        }
        let n = self.node_named("implicit-ret", || format!("implicit-ret({t})"));
        self.json_rt_ret.insert(t, n);
        n
    }

    /// `Cons::JsonCopy` on node values `toks` / `prim`: what the parse
    /// holds at the place `dst` for them.
    pub(crate) fn json_rt_apply(
        &mut self,
        dst: NodeId,
        site: Tok,
        arr: Tok,
        at: u8,
        toks: Tokens<'_>,
        prim: u16,
    ) {
        let p = json_rt_prims(prim, at);
        if p != 0 {
            self.add_prim(dst, p);
        }
        for &t in toks {
            let unknown = t == TOP
                || self.is_marker(t)
                || matches!(self.toks[t as usize], TokKind::Proxy(_))
                || !self.own_props_modelled(t);
            if unknown {
                let (root, inner) = self.json_rt_loose(site, arr);
                self.edge(if at == JSON_AT_ROOT { root } else { inner }, dst);
                continue;
            }
            // Step 2: `toJSON(key)`'s result is serialized in its place.
            let r = self.json_rt_ret(t);
            self.cons(r, Cons::JsonCopy { dst, site, arr, at });
            if self.is_callable_tok(t) {
                if at == JSON_AT_ELEM {
                    self.add_prim(dst, prim::NULL);
                }
                continue;
            }
            let c = self.json_rt_copy(site, arr, t);
            self.add_tok(dst, c);
        }
    }

    /// The copy of object `t` made by parse site `site`.
    fn json_rt_copy(&mut self, site: Tok, sarr: Tok, t: Tok) -> Tok {
        if let Some(&c) = self.json_rt_copy.get(&(site, t)) {
            return c;
        }
        let arr = self.is_arr(t);
        let like = match arr {
            true => sarr,
            false => site,
        };
        let TokKind::Obj(tm, ctx) = self.toks[like as usize] else {
            unreachable!("a JSON-RT site is an object token")
        };
        let d = format!("{}~rt{t}", self.tmpls[tm as usize].display);
        let c = self.json_clone(like, ctx, d);
        self.json_rt_copy.insert((site, t), c);
        self.json_rt_to.entry(t).or_default().push((c, site, sarr));
        self.rel(|| format!("jsonrt {c} of {t} site {site}"));
        if arr {
            // SerializeJSONArray: the elements, a hole `null`.
            let ca = self.field_any(c);
            let fa = self.field_any(t);
            let h = self.hole(t);
            for n in [fa, h] {
                self.cons(n, Cons::JsonCopy { dst: ca, site, arr: sarr, at: JSON_AT_ELEM });
            }
            for (_, n, store) in self.shaped_of.get(&t).cloned().unwrap_or_default() {
                if store {
                    self.cons(n, Cons::JsonCopy { dst: ca, site, arr: sarr, at: JSON_AT_ELEM });
                }
            }
            return c;
        }
        let getters = self.json_rt_getters(t);
        for a in self.field_keys.get(&t).cloned().unwrap_or_default() {
            let f = self.field_r(t, a);
            if self.enum_own(t, f, a) {
                self.json_rt_key(t, a, f, c, site, sarr, getters);
            }
        }
        let ca = self.any_key_node(c);
        let fa = self.field_any(t);
        self.cons(fa, Cons::JsonCopy { dst: ca, site, arr: sarr, at: JSON_AT_PROP });
        for (_, n, store) in self.shaped_of.get(&t).cloned().unwrap_or_default() {
            if store {
                self.cons(n, Cons::JsonCopy { dst: ca, site, arr: sarr, at: JSON_AT_PROP });
            }
        }
        if getters {
            // An own accessor under a key no store named: its [[Get]].
            let v = self.json_rt_get(t, Key::any());
            self.cons(v, Cons::JsonCopy { dst: ca, site, arr: sarr, at: JSON_AT_PROP });
        }
        if self.is_escaped(t) {
            self.json_rt_escaped(t);
        }
        c
    }

    /// May `t` have an own accessor (its getter runs in the serialization's
    /// [[Get]])?
    fn json_rt_getters(&self, t: Tok) -> bool {
        let own = self.accessor_keys.get(&t).is_some_and(|k| !k.is_empty());
        own || matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if !self.tmpls[tm as usize].accessor_keys.is_empty())
    }

    /// The [[Get]] of `key` on `t` (getters run).
    fn json_rt_get(&mut self, t: Tok, key: Key) -> NodeId {
        let recv = self.node_l("json-rt-recv");
        let dst = self.node_l("json-rt-get");
        self.cons(recv, Cons::Load { key, dst });
        self.add_tok(recv, t);
        dst
    }

    /// Own enumerable key `a` of `t` (field `f`): copy `c` has it, holding
    /// the serialized values.
    #[allow(clippy::too_many_arguments)]
    fn json_rt_key(
        &mut self,
        t: Tok,
        a: Atom,
        f: NodeId,
        c: Tok,
        site: Tok,
        sarr: Tok,
        getters: bool,
    ) {
        let name = &self.atoms[a as usize];
        if self.is_private_atom(a) || builtins::is_internal_key(name) || name.starts_with("@@") {
            return;
        }
        let src = match getters {
            true => self.json_rt_get(t, Key::Named(a)),
            false => f,
        };
        let cf = self.field(c, a);
        self.cons(src, Cons::JsonCopy { dst: cf, site, arr: sarr, at: JSON_AT_PROP });
    }

    /// Key `a` of `t` (field `f`) became an own enumerable key: every copy
    /// of `t` has it (`spread_replay`).
    pub(super) fn json_rt_replay(&mut self, t: Tok, a: Atom, f: NodeId) {
        let Some(v) = self.json_rt_to.get(&t).cloned() else {
            return;
        };
        if self.is_arr(t) {
            return;
        }
        let getters = self.json_rt_getters(t);
        for (c, site, sarr) in v {
            self.json_rt_key(t, a, f, c, site, sarr, getters);
        }
    }

    /// A shaped computed store `n` of `t` made after its copies.
    pub(super) fn json_rt_shaped(&mut self, t: Tok, n: NodeId) {
        let Some(v) = self.json_rt_to.get(&t).cloned() else {
            return;
        };
        let arr = self.is_arr(t);
        for (c, site, sarr) in v {
            let (dst, at) = match arr {
                true => (self.field_any(c), JSON_AT_ELEM),
                false => (self.any_key_node(c), JSON_AT_PROP),
            };
            self.cons(n, Cons::JsonCopy { dst, site, arr: sarr, at });
        }
    }

    /// `t` escaped: unknown code may give it any key with any value, so
    /// every copy may hold any key with any JSON value.
    pub(super) fn json_rt_escaped(&mut self, t: Tok) {
        let Some(v) = self.json_rt_to.get(&t).cloned() else {
            return;
        };
        let arr = self.is_arr(t);
        for (c, site, sarr) in v {
            let (_, inner) = self.json_rt_loose(site, sarr);
            let dst = match arr {
                true => self.field_any(c),
                false => self.any_key_node(c),
            };
            self.edge(inner, dst);
        }
    }

    /// The any-JSON value of parse site `site` (`gen/glue.rs` `json_value`
    /// without positions or reviver), made on first use: the root's
    /// tokens and the nested ones (`json_inner`).
    fn json_rt_loose(&mut self, site: Tok, arr: Tok) -> (NodeId, NodeId) {
        if let Some(&l) = self.json_rt_loose.get(&site) {
            return l;
        }
        let (io, ia) = self.json_inner(site, arr);
        for o in [site, arr, io, ia] {
            let fa = self.field_any(o);
            self.add_prim(fa, prim::JSON);
            self.add_tok(fa, io);
            self.add_tok(fa, ia);
        }
        let root = self.node_l("json-rt-unknown");
        self.add_tok(root, site);
        self.add_tok(root, arr);
        self.add_prim(root, prim::JSON);
        let inner = self.node_l("json-rt-unknown");
        self.add_tok(inner, io);
        self.add_tok(inner, ia);
        self.add_prim(inner, prim::JSON);
        let l = (root, inner);
        self.json_rt_loose.insert(site, l);
        l
    }
}
