//! Integer return contracts for closed additive, decreasing scalar callees.
//!
//! This is a proof, not an evaluator: no computed function result is emitted.
//! A rational exponential majorant bounds all source partial sums. Exact u128
//! arithmetic checks the majorant; overflow refuses a proof. The guarded input
//! domain is derived from i32's value range, without a heuristic cutoff.
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Stmt};

#[derive(Clone, Copy)]
pub(super) enum Base {
    Param,
    Constant(i32),
}
#[derive(Clone, Copy)]
pub(super) struct Contract {
    pub input_max: i32,
    pub base_limit: i32,
    pub base: Base,
}
fn integer(e: &Expr) -> Option<u32> {
    match e {
        Expr::Integer(n) => u32::try_from(*n).ok().filter(|n| *n <= i32::MAX as u32),
        Expr::Number(n)
            if n.is_finite()
                && *n >= 0.0
                && *n <= i32::MAX as f64
                && n.fract() == 0.0
                && n.to_bits() != (-0.0f64).to_bits() =>
        {
            Some(*n as u32)
        }
        _ => None,
    }
}
fn ret(body: &[Stmt]) -> Option<&Expr> {
    match body {
        [Stmt::Return(Some(e))] => Some(e),
        _ => None,
    }
}
fn pieces(f: &Function) -> Option<(&Expr, &Expr, &Expr)> {
    match f.body.as_slice() {
        [Stmt::If {
            condition,
            then_branch,
            else_branch: None,
        }, Stmt::Return(Some(rest))] => Some((condition, ret(then_branch)?, rest)),
        [Stmt::If {
            condition,
            then_branch,
            else_branch: Some(other),
        }] => Some((condition, ret(then_branch)?, ret(other)?)),
        [Stmt::Return(Some(Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        }))] => Some((condition, then_expr, else_expr)),
        _ => None,
    }
}
fn terms(e: &Expr, f: &Function, steps: &mut Vec<u32>, forcing: &mut u128) -> Option<()> {
    match e {
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => {
            terms(left, f, steps, forcing)?;
            terms(right, f, steps, forcing)
        }
        Expr::Call { callee, args, .. }
            if matches!(callee.as_ref(), Expr::FuncRef(id) if *id == f.id) && args.len() == 1 =>
        {
            let Expr::Binary {
                op: BinaryOp::Sub,
                left,
                right,
            } = &args[0]
            else {
                return None;
            };
            if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == f.params[0].id) {
                return None;
            }
            let step = integer(right)?;
            if step == 0 {
                return None;
            }
            steps.push(step);
            Some(())
        }
        _ => {
            *forcing = forcing.checked_add(integer(e)? as u128)?;
            Some(())
        }
    }
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn rate_holds(p: u128, q: u128, steps: &[u32]) -> Option<bool> {
    let depth = *steps.iter().max()?;
    let denominator = p.checked_pow(depth)?;
    let mut numerator = 0u128;
    for step in steps {
        let term = q
            .checked_pow(*step)?
            .checked_mul(p.checked_pow(depth - step)?)?;
        numerator = numerator.checked_add(term)?;
    }
    Some(numerator <= denominator)
}
fn max_input(mut safe: impl FnMut(u32) -> bool) -> u32 {
    // "Proven safe" is a prefix: both the majorant and any checked-power
    // overflow are monotone on nonnegative inputs.
    let mut lo = 0;
    let mut hi = i32::MAX as u32;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if safe(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}
pub(super) fn prove(f: &Function) -> Option<Contract> {
    if f.params.len() != 1 {
        return None;
    }
    let (condition, base_expr, recursive) = pieces(f)?;
    let Expr::Compare { op, left, right } = condition else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == f.params[0].id) {
        return None;
    }
    let mut boundary = integer(right)?;
    match op {
        CompareOp::Lt => {}
        CompareOp::Le => boundary = boundary.checked_add(1)?,
        _ => return None,
    }
    if boundary == 0 || boundary > i32::MAX as u32 {
        return None;
    }
    let (base, initial) = if matches!(base_expr, Expr::LocalGet(id) if *id == f.params[0].id) {
        (Base::Param, boundary - 1)
    } else {
        let value = integer(base_expr)?;
        (Base::Constant(value as i32), value)
    };
    let mut steps = Vec::new();
    let mut forcing = 0;
    terms(recursive, f, &mut steps, &mut forcing)?;
    if steps.is_empty() || steps.iter().any(|s| *s > boundary) {
        return None;
    }
    // Thus every recursive argument lies in [0,n-1]. Base values are total,
    // primitive and nonnegative; every recursive result/partial sum is too.
    let limit = i32::MAX as u128;
    let input_max = if steps.len() == 1 {
        // A unary recurrence makes at most n recursive steps.
        max_input(|n| {
            forcing
                .checked_mul(n as u128)
                .and_then(|v| v.checked_add(initial as u128))
                .is_some_and(|v| v <= limit)
        })
    } else {
        // Y=R+shift removes nonnegative forcing:
        // Y(n) <= sum(Y(n-step)) when forcing <= (arity-1)*shift.
        let arity = steps.len() as u128;
        let shift = forcing.checked_add(arity - 2)?.checked_div(arity - 1)?;
        let coefficient = (initial as u128).checked_add(shift)?;
        if coefficient == 0 {
            i32::MAX as u32
        } else {
            let mut q = steps
                .iter()
                .try_fold(0u128, |n, s| n.checked_add(*s as u128))?;
            let mut hi = arity.checked_mul(q)?;
            // p/q=arity is always a valid rate since every step is >=1.
            let mut lo = q;
            while hi - lo > 1 {
                let p = lo + (hi - lo) / 2;
                if rate_holds(p, q, &steps) == Some(true) {
                    hi = p;
                } else {
                    lo = p;
                }
            }
            let d = gcd(hi, q);
            let p = hi / d;
            q /= d;
            max_input(|n| {
                p.checked_pow(n)
                    .and_then(|v| coefficient.checked_mul(v))
                    .zip(q.checked_pow(n).and_then(|v| limit.checked_mul(v)))
                    .is_some_and(|(a, b)| a <= b)
            })
        }
    };
    if input_max < boundary {
        return None;
    }
    Some(Contract {
        input_max: input_max as i32,
        base_limit: boundary as i32,
        base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rational_rate_is_a_return_bound_not_an_answer() {
        assert_eq!(rate_holds(5, 3, &[1, 2]), Some(true));
        assert_eq!(rate_holds(3, 2, &[1, 2]), Some(false));
        assert_eq!(
            max_input(|n| 5u128
                .checked_pow(n)
                .zip(
                    3u128
                        .checked_pow(n)
                        .and_then(|x| x.checked_mul(i32::MAX as u128))
                )
                .is_some_and(|(a, b)| a <= b)),
            42
        );
    }
}
