//! Constraint generation, part 1c: walking statements.

use super::*;

impl<'a> Gen<'a> {
    // ---------------------------------------------------------- statements

    pub(crate) fn stmts(&mut self, fx: &Fx, ss: &[Stmt]) {
        // ONCHANGE3 rule A: the born-with runs of this list (`born_with.rs`).
        for r in crate::born_with::runs(ss) {
            let e = self.local(fx.module, r.e);
            let news = r.news.iter().map(|&x| self.local(fx.module, x)).collect();
            let calls = r
                .calls
                .iter()
                .map(|(o, k, on_e)| (*o, self.m.atom(k), *on_e))
                .collect();
            let module = fx.module;
            self.m.born_recs.push(crate::born_with::BornRec {
                module,
                e,
                news,
                calls,
            });
        }
        // Rule RP1: the positional steps of each array pattern record.
        for (i, s) in ss.iter().enumerate() {
            if let Some(c) = super::record_steps::cursor_let(s) {
                let mut steps = Vec::new();
                super::record_steps::positional_steps(c, &ss[i + 1..], &mut steps);
                self.record_steps.extend(steps);
            }
        }
        let mut pushed = 0;
        let mut i = 0;
        // A local derived below a guarded counter in this block
        // (`gen/in_bounds.rs`) is one until the block ends.
        let derived = self.in_bounds.derived.len();
        while i < ss.len() {
            if let Some(value) = for_of_binding(&ss[i..]) {
                // `value = arrayRecordForValue(…); state = n; let s = value`:
                // the binding reads the value just written, not every write
                // of the value local (its `undefined` initializer and
                // release, the step's output).
                let Stmt::Expr(set) = &ss[i] else {
                    unreachable!()
                };
                let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
                let n = self.expr(fx, set);
                self.m.cur_fn = prev;
                self.stmt(fx, &ss[i + 1]);
                self.for_binding = Some((fx.module, value, n));
                self.stmt(fx, &ss[i + 2]);
                self.for_binding = None;
                i += 3;
                continue;
            }
            self.last_local_set = None;
            let da = self.def_assign_begin(fx, &ss[i]);
            self.stmt(fx, &ss[i]);
            pushed += self.narrow_assigned(fx, &ss[i]);
            pushed += self.def_assign_end(fx, da);
            // An `if` whose one branch always leaves: what follows runs
            // under the other outcome of its test (`narrow_guard`).
            if let Stmt::If {
                condition,
                then_branch,
                else_branch,
            } = &ss[i]
            {
                let else_exits = else_branch.as_deref().is_some_and(narrow::always_exits);
                if narrow::always_exits(then_branch) && !else_exits {
                    pushed += self.narrow_guard(fx, condition, false);
                } else if else_exits && !narrow::always_exits(then_branch) {
                    pushed += self.narrow_guard(fx, condition, true);
                }
            }
            i += 1;
        }
        self.in_bounds.derived.truncate(derived);
        self.narrow_pop(pushed);
    }

    pub(crate) fn stmt(&mut self, fx: &Fx, s: &Stmt) {
        if let Some((array, counter)) = guarded_for_of_walk(s) {
            self.for_of_walks.insert((fx.module, array, counter));
        }
        let prev = std::mem::replace(&mut self.m.cur_fn, fx.f);
        let first = self
            .m
            .proto_first_stmts
            .get(&(s as *const Stmt as usize))
            .copied();
        let prev_first = std::mem::replace(&mut self.m.proto_first_cur, first);
        // A declaration's own operations stand at the declaration.
        let kind0 =
            self.m.stop_top.as_mut().map(|st| {
                std::mem::replace(&mut st.gen_kind, crate::topstop::variant(s, "Stmt::"))
            });
        let pos0 = match s {
            Stmt::Let { id, .. } if self.m.stop_top.is_some() => self.modules[fx.module as usize]
                .1
                .local_source_spans
                .get(id)
                .map(|sp| {
                    std::mem::replace(
                        &mut self.m.stop_top.as_mut().unwrap().gen_pos,
                        (fx.module, sp.start),
                    )
                }),
            Stmt::Let { .. } => None,
            _ => match (
                self.m.stop_top.as_mut(),
                s.byte_offset().filter(|&o| o != 0),
            ) {
                (Some(st), Some(o)) => Some(std::mem::replace(&mut st.gen_pos, (fx.module, o))),
                _ => None,
            },
        };
        self.stmt_inner(fx, s);
        if let Some(p0) = pos0 {
            self.m.stop_top.as_mut().unwrap().gen_pos = p0;
        }
        if let (Some(k0), Some(st)) = (kind0, self.m.stop_top.as_mut()) {
            st.gen_kind = k0;
        }
        self.m.proto_first_cur = prev_first;
        self.m.cur_fn = prev;
    }

    /// Per unit: a module-scope TS enum's object (`let E = { … }`, the
    /// lowering of `enum E`) is named `<module>:enum E`, the same in every
    /// unit (a site number is not), so another unit can name its contract
    /// (§18.2). An enum name is one module-scope binding.
    pub(super) fn name_enum_object(&mut self, mi: u32, s: &Stmt) {
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
            Stmt::Let {
                id,
                name,
                init,
                ty,
                mutable,
                ..
            } => {
                let n = self.local(fx.module, *id);
                if let (
                    false,
                    Some(
                        perry_hir::Expr::IndexGet { object, index, .. }
                        | perry_hir::Expr::RegionOut { object, index, .. },
                    ),
                ) = (*mutable, init)
                {
                    if let perry_hir::Expr::LocalGet(k) = index.as_ref() {
                        self.same_key_reads
                            .insert((fx.module, *id), (*k, object.as_ref().clone(), fx.f));
                    }
                }
                if [
                    "__forof_lit_src_",
                    "__forof_lit_arr_",
                    "__iterator_record_",
                    "__arr_",
                ]
                .iter()
                .any(|p| name.starts_with(p))
                {
                    self.m.for_of_relays.insert(n);
                }
                if matches!(init, Some(perry_hir::Expr::Array(x, _)) if x.is_empty()) {
                    self.direct_arr = Some(crate::records::ArrBirth::Local(fx.module, *id));
                }
                if !*mutable && init.is_some() {
                    self.initialized_consts.insert((fx.module, *id));
                }
                if matches!(ty, perry_hir::types::Type::Array(_)) {
                    self.cell_locals.insert((fx.module, *id));
                }
                if let (Some(perry_hir::Expr::ClassMethodHome { .. }), Some(tm)) = (init, fx.class)
                {
                    self.class_homes.insert((fx.module, *id), tm);
                }
                let list_tmpl = self.m.tmpls.len() as TmplId;
                match init {
                    Some(e) => {
                        let v = self.expr_zs(fx, e);
                        self.m.edge(v, n);
                        // A `var` redeclared with an initializer writes the
                        // same local: it ends every narrowing of it.
                        self.narrowed_write(fx.module, *id, v);
                        // The local's node takes the value unguarded, and so
                        // does its version (`narrow_assigned`).
                        self.last_local_set = Some((fx.module, *id, v, None));
                    }
                    None => self.m.add_prim(n, prim::UNDEF),
                }
                self.in_bounds_let(fx, *id, init.as_ref());
                // `let id = []`: a list birth (`list_caps`). The empty literal
                // bore one template, the first made since the statement began.
                if matches!(init, Some(perry_hir::Expr::Array(x, _)) if x.is_empty())
                    && self.m.tmpls.len() as TmplId > list_tmpl
                {
                    self.list_birth(fx.module, *id, list_tmpl);
                }
                self.direct_arr = None;
                // FVA 4.3b / E1: the keys local of a for-in, and the source of
                // a for-of over `Object.keys(o)` / `getOwnPropertyNames(o)`
                // written in the loop head (a fresh Array no other code sees;
                // the counted walk binds exactly its elements, `for_of_walk`).
                use crate::model::KeyList;
                // The namespace pair `NamespacePatched(Object) ? Object.keys(o)
                // : <intrinsic>` is one call of the realm's own member while
                // its generic arm yields nothing (`namespace_pair`): its keys
                // are `o`'s; what a replaced member returns runs the body with
                // every key (`Gen::keys_generic`, `for_in_body`).
                let generic = self.last_ns_generic.take();
                let (kinit, generic) = match init {
                    Some(perry_hir::Expr::Conditional {
                        condition,
                        else_expr,
                        ..
                    }) if self.m.ns_pairs
                        && matches!(condition.as_ref(), perry_hir::Expr::NamespacePatched(ns) if ns == "Object")
                        && generic.is_some() =>
                    {
                        (Some(else_expr.as_ref()), generic)
                    }
                    _ => (init.as_ref(), None),
                };
                let keys_src = match kinit {
                    Some(perry_hir::Expr::ForInKeys(o)) => Some((o, KeyList::ForIn)),
                    Some(perry_hir::Expr::ObjectKeys(o))
                        if name.starts_with("__forof_lit_src_") =>
                    {
                        Some((o, KeyList::OwnKeys))
                    }
                    Some(perry_hir::Expr::ObjectGetOwnPropertyNames(o))
                        if name.starts_with("__forof_lit_src_") =>
                    {
                        Some((o, KeyList::OwnNames))
                    }
                    _ => None,
                };
                if let Some((o, list)) = keys_src {
                    if let perry_hir::Expr::LocalGet(obj) = o.as_ref() {
                        self.for_in_keys_of.insert((fx.module, *id), (*obj, list));
                        if let Some(g) = generic {
                            self.keys_generic.insert((fx.module, *id), g);
                        }
                    }
                }
                if let (Some(perry_hir::Expr::LocalGet(src)), true) =
                    (kinit, name.starts_with("__forof_lit_arr_"))
                {
                    if let Some(&v) = self.for_in_keys_of.get(&(fx.module, *src)) {
                        self.for_in_keys_of.insert((fx.module, *id), v);
                    }
                    if let Some(&g) = self.keys_generic.get(&(fx.module, *src)) {
                        self.keys_generic.insert((fx.module, *id), g);
                    }
                }
                if let Some(perry_hir::Expr::LocalGet(src)) = init {
                    self.local_copies.insert((fx.module, *id), *src);
                }
                if name.starts_with("__forin_obj_") {
                    if let Some(p) = init.as_ref().and_then(super::for_in::subj_path) {
                        self.for_in_paths.insert((fx.module, *id), p);
                    }
                }
            }
            Stmt::Expr(e) => {
                let _ = self.expr_zs(fx, e);
            }
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    let v = self.expr_zs(fx, e);
                    let v = self.live_gate(fx, v);
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
                let plan = self.own_guard_plan(condition, Region::Stmts(then_branch));
                let (t, then_test, else_test) = self.guard_test(fx, condition);
                let own = self.own_guard_arm(plan, t);
                // `if (__step_is_error)` / `if (IterResultGetDone)`: plain-async
                // step arms keep the enclosing arm's `live` (not gated).
                let special = matches!(condition, Expr::IterResultGetDone)
                    || matches!(condition, Expr::LocalGet(id) if self.step.by_flag.contains_key(&(fx.module, *id)));
                let then_live = if special {
                    fx.live
                } else {
                    self.arm_live(fx, t, then_test)
                };
                let then_fx = Fx {
                    guard: Some((t, then_test)),
                    live: then_live,
                    ..fx.clone()
                };
                // A branch that always leaves: its writes reach only where
                // it jumps to (`narrow_leave`).
                let pre = self.narrow_snapshot();
                let k = self.narrow_guard(fx, condition, true);
                // `if (__step_is_error)`: a plain-async step's error arm.
                let arm = match condition {
                    Expr::LocalGet(id) => self.step.by_flag.get(&(fx.module, *id)).copied(),
                    _ => None,
                };
                if let Some(v) = arm {
                    self.step.arms.push((fx.module, v));
                }
                // `if (IterResultGetDone)`: the plain-async step's resolve
                // arm, entered only after a `return` wrote the slot.
                let done_arm = matches!(condition, Expr::IterResultGetDone)
                    .then(|| self.iter_done_arm.replace(fx.f));
                self.stmts(&then_fx, then_branch);
                if let Some(p) = done_arm {
                    self.iter_done_arm = p;
                }
                if arm.is_some() {
                    self.step.arms.pop();
                }
                self.narrow_pop(k);
                self.narrow_leave(then_branch, pre);
                self.own_guard_done(own);
                if let Some(e) = else_branch {
                    let else_live = if special {
                        fx.live
                    } else {
                        self.arm_live(fx, t, else_test)
                    };
                    let else_fx = Fx {
                        guard: Some((t, else_test)),
                        live: else_live,
                        ..fx.clone()
                    };
                    let pre = self.narrow_snapshot();
                    let k = self.narrow_guard(fx, condition, false);
                    self.stmts(&else_fx, e);
                    self.narrow_pop(k);
                    self.narrow_leave(e, pre);
                }
            }
            Stmt::While { condition, body } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                self.jumps += 1;
                let _ = self.expr_zs(fx, condition);
                let k = self.narrow_guard(fx, condition, true);
                self.stmts(fx, body);
                self.narrow_pop(k);
                self.jumps -= 1;
                self.loop_depth -= 1;
            }
            Stmt::DoWhile { body, condition } => {
                self.narrow_enter_loop(s);
                self.loop_depth += 1;
                self.jumps += 1;
                let _ = self.expr_zs(fx, condition);
                self.stmts(fx, body);
                self.jumps -= 1;
                self.loop_depth -= 1;
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.narrow_enter_loop(s);
                if let Some(i) = init {
                    self.stmt(fx, i);
                }
                self.loop_depth += 1;
                self.jumps += 1;
                if let Some(c) = condition {
                    let _ = self.expr_zs(fx, c);
                }
                // The update runs after the body: it reads the end node.
                let narrowed = self.narrow_for(
                    fx,
                    init.as_deref(),
                    condition.as_ref(),
                    update.as_ref(),
                    body,
                );
                // `for (init; c; u) B` is `init; while (c) { B; u }`: B and
                // u see `c` true, as a `while` body does. The counter
                // `narrow_for` narrows keeps its ranges alone.
                let counter = match (narrowed, init.as_deref()) {
                    (true, Some(Stmt::Let { id, .. })) => Some(*id),
                    _ => None,
                };
                let k = match condition {
                    Some(c) => self.narrow_guard_skip(fx, c, true, counter),
                    None => 0,
                };
                let pushed = k + usize::from(narrowed);
                self.narrow_in_update(pushed, true);
                if let Some(u) = update {
                    let _ = self.expr_zs(fx, u);
                }
                self.narrow_in_update(pushed, false);
                let scope = self.enum_scope(fx, body);
                let scoped = scope.is_some();
                if let Some(s) = scope {
                    self.for_in_reads.push(s);
                }
                match self.for_in_of(fx, body) {
                    Some(f) => self.for_in_body(fx, &f),
                    None => self.stmts(fx, body),
                }
                if scoped {
                    self.for_in_reads.pop();
                }
                self.narrow_pop(k);
                if narrowed {
                    self.narrow_end();
                }
                self.jumps -= 1;
                self.loop_depth -= 1;
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
                    let n = self.dead_zone(fx.module, *id);
                    self.m.add_prim(n, prim::UNDEF);
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(fx, e);
                let r = self.region();
                self.m.edge(v, r);
            }
            Stmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                // The try body is a throw region of its own; its catch and
                // finally run in the enclosing one.
                let rt = self.m.node_l("RAISE-try");
                let outer = std::mem::replace(&mut self.m.gen_region, Some(rt));
                self.tries += 1;
                self.stmts(fx, body);
                self.m.gen_region = outer;
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        let n = self.local(fx.module, *id);
                        self.m.edge(rt, n);
                        self.m.edge(self.thrown, n);
                    }
                    self.stmts(fx, &c.body);
                } else {
                    let r = self.region();
                    self.m.edge(rt, r);
                }
                if let Some(f) = finally {
                    self.stmts(fx, f);
                }
                self.tries -= 1;
            }
            Stmt::Switch {
                discriminant,
                cases,
                byte_offset,
            } => {
                let d = self.expr_zs(fx, discriminant);
                // `===` against case values: no coercion, identity.
                self.jumps += 1;
                self.m.identity(d, IdentityRoute::Switch);
                // Rule M1: a switch outside every loop, try and other switch
                // of the body (blocks and `if` around it only), switching on
                // a parameter (its own node: no default value, no
                // `arguments` object, never assigned: no `=` or `++` of it)
                // with string-literal case tests only.
                // Per clause, the runs that reach it: the run entering at
                // clause j runs j and each next clause while the one before
                // may complete normally (ECMA-262 14.12.4: fallthrough; the
                // default clause is entered when no case test is equal).
                let f = self.m.cur_fn;
                // `jumps` counts this switch itself.
                let top = self.jumps == 1 && self.tries == 0 && self.m.clause.is_none();
                let param = match discriminant {
                    Expr::LocalGet(id)
                        if top
                            && f != NO_FN
                            && cases.len() < 64
                            && !self.m.funcs[f as usize].has_arguments
                            && !self.m.switch_runs.contains_key(&f)
                            && !self.written_locals(fx.module).contains(id) =>
                    {
                        let pn = self.local(fx.module, *id);
                        self.m.funcs[f as usize]
                            .params
                            .iter()
                            .position(|&q| q == pn)
                    }
                    _ => None,
                };
                let mut labels: Vec<Option<crate::model::Atom>> = Vec::new();
                let mut masks: Vec<u64> = Vec::new();
                if param.is_some() {
                    for c in cases.iter() {
                        match &c.test {
                            None => labels.push(None),
                            Some(Expr::String(t)) => labels.push(Some(self.m.atom(t))),
                            Some(_) => {
                                labels.clear();
                                break;
                            }
                        }
                    }
                }
                if labels.len() == cases.len()
                    && !cases.is_empty()
                    && labels.iter().filter(|l| l.is_none()).count() <= 1
                {
                    if let Some(j) = param {
                        let ends = |c: &perry_hir::SwitchCase| {
                            matches!(
                                c.body.last(),
                                Some(
                                    Stmt::Break
                                        | Stmt::Continue
                                        | Stmt::LabeledBreak(_)
                                        | Stmt::LabeledContinue(_)
                                        | Stmt::Return(_, _)
                                        | Stmt::Throw(_)
                                )
                            )
                        };
                        for i in 0..cases.len() {
                            let mut m = 0u64;
                            let mut k = i;
                            loop {
                                m |= 1 << k;
                                if k == 0 || ends(&cases[k - 1]) {
                                    break;
                                }
                                k -= 1;
                            }
                            masks.push(m);
                        }
                        self.m.switch_runs.insert(f, (j as u32, labels));
                    }
                }
                for (i, c) in cases.iter().enumerate() {
                    if let Some(t) = &c.test {
                        let tv = self.expr_zs(fx, t);
                        self.m.identity(tv, IdentityRoute::Switch);
                        // N1: a case test is the strict equality it means.
                        self.equality_demand(discriminant, t, d, tv);
                    }
                    let clause = self.m.clause;
                    if let Some(&m) = masks.get(i) {
                        self.m.clause = Some((f, m));
                    }
                    // Rule G3: a clause only literal tests select narrows
                    // the host of the discriminant (`Gen::case_narrow`).
                    let g3 = self.case_narrow(fx, discriminant, cases, i, *byte_offset);
                    self.stmts(fx, &c.body);
                    self.narrow_pop(g3);
                    self.m.clause = clause;
                }
                self.jumps -= 1;
            }
        }
    }
}

/// The counted walk of a for-of source that is an ordinary array
/// (`perry_hir::lower::for_of_guard`): `for (let __forof_lit_k_N = 0;
/// __forof_lit_k_N < a.length; __forof_lit_k_N++) { const x =
/// a[__forof_lit_k_N]; body }`, where nothing in the body writes the
/// counter or the array binding. Its array and counter locals. The whole
/// shape is checked, not only the temp's name, because the binding read is
/// then taken to lie below the length.
fn for_of_walk(s: &Stmt) -> Option<(u32, u32)> {
    let Stmt::For {
        init: Some(init),
        condition: Some(condition),
        update: Some(update),
        body,
    } = s
    else {
        return None;
    };
    let Stmt::Let {
        id: k,
        name,
        init: Some(Expr::Integer(0)),
        ..
    } = init.as_ref()
    else {
        return None;
    };
    let k = *k;
    if !name.starts_with("__forof_lit_k_") {
        return None;
    }
    let Some(Stmt::Let {
        init: Some(Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. }),
        ..
    }) = body.first()
    else {
        return None;
    };
    let (Expr::LocalGet(a), Expr::LocalGet(i)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    let a = *a;
    if *i != k {
        return None;
    }
    let bounded = matches!(condition, Expr::Compare {
        op: perry_hir::CompareOp::Lt,
        left,
        right, ..
    } if matches!(left.as_ref(), Expr::LocalGet(l) if *l == k)
        && matches!(right.as_ref(), Expr::PropertyGet { object, property, .. }
            if property == "length" && matches!(object.as_ref(), Expr::LocalGet(o) if *o == a)));
    let stepped = matches!(update, Expr::Update {
        id,
        op: perry_hir::UpdateOp::Increment,
        ..
    } if *id == k);
    let untouched = !body
        .iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| writes_local(e, k, a)));
    (bounded && stepped && untouched).then_some((a, k))
}

/// The proven arm of a for-of over a source that is not a literal
/// (`perry_hir::lower::for_of_guard::source_for_of`):
/// `if (arrayRecordNeedsIterator(t) === false) { [const a = t;] walk }`.
/// The runtime test holds only for an ordinary Array whose iteration
/// members nobody replaced, so inside the arm `a` is an Array and the walk's
/// element is what iterating it yields. A loop of the walk's shape outside
/// that arm (over an array-like, say) is not a walk.
fn guarded_for_of_walk(s: &Stmt) -> Option<(u32, u32)> {
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Eq,
                left,
                right,
                ..
            },
        then_branch,
        ..
    } = s
    else {
        return None;
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = left.as_ref()
    else {
        return None;
    };
    let (Some(Expr::LocalGet(t)), Expr::Bool(false)) = (args.first(), right.as_ref()) else {
        return None;
    };
    if module != "__perry_runtime" || method != "arrayRecordNeedsIterator" {
        return None;
    }
    let (array, walk) = match then_branch.as_slice() {
        [walk] => (*t, walk),
        [Stmt::Let {
            id,
            init: Some(Expr::LocalGet(src)),
            ..
        }, walk]
            if src == t =>
        {
            (*id, walk)
        }
        _ => return None,
    };
    for_of_walk(walk).filter(|(a, _)| *a == array)
}

/// `e` writes local `k` or `a`, or holds a closure that may (a mutable
/// capture of either).
fn writes_local(e: &Expr, k: u32, a: u32) -> bool {
    match e {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } if *id == k || *id == a => true,
        Expr::Closure {
            mutable_captures, ..
        } if mutable_captures.iter().any(|id| *id == k || *id == a) => true,
        _ => {
            let mut hit = false;
            perry_hir::walker::walk_expr_children(e, &mut |c| hit = hit || writes_local(c, k, a));
            hit
        }
    }
}

/// The head of a for-of body as HIR lowers it (`IteratorRecord::drive`,
/// the shape codegen's `record_binding_prefix` reads): `value =
/// arrayRecordForValue(mode, value, element); state = n; let s = value`.
/// The value local.
fn for_of_binding(ss: &[Stmt]) -> Option<u32> {
    let [Stmt::Expr(Expr::LocalSet(value, read)), Stmt::Expr(Expr::LocalSet(state, n)), Stmt::Let {
        init: Some(Expr::LocalGet(bound)),
        ..
    }, ..] = ss
    else {
        return None;
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args,
        ..
    } = read.as_ref()
    else {
        return None;
    };
    let ok = module == "__perry_runtime"
        && method == "arrayRecordForValue"
        && matches!(args.get(1), Some(Expr::LocalGet(v)) if v == value)
        && matches!(n.as_ref(), Expr::Number(_))
        && state != value
        && bound == value;
    ok.then_some(*value)
}
