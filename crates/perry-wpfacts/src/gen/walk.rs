//! Constraint generation, part 1c: walking modules, classes and statements.

use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    // ----------------------------------------------------------- walking

    pub fn walk_all(&mut self) {
        let modules = self.modules.clone();
        // Constructor phase analysis first (needs every class).
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                let ph = crate::ctor_phase::analyze(c);
                self.ctor_phase.insert(tm, ph);
            }
        }
        // Parent leaks close the child's phase at `super()`.
        let tms: Vec<TmplId> = self.ctor_phase.keys().copied().collect();
        for tm in tms.iter().copied() {
            let leaks = self.parent_chain_leaks(tm);
            if let Some(ph) = self.ctor_phase.get_mut(&tm) {
                ph.parent_leaks = leaks;
            }
        }
        // Birth keys: root → leaf.
        for (mi, (_, module)) in modules.iter().enumerate() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                self.compute_birth_keys(tm, module);
            }
        }
        for mi in 0..modules.len() {
            // Over a bound: the program gets no facts; stop generating.
            if self.m.aborted {
                return;
            }
            let mi = mi as u32;
            let init_fn = self.synthetic_fn(mi, "<init>");
            let this = self.m.funcs[init_fn as usize].this;
            // Module top-level `this`: CJS `module.exports`-like object or
            // undefined in ESM; ⊤-free: a fresh literal-ish object.
            self.m.add_prim(this, prim::UNDEF);
            let fx = Fx {
                module: mi,
                f: init_fn,
                this,
                ret: self.m.funcs[init_fn as usize].ret,
                class: None,
                ctor_of: None,
                new_target: self.m.all_prim_node,
                fn_ctor: false,
            };
            for g in self.globals_of(mi as usize) {
                if let Some(init) = &g.init {
                    let v = self.expr(&fx, init);
                    let gn = self.global(mi, g.id);
                    self.m.edge(v, gn);
                }
            }
            for s in self.init_of(mi as usize) {
                self.stmt(&fx, s);
                self.name_enum_object(mi, s);
            }
            for f in self.functions_of(mi as usize) {
                if self.m.aborted {
                    return;
                }
                let fxi = self.mods[mi as usize].funcs[&f.id];
                self.walk_fn(mi, fxi, f, None, None);
            }
            for c in self.classes_of(mi as usize) {
                if self.m.aborted {
                    return;
                }
                self.walk_class(mi, c, &fx);
            }
        }
    }

    fn compute_birth_keys(&mut self, tm: TmplId, module: &Module) {
        // Another unit's class: its keys are its contract's.
        if !self.m.tmpls[tm as usize].birth_keys.is_empty() || self.stand_in_cc.contains_key(&tm) {
            return;
        }
        let mut keys = Vec::new();
        let mut definite = Vec::new();
        if let Some(p) = self.m.tmpls[tm as usize].parent {
            if let TmplKind::Class { module: pm, .. } = self.m.tmpls[p as usize].kind {
                let pmod = self.modules[pm as usize].1;
                self.compute_birth_keys(p, pmod);
            }
            keys = self.m.tmpls[p as usize].birth_keys.clone();
            definite = self.m.tmpls[p as usize].definite.clone();
        }
        if let Some((_, np)) = self.m.tmpls[tm as usize].native_parent.clone() {
            // Keys the native parent constructor defines (summary, E21).
            for k in crate::builtins::native_parent_keys(&np) {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
        }
        let TmplKind::Class { name, .. } = self.m.tmpls[tm as usize].kind.clone() else {
            return;
        };
        let Some(c) = module.classes.iter().find(|c| c.name == name) else {
            return;
        };
        let mut declared = Vec::new();
        for f in &c.fields {
            if f.is_private || f.key_expr.is_some() {
                continue;
            }
            let a = self.m.atom(&f.name);
            push_unique(&mut keys, a);
            if f.origin == perry_hir::ClassFieldOrigin::Declared {
                push_unique(&mut declared, a);
            } else {
                push_unique(&mut definite, a);
            }
        }
        self.m.tmpls[tm as usize].declared = declared;
        if let Some(ph) = self.ctor_phase.get(&tm).cloned() {
            for k in &ph.assigned_order {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
            for k in &ph.definite {
                let a = self.m.atom(k);
                push_unique(&mut definite, a);
            }
        }
        // A parent that is a value (§17.2) is not known before the solve
        // settles it: the keys its constructor adds first are unknown, so
        // no key of this class has a known slot or is known present (a read
        // may see `undefined` while the parent runs). Settled, the parent's
        // keys are the prefix (`settle_parent_layouts`).
        if self.parent_pending(tm) {
            definite.clear();
        } else if self.value_parent_on_chain(tm) && self.parent_chain_leaks(tm) {
            let n = self.m.tmpls[tm as usize]
                .parent
                .map_or(0, |p| self.m.tmpls[p as usize].definite.len());
            definite.truncate(n);
        }
        self.m.tmpls[tm as usize].birth_keys = keys;
        self.m.tmpls[tm as usize].definite = definite;
    }

    /// Some class on `tm`'s chain has a parent value not settled to one
    /// class (§17.2).
    fn parent_pending(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            let t = &self.m.tmpls[c as usize];
            if t.parent_value.is_some() && t.parent.is_none() {
                return true;
            }
            cur = t.parent;
        }
        false
    }

    /// Some class on `tm`'s chain takes its parent from a value (§17.2).
    fn value_parent_on_chain(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.m.tmpls[c as usize].parent_value.is_some() {
                return true;
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    /// A constructor up `tm`'s chain may leak `this` before `tm`'s own
    /// stores run: one that leaks, a native parent, a parent value not
    /// settled, or another unit's class (its constructor is not seen).
    /// Whether `tm`'s constructor or one up its chain leaks `this`
    /// (published as `ClassContract::ctor_leaks`).
    pub(crate) fn chain_leaks(&self, tm: TmplId) -> bool {
        self.ctor_phase.get(&tm).map_or(true, |ph| ph.leaks) || self.parent_chain_leaks(tm)
    }

    fn parent_chain_leaks(&self, tm: TmplId) -> bool {
        if self.m.tmpls[tm as usize].native_parent.is_some() || self.parent_pending(tm) {
            return true;
        }
        let mut cur = self.m.tmpls[tm as usize].parent;
        while let Some(p) = cur {
            if self.ctor_phase.get(&p).is_some_and(|ph| ph.leaks)
                || self.stand_in_cc.get(&p).is_some_and(|c| c.ctor_leaks)
                || self.m.tmpls[p as usize].native_parent.is_some()
            {
                return true;
            }
            cur = self.m.tmpls[p as usize].parent;
        }
        false
    }

    /// After the solve (§17.2 item 3, DECISIONS 47): each class whose parent
    /// value settled to one class takes that class's keys as its layout
    /// prefix, and every class below it in turn; its own keys are definite
    /// only when no constructor up the chain leaks `this`. The solve ran
    /// with no definite keys for them (`compute_birth_keys`): it saw every
    /// read as possibly `undefined`, which holds for any settled parent.
    pub(crate) fn settle_parent_layouts(&mut self) {
        self.m.settle_parent_values();
        let modules = self.modules.clone();
        let mut redo: Vec<(TmplId, usize)> = Vec::new();
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                if self.value_parent_on_chain(tm) {
                    redo.push((tm, mi));
                }
            }
        }
        for &(tm, _) in &redo {
            self.m.tmpls[tm as usize].birth_keys.clear();
            self.m.tmpls[tm as usize].definite.clear();
        }
        for (tm, mi) in redo {
            self.compute_birth_keys(tm, modules[mi].1);
        }
    }

    pub(crate) fn walk_fn(
        &mut self,
        mi: u32,
        fxi: FnIdx,
        f: &Function,
        class: Option<TmplId>,
        ctor_of: Option<TmplId>,
    ) {
        let info = self.m.funcs[fxi as usize].clone();
        let fn_ctor = class.is_none() && ctor_of.is_none();
        if fn_ctor {
            self.note_fn_phase(fxi, crate::ctor_phase::analyze_fn(f));
        }
        self.rooted_stores.extend(crate::rooted::stores(&f.body));
        let fx = Fx {
            module: mi,
            f: fxi,
            this: info.this,
            ret: info.ret,
            class,
            ctor_of,
            new_target: self.m.all_prim_node,
            fn_ctor,
        };
        self.params(&fx, &f.params);
        if f.is_async || f.is_generator {
            // Not transformed (rare at this point): results are runtime
            // objects.
            self.m.add_top(info.ret, |_| {
                "async/generator function result (untransformed)".into()
            });
        }
        if self.m.funcs[fxi as usize].forwards_super {
            // `run_ctor_chain` constructs the parent with the `new` call;
            // the rest of the body (capture rebinds and stashes) runs here.
            let rest = f.params[0].id;
            let body: Vec<Stmt> = f
                .body
                .iter()
                .filter(|s| !crate::gen::is_forward_of(s, rest))
                .cloned()
                .collect();
            self.stmts(&fx, &body);
            return;
        }
        self.stmts(&fx, &f.body);
    }

    pub(crate) fn params(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.params_inner(fx, params);
        self.m.cur_fn = prev;
    }

    fn params_inner(&mut self, fx: &Fx, params: &[perry_hir::Param]) {
        for p in params {
            let n = self.local(fx.module, p.id);
            if let Some(d) = &p.default {
                let v = self.expr(fx, d);
                self.m.edge(v, n);
            }
        }
    }

    fn walk_class(&mut self, mi: u32, c: &Class, outer: &Fx) {
        let tm = self.mods[mi as usize].classes[&c.name];
        // Instance field initializers run in the field-init context.
        let fi = self.field_init_fn[&tm];
        let fthis = self.m.funcs[fi as usize].this;
        let ffx = Fx {
            module: mi,
            f: fi,
            this: fthis,
            ret: self.m.funcs[fi as usize].ret,
            class: Some(tm),
            ctor_of: Some(tm),
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let ctor_definite: Vec<String> = self
            .ctor_phase
            .get(&tm)
            .filter(|ph| !ph.parent_leaks)
            .map(|ph| ph.definite.clone())
            .unwrap_or_default();
        for f in &c.fields {
            let site = self.store_site(mi, fi, format!("field {}", f.name), Some(tm), true);
            let v = match &f.init {
                Some(e) => self.expr(&ffx, e),
                // `x: T;` defines `undefined` first; when the constructor
                // assigns it before anyone can observe the instance, that
                // `undefined` is never seen.
                None if f.key_expr.is_none() && ctor_definite.contains(&f.name) => continue,
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
                            key: Key::Any,
                            src: v,
                            site,
                        },
                    );
                }
                None => {
                    if f.is_private {
                        // `#x`: a private slot, never reachable by name.
                        continue;
                    }
                    let a = self.m.atom(&f.name);
                    let thisn = self.m.node_l("field-this");
                    self.m.edge(fthis, thisn);
                    self.m.cons(
                        thisn,
                        Cons::Store {
                            key: Key::Named(a),
                            src: v,
                            site,
                        },
                    );
                }
            }
        }
        // Static fields and static blocks: `this` is the class object.
        let si = self.static_init_fn[&tm];
        let sfx = Fx {
            module: mi,
            f: si,
            this: self.m.funcs[si as usize].this,
            ret: self.m.funcs[si as usize].ret,
            class: Some(tm),
            ctor_of: None,
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        for f in &c.static_fields {
            // A declared static field (`Model::static_fields`), whether the
            // walk sees its initializer here or where a nested class is
            // evaluated.
            if f.key_expr.is_none() {
                let a = self.m.atom(&f.name);
                self.m.static_fields.insert((tm, a));
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
            let a = self.m.atom(&f.name);
            let fl = match &f.key_expr {
                Some(k) => {
                    let _ = self.expr(&sfx, k);
                    self.m.field_any(cobj)
                }
                None => self.m.field(cobj, a),
            };
            self.m.edge(v, fl);
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
            module: mi,
            func: f,
            desc,
            ctor_phase_of,
            birth,
            ctor_fn: None,
            rooted: false,
            deep: false,
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

    // ---------------------------------------------------------- statements

    pub(crate) fn stmts(&mut self, fx: &Fx, ss: &[Stmt]) {
        for s in ss {
            self.stmt(fx, s);
        }
    }

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        self.stmt_inner(fx, s);
        self.m.cur_fn = prev;
    }

    /// Per unit: a module-scope TS enum's object (`let E = { … }`, the
    /// lowering of `enum E`) is named `<module>:enum E`, the same in every
    /// unit (a site number is not), so another unit can name its contract
    /// (§18.2). An enum name is one module-scope binding.
    fn name_enum_object(&mut self, mi: u32, s: &Stmt) {
        let Stmt::Let {
            name,
            init: Some(e),
            ..
        } = s
        else {
            return;
        };
        if self.view.is_none()
            || !self.modules[mi as usize]
                .1
                .enums
                .iter()
                .any(|x| &x.name == name)
        {
            return;
        }
        let Some(&(tm, _)) = self.lit_births.get(&(e as *const perry_hir::Expr as usize)) else {
            return;
        };
        let display = format!("{}:enum {name}", self.mods[mi as usize].path_short());
        let t = &mut self.m.tmpls[tm as usize];
        if let TmplKind::Lit { enum_image, .. } = &mut t.kind {
            *enum_image = true;
            t.display = display;
        }
    }

    fn stmt_inner(&mut self, fx: &Fx, s: &Stmt) {
        match s {
            Stmt::Let { id, init, ty, .. } => {
                let n = self.local(fx.module, *id);
                if matches!(ty, perry_hir::types::Type::Array(_)) {
                    self.cell_locals.insert((fx.module, *id));
                }
                if let (Some(perry_hir::Expr::ClassMethodHome { .. }), Some(tm)) = (init, fx.class)
                {
                    self.class_homes.insert((fx.module, *id), tm);
                }
                match init {
                    Some(e) => {
                        let v = self.expr(fx, e);
                        self.m.edge(v, n);
                    }
                    None => self.m.add_prim(n, prim::UNDEF),
                }
                if let Some(perry_hir::Expr::ForInKeys(o)) = init {
                    if let perry_hir::Expr::LocalGet(obj) = o.as_ref() {
                        self.for_in_keys_of.insert((fx.module, *id), *obj);
                    }
                }
            }
            Stmt::Expr(e) => {
                let _ = self.expr(fx, e);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let v = self.expr(fx, e);
                    self.m.edge(v, fx.ret);
                } else {
                    self.m.add_prim(fx.ret, prim::UNDEF);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let _ = self.expr(fx, condition);
                self.stmts(fx, then_branch);
                if let Some(e) = else_branch {
                    self.stmts(fx, e);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                let _ = self.expr(fx, condition);
                self.stmts(fx, body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(fx, i);
                }
                if let Some(c) = condition {
                    let _ = self.expr(fx, c);
                }
                if let Some(u) = update {
                    let _ = self.expr(fx, u);
                }
                match self.for_in_of(fx, body) {
                    Some(f) => self.for_in_body(fx, &f),
                    None => self.stmts(fx, body),
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(fx, body),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
            Stmt::PreallocateTdzBoxes(ids) => {
                for id in ids {
                    let n = self.local(fx.module, *id);
                    self.m.add_prim(n, prim::UNDEF);
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(fx, e);
                self.m.edge(v, self.thrown);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(fx, body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        let n = self.local(fx.module, *id);
                        self.m.edge(self.thrown, n);
                    }
                    self.stmts(fx, &c.body);
                }
                if let Some(f) = finally {
                    self.stmts(fx, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                let d = self.expr(fx, discriminant);
                // `===` against case values: no coercion.
                let _ = d;
                for c in cases {
                    if let Some(t) = &c.test {
                        let _ = self.expr(fx, t);
                    }
                    self.stmts(fx, &c.body);
                }
            }
        }
    }
}

/// The variant name of an expression, without formatting its children: a
/// `fmt::Write` that stops at the first non-identifier character.
pub(crate) fn variant_name(e: &Expr) -> String {
    struct Head(String, bool);
    impl std::fmt::Write for Head {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            if self.1 {
                return Err(std::fmt::Error);
            }
            for c in s.chars() {
                if c.is_alphanumeric() || c == '_' {
                    self.0.push(c);
                } else {
                    self.1 = true;
                    return Err(std::fmt::Error);
                }
            }
            Ok(())
        }
    }
    let mut h = Head(String::new(), false);
    let _ = std::fmt::write(&mut h, format_args!("{e:?}"));
    h.0
}
