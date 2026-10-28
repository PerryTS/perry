//! Scalar callback admission inside an existing guarded Number-loop scope.
//! The emitted body identity is static; capture and argument kinds are live
//! values. A private pure Number body returns a Number for those inputs, so
//! the loop's greatest fixed point can carry that contract across its phi.
use crate::codegen::TypedParamRep;
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I64};
use anyhow::Result;
use perry_hir::Expr;

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub local: u32,
    pub arity: usize,
    captures: usize,
    symbol: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Admission {
    pub arity: usize,
    symbol: String,
    captures: Vec<String>,
}

pub(super) fn candidate(ctx: &FnCtx<'_>, callee: &Expr, argc: usize) -> Option<Plan> {
    let Expr::LocalGet(id) = callee else {
        return None;
    };
    if ctx.wpfacts_off
        || ctx.wpfacts.interp_guard
        || ctx.target_triple.contains("wasm")
        || !ctx.locals.contains_key(id)
        || ctx.boxed_vars.contains(id)
        || ctx.closure_captures.contains_key(id)
        || ctx.module_globals.contains_key(id)
        || ctx.repsel_closure_ref_locals.contains(id)
        || ctx.tdz_boxes.contains(id)
        || ctx.strings.tdz_binding_names.contains_key(id)
    {
        return None;
    }
    let [body] = ctx.wpfacts.local_bodies.get(id)?.as_slice() else {
        return None;
    };
    if !body.closure
        || body.params as usize != argc
        || !ctx.typed_f64_closures.contains(&body.func_id)
    {
        return None;
    }
    let reps = ctx.typed_i1_closure_param_reps.get(&body.func_id)?;
    if reps.len() != argc || !reps.iter().all(|r| *r == TypedParamRep::F64) {
        return None;
    }
    let captures = ctx.typed_closure_capture_reps.get(&body.func_id)?;
    if !captures.iter().all(|r| *r == TypedParamRep::F64) {
        return None;
    }
    let public = crate::fn_info::closure_body_symbol(&ctx.strings.module_prefix(), body.func_id);
    let typed = crate::codegen::typed_f64_closure_name(&public);
    let symbol = if captures.is_empty() {
        typed
    } else {
        crate::fn_info::normalized_capture_body_symbol(&typed)
    };
    Some(Plan {
        local: *id,
        arity: argc,
        captures: captures.len(),
        symbol,
    })
}

/// Read immutable capture words once. A mutable capture is a cell pointer,
/// which fails this Number guard and retains the complete original loop.
pub(super) fn capture_guard(
    ctx: &mut FnCtx<'_>,
    plans: &[Plan],
) -> Result<(String, Vec<(u32, Admission)>)> {
    let mut valid = "true".to_owned();
    let mut admitted = Vec::new();
    for plan in plans {
        let callee = lower_expr(ctx, &Expr::LocalGet(plan.local))?;
        let handle = crate::expr::unbox_to_i64(ctx.block(), &callee);
        let header = crate::target_layout::closure_header_size_bytes(ctx.target_triple);
        let mut captures = Vec::new();
        for i in 0..plan.captures {
            let addr = ctx
                .block()
                .add(I64, &handle, &(header + 8 * i as u64).to_string());
            let ptr = ctx.block().inttoptr(I64, &addr);
            let bits = ctx.block().load(I64, &ptr);
            let value = ctx.block().bitcast_i64_to_double(&bits);
            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), TypedParamRep::F64, &value);
            valid = ctx.block().and(I1, &valid, &ok);
            captures.push(value);
        }
        admitted.push((
            plan.local,
            Admission {
                arity: plan.arity,
                symbol: plan.symbol.clone(),
                captures,
            },
        ));
    }
    Ok((valid, admitted))
}

/// Normalize only in the admitted arm, after all actual Number tests passed.
pub(super) fn materialize(ctx: &mut FnCtx<'_>, scope: u32, admissions: Vec<(u32, Admission)>) {
    for (id, mut admission) in admissions {
        for value in &mut admission.captures {
            *value = crate::codegen::emit_typed_arg_to_raw(ctx.block(), TypedParamRep::F64, value);
        }
        ctx.receiver_descriptors
            .materialize_numeric_callback(scope, id, admission);
    }
}

pub(crate) fn returns_number(ctx: &FnCtx<'_>, callee: &Expr, argc: usize) -> bool {
    matches!(callee, Expr::LocalGet(id) if ctx.receiver_descriptors.numeric_callback(*id)
        .is_some_and(|a| a.arity == argc))
}

pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::LocalGet(id) = callee else {
        return Ok(None);
    };
    let Some(admission) = ctx.receiver_descriptors.numeric_callback(*id).cloned() else {
        return Ok(None);
    };
    if admission.arity != args.len() {
        return Ok(None);
    }
    let mut values = Vec::new();
    for arg in args {
        let value = lower_expr(ctx, arg)?;
        let raw = match arg {
            Expr::LocalGet(id) => ctx.receiver_descriptors.callback_local_is_raw_f64(*id),
            Expr::Call { callee, args, .. } => returns_number(ctx, callee, args.len()),
            _ => false,
        };
        values.push(if raw {
            value
        } else {
            crate::codegen::emit_typed_arg_to_raw(ctx.block(), TypedParamRep::F64, &value)
        });
    }
    // The normalized private body reads only scalar parameters and seeds;
    // its otherwise unused closure argument carries no heap pointer.
    let mut actual = vec![(I64, "0")];
    actual.extend(
        values
            .iter()
            .chain(&admission.captures)
            .map(|v| (DOUBLE, v.as_str())),
    );
    let value = ctx.block().call_gc_leaf(DOUBLE, &admission.symbol, &actual);
    Ok(Some(value))
}
