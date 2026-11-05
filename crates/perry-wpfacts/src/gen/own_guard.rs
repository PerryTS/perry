//! OWN1: a computed read guarded by an own-property test of the same object
//! and key (`Cons::IdxOwnLoad`, `solve/own_read.rs`).
//!
//! The test is `H.call(o, k)` (H's value gated to
//! `Object.prototype.hasOwnProperty` / `propertyIsEnumerable` at solve time,
//! and H's own `call` field to nothing written),
//! `o.hasOwnProperty(k)` (the member gated the same way) or
//! `Object.hasOwn(o, k)` (the `Object` constructor's `hasOwn` field gated
//! to nothing written), or `f(o,k)` (the callee value gated to program
//! functions with an exact own-test return). `o` and `k` are bindings (a local, a parameter,
//! `this`). The read is `o[k]` with the same bindings, and it is the first
//! thing the guarded region evaluates that can run code: the consequent of
//! `test ? … : …`, the right operand of `test && …`, the then-branch of
//! `if (test)`. Before it only bindings and literals are evaluated (also
//! the bindings of a store target, `out[k] = o[k]`), so nothing between
//! the test and the read can delete the key or rebind `o` or `k`.

use super::*;
use perry_hir::Expr;

/// The forms of the own-property test.
#[derive(Clone, Copy)]
pub(crate) enum OwnTest {
    /// `H.call(o, k)`.
    Call,
    /// `o.hasOwnProperty(k)`.
    Method,
    /// `Object.hasOwn(o, k)` (HIR `ObjectHasOwn`: the runtime's own).
    HasOwn,
    /// `f(o, k)`, gated on the callee value's body-derived predicate.
    Helper,
}

/// What the test guards: an expression or the statements of a branch.
pub(crate) enum Region<'e> {
    Expr(&'e Expr),
    Stmts(&'e [Stmt]),
}

/// The read a guarded region begins with (`read`: the `IndexGet`'s address).
pub(crate) struct OwnPlan {
    form: OwnTest,
    read: usize,
}

enum First {
    /// Only bindings and literals: nothing ran.
    Pure,
    /// The guarded read runs first.
    Read(usize),
    /// Something else may run first.
    Stop,
}

fn binding(e: &Expr) -> bool {
    matches!(e, Expr::LocalGet(_) | Expr::This)
}

fn same(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::LocalGet(x), Expr::LocalGet(y)) => x == y,
        (Expr::This, Expr::This) => true,
        _ => false,
    }
}

fn pure(e: &Expr) -> bool {
    binding(e)
        || matches!(
            e,
            Expr::Undefined
                | Expr::Null
                | Expr::Bool(_)
                | Expr::Number(_)
                | Expr::Integer(_)
                | Expr::String(_)
        )
}

/// The own-property test `cond`: its form, object and key.
fn own_test(cond: &Expr) -> Option<(OwnTest, &Expr, &Expr)> {
    let t = match cond {
        Expr::ObjectHasOwn(o, k) => (OwnTest::HasOwn, o.as_ref(), k.as_ref()),
        Expr::Call { callee, args, .. } => match (callee.as_ref(), args.as_slice()) {
            (Expr::PropertyGet { property, .. }, [o, k]) if property == "call" => {
                (OwnTest::Call, o, k)
            }
            (
                Expr::PropertyGet {
                    object, property, ..
                },
                [k],
            ) if property == "hasOwnProperty" => (OwnTest::Method, object.as_ref(), k),
            (Expr::LocalGet(_) | Expr::FuncRef(_) | Expr::ExternFuncRef { .. }, [o, k]) => {
                (OwnTest::Helper, o, k)
            }
            _ => return None,
        },
        _ => return None,
    };
    (binding(t.1) && binding(t.2)).then_some(t)
}

/// `f` when it found the read or stopped; after `Pure`, the operation that
/// follows its operands may run code.
fn then_stop(f: First) -> First {
    match f {
        First::Pure => First::Stop,
        x => x,
    }
}

/// Operands evaluated in order.
fn seq(es: &[&Expr], o: &Expr, k: &Expr) -> First {
    for e in es {
        match first(e, o, k) {
            First::Pure => {}
            x => return x,
        }
    }
    First::Pure
}

/// What `e` evaluates first, in evaluation order.
fn first(e: &Expr, o: &Expr, k: &Expr) -> First {
    if pure(e) {
        return First::Pure;
    }
    match e {
        Expr::IndexGet { object, index, .. } if same(object, o) && same(index, k) => {
            First::Read(e as *const Expr as usize)
        }
        Expr::IndexGet { object, index, .. } => then_stop(seq(&[object, index], o, k)),
        Expr::PropertyGet { object, .. } => then_stop(first(object, o, k)),
        // `x.m(…)`: `x`, then the member read; `f(…)`: `f` (a binding is a
        // value whose call may run first only after the arguments).
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet { object, .. } => then_stop(first(object, o, k)),
            c => then_stop(first(c, o, k)),
        },
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            then_stop(seq(&[left, right], o, k))
        }
        Expr::Logical { left, .. } => then_stop(first(left, o, k)),
        Expr::Conditional { condition, .. } => then_stop(first(condition, o, k)),
        Expr::Unary { operand, .. } => then_stop(first(operand, o, k)),
        Expr::LocalSet(_, value) => then_stop(first(value, o, k)),
        // `a[i] = v` evaluates `a` and `i`, then `v`: the store target is
        // only bindings, its key the read's own or a literal (any other key
        // may be an object whose conversion runs first).
        Expr::IndexSet {
            object,
            index,
            value,
            ..
        } if binding(object) && (same(index, k) || pure(index) && !binding(index)) => {
            then_stop(first(value, o, k))
        }
        Expr::PropertySet { object, value, .. } if binding(object) => then_stop(first(value, o, k)),
        // PutValue `a[i] = v` (target, key, value in source order).
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } if binding(target)
            && same(receiver, target)
            && (same(key, k) || pure(key) && !binding(key)) =>
        {
            then_stop(first(value, o, k))
        }
        _ => First::Stop,
    }
}

fn first_stmts(stmts: &[Stmt], o: &Expr, k: &Expr) -> First {
    for s in stmts {
        let f = match s {
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => First::Pure,
            // A new binding: never `o` or `k`.
            Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } => first(e, o, k),
            Stmt::Return(Some(e)) => then_stop(first(e, o, k)),
            _ => First::Stop,
        };
        match f {
            First::Pure => {}
            x => return x,
        }
    }
    First::Stop
}

impl<'a> Gen<'a> {
    /// Before the test `cond` guarding `region` is lowered: the read the
    /// region begins with when `cond` is an own-property test of its object
    /// and key.
    pub(crate) fn own_guard_plan(&mut self, cond: &Expr, region: Region<'_>) -> Option<OwnPlan> {
        self.own_test_recv = None;
        let (form, o, k) = own_test(cond)?;
        let f = match region {
            Region::Expr(e) => first(e, o, k),
            Region::Stmts(s) => first_stmts(s, o, k),
        };
        match f {
            First::Read(read) => Some(OwnPlan { form, read }),
            _ => None,
        }
    }

    /// After the test is lowered (its value `t`): derive the shared
    /// own-test predicate from its callee and result. Returns the read to
    /// forget once the region is lowered (`own_guard_done`).
    pub(crate) fn own_guard_arm(&mut self, plan: Option<OwnPlan>, t: NodeId) -> Option<usize> {
        let recv = self.own_test_recv.take();
        let plan = plan?;
        let gates = self.own_test_gates(plan.form, recv, t)?;
        let bad = self.tmp("own-test-guard");
        for (n, at) in gates {
            self.m.cons(n, Cons::OwnTest { dst: bad, at });
        }
        self.own_reads.insert(plan.read, bad);
        Some(plan.read)
    }

    /// The direct predicate is also the helper body's evidence. Only an
    /// exact return is accepted: no statement can rebind the operands,
    /// replace the test or run after it before the caller's read.
    pub(crate) fn own_helper_shape(
        &self,
        params: &[perry_hir::Param],
        body: &[Stmt],
        suspended: bool,
    ) -> Option<OwnTest> {
        if suspended
            || params.len() < 2
            || params
                .iter()
                .any(|p| p.default.is_some() || p.is_rest || p.arguments_object.is_some())
        {
            return None;
        }
        let [Stmt::Return(Some(e))] = body else {
            return None;
        };
        let (form, o, k) = own_test(e)?;
        if !matches!(o, Expr::LocalGet(id) if *id == params[0].id)
            || !matches!(k, Expr::LocalGet(id) if *id == params[1].id)
        {
            return None;
        }
        match form {
            OwnTest::HasOwn => Some(form),
            OwnTest::Call => {
                let Expr::Call { callee, .. } = e else {
                    return None;
                };
                let Expr::PropertyGet { object, .. } = callee.as_ref() else {
                    return None;
                };
                // Evaluating H must not run a closure that rebinds p0/p1.
                matches!(
                    object.as_ref(),
                    Expr::LocalGet(_) | Expr::ExternFuncRef { .. }
                )
                .then_some(form)
            }
            _ => None,
        }
    }

    pub(crate) fn own_helper_finish(&mut self, f: FnIdx, form: Option<OwnTest>) {
        let OwnTestBody::Pending(dst) = self.m.funcs[f as usize].own_test else {
            return;
        };
        self.m.funcs[f as usize].own_test = OwnTestBody::Checked(dst);
        let ret = self.m.funcs[f as usize].ret;
        let recv = self.own_test_recv.take();
        let form = form.filter(|_| !self.m.aborted);
        if let Some(gates) = form.and_then(|form| self.own_test_gates(form, recv, ret)) {
            for (n, at) in gates {
                self.m.cons(n, Cons::OwnTest { dst, at });
            }
        } else {
            self.m.refute_own_test(dst);
        }
    }

    fn own_test_gates(
        &mut self,
        form: OwnTest,
        recv: Option<NodeId>,
        t: NodeId,
    ) -> Option<Vec<(NodeId, OwnGateAt)>> {
        Some(match (form, recv) {
            (OwnTest::HasOwn, _) => {
                let obj = self.m.tok(TokKind::Builtin(BuiltinNs::Object));
                vec![(self.tok_node(obj), OwnGateAt::ObjectCtor)]
            }
            (OwnTest::Call, Some(h)) => vec![(h, OwnGateAt::OwnFn), (t, OwnGateAt::Value)],
            (OwnTest::Method, Some(o)) => {
                let a = self.m.atom("hasOwnProperty");
                let h = self.load(o, Key::Named(a));
                vec![(h, OwnGateAt::OwnFn), (t, OwnGateAt::Value)]
            }
            (OwnTest::Helper, Some(f)) => vec![(f, OwnGateAt::Helper), (t, OwnGateAt::Value)],
            (_, None) => return None,
        })
    }

    pub(crate) fn own_guard_done(&mut self, read: Option<usize>) {
        if let Some(r) = read {
            self.own_reads.remove(&r);
        }
    }
}
