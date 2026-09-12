//! #10086: the non-iterator arm of array destructuring.
//!
//! Both destructuring entry points (`[a, b] = rhs` as a statement and
//! `const [a, b] = rhs`) lower an array pattern through the full spec iterator
//! protocol: `GetIterator`, one `iteratorNextResult` per element (each of which
//! allocates a `{ value, done }` result object), two property reads off that
//! result, and `IteratorClose`. For a plain array that is the whole cost — the
//! measured swap (`[x, y] = [y, x]`) spent 76x Node on machinery that is, for an
//! unpatched `Array.prototype[Symbol.iterator]`, entirely unobservable.
//!
//! The patch is a RUNTIME fact and the choice of lowering is a COMPILE-TIME one,
//! so — exactly as #7760 item 1 did for `for…of` over a proven array — the only
//! correct answer is to emit both and branch on [`Expr::ArrayIterationPatched`],
//! the volatile `i8` read of the runtime's sticky
//! `PERRY_ARRAY_ITERATION_NOT_PRISTINE` byte.
//!
//! Two properties shaped this differently from the `for…of` guard:
//!
//!   * The branch is PER ELEMENT, not around the whole pattern. A destructuring
//!     pattern DECLARES bindings (`const [a, b] = …`); lowering the pattern
//!     twice would allocate two different `LocalId`s for `a`, and every use
//!     after the `if` would resolve to whichever arm was lowered last.
//!     Branching only where the element VALUE is produced keeps one binding per
//!     leaf.
//!   * Branching per element is also what keeps the lowering spec-exact. Both
//!     arms interleave element production with the pattern's own work, so
//!     `let [a = f(), b] = src` still evaluates `f()` between producing element
//!     0 and producing element 1 — an eager "pull N values, then bind" fast arm
//!     would not.
//!
//! The iterator arm is what the pre-existing lowering emitted apart from
//! `GetIterator` moving behind the guard, so a patched iterator behaves exactly
//! as it did before.
//!
//! The guard covers BOTH ways array iteration stops being the builtin protocol.
//! A replaced or deleted `Array.prototype[Symbol.iterator]` already set that
//! byte (#7760). A replaced `%ArrayIteratorPrototype%.next` did not: the runtime
//! detects it per `.next()` call, which an arm that never calls `.next()` cannot
//! observe. #10086 also publishes the byte when the array-iterator prototype
//! object escapes to user code — the only way to name it in order to patch it —
//! so both arms decline. That closed the same hole in the `for…of` index loop,
//! which had it since #7760. Spread keeps its own Rust-side proof
//! (`array::dense_spread_source`) and is unchanged.
//!
//! Element production on the fast arm comes in two shapes ([`FastElements`]):
//! an index read for a statically-proven array, and — when the source is a
//! spread-free array literal written in place — the literal's already-spilled
//! element temps, which removes the array ALLOCATION as well (the swap's one GC
//! allocation per iteration).

use super::*;

/// How one array pattern reaches its elements.
pub(crate) enum ArraySource {
    /// No proof that the source is a plain array — drive the spec iterator
    /// protocol on this expression, unguarded. What every array pattern did
    /// before #10086, and what a nested pattern, a rest element or an unproven
    /// source still does.
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

    /// Produce element `idx` into `value_id`. `iter_pull` is the caller's
    /// existing iterator-step sequence, used verbatim on the protocol arm.
    pub(crate) fn pull(&self, idx: usize, value_id: LocalId, iter_pull: Vec<Stmt>) -> Vec<Stmt> {
        match self {
            ArraySource::Iterator(_) => iter_pull,
            ArraySource::Guarded(plan) => vec![plan.guarded_pull(idx, value_id, iter_pull)],
        }
    }

    /// `IteratorClose`, run only where an iterator was actually created.
    pub(crate) fn close(&self, close: Stmt) -> Stmt {
        match self {
            ArraySource::Iterator(_) => close,
            ArraySource::Guarded(plan) => plan.guarded_close(close),
        }
    }
}

/// Where the fast arm reads element `i` from.
pub(crate) enum FastElements {
    /// A local holding a statically-proven plain Array. Element `i` is
    /// `i < src.length ? src[i] : undefined` — the bounds test is what makes an
    /// out-of-range element `undefined` (so a destructuring default still
    /// fires) instead of whatever the backing store answers past its end.
    /// `length` is re-read per element because the spec's `IteratorStep` does:
    /// a default initializer that truncates the array must be visible to the
    /// next element.
    Indexed(LocalId),
    /// One local per element of a spread-free array literal, spilled in source
    /// order BEFORE the guard (so each element expression is evaluated exactly
    /// once, whichever arm runs). Element `i` past the end is `undefined`, the
    /// value the iterator would have produced once exhausted.
    Scalars(Vec<LocalId>),
}

/// The guarded-pull plan for one array pattern.
pub(crate) struct FastPlan {
    /// Boolean local holding the once-evaluated [`Expr::ArrayIterationPatched`].
    /// Read once per element; the branch is loop-invariant and perfectly
    /// predicted.
    use_iter: LocalId,
    /// The expression the iterator arm calls `GetIterator` on. Evaluated ONLY on
    /// that arm — for the `Scalars` shape it is the array literal rebuilt from
    /// the spilled temps, so the fast arm allocates no array at all.
    iter_source: Expr,
    elements: FastElements,
}

impl FastPlan {
    /// The fast arm's value for element `idx`.
    fn value(&self, idx: usize) -> Expr {
        match &self.elements {
            FastElements::Scalars(temps) => match temps.get(idx) {
                Some(id) => Expr::LocalGet(*id),
                None => Expr::Undefined,
            },
            FastElements::Indexed(src) => Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::Number(idx as f64)),
                    right: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::LocalGet(*src)),
                        property: "length".to_string(),
                    }),
                }),
                then_expr: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(*src)),
                    index: Box::new(Expr::Number(idx as f64)),
                }),
                else_expr: Box::new(Expr::Undefined),
            },
        }
    }

    /// `__use ? GetIterator(<source>) : undefined` — the iterator (and, for the
    /// literal shape, the array it iterates) is materialized only when the
    /// protocol is actually patched. A `Conditional` rather than a mutable
    /// local written inside an `if` keeps the iterator binding an immutable
    /// `Let`, the same shape the unguarded lowering emitted.
    pub(crate) fn guarded_get_iterator(&self) -> Expr {
        Expr::Conditional {
            condition: Box::new(Expr::LocalGet(self.use_iter)),
            then_expr: Box::new(Expr::GetIterator(Box::new(self.iter_source.clone()))),
            else_expr: Box::new(Expr::Undefined),
        }
    }

    /// `if (__use) { <iterator pull> } else { value = <fast element> }`.
    /// `iter_pull` is the caller's existing per-element iterator sequence,
    /// unchanged.
    pub(crate) fn guarded_pull(&self, idx: usize, value_id: LocalId, iter_pull: Vec<Stmt>) -> Stmt {
        Stmt::If {
            condition: Expr::LocalGet(self.use_iter),
            then_branch: iter_pull,
            else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
                value_id,
                Box::new(self.value(idx)),
            ))]),
        }
    }

    /// `if (__use) { <IteratorClose> }` — there is nothing to close on the fast
    /// arm, which never created an iterator.
    pub(crate) fn guarded_close(&self, close: Stmt) -> Stmt {
        Stmt::If {
            condition: Expr::LocalGet(self.use_iter),
            then_branch: vec![close],
            else_branch: None,
        }
    }
}

fn fresh(ctx: &mut LoweringContext, ty: Type) -> (LocalId, String) {
    let id = ctx.fresh_local();
    let name = format!("__destruct_fast_{}", id);
    ctx.locals.push((name.clone(), id, ty));
    (id, name)
}

fn push_guard(ctx: &mut LoweringContext, out: &mut Vec<Stmt>) -> LocalId {
    let (id, name) = fresh(ctx, Type::Boolean);
    out.push(Stmt::Let {
        id,
        name,
        ty: Type::Boolean,
        mutable: false,
        init: Some(Expr::ArrayIterationPatched),
    });
    id
}

/// Plan for a spread-free array-literal source: spill every element into a temp
/// in source order (evaluated exactly once, on either arm), then read the guard.
pub(crate) fn plan_for_literal(
    ctx: &mut LoweringContext,
    elems: &[&ast::Expr],
    out: &mut Vec<Stmt>,
) -> Result<FastPlan> {
    let mut temps = Vec::with_capacity(elems.len());
    for elem in elems {
        let value = lower_expr(ctx, elem)?;
        let (id, name) = fresh(ctx, Type::Any);
        out.push(Stmt::Let {
            id,
            name,
            ty: Type::Any,
            mutable: false,
            init: Some(value),
        });
        temps.push(id);
    }
    let use_iter = push_guard(ctx, out);
    Ok(FastPlan {
        use_iter,
        iter_source: Expr::Array(temps.iter().map(|id| Expr::LocalGet(*id)).collect()),
        elements: FastElements::Scalars(temps),
    })
}

/// Plan for a source whose static type proves a plain Array: spill it into one
/// local that BOTH arms read, then read the guard.
pub(crate) fn plan_for_proven_array(
    ctx: &mut LoweringContext,
    source: Expr,
    out: &mut Vec<Stmt>,
) -> FastPlan {
    // `Array(Any)` (not `Any`): an `Any`-typed temp routes element reads through
    // the typed-element fast path, which answers a miss with `0` rather than
    // `undefined` and so breaks destructuring defaults. Same choice, for the
    // same reason, as `emit_for_of_pattern_binding`'s temp.
    let ty = Type::Array(Box::new(Type::Any));
    let (src_id, src_name) = fresh(ctx, ty.clone());
    out.push(Stmt::Let {
        id: src_id,
        name: src_name,
        ty,
        mutable: false,
        init: Some(source),
    });
    let use_iter = push_guard(ctx, out);
    FastPlan {
        use_iter,
        iter_source: Expr::LocalGet(src_id),
        elements: FastElements::Indexed(src_id),
    }
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

/// Can every element of this pattern be produced by index?
///
/// A rest element cannot: draining the remainder through the iterator builds a
/// DENSE array, while the obvious index-side equivalent (`slice`) preserves
/// holes, so the two disagree on a sparse source. A pattern with a rest element
/// keeps the unguarded iterator lowering.
pub(crate) fn pattern_admits_index_reads(elems: &[Option<ast::Pat>]) -> bool {
    !elems.iter().any(|e| matches!(e, Some(ast::Pat::Rest(_))))
}

/// #10086: build the guarded non-iterator plan for a destructuring source, or
/// `None` when this pattern/source pair keeps the plain iterator lowering.
/// Returns the setup statements the plan depends on (the literal's element
/// spills or the source spill, plus the guard read) alongside it.
///
/// `source` is lowered ONLY on the paths that return `Some`, so a caller that
/// falls back still lowers it exactly once.
pub(crate) fn plan_for_source(
    ctx: &mut LoweringContext,
    elems: &[Option<ast::Pat>],
    source: &ast::Expr,
) -> Result<Option<(Vec<Stmt>, FastPlan)>> {
    // An empty pattern (`[] = x`) reads no element, so the fast arm would touch
    // the source not at all — and `GetIterator` is the only thing that makes
    // `[] = <not iterable>` throw. Keep the protocol so it still does.
    if elems.is_empty() || !pattern_admits_index_reads(elems) {
        return Ok(None);
    }
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
    Ok(None)
}
