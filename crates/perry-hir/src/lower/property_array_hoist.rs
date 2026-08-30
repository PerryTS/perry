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
    let (recv_id, property) = counted_loop_property_array(condition)?;
    if !property_is_anon_shape_data_field(ctx, recv_id, &property) {
        return None;
    }
    if !loop_is_hoist_safe(condition, update, body, recv_id) {
        return None;
    }
    // Reads of `RECV.PROP` must actually occur in the body, or the rewrite
    // moves work without removing any.
    if !body.iter().any(|stmt| stmt_reads_property(stmt, recv_id, &property)) {
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

    let new_condition = rewrite_expr(condition, recv_id, &property, hoist_id);
    let new_body = body
        .iter()
        .map(|stmt| rewrite_stmt(stmt, recv_id, &property, hoist_id))
        .collect();
    Some((hoist, new_condition, new_body))
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
fn property_is_anon_shape_data_field(
    ctx: &LoweringContext,
    recv_id: u32,
    property: &str,
) -> bool {
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

fn anon_shape_field_type(
    ctx: &LoweringContext,
    class_name: &str,
    property: &str,
) -> Option<Type> {
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
    recv_id: u32,
) -> bool {
    expr_is_hoist_safe(condition, recv_id)
        && update.is_none_or(|expr| expr_is_hoist_safe(expr, recv_id))
        && body.iter().all(|stmt| stmt_is_hoist_safe(stmt, recv_id))
}

fn stmt_is_hoist_safe(stmt: &Stmt, recv_id: u32) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_is_hoist_safe(expr, recv_id),
        Stmt::Let { id, init, .. } => {
            *id != recv_id
                && init
                    .as_ref()
                    .is_none_or(|expr| expr_is_hoist_safe(expr, recv_id))
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_is_hoist_safe(condition, recv_id)
                && then_branch.iter().all(|s| stmt_is_hoist_safe(s, recv_id))
                && else_branch
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| stmt_is_hoist_safe(s, recv_id)))
        }
        Stmt::Break | Stmt::Continue => true,
        _ => false,
    }
}

fn expr_is_hoist_safe(expr: &Expr, recv_id: u32) -> bool {
    match expr {
        // Condition 2: never let the receiver be rebound.
        Expr::LocalSet(id, value) => *id != recv_id && expr_is_hoist_safe(value, recv_id),
        Expr::Update { id, .. } => *id != recv_id,
        Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) | Expr::String(_)
        | Expr::Bool(_) | Expr::Null | Expr::Undefined => true,
        Expr::PropertyGet { object, .. } => expr_is_hoist_safe(object, recv_id),
        Expr::IndexGet { object, index } => {
            expr_is_hoist_safe(object, recv_id) && expr_is_hoist_safe(index, recv_id)
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            expr_is_hoist_safe(left, recv_id) && expr_is_hoist_safe(right, recv_id)
        }
        Expr::Compare { left, right, .. } => {
            expr_is_hoist_safe(left, recv_id) && expr_is_hoist_safe(right, recv_id)
        }
        Expr::Unary { operand, .. } => expr_is_hoist_safe(operand, recv_id),
        Expr::NumberCoerce(inner) | Expr::BooleanCoerce(inner) => {
            expr_is_hoist_safe(inner, recv_id)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_is_hoist_safe(condition, recv_id)
                && expr_is_hoist_safe(then_expr, recv_id)
                && expr_is_hoist_safe(else_expr, recv_id)
        }
        // Condition 3: anything that could call, allocate, or store is out,
        // as is anything this pass does not positively recognise.
        _ => false,
    }
}

fn stmt_reads_property(stmt: &Stmt, recv_id: u32, property: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_reads_property(expr, recv_id, property),
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_reads_property(expr, recv_id, property)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_reads_property(condition, recv_id, property)
                || then_branch
                    .iter()
                    .any(|s| stmt_reads_property(s, recv_id, property))
                || else_branch.as_ref().is_some_and(|b| {
                    b.iter().any(|s| stmt_reads_property(s, recv_id, property))
                })
        }
        _ => false,
    }
}

fn expr_reads_property(expr: &Expr, recv_id: u32, property: &str) -> bool {
    if is_target_property(expr, recv_id, property) {
        return true;
    }
    match expr {
        Expr::PropertyGet { object, .. } => expr_reads_property(object, recv_id, property),
        Expr::IndexGet { object, index } => {
            expr_reads_property(object, recv_id, property)
                || expr_reads_property(index, recv_id, property)
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            expr_reads_property(left, recv_id, property)
                || expr_reads_property(right, recv_id, property)
        }
        Expr::Unary { operand, .. } => expr_reads_property(operand, recv_id, property),
        Expr::NumberCoerce(inner) | Expr::BooleanCoerce(inner) => {
            expr_reads_property(inner, recv_id, property)
        }
        Expr::LocalSet(_, value) => expr_reads_property(value, recv_id, property),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_reads_property(condition, recv_id, property)
                || expr_reads_property(then_expr, recv_id, property)
                || expr_reads_property(else_expr, recv_id, property)
        }
        _ => false,
    }
}

fn is_target_property(expr: &Expr, recv_id: u32, property: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property: prop, .. }
            if prop == property && matches!(object.as_ref(), Expr::LocalGet(id) if *id == recv_id)
    )
}

fn rewrite_stmt(stmt: &Stmt, recv_id: u32, property: &str, hoist_id: u32) -> Stmt {
    match stmt {
        Stmt::Expr(expr) => Stmt::Expr(rewrite_expr(expr, recv_id, property, hoist_id)),
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
                .map(|expr| rewrite_expr(expr, recv_id, property, hoist_id)),
        },
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Stmt::If {
            condition: rewrite_expr(condition, recv_id, property, hoist_id),
            then_branch: then_branch
                .iter()
                .map(|s| rewrite_stmt(s, recv_id, property, hoist_id))
                .collect(),
            else_branch: else_branch.as_ref().map(|b| {
                b.iter()
                    .map(|s| rewrite_stmt(s, recv_id, property, hoist_id))
                    .collect()
            }),
        },
        other => other.clone(),
    }
}

fn rewrite_expr(expr: &Expr, recv_id: u32, property: &str, hoist_id: u32) -> Expr {
    if is_target_property(expr, recv_id, property) {
        return Expr::LocalGet(hoist_id);
    }
    let rec = |e: &Expr| Box::new(rewrite_expr(e, recv_id, property, hoist_id));
    match expr {
        Expr::PropertyGet {
            object,
            property: prop,
            byte_offset,
        } => Expr::PropertyGet {
            object: rec(object),
            property: prop.clone(),
            byte_offset: *byte_offset,
        },
        Expr::IndexGet { object, index } => Expr::IndexGet {
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
