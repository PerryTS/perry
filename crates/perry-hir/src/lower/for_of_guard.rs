//! Synchronous array consumers use one IteratorRecord loop.
//!
//! `for (const x of [e0, …, eN-1])` over a short literal also gets a counted
//! loop over the evaluated elements, with no array and no IteratorRecord:
//!
//! ```text
//! const t0 = e0; …; const tN-1 = eN-1;       // the literal's own order
//! if (!ArrayIterationPatched) {
//!   for (let k = 0; k < N; k++) { const x = k === 0 ? t0 : … : tN-1; body }
//! } else {
//!   for (const x of [t0, …, tN-1]) body       // the IteratorRecord loop
//! }
//! ```
//!
//! `ArrayIterationPatched` is the constant `false` when no write in the
//! program can reach an iteration member (`Symbol.iterator` on an array or
//! `%Array.prototype%`, or `%ArrayIteratorPrototype%`'s `next`), so codegen
//! folds the branch and keeps one loop. Otherwise it is one check per loop
//! entry. A fresh literal has no holes, no own iterator and no other
//! observer, so the builtin walk binds exactly `t0 … tN-1` in order. An
//! abrupt exit (break, return, throw) closes an array iterator, which is
//! unobservable unless an iteration member was written, and then the
//! IteratorRecord loop runs instead.
//!
//! In the chess engine, `for (const delta of [-1, 1])` cost 281 instructions
//! per iteration (Bun: 8): the literal's allocation, the record's mode
//! selectors, and the `try`/`catch`/`finally` of the iterator close. Rewriting
//! the three such loops by hand saved 5.1% of search.
//!
//! Any other source is evaluated once and walked by index when it is an
//! ordinary array with default iteration (`source_for_of`), else iterated by
//! the IteratorRecord loop as before.
//!
//! Admitted: not `for await`; at most eight elements, no holes or spreads; one
//! `const`/`let` identifier binding; no labeled `break`/`continue` in the body
//! (the label would name the `if`, not the loop); for a non-literal source, a
//! body of at most 2 KiB of source (both arms lower it). `PERRY_LITERAL_FOR_OF=0`
//! keeps the IteratorRecord loop only (A/B control).
use super::{lower_expr, stmt_loops::lower_stmt_for_of_inner, LoweringContext};
use crate::lower_decl::lower_body_stmt;
use crate::types::Type;
use crate::{CompareOp, Expr, Module, Stmt, UnaryOp, UpdateOp};
use anyhow::Result;
use swc_common::DUMMY_SP;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

pub(crate) fn lower_stmt_for_of(
    ctx: &mut LoweringContext,
    module: &mut Module,
    stmt: &ast::ForOfStmt,
) -> Result<()> {
    let lowered = literal_for_of(ctx, stmt, &mut |ctx, fallback| {
        let start = module.init.len();
        lower_stmt_for_of_inner(ctx, module, fallback)?;
        Ok(module.init.drain(start..).collect())
    })?;
    match lowered {
        Some(stmts) => {
            module.init.extend(stmts);
            Ok(())
        }
        None => lower_stmt_for_of_inner(ctx, module, stmt),
    }
}

/// The same rewrite for a for-of inside a function body
/// (`lower_decl::body_stmt`), which lowers its fallback through the body
/// lowering. `None`: the loop is not admitted; lower it as before.
pub(crate) fn lower_body_literal_for_of(
    ctx: &mut LoweringContext,
    stmt: &ast::ForOfStmt,
) -> Result<Option<Vec<Stmt>>> {
    literal_for_of(ctx, stmt, &mut |ctx, fallback| {
        crate::lower_decl::lower_body_stmt(ctx, &ast::Stmt::ForOf(fallback.clone()))
    })
}

type LowerFallback<'a> = dyn FnMut(&mut LoweringContext, &ast::ForOfStmt) -> Result<Vec<Stmt>> + 'a;

const TEMP_PREFIX: &str = "__forof_lit_";
const MAX_ELEMENTS: usize = 8;

/// Does the body contain a labeled `break` or `continue` anywhere?
struct LabeledJump(bool);

impl Visit for LabeledJump {
    fn visit_break_stmt(&mut self, s: &ast::BreakStmt) {
        self.0 |= s.label.is_some();
    }
    fn visit_continue_stmt(&mut self, s: &ast::ContinueStmt) {
        self.0 |= s.label.is_some();
    }
}

/// The loop's binding, when the rewrite admits it: one `const`/`let`
/// identifier, and no labeled jump in the body.
fn admitted_binding(stmt: &ast::ForOfStmt) -> Option<(&ast::VarDecl, &ast::BindingIdent)> {
    let ast::ForHead::VarDecl(var_decl) = &stmt.left else {
        return None;
    };
    if var_decl.kind == ast::VarDeclKind::Var || var_decl.decls.len() != 1 {
        return None;
    }
    let ast::Pat::Ident(binding) = &var_decl.decls[0].name else {
        return None;
    };
    let mut labeled = LabeledJump(false);
    stmt.body.visit_with(&mut labeled);
    (!labeled.0).then_some((var_decl.as_ref(), binding))
}

fn runtime(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}

/// A source the IteratorRecord or a collection view already lowers better
/// than the walk: a (parenthesized or cast) array literal keeps the record's
/// scalar custody and constant bound, and `x.values()` / `x.keys()` /
/// `x.entries()` keep the Map/Set view's index path, which matches that call
/// as the loop's own source.
fn has_own_route(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Paren(p) => has_own_route(&p.expr),
        ast::Expr::TsAs(t) => has_own_route(&t.expr),
        ast::Expr::TsSatisfies(t) => has_own_route(&t.expr),
        ast::Expr::TsNonNull(t) => has_own_route(&t.expr),
        ast::Expr::TsTypeAssertion(t) => has_own_route(&t.expr),
        ast::Expr::TsConstAssertion(t) => has_own_route(&t.expr),
        ast::Expr::Array(_) => true,
        ast::Expr::Call(call) => matches!(
            &call.callee,
            ast::Callee::Expr(callee) if matches!(
                callee.as_ref(),
                ast::Expr::Member(m) if matches!(
                    &m.prop,
                    ast::MemberProp::Ident(p) if matches!(&*p.sym, "values" | "keys" | "entries")
                )
            )
        ),
        _ => false,
    }
}

#[cfg(test)]
thread_local! {
    /// Lets the IteratorRecord's own tests see the loop it lowers alone.
    pub(crate) static SOURCE_WALK_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn source_walk_enabled() -> bool {
    #[cfg(test)]
    if SOURCE_WALK_OFF.with(|c| c.get()) {
        return false;
    }
    true
}

/// The most body source bytes the general rewrite duplicates. Each arm lowers
/// the body once, so nested loops double per level; the cap bounds that.
const MAX_SOURCE_BODY_BYTES: u32 = 2048;

/// `for (const x of e)` over any other source: evaluate `e` once, then a
/// counted walk when it is an ordinary array whose iteration nobody replaced
/// (`arrayRecordNeedsIterator`, the IteratorRecord's own array-mode proof), else
/// the IteratorRecord loop over the same value.
///
/// ```text
/// const t = e;
/// if (!arrayRecordNeedsIterator(t)) {
///   const a: T[] = t;  // T: the source local's declared element type, or any
///   for (let k = 0; k < a.length; k++) { const x = a[k]; body }
/// } else {
///   for (const x of t) body
/// }
/// ```
///
/// The built-in array iterator reads `length`, then the element, at every
/// step; the walk does the same in the same order, so a body that pushes to
/// or truncates `t` sees what the iterator would. An abrupt exit closes no
/// iterator, exactly as the IteratorRecord's array mode under the same proof.
fn source_for_of(
    ctx: &mut LoweringContext,
    stmt: &ast::ForOfStmt,
    lower_fallback: &mut LowerFallback<'_>,
) -> Result<Option<Vec<Stmt>>> {
    // The fallback iterates the temp below; it must not take this path again.
    if matches!(stmt.right.as_ref(), ast::Expr::Ident(id) if id.sym.starts_with(TEMP_PREFIX)) {
        return Ok(None);
    }
    if !source_walk_enabled() || has_own_route(stmt.right.as_ref()) {
        return Ok(None);
    }
    {
        use swc_common::Spanned;
        let span = stmt.body.span();
        if span.hi.0.saturating_sub(span.lo.0) > MAX_SOURCE_BODY_BYTES {
            return Ok(None);
        }
    }
    let Some((var_decl, binding)) = admitted_binding(stmt) else {
        return Ok(None);
    };
    let scope = ctx.push_block_scope();
    let mut out = Vec::new();
    let result = (|| -> Result<()> {
        // The temp carries the source binding's own declared type, so the
        // IteratorRecord arm lowers exactly as the loop over that binding did.
        let source_ty = match stmt.right.as_ref() {
            ast::Expr::Ident(id) => ctx.lookup_local_type(&id.sym).cloned().unwrap_or(Type::Any),
            _ => Type::Any,
        };
        let value = lower_expr(ctx, &stmt.right)?;
        let source = ctx.fresh_local();
        let source_name = format!("{TEMP_PREFIX}src_{source}");
        ctx.locals
            .push((source_name.clone(), source, source_ty.clone()));
        out.push(Stmt::Let {
            id: source,
            name: source_name.clone(),
            ty: source_ty.clone(),
            mutable: false,
            init: Some(value),
        });

        // Inside the proven arm the source is an ordinary array: rebind it
        // with an array type (the source's own, when it is a local declared
        // as one) so its `length` and element reads take the array lowering.
        let array_ty = match stmt.right.as_ref() {
            ast::Expr::Ident(id) => match ctx.lookup_local_type(&id.sym) {
                Some(Type::Array(element)) => Type::Array(element.clone()),
                _ => Type::Array(Box::new(Type::Any)),
            },
            _ => Type::Array(Box::new(Type::Any)),
        };
        // A source already typed as that array is walked through the temp
        // itself. For a Number array the counted entry below
        // (`arrayRecordNeedsIterator(t, true)`) admits the packed-f64 loop over
        // `t` in the same runtime call as the proof, so the loop needs no
        // guard call of its own.
        let walk_source = array_ty == source_ty;
        let array = if walk_source {
            source
        } else {
            ctx.fresh_local()
        };
        let array_name = format!("{TEMP_PREFIX}arr_{array}");
        if !walk_source {
            ctx.locals
                .push((array_name.clone(), array, array_ty.clone()));
        }
        let element_ty = match &array_ty {
            Type::Array(element) => element.as_ref().clone(),
            _ => Type::Any,
        };

        let counter = ctx.fresh_local();
        let counter_name = format!("{TEMP_PREFIX}k_{counter}");
        ctx.locals
            .push((counter_name.clone(), counter, Type::Number));
        let body_scope = ctx.push_block_scope();
        let name = binding.id.sym.to_string();
        let x = ctx.define_local_spanned(name.clone(), element_ty.clone(), binding.id.span);
        if var_decl.kind == ast::VarDeclKind::Const {
            ctx.mark_local_immutable(x);
        }
        let body = lower_body_stmt(ctx, &stmt.body);
        ctx.pop_block_scope(body_scope);
        let body = body?;
        let mut loop_body = vec![Stmt::Let {
            id: x,
            name,
            ty: element_ty,
            mutable: var_decl.kind != ast::VarDeclKind::Const,
            init: Some(Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                element_offset: 0,
                object: Box::new(Expr::LocalGet(array)),
                index: Box::new(Expr::LocalGet(counter)),
            }),
        }];
        loop_body.extend(body);
        let counted = Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: counter,
                name: counter_name,
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(counter)),
                right: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    byte_end: 0,
                    object: Box::new(Expr::LocalGet(array)),
                    property: "length".into(),
                }),
            }),
            update: Some(Expr::Update {
                id: counter,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: loop_body,
        };

        let mut fallback = stmt.clone();
        fallback.right = Box::new(ast::Expr::Ident(ast::Ident::new(
            source_name.as_str().into(),
            DUMMY_SP,
            Default::default(),
        )));
        let protocol = lower_fallback(ctx, &fallback)?;

        // The IteratorRecord's own array-mode proof: an ordinary array whose
        // iteration members (its prototype's, the array iterator's) nobody
        // replaced. It reads the program's iteration facts itself.
        // Only a Number array can take the packed-f64 loop; asking any other
        // source for the admission would scan its slots for nothing.
        let mut entry_args = vec![Expr::LocalGet(source)];
        if walk_source && matches!(&array_ty, Type::Array(e) if **e == Type::Number) {
            entry_args.push(Expr::Bool(true));
        }
        let plain = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(runtime("arrayRecordNeedsIterator", entry_args)),
            right: Box::new(Expr::Bool(false)),
        };
        let then_branch = if walk_source {
            vec![counted]
        } else {
            vec![
                Stmt::Let {
                    id: array,
                    name: array_name,
                    ty: array_ty,
                    mutable: false,
                    init: Some(Expr::LocalGet(source)),
                },
                counted,
            ]
        };
        out.push(Stmt::If {
            condition: plain,
            then_branch,
            else_branch: Some(protocol),
        });
        Ok(())
    })();
    ctx.pop_block_scope(scope);
    result.map(|()| Some(out))
}

/// How many rewritten loops may enclose another rewrite. Each rewrite lowers
/// its body once per arm, so a loop nested inside `n` rewrites is lowered 2^n
/// times; past this depth a loop takes the IteratorRecord lowering alone, and
/// nested eligible loops cost at most 2^MAX_REWRITE_NESTING body copies.
const MAX_REWRITE_NESTING: u32 = 3;

thread_local! {
    static REWRITE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Marks one rewrite in progress for as long as it lives, so the bodies it
/// lowers see one more enclosing rewrite.
struct Nesting;

impl Nesting {
    fn enter() -> Option<Self> {
        REWRITE_DEPTH.with(|d| {
            (d.get() < MAX_REWRITE_NESTING).then(|| {
                d.set(d.get() + 1);
                Nesting
            })
        })
    }
}

impl Drop for Nesting {
    fn drop(&mut self) {
        REWRITE_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

fn literal_for_of(
    ctx: &mut LoweringContext,
    stmt: &ast::ForOfStmt,
    lower_fallback: &mut LowerFallback<'_>,
) -> Result<Option<Vec<Stmt>>> {
    if stmt.is_await || std::env::var("PERRY_LITERAL_FOR_OF").as_deref() == Ok("0") {
        return Ok(None);
    }
    let Some(_nesting) = Nesting::enter() else {
        return Ok(None);
    };
    let ast::Expr::Array(lit) = stmt.right.as_ref() else {
        return source_for_of(ctx, stmt, lower_fallback);
    };
    let n = lit.elems.len();
    if n == 0 || n > MAX_ELEMENTS {
        return Ok(None);
    }
    let mut elems = Vec::with_capacity(n);
    for e in &lit.elems {
        match e {
            Some(ast::ExprOrSpread { spread: None, expr }) => elems.push(expr.as_ref()),
            _ => return Ok(None),
        }
    }
    // The IteratorRecord arm below iterates a literal of these temps; it
    // must not take this path again.
    if elems
        .iter()
        .any(|e| matches!(e, ast::Expr::Ident(id) if id.sym.starts_with(TEMP_PREFIX)))
    {
        return Ok(None);
    }
    let ast::ForHead::VarDecl(var_decl) = &stmt.left else {
        return Ok(None);
    };
    if var_decl.kind == ast::VarDeclKind::Var || var_decl.decls.len() != 1 {
        return Ok(None);
    }
    let ast::Pat::Ident(binding) = &var_decl.decls[0].name else {
        return Ok(None);
    };
    let mut labeled = LabeledJump(false);
    stmt.body.visit_with(&mut labeled);
    if labeled.0 {
        return Ok(None);
    }

    let scope = ctx.push_block_scope();
    let mut out = Vec::new();
    let result = (|| -> Result<()> {
        // The literal's elements, once, in order.
        let mut temps = Vec::with_capacity(n);
        for e in &elems {
            let value = lower_expr(ctx, e)?;
            let id = ctx.fresh_local();
            let name = format!("{TEMP_PREFIX}{id}");
            ctx.locals.push((name.clone(), id, Type::Any));
            out.push(Stmt::Let {
                id,
                name: name.clone(),
                ty: Type::Any,
                mutable: false,
                init: Some(value),
            });
            temps.push((id, name));
        }

        // The counted loop.
        let counter = ctx.fresh_local();
        let counter_name = format!("{TEMP_PREFIX}k_{counter}");
        ctx.locals
            .push((counter_name.clone(), counter, Type::Number));
        let body_scope = ctx.push_block_scope();
        let name = binding.id.sym.to_string();
        let x = ctx.define_local_spanned(name.clone(), Type::Any, binding.id.span);
        if var_decl.kind == ast::VarDeclKind::Const {
            ctx.mark_local_immutable(x);
        }
        let body = lower_body_stmt(ctx, &stmt.body);
        ctx.pop_block_scope(body_scope);
        let body = body?;
        let mut select = Expr::LocalGet(temps[n - 1].0);
        for (i, (id, _)) in temps.iter().enumerate().take(n - 1).rev() {
            select = Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(counter)),
                    right: Box::new(Expr::Integer(i as i64)),
                }),
                then_expr: Box::new(Expr::LocalGet(*id)),
                else_expr: Box::new(select),
            };
        }
        let mut loop_body = vec![Stmt::Let {
            id: x,
            name,
            ty: Type::Any,
            mutable: var_decl.kind != ast::VarDeclKind::Const,
            init: Some(select),
        }];
        loop_body.extend(body);
        let counted = Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: counter,
                name: counter_name,
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(counter)),
                right: Box::new(Expr::Integer(n as i64)),
            }),
            update: Some(Expr::Update {
                id: counter,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: loop_body,
        };

        // The IteratorRecord loop over the same values, for a program that
        // may have patched iteration.
        let mut fallback = stmt.clone();
        fallback.right = Box::new(ast::Expr::Array(ast::ArrayLit {
            span: lit.span,
            elems: temps
                .iter()
                .map(|(_, name)| {
                    Some(ast::ExprOrSpread {
                        spread: None,
                        expr: Box::new(ast::Expr::Ident(ast::Ident::new(
                            name.as_str().into(),
                            DUMMY_SP,
                            Default::default(),
                        ))),
                    })
                })
                .collect(),
        }));
        let protocol = lower_fallback(ctx, &fallback)?;

        out.push(Stmt::If {
            condition: Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(Expr::ArrayIterationPatched),
            },
            then_branch: vec![counted],
            else_branch: Some(protocol),
        });
        Ok(())
    })();
    ctx.pop_block_scope(scope);
    result.map(|()| Some(out))
}
