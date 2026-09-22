//! Compile-time profitability hints; never representation or shape proofs.
use perry_hir::{Expr, Module};
use std::collections::{HashMap, HashSet};

pub(super) const SHORT_TRIP_LIMIT: f64 = 16.0;

pub(crate) fn short_bounds(module: &Module) -> HashMap<u32, HashSet<u32>> {
    let mut calls: HashMap<u32, Vec<bool>> = HashMap::new();
    let mut visit = |expr: &Expr| {
        let Expr::Call { callee, args, .. } = expr else {
            return;
        };
        let Expr::FuncRef(id) = callee.as_ref() else {
            return;
        };
        let small: Vec<bool> = args
            .iter()
            .map(|arg| match arg {
                Expr::Integer(n) => *n >= 0 && (*n as f64) < SHORT_TRIP_LIMIT,
                Expr::Number(n) => *n >= 0.0 && *n < SHORT_TRIP_LIMIT,
                _ => false,
            })
            .collect();
        calls
            .entry(*id)
            .and_modify(|prior| {
                for (index, short) in prior.iter_mut().enumerate() {
                    *short &= small.get(index).copied().unwrap_or(false);
                }
            })
            .or_insert(small);
    };
    // These are hints only: an unobserved indirect/exported call can lose an
    // optimization, never gain an unsound proof. Any observed long/unknown
    // argument keeps versioning enabled for that parameter.
    crate::collectors::for_each_expr_in_stmts(&module.init, &mut visit);
    for function in &module.functions {
        crate::collectors::for_each_expr_in_stmts(&function.body, &mut visit);
    }
    module
        .functions
        .iter()
        .filter_map(|function| {
            let args = calls.get(&function.id)?;
            let bounds: HashSet<u32> = function
                .params
                .iter()
                .zip(args)
                .filter_map(|(param, short)| short.then_some(param.id))
                .collect();
            (!bounds.is_empty()).then_some((function.id, bounds))
        })
        .collect()
}
