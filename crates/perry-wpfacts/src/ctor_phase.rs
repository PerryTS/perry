//! The first of the two flow-sensitive pieces (design 3.3.1): construction
//! phase. Walks a class constructor body in program order and finds
//!
//! - the store sites `this.k = v` that run before `this` can be observed by
//!   anyone else ("construction-phase writers");
//! - the keys definitely assigned on every exit path, before any read of
//!   `this.k` and before `this` leaks (`definite`);
//! - whether `this` leaks at all.
//!
//! A leak is any use of `this` other than as the receiver of a named
//! property read/write in this body: passing it, storing it, calling a
//! method on it (the callee receives it), capturing it in a closure, a
//! `super` method call or accessor, and a read of a key not yet assigned
//! that is no method of the class (it consults the prototype chain, where
//! an accessor can run with `this`). Conservative by construction:
//! everything unrecognised closes the phase.

use perry_hir::walker::walk_expr_children;
use perry_hir::{Class, Expr, Stmt};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct CtorPhase {
    /// `this` leaks somewhere in the constructor body (or field inits).
    pub leaks: bool,
    /// A parent constructor leaks (set by the caller).
    pub parent_leaks: bool,
    /// Keys assigned in construction phase, first-assignment order.
    pub assigned_order: Vec<String>,
    /// Keys definitely assigned before any observation.
    pub definite: Vec<String>,
    /// Identity (`*const Expr as usize`) of construction-phase store exprs.
    pub phase_stores: BTreeSet<usize>,
}

struct St {
    open: bool,
    assigned: BTreeSet<String>,
}

struct W {
    ph: CtorPhase,
    /// The class's own plain methods: reading one runs no code.
    methods: BTreeSet<String>,
    late: BTreeSet<String>,
    read_before: BTreeSet<String>,
    exits: Vec<BTreeSet<String>>,
}

pub fn analyze(c: &Class) -> CtorPhase {
    let fields: Vec<(&Option<Expr>, Option<&str>)> = c
        .fields
        .iter()
        .map(|f| {
            let named = (!f.is_private && f.key_expr.is_none()).then_some(f.name.as_str());
            (&f.init, named)
        })
        .collect();
    let methods: BTreeSet<String> = c.methods.iter().map(|m| m.name.clone()).collect();
    match &c.constructor {
        Some(ctor) => analyze_body(&fields, methods, &ctor.params, &ctor.body),
        None => analyze_body(&fields, methods, &[], &[]),
    }
}

/// The construction phase of a constructor-function body (`new f()`).
pub fn analyze_fn(f: &perry_hir::Function) -> CtorPhase {
    analyze_body(&[], BTreeSet::new(), &f.params, &f.body)
}

/// Same for a closure body used as a constructor.
pub fn analyze_closure(params: &[perry_hir::Param], body: &[Stmt]) -> CtorPhase {
    analyze_body(&[], BTreeSet::new(), params, body)
}

fn analyze_body(
    fields: &[(&Option<Expr>, Option<&str>)],
    methods: BTreeSet<String>,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> CtorPhase {
    let mut w = W {
        ph: CtorPhase::default(),
        methods,
        late: BTreeSet::new(),
        read_before: BTreeSet::new(),
        exits: Vec::new(),
    };
    let mut st = St {
        open: true,
        assigned: BTreeSet::new(),
    };
    // Field initializers run first (base class) / right after super().
    for (init, named) in fields {
        if let Some(init) = init {
            w.expr(&mut st, init);
        }
        if let Some(n) = named {
            st.assigned.insert(n.to_string());
        }
    }
    for p in params {
        if let Some(d) = &p.default {
            w.expr(&mut st, d);
        }
    }
    w.stmts(&mut st, body);
    w.exits.push(st.assigned.clone());
    let mut definite: Option<BTreeSet<String>> = None;
    for e in &w.exits {
        definite = Some(match definite {
            None => e.clone(),
            Some(d) => d.intersection(e).cloned().collect(),
        });
    }
    let definite = definite.unwrap_or_default();
    w.ph.definite = definite
        .into_iter()
        .filter(|k| !w.late.contains(k) && !w.read_before.contains(k))
        // Declared fields are reported by the caller; keep ctor keys only.
        .filter(|k| w.ph.assigned_order.contains(k))
        .collect();
    w.ph
}

impl W {
    fn leak(&mut self, st: &mut St) {
        if st.open {
            st.open = false;
            self.ph.leaks = true;
            // Keys not yet assigned on this path may be observed unset.
            for k in &self.ph.assigned_order {
                if !st.assigned.contains(k) {
                    self.late.insert(k.clone());
                }
            }
        }
        self.ph.leaks = true;
    }

    fn stmts(&mut self, st: &mut St, ss: &[Stmt]) {
        for s in ss {
            self.stmt(st, s);
        }
    }

    fn branch(&mut self, st: &St, ss: &[Stmt]) -> St {
        let mut b = St {
            open: st.open,
            assigned: st.assigned.clone(),
        };
        self.stmts(&mut b, ss);
        b
    }

    fn stmt(&mut self, st: &mut St, s: &Stmt) {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.expr(st, e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(st, e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(st, e);
                }
                self.exits.push(st.assigned.clone());
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(st, condition);
                let a = self.branch(st, then_branch);
                let b = match else_branch {
                    Some(e) => self.branch(st, e),
                    None => St {
                        open: st.open,
                        assigned: st.assigned.clone(),
                    },
                };
                st.open = a.open && b.open;
                st.assigned = a.assigned.intersection(&b.assigned).cloned().collect();
            }
            Stmt::While { condition, body } => {
                self.expr(st, condition);
                let b = self.branch(st, body);
                st.open = st.open && b.open;
            }
            Stmt::DoWhile { body, condition } => {
                // The body runs at least once.
                self.stmts(st, body);
                self.expr(st, condition);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(st, i);
                }
                if let Some(c) = condition {
                    self.expr(st, c);
                }
                let mut b = self.branch(st, body);
                if let Some(u) = update {
                    self.expr(&mut b, u);
                }
                st.open = st.open && b.open;
            }
            Stmt::Labeled { body, .. } => {
                let b = self.branch(st, std::slice::from_ref(body.as_ref()));
                st.open = st.open && b.open;
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let b = self.branch(st, body);
                let mut open = st.open && b.open;
                if let Some(c) = catch {
                    let cb = self.branch(st, &c.body);
                    open = open && cb.open;
                }
                if let Some(f) = finally {
                    let fb = self.branch(st, f);
                    open = open && fb.open;
                }
                st.open = open;
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(st, discriminant);
                let mut open = st.open;
                for c in cases {
                    let mut b = St {
                        open: st.open,
                        assigned: st.assigned.clone(),
                    };
                    if let Some(t) = &c.test {
                        self.expr(&mut b, t);
                    }
                    self.stmts(&mut b, &c.body);
                    open = open && b.open;
                }
                st.open = open;
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn store(&mut self, st: &mut St, e: &Expr, key: &str) {
        if !self.ph.assigned_order.iter().any(|k| k == key) {
            self.ph.assigned_order.push(key.to_string());
        }
        if st.open {
            self.ph.phase_stores.insert(e as *const Expr as usize);
            st.assigned.insert(key.to_string());
        } else {
            self.late.insert(key.to_string());
        }
    }

    fn expr(&mut self, st: &mut St, e: &Expr) {
        match e {
            Expr::This => self.leak(st),
            // `this.m(...)`: the callee runs with `this` as its receiver.
            Expr::Call { callee, .. } | Expr::CallSpread { callee, .. }
                if matches!(callee.as_ref(), Expr::PropertyGet { object, .. }
                    if matches!(object.as_ref(), Expr::This)) =>
            {
                self.leak(st);
                walk_expr_children(e, &mut |c| self.expr(st, c));
            }
            // A `super` method or accessor runs with `this`.
            Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. } => {
                self.leak(st);
                walk_expr_children(e, &mut |c| self.expr(st, c));
            }
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                if !st.assigned.contains(property) {
                    self.read_before.insert(property.clone());
                    // Not own yet: the read walks the prototype chain, where
                    // an accessor runs with `this`.
                    if !self.methods.contains(property) {
                        self.leak(st);
                    }
                }
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } if matches!(object.as_ref(), Expr::This) => {
                self.expr(st, value);
                self.store(st, e, property);
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This)
                && matches!(key.as_ref(), Expr::String(_)) =>
            {
                self.expr(st, value);
                if let Expr::String(k) = key.as_ref() {
                    self.store(st, e, k);
                }
            }
            Expr::PropertyUpdate {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                if !st.assigned.contains(property) {
                    self.read_before.insert(property.clone());
                    if !self.methods.contains(property) {
                        self.leak(st);
                    }
                }
                self.store(st, e, property);
            }
            Expr::SuperCall(args) => {
                for a in args {
                    self.expr(st, a);
                }
            }
            Expr::Closure {
                captures_this,
                body,
                ..
            } => {
                if *captures_this || body_mentions_this(body) {
                    self.leak(st);
                }
            }
            _ => {
                walk_expr_children(e, &mut |c| self.expr(st, c));
            }
        }
    }
}

fn body_mentions_this(body: &[Stmt]) -> bool {
    fn ex(e: &Expr) -> bool {
        if matches!(e, Expr::This) {
            return true;
        }
        let mut found = false;
        walk_expr_children(e, &mut |c| {
            if !found && ex(c) {
                found = true;
            }
        });
        if let Expr::Closure { body, .. } = e {
            found = found || body_mentions_this(body);
        }
        found
    }
    body.iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| ex(e)))
}
