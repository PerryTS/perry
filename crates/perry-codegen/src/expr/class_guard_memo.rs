//! One class-field receiver guard per straight-line run of statements.
//!
//! In `const i = a.size++; a.left[i] = l; a.right[i] = r;` every access to `a`
//! re-proves `a`'s ShapeId. Nothing between them can change that shape
//! without a call: their inline paths read slots, write a Number into an
//! existing raw-f64 lane, or write raw typed-array bytes. So a run of such
//! statements carries the proof in a flag:
//!
//! * the run starts by clearing the flag;
//! * the receiver's first site in the run runs the full guard and, when it
//!   passes, sets the flag;
//! * a later site whose proof an earlier site provides tests only the flag:
//!   set, it goes straight to its slot access; clear, it takes the unchanged
//!   guard call (the complete IC), so it emits no inline guard of its own;
//! * every call the run emits clears the flag before entering the callee (the
//!   call-emission choke point, `LlBlock::dirty_revalidations_before_call`),
//!   so after any slow path, user code or collection later sites take the IC.
//!
//! The flag is a stack slot that mem2reg turns into SSA, so on the hit path
//! the reuse costs one branch. It is not a side table: it lives in one
//! activation and is consumed by the same activation's guards. A run admits
//! only straight-line statements whose expressions are reads, local writes,
//! arithmetic, class field updates and declared typed-array element stores
//! (`IndexSet`, or the `PutValueSet` that `index_set::lower` serves); anything
//! else (a conditional or logical operator, another property store, a call, a
//! nested statement list, a closure) ends the run. A flag is set only by a
//! passing guard of its own run and cleared by every call, so a site that
//! finds it set is proven whatever path reached it.
use super::FnCtx;
use perry_hir::{Expr, Stmt};
use std::collections::BTreeMap;

pub(crate) struct Window {
    list: usize,
    next: usize,
    active: bool,
    /// Per receiver local: the (class identity, ShapeId) flag, the
    /// ShapeId-only flag, and which of the two an earlier site of the run
    /// already sets. An identity proof implies the ShapeId proof.
    slots: BTreeMap<u32, Receiver>,
}

struct Receiver {
    identity: String,
    shape: String,
    identity_set: bool,
    shape_set: bool,
}

/// What a read guard on a run receiver does with the run's flags.
pub(crate) enum Memo {
    /// The first site needing this proof: the full guard, which on a pass
    /// sets these flags.
    Set(Vec<String>),
    /// An earlier site already sets this flag: test it alone, and take the
    /// guard call when it is clear.
    Reuse(String),
}

pub(crate) fn memo_for(ctx: &mut FnCtx<'_>, receiver: &Expr, identity: bool) -> Option<Memo> {
    let window = ctx.class_guard_window.as_mut()?;
    if !window.active {
        return None;
    }
    let Expr::LocalGet(id) = receiver else {
        return None;
    };
    let r = window.slots.get_mut(id)?;
    Some(if identity {
        if r.identity_set {
            Memo::Reuse(r.identity.clone())
        } else {
            r.identity_set = true;
            r.shape_set = true;
            Memo::Set(vec![r.identity.clone(), r.shape.clone()])
        }
    } else if r.shape_set {
        Memo::Reuse(r.shape.clone())
    } else {
        r.shape_set = true;
        Memo::Set(vec![r.shape.clone()])
    })
}

/// A receiver whose field accesses take the declared-class guard: a local
/// that is never reassigned or boxed, with a class nominated for it.
fn receiver_local(ctx: &FnCtx<'_>, expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id)
            if !ctx.reassigned_locals.contains(id)
                && !ctx.boxed_vars.contains(id)
                && crate::type_analysis::receiver_class_name(ctx, expr)
                    .or_else(|| {
                        super::property_get::guarded_declared_class_get_candidate(ctx, expr)
                    })
                    .is_some() =>
        {
            Some(*id)
        }
        _ => None,
    }
}

/// Whether `expr`'s inline paths can change no object's shape, counting the
/// guarded class-field sites per receiver local.
fn expr_safe(ctx: &FnCtx<'_>, expr: &Expr, sites: &mut BTreeMap<u32, usize>) -> bool {
    match expr {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::LocalGet(_)
        | Expr::Update { .. } => true,
        Expr::LocalSet(_, value) | Expr::Unary { operand: value, .. } => {
            expr_safe(ctx, value, sites)
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            expr_safe(ctx, left, sites) && expr_safe(ctx, right, sites)
        }
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            if let Some(id) = receiver_local(ctx, object) {
                *sites.entry(id).or_default() += 1;
            }
            expr_safe(ctx, object, sites)
        }
        Expr::IndexGet { object, index } => {
            expr_safe(ctx, object, sites) && expr_safe(ctx, index, sites)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            matches!(target.as_ref(), Expr::PropertyGet { .. })
                && super::typed_element::declared_kind(ctx, target).is_some()
                && super::proxy_reflect::put_value_index_fast_path(ctx, target, key, receiver)
                && expr_safe(ctx, target, sites)
                && expr_safe(ctx, key, sites)
                && expr_safe(ctx, value, sites)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            matches!(object.as_ref(), Expr::PropertyGet { .. })
                && super::typed_element::declared_kind(ctx, object).is_some()
                && expr_safe(ctx, object, sites)
                && expr_safe(ctx, index, sites)
                && expr_safe(ctx, value, sites)
        }
        _ => false,
    }
}

fn stmt_safe(ctx: &FnCtx<'_>, stmt: &Stmt, sites: &mut BTreeMap<u32, usize>) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(init), ..
        } => expr_safe(ctx, init, sites),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) => expr_safe(ctx, expr, sites),
        _ => false,
    }
}

/// Called right before `lower_stmt(stmts[i])`: continue, close or open a run.
pub(crate) fn before_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    let list = stmts.as_ptr() as usize;
    if !stmt_safe(ctx, &stmts[i], &mut BTreeMap::new()) {
        close(ctx);
        return;
    }
    if let Some(window) = ctx.class_guard_window.as_mut() {
        if window.list == list && window.next == i {
            window.next = i + 1;
            window.active = true;
            return;
        }
    }
    close(ctx);
    let mut sites = BTreeMap::new();
    for stmt in &stmts[i..] {
        if !stmt_safe(ctx, stmt, &mut sites) {
            break;
        }
    }
    let mut slots = BTreeMap::new();
    for (id, count) in sites {
        if count < 2 {
            continue;
        }
        let identity = ctx.func.alloca_entry(crate::types::I8);
        let shape = ctx.func.alloca_entry(crate::types::I8);
        for slot in [&identity, &shape] {
            ctx.func
                .entry_allocas_push_store(crate::types::I8, "0", slot);
            ctx.block().store(crate::types::I8, "0", slot);
            ctx.func.reg_counter().push_guard_memo_slot(slot.clone());
        }
        slots.insert(
            id,
            Receiver {
                identity,
                shape,
                identity_set: false,
                shape_set: false,
            },
        );
    }
    if slots.is_empty() {
        return;
    }
    ctx.class_guard_window = Some(Window {
        list,
        next: i + 1,
        active: true,
        slots,
    });
}

/// Called right after `lower_stmt`: guards outside a run's own statements
/// never consult it.
pub(crate) fn after_stmt(ctx: &mut FnCtx<'_>) {
    if let Some(window) = ctx.class_guard_window.as_mut() {
        window.active = false;
    }
}

/// End the run: later calls no longer clear its flags.
pub(crate) fn close(ctx: &mut FnCtx<'_>) {
    if ctx.class_guard_window.take().is_some() {
        ctx.func.reg_counter().clear_guard_memo_slots();
    }
}
