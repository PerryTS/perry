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
use std::collections::{BTreeMap, BTreeSet};

/// `PERRY_FACTS_CTOR_ORDER=0`: a key the constructor may first store out of
/// the layout's order (or after such a store) stays definite, as before
/// (`CtorPhase::out_of_order`). Off is wrong: the store bypasses the
/// overflow the runtime installed for the out-of-order add, and the key is
/// missing from the object's enumeration.
pub(crate) fn ctor_order() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_CTOR_ORDER").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

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
    /// Keys the constructor body itself stores on every exit path before
    /// any observation. A declared field (`x: T;`) defines `undefined` at
    /// field-init time: that makes its key present (`definite`), not its
    /// `undefined` unobservable; only a store here does.
    pub stored: Vec<String>,
    /// Named fields defined after `this` may have been observed (an earlier
    /// value, or their own, leaked it): an observer can find them absent.
    pub late_fields: Vec<String>,
    /// Identity (`*const Expr as usize`) of construction-phase store exprs.
    pub phase_stores: BTreeSet<usize>,
    /// Keys whose first store may add them out of the layout's order (a key
    /// at a lower slot than one some run already added), or after such an
    /// add on the same run. The runtime orders the object by an overflow
    /// from the out-of-order add on, and a key added after it gets an
    /// entry there: its store must take the checked route, so it is no
    /// `definite` key (it reads behind the hole compare, as a reservation).
    pub out_of_order: BTreeSet<String>,
    /// Some run may leave the constructor with an out-of-order add behind
    /// it: a subclass's keys are then added to an object an overflow orders.
    pub disordered: bool,
}

#[derive(Clone)]
struct St {
    open: bool,
    assigned: BTreeSet<String>,
    /// One past the highest layout position a key added on some run so far
    /// holds (`W::order`).
    top: usize,
    /// Some run so far added a key out of the layout's order.
    disordered: bool,
}

impl St {
    /// `other`'s runs join this state's: the higher position, either order.
    fn join_order(&mut self, other: &St) {
        self.top = self.top.max(other.top);
        self.disordered |= other.disordered;
    }
}

struct W {
    ph: CtorPhase,
    /// The class's own plain methods: reading one runs no code.
    methods: BTreeSet<String>,
    late: BTreeSet<String>,
    read_before: BTreeSet<String>,
    exits: Vec<BTreeSet<String>>,
    /// The layout position of each key the class names (its fields in
    /// construction order); `None` for a constructor function, whose order
    /// is not tracked.
    order: Option<BTreeMap<String, usize>>,
}

pub fn analyze(c: &Class) -> CtorPhase {
    let fields: Vec<(&Option<Expr>, Option<&str>)> = c
        .fields
        .iter()
        .map(|f| {
            // Only a definition creates its key at field-init time. A
            // reservation (`ClassFieldOrigin::is_reservation`) is created by
            // its store, which a run can skip (a branch, a loop body, an
            // early `return`): the body walk below decides whether every
            // exit path stores it.
            let named = (!f.is_private && f.key_expr.is_none() && f.origin.is_definition())
                .then_some(f.name.as_str());
            (&f.init, named)
        })
        .collect();
    let methods: BTreeSet<String> = c.methods.iter().map(|m| m.name.clone()).collect();
    let (params, body): (&[perry_hir::Param], &[Stmt]) = match &c.constructor {
        Some(ctor) => (&ctor.params, &ctor.body),
        None => (&[], &[]),
    };
    let order: Option<BTreeMap<String, usize>> = ctor_order().then(|| {
        let mut m = BTreeMap::new();
        for f in perry_hir::private_names::construction_entries([c.fields.as_slice()]) {
            if !f.is_private && f.key_expr.is_none() {
                let n = m.len();
                m.entry(f.name.clone()).or_insert(n);
            }
        }
        m
    });
    let mut ph = analyze_body(&fields, methods.clone(), params, body, order.clone());
    // The same walk with only initialized fields holding a value: a key a
    // path leaves at its declared `undefined` is no `stored` key (a read of
    // it before the store closes the phase there: fewer keys, never more).
    let valued: Vec<(&Option<Expr>, Option<&str>)> = fields
        .iter()
        .map(|&(init, named)| (init, named.filter(|_| init.is_some())))
        .collect();
    ph.stored = analyze_body(&valued, methods, params, body, order).definite;
    ph.stored.retain(|k| !ph.out_of_order.contains(k));
    ph
}

/// The construction phase of a constructor-function body (`new f()`).
pub fn analyze_fn(f: &perry_hir::Function) -> CtorPhase {
    analyze_closure(&f.params, &f.body)
}

/// Same for a closure body used as a constructor. Its birth keys are its
/// first-store order (`assigned_order`, `Gen::note_fn_phase`): a second walk
/// checks the stores against that order.
pub fn analyze_closure(params: &[perry_hir::Param], body: &[Stmt]) -> CtorPhase {
    let ph = analyze_body(&[], BTreeSet::new(), params, body, None);
    if !ctor_order() || ph.assigned_order.is_empty() {
        return ph;
    }
    let order = ph
        .assigned_order
        .iter()
        .enumerate()
        .map(|(i, k)| (k.clone(), i))
        .collect();
    analyze_body(&[], BTreeSet::new(), params, body, Some(order))
}

fn analyze_body(
    fields: &[(&Option<Expr>, Option<&str>)],
    methods: BTreeSet<String>,
    params: &[perry_hir::Param],
    body: &[Stmt],
    order: Option<BTreeMap<String, usize>>,
) -> CtorPhase {
    let mut w = W {
        ph: CtorPhase::default(),
        methods,
        late: BTreeSet::new(),
        read_before: BTreeSet::new(),
        exits: Vec::new(),
        order,
    };
    let mut st = St {
        open: true,
        assigned: BTreeSet::new(),
        top: 0,
        disordered: false,
    };
    // A base constructor defines its fields right after binding `this`,
    // before the parameter defaults run (node: field, then param). A derived
    // one evaluates its parameter defaults first, but `this` is unbound
    // there, so they cannot observe the instance; its fields are defined
    // right after super().
    for (init, named) in fields {
        if let Some(init) = init {
            w.expr(&mut st, init);
        }
        if let Some(n) = named {
            if !st.open {
                w.ph.late_fields.push(n.to_string());
            }
            st.assigned.insert(n.to_string());
            if let Some(i) = w.order.as_ref().and_then(|o| o.get(*n)) {
                st.top = st.top.max(i + 1);
            }
        }
    }
    for p in params {
        if let Some(d) = &p.default {
            w.expr(&mut st, d);
        }
    }
    w.stmts(&mut st, body);
    w.exits.push(st.assigned.clone());
    w.ph.disordered |= st.disordered;
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
        .filter(|k| !w.ph.out_of_order.contains(k))
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
        let mut b = st.clone();
        self.stmts(&mut b, ss);
        b
    }

    /// A loop body: it may run any number of times, so a second run starts
    /// where the first left the order (a key the first run skipped may be
    /// added below one it added). Returns the body's state after both.
    fn loop_body(&mut self, st: &St, ss: &[Stmt]) -> St {
        let first = self.branch(st, ss);
        let mut again = st.clone();
        again.join_order(&first);
        let mut second = self.branch(&again, ss);
        second.open = second.open && first.open;
        second
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
                self.ph.disordered |= st.disordered;
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
                    None => st.clone(),
                };
                st.open = a.open && b.open;
                st.assigned = a.assigned.intersection(&b.assigned).cloned().collect();
                st.join_order(&a);
                st.join_order(&b);
            }
            Stmt::While { condition, body } => {
                self.expr(st, condition);
                let b = self.loop_body(st, body);
                st.open = st.open && b.open;
                st.join_order(&b);
            }
            // The body runs at least once (and maybe again), but a `break`
            // or `continue` in it can leave before its later stores: then it
            // is a loop body like any other.
            Stmt::DoWhile { body, condition } if ctor_order() && jumps(body) => {
                let b = self.loop_body(st, body);
                st.open = st.open && b.open;
                st.join_order(&b);
                self.expr(st, condition);
            }
            Stmt::DoWhile { body, condition } => {
                self.stmts(st, body);
                self.expr(st, condition);
                let b = self.loop_body(st, body);
                st.join_order(&b);
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
                let mut b = self.loop_body(st, body);
                if let Some(u) = update {
                    self.expr(&mut b, u);
                }
                st.open = st.open && b.open;
                st.join_order(&b);
            }
            Stmt::Labeled { body, .. } => {
                let b = self.loop_body(st, std::slice::from_ref(body.as_ref()));
                st.open = st.open && b.open;
                st.join_order(&b);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let b = self.branch(st, body);
                let mut open = st.open && b.open;
                // The handler and the finalizer run after any part of the
                // body: they start where it may have left the order.
                let mut after = st.clone();
                after.join_order(&b);
                if let Some(c) = catch {
                    let cb = self.branch(&after, &c.body);
                    open = open && cb.open;
                    after.join_order(&cb);
                }
                if let Some(f) = finally {
                    let fb = self.branch(&after, f);
                    open = open && fb.open;
                    after.join_order(&fb);
                }
                st.open = open;
                st.join_order(&after);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(st, discriminant);
                let mut open = st.open;
                // A case falls through into the next: each starts where the
                // cases before it may have left the order.
                let mut through = st.clone();
                for c in cases {
                    let mut b = through.clone();
                    b.open = st.open;
                    b.assigned = st.assigned.clone();
                    if let Some(t) = &c.test {
                        self.expr(&mut b, t);
                    }
                    self.stmts(&mut b, &c.body);
                    open = open && b.open;
                    through.join_order(&b);
                }
                st.open = open;
                st.join_order(&through);
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
        self.order_store(st, key);
        if st.open {
            self.ph.phase_stores.insert(e as *const Expr as usize);
            st.assigned.insert(key.to_string());
        } else {
            self.late.insert(key.to_string());
        }
    }

    /// The layout order at a store of `key`: a first store at a position
    /// below one some run already added is an out-of-order add (the runtime
    /// installs the overflow there), and so is any first store after one.
    /// A key the class's layout does not name (inherited, an accessor's or
    /// a method's name) may land anywhere: an out-of-order add. A private
    /// name is claimed at its declaration, not here.
    fn order_store(&mut self, st: &mut St, key: &str) {
        let Some(order) = &self.order else {
            return;
        };
        if key.starts_with('#') || st.assigned.contains(key) {
            return;
        }
        let at = order.get(key).copied();
        if st.disordered || at.is_none_or(|i| i + 1 < st.top) {
            st.disordered = true;
            self.ph.out_of_order.insert(key.to_string());
        }
        if let Some(i) = at {
            st.top = st.top.max(i + 1);
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

/// Some statement in `ss` (at any depth) is a `break` or `continue`.
fn jumps(ss: &[Stmt]) -> bool {
    ss.iter().any(|s| match s {
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => jumps(then_branch) || else_branch.as_deref().is_some_and(jumps),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            jumps(body)
        }
        Stmt::Labeled { body, .. } => jumps(std::slice::from_ref(body.as_ref())),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            jumps(body)
                || catch.as_ref().is_some_and(|c| jumps(&c.body))
                || finally.as_deref().is_some_and(jumps)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| jumps(&c.body)),
        _ => false,
    })
}

pub(crate) fn body_mentions_this(body: &[Stmt]) -> bool {
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
