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
    /// Rule BD-A: the `Object.defineProperty(this, "k", L)` /
    /// `Object.defineProperties(this, {k: L, ..})` statements (identity of
    /// the call expr, in `phase_stores` too) that define fresh keys with
    /// literal data descriptors (`literal_defines`) on every exit before
    /// `this` can be observed: each key is a birth key with the attribute
    /// entry the descriptor gives it (ECMA-262 20.1.2.3 / 20.1.2.4 ->
    /// 10.1.6.3 ValidateAndApplyPropertyDescriptor step 2: `current`
    /// undefined, the key is created with the descriptor's attributes).
    pub birth_defines: BTreeMap<usize, Vec<(String, u8)>>,
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
    /// `super()` has returned on every run so far (R9BIRTH1 rule C).
    supered: bool,
}

impl St {
    /// `other`'s runs join this state's: the higher position, either order.
    fn join_order(&mut self, other: &St) {
        self.top = self.top.max(other.top);
        self.disordered |= other.disordered;
    }
}

struct W<'a> {
    /// The data keys of the module's plain `__AnonShape_*` literal classes
    /// (`plain_shapes`).
    shapes: &'a BTreeMap<String, Vec<String>>,
    ph: CtorPhase,
    /// The class's own plain methods: reading one runs no code.
    methods: BTreeSet<String>,
    late: BTreeSet<String>,
    read_before: BTreeSet<String>,
    exits: Vec<BTreeSet<String>>,
    /// A class constructor (rule BD-A applies; a constructor function's
    /// defines stay post-birth adds).
    class_ctor: bool,
    /// The layout position of each key the class names (its fields in
    /// construction order); `None` for a constructor function, whose order
    /// is not tracked.
    order: Option<BTreeMap<String, usize>>,
    /// R9BIRTH1 rule C: the keys the parent chain's construction makes own
    /// data properties on every run (`analyze_in_present`): present once
    /// `super()` has returned.
    present: &'a BTreeSet<String>,
    /// R9BIRTH2 rule L: `const` locals initialised with `this`
    /// (`const scope = this`). Binding one observes nothing; any other use
    /// of it is a use of `this` (a leak): a read of the local, a closure
    /// that captures it.
    aliases: BTreeSet<perry_hir::types::LocalId>,
    /// Aliases whose `const` statement the walk has not reached yet.
    alias_pending: BTreeSet<perry_hir::types::LocalId>,
    /// Pending aliases a closure (a hoisted function declaration) captured.
    alias_early: BTreeSet<perry_hir::types::LocalId>,
}

pub fn analyze(c: &Class) -> CtorPhase {
    analyze_in(c, &BTreeMap::new())
}

/// The field keys of each plain-literal class of `classes`: an object
/// literal lowers to `new __AnonShape_*(values...)` with its keys as the
/// class's fields, in order. Only one of named public fields, no method,
/// accessor or parent.
pub fn plain_shapes(classes: &[Class]) -> BTreeMap<String, Vec<String>> {
    classes
        .iter()
        .filter(|c| {
            c.name.starts_with("__AnonShape_")
                && c.methods.is_empty()
                && c.getters.is_empty()
                && c.setters.is_empty()
                && c.extends.is_none()
                && c.extends_name.is_none()
                && c.extends_expr.is_none()
                && c.native_extends.is_none()
                && c.fields.iter().all(|f| !f.is_private && f.key_expr.is_none())
        })
        .map(|c| (c.name.clone(), c.fields.iter().map(|f| f.name.clone()).collect()))
        .collect()
}

pub fn analyze_in(c: &Class, shapes: &BTreeMap<String, Vec<String>>) -> CtorPhase {
    analyze_in_present(c, shapes, &BTreeSet::new())
}

/// [`analyze_in`] of a derived class whose parent chain's construction
/// leaves the keys `present` as own data properties on every run, before
/// anyone can observe the instance (R9BIRTH1 rule C, ECMA-262 10.1.9.2
/// OrdinarySetWithOwnDescriptor: a store of an own data key changes its
/// value and creates no key, so it is no add at any layout position).
pub fn analyze_in_present(
    c: &Class,
    shapes: &BTreeMap<String, Vec<String>>,
    present: &BTreeSet<String>,
) -> CtorPhase {
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
    let mut ph = analyze_body(shapes, &fields, methods.clone(), params, body, order.clone(), true, present);
    // The same walk with only initialized fields holding a value: a key a
    // path leaves at its declared `undefined` is no `stored` key (a read of
    // it before the store closes the phase there: fewer keys, never more).
    let valued: Vec<(&Option<Expr>, Option<&str>)> = fields
        .iter()
        .map(|&(init, named)| (init, named.filter(|_| init.is_some())))
        .collect();
    ph.stored = analyze_body(shapes, &valued, methods, params, body, order, true, present).definite;
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
    // A closure or function names no class: no class shape to read
    // (`plain_shapes`).
    let shapes = BTreeMap::new();
    let ph = analyze_body(&shapes, &[], BTreeSet::new(), params, body, None, false, &BTreeSet::new());
    if !ctor_order() || ph.assigned_order.is_empty() {
        return ph;
    }
    let order = ph
        .assigned_order
        .iter()
        .enumerate()
        .map(|(i, k)| (k.clone(), i))
        .collect();
    analyze_body(&shapes, &[], BTreeSet::new(), params, body, Some(order), false, &BTreeSet::new())
}

fn analyze_body(
    shapes: &BTreeMap<String, Vec<String>>,
    fields: &[(&Option<Expr>, Option<&str>)],
    methods: BTreeSet<String>,
    params: &[perry_hir::Param],
    body: &[Stmt],
    order: Option<BTreeMap<String, usize>>,
    class_ctor: bool,
    present: &BTreeSet<String>,
) -> CtorPhase {
    let mut w = W {
        present,
        class_ctor,
        shapes,
        ph: CtorPhase::default(),
        methods,
        late: BTreeSet::new(),
        read_before: BTreeSet::new(),
        exits: Vec::new(),
        order,
        aliases: BTreeSet::new(),
        alias_pending: BTreeSet::new(),
        alias_early: BTreeSet::new(),
    };
    let mut st = St {
        open: true,
        assigned: BTreeSet::new(),
        top: 0,
        disordered: false,
        supered: false,
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
    // Rule L: the body's top-level `const x = this` aliases, known before
    // the walk: a function declaration is hoisted (its closure is created
    // at the top of the body, ECMA-262 10.2.11 step 36), ahead of the
    // alias's own statement; its capture of the alias is a use of `this`
    // from the alias's statement on (`alias_early`).
    for s in body {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::This),
            ..
        } = s
        {
            w.aliases.insert(*id);
            w.alias_pending.insert(*id);
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
    // BD-A holds only for a define whose keys are all definite: one some run
    // skips (a branch, a loop body) or that runs after an observation is a
    // define on an object that may lack the key, as before (a post-birth add).
    let dropped: Vec<usize> = w
        .ph
        .birth_defines
        .iter()
        .filter(|(_, ks)| !ks.iter().all(|(k, _)| w.ph.definite.contains(k)))
        .map(|(&id, _)| id)
        .collect();
    for id in dropped {
        let ks = w.ph.birth_defines.remove(&id).unwrap_or_default();
        w.ph.phase_stores.remove(&id);
        w.ph.assigned_order.retain(|k| !ks.iter().any(|(x, _)| x == k));
    }
    w.ph
}

impl W<'_> {

    /// The (key, value) parts of object literal `e` (an `Expr::Object` or a
    /// plain `__AnonShape_*` construction), or `None`: a spread, a computed
    /// key, a method or accessor is no such literal.
    fn literal_parts<'e>(&'e self, e: &'e Expr) -> Option<Vec<(&'e str, &'e Expr)>> {
        literal_parts(self.shapes, e)
    }

    /// Rule BD-A: `k` may become a birth key by a define here: no earlier
    /// store, read or define of it, not a method of the class (a method is
    /// the prototype's), not `__proto__` / `constructor`.
    fn fresh_define(&self, k: &str) -> bool {
        k != "__proto__"
            && k != "constructor"
            && !self.ph.assigned_order.iter().any(|x| x == k)
            && !self.read_before.contains(k)
            && !self.late.contains(k)
            && !self.methods.contains(k)
    }


    /// An object literal of only the data-descriptor fields `value`,
    /// `writable`, `enumerable`, `configurable` (ECMA-262 6.2.6.5): no
    /// spread, no computed key, no `__proto__` (it would set the literal's
    /// prototype), no `get` / `set` of its own.
    fn data_descriptor(&self, d: &Expr) -> bool {
        self.literal_parts(d).is_some_and(|ps| {
            ps.iter().all(|(k, _)| {
                matches!(*k, "value" | "writable" | "enumerable" | "configurable")
            })
        })
    }
    /// An inherited key that is an own data property on this run (rule C).
    fn is_present(&self, st: &St, key: &str) -> bool {
        st.open && st.supered && self.present.contains(key)
    }

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
            // Rule L: `const scope = this` binds the value and runs no code.
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::This),
                ..
            } if self.aliases.contains(id) => {
                // A closure created above (hoisted) holds the binding: from
                // here on its calls see `this` (before, they throw: the
                // binding is uninitialised, 9.1.1.1.6 GetBindingValue).
                self.alias_pending.remove(id);
                if self.alias_early.contains(id) {
                    self.leak(st);
                }
            }
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
                st.supered = a.supered && b.supered;
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
                // R9BIRTH1 rule B (ECMA-262 14.12.4 CaseBlockEvaluation): a
                // case body runs after the selection, which evaluates the
                // case tests (all of them before a `default`), or by falling
                // through from the body above when that body completes
                // normally. Each case starts from the dispatch (every test
                // evaluated: the widest order) and, unless the body above
                // ends in a jump, from where that body left the order.
                let mut disp = st.clone();
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(&mut disp, t);
                    }
                }
                let mut open = disp.open;
                let mut through = disp.clone();
                let mut above: Option<St> = None;
                // The keys every run that leaves the switch to its next
                // statement has assigned: known when each body's only jump
                // is its last statement (`case_exit`).
                let mut exits: Option<Vec<BTreeSet<String>>> = Some(Vec::new());
                for (ci, c) in cases.iter().enumerate() {
                    let mut b = disp.clone();
                    b.open = st.open;
                    b.assigned = st.assigned.clone();
                    if let Some(a) = &above {
                        b.join_order(a);
                        b.open = b.open && a.open;
                    }
                    self.stmts(&mut b, &c.body);
                    open = open && b.open;
                    through.join_order(&b);
                    match case_exit(&c.body) {
                        Some(CaseEnd::Leaves) => {}
                        // A body that falls through leaves the switch only
                        // when it is the last one.
                        Some(CaseEnd::Falls) if ci + 1 < cases.len() => {}
                        Some(CaseEnd::Exits | CaseEnd::Falls) => {
                            if let Some(e) = exits.as_mut() {
                                e.push(b.assigned.clone());
                            }
                        }
                        None => exits = None,
                    }
                    let falls = matches!(case_exit(&c.body), Some(CaseEnd::Falls) | None);
                    above = falls.then_some(b);
                }
                // No case matched and there is no `default`: the switch is
                // left with the dispatch's keys.
                if !cases.iter().any(|c| c.test.is_none()) {
                    if let Some(e) = exits.as_mut() {
                        e.push(st.assigned.clone());
                    }
                }
                st.open = open;
                st.join_order(&through);
                if let Some(e) = exits {
                    let mut it = e.into_iter();
                    if let Some(first) = it.next() {
                        st.assigned = it.fold(first, |acc, s| acc.intersection(&s).cloned().collect());
                    }
                }
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
        if key.starts_with('#') || st.assigned.contains(key) || self.is_present(st, key) {
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
            // `delete this.k` removes the key (10.1.10 OrdinaryDelete): a
            // later store adds it again, at the end of the layout, and the
            // instance is no longer the one construction lays out. The
            // phase closes there.
            Expr::Delete(inner)
                if matches!(inner.as_ref(), Expr::PropertyGet { object, .. }
                    if matches!(object.as_ref(), Expr::This)) =>
            {
                if let Expr::PropertyGet { property, .. } = inner.as_ref() {
                    st.assigned.remove(property);
                }
                self.leak(st);
            }
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                if !st.assigned.contains(property) && !self.is_present(st, property) {
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
                if !st.assigned.contains(property) && !self.is_present(st, property) {
                    self.read_before.insert(property.clone());
                    if !self.methods.contains(property) {
                        self.leak(st);
                    }
                }
                self.store(st, e, property);
            }
            // Rule BD-A: a define of fresh keys with literal data
            // descriptors while `this` is unobserved creates each key
            // (10.1.6.3 step 2) with the descriptor's value: a birth store
            // that runs no code (no setter: [[DefineOwnProperty]]). Its
            // position is the family layout's (codegen `define_stores`), not
            // the [[Set]] order `order_store` tracks.
            Expr::ObjectDefineProperty(t, ..) | Expr::ObjectDefineProperties(t, ..)
                if self.class_ctor
                    && st.open
                    && matches!(t.as_ref(), Expr::This)
                    && literal_defines(self.shapes, e)
                        .is_some_and(|ds| ds.iter().all(|d| self.fresh_define(d.0))) =>
            {
                let ds = literal_defines(self.shapes, e).unwrap_or_default();
                for d in &ds {
                    for v in &d.3 {
                        self.expr(st, v);
                    }
                }
                if st.open {
                    let id = e as *const Expr as usize;
                    for d in &ds {
                        self.ph.assigned_order.push(d.0.to_string());
                        st.assigned.insert(d.0.to_string());
                    }
                    self.ph.phase_stores.insert(id);
                    self.ph
                        .birth_defines
                        .insert(id, ds.iter().map(|d| (d.0.to_string(), d.1)).collect());
                }
            }
            // ECMA-262 20.1.2.4 Object.defineProperty / 20.1.2.3
            // Object.defineProperties on `this` with plain data-descriptor
            // literals: `this` as the target is no leak. ToPropertyDescriptor
            // (6.2.6.5) reads the descriptor's fields with HasProperty / Get on
            // the literal (an accessor an Object.prototype field may hold
            // runs with the literal as receiver, never with `this`), and
            // OrdinaryDefineOwnProperty (10.1.6.1) on the ordinary `this` runs
            // no code. The key and the values are walked as any expression: a
            // value that holds or passes `this` leaks it. The defined key is
            // not assigned: it may be an accessor (an inherited `get` / `set`).
            Expr::ObjectDefineProperty(t, k, d)
                if matches!(t.as_ref(), Expr::This)
                    && matches!(k.as_ref(), Expr::String(_))
                    && self.data_descriptor(d) =>
            {
                self.expr(st, d);
            }
            Expr::ObjectDefineProperties(t, ds)
                if matches!(t.as_ref(), Expr::This)
                    && self.literal_parts(ds).is_some_and(|ps| {
                        ps.iter().all(|(k, d)| *k != "__proto__" && self.data_descriptor(d))
                    }) =>
            {
                self.expr(st, ds);
            }
            Expr::SuperCall(args) => {
                for a in args {
                    self.expr(st, a);
                }
                st.supered = true;
            }
            Expr::Closure {
                captures_this,
                body,
                captures,
                mutable_captures,
                ..
            } => {
                let mut alias = false;
                for c in captures.iter().chain(mutable_captures) {
                    if self.alias_pending.contains(c) {
                        self.alias_early.insert(*c);
                    } else if self.aliases.contains(c) {
                        alias = true;
                    }
                }
                if *captures_this || body_mentions_this(body) || alias {
                    self.leak(st);
                }
            }
            // Rule L: the value of a `this` alias is `this`.
            Expr::LocalGet(id) if self.aliases.contains(id) => self.leak(st),
            _ => {
                walk_expr_children(e, &mut |c| self.expr(st, c));
            }
        }
    }
}

/// The (key, value) parts of object literal `e` (an `Expr::Object` or a
/// plain `__AnonShape_*` construction, `plain_shapes`), or `None`: a spread,
/// a computed key, a method or accessor is no such literal.
pub(crate) fn literal_parts<'e>(
    shapes: &'e BTreeMap<String, Vec<String>>,
    e: &'e Expr,
) -> Option<Vec<(&'e str, &'e Expr)>> {
    match e {
        Expr::Object(ps) => Some(ps.iter().map(|(k, v)| (k.as_str(), v)).collect()),
        Expr::New {
            class_name, args, ..
        } => {
            let keys = shapes.get(class_name)?;
            (keys.len() == args.len())
                .then(|| keys.iter().map(|k| k.as_str()).zip(args.iter()).collect())
        }
        _ => None,
    }
}

/// One key a define statement creates (rule BD-A): its name, its attribute
/// entry (`perry_abi::LAYOUT_KEY_*`), and the descriptor's field values in
/// evaluation order (`value` among them, or none: `undefined`).
pub(crate) type BirthDefine<'e> = (&'e str, u8, Option<&'e Expr>, Vec<&'e Expr>);

/// The keys `e` defines when it is `Object.defineProperty(this, "k", L)` or
/// `Object.defineProperties(this, {k: L, ..})` and every `L` is a literal
/// DATA descriptor with literal attributes (`perry_hir::descriptor::
/// literal_descriptor_attrs`, the definition the family layout uses), in
/// definition order. `None` for any other expr, an accessor, a computed,
/// duplicated or array-index key (`defineProperties` defines index keys
/// first, 20.1.2.3.1 step 3 OwnPropertyKeys order).
pub(crate) fn literal_defines<'e>(
    shapes: &'e BTreeMap<String, Vec<String>>,
    e: &'e Expr,
) -> Option<Vec<BirthDefine<'e>>> {
    let one = |k: &'e str, d: &'e Expr| -> Option<BirthDefine<'e>> {
        if k.parse::<u32>().is_ok() {
            return None;
        }
        let fields = literal_parts(shapes, d)?;
        let attrs = perry_hir::descriptor::literal_descriptor_attrs(&fields)?;
        if attrs & perry_abi::LAYOUT_KEY_ACCESSOR != 0 {
            return None;
        }
        let value = fields.iter().find(|(n, _)| *n == "value").map(|(_, v)| *v);
        Some((k, attrs, value, fields.into_iter().map(|(_, v)| v).collect()))
    };
    match e {
        Expr::ObjectDefineProperty(t, k, d) if matches!(t.as_ref(), Expr::This) => {
            let Expr::String(k) = k.as_ref() else {
                return None;
            };
            Some(vec![one(k.as_str(), d)?])
        }
        Expr::ObjectDefineProperties(t, ps) if matches!(t.as_ref(), Expr::This) => {
            let mut out: Vec<BirthDefine<'e>> = Vec::new();
            for (k, d) in literal_parts(shapes, ps)? {
                if out.iter().any(|x| x.0 == k) {
                    return None;
                }
                out.push(one(k, d)?);
            }
            Some(out)
        }
        _ => None,
    }
}

/// Some `return` in constructor body `ss` returns a value: `super()` may
/// then bind `this` to another object (ECMA-262 10.2.2 step 10-12).
pub(crate) fn returns_value(ss: &[Stmt]) -> bool {
    ss.iter().any(|s| match s {
        Stmt::Return(e) => e.is_some(),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => returns_value(then_branch) || else_branch.as_deref().is_some_and(returns_value),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            returns_value(body)
        }
        Stmt::Labeled { body, .. } => returns_value(std::slice::from_ref(body.as_ref())),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            returns_value(body)
                || catch.as_ref().is_some_and(|c| returns_value(&c.body))
                || finally.as_deref().is_some_and(returns_value)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| returns_value(&c.body)),
        _ => false,
    })
}

/// How a switch case body ends (`case_exit`).
enum CaseEnd {
    /// It completes normally: it falls through into the next body.
    Falls,
    /// Its last statement is a `break` (or a labeled one, maybe of the
    /// switch itself): the run may continue after the switch.
    Exits,
    /// Its last statement is a `return`, `throw` or `continue`: the run
    /// never continues after the switch from here.
    Leaves,
}

/// The end of switch case body `ss` when its only jump is its last
/// statement; `None` when a jump sits elsewhere in it (a run may leave the
/// body part way, with fewer keys).
fn case_exit(ss: &[Stmt]) -> Option<CaseEnd> {
    let Some((last, init)) = ss.split_last() else {
        return Some(CaseEnd::Falls);
    };
    if jumps(init) {
        return None;
    }
    Some(match last {
        Stmt::Break | Stmt::LabeledBreak(_) => CaseEnd::Exits,
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Continue | Stmt::LabeledContinue(_) => CaseEnd::Leaves,
        s if jumps(std::slice::from_ref(s)) => return None,
        _ => CaseEnd::Falls,
    })
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
