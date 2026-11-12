//! Virtual receivers: classes whose instances a token stands for, and the overrides, stores and escapes that reach them.

use super::*;

impl Model {
    /// `t`, a receiver token or a receiver marker, stands for instances of
    /// `classes` (DECISIONS 59): its prototypes are theirs, and a named load
    /// on it finds the chain's value; the own field (unknown for these
    /// tokens) only once the key may be an own key of such an instance
    /// (`virt_load`).
    pub(crate) fn register_virt(&mut self, t: Tok, classes: &[TmplId]) {
        let ps = self.virt_protos.entry(t).or_default();
        let fresh: Vec<TmplId> = classes
            .iter()
            .copied()
            .filter(|c| !ps.contains(c))
            .collect();
        ps.extend(fresh.iter().copied());
        for &c in &fresh {
            self.virt_by_class.entry(c).or_default().push(t);
        }
        if self.shadowed_classed.contains(&t) {
            for &c in &fresh {
                self.shadowed_by_class.entry(c).or_default().push(t);
            }
        }
        if !fresh.is_empty() {
            self.flush_shadowed(Some(t));
        }
        if let Some(&pn) = self.proto_node.get(&t) {
            for &p in &fresh {
                let pt = self.tok(TokKind::Proto(self.constructor_origin(p)));
                self.add_tok(pn, pt);
            }
        }
        // DECISIONS 231(b): an argument marker `t` joined before it was a
        // receiver stands for its classes too, if it stands for receivers
        // only (`virt_member`).
        if fresh.is_empty() || self.virt_tok_open.contains(&t) {
            return;
        }
        for r in self.virt_member_of.get(&t).cloned().unwrap_or_default() {
            if self.virt_protos.contains_key(&r) {
                self.register_virt(r, classes);
            } else if self.arg_members.get(&r).is_some_and(|ms| {
                ms.iter()
                    .all(|m| self.virt_protos.contains_key(m) && !self.virt_tok_open.contains(m))
            }) {
                let mut cs: Vec<TmplId> = Vec::new();
                for m in self.arg_members[&r].clone() {
                    cs.extend(self.virt_protos[&m].iter().copied());
                }
                self.register_virt(r, &cs);
            }
        }
    }

    /// The load node of key `a` on receiver token / marker `t` (`None`: `t`
    /// is neither). It holds the chain's values, and the own field once `a`
    /// may be an own key of an instance `t` stands for: the class code
    /// stores `a` (or a computed key) into one, uses a hard operation on one
    /// or lets one escape; a subclass of another unit defines `a` (census,
    /// `override_keys`); an accessor on the chain may answer it.
    pub(crate) fn virt_load(&mut self, t: Tok, a: Atom) -> Option<NodeId> {
        let classes = self.virt_protos.get(&t)?.clone();
        if let Some(&n) = self.virt_nodes.get(&(t, a)) {
            return Some(n);
        }
        let n = self.node_l("virt-load");
        self.virt_nodes.insert((t, a), n);
        self.virt_tok_keys.entry(t).or_default().push(a);
        // DECISIONS 234: an own definite data key of every instance shadows
        // the builtin prototypes; its value is the own field.
        let shadow = self.shadows(t, a);
        let cv = self.cv_of(t, Key::Named(a), !shadow);
        self.edge(cv, n);
        if shadow {
            self.note_shadowed(t, a, crate::model::ChainAct::Load(n), false);
        }
        // DECISIONS 231(c): the receiver token's own field is what this
        // unit's code stores under `a` into instances and receivers of the
        // classes and their subclasses (other units' store effects and ⊤
        // stores into escaped ones arrive as such stores); its field (⊤)
        // only where the key may hold a value no such store makes
        // (`virt_own`, a subclass of another unit defining it, a shadowed
        // key no store of this unit makes). A marker's own field stays its
        // keyed reach: the caller's own value, which receiver flows carry
        // relative to each caller (DECISIONS 179); the stored values beside
        // it would be this unit's tokens on every caller's object.
        let marker = self.is_marker(t);
        let mut stored = false;
        for &p in &classes {
            if let Some(&s) = self.virt_stored.get(&(p, a)) {
                if !marker {
                    self.edge(s, n);
                }
                stored = true;
            }
        }
        if self.virt_own(t, &classes, a)
            || !self.virt_overriders(t, a).is_empty()
            || (shadow && !stored)
            || (stored && marker)
        {
            let f = self.field_r(t, a);
            self.edge(f, n);
        }
        Some(n)
    }

    /// Key `a` may be an own key of the instances a receiver of `classes`
    /// stands for in this unit's view: the class code stores it, uses a
    /// hard operation on one or lets one escape, or an accessor on the
    /// chain may answer it.
    fn virt_own(&self, t: Tok, classes: &[TmplId], a: Atom) -> bool {
        self.virt_tok_open.contains(&t)
            || classes.iter().any(|&p| {
                self.virt_open.contains(&p)
                    || self.virt_revoked.contains(&(p, a))
                    || self.chain_accessor_key(p, a)
            })
    }

    /// The classes of receiver token / marker `t` that another unit's
    /// subclass overrides `a` of (census, `override_keys`).
    pub(crate) fn virt_overriders(&mut self, t: Tok, a: Atom) -> Vec<TmplId> {
        let star = self.atom(crate::STAR_KEY);
        let Some(classes) = self.virt_protos.get(&t) else {
            return Vec::new();
        };
        classes
            .iter()
            .copied()
            .filter(|p| {
                self.override_keys
                    .get(p)
                    .is_some_and(|v| v.contains(&a) || v.contains(&star))
            })
            .collect()
    }

    /// The callee of `t.a(...)` beyond the chain, on receiver token /
    /// marker `t` (DECISIONS 97): when `a` is own only because another
    /// unit's subclass defines it, the call runs that subclass's method on
    /// a subclass instance, i.e. the override token of each such class
    /// (`override_node`); else nothing (`virt_load` holds the own field).
    pub(crate) fn virt_call(&mut self, t: Tok, a: Atom) -> Vec<NodeId> {
        let classes = self.virt_protos.get(&t).cloned().unwrap_or_default();
        if self.virt_own(t, &classes, a) {
            return Vec::new();
        }
        let ps = self.virt_overriders(t, a);
        ps.into_iter().map(|p| self.override_node(p, a)).collect()
    }

    /// The node holding the override token of key `a` of published class
    /// `tm` (DECISIONS 97): the function another unit's subclass defines
    /// under `a`, as a call through a receiver of `tm` sees it. That unit
    /// runs it unseen: the arguments escape, the result is unknown, and its
    /// receiver is checked (`Cons::OverrideThis`).
    fn override_node(&mut self, tm: TmplId, a: Atom) -> NodeId {
        if let Some(&n) = self.override_fns.get(&(tm, a)) {
            return n;
        }
        let fx = self.funcs.len() as FnIdx;
        let tok = self.tok(TokKind::Func(fx, NO_CTX));
        let this = self.node_l("this(override)");
        let ret = self.node_l("ret(override)");
        let args_all = self.node_l("args(override)");
        let name = format!(
            "<override {} of {}>",
            self.atoms[a as usize], self.tmpls[tm as usize].display
        );
        self.funcs.push(FuncInfo {
            #[cfg(test)]
            body_packet: Default::default(),
            module: 0,
            func_id: u32::MAX,
            raise: u32::MAX,
            name,
            params: Vec::new(),
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: true,
            arguments: None,
            own_test: crate::model::OwnTestBody::Unknown,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_proto: None,
            gen_fn_protos: None,
            gen_async: false,
            gen_reyields: false,
            proto: None,
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded: false,
        });
        let r = self.reason("passed to an unknown function");
        self.sink_node(args_all, r);
        self.add_top(ret, |_| "result of an unknown function".to_string());
        self.add_prim(ret, prim::ALL);
        let rt = self.reason("receiver of a call of an unknown function value");
        self.cons(
            this,
            Cons::OverrideThis {
                tmpl: tm,
                key: a,
                reason: rt,
            },
        );
        let n = self.node_l("override-callee");
        self.add_tok(n, tok);
        self.override_fns.insert((tm, a), n);
        n
    }

    pub(super) fn chain_accessor_key(&self, tm: TmplId, a: Atom) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            let t = &self.tmpls[c as usize];
            if t.accessor_keys.contains(&a) || t.has_computed_members {
                return true;
            }
            cur = t.parent;
        }
        false
    }

    /// `t` gains own key `a` (`None`: any key): for each published class
    /// whose instances `t` is or stands for, the key is open on its
    /// receiver token and markers from now on (`virt_load`).
    pub(crate) fn virt_revoke(&mut self, t: Tok, key: Option<Atom>) {
        if self.virt_by_class.is_empty() {
            return;
        }
        for p in self.virt_classes(t) {
            self.virt_open_class(p, key);
        }
    }

    /// The published classes whose receivers may denote what `t` is or
    /// stands for: its classes (an instance's template, a receiver's
    /// classes) and their ancestors (a receiver of a class stands for its
    /// subclasses' instances too).
    fn virt_classes(&self, t: Tok) -> Vec<TmplId> {
        let mut work: Vec<TmplId> = match self.virt_protos.get(&t) {
            Some(ps) => ps.clone(),
            None => match self.tmpl_of(t) {
                Some(tm) => vec![tm],
                None => return Vec::new(),
            },
        };
        let mut out = Vec::new();
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            if self.virt_by_class.contains_key(&c) {
                out.push(c);
            }
            let tp = &self.tmpls[c as usize];
            work.extend(tp.parent);
            work.extend(tp.parent_links.iter().copied());
        }
        out
    }

    /// DECISIONS 231(c): this unit's code stores `src` under `a` into `t`:
    /// the receivers of the classes `t` may be an instance of read it as
    /// their own field (`virt_load`).
    pub(crate) fn virt_store(&mut self, t: Tok, a: Atom, src: NodeId) {
        if self.virt_by_class.is_empty() {
            return;
        }
        for p in self.virt_classes(t) {
            let s = match self.virt_stored.get(&(p, a)) {
                Some(&s) => s,
                None => {
                    let s = self.node_l("virt-stored");
                    self.virt_stored.insert((p, a), s);
                    for vt in self.virt_by_class.get(&p).cloned().unwrap_or_default() {
                        if let Some(&n) = self.virt_nodes.get(&(vt, a)) {
                            if self.is_marker(vt) {
                                let f = self.field_r(vt, a);
                                self.edge(f, n);
                            } else {
                                self.edge(s, n);
                            }
                        }
                    }
                    s
                }
            };
            self.edge(src, s);
        }
    }

    /// An instance or receiver `t` escaped: unknown code holds it, which per
    /// unit is other units' code too, and its writes through ⊤ reach it
    /// (`top_store`): each key stored into ⊤ may be any value on it.
    pub(crate) fn virt_escape(&mut self, t: Tok) {
        if self.virt_by_class.is_empty() {
            return;
        }
        for p in self.virt_classes(t) {
            if !self.virt_escaped.insert(p) {
                continue;
            }
            let ks: Vec<Option<Atom>> = self.top_stored.iter().copied().collect();
            for k in ks {
                self.virt_open_class(p, k);
            }
        }
    }

    /// A write through ⊤ under key `k` (`None`: computed, or a write of any
    /// key: a hard operation, an accessor, unmodeled code): on each escaped
    /// instance the key may hold any value.
    pub(crate) fn top_store(&mut self, k: Option<Atom>) {
        if !self.top_stored.insert(k) {
            return;
        }
        let ps: Vec<TmplId> = self.virt_escaped.iter().copied().collect();
        for p in ps {
            self.virt_open_class(p, k);
        }
    }

    /// Key `key` (`None`: every key) of class `p`'s instances may hold a
    /// value no store of this unit makes: its receivers read their own field.
    fn virt_open_class(&mut self, p: TmplId, key: Option<Atom>) {
        {
            if self.virt_open.contains(&p) {
                return;
            }
            let opened: Option<Atom> = match key {
                Some(a) => {
                    if !self.virt_revoked.insert((p, a)) {
                        return;
                    }
                    Some(a)
                }
                // The keys an instance gains take none away (`class_holds`
                // does not read `virt_open`): no shadow to re-check.
                None => {
                    self.virt_open.insert(p);
                    None
                }
            };
            for vt in self.virt_by_class.get(&p).cloned().unwrap_or_default() {
                let keys: Vec<Atom> = match opened {
                    Some(a) => vec![a],
                    None => self.virt_tok_keys.get(&vt).cloned().unwrap_or_default(),
                };
                for a in keys {
                    if let Some(&n) = self.virt_nodes.get(&(vt, a)) {
                        let f = self.field_r(vt, a);
                        self.edge(f, n);
                    }
                }
            }
        }
    }

    /// Is `owner` `tm` or an ancestor of it: the static parent, then the
    /// parents `super()` ran (`parent_links`, §17.2).
    pub(crate) fn has_ancestor(&self, tm: TmplId, owner: TmplId) -> bool {
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if c == owner {
                return true;
            }
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let t = &self.tmpls[c as usize];
            work.extend(t.parent);
            work.extend(t.parent_links.iter().copied());
        }
        false
    }

    /// `t` reaches `this` of another unit's method of class `tmpl` whose
    /// receiver contract is `this` (DECISIONS 57): admitted when `t` is an
    /// instance of a subclass of `tmpl` and the method neither uses its
    /// receiver beyond reading it nor reads a key a subclass accessor may
    /// answer; then the method's published effects happen to `t` (the
    /// stored keys hold unknown values, what it reaches is escaped or
    /// touched as published). False: `t` escapes.
    pub(crate) fn admit_this(&mut self, t: Tok, tmpl: TmplId, this: u32, reason: u32) -> bool {
        use crate::model::{MARK_ESCAPE, MARK_READ, MARK_TOUCHES};
        let Some(c) = self.this_contracts.get(this as usize).cloned() else {
            return false;
        };
        if c.uses & (MARK_ESCAPE | MARK_TOUCHES) != 0 {
            return false;
        }
        // A published receiver token stands for its class's instances; a
        // receiver marker for its classes' (DECISIONS 59): the method runs
        // on them, so its published effects on its receiver are the
        // marker's own (they compose, as arguments do).
        let marker = matches!(self.param_marker.get(&t), Some(&(_, u32::MAX)))
            && !self.reach_marker.contains(&t);
        let classes: Vec<TmplId> = match self.toks[t as usize] {
            _ if marker => self.virt_protos.get(&t).cloned().unwrap_or_default(),
            _ if self.recv_class.contains_key(&t) => vec![self.recv_class[&t]],
            TokKind::Obj(tm, _) if tm != tmpl => vec![tm],
            _ => Vec::new(),
        };
        if classes.is_empty()
            || classes
                .iter()
                .any(|&tm| !self.has_ancestor(tm, tmpl) || (!marker && tm == tmpl))
        {
            return false;
        }
        // A read may run a subclass getter unseen by the method's unit.
        if c.uses & MARK_READ != 0 && classes.iter().any(|&tm| self.chain_has_accessors(tm, tmpl)) {
            return false;
        }
        if marker {
            let (f, _) = self.param_marker[&t];
            self.mark_use(t, c.uses);
            if let Some(&r) = self.marker_reach.get(&t) {
                self.mark_use(r, c.reach);
            }
            match &c.stores {
                Some(ks) => {
                    for k in ks {
                        let a = self.atom(k);
                        self.virt_revoke(t, Some(a));
                        let ks = self.this_stores.entry(f).or_default();
                        if !ks.contains(&a) {
                            ks.push(a);
                        }
                    }
                }
                None => {
                    self.virt_revoke(t, None);
                    self.this_open.insert(f);
                }
            }
            return true;
        }
        let why = |_: &Self| "field of another unit's class".to_string();
        let top = self.node_l("admitted-this-store");
        self.add_top(top, why);
        self.add_prim(top, prim::ALL);
        let site = self.anon_post_site("store by another unit's method");
        match &c.stores {
            Some(ks) => {
                // DECISIONS 179: what the method's flow stores there.
                for (i, k) in ks.iter().enumerate() {
                    let a = self.atom(k);
                    let v = c.values.get(i).copied().unwrap_or(top);
                    self.store_into(t, Key::Named(a), v, site);
                }
            }
            None => self.store_into(t, Key::any(), top, site),
        }
        // What the receiver reaches (its fields, transitively), as for an
        // argument (gen.rs `stand_in_fn_of`).
        if c.reach & (MARK_ESCAPE | MARK_TOUCHES) != 0 {
            let h = self.node_l("admitted-this");
            let q = self.node_l("admitted-this-reach");
            self.cons(
                h,
                Cons::Load {
                    key: Key::any(),
                    dst: q,
                },
            );
            self.cons(
                q,
                Cons::Load {
                    key: Key::any(),
                    dst: q,
                },
            );
            if c.reach & MARK_ESCAPE != 0 {
                self.sink_node(q, reason);
            } else {
                for proto in Protocol::of_touches(c.reach) {
                    self.cons(q, Cons::Implicit { reason, proto });
                }
            }
            self.add_tok(h, t);
        }
        true
    }

    /// A template from `tm` up to (not including) ancestor `owner` defines
    /// an accessor or computed members.
    fn chain_has_accessors(&self, tm: TmplId, owner: TmplId) -> bool {
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(c) = work.pop() {
            if c == owner || seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let t = &self.tmpls[c as usize];
            if !t.accessor_keys.is_empty() || t.has_computed_members {
                return true;
            }
            work.extend(t.parent);
            work.extend(t.parent_links.iter().copied());
        }
        false
    }
}
