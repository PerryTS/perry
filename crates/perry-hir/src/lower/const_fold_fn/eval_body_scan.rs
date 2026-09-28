//! Eval-body AST predicates for the indirect-eval fold: does the body declare
//! a binding, carry a `var` initializer, or contain a class declaration.
//! Split out of `const_fold_fn.rs` to stay under the 2,000-line cap (#10750).

use swc_ecma_ast as ast;

/// Does an (indirect) eval body declare any binding — `var` / `function` /
/// `class` / `let` / `const` / `using` — anywhere within it? Scans recursively
/// through every statement form that can nest a declaration (a `var`/`function`
/// hoists out of blocks, loops, `try`, `switch`, `with`, labeled and `if`
/// statements), so the answer is conservative: a `true` defers the fold.
pub(super) fn eval_body_declares_bindings(stmts: &[ast::Stmt]) -> bool {
    stmts.iter().any(stmt_declares_binding)
}

fn stmt_declares_binding(stmt: &ast::Stmt) -> bool {
    use ast::Stmt;
    match stmt {
        Stmt::Decl(_) => true,
        Stmt::Block(b) => eval_body_declares_bindings(&b.stmts),
        Stmt::Labeled(l) => stmt_declares_binding(&l.body),
        Stmt::If(i) => {
            stmt_declares_binding(&i.cons) || i.alt.as_deref().is_some_and(stmt_declares_binding)
        }
        Stmt::For(f) => {
            matches!(&f.init, Some(ast::VarDeclOrExpr::VarDecl(_)))
                || stmt_declares_binding(&f.body)
        }
        Stmt::ForIn(f) => {
            matches!(
                &f.left,
                ast::ForHead::VarDecl(_) | ast::ForHead::UsingDecl(_)
            ) || stmt_declares_binding(&f.body)
        }
        Stmt::ForOf(f) => {
            matches!(
                &f.left,
                ast::ForHead::VarDecl(_) | ast::ForHead::UsingDecl(_)
            ) || stmt_declares_binding(&f.body)
        }
        Stmt::While(w) => stmt_declares_binding(&w.body),
        Stmt::DoWhile(d) => stmt_declares_binding(&d.body),
        Stmt::With(w) => stmt_declares_binding(&w.body),
        Stmt::Try(t) => {
            eval_body_declares_bindings(&t.block.stmts)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| eval_body_declares_bindings(&h.body.stmts))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| eval_body_declares_bindings(&f.stmts))
        }
        Stmt::Switch(s) => s.cases.iter().any(|c| eval_body_declares_bindings(&c.cons)),
        // Statements that cannot introduce a binding. Listed explicitly (no
        // `_` catch-all) so a future `ast::Stmt` variant that *can* nest a
        // declaration is a compile error here rather than a silent miss.
        Stmt::Expr(_)
        | Stmt::Empty(_)
        | Stmt::Debugger(_)
        | Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_) => false,
    }
}

/// Is an eval body safe to fold via [`apply_global_eval_hoist`] from a *nested*
/// (non-module-top) scope? The hoisted rewrite emits a bare `name = init`
/// assignment for `var` declarations that have an initializer, which would
/// capture an enclosing function-local of the same name rather than the global.
/// Var declarations without initializers and function/generator declarations
/// only produce `globalThis.*` writes, so they are safe from nested scopes.
pub(super) fn eval_body_no_var_initializers(stmts: &[ast::Stmt]) -> bool {
    !stmts.iter().any(stmt_has_var_with_initializer)
}

fn stmt_has_var_with_initializer(stmt: &ast::Stmt) -> bool {
    use ast::Stmt;
    match stmt {
        Stmt::Decl(ast::Decl::Var(v)) => {
            matches!(v.kind, ast::VarDeclKind::Var) && v.decls.iter().any(|d| d.init.is_some())
        }
        Stmt::Decl(_) => false,
        Stmt::Block(b) => b.stmts.iter().any(stmt_has_var_with_initializer),
        Stmt::Labeled(l) => stmt_has_var_with_initializer(&l.body),
        Stmt::If(i) => {
            stmt_has_var_with_initializer(&i.cons)
                || i.alt.as_deref().is_some_and(stmt_has_var_with_initializer)
        }
        Stmt::For(f) => {
            matches!(
                &f.init,
                Some(ast::VarDeclOrExpr::VarDecl(v)) if v.decls.iter().any(|d| d.init.is_some())
            ) || stmt_has_var_with_initializer(&f.body)
        }
        Stmt::ForIn(f) => stmt_has_var_with_initializer(&f.body),
        Stmt::ForOf(f) => stmt_has_var_with_initializer(&f.body),
        Stmt::While(w) => stmt_has_var_with_initializer(&w.body),
        Stmt::DoWhile(d) => stmt_has_var_with_initializer(&d.body),
        Stmt::With(w) => stmt_has_var_with_initializer(&w.body),
        Stmt::Try(t) => {
            t.block.stmts.iter().any(stmt_has_var_with_initializer)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.stmts.iter().any(stmt_has_var_with_initializer))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.stmts.iter().any(stmt_has_var_with_initializer))
        }
        Stmt::Switch(s) => s
            .cases
            .iter()
            .any(|c| c.cons.iter().any(stmt_has_var_with_initializer)),
        Stmt::Expr(_)
        | Stmt::Empty(_)
        | Stmt::Debugger(_)
        | Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_) => false,
    }
}

/// Can a declaration-bearing (indirect) eval body be folded to the completion
/// IIFE — executing it for its side effects and completion value — without an
/// observably wrong result? The one disqualifier is a *class declaration*: Perry
/// registers class names at module scope when lowering them inside the IIFE,
/// which would leak the class past the eval (real global eval discards the
/// eval's own lexical environment, so the class is invisible afterward — test262
/// `language/eval-code/indirect/lex-env-distinct-cls`). `var`/`function` only
/// fail to *publish* to the global var environment (trapped as arrow-locals),
/// which is no worse than the runtime thunk not executing the body at all; and
/// `let`/`const` correctly stay arrow-local (matching the eval's fresh, discarded
/// lexical environment). Scans recursively, mirroring [`stmt_declares_binding`].
pub(super) fn eval_body_iife_foldable(stmts: &[ast::Stmt]) -> bool {
    !stmts.iter().any(stmt_has_class_decl)
}

fn stmt_has_class_decl(stmt: &ast::Stmt) -> bool {
    use ast::Stmt;
    match stmt {
        Stmt::Decl(ast::Decl::Class(_)) => true,
        Stmt::Decl(_) => false,
        Stmt::Block(b) => b.stmts.iter().any(stmt_has_class_decl),
        Stmt::Labeled(l) => stmt_has_class_decl(&l.body),
        Stmt::If(i) => {
            stmt_has_class_decl(&i.cons) || i.alt.as_deref().is_some_and(stmt_has_class_decl)
        }
        Stmt::For(f) => stmt_has_class_decl(&f.body),
        Stmt::ForIn(f) => stmt_has_class_decl(&f.body),
        Stmt::ForOf(f) => stmt_has_class_decl(&f.body),
        Stmt::While(w) => stmt_has_class_decl(&w.body),
        Stmt::DoWhile(d) => stmt_has_class_decl(&d.body),
        Stmt::With(w) => stmt_has_class_decl(&w.body),
        Stmt::Try(t) => {
            t.block.stmts.iter().any(stmt_has_class_decl)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.stmts.iter().any(stmt_has_class_decl))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.stmts.iter().any(stmt_has_class_decl))
        }
        Stmt::Switch(s) => s
            .cases
            .iter()
            .any(|c| c.cons.iter().any(stmt_has_class_decl)),
        Stmt::Expr(_)
        | Stmt::Empty(_)
        | Stmt::Debugger(_)
        | Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_) => false,
    }
}
