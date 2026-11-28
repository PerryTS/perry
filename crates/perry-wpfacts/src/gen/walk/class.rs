//! Constraint generation, part 1c: walking classes and recording store sites.

use super::*;

impl<'a> Gen<'a> {
    pub(super) fn walk_class(&mut self, mi: u32, c: &'a Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // DECISIONS 225: the field initializers run when an instance is
        // constructed (`run_ctor_chain`).
        let fi = self.field_init_fn[&tm];
        if !self.defer_walk(fi, PendingWalk::Fields(mi, c)) {
            self.walk_fields_body(mi, c);
        }
        self.walk_class_rest(mi, c, outer);
    }

    /// Field initializers run per construction: unordered (RPG1b).
    pub(super) fn walk_fields_body(&mut self, mi: u32, c: &'a Class) {
        // Field initializers run inside the constructor call: the shared
        // cell, which every catch reads.
        let region = std::mem::replace(&mut self.m.gen_region, Some(self.thrown));
        self.walk_fields_body_region(mi, c);
        self.m.gen_region = region;
    }

    fn walk_fields_body_region(&mut self, mi: u32, c: &'a Class) {
        let saved = (self.order_ok, self.loop_depth);
        (self.order_ok, self.loop_depth) = (false, 0);
        let list_saved = self.list_enter_body();
        #[cfg(test)]
        let packet_prior = self
            .m
            .packet_body(self.field_init_fn[&self.mods[mi as usize].classes[&c.name]]);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        self.walk_fields_body_inner(mi, c);
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        (self.order_ok, self.loop_depth) = saved;
        self.list_body = list_saved;
    }

    fn walk_fields_body_inner(&mut self, mi: u32, c: &'a Class) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Instance field initializers run in the field-init context.
        let fi = self.field_init_fn[&tm];
        self.m.note_walked(fi);
        let fthis = self.m.funcs[fi as usize].this;
        let ffx = Fx {
            guard: None,
            live: None,
            module: mi,
            f: fi,
            this: fthis,
            ret: self.m.funcs[fi as usize].ret,
            class: Some(tm),
            ctor_of: Some(tm),
            new_target: None,
            fn_ctor: false,
        };
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        // A declared field's `undefined` is unobservable only when the
        // constructor body stores the key on every path first (`stored`; its
        // key being present from birth, `definite`, is not enough).
        let ctor_definite: Vec<String> = self
            .ctor_phase
            .get(&tm)
            .filter(|ph| !ph.parent_leaks)
            .map(|ph| ph.stored.clone())
            .unwrap_or_default();
        for f in &c.fields {
            let site = self.store_site(mi, fi, format!("field {}", f.name), Some(tm), true);
            let v = match &f.init {
                Some(e) => {
                    // A push-filled `new Array()`: the builtin's birth at
                    // its call site is the fill's (`new_builtin` names the
                    // site before the call exists: the solve is online).
                    if matches!(e, Expr::New { .. }) {
                        self.push_fill_new = self
                            .push_fill
                            .births
                            .get(&(e as *const Expr as usize))
                            .copied();
                    }
                    if matches!(e, Expr::Array(x, _) if x.is_empty())
                        && f.key_expr.is_none()
                        && !f.is_private
                    {
                        self.direct_arr =
                            Some(crate::records::ArrBirth::Field(mi, c.id, f.name.clone()));
                    }
                    let v = self.expr(&ffx, e);
                    self.push_fill_new = None;
                    self.direct_arr = None;
                    v
                }
                // `x: T;` defines `undefined` first; when the constructor
                // assigns it before anyone can observe the instance, that
                // `undefined` is never seen.
                None if f.key_expr.is_none()
                    && (ctor_definite.contains(&f.name) || c.is_literal_shape()) =>
                {
                    // An object literal's shape: its constructor stores
                    // every key from the literal's values first.
                    continue;
                }
                // R9READ2 rule I: a reservation the construction stores
                // first once its chain reads run no code: its `undefined`
                // only while that may fail (`Model::inert_undef`).
                None if f.key_expr.is_none()
                    && !f.is_private
                    && matches!(f.origin, perry_hir::ClassFieldOrigin::ConstructorStore)
                    && self.m.atom_index.get(f.name.as_str()).is_some_and(|&a| {
                        self.m.tmpls[tm as usize].inert_definite.contains(&a)
                    }) =>
                {
                    let a = self.m.atom(&f.name);
                    let v = self.m.node_l("inert-birth");
                    self.m.inert_undef(tm, a, v);
                    v
                }
                None => self.prim_node(prim::UNDEF),
            };
            match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&ffx, k);
                    let thisn = self.m.node_l("field-this");
                    self.m.add_tok(thisn, inst);
                    self.m.edge(fthis, thisn);
                    self.m.cons(
                        thisn,
                        Cons::Store {
                            key: Key::any(),
                            src: v,
                            site,
                            region: self.region(),
                        },
                    );
                }
                None => {
                    // `#x`: the class's private field, a key apart from
                    // every public one (`Model::private_atom`).
                    let a = match f.is_private {
                        true => {
                            let key = self.private_field_key(mi, c, &f.name);
                            self.m.private_atom(&key)
                        }
                        false => self.m.atom(&f.name),
                    };
                    let thisn = self.m.node_l("field-this");
                    self.m.edge(fthis, thisn);
                    self.m.cons(
                        thisn,
                        Cons::Store {
                            key: Key::Named(a),
                            src: v,
                            site,
                            region: self.region(),
                        },
                    );
                }
            }
        }
    }

    fn walk_class_rest(&mut self, mi: u32, c: &'a Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Static fields and static blocks: `this` is the class object.
        let si = self.static_init_fn[&tm];
        self.m.note_walked(si);
        #[cfg(test)]
        let packet_prior = self.m.packet_body(si);
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
        let sfx = Fx {
            guard: None,
            live: None,
            module: mi,
            f: si,
            this: self.m.funcs[si as usize].this,
            ret: self.m.funcs[si as usize].ret,
            class: Some(tm),
            ctor_of: None,
            new_target: None,
            fn_ctor: false,
        };
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        for f in &c.static_fields {
            // A declared static field (`Model::static_fields`), whether the
            // walk sees its initializer here or where a nested class is
            // evaluated.
            let named = match f.is_private {
                true => {
                    let key = self.private_field_key(mi, c, &f.name);
                    self.m.private_atom(&key)
                }
                false => self.m.atom(&f.name),
            };
            if f.key_expr.is_none() {
                self.m.static_fields.insert((tm, named));
            }
            if c.is_nested && (f.init.is_some() || f.key_expr.is_some()) {
                // A nested class's static initializers run where the class
                // is evaluated (`StaticFieldSet`, `ClassStaticSymbolSet`,
                // the statics of `ClassExprFresh`), never from the
                // declaration (codegen `static_fields.rs`); this copy is
                // not rewritten for shared-mutable captures (DECISIONS 50).
                continue;
            }
            let v = match &f.init {
                Some(e) => self.expr(&sfx, e),
                None => self.prim_node(prim::UNDEF),
            };
            let fl = match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&sfx, k);
                    self.m.field_any(cobj)
                }
                None => self.m.field(cobj, named),
            };
            self.m.edge(v, fl);
        }
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        for d in &c.decorators {
            for a in &d.args {
                let v = self.expr(outer, a);
                let r = self.m.reason("decorator argument");
                self.m.sink_node(v, r);
            }
            // Decorators receive the class value: unmodeled code.
            let r = self.m.reason("class decorator");
            self.m.escape(cobj, r);
        }
        if let Some(e) = &c.extends_expr {
            let _ = self.expr(outer, e);
        }
        if let Some(ctor) = &c.constructor {
            let f = self.mods[mi as usize].funcs[&ctor.id];
            self.walk_fn(mi, f, ctor, Some(tm), Some(tm));
        }
        for meth in c.methods.iter().chain(c.static_methods.iter()) {
            let f = self.mods[mi as usize].funcs[&meth.id];
            self.walk_fn(mi, f, meth, Some(tm), None);
        }
        for (_, g) in c.getters.iter().chain(c.setters.iter()) {
            let f = self.mods[mi as usize].funcs[&g.id];
            self.walk_fn(mi, f, g, Some(tm), None);
        }
        for cm in &c.computed_members {
            let _ = self.expr(outer, &cm.key_expr);
            let f = self.mods[mi as usize].funcs[&cm.function.id];
            // DECISIONS 225: a computed accessor runs on reads through the
            // prototype under any key (its `this` is seeded at
            // registration), never through a call: active with its class.
            let at = self.m.cur_fn;
            self.m
                .effect_in(at, crate::model::Effect::Activate(f, "computed"));
            self.walk_fn(mi, f, &cm.function, Some(tm), None);
        }
        for d in c.fields.iter().chain(c.static_fields.iter()) {
            for dec in &d.decorators {
                let r = self.m.reason("field decorator");
                self.m.escape(cobj, r);
                for a in &dec.args {
                    let _ = self.expr(outer, a);
                }
            }
        }
        for meth in c.methods.iter().chain(c.static_methods.iter()) {
            if !meth.decorators.is_empty() {
                let r = self.m.reason("method decorator");
                self.m.escape(cobj, r);
            }
        }
    }

    pub(crate) fn store_site(
        &mut self,
        mi: u32,
        f: FnIdx,
        desc: String,
        ctor_phase_of: Option<TmplId>,
        birth: bool,
    ) -> SiteId {
        let id = self.m.stores.len() as SiteId;
        self.m.stores.push(StoreSite {
            proto_first: self.m.proto_first_cur,
            module: mi,
            func: f,
            desc,
            ctor_phase_of,
            birth,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
            proto_named: false,
            assign: false,
            proto_of: None,
            in_loop: self.order_ok.then_some(self.loop_depth > 0),
            define_attrs: None,
        });
        id
    }

    /// Record the construction-phase analysis of a function that `new` may
    /// call: its `this` stores before a leak are birth writers of `new f()`.
    pub(crate) fn note_fn_phase(&mut self, f: FnIdx, ph: CtorPhase) {
        if ph.assigned_order.is_empty() {
            return;
        }
        let birth: Vec<Atom> = ph.assigned_order.iter().map(|k| self.m.atom(k)).collect();
        let definite: Vec<Atom> = ph.definite.iter().map(|k| self.m.atom(k)).collect();
        self.m.fn_birth.insert(f, (birth, definite));
        self.fn_phase.insert(f, ph);
    }
}
