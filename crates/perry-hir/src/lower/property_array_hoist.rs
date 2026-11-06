//! Hoist a loop-invariant `RECV.PROP` array receiver out of a counted `for`.
//!
//! `for (let i = 0; i < holder.arr.length; i++) l = holder.arr[i];` re-runs a
//! by-name property lookup on **every iteration** — the emitted body carries
//! `js_object_get_field_by_name_f64` plus IC-miss handling — and, worse, the
//! packed-array machinery never engages at all, because its matcher requires
//! the array expression to be a bare local. Measured on a quiet host, that
//! shape costs 20.58 ns/iteration against node's 0.54; writing the hoist by
//! hand (`const a = holder.arr;`) drops it to 0.50, i.e. parity. This pass
//! performs that rewrite when it is observationally equivalent.
//!
//! Equivalence rests on three conditions, all checked before rewriting:
//!
//! 1. **The property is a data field, not an accessor.** Only receivers whose
//!    static type is a class this module synthesized for a closed-shape object
//!    literal (`__AnonShape_*`) qualify. `is_closed_shape` rejects getters and
//!    setters outright, so such a class's fields are data by construction and
//!    reading one is side-effect free — hoisting cannot change how many times
//!    user code runs.
//! 2. **Nothing in the loop can rebind the receiver.** A `holder = other`
//!    assignment would leave the hoisted temp pointing at the previous
//!    object's array. Note no shape- or identity-based runtime check can
//!    recover this: two objects from the same literal share a shape, so the
//!    rewrite must simply refuse.
//! 3. **Nothing in the loop can write the property or call anything.** A call
//!    could assign `holder.arr` behind our back, and a direct write is visible
//!    syntactically. The scan below rejects calls, closures, `new`, property
//!    and index writes, and anything it does not positively recognise.
//!
//! Condition 3 is deliberately stricter than necessary; it matches the shape
//! the packed-array machinery admits anyway, which is exactly where the win
//! is, and it keeps this pass from having to reason about aliasing.

use crate::ir::{Expr, Stmt};
use crate::lower::LoweringContext;
use crate::types::Type;

/// Rewrites `condition`/`body` to read a hoisted local and returns the `Let`
/// that materialises it, or `None` when the loop does not qualify.
pub(crate) fn hoist_loop_invariant_property_array(
    ctx: &mut LoweringContext,
    condition: &Expr,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<(Stmt, Expr, Vec<Stmt>)> {
    if !hoist_enabled() {
        return None;
    }
    let (recv_id, property) = counted_loop_property_array(condition)?;
    if !property_is_anon_shape_data_field(ctx, recv_id, &property) {
        return None;
    }
    let recv = Recv::Local(recv_id);
    if !loop_is_hoist_safe(condition, update, body, recv, &[]) {
        return None;
    }
    // Reads of `RECV.PROP` must actually occur in the body, or the rewrite
    // moves work without removing any.
    if !body
        .iter()
        .any(|stmt| stmt_reads_property(stmt, recv, &property))
    {
        return None;
    }

    let element_ty = ctx
        .closed_shape_literal_locals
        .get(&recv_id)
        .cloned()
        .and_then(|class_name| anon_shape_field_type(ctx, &class_name, &property))
        .or_else(|| match ctx.lookup_local_type_by_id(recv_id) {
            Some(Type::Object(obj)) => obj.properties.get(&property).map(|p| p.ty.clone()),
            _ => None,
        })
        .unwrap_or(Type::Any);

    let hoist_id = ctx.define_local(format!("__perry_hoist_{property}"), element_ty.clone());
    ctx.immutable_locals.insert(hoist_id);

    let init = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::LocalGet(recv_id)),
        property: property.clone(),
        byte_offset: 0,
    };
    let hoist = Stmt::Let {
        id: hoist_id,
        name: format!("__perry_hoist_{property}"),
        ty: element_ty,
        mutable: false,
        init: Some(init),
    };

    let new_condition = rewrite_expr(condition, recv, &property, hoist_id);
    let new_body = body
        .iter()
        .map(|stmt| rewrite_stmt(stmt, recv, &property, hoist_id))
        .collect();
    Some((hoist, new_condition, new_body))
}

/// `PERRY_LOOP_PROPERTY_HOIST=0` restores the pre-hoist lowering, so the pass
/// can be A/B'd on a single build and switched off in the field if a program
/// ever slips past the three equivalence checks.
fn hoist_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_LOOP_PROPERTY_HOIST").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `i < RECV.PROP.length` — returns `(RECV, PROP)`.
fn counted_loop_property_array(condition: &Expr) -> Option<(u32, String)> {
    let Expr::Compare { op, right, .. } = condition else {
        return None;
    };
    if !matches!(op, crate::ir::CompareOp::Lt | crate::ir::CompareOp::Le) {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = right.as_ref()
    else {
        return None;
    };
    if property != "length" {
        return None;
    }
    let Expr::PropertyGet {
        object: recv,
        property: array_prop,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    match recv.as_ref() {
        Expr::LocalGet(id) => Some((*id, array_prop.clone())),
        _ => None,
    }
}

/// Condition 1: the receiver's static type is a synthesized closed-shape
/// literal class, whose members are data fields by construction.
fn property_is_anon_shape_data_field(ctx: &LoweringContext, recv_id: u32, property: &str) -> bool {
    // Keyed on the INITIALIZER, not the binding's type. A getter-bearing
    // literal infers as `Any` so a type check would happen to reject it, but
    // an annotated structural object type can still be backed by an accessor —
    // only "this binding was initialized by a closed-shape literal" actually
    // proves the read is side-effect free.
    let Some(class_name) = ctx.closed_shape_literal_locals.get(&recv_id) else {
        return false;
    };
    ctx.anon_shape_fields
        .get(class_name)
        .is_some_and(|fields| fields.iter().any(|field| field == property))
}

fn anon_shape_field_type(ctx: &LoweringContext, class_name: &str, property: &str) -> Option<Type> {
    let idx = *ctx.classes_index.get(class_name)?;
    ctx.pending_classes
        .get(idx)
        .or_else(|| ctx.pending_classes.iter().find(|c| c.name == class_name))
        .and_then(|class| {
            class
                .fields
                .iter()
                .find(|field| field.name == property)
                .map(|field| field.ty.clone())
        })
}

/// Conditions 2 and 3 over the whole loop.
fn loop_is_hoist_safe(
    condition: &Expr,
    update: Option<&Expr>,
    body: &[Stmt],
    recv: Recv,
    writable: &[String],
) -> bool {
    let scan = Scan { recv, writable };
    expr_is_hoist_safe(condition, &scan)
        && update.is_none_or(|expr| expr_is_hoist_safe(expr, &scan))
        && body.iter().all(|stmt| stmt_is_hoist_safe(stmt, &scan))
}

/// What the safety walk is checking against: the hoisted receiver, and the
/// declared data fields of `this` the loop may store (empty for a local
/// receiver, whose loop may store nothing).
struct Scan<'a> {
    recv: Recv,
    writable: &'a [String],
}

fn stmt_is_hoist_safe(stmt: &Stmt, scan: &Scan<'_>) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_is_hoist_safe(expr, scan),
        Stmt::Let { id, init, .. } => {
            scan.recv != Recv::Local(*id)
                && init
                    .as_ref()
                    .is_none_or(|expr| expr_is_hoist_safe(expr, scan))
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_is_hoist_safe(condition, scan)
                && then_branch.iter().all(|s| stmt_is_hoist_safe(s, scan))
                && else_branch
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| stmt_is_hoist_safe(s, scan)))
        }
        // Nested loops are the common case this pass exists for — `m.rows[i]`
        // in an outer loop with an inner loop over the row — so recurse rather
        // than refuse. A nested loop is safe on exactly the same terms: it may
        // not rebind the receiver and may not call anything.
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_is_hoist_safe(condition, scan)
                && body.iter().all(|s| stmt_is_hoist_safe(s, scan))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_none_or(|s| stmt_is_hoist_safe(s, scan))
                && condition
                    .as_ref()
                    .is_none_or(|e| expr_is_hoist_safe(e, scan))
                && update
                    .as_ref()
                    .is_none_or(|e| expr_is_hoist_safe(e, scan))
                && body.iter().all(|s| stmt_is_hoist_safe(s, scan))
        }
        Stmt::Labeled { body, .. } => stmt_is_hoist_safe(body, scan),
        // Leaving the loop early is fine: the hoisted `Let` is evaluated
        // before the loop either way, and the property read it replaces was
        // never reached on this path.
        Stmt::Return(value) => value
            .as_ref()
            .is_none_or(|e| expr_is_hoist_safe(e, scan)),
        Stmt::Throw(value) => expr_is_hoist_safe(value, scan),
        Stmt::Break | Stmt::Continue => true,
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        _ => false,
    }
}

fn expr_is_hoist_safe(expr: &Expr, scan: &Scan<'_>) -> bool {
    match expr {
        // Condition 2: never let the receiver be rebound.
        Expr::LocalSet(id, value) => {
            scan.recv != Recv::Local(*id) && expr_is_hoist_safe(value, scan)
        }
        Expr::Update { id, .. } => scan.recv != Recv::Local(*id),
        // Reading `this` has no effect. (A derived constructor's TDZ read is
        // never reached: the `this` hoist skips constructors.)
        Expr::This => true,
        // A store to one of `this`'s other declared data fields: no setter
        // runs, and it cannot change the hoisted field.
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } => {
            matches!(object.as_ref(), Expr::This)
                && scan.writable.iter().any(|f| f == property)
                && expr_is_hoist_safe(value, scan)
        }
        Expr::LocalGet(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined => true,
        Expr::PropertyGet { object, .. } => expr_is_hoist_safe(object, scan),
        Expr::IndexGet { object, index, .. } => {
            expr_is_hoist_safe(object, scan) && expr_is_hoist_safe(index, scan)
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            expr_is_hoist_safe(left, scan) && expr_is_hoist_safe(right, scan)
        }
        Expr::Compare { left, right, .. } => {
            expr_is_hoist_safe(left, scan) && expr_is_hoist_safe(right, scan)
        }
        Expr::Unary { operand, .. } => expr_is_hoist_safe(operand, scan),
        Expr::NumberCoerce(inner) | Expr::BooleanCoerce(inner) => {
            expr_is_hoist_safe(inner, scan)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_is_hoist_safe(condition, scan)
                && expr_is_hoist_safe(then_expr, scan)
                && expr_is_hoist_safe(else_expr, scan)
        }
        // Condition 3: anything that could call, allocate, or store is out,
        // as is anything this pass does not positively recognise.
        _ => false,
    }
}

fn stmt_reads_property(stmt: &Stmt, recv: Recv, property: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_reads_property(expr, recv, property),
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_reads_property(expr, recv, property)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_reads_property(condition, recv, property)
                || then_branch
                    .iter()
                    .any(|s| stmt_reads_property(s, recv, property))
                || else_branch
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_reads_property(s, recv, property)))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_reads_property(condition, recv, property)
                || body
                    .iter()
                    .any(|s| stmt_reads_property(s, recv, property))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|s| stmt_reads_property(s, recv, property))
                || condition
                    .as_ref()
                    .is_some_and(|e| expr_reads_property(e, recv, property))
                || update
                    .as_ref()
                    .is_some_and(|e| expr_reads_property(e, recv, property))
                || body
                    .iter()
                    .any(|s| stmt_reads_property(s, recv, property))
        }
        Stmt::Labeled { body, .. } => stmt_reads_property(body, recv, property),
        Stmt::Return(value) => value
            .as_ref()
            .is_some_and(|e| expr_reads_property(e, recv, property)),
        Stmt::Throw(value) => expr_reads_property(value, recv, property),
        _ => false,
    }
}

fn expr_reads_property(expr: &Expr, recv: Recv, property: &str) -> bool {
    if is_target_property(expr, recv, property) {
        return true;
    }
    match expr {
        Expr::PropertyGet { object, .. } => expr_reads_property(object, recv, property),
        Expr::IndexGet { object, index, .. } => {
            expr_reads_property(object, recv, property)
                || expr_reads_property(index, recv, property)
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            expr_reads_property(left, recv, property)
                || expr_reads_property(right, recv, property)
        }
        Expr::Unary { operand, .. } => expr_reads_property(operand, recv, property),
        Expr::NumberCoerce(inner) | Expr::BooleanCoerce(inner) => {
            expr_reads_property(inner, recv, property)
        }
        Expr::LocalSet(_, value) => expr_reads_property(value, recv, property),
        Expr::PropertySet { object, value, .. } => {
            expr_reads_property(object, recv, property)
                || expr_reads_property(value, recv, property)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_reads_property(condition, recv, property)
                || expr_reads_property(then_expr, recv, property)
                || expr_reads_property(else_expr, recv, property)
        }
        _ => false,
    }
}

fn is_target_property(expr: &Expr, recv: Recv, property: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property: prop, .. }
            if prop == property && recv.is(object)
    )
}

/// The receiver whose property is hoisted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Recv {
    Local(u32),
    This,
}

impl Recv {
    fn is(self, e: &Expr) -> bool {
        match (self, e) {
            (Recv::Local(id), Expr::LocalGet(got)) => id == *got,
            (Recv::This, Expr::This) => true,
            _ => false,
        }
    }
}

fn rewrite_stmt(stmt: &Stmt, recv: Recv, property: &str, hoist_id: u32) -> Stmt {
    match stmt {
        Stmt::Expr(expr) => Stmt::Expr(rewrite_expr(expr, recv, property, hoist_id)),
        Stmt::Let {
            id,
            name,
            ty,
            mutable,
            init,
        } => Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: ty.clone(),
            mutable: *mutable,
            init: init
                .as_ref()
                .map(|expr| rewrite_expr(expr, recv, property, hoist_id)),
        },
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Stmt::If {
            condition: rewrite_expr(condition, recv, property, hoist_id),
            then_branch: then_branch
                .iter()
                .map(|s| rewrite_stmt(s, recv, property, hoist_id))
                .collect(),
            else_branch: else_branch.as_ref().map(|b| {
                b.iter()
                    .map(|s| rewrite_stmt(s, recv, property, hoist_id))
                    .collect()
            }),
        },
        // These mirror the arms `stmt_is_hoist_safe` admits. Anything it
        // admits must be rewritten here too, or the read it vouched for keeps
        // the per-iteration lookup.
        Stmt::While { condition, body } => Stmt::While {
            condition: rewrite_expr(condition, recv, property, hoist_id),
            body: rewrite_block(body, recv, property, hoist_id),
        },
        Stmt::DoWhile { body, condition } => Stmt::DoWhile {
            body: rewrite_block(body, recv, property, hoist_id),
            condition: rewrite_expr(condition, recv, property, hoist_id),
        },
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => Stmt::For {
            init: init
                .as_ref()
                .map(|s| Box::new(rewrite_stmt(s, recv, property, hoist_id))),
            condition: condition
                .as_ref()
                .map(|e| rewrite_expr(e, recv, property, hoist_id)),
            update: update
                .as_ref()
                .map(|e| rewrite_expr(e, recv, property, hoist_id)),
            body: rewrite_block(body, recv, property, hoist_id),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(rewrite_stmt(body, recv, property, hoist_id)),
        },
        Stmt::Return(value) => Stmt::Return(
            value
                .as_ref()
                .map(|e| rewrite_expr(e, recv, property, hoist_id)),
        ),
        Stmt::Throw(value) => Stmt::Throw(rewrite_expr(value, recv, property, hoist_id)),
        other => other.clone(),
    }
}

fn rewrite_block(body: &[Stmt], recv: Recv, property: &str, hoist_id: u32) -> Vec<Stmt> {
    body.iter()
        .map(|s| rewrite_stmt(s, recv, property, hoist_id))
        .collect()
}

fn rewrite_expr(expr: &Expr, recv: Recv, property: &str, hoist_id: u32) -> Expr {
    if is_target_property(expr, recv, property) {
        return Expr::LocalGet(hoist_id);
    }
    let rec = |e: &Expr| Box::new(rewrite_expr(e, recv, property, hoist_id));
    match expr {
        Expr::PropertyGet {
            object,
            property: prop,
            byte_offset,
            byte_end,
            ..
        } => Expr::PropertyGet {
            byte_end: *byte_end,
            object: rec(object),
            property: prop.clone(),
            byte_offset: *byte_offset,
        },
        Expr::IndexGet {
            object,
            index,
            byte_end,
            byte_offset,
            ..
        } => Expr::IndexGet {
            byte_offset: *byte_offset,
            byte_end: *byte_end,
            object: rec(object),
            index: rec(index),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: rec(left),
            right: rec(right),
        },
        Expr::Logical { op, left, right } => Expr::Logical {
            op: *op,
            left: rec(left),
            right: rec(right),
        },
        Expr::Compare { op, left, right } => Expr::Compare {
            op: *op,
            left: rec(left),
            right: rec(right),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: rec(operand),
        },
        Expr::NumberCoerce(inner) => Expr::NumberCoerce(rec(inner)),
        Expr::BooleanCoerce(inner) => Expr::BooleanCoerce(rec(inner)),
        Expr::LocalSet(id, value) => Expr::LocalSet(*id, rec(value)),
        Expr::PropertySet {
            object,
            property: prop,
            value,
            byte_offset,
            byte_end,
        } => Expr::PropertySet {
            object: rec(object),
            property: prop.clone(),
            value: rec(value),
            byte_offset: *byte_offset,
            byte_end: *byte_end,
        },
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => Expr::Conditional {
            condition: rec(condition),
            then_expr: rec(then_expr),
            else_expr: rec(else_expr),
        },
        other => other.clone(),
    }
}

/// `PERRY_UNBOXED_CLASS_FIELDS=0` turns off the `this.F` hoist below (and the
/// rest of that change) without touching the closed-shape hoist above.
fn this_hoist_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_UNBOXED_CLASS_FIELDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Hoist `this.F` out of loops in a class's instance methods, where `F` is a
/// declared array-typed data field of that class.
///
/// `this.board[i & 127]` in a loop re-reads the field, re-tests the array and
/// re-derives its element base on every iteration; `const b = this.board`
/// before the loop lets the local-array tiers keep all of that out of the loop
/// (measured: 47 instructions per element against 14). The rewrite is the
/// closed-shape hoist's, with the receiver `this`, on the same three
/// conditions:
///
/// 1. **`this.F` is a data property.** `F` is declared on the class with a
///    static key (`F: T[] = …`), and the class has no accessor named `F` and
///    no computed member at all. A field definition creates an own data
///    property that shadows any accessor up or down the chain. The rewrite is
///    versioned on `this instanceof C`; any other receiver (a detached call, a
///    `.call(foreign)`) runs the original loop, so a missing or accessor `F`
///    on a foreign object is never read early.
/// 2. **Nothing in the loop can rebind the receiver.** `this` cannot be
///    rebound, and only method bodies are walked, never a nested function's.
/// 3. **Nothing in the loop can write `F` or call anything.** The walk is the
///    closed-shape hoist's whitelist, plus stores to the class's OTHER
///    declared data fields on `this` (no setter runs; `F` is untouched) and
///    the loop's own initializer.
///
/// Constructors are not walked: a derived constructor's `this` is in its TDZ
/// until `super()`, and a loop that never runs must not read it early.
pub(crate) fn hoist_this_array_fields(
    ctx: &mut LoweringContext,
    class_name: &str,
    fields: &[crate::ir::ClassField],
    getters: &[(String, crate::ir::Function)],
    setters: &[(String, crate::ir::Function)],
    has_computed_members: bool,
    methods: &mut [crate::ir::Function],
) {
    if !hoist_enabled() || !this_hoist_enabled() || has_computed_members {
        return;
    }
    let accessor = |n: &str| {
        getters.iter().any(|(g, _)| g == n) || setters.iter().any(|(s, _)| s == n)
    };
    let data: Vec<String> = fields
        .iter()
        .filter(|f| {
            matches!(f.origin, crate::ir::ClassFieldOrigin::Definition)
                && !f.is_private
                && f.key_expr.is_none()
                && !accessor(&f.name)
        })
        .map(|f| f.name.clone())
        .collect();
    let arrays: Vec<(String, Type)> = fields
        .iter()
        .filter(|f| data.contains(&f.name) && matches!(f.ty, Type::Array(_)))
        .map(|f| (f.name.clone(), f.ty.clone()))
        .collect();
    if arrays.is_empty() {
        return;
    }
    let no_writes: Vec<String> = Vec::new();
    let pure_scan = Scan {
        recv: Recv::This,
        writable: &no_writes,
    };
    let accessor_or_field = |n: &str| accessor(n) || fields.iter().any(|f| f.name == n);
    let inlinable: std::collections::HashMap<String, Inlinable> = methods
        .iter()
        .filter(|m| !m.is_async && !m.is_generator && !accessor_or_field(&m.name))
        .filter(|m| m.params.iter().all(|p| p.default.is_none() && !p.is_rest))
        .filter_map(|m| match m.body.as_slice() {
            [Stmt::Return(Some(e))] if expr_is_hoist_safe(e, &pure_scan) && !expr_has_set(e) => {
                Some((
                    m.name.clone(),
                    Inlinable {
                        params: m.params.iter().map(|p| p.id).collect(),
                        body: e.clone(),
                    },
                ))
            }
            _ => None,
        })
        .collect();
    let class = ThisClass {
        name: class_name,
        data: &data,
        arrays: &arrays,
        inlinable: &inlinable,
    };
    for m in methods.iter_mut() {
        if m.is_generator {
            continue;
        }
        let body = std::mem::take(&mut m.body);
        m.body = hoist_in_block(ctx, &class, body);
    }
}

struct ThisClass<'a> {
    name: &'a str,
    data: &'a [String],
    arrays: &'a [(String, Type)],
    /// Methods whose body is one `return <pure read>` (see [`Inlinable`]).
    inlinable: &'a std::collections::HashMap<String, Inlinable>,
}

/// A method `m(p…) { return e; }` whose `e` passes the hoist whitelist with no
/// stores: inlined into a hoisted loop as `e[p := arg]`, so the field reads
/// it makes are hoisted with the loop's own. The call is replaced only in the
/// version whose guard also proves `this.m === C.prototype.m`, so an override
/// (a subclass, an own property, a patched prototype) runs the original loop.
#[derive(Clone)]
struct Inlinable {
    params: Vec<u32>,
    body: Expr,
}

fn hoist_in_block(ctx: &mut LoweringContext, class: &ThisClass<'_>, stmts: Vec<Stmt>) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        match stmt {
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                let body = hoist_in_block(ctx, class, body);
                out.extend(hoist_this_for(ctx, class, init, condition, update, body));
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => out.push(Stmt::If {
                condition,
                then_branch: hoist_in_block(ctx, class, then_branch),
                else_branch: else_branch.map(|b| hoist_in_block(ctx, class, b)),
            }),
            Stmt::While { condition, body } => out.push(Stmt::While {
                condition,
                body: hoist_in_block(ctx, class, body),
            }),
            Stmt::DoWhile { body, condition } => out.push(Stmt::DoWhile {
                body: hoist_in_block(ctx, class, body),
                condition,
            }),
            // A labeled loop keeps its shape: wrapping it in the version `if`
            // would move the label off the loop.
            other => out.push(other),
        }
    }
    out
}

fn hoist_this_for(
    ctx: &mut LoweringContext,
    class: &ThisClass<'_>,
    init: Option<Box<Stmt>>,
    condition: Option<Expr>,
    update: Option<Expr>,
    body: Vec<Stmt>,
) -> Vec<Stmt> {
    let original = Stmt::For {
        init: init.clone(),
        condition: condition.clone(),
        update: update.clone(),
        body: body.clone(),
    };
    let Some(cond) = condition else {
        return vec![original];
    };
    // Inline trivial `this.m(…)` calls first, so the reads they make are
    // visible to the hoist below. Kept only if a field is then hoisted.
    let mut inlined: Vec<String> = Vec::new();
    let mut cond = inline_this_calls(&cond, class, &mut inlined);
    let mut update = update.map(|u| inline_this_calls(&u, class, &mut inlined));
    let mut body: Vec<Stmt> = body
        .iter()
        .map(|s| inline_this_calls_stmt(s, class, &mut inlined))
        .collect();
    inlined.sort();
    inlined.dedup();
    let mut hoists: Vec<Stmt> = Vec::new();
    for (field, ty) in class.arrays {
        let reads = expr_reads_property(&cond, Recv::This, field)
            || update
                .as_ref()
                .is_some_and(|u| expr_reads_property(u, Recv::This, field))
            || body
                .iter()
                .any(|s| stmt_reads_property(s, Recv::This, field));
        if !reads {
            continue;
        }
        let writable: Vec<String> = class.data.iter().filter(|f| *f != field).cloned().collect();
        let scan = Scan {
            recv: Recv::This,
            writable: &writable,
        };
        let init_safe = init
            .as_deref()
            .is_none_or(|s| stmt_is_hoist_safe(s, &scan));
        if !init_safe || !loop_is_hoist_safe(&cond, update.as_ref(), &body, Recv::This, &writable) {
            continue;
        }
        let name = format!("__perry_hoist_this_{field}");
        // A fresh id, not `define_local`: the method's scope is closed, and
        // the binding is only ever named by the rewritten loop.
        let hoist_id = ctx.fresh_local();
        ctx.immutable_locals.insert(hoist_id);
        hoists.push(Stmt::Let {
            id: hoist_id,
            name,
            ty: ty.clone(),
            mutable: false,
            init: Some(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::This),
                property: field.clone(),
                byte_offset: 0,
            }),
        });
        cond = rewrite_expr(&cond, Recv::This, field, hoist_id);
        update = update.map(|u| rewrite_expr(&u, Recv::This, field, hoist_id));
        body = rewrite_block(&body, Recv::This, field, hoist_id);
    }
    if hoists.is_empty() {
        return vec![original];
    }
    hoists.push(Stmt::For {
        init,
        condition: Some(cond),
        update,
        body,
    });
    let mut guard = Expr::InstanceOf {
        expr: Box::new(Expr::This),
        ty: class.name.to_string(),
        ty_expr: None,
    };
    for m in &inlined {
        let method_of = |object: Expr| Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(object),
            property: m.clone(),
            byte_offset: 0,
        };
        let proto = Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::ClassRef(class.name.to_string())),
            property: "prototype".to_string(),
            byte_offset: 0,
        };
        guard = Expr::Logical {
            op: crate::ir::LogicalOp::And,
            left: Box::new(guard),
            right: Box::new(Expr::Compare {
                op: crate::ir::CompareOp::Eq,
                left: Box::new(method_of(Expr::This)),
                right: Box::new(method_of(proto)),
            }),
        };
    }
    vec![Stmt::If {
        condition: guard,
        then_branch: hoists,
        else_branch: Some(vec![original]),
    }]
}

fn expr_has_set(e: &Expr) -> bool {
    let s = format!("{e:?}");
    s.contains("PropertySet") || s.contains("LocalSet") || s.contains("Update")
}

/// An argument that may be dropped or duplicated by inlining: evaluating it
/// cannot throw and has no effect.
fn arg_is_trivial(e: &Expr) -> bool {
    matches!(e, Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_))
}

fn count_local(e: &Expr, id: u32) -> usize {
    let s = format!("{e:?}");
    s.matches(&format!("LocalGet({id})")).count()
        + s.matches(&format!("LocalSet({id},")).count()
}

fn substitute(e: &Expr, map: &std::collections::HashMap<u32, Expr>) -> Expr {
    map_expr(e, &mut |x| match x {
        Expr::LocalGet(id) => map.get(id).cloned(),
        _ => None,
    })
}

fn inline_this_calls(e: &Expr, class: &ThisClass<'_>, used: &mut Vec<String>) -> Expr {
    map_expr(e, &mut |x| {
        let Expr::Call { callee, args, .. } = x else {
            return None;
        };
        let Expr::PropertyGet { object, property, .. } = callee.as_ref() else {
            return None;
        };
        if !matches!(object.as_ref(), Expr::This) {
            return None;
        }
        let m = class.inlinable.get(property)?;
        if args.len() != m.params.len() {
            return None;
        }
        let mut map = std::collections::HashMap::new();
        for (param, arg) in m.params.iter().zip(args) {
            let uses = count_local(&m.body, *param);
            if uses != 1 && !arg_is_trivial(arg) {
                return None;
            }
            // Arguments are inlined unevaluated: they must be effect-free
            // reads themselves (the loop scan later re-checks the result).
            map.insert(*param, inline_this_calls(arg, class, used));
        }
        used.push(property.clone());
        Some(substitute(&m.body, &map))
    })
}

fn inline_this_calls_stmt(s: &Stmt, class: &ThisClass<'_>, used: &mut Vec<String>) -> Stmt {
    match s {
        Stmt::Expr(e) => Stmt::Expr(inline_this_calls(e, class, used)),
        Stmt::Let {
            id,
            name,
            ty,
            mutable,
            init,
        } => Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: ty.clone(),
            mutable: *mutable,
            init: init.as_ref().map(|e| inline_this_calls(e, class, used)),
        },
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Stmt::If {
            condition: inline_this_calls(condition, class, used),
            then_branch: then_branch
                .iter()
                .map(|s| inline_this_calls_stmt(s, class, used))
                .collect(),
            else_branch: else_branch.as_ref().map(|b| {
                b.iter()
                    .map(|s| inline_this_calls_stmt(s, class, used))
                    .collect()
            }),
        },
        Stmt::Return(v) => Stmt::Return(v.as_ref().map(|e| inline_this_calls(e, class, used))),
        other => other.clone(),
    }
}

/// Rebuild `e` bottom-up over the shapes the hoist whitelist admits (plus
/// `Call`, which the inliner consumes); `f` replaces a node when it returns
/// `Some`. Any other node is copied unchanged.
fn r(x: &Expr, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Box<Expr> {
    Box::new(map_expr(x, f))
}

fn map_expr(e: &Expr, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(r) = f(e) {
        return r;
    }
    match e {
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            byte_end,
        } => Expr::PropertyGet {
            object: r(object, f),
            property: property.clone(),
            byte_offset: *byte_offset,
            byte_end: *byte_end,
        },
        Expr::PropertySet {
            object,
            property,
            value,
            byte_offset,
            byte_end,
        } => {
            let object = r(object, f);
            Expr::PropertySet {
                object,
                property: property.clone(),
                value: r(value, f),
                byte_offset: *byte_offset,
                byte_end: *byte_end,
            }
        }
        Expr::IndexGet {
            object,
            index,
            byte_offset,
            byte_end,
        } => {
            let object = r(object, f);
            Expr::IndexGet {
                object,
                index: r(index, f),
                byte_offset: *byte_offset,
                byte_end: *byte_end,
            }
        }
        Expr::Binary { op, left, right } => {
            let left = r(left, f);
            Expr::Binary {
                op: *op,
                left,
                right: r(right, f),
            }
        }
        Expr::Logical { op, left, right } => {
            let left = r(left, f);
            Expr::Logical {
                op: *op,
                left,
                right: r(right, f),
            }
        }
        Expr::Compare { op, left, right } => {
            let left = r(left, f);
            Expr::Compare {
                op: *op,
                left,
                right: r(right, f),
            }
        }
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: r(operand, f),
        },
        Expr::NumberCoerce(x) => Expr::NumberCoerce(r(x, f)),
        Expr::BooleanCoerce(x) => Expr::BooleanCoerce(r(x, f)),
        Expr::LocalSet(id, v) => Expr::LocalSet(*id, r(v, f)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let condition = r(condition, f);
            let then_expr = r(then_expr, f);
            Expr::Conditional {
                condition,
                then_expr,
                else_expr: r(else_expr, f),
            }
        }
        Expr::Call {
            callee,
            args,
            type_args,
            byte_offset,
            byte_end,
        } => {
            let callee = r(callee, f);
            Expr::Call {
                callee,
                args: args.iter().map(|a| map_expr(a, f)).collect(),
                type_args: type_args.clone(),
                byte_offset: *byte_offset,
                byte_end: *byte_end,
            }
        }
        other => other.clone(),
    }
}
