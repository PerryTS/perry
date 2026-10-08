//! Array binding sources share the compiler-owned IteratorRecord lowering
//! used by for-of. Only the step representation differs after the entry proof.
use super::*;

/// How one array pattern reaches its elements.
pub(crate) enum ArraySource {
    /// Drive the spec iterator protocol on this expression, unguarded. What
    /// every array pattern did before #10086, and what a source statically known not to be an array does.
    Iterator(Expr),
    /// The source admits the non-iterator arm: [`FastPlan`] carries both the
    /// runtime guard and the fast element source.
    Guarded(FastPlan),
}

impl ArraySource {
    /// The initializer for the pattern's iterator local.
    pub(crate) fn iter_init(&self) -> Expr {
        match self {
            ArraySource::Iterator(source) => Expr::GetIterator(Box::new(source.clone())),
            ArraySource::Guarded(plan) => plan.guarded_get_iterator(),
        }
    }

    pub(crate) fn next_init(&self, iter_id: LocalId) -> Expr {
        match self {
            Self::Iterator(_) => crate::lower::iterator_next_method_call(iter_id),
            Self::Guarded(plan) => plan.next_init(iter_id),
        }
    }

    /// Produce element `idx` into `value_id`. `iter_pull` is the caller's
    /// existing iterator-step sequence, used verbatim on the protocol arm.
    pub(crate) fn pull(
        &self,
        _idx: usize,
        _value_id: LocalId,
        mut iter_pull: Vec<Stmt>,
    ) -> Vec<Stmt> {
        match self {
            ArraySource::Iterator(_) => iter_pull,
            ArraySource::Guarded(plan) => {
                for stmt in &mut iter_pull {
                    plan.rewrite(stmt, None, true);
                }
                iter_pull
            }
        }
    }

    /// Drain through the same record step used by ordinary bindings. Each
    /// value is pushed, so rest produces dense undefined entries for holes.
    pub(crate) fn rest(
        &self,
        ctx: &mut LoweringContext,
        id: LocalId,
        name: String,
        iter: LocalId,
        next: LocalId,
        done: LocalId,
    ) -> Vec<Stmt> {
        if let Self::Iterator(_) = self {
            return vec![Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::NativeMethodCall {
                    module: "__perry_runtime".into(),
                    class_name: None,
                    object: None,
                    method: "iteratorRestToArray".into(),
                    args: vec![
                        Expr::LocalGet(iter),
                        Expr::LocalGet(next),
                        Expr::LocalGet(done),
                    ],
                }),
            }];
        }
        let Self::Guarded(plan) = self else {
            unreachable!()
        };
        let ty = Type::Array(Box::new(Type::Any));
        ctx.locals
            .iter_mut()
            .find(|(_, local, _)| *local == id)
            .unwrap()
            .2 = ty.clone();
        let value = ctx.fresh_local();
        let value_name = format!("__iterator_rest_value_{value}");
        ctx.locals.push((value_name.clone(), value, Type::Any));
        vec![
            Stmt::Let {
                id,
                name,
                ty,
                mutable: true,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Let {
                id: value,
                name: value_name,
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::If {
                condition: Expr::LocalGet(done),
                then_branch: vec![],
                else_branch: Some(vec![Stmt::While {
                    condition: Expr::Bool(true),
                    body: vec![Stmt::If {
                        condition: plan.step(crate::lower::iterator_step_call(iter, next, value)),
                        then_branch: vec![Stmt::Break],
                        else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
                            id,
                            Box::new(Expr::NativeMethodCall {
                                module: "__perry_runtime".into(),
                                class_name: None,
                                object: None,
                                method: "iteratorRestAppend".into(),
                                args: vec![Expr::LocalGet(id), Expr::LocalGet(value)],
                            }),
                        ))]),
                    }],
                }]),
            },
        ]
    }

    /// `IteratorClose`, run only where an iterator was actually created.
    pub(crate) fn close(&self, mut close: Stmt) -> Stmt {
        match self {
            ArraySource::Iterator(_) => close,
            ArraySource::Guarded(plan) => {
                plan.rewrite(&mut close, None, false);
                close
            }
        }
    }
}

pub(crate) use crate::iterator_record::IteratorRecordPlan as FastPlan;

pub(crate) fn plan_for_literal(
    ctx: &mut LoweringContext,
    elems: &[&ast::Expr],
    out: &mut Vec<Stmt>,
) -> Result<FastPlan> {
    let values = elems
        .iter()
        .map(|e| lower_expr(ctx, e))
        .collect::<Result<Vec<_>>>()?;
    Ok(FastPlan::new(ctx, Expr::Array(values), out))
}

pub(crate) fn plan_for_proven_array(
    ctx: &mut LoweringContext,
    source: Expr,
    out: &mut Vec<Stmt>,
) -> FastPlan {
    FastPlan::new(ctx, source, out)
}

pub(crate) fn plan_for_unproven_source(
    ctx: &mut LoweringContext,
    source: Expr,
    out: &mut Vec<Stmt>,
) -> FastPlan {
    FastPlan::new(ctx, source, out)
}

/// A spread-free array literal's element expressions, or `None` for anything
/// else. Holes are rejected: a hole is a genuinely ABSENT index whose read walks
/// the prototype chain, which substituting `undefined` would not do.
pub(crate) fn spread_free_array_literal(expr: &ast::Expr) -> Option<Vec<&ast::Expr>> {
    let ast::Expr::Array(arr) = expr else {
        return None;
    };
    let mut out = Vec::with_capacity(arr.elems.len());
    for elem in &arr.elems {
        let elem = elem.as_ref()?;
        if elem.spread.is_some() {
            return None;
        }
        out.push(elem.expr.as_ref());
    }
    Some(out)
}

/// Does `expr`'s static type prove a plain Array? The same predicate the
/// `for…of` desugar uses to decide it may read `.length` / `[i]` directly
/// (`stmt_loops.rs`'s `proven_array`).
pub(crate) fn proven_array(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match infer_type_from_expr(expr, ctx) {
        Type::Array(_) => true,
        Type::Generic { base, .. } => base == "Array",
        _ => false,
    }
}

/// #10524: is `expr`'s static type one whose runtime value can still be an
/// Array? Types that provably cannot (a primitive, a `Map`/`Set`/generator
/// instantiation, …) keep the plain protocol: the runtime guard would always
/// decline for them, so the fast arm would be dead code.
pub(crate) fn may_be_array_at_runtime(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    // Generator declarations are known even when return inference is Any.
    // This is an existing compile-time callee fact, not a runtime name guard.
    if let ast::Expr::Call(call) = expr {
        if let ast::Callee::Expr(callee) = &call.callee {
            if let ast::Expr::Ident(ident) = callee.as_ref() {
                if ctx.generator_func_names.contains(&ident.sym.to_string()) {
                    return false;
                }
            }
        }
    }
    match infer_type_from_expr(expr, ctx) {
        Type::Void
        | Type::Null
        | Type::Boolean
        | Type::Number
        | Type::Int32
        | Type::BigInt
        | Type::String
        | Type::StringLiteral(_)
        | Type::Symbol
        | Type::Never
        | Type::Function(_)
        | Type::Promise(_) => false,
        Type::Generic { base, .. } => base == "Array" || base == "ReadonlyArray",
        _ => true,
    }
}

/// Build the array representation of the shared record when the source can
/// be an Array. GetIterator still runs when its shape proof declines, including
/// empty patterns and invalid non-iterable values.
pub(crate) fn plan_for_source(
    ctx: &mut LoweringContext,
    _elems: &[Option<ast::Pat>],
    source: &ast::Expr,
) -> Result<Option<(Vec<Stmt>, FastPlan)>> {
    let mut setup = Vec::new();
    if let Some(literal) = spread_free_array_literal(source) {
        let plan = plan_for_literal(ctx, &literal, &mut setup)?;
        return Ok(Some((setup, plan)));
    }
    if proven_array(ctx, source) {
        let lowered = lower_expr(ctx, source)?;
        let plan = plan_for_proven_array(ctx, lowered, &mut setup);
        return Ok(Some((setup, plan)));
    }
    if may_be_array_at_runtime(ctx, source) {
        let lowered = lower_expr(ctx, source)?;
        let plan = plan_for_unproven_source(ctx, lowered, &mut setup);
        return Ok(Some((setup, plan)));
    }
    Ok(None)
}
