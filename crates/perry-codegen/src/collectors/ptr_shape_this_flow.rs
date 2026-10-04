//! Pass 3 of the `Ptr<Shape>` collector: `this`-flow safety of constructors and
//! called methods. Split out of ptr_shape.rs for the 2000-line file-size gate;
//! still a child module, so `use super::*` reaches the collector's private items.

use super::*;

// ── Pass 3: `this`-flow safety of constructors and called methods ──────────

/// A `this.field = value` store observed inside a constructor, a field
/// initializer, or a method body, with the owning function's parameter ids so
/// parameter-mediated values can be resolved through call-site arguments.
pub(super) struct ThisStoreRecord<'a> {
    pub(super) field: String,
    /// `None` = `++`/`--` update (numeric); `Some` = the stored expression.
    pub(super) value: Option<&'a Expr>,
    /// Owning context: `None` for field initializers; `Some((owner_class,
    /// method_name, param_ids))` for constructor ("constructor") and methods.
    pub(super) context: Option<(String, String, Vec<u32>)>,
}

/// Pass-3 safety verdict for one class chain plus the methods invoked on the
/// value: `this`-flow containment of the constructor chain, prototype
/// stability when any method is called, field/method name-ambiguity, and
/// per-method `this`-flow safety. Returns the analysis (holding the store
/// records, `super(...)` argument lists, and internally-invoked set the
/// numeric proof consumes) or the FIRST failed obligation.
///
/// This is the single implementation behind both the per-candidate `'cand`
/// loop and the group-scope numeric proof (#7770,
/// `ptr_shape_numeric.rs::prove_group_numeric_fields`). The verdict licenses
/// a bare unchecked `load double`; keeping the two callers on one function
/// is what makes "tighten an obligation" a one-place change.
pub(super) fn chain_this_flow_verdict<'a, 'b>(
    classes: &HashMap<String, &'a Class>,
    module_dispatch: &ModuleDispatchFacts,
    class_name: &str,
    chain: &'b [&'a Class],
    fields: &'b HashSet<String>,
    methods: &'b HashMap<String, (String, &'a perry_hir::Function)>,
    called: Option<&HashMap<String, Vec<&'a [Expr]>>>,
) -> Result<ThisFlowAnalysis<'a, 'b>, ShapeDenial> {
    let mut analysis = ThisFlowAnalysis {
        chain,
        fields,
        methods,
        visited: HashSet::new(),
        store_records: Vec::new(),
        super_call_args: HashMap::new(),
        internally_invoked: HashSet::new(),
        allow_this_in_store_values: false,
    };
    if !analysis.ctor_chain_safe() {
        return Err(report::THIS_ESCAPE);
    }
    if let Some(called) = called {
        if !called.is_empty() && !module_dispatch.prototype_is_stable(classes, class_name) {
            return Err(report::UNSTABLE_PROTOTYPE);
        }
        for m in called.keys() {
            if fields.contains(m.as_str()) {
                // A name that is both a field and a method is ambiguous
                // under own-property shadowing — bail.
                return Err(report::FIELD_METHOD_AMBIGUITY);
            }
            let Some((owner, func)) = methods.get(m.as_str()) else {
                return Err(report::ESC_UNRESOLVED_METHOD);
            };
            if !analysis.method_safe(owner, func) {
                return Err(report::METHOD_THIS_ESCAPE);
            }
        }
    }
    Ok(analysis)
}

pub(in crate::collectors) struct ThisFlowAnalysis<'a, 'b> {
    pub(super) chain: &'b [&'a Class],
    pub(super) fields: &'b HashSet<String>,
    pub(super) methods: &'b HashMap<String, (String, &'a perry_hir::Function)>,
    pub(super) visited: HashSet<(String, String, bool)>,
    pub(super) store_records: Vec<ThisStoreRecord<'a>>,
    /// `super(...)` argument lists observed in chain constructors, keyed by
    /// the PARENT (callee) class name. Feeds the parent-ctor parameter
    /// resolution of the numeric-field proof.
    pub(super) super_call_args: HashMap<String, Vec<&'a [Expr]>>,
    /// Method names invoked INTERNALLY — `this.m(...)` from a constructor or
    /// another method, and `super.m(...)`. Their argument expressions live in
    /// the CALLING method's scope, which the numeric-field proof's
    /// `ParamEnv::Sites` (function-scope call-site args) cannot resolve —
    /// so parameters of internally-invoked methods must stay unproven even
    /// when every EXTERNAL call site passes numeric arguments.
    pub(super) internally_invoked: HashSet<String>,
    /// Phase 5a only: permit a `this.f = <expr mentioning this>` store.
    ///
    /// Phase 3b rejects those because its numeric-field proof resolves store
    /// VALUES through constructor/method call-site arguments, and a
    /// `this`-dependent value cannot be resolved that way — so the store must
    /// not be recorded as provably-numeric. Phase 5a claims no numeric fields
    /// at all (`collectors/proven_this.rs`), so the restriction buys it
    /// nothing while excluding the single most common method shape there is:
    /// `this.value = this.value + 1`. Safety is unaffected — the value
    /// expression still goes through `expr_this_safe`, which rejects `this`
    /// in value position and admits only declared-chain `this.field` reads.
    pub(super) allow_this_in_store_values: bool,
}

impl<'a, 'b> ThisFlowAnalysis<'a, 'b> {
    /// A fresh analysis over one class chain. Phase 5a
    /// (`collectors/proven_this.rs`) reuses the walk for a method's `this`
    /// without the constructor-chain obligations: the receiver of a proven
    /// `this` already exists, and its shape is established by the CALL SITE
    /// guard (class id + keys token) rather than by in-function provenance.
    pub(in crate::collectors) fn new(
        chain: &'b [&'a Class],
        fields: &'b HashSet<String>,
        methods: &'b HashMap<String, (String, &'a perry_hir::Function)>,
    ) -> Self {
        Self {
            chain,
            fields,
            methods,
            visited: HashSet::new(),
            store_records: Vec::new(),
            super_call_args: HashMap::new(),
            internally_invoked: HashSet::new(),
            allow_this_in_store_values: true,
        }
    }

    /// Did the vetted walk observe any `this.<field> = …` store (in this
    /// method or anything it transitively invokes on the same `this`)?
    /// Phase 5a gates the freeze-family kill on this.
    pub(in crate::collectors) fn has_this_store_records(&self) -> bool {
        self.store_records.iter().any(|r| r.context.is_some())
    }

    /// Walk the constructor chain (self-first `super(...)` order) and every
    /// chain field initializer under the strict `this` discipline.
    fn ctor_chain_safe(&mut self) -> bool {
        for class in self.chain {
            for field in &class.fields {
                if let Some(init) = &field.init {
                    if expr_mentions_this(init) {
                        return false;
                    }
                    self.store_records.push(ThisStoreRecord {
                        field: field.name.clone(),
                        value: Some(init),
                        context: None,
                    });
                }
            }
        }
        for class in self.chain {
            if let Some(ctor) = &class.constructor {
                if !self.function_this_safe(&class.name, "constructor", ctor, false) {
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn method_safe(&mut self, owner: &str, func: &'a perry_hir::Function) -> bool {
        self.function_this_safe(owner, &func.name, func, false)
    }

    /// Phase 5a root-method variant: `return this` is safe only as the final
    /// statement of the method whose receiver was already guarded. It creates
    /// no alias until every specialized field access has completed. Nested
    /// methods continue through [`Self::method_safe`] and may not return the
    /// receiver, preserving Phase 3b's no-escape contract.
    pub(in crate::collectors) fn method_safe_with_terminal_this_return(
        &mut self,
        owner: &str,
        func: &'a perry_hir::Function,
    ) -> bool {
        self.function_this_safe(owner, &func.name, func, true)
    }

    fn function_this_safe(
        &mut self,
        owner: &str,
        name: &str,
        func: &'a perry_hir::Function,
        allow_terminal_this_return: bool,
    ) -> bool {
        // Keyed by the terminal-`this`-return allowance too: the strict
        // (`false`) vetting of a nested `this.m()` / `super.m()` edge must not
        // be satisfied by an earlier lenient (`true`) visit of the same method.
        let key = (
            owner.to_string(),
            name.to_string(),
            allow_terminal_this_return,
        );
        if !self.visited.insert(key) {
            return true; // already vetted (or in-progress higher up the stack)
        }
        if self.visited.len() > 64 {
            return false;
        }
        if func.is_async || func.is_generator || func.was_plain_async {
            return false;
        }
        let param_ids: Vec<u32> = func.params.iter().map(|p| p.id).collect();
        let ctx = (owner.to_string(), name.to_string(), param_ids);
        let mut safe = true;
        for (index, s) in func.body.iter().enumerate() {
            if !safe {
                break;
            }
            let terminal_this_return = allow_terminal_this_return
                && index + 1 == func.body.len()
                && matches!(s, Stmt::Return(Some(Expr::This)));
            safe &= terminal_this_return || self.stmt_this_safe(s, &ctx);
        }
        safe
    }

    fn stmt_this_safe(&mut self, s: &'a Stmt, ctx: &(String, String, Vec<u32>)) -> bool {
        match s {
            Stmt::Let { init, .. } => init
                .as_ref()
                .map(|e| self.expr_this_safe(e, ctx))
                .unwrap_or(true),
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr_this_safe(e, ctx),
            Stmt::Return(opt) => {
                // A constructor `return <expr>` can OVERRIDE the `new` result
                // (`js_ctor_return_override`): the provenance proof "the local
                // holds exactly a C instance" would be wrong. Disqualify any
                // value-returning chain constructor (conservative — even
                // primitive returns, which JS ignores).
                if ctx.1 == "constructor" && opt.is_some() {
                    return false;
                }
                opt.as_ref()
                    .map(|e| self.expr_this_safe(e, ctx))
                    .unwrap_or(true)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_this_safe(condition, ctx)
                    && then_branch.iter().all(|s| self.stmt_this_safe(s, ctx))
                    && else_branch
                        .as_ref()
                        .map(|b| b.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr_this_safe(condition, ctx)
                    && body.iter().all(|s| self.stmt_this_safe(s, ctx))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref()
                    .map(|s| self.stmt_this_safe(s, ctx))
                    .unwrap_or(true)
                    && condition
                        .as_ref()
                        .map(|e| self.expr_this_safe(e, ctx))
                        .unwrap_or(true)
                    && update
                        .as_ref()
                        .map(|e| self.expr_this_safe(e, ctx))
                        .unwrap_or(true)
                    && body.iter().all(|s| self.stmt_this_safe(s, ctx))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(|s| self.stmt_this_safe(s, ctx))
                    && catch
                        .as_ref()
                        .map(|c| c.body.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
                    && finally
                        .as_ref()
                        .map(|f| f.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr_this_safe(discriminant, ctx)
                    && cases.iter().all(|case| {
                        case.test
                            .as_ref()
                            .map(|t| self.expr_this_safe(t, ctx))
                            .unwrap_or(true)
                            && case.body.iter().all(|s| self.stmt_this_safe(s, ctx))
                    })
            }
            Stmt::Labeled { body, .. } => self.stmt_this_safe(body.as_ref(), ctx),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => true,
        }
    }

    fn expr_this_safe(&mut self, e: &'a Expr, ctx: &(String, String, Vec<u32>)) -> bool {
        match e {
            // #11791: a private field read or write, a private method call or
            // a brand check on `this`. Each is checked by its own private site
            // (the receiver's ShapeId names the element), and none changes
            // the receiver's public layout: a private field is never added
            // outside construction (writing an absent one throws), and its
            // representation lane is its own slot's.
            Expr::PropertyGet { object, .. } if private_this(object, 0, 0) => true,
            Expr::PropertySet { object, value, .. } if private_this(object, 0, 1) => {
                self.expr_this_safe(value, ctx)
            }
            Expr::Call { callee, args, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, .. } if private_this(object, 1, 0)
                ) =>
            {
                let Expr::PropertyGet { object, .. } = callee.as_ref() else {
                    unreachable!()
                };
                let Expr::PrivateGuard { field_name, .. } = object.as_ref() else {
                    unreachable!()
                };
                let Some((owner, func)) = self.methods.get(field_name).cloned() else {
                    return false;
                };
                self.internally_invoked.insert(field_name.clone());
                if !self.function_this_safe(&owner, field_name, func, false) {
                    return false;
                }
                args.iter().all(|a| self.expr_this_safe(a, ctx))
            }
            Expr::PrivateBrandCheck { object, .. } if matches!(object.as_ref(), Expr::This) => true,
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => self.fields.contains(property),
            Expr::PropertySet {
                object,
                property,
                value,
            } if matches!(object.as_ref(), Expr::This) => {
                if !self.fields.contains(property)
                    || (!self.allow_this_in_store_values && expr_mentions_this(value))
                {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: Some(value),
                    context: Some(ctx.clone()),
                });
                self.expr_this_safe(value, ctx)
            }
            Expr::PropertyUpdate {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                if !self.fields.contains(property) {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: None,
                    context: Some(ctx.clone()),
                });
                true
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This) =>
            {
                let Expr::String(property) = key.as_ref() else {
                    return false;
                };
                if !self.fields.contains(property)
                    || (!self.allow_this_in_store_values && expr_mentions_this(value))
                {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: Some(value),
                    context: Some(ctx.clone()),
                });
                self.expr_this_safe(value, ctx)
            }
            // `this.m(args)` — vet the callee method transitively.
            Expr::Call { callee, args, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, .. } if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                let Expr::PropertyGet { property, .. } = callee.as_ref() else {
                    unreachable!()
                };
                if self.fields.contains(property) {
                    return false; // calling a field-held closure: dynamic
                }
                let Some((owner, func)) = self.methods.get(property).cloned() else {
                    return false;
                };
                self.internally_invoked.insert(property.clone());
                if !self.function_this_safe(&owner, property, func, false) {
                    return false;
                }
                // Arguments are vetted as ordinary expressions: a bare `this`
                // in value position, a `this`-capturing closure and a
                // non-field `this.x` read all reject there already. A declared
                // field READ passed along (`this.m(this.ents[id])`) hands the
                // callee a field's value, never the receiver, and must not
                // disqualify the caller — wolf-ecs `addComponent` /
                // `removeComponent` / `createEntity` each call a sibling
                // method with such an argument.
                args.iter().all(|a| self.expr_this_safe(a, ctx))
            }
            // `super(...)`: the parent constructor body was already vetted by
            // `ctor_chain_safe` (whole chain). Args must not leak `this`; in
            // constructor context, record them for parent-ctor parameter
            // resolution in the numeric-field proof.
            Expr::SuperCall(args) => {
                if ctx.1 == "constructor" {
                    if let Some(pos) = self.chain.iter().position(|c| c.name == ctx.0) {
                        if let Some(parent) = self.chain.get(pos + 1) {
                            self.super_call_args
                                .entry(parent.name.clone())
                                .or_default()
                                .push(args.as_slice());
                        }
                    }
                }
                args.iter()
                    .all(|a| !expr_mentions_this(a) && self.expr_this_safe(a, ctx))
            }
            // `super.m(...)` resolves on the parent chain with the same `this`.
            Expr::SuperMethodCall { method, args, .. } => {
                let Some((owner, func)) = self.methods.get(method).cloned() else {
                    return false;
                };
                self.internally_invoked.insert(method.clone());
                if !self.function_this_safe(&owner, method, func, false) {
                    return false;
                }
                args.iter().all(|a| self.expr_this_safe(a, ctx))
            }
            // Shape barriers on `this` inside a method body (the module-wide
            // kill already covers these; kept as defense in depth).
            Expr::Delete(inner)
                if matches!(
                    inner.as_ref(),
                    Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. }
                        if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                false
            }
            // Any other appearance of `this` — including inside closures —
            // is a potential leak.
            Expr::This => false,
            Expr::Closure { body, .. } => {
                // A closure that touches `this` (captures_this or body use)
                // leaks it; a `this`-free closure is fine but its body may
                // reference nothing we track here (locals are the outer
                // function's problem — the use walk already handled the
                // candidate local itself).
                if expr_mentions_this(e) {
                    return false;
                }
                let _ = body;
                true
            }
            _ => {
                let mut ok = true;
                perry_hir::walker::walk_expr_children(e, &mut |c| {
                    if ok {
                        ok = self.expr_this_safe(c, ctx);
                    }
                });
                ok
            }
        }
    }
}

/// Does the expression mention `this` anywhere (including closure bodies and
/// `captures_this`)?
/// `object` is `this` guarded for a private access of `kind` (0 field, 1
/// method) and `op` (0 read, 1 write).
fn private_this(object: &Expr, kind: u8, op: u8) -> bool {
    matches!(
        object,
        Expr::PrivateGuard {
            kind: k,
            op: o,
            object,
            ..
        } if *k == kind && *o == op && matches!(object.as_ref(), Expr::This)
    )
}

fn expr_mentions_this(e: &Expr) -> bool {
    let mut found = false;
    fn visit(e: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        match e {
            Expr::This => {
                *found = true;
            }
            Expr::Closure {
                body,
                captures_this,
                ..
            } => {
                if *captures_this {
                    *found = true;
                    return;
                }
                for s in body {
                    stmt_visit(s, found);
                }
            }
            _ => {}
        }
        if *found {
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit(c, found));
    }
    fn stmt_visit(s: &Stmt, found: &mut bool) {
        if *found {
            return;
        }
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    visit(e, found);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => visit(e, found),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit(condition, found);
                for s in then_branch {
                    stmt_visit(s, found);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                visit(condition, found);
                for s in body {
                    stmt_visit(s, found);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    stmt_visit(i, found);
                }
                if let Some(c) = condition {
                    visit(c, found);
                }
                if let Some(u) = update {
                    visit(u, found);
                }
                for s in body {
                    stmt_visit(s, found);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    stmt_visit(s, found);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        stmt_visit(s, found);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                visit(discriminant, found);
                for case in cases {
                    if let Some(t) = &case.test {
                        visit(t, found);
                    }
                    for s in &case.body {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::Labeled { body, .. } => stmt_visit(body.as_ref(), found),
            _ => {}
        }
    }
    visit(e, &mut found);
    found
}
