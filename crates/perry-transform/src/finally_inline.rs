//! Inline `finally` bodies before each abrupt completion that would
//! escape an enclosing `try { ... } finally { ... }`.
//!
//! ECMAScript spec: any `return` / `break` / `continue` (labeled or
//! unlabeled) that escapes a try body must run the corresponding
//! `finally` body before the abrupt completion takes effect. Pre-fix
//! Perry's codegen `Stmt::Return` lowering pop'd open try frames via
//! `js_try_end` and emitted `ret` directly, never running the `finally`
//! body; the same shape of bug applies to `Stmt::Break` / `Continue` /
//! `LabeledBreak` / `LabeledContinue` (the abrupt-completion stmt
//! terminates the basic block before the `br finally_label` is emitted).
//! The async-to-generator transform additionally flattens the try body
//! into a sequence of states with the finally body pushed AFTER the
//! body — but an abrupt completion in the body terminates the state
//! before the appended finally runs.
//!
//! This pass desugars at the HIR level, BEFORE async/generator transforms:
//!
//!   try { ...; return X; ... } finally { Y }
//!     →
//!   try { ...; let __ret = X; Y_clone; return __ret; ... } finally { Y }
//!
//!   for (...) {
//!     try { ...; if (cond) break; ... } finally { Y }
//!   }
//!     →
//!   for (...) {
//!     try { ...; if (cond) { Y_clone; break; } ... } finally { Y }
//!   }
//!
//! The `return` operand is hoisted into a fresh `let` so it evaluates
//! BEFORE the finally body — matching ECMAScript spec ordering (the
//! operand of `return` is evaluated, THEN finally runs, THEN the function
//! returns). Naïvely `Y_clone; return X` would inadvertently run Y before
//! X is evaluated, breaking the `return await foo()` pattern at the
//! heart of issue #536 (the await would suspend AFTER finally already
//! ran). `break` / `continue` have no operand so the prepend is direct.
//!
//! `return;` (no value) is rewritten as `Y_clone; return;` directly —
//! no hoist needed since there's no operand to evaluate.
//!
//! `break` / `continue` only inline finallies for trys that are INSIDE
//! the loop being escaped — finallies for OUTER trys (whose body
//! contains the loop, not the other way around) keep running through
//! the existing setjmp/longjmp path. The pass tracks `loop_depth` at
//! which each finally was pushed and runs only those whose
//! `loop_depth_at_push >= current_loop_depth_for_break`. Labeled
//! `break L` / `continue L` use the same comparison against the loop
//! depth recorded for the matching label.
//!
//! The original finally is preserved so non-abrupt-completion paths
//! still run it (and the throw path keeps using setjmp/longjmp through
//! `lower_try`). Each abrupt completion inside the body (or catch)
//! gets a clone of the innermost finally inlined immediately before
//! it. Nested try-with-finally stack the inlined clones
//! innermost→outermost.
//!
//! Limitations:
//!   - Code after the abrupt completion becomes dead — fine.
//!   - If `Y_clone` itself throws, the throw routes to the same try's
//!     catch (if any) instead of propagating directly. Per spec the
//!     throw should override the pending abrupt completion without
//!     going through the catch. Rare; matters only when the finally
//!     throws AND there's a same-level catch.
//!
//! Issue #536: `@perryts/mysql`'s `Pool.query` uses
//! `try { return await conn.query(...) } finally { this.release(conn) }`
//! — without this pass, `release` never runs, the connection stays
//! checked out, and `pool.end()` sees zero idle conns to close. Process
//! hangs because the underlying tokio socket task was never told to shut
//! down. The break/continue arms ride along (test_finally_break.ts and
//! similar shapes in user code that mix loop control with
//! resource-cleanup finallies).

use std::collections::HashMap;

use perry_hir::{Expr, Module, Stmt};
use perry_types::{LocalId, Type};

// #5293: the max-LocalId scan was copy-pasted here; route through the canonical
// (exhaustive-walker-backed) implementation in `generator::id_scan` instead.
use crate::generator::compute_max_local_id;

/// One open try-with-finally on the lowering stack. The cloned `body`
/// gets inlined ahead of any abrupt completion (return/break/continue)
/// that would escape this try frame. `loop_depth_at_push` is the value
/// of the surrounding `loop_depth` counter at the moment this try was
/// entered — used to filter which finallies a `break`/`continue` would
/// actually skip past (a `break` only escapes the innermost loop, so
/// a finally pushed BEFORE entering that loop stays untouched).
#[derive(Clone)]
struct EnclosingFinally {
    body: Vec<Stmt>,
    loop_depth_at_push: usize,
}

/// Run the pass over an entire HIR module.
pub fn inline_finally_into_returns(module: &mut Module) {
    let mut next_local_id = compute_max_local_id(module).saturating_add(1);
    let mut label_depths: HashMap<String, usize> = HashMap::new();
    for func in &mut module.functions {
        process_stmts(
            &mut func.body,
            &[],
            0,
            &mut label_depths,
            &mut next_local_id,
        );
    }
    for class in &mut module.classes {
        for method in &mut class.methods {
            process_stmts(
                &mut method.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
        for static_method in &mut class.static_methods {
            process_stmts(
                &mut static_method.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
        if let Some(ctor) = &mut class.constructor {
            process_stmts(
                &mut ctor.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
        for getter in class.getters.iter_mut() {
            process_stmts(
                &mut getter.1.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
        for setter in class.setters.iter_mut() {
            process_stmts(
                &mut setter.1.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
        // #5854: computed-key members (`async [k]() { try … finally … }`) have
        // method-like bodies and must inline finally-into-returns exactly like
        // `methods`/`static_methods` above. Omitting them left an async computed
        // method's finally un-inlined, so its generator state machine could skip
        // the finally on a return path.
        for member in class.computed_members.iter_mut() {
            process_stmts(
                &mut member.function.body,
                &[],
                0,
                &mut label_depths,
                &mut next_local_id,
            );
        }
    }
    for stmt in module.init.iter_mut() {
        let mut single = vec![std::mem::replace(stmt, Stmt::Break)];
        process_stmts(&mut single, &[], 0, &mut label_depths, &mut next_local_id);
        if let Some(s) = single.into_iter().next() {
            *stmt = s;
        }
    }
}

/// Process a statement list with the given stack of enclosing finally
/// bodies (innermost last). Each abrupt-completion stmt encountered
/// (`Stmt::Return`, `Stmt::Break`, `Stmt::Continue`,
/// `Stmt::LabeledBreak`, `Stmt::LabeledContinue`) is rewritten to
/// prepend a clone of every finally that the abrupt completion would
/// SKIP PAST (innermost first). Recurses into nested control flow so
/// deeply nested completions are caught.
///
/// `loop_depth` is the number of enclosing `for` / `while` / `do…while`
/// at the current point — used to decide which finallies a `break`/
/// `continue` would actually skip (only those pushed at or beyond the
/// current loop's body depth).
///
/// `label_depths` maps each labeled-loop's label to the `loop_depth`
/// recorded just inside its body (i.e. the threshold for filtering
/// finallies on `break LABEL` / `continue LABEL`).
fn process_stmts(
    stmts: &mut Vec<Stmt>,
    enclosing: &[EnclosingFinally],
    loop_depth: usize,
    label_depths: &mut HashMap<String, usize>,
    next_local_id: &mut LocalId,
) {
    let owned = std::mem::take(stmts);
    let mut out: Vec<Stmt> = Vec::with_capacity(owned.len());
    for mut stmt in owned {
        // Walk into closure bodies INSIDE the stmt's expressions first.
        // Each closure is a fresh function scope — its returns are
        // governed by its OWN enclosing finallies, not ours.
        walk_stmt_exprs(&mut stmt, &mut |e| {
            process_expr_closure_bodies(e, next_local_id)
        });

        match stmt {
            Stmt::Return(value) if !enclosing.is_empty() => {
                // Hoist the return operand into a fresh `let` so it
                // evaluates BEFORE the inlined finally body. Critical
                // for `return await foo()` (issue #536's pool.query
                // shape) — without the hoist, finally would run BEFORE
                // the await suspends and the await result would be
                // discarded.
                let return_value: Option<Expr> = match value {
                    Some(e) => {
                        let id = *next_local_id;
                        *next_local_id += 1;
                        out.push(Stmt::Let {
                            id,
                            name: format!("__finally_ret_{}", id),
                            ty: Type::Any,
                            mutable: false,
                            init: Some(e),
                        });
                        Some(Expr::LocalGet(id))
                    }
                    None => None,
                };
                // A `return` escapes the WHOLE function — every
                // enclosing finally runs (innermost first), regardless
                // of loop_depth.
                inline_finallies(
                    &mut out,
                    enclosing,
                    enclosing.len(), // run all of them
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::Return(return_value));
            }
            Stmt::Break => {
                inline_finallies_for_break(
                    &mut out,
                    enclosing,
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::Break);
            }
            Stmt::Continue => {
                inline_finallies_for_break(
                    &mut out,
                    enclosing,
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::Continue);
            }
            Stmt::LabeledBreak(label) => {
                inline_finallies_for_labeled(
                    &mut out,
                    enclosing,
                    &label,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::LabeledBreak(label));
            }
            Stmt::LabeledContinue(label) => {
                inline_finallies_for_labeled(
                    &mut out,
                    enclosing,
                    &label,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::LabeledContinue(label));
            }
            Stmt::Try {
                mut body,
                mut catch,
                finally,
            } => {
                let mut extended: Vec<EnclosingFinally> = enclosing.to_vec();
                if let Some(f) = &finally {
                    extended.push(EnclosingFinally {
                        body: f.clone(),
                        loop_depth_at_push: loop_depth,
                    });
                }
                process_stmts(
                    &mut body,
                    &extended,
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                if let Some(c) = &mut catch {
                    process_stmts(
                        &mut c.body,
                        &extended,
                        loop_depth,
                        label_depths,
                        next_local_id,
                    );
                }
                let new_finally = finally.map(|mut f| {
                    process_stmts(&mut f, enclosing, loop_depth, label_depths, next_local_id);
                    f
                });
                out.push(Stmt::Try {
                    body,
                    catch,
                    finally: new_finally,
                });
            }
            Stmt::If {
                condition,
                mut then_branch,
                mut else_branch,
            } => {
                process_stmts(
                    &mut then_branch,
                    enclosing,
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                if let Some(eb) = &mut else_branch {
                    process_stmts(eb, enclosing, loop_depth, label_depths, next_local_id);
                }
                out.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                });
            }
            Stmt::While {
                condition,
                mut body,
            } => {
                process_stmts(
                    &mut body,
                    enclosing,
                    loop_depth + 1,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::While { condition, body });
            }
            Stmt::DoWhile {
                mut body,
                condition,
            } => {
                process_stmts(
                    &mut body,
                    enclosing,
                    loop_depth + 1,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::DoWhile { body, condition });
            }
            Stmt::For {
                mut init,
                condition,
                update,
                mut body,
            } => {
                if let Some(i) = &mut init {
                    let mut single = vec![std::mem::replace(i.as_mut(), Stmt::Break)];
                    process_stmts(
                        &mut single,
                        enclosing,
                        loop_depth,
                        label_depths,
                        next_local_id,
                    );
                    if let Some(s) = single.into_iter().next() {
                        **i = s;
                    }
                }
                process_stmts(
                    &mut body,
                    enclosing,
                    loop_depth + 1,
                    label_depths,
                    next_local_id,
                );
                out.push(Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                });
            }
            Stmt::Labeled { label, mut body } => {
                // If the labeled body is a loop, the depth at which a
                // `break LABEL` filters finallies is the depth INSIDE
                // the loop (loop_depth + 1). If it's a non-loop
                // labeled block, the threshold is loop_depth itself.
                let is_loop = matches!(
                    body.as_ref(),
                    Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. }
                );
                let label_threshold = if is_loop { loop_depth + 1 } else { loop_depth };
                let prev = label_depths.insert(label.clone(), label_threshold);
                let mut single = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                process_stmts(
                    &mut single,
                    enclosing,
                    loop_depth,
                    label_depths,
                    next_local_id,
                );
                if let Some(s) = single.into_iter().next() {
                    *body = s;
                }
                match prev {
                    Some(p) => {
                        label_depths.insert(label.clone(), p);
                    }
                    None => {
                        label_depths.remove(&label);
                    }
                }
                out.push(Stmt::Labeled { label, body });
            }
            Stmt::Switch {
                discriminant,
                mut cases,
            } => {
                for case in cases.iter_mut() {
                    process_stmts(
                        &mut case.body,
                        enclosing,
                        loop_depth,
                        label_depths,
                        next_local_id,
                    );
                }
                out.push(Stmt::Switch {
                    discriminant,
                    cases,
                });
            }
            other => out.push(other),
        }
    }
    *stmts = out;
}

/// Inline the innermost N finally bodies (innermost first) into `out`,
/// recursively processing each clone with the OUTER enclosing-finally
/// stack so a return-in-finally still gets the outer finallies prepended.
fn inline_finallies(
    out: &mut Vec<Stmt>,
    enclosing: &[EnclosingFinally],
    count: usize,
    loop_depth: usize,
    label_depths: &mut HashMap<String, usize>,
    next_local_id: &mut LocalId,
) {
    // Take the last `count` entries (innermost). Walk them in reverse
    // (innermost first) — matches ECMAScript abrupt-completion ordering.
    let take_from = enclosing.len().saturating_sub(count);
    for (idx, fin) in enclosing[take_from..].iter().enumerate().rev() {
        let mut cloned = fin.body.clone();
        // The cloned finally body sits at the abrupt-completion's
        // position. Its own internal abrupt completions are governed by
        // the OUTER finallies that are still on the stack — every
        // finally innermore than this one has already been "popped"
        // conceptually by the point this clone runs. Trim the slice
        // accordingly: keep entries [0..take_from + idx], drop the
        // rest (including this finally and all innermores).
        let outer_slice = &enclosing[..take_from + idx];
        process_stmts(
            &mut cloned,
            outer_slice,
            loop_depth,
            label_depths,
            next_local_id,
        );
        out.extend(cloned);
    }
}

/// Inline finallies for a plain `break` / `continue`. Only finallies
/// pushed AT OR DEEPER than the current loop's body depth get inlined —
/// outer finallies (those wrapping the loop) keep running through the
/// regular setjmp/longjmp path because the break stays inside their try.
fn inline_finallies_for_break(
    out: &mut Vec<Stmt>,
    enclosing: &[EnclosingFinally],
    loop_depth: usize,
    label_depths: &mut HashMap<String, usize>,
    next_local_id: &mut LocalId,
) {
    let count = enclosing
        .iter()
        .rev()
        .take_while(|f| f.loop_depth_at_push >= loop_depth)
        .count();
    if count > 0 {
        inline_finallies(
            out,
            enclosing,
            count,
            loop_depth,
            label_depths,
            next_local_id,
        );
    }
}

/// Inline finallies for a labeled `break L` / `continue L`. The label
/// resolves to the depth of the labeled loop's body — finallies pushed
/// at or below that depth stay (they wrap the labeled loop), and
/// finallies pushed deeper get inlined.
fn inline_finallies_for_labeled(
    out: &mut Vec<Stmt>,
    enclosing: &[EnclosingFinally],
    label: &str,
    label_depths: &mut HashMap<String, usize>,
    next_local_id: &mut LocalId,
) {
    let label_depth = match label_depths.get(label).copied() {
        Some(d) => d,
        None => {
            // Label not on the stack — leave the abrupt completion
            // untouched and let codegen handle it (likely a surface
            // syntax error or out-of-scope label that codegen will
            // diagnose).
            return;
        }
    };
    let count = enclosing
        .iter()
        .rev()
        .take_while(|f| f.loop_depth_at_push >= label_depth)
        .count();
    if count > 0 {
        // For label resolution inside cloned finallies, pass the same
        // label_depths (the labels are still in scope when the finally
        // body runs in its current position).
        let take_from = enclosing.len().saturating_sub(count);
        for (idx, fin) in enclosing[take_from..].iter().enumerate().rev() {
            let mut cloned = fin.body.clone();
            let outer_slice = &enclosing[..take_from + idx];
            // loop_depth doesn't strictly matter for inlining the
            // finally body itself (the finally body's own breaks are
            // governed by their own loop nesting), but pass the
            // finally's recorded depth so any nested-loop logic stays
            // consistent.
            let inline_loop_depth = fin.loop_depth_at_push;
            process_stmts(
                &mut cloned,
                outer_slice,
                inline_loop_depth,
                label_depths,
                next_local_id,
            );
            out.extend(cloned);
        }
    }
}

/// Walk all top-level expression operands inside `stmt`, calling `f` on
/// each. Used to recurse into expressions so we can find Closure bodies
/// to process.
fn walk_stmt_exprs<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => f(e),
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            for s in then_branch {
                walk_stmt_exprs(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            for s in body {
                walk_stmt_exprs(s, f);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                walk_stmt_exprs(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
            for s in body {
                walk_stmt_exprs(s, f);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                walk_stmt_exprs(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    walk_stmt_exprs(s, f);
                }
            }
            if let Some(fin) = finally {
                for s in fin {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &mut c.test {
                    f(t);
                }
                for s in &mut c.body {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::Labeled { body, .. } => walk_stmt_exprs(body, f),
        _ => {}
    }
}

/// Recursively walk an expression, processing the body of any `Closure`
/// found. Each closure's body starts with an empty enclosing-finally
/// stack — the closure is a fresh function frame.
fn process_expr_closure_bodies(expr: &mut Expr, next_local_id: &mut LocalId) {
    if let Expr::Closure { body, .. } = expr {
        let mut label_depths: HashMap<String, usize> = HashMap::new();
        process_stmts(body, &[], 0, &mut label_depths, next_local_id);
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |e| {
        process_expr_closure_bodies(e, next_local_id)
    });
}
