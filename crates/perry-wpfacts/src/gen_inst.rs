//! Constraint generation, part 4: per-call-site instances of statically
//! called functions.
//!
//! A call whose callee is known before solving (a module function, or a
//! local bound once to a closure) and that passes a compile-time constant
//! gets its own instance of the callee's body: fresh nodes for the
//! parameters, `let`s and result, the constants bound to their parameters,
//! and every `if` those constants decide folded to its taken branch. It is
//! the callee's summary instantiated at the call, so `require('./a')` and
//! `require('./b')` no longer meet in one parameter node, and a branch that
//! cannot run for this call contributes nothing to its result.
//!
//! The shared body is still walked once and serves every other call (value
//! calls, escaped uses); an instanced call connects only to its instance.
//! Codegen compiles one body for all of them, so what a local or a read site
//! may hold is the union over the shared walk and every instance
//! (`Gen::inst_locals`, the read-site merge in `derive`).
//!
//! Instances stay small and closed: no closures, classes, `this`,
//! `arguments`, rest or default parameters, async or generator bodies, and
//! a parameter bound to a constant is never assigned. Objects born inside an
//! instance get their own templates (heap cloning); codegen consumes class
//! facts only, and classes are never born per instance.

use crate::gen::{Fx, Gen};
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::{CompareOp, Expr, LogicalOp, Param, Stmt, UnaryOp};

/// Body-size bound (HIR expressions and statements) for an instance.
const MAX_BODY: usize = 2000;
/// Instances per function, and nested instance depth.
const MAX_PER_FN: u32 = 64;
const MAX_DEPTH: usize = 6;
/// A body this small is instanced at every static call, constant or not:
/// its parameters then meet only this call's arguments (per-call summaries).
/// Instances per such function are bounded by the program's call sites.
const SMALL_BODY: usize = 48;
const MAX_SMALL_PER_FN: u32 = 1 << 14;

/// A compile-time constant argument or condition value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Const {
    Str(String),
    Num(f64),
    Bool(bool),
    Undef,
    Null,
}

impl Const {
    fn truthy(&self) -> bool {
        match self {
            Const::Str(s) => !s.is_empty(),
            Const::Num(n) => *n != 0.0 && !n.is_nan(),
            Const::Bool(b) => *b,
            Const::Undef | Const::Null => false,
        }
    }

    fn type_of(&self) -> &'static str {
        match self {
            Const::Str(_) => "string",
            Const::Num(_) => "number",
            Const::Bool(_) => "boolean",
            Const::Undef => "undefined",
            Const::Null => "object",
        }
    }
}

/// A function whose body an instance walks.
#[derive(Clone, Copy)]
pub(crate) struct StaticFn<'a> {
    pub func_id: u32,
    pub params: &'a [Param],
    pub body: &'a [Stmt],
}

/// One instance being walked: its locals and its constant parameters.
pub(crate) struct Frame<'a> {
    pub module: u32,
    pub locals: FxHashMap<u32, NodeId>,
    pub consts: FxHashMap<u32, Const>,
    /// Parameters bound to a statically known function at this call.
    pub fns: FxHashMap<u32, StaticFn<'a>>,
}

/// What an instance's body must not contain, and the locals it assigns.
#[derive(Default)]
struct Scan {
    size: usize,
    closed: bool,
    lets: Vec<u32>,
    assigned: Vec<u32>,
}

impl Scan {
    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        self.size += 1;
        match s {
            Stmt::Let { id, init, .. } => {
                self.lets.push(*id);
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(e) = else_branch {
                    self.stmts(e);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = condition {
                    self.expr(c);
                }
                if let Some(u) = update {
                    self.expr(u);
                }
                self.stmts(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        self.lets.push(*id);
                    }
                    self.stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t);
                    }
                    self.stmts(&c.body);
                }
            }
            Stmt::PreallocateTdzBoxes(ids) => self.lets.extend(ids.iter().copied()),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.size += 1;
        match e {
            // Bodies, classes and the caller's frame (`this`, `new.target`,
            // `super`) are not instanced.
            Expr::Closure { .. }
            | Expr::This
            | Expr::NewTarget
            | Expr::ClassExprFresh { .. }
            | Expr::ClassIsFirstEvaluation { .. }
            | Expr::RegisterClassParentDynamic { .. }
            | Expr::RegisterClassCaptures { .. }
            | Expr::RefreshClassExprCaptures { .. }
            | Expr::RegisterClassStaticSymbol { .. }
            | Expr::RegisterClassComputedMethod { .. }
            | Expr::RegisterClassComputedAccessor { .. }
            | Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. } => {
                self.closed = false;
                return;
            }
            Expr::LocalSet(id, _) => self.assigned.push(*id),
            Expr::Update { id, .. } => self.assigned.push(*id),
            _ => {}
        }
        if self.closed {
            perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
        }
    }
}

impl<'a> Gen<'a> {
    /// The function a call's callee names before solving, if any.
    pub(crate) fn static_callee(&mut self, mi: u32, callee: &Expr) -> Option<StaticFn<'a>> {
        let module: &'a perry_hir::Module = self.modules[mi as usize].1;
        match callee {
            Expr::FuncRef(id) => {
                if !self.static_funcs.contains_key(&(mi, u32::MAX)) {
                    self.static_funcs.insert((mi, u32::MAX), None);
                    for f in &module.functions {
                        let sf = (!f.is_async && !f.is_generator).then_some(StaticFn {
                            func_id: f.id,
                            params: &f.params,
                            body: &f.body,
                        });
                        self.static_funcs.insert((mi, f.id), sf);
                    }
                }
                self.static_funcs.get(&(mi, *id)).copied().flatten()
            }
            Expr::LocalGet(id) => {
                if let Some(fr) = self.frames.last() {
                    if fr.module == mi {
                        if let Some(&sf) = fr.fns.get(id) {
                            return Some(sf);
                        }
                        if fr.locals.contains_key(id) {
                            return None;
                        }
                    }
                }
                if !self.static_locals_built.contains(&mi) {
                    self.build_static_locals(mi);
                }
                self.static_locals.get(&(mi, *id)).copied()
            }
            _ => None,
        }
    }

    /// Locals bound once to a closure and never assigned again, per module.
    fn build_static_locals(&mut self, mi: u32) {
        self.static_locals_built.insert(mi);
        let module: &'a perry_hir::Module = self.modules[mi as usize].1;
        let mut binds: FxHashMap<u32, Option<StaticFn<'a>>> = FxHashMap::default();
        let mut assigned: crate::pts::FxHashSet<u32> = Default::default();
        fn visit_stmts<'b>(
            ss: &'b [Stmt],
            binds: &mut FxHashMap<u32, Option<StaticFn<'b>>>,
            assigned: &mut crate::pts::FxHashSet<u32>,
        ) {
            for s in ss {
                let mut exprs: Vec<&'b Expr> = Vec::new();
                let mut kids: Vec<&'b [Stmt]> = Vec::new();
                match s {
                    Stmt::Let { id, init, .. } => {
                        if let Some(e) = init {
                            let sf = match e {
                                Expr::Closure {
                                    func_id,
                                    params,
                                    body,
                                    is_async: false,
                                    is_generator: false,
                                    ..
                                } => Some(StaticFn {
                                    func_id: *func_id,
                                    params,
                                    body,
                                }),
                                _ => None,
                            };
                            // A second binding of the same id: not static.
                            match binds.get(id) {
                                Some(_) => {
                                    binds.insert(*id, None);
                                }
                                None => {
                                    binds.insert(*id, sf);
                                }
                            }
                            exprs.push(e);
                        }
                    }
                    Stmt::Expr(e) | Stmt::Throw(e) => exprs.push(e),
                    Stmt::Return(Some(e)) => exprs.push(e),
                    Stmt::Return(None) => {}
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        exprs.push(condition);
                        kids.push(then_branch);
                        if let Some(e) = else_branch {
                            kids.push(e);
                        }
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                        exprs.push(condition);
                        kids.push(body);
                    }
                    Stmt::For {
                        init,
                        condition,
                        update,
                        body,
                    } => {
                        if let Some(i) = init {
                            visit_stmts(std::slice::from_ref(&**i), binds, assigned);
                        }
                        exprs.extend(condition.iter());
                        exprs.extend(update.iter());
                        kids.push(body);
                    }
                    Stmt::Labeled { body, .. } => {
                        visit_stmts(std::slice::from_ref(&**body), binds, assigned)
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        kids.push(body);
                        if let Some(c) = catch {
                            kids.push(&c.body);
                        }
                        if let Some(f) = finally {
                            kids.push(f);
                        }
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        exprs.push(discriminant);
                        for c in cases {
                            exprs.extend(c.test.iter());
                            kids.push(&c.body);
                        }
                    }
                    _ => {}
                }
                for e in exprs {
                    visit_expr(e, binds, assigned);
                }
                for k in kids {
                    visit_stmts(k, binds, assigned);
                }
            }
        }
        fn visit_expr<'b>(
            e: &'b Expr,
            binds: &mut FxHashMap<u32, Option<StaticFn<'b>>>,
            assigned: &mut crate::pts::FxHashSet<u32>,
        ) {
            match e {
                Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                    assigned.insert(*id);
                }
                Expr::Closure { body, params, .. } => {
                    for p in params {
                        if let Some(d) = &p.default {
                            visit_expr(d, binds, assigned);
                        }
                    }
                    visit_stmts(body, binds, assigned);
                    return;
                }
                _ => {}
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, binds, assigned));
        }
        visit_stmts(&module.init, &mut binds, &mut assigned);
        for f in &module.functions {
            visit_stmts(&f.body, &mut binds, &mut assigned);
        }
        for c in &module.classes {
            for m in c.methods.iter() {
                visit_stmts(&m.body, &mut binds, &mut assigned);
            }
            if let Some(ctor) = &c.constructor {
                visit_stmts(&ctor.body, &mut binds, &mut assigned);
            }
        }
        for (id, sf) in binds {
            if let Some(sf) = sf {
                if !assigned.contains(&id) {
                    self.static_locals.insert((mi, id), sf);
                }
            }
        }
    }

    /// The constant value of a pure expression in the current instance.
    pub(crate) fn const_eval(&mut self, mi: u32, e: &Expr) -> Option<Const> {
        self.const_eval_d(mi, e, 0)
    }

    fn const_eval_d(&mut self, mi: u32, e: &Expr, depth: usize) -> Option<Const> {
        if depth > 8 {
            return None;
        }
        Some(match e {
            Expr::String(s) => Const::Str(s.clone()),
            Expr::Number(n) => Const::Num(*n),
            Expr::Integer(n) => Const::Num(*n as f64),
            Expr::Bool(b) => Const::Bool(*b),
            Expr::Undefined => Const::Undef,
            Expr::Null => Const::Null,
            Expr::LocalGet(id) => {
                let fr = self.frames.last()?;
                if fr.module != mi {
                    return None;
                }
                fr.consts.get(id)?.clone()
            }
            Expr::TypeOf(x) => Const::Str(self.const_eval_d(mi, x, depth + 1)?.type_of().into()),
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => Const::Bool(!self.const_eval_d(mi, operand, depth + 1)?.truthy()),
            Expr::Compare { op, left, right } => {
                let l = self.const_eval_d(mi, left, depth + 1)?;
                let r = self.const_eval_d(mi, right, depth + 1)?;
                Const::Bool(compare(*op, &l, &r)?)
            }
            Expr::Logical { op, left, right } => {
                let l = self.const_eval_d(mi, left, depth + 1)?;
                let pick_left = match op {
                    LogicalOp::And => !l.truthy(),
                    LogicalOp::Or => l.truthy(),
                    LogicalOp::Coalesce => !matches!(l, Const::Undef | Const::Null),
                };
                if pick_left {
                    l
                } else {
                    self.const_eval_d(mi, right, depth + 1)?
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                if self.const_eval_d(mi, condition, depth + 1)?.truthy() {
                    self.const_eval_d(mi, then_expr, depth + 1)?
                } else {
                    self.const_eval_d(mi, else_expr, depth + 1)?
                }
            }
            Expr::Call { callee, args, .. } => {
                // A static function whose body is one `return <pure>`.
                let sf = self.static_callee(mi, callee)?;
                let [Stmt::Return(Some(ret))] = sf.body else {
                    return None;
                };
                let mut consts = FxHashMap::default();
                for (i, p) in sf.params.iter().enumerate() {
                    let v = match args.get(i) {
                        Some(a) => self.const_eval_d(mi, a, depth + 1)?,
                        None => Const::Undef,
                    };
                    consts.insert(p.id, v);
                }
                self.frames.push(Frame {
                    module: mi,
                    locals: FxHashMap::default(),
                    consts,
                    fns: FxHashMap::default(),
                });
                let v = self.const_eval_d(mi, ret, depth + 1);
                self.frames.pop();
                v?
            }
            _ => return self.const_eval_intrinsic(mi, e, depth),
        })
    }

    /// Constant string intrinsics and `isBuiltin` (the forms the lowering
    /// gives them are matched by name, see `PERRY_FACTS_INST_TRACE`).
    fn const_eval_intrinsic(&mut self, mi: u32, e: &Expr, depth: usize) -> Option<Const> {
        if let Expr::NativeMethodCall {
            module,
            method,
            args,
            ..
        } = e
        {
            let m = module.strip_prefix("node:").unwrap_or(module);
            if m == "module" && method == "isBuiltin" && args.len() == 1 {
                let Const::Str(s) = self.const_eval_d(mi, &args[0], depth + 1)? else {
                    return Some(Const::Bool(false));
                };
                return Some(Const::Bool(crate::builtins::is_builtin_module(&s)));
            }
        }
        None
    }

    /// Instance a call of `sf` with `args` (their nodes) and `consts` (their
    /// constant values, by position). `None`: not instanced; the caller
    /// connects the call to the shared body as before.
    pub(crate) fn instance_call(
        &mut self,
        fx: &Fx,
        sf: StaticFn<'a>,
        args: &[NodeId],
        consts: &[Option<Const>],
        fn_args: &[Option<StaticFn<'a>>],
    ) -> Option<NodeId> {
        let mi = fx.module;
        if self.frames.len() >= MAX_DEPTH || self.m.aborted {
            return None;
        }
        let fidx = *self.mods[mi as usize].funcs.get(&sf.func_id)?;
        let mut scan = Scan {
            closed: true,
            ..Default::default()
        };
        scan.stmts(sf.body);
        let small = scan.size <= SMALL_BODY;
        let given = consts.iter().any(Option::is_some) || fn_args.iter().any(Option::is_some);
        if !small && !given {
            return None;
        }
        let cap = if small { MAX_SMALL_PER_FN } else { MAX_PER_FN };
        if self.inst_count.get(&fidx).copied().unwrap_or(0) >= cap {
            return None;
        }
        for p in sf.params {
            if p.is_rest || p.default.is_some() || p.arguments_object.is_some() {
                return None;
            }
        }
        if !scan.closed || scan.size > MAX_BODY {
            return None;
        }
        *self.inst_count.entry(fidx).or_default() += 1;
        let trace = self.inst_trace;
        let mut fr = Frame {
            module: mi,
            locals: FxHashMap::default(),
            consts: FxHashMap::default(),
            fns: FxHashMap::default(),
        };
        for (i, p) in sf.params.iter().enumerate() {
            if let Some(Some(f)) = fn_args.get(i) {
                if !scan.assigned.contains(&p.id) {
                    fr.fns.insert(p.id, *f);
                }
            }
            let n = self.m.node_l("inst-param");
            match args.get(i) {
                Some(&a) => self.m.edge(a, n),
                None => self.m.add_prim(n, prim::UNDEF),
            }
            fr.locals.insert(p.id, n);
            let c = match args.get(i) {
                Some(_) => consts.get(i).cloned().flatten(),
                None => Some(Const::Undef),
            };
            if let Some(c) = c {
                if !scan.assigned.contains(&p.id) {
                    fr.consts.insert(p.id, c);
                }
            }
        }
        for &id in &scan.lets {
            if !fr.locals.contains_key(&id) {
                let n = self.m.node_l("inst-local");
                fr.locals.insert(id, n);
            }
        }
        if trace {
            eprintln!(
                "perry: wpfacts inst: {} consts {:?}",
                self.m.funcs[fidx as usize].name, fr.consts
            );
        }
        for (&id, &n) in &fr.locals {
            self.inst_locals.entry((mi, id)).or_default().push(n);
        }
        let ret = self.m.node_l("inst-ret");
        let this = self.m.node_l("inst-this");
        self.m.add_prim(this, prim::UNDEF);
        let ifx = Fx {
            module: mi,
            f: fidx,
            this,
            ret,
            class: None,
            ctor_of: None,
            new_target: self.m.all_prim_node,
            fn_ctor: false,
        };
        self.frames.push(fr);
        let ends = self.inst_stmts(&ifx, sf.body);
        self.frames.pop();
        if !ends {
            self.m.add_prim(ret, prim::UNDEF);
        }
        Some(ret)
    }

    /// Walk an instance's statements; `true` when they end in a definite
    /// `return` or `throw` (later statements of the block cannot run).
    fn inst_stmts(&mut self, fx: &Fx, ss: &[Stmt]) -> bool {
        for s in ss {
            if self.inst_stmt(fx, s) {
                return true;
            }
        }
        false
    }

    fn inst_stmt(&mut self, fx: &Fx, s: &Stmt) -> bool {
        match s {
            Stmt::Return(_) | Stmt::Throw(_) => {
                self.stmt(fx, s);
                true
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => match self.const_eval(fx.module, condition).map(|c| c.truthy()) {
                Some(true) => self.inst_stmts(fx, then_branch),
                Some(false) => else_branch
                    .as_ref()
                    .is_some_and(|e| self.inst_stmts(fx, e)),
                None => {
                    if self.inst_trace {
                        let mut d = format!("{condition:?}");
                        d.truncate(400);
                        eprintln!("perry: wpfacts inst: unfolded {d}");
                    }
                    let _ = self.expr(fx, condition);
                    let a = self.inst_stmts(fx, then_branch);
                    let b = else_branch
                        .as_ref()
                        .is_some_and(|e| self.inst_stmts(fx, e));
                    a && b
                }
            },
            _ => {
                self.stmt(fx, s);
                false
            }
        }
    }
}

fn compare(op: CompareOp, l: &Const, r: &Const) -> Option<bool> {
    let strict = |l: &Const, r: &Const| match (l, r) {
        (Const::Num(a), Const::Num(b)) => a == b,
        (a, b) => a == b,
    };
    Some(match op {
        CompareOp::Eq => strict(l, r),
        CompareOp::Ne => !strict(l, r),
        CompareOp::LooseEq | CompareOp::LooseNe => {
            let nullish = |c: &Const| matches!(c, Const::Undef | Const::Null);
            let eq = if nullish(l) || nullish(r) {
                nullish(l) && nullish(r)
            } else if l.type_of() == r.type_of() {
                strict(l, r)
            } else {
                return None;
            };
            if op == CompareOp::LooseEq {
                eq
            } else {
                !eq
            }
        }
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
            let (Const::Num(a), Const::Num(b)) = (l, r) else {
                return None;
            };
            match op {
                CompareOp::Lt => a < b,
                CompareOp::Le => a <= b,
                CompareOp::Gt => a > b,
                _ => a >= b,
            }
        }
    })
}
