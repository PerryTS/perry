//! Annex B.3.3.3 (Changes to EvalDeclarationInstantiation) — global var-scoped
//! hoisting for sloppy global `eval`. A block-scoped function declaration in a
//! global-eval body must bind in the global variable environment, which Perry's
//! completion-IIFE fold would otherwise trap as an arrow-local. This module
//! rewrites those declarations into global publishes before the fold; see
//! [`apply_global_eval_hoist`]. Split out of `const_fold_fn` to keep both files
//! under the workspace file-size gate.

use swc_ecma_ast as ast;

// ---- B.3.3.3 EvalDeclarationInstantiation: global var-scoped hoisting -------
//
// At global scope, sloppy direct/indirect eval routes the `var` and `function`
// declarations of its body into the *caller's* (global) variable environment —
// they survive after the eval returns (Annex B.3.3.3 / GlobalDeclarationInst-
// antiation). Perry folds an eval body into a scope-capturing arrow IIFE
// ([`build_eval_completion_iife`]); a `var`/`function` declared *inside* that
// arrow would be trapped as an arrow-local and vanish on return. So before
// folding, rewrite those var-scoped declarations into assignments to the global
// variable environment — which, in global-script mode, is `globalThis` itself
// (a sloppy undeclared assignment creates an own, enumerable, writable,
// configurable property, exactly matching CreateGlobalVarBinding /
// CreateGlobalFunctionBinding). Lexical declarations (`let`/`const`) are left in
// place: they belong to the eval's own lexical environment, which the arrow
// scope already models, and a `class` aborts the rewrite (Perry registers class
// names at module scope, which would leak past the eval).

/// Parse a single synthesized statement from source. Inputs are always
/// validated identifiers / string literals, so this never fails in practice;
/// `None` keeps the caller on its fallback.
fn parse_single_stmt(src: &str) -> Option<ast::Stmt> {
    let module = perry_parser::parse_typescript(src, "<eval hoist>.cjs").ok()?;
    match module.body.into_iter().next()? {
        ast::ModuleItem::Stmt(s) => Some(s),
        _ => None,
    }
}

/// Build `<name> = <init>;` as an expression statement, reusing the parsed
/// initializer. The bare assignment target resolves the same way the eval body's
/// own references do — to a pre-existing same-named variable in the enclosing
/// (global) variable environment if there is one, else a fresh sloppy global —
/// which is exactly the variable environment Annex B.3.3.3 binds into. Cloning a
/// parsed `__perry_lhs = 0;` template swaps the target identifier and right-hand
/// side, avoiding hand-built version-sensitive SWC `AssignExpr` nodes (same
/// approach as [`cv_assign_from_template`]).
fn synth_assign_stmt(name: &str, init: Box<ast::Expr>) -> Option<ast::Stmt> {
    let mut stmt = parse_single_stmt("__perry_lhs = 0;")?;
    let ast::Stmt::Expr(es) = &mut stmt else {
        return None;
    };
    let ast::Expr::Assign(a) = es.expr.as_mut() else {
        return None;
    };
    let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(binding)) = &mut a.left else {
        return None;
    };
    binding.id.sym = name.into();
    a.right = init;
    Some(stmt)
}

/// `<name> = <ident>;` — the value-transfer assignment that publishes a renamed
/// hidden function binding to the (global) variable-environment name.
fn synth_ident_assign_stmt(name: &str, ident: &str) -> Option<ast::Stmt> {
    synth_assign_stmt(
        name,
        Box::new(ast::Expr::Ident(ast::Ident {
            span: swc_common::DUMMY_SP,
            ctxt: Default::default(),
            sym: ident.into(),
            optional: false,
        })),
    )
}

/// `{ let <sink> = (<name> = <init>); }` — like [`synth_assign_stmt`] but
/// wrapped as a *completion-inert* lexical declaration so it keeps the *empty*
/// completion value of the `var` declaration it replaces. A VariableStatement's
/// completion is empty (§14.3.2): `(0,eval)("var x = 1")` must yield `undefined`,
/// and — crucially — `eval("9; var x = 1")` must yield `9` (the empty `var`
/// completion falls through to the prior statement). A bare `x = init` expression
/// statement would yield `init`; a `void (x = init)` statement is still an
/// expression statement, so the completion tracker rewrote it to
/// `__perry_cv = void(x = init)` = `undefined`, which *clobbered* a preceding
/// value (`eval("9; var x = 1")` wrongly became `undefined`). Wrapping the
/// publish as a `let` declaration inside its own block makes it a declaration —
/// which the completion tracker leaves untouched (like the original `var`) —
/// while the inner assignment still publishes to the (global) variable
/// environment. The throwaway is `let` (not `var`) so it stays lexically scoped
/// to the completion IIFE and is never re-hoisted to the global environment.
///
/// `sink` must be a caller-generated hidden name ([`GlobalEvalHoist::fresh_hidden`]),
/// not a fixed literal: the lexical `sink` binding is in TDZ while its own
/// initializer (`<name> = <init>`) evaluates, so a fixed name that the user's
/// `init` could reference (`eval("var x = __perry_eval_void")`) would throw a
/// spurious `ReferenceError`. A fresh per-publish name keeps `init`'s references
/// resolving exactly as they did before the rewrite. (test262
/// `language/statements/variable/cptn-value`, #5735.)
fn synth_inert_assign_stmt(sink: &str, name: &str, init: Box<ast::Expr>) -> Option<ast::Stmt> {
    let inner = synth_assign_stmt(name, init)?;
    let ast::Stmt::Expr(inner_es) = inner else {
        return None;
    };
    let mut block = parse_single_stmt("{ let __perry_eval_void = 0; }")?;
    let ast::Stmt::Block(b) = &mut block else {
        return None;
    };
    let Some(ast::Stmt::Decl(ast::Decl::Var(var))) = b.stmts.first_mut() else {
        return None;
    };
    let decl = var.decls.first_mut()?;
    let ast::Pat::Ident(binding) = &mut decl.name else {
        return None;
    };
    binding.id.sym = sink.into();
    decl.init = Some(inner_es.expr);
    Some(block)
}

/// CreateGlobalFunctionBinding for a renamed hidden *top-level* function:
/// publish its value to the global name `<name>` with the spec's descriptor
/// rules. This is declaration-instantiation machinery, not a statement of the
/// eval source, so it must contribute an *empty* completion value: the
/// `Object.defineProperty(...)` calls are `void`-wrapped because `defineProperty`
/// returns the target object (`globalThis`) — without the wrapper the completion
/// tracker rewrites the call to `__perry_cv = Object.defineProperty(...)` and a
/// declaration-only eval body (`eval("function f() {}")`) yields `globalThis`
/// instead of `undefined` (test262 `language/statements/*/cptn-decl`). Same
/// empty-completion reasoning as [`synth_inert_assign_stmt`] for the `var`
/// publish (this block is *prepended* before the body, so `void`-wrapping its
/// stores to `undefined` suffices — a later user statement overwrites it, and a
/// declaration-only body correctly ends at `undefined`).
///
/// Implements `CanDeclareGlobalFunction` (ECMA-262 §8.1.1.4.15):
/// ```text
/// { let __perry_d = Object.getOwnPropertyDescriptor(globalThis, "<name>");
///   if (__perry_d === void 0) {
///     if (!Object.isExtensible(globalThis))
///       throw new TypeError("Cannot declare global function: <name>");
///     void Object.defineProperty(globalThis, "<name>",
///                           { value: <hidden>, writable: true, enumerable: true, configurable: true });
///   } else if (__perry_d.configurable) {
///     void Object.defineProperty(globalThis, "<name>",
///                           { value: <hidden>, writable: true, enumerable: true, configurable: true });
///   } else if (!__perry_d.writable || !__perry_d.enumerable) {
///     throw new TypeError("Cannot declare global function: <name>");
///   } else {
///     void Object.defineProperty(globalThis, "<name>", { value: <hidden> });
///   }
/// }
/// ```
///
/// - Absent property: allowed only when the global is extensible (step 3).
/// - Configurable property: always allowed (step 4).
/// - Non-configurable, writable+enumerable data property: update value (step 5
///   returns true → CreateGlobalFunctionBinding value-only update).
/// - Non-configurable, non-(writable+enumerable): `CanDeclareGlobalFunction`
///   returns false → throw TypeError (step 6), matching `eval("function NaN(){}")`
///   (test262 `*/non-definable-global-{function,generator}`).
///
/// Depends on `globalThis`/`Object`; the caller bails the whole rewrite if the
/// body rebinds either name.
fn synth_create_global_fn_binding(name: &str, ident: &str) -> Option<ast::Stmt> {
    parse_single_stmt(&format!(
        "{{ let __perry_d = Object.getOwnPropertyDescriptor(globalThis, {name:?}); \
         if (__perry_d === void 0) \
         {{ if (!Object.isExtensible(globalThis)) \
              {{ throw new TypeError(\"Cannot declare global function: {name}\"); }} \
            void Object.defineProperty(globalThis, {name:?}, \
               {{ value: {ident}, writable: true, enumerable: true, configurable: true }}); }} \
         else if (__perry_d.configurable) \
         {{ void Object.defineProperty(globalThis, {name:?}, \
              {{ value: {ident}, writable: true, enumerable: true, configurable: true }}); }} \
         else if (!__perry_d.writable || !__perry_d.enumerable) \
         {{ throw new TypeError(\"Cannot declare global function: {name}\"); }} \
         else {{ void Object.defineProperty(globalThis, {name:?}, {{ value: {ident} }}); }} }}"
    ))
}

/// `if (!({}).hasOwnProperty.call(globalThis, "<name>")) { ... }` — the "create
/// the global `var` binding, initialized to `undefined`, if it does not already
/// exist" step. Implements `CanDeclareGlobalVar` (ECMA-262 §8.1.1.4.14):
///
/// - Property already exists → true, no pre-init needed (caller skips).
/// - Property absent and global is extensible → create via `globalThis["<name>"]
///   = void 0`.
/// - Property absent and global is **not** extensible → `CanDeclareGlobalVar`
///   returns false → throw TypeError (test262 `*/non-definable-global-var`).
///
/// `({}).hasOwnProperty` avoids depending on the user-shadowable `Object` name;
/// the `Object.isExtensible` extensibility check does use `Object`, which the
/// caller guards against rebinding in the eval body. The receiver stays the bare
/// `globalThis` (not `this`): the completion IIFE is an arrow whose `this` is the
/// caller's, not the global object. The assignment also targets `globalThis`
/// explicitly so a same-named top-level function in the IIFE is never clobbered.
fn synth_create_if_absent_stmt(name: &str) -> Option<ast::Stmt> {
    // Use Object.defineProperty instead of a plain assignment so inherited
    // prototype setters (e.g. from Object.prototype) cannot intercept the
    // binding creation — matching CreateGlobalVarBinding step 5a which calls
    // OrdinaryDefineOwnProperty directly on the global object. This module
    // only rewrites *eval* bodies (Annex B.3.3.3 EvalDeclarationInstantiation),
    // which always calls CreateGlobalVarBinding(vn, /* D = */ true) — unlike a
    // top-level Script's own GlobalDeclarationInstantiation, which passes
    // D = false. So the created binding must be configurable: true (test262
    // `language/eval-code/*/var-env-var-init-global-new`, `annexB/language/
    // eval-code/*/global-*-eval-global-init`).
    parse_single_stmt(&format!(
        "if (!({{}}).hasOwnProperty.call(globalThis, {name:?})) \
         {{ if (!Object.isExtensible(globalThis)) \
              {{ throw new TypeError(\"Cannot declare global var: {name}\"); }} \
            void Object.defineProperty(globalThis, {name:?}, \
              {{ value: void 0, writable: true, enumerable: true, configurable: true }}); }}"
    ))
}

/// Rename a hoisted block function's self-references inside its own body from
/// `from` to `to`, so an inner read/reassignment (`f`, `f = 123`) targets the
/// renamed block-scoped binding rather than the now-published global var of the
/// same name. Without this, BlockDeclarationInstantiation's block-scoping
/// invariant breaks — the block binding and the outer var binding must stay
/// independent (test262 `*-eval-global-block-scoping`).
///
/// Recursion stops at nested function / arrow / class boundaries: those open a
/// new scope, and a same-named declaration there shadows the function name (a
/// nested reference to the *outer* block function is rare and, left unrenamed,
/// degrades to the published global value — the pre-existing behavior). Forms
/// not walked are likewise left unchanged (never worse than not renaming).
fn rename_ident_in_block(block: &mut ast::BlockStmt, from: &str, to: &str) {
    for stmt in &mut block.stmts {
        rename_ident_in_stmt(stmt, from, to);
    }
}

fn rename_ident_in_stmt(stmt: &mut ast::Stmt, from: &str, to: &str) {
    use ast::Stmt;
    match stmt {
        Stmt::Expr(e) => rename_ident_in_expr(&mut e.expr, from, to),
        Stmt::Return(r) => {
            if let Some(a) = r.arg.as_mut() {
                rename_ident_in_expr(a, from, to);
            }
        }
        Stmt::Throw(t) => rename_ident_in_expr(&mut t.arg, from, to),
        Stmt::Block(b) => rename_ident_in_block(b, from, to),
        Stmt::If(i) => {
            rename_ident_in_expr(&mut i.test, from, to);
            rename_ident_in_stmt(&mut i.cons, from, to);
            if let Some(alt) = i.alt.as_mut() {
                rename_ident_in_stmt(alt, from, to);
            }
        }
        Stmt::While(w) => {
            rename_ident_in_expr(&mut w.test, from, to);
            rename_ident_in_stmt(&mut w.body, from, to);
        }
        Stmt::DoWhile(d) => {
            rename_ident_in_expr(&mut d.test, from, to);
            rename_ident_in_stmt(&mut d.body, from, to);
        }
        Stmt::Switch(s) => {
            rename_ident_in_expr(&mut s.discriminant, from, to);
            for case in &mut s.cases {
                if let Some(t) = case.test.as_mut() {
                    rename_ident_in_expr(t, from, to);
                }
                for st in &mut case.cons {
                    rename_ident_in_stmt(st, from, to);
                }
            }
        }
        Stmt::Decl(ast::Decl::Var(v)) => {
            for d in &mut v.decls {
                if let Some(init) = d.init.as_mut() {
                    rename_ident_in_expr(init, from, to);
                }
            }
        }
        Stmt::Labeled(l) => rename_ident_in_stmt(&mut l.body, from, to),
        Stmt::Try(t) => {
            rename_ident_in_block(&mut t.block, from, to);
            if let Some(h) = t.handler.as_mut() {
                // A `catch (from)` parameter shadows the function name in the
                // handler — its references are the catch binding, not ours.
                let mut p = std::collections::HashSet::new();
                if let Some(param) = &h.param {
                    collect_pattern_names(param, &mut p);
                }
                if !p.contains(from) {
                    rename_ident_in_block(&mut h.body, from, to);
                }
            }
            if let Some(f) = t.finalizer.as_mut() {
                rename_ident_in_block(f, from, to);
            }
        }
        Stmt::For(s) => {
            // A `for (let/var from …)` head rebinds the name; its test / update /
            // body references belong to that binding, so skip the whole loop.
            let head_binds = matches!(
                &s.init,
                Some(ast::VarDeclOrExpr::VarDecl(v)) if var_decl_binds(v, from)
            );
            if !head_binds {
                match s.init.as_mut() {
                    Some(ast::VarDeclOrExpr::Expr(e)) => rename_ident_in_expr(e, from, to),
                    Some(ast::VarDeclOrExpr::VarDecl(v)) => {
                        for d in &mut v.decls {
                            if let Some(i) = d.init.as_mut() {
                                rename_ident_in_expr(i, from, to);
                            }
                        }
                    }
                    None => {}
                }
                if let Some(t) = s.test.as_mut() {
                    rename_ident_in_expr(t, from, to);
                }
                if let Some(u) = s.update.as_mut() {
                    rename_ident_in_expr(u, from, to);
                }
                rename_ident_in_stmt(&mut s.body, from, to);
            }
        }
        Stmt::ForIn(s) => {
            if !for_head_binds(&s.left, from) {
                rename_ident_in_expr(&mut s.right, from, to);
                rename_ident_in_stmt(&mut s.body, from, to);
            }
        }
        Stmt::ForOf(s) => {
            if !for_head_binds(&s.left, from) {
                rename_ident_in_expr(&mut s.right, from, to);
                rename_ident_in_stmt(&mut s.body, from, to);
            }
        }
        // Remaining forms (`with`, empty, debugger, break/continue) have no
        // renamable self-reference, or open a dynamic scope we don't model.
        _ => {}
    }
}

/// Does a `var`/`let`/`const` declaration bind `name` (so it shadows an
/// outer same-named function binding within its scope)?
fn var_decl_binds(decl: &ast::VarDecl, name: &str) -> bool {
    let mut names = std::collections::HashSet::new();
    for d in &decl.decls {
        collect_pattern_names(&d.name, &mut names);
    }
    names.contains(name)
}

/// Does a `for-in` / `for-of` head (`for (let x …)`) bind `name`?
fn for_head_binds(head: &ast::ForHead, name: &str) -> bool {
    matches!(head, ast::ForHead::VarDecl(v) if var_decl_binds(v, name))
}

fn rename_ident_in_expr(expr: &mut ast::Expr, from: &str, to: &str) {
    use ast::Expr;
    match expr {
        Expr::Ident(id) => {
            if id.sym.as_ref() == from {
                id.sym = to.into();
            }
        }
        Expr::Assign(a) => {
            if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(b)) = &mut a.left {
                if b.id.sym.as_ref() == from {
                    b.id.sym = to.into();
                }
            }
            rename_ident_in_expr(&mut a.right, from, to);
        }
        Expr::Bin(b) => {
            rename_ident_in_expr(&mut b.left, from, to);
            rename_ident_in_expr(&mut b.right, from, to);
        }
        Expr::Unary(u) => rename_ident_in_expr(&mut u.arg, from, to),
        Expr::Update(u) => rename_ident_in_expr(&mut u.arg, from, to),
        Expr::Paren(p) => rename_ident_in_expr(&mut p.expr, from, to),
        Expr::Cond(c) => {
            rename_ident_in_expr(&mut c.test, from, to);
            rename_ident_in_expr(&mut c.cons, from, to);
            rename_ident_in_expr(&mut c.alt, from, to);
        }
        Expr::Seq(s) => {
            for e in &mut s.exprs {
                rename_ident_in_expr(e, from, to);
            }
        }
        Expr::Call(c) => {
            if let ast::Callee::Expr(callee) = &mut c.callee {
                rename_ident_in_expr(callee, from, to);
            }
            for a in &mut c.args {
                rename_ident_in_expr(&mut a.expr, from, to);
            }
        }
        Expr::New(n) => {
            rename_ident_in_expr(&mut n.callee, from, to);
            if let Some(args) = n.args.as_mut() {
                for a in args {
                    rename_ident_in_expr(&mut a.expr, from, to);
                }
            }
        }
        Expr::Member(m) => {
            rename_ident_in_expr(&mut m.obj, from, to);
            // Only a computed property (`o[f]`) is an identifier *reference*;
            // a static `.f` is a property name and must not be renamed.
            if let ast::MemberProp::Computed(c) = &mut m.prop {
                rename_ident_in_expr(&mut c.expr, from, to);
            }
        }
        Expr::Array(a) => {
            for elem in a.elems.iter_mut().flatten() {
                rename_ident_in_expr(&mut elem.expr, from, to);
            }
        }
        Expr::Object(o) => {
            for prop in &mut o.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_mut() {
                        rename_ident_in_expr(&mut kv.value, from, to);
                    }
                }
            }
        }
        Expr::Await(a) => rename_ident_in_expr(&mut a.arg, from, to),
        Expr::Tpl(t) => {
            for e in &mut t.exprs {
                rename_ident_in_expr(e, from, to);
            }
        }
        // Nested function / arrow / class open a new scope where the name may be
        // re-bound; do not rename across that boundary. Other leaf forms have no
        // identifier reference to rewrite.
        _ => {}
    }
}

/// Lexical (`let`/`const`/`class`) binding names declared *directly* in a
/// statement list (one block scope) — a block-scoped function declaration whose
/// name collides with one of these in an enclosing scope is *not* legacy-hoisted
/// (B.3.3.3 "would not produce any Early Errors": a `var` replacement would
/// clash with the lexical binding), so it stays a plain block declaration inside
/// the IIFE and never reaches the global variable environment.
fn block_lexical_names(stmts: &[ast::Stmt], out: &mut std::collections::HashSet<String>) {
    for stmt in stmts {
        if let ast::Stmt::Decl(decl) = stmt {
            match decl {
                ast::Decl::Var(v) if v.kind != ast::VarDeclKind::Var => {
                    for d in &v.decls {
                        collect_pattern_names(&d.name, out);
                    }
                }
                ast::Decl::Class(c) => {
                    out.insert(c.ident.sym.to_string());
                }
                _ => {}
            }
        }
    }
}

/// Does a statement list bind `name` *at function scope* — a top-level
/// `function`/`let`/`const`/`class`/`var` declaration, or a `var` hoisted out of
/// any nested block/loop/`try`/`switch`? (Block-scoped `let`/`const`/`catch`/
/// `for`-head bindings in *nested* scopes don't reach function scope and are not
/// counted.) Used to (1) bail the whole hoist when the eval body rebinds
/// `globalThis` — the prelude reads/writes that receiver and a shadow/TDZ would
/// break it — and (2) skip a function's self-rename when its body rebinds the
/// function name (`function f(){ var f; return f; }` reads the inner `var f`,
/// not the renamed block binding).
fn binds_at_function_scope(stmts: &[ast::Stmt], name: &str) -> bool {
    stmts.iter().any(|s| top_level_binds(s, name)) || var_hoisted_binds(stmts, name)
}

fn top_level_binds(stmt: &ast::Stmt, name: &str) -> bool {
    let ast::Stmt::Decl(decl) = stmt else {
        return false;
    };
    match decl {
        ast::Decl::Fn(f) => f.ident.sym.as_ref() == name,
        ast::Decl::Class(c) => c.ident.sym.as_ref() == name,
        ast::Decl::Var(v) => {
            let mut names = std::collections::HashSet::new();
            for d in &v.decls {
                collect_pattern_names(&d.name, &mut names);
            }
            names.contains(name)
        }
        _ => false,
    }
}

/// Recursively scan for a `var` declaration binding `name` — `var` hoists out of
/// every nested block/`if`/loop/`try`/`switch`/labeled statement to function
/// scope.
fn var_hoisted_binds(stmts: &[ast::Stmt], name: &str) -> bool {
    fn stmt_has(stmt: &ast::Stmt, name: &str) -> bool {
        use ast::Stmt;
        match stmt {
            Stmt::Decl(ast::Decl::Var(v)) if v.kind == ast::VarDeclKind::Var => {
                let mut names = std::collections::HashSet::new();
                for d in &v.decls {
                    collect_pattern_names(&d.name, &mut names);
                }
                names.contains(name)
            }
            Stmt::Block(b) => var_hoisted_binds(&b.stmts, name),
            Stmt::Labeled(l) => stmt_has(&l.body, name),
            Stmt::If(i) => {
                stmt_has(&i.cons, name) || i.alt.as_deref().is_some_and(|a| stmt_has(a, name))
            }
            Stmt::While(w) => stmt_has(&w.body, name),
            Stmt::DoWhile(d) => stmt_has(&d.body, name),
            Stmt::With(w) => stmt_has(&w.body, name),
            Stmt::For(f) => {
                matches!(&f.init, Some(ast::VarDeclOrExpr::VarDecl(v)) if v.kind == ast::VarDeclKind::Var && {
                    let mut n = std::collections::HashSet::new();
                    for d in &v.decls { collect_pattern_names(&d.name, &mut n); }
                    n.contains(name)
                }) || stmt_has(&f.body, name)
            }
            Stmt::ForIn(f) => for_head_var(&f.left, name) || stmt_has(&f.body, name),
            Stmt::ForOf(f) => for_head_var(&f.left, name) || stmt_has(&f.body, name),
            Stmt::Try(t) => {
                var_hoisted_binds(&t.block.stmts, name)
                    || t.handler
                        .as_ref()
                        .is_some_and(|h| var_hoisted_binds(&h.body.stmts, name))
                    || t.finalizer
                        .as_ref()
                        .is_some_and(|f| var_hoisted_binds(&f.stmts, name))
            }
            Stmt::Switch(s) => s.cases.iter().any(|c| var_hoisted_binds(&c.cons, name)),
            _ => false,
        }
    }
    stmts.iter().any(|s| stmt_has(s, name))
}

fn for_head_var(head: &ast::ForHead, name: &str) -> bool {
    matches!(head, ast::ForHead::VarDecl(v) if v.kind == ast::VarDeclKind::Var && {
        let mut n = std::collections::HashSet::new();
        for d in &v.decls { collect_pattern_names(&d.name, &mut n); }
        n.contains(name)
    })
}

/// Collect the identifier names bound by a binding pattern (a `let`/`const`
/// declarator target, a `catch` parameter, or a `for` head) — covers plain
/// idents, array/object destructuring, defaults, and rest elements.
fn collect_pattern_names(pat: &ast::Pat, out: &mut std::collections::HashSet<String>) {
    match pat {
        ast::Pat::Ident(b) => {
            out.insert(b.id.sym.to_string());
        }
        ast::Pat::Array(a) => {
            for elem in a.elems.iter().flatten() {
                collect_pattern_names(elem, out);
            }
        }
        ast::Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => collect_pattern_names(&kv.value, out),
                    ast::ObjectPatProp::Assign(a) => {
                        out.insert(a.key.id.sym.to_string());
                    }
                    ast::ObjectPatProp::Rest(r) => collect_pattern_names(&r.arg, out),
                }
            }
        }
        ast::Pat::Assign(a) => collect_pattern_names(&a.left, out),
        ast::Pat::Rest(r) => collect_pattern_names(&r.arg, out),
        _ => {}
    }
}

/// Which variable environment a sloppy eval body's var-scoped declarations
/// bind into — the two Annex B.3.3.3 cases Perry models by rewriting the body
/// before the completion-IIFE fold.
enum HoistMode {
    /// Sloppy *global* eval (module-top / global script): `var`/`function`
    /// declarations bind in the global variable environment. Both top-level and
    /// block/`if`/`switch`-nested declarations are published (with a
    /// create-if-absent `globalThis` prelude); a `class` aborts the rewrite.
    Global,
    /// Sloppy direct eval *inside a function*: only a block/`if`/`switch`-nested
    /// function declaration whose name already binds in the enclosing function
    /// variable environment (a parameter or outer `var`) is republished to it —
    /// the completion IIFE otherwise traps it as an arrow-local that wrongly
    /// shadows the pre-existing binding (test262 annexB `.../func-*-eval-func-
    /// no-skip-param`). No `globalThis` prelude is emitted, and top-level
    /// declarations / `var`s / a `class` are left exactly as the unmodified fold
    /// produced them — their EvalDeclarationInstantiation is already modeled.
    Function {
        bound: std::collections::HashSet<String>,
    },
}

/// In-progress state for the global-eval var-scoped rewrite.
struct GlobalEvalHoist {
    /// Which variable environment the var-scoped declarations bind into.
    mode: HoistMode,
    /// Function-scope mode: set when at least one nested function was
    /// republished, so the caller keeps the rewrite (vs. an untouched body).
    hoisted_any: bool,
    /// Unique-suffix counter for renamed hidden function bindings.
    counter: usize,
    /// Names needing a create-if-absent prelude (block/`if`/`switch`-nested
    /// function declarations — initialized to `undefined` at instantiation,
    /// assigned when the declaration is reached).
    prelude_names: Vec<String>,
    /// Top-level `var` names — CreateGlobalVarBinding: a create-if-absent
    /// prelude slot (initialized to `undefined`, not reinitialized if the global
    /// already exists), with each `var x = init` rewritten in place to a
    /// `void (x = init)` global publish (the `void` keeps the statement's empty
    /// completion value). (test262 `language/eval-code/*/var-env-var-*`.)
    var_prelude_names: Vec<String>,
    /// Top-level `function` declarations — CreateGlobalFunctionBinding: the
    /// function value is present at instantiation, so each is renamed to a hidden
    /// binding and published with a `void (f = <hidden>)` at the top of the body
    /// (recorded as `(orig, hidden)`). (test262 `*/var-env-func-*`.)
    top_fn_publishes: Vec<(String, String)>,
    /// Enclosing lexical (`let`/`const`/`class`/`catch`/`for`-head) names — a
    /// nested function whose name collides with one is an early-error skip
    /// (B.3.3.3) and must not be hoisted. Maintained as a scope stack by
    /// `rewrite_list` / `with_lexical_scope`.
    lexical: std::collections::HashSet<String>,
    /// Cleared to `false` on any construct the rewrite can't safely model, so
    /// the caller falls back to the unmodified fold.
    ok: bool,
}

impl GlobalEvalHoist {
    fn fresh_hidden(&mut self) -> String {
        let h = format!("__perry_ev_fn_{}", self.counter);
        self.counter += 1;
        h
    }

    /// Run `body` with `names` added to the enclosing lexical set, restoring it
    /// afterward — used when descending into a scope that binds those names
    /// (a block's `let`/`const`/`class`, a `catch` parameter, a `for` head).
    fn with_lexical_scope(
        &mut self,
        names: std::collections::HashSet<String>,
        body: impl FnOnce(&mut Self),
    ) {
        let added: Vec<String> = names
            .into_iter()
            .filter(|n| self.lexical.insert(n.clone()))
            .collect();
        body(self);
        for n in added {
            self.lexical.remove(&n);
        }
    }

    /// Rewrite one statement list. `top_level` distinguishes the eval body's own
    /// top level (function declarations are var-scoped with their value present
    /// at instantiation) from a nested block / branch / case (function
    /// declarations are legacy-hoisted: `undefined` at instantiation, assigned
    /// when reached). The eval body's own lexical bindings at this block level
    /// are added to the enclosing lexical set first, so a same-named function in
    /// a deeper block is recognized as an early-error skip (B.3.3.3) and left
    /// unhoisted.
    fn rewrite_list(&mut self, stmts: &mut Vec<ast::Stmt>, top_level: bool) {
        let mut block_lex = std::collections::HashSet::new();
        block_lexical_names(stmts, &mut block_lex);
        let added: Vec<String> = block_lex
            .into_iter()
            .filter(|n| self.lexical.insert(n.clone()))
            .collect();
        self.rewrite_list_inner(stmts, top_level);
        for n in added {
            self.lexical.remove(&n);
        }
    }

    fn rewrite_list_inner(&mut self, stmts: &mut Vec<ast::Stmt>, top_level: bool) {
        let mut out: Vec<ast::Stmt> = Vec::with_capacity(stmts.len());
        for mut stmt in stmts.drain(..) {
            if !self.ok {
                out.push(stmt);
                continue;
            }
            match &mut stmt {
                ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) if fn_decl.function.body.is_some() => {
                    let orig = fn_decl.ident.sym.to_string();
                    // A function colliding with an enclosing lexical name is an
                    // early-error skip (B.3.3.3) — leave it in the IIFE.
                    if self.lexical.contains(&orig) {
                        out.push(stmt);
                        continue;
                    }
                    // Function-scope mode only republishes a *nested* function
                    // whose name already binds in the enclosing function variable
                    // environment; a top-level function, or any nested function
                    // whose name is new, is left exactly as the unmodified fold
                    // produced it (the IIFE already models its binding).
                    if let HoistMode::Function { bound } = &self.mode {
                        if top_level || !bound.contains(&orig) {
                            out.push(stmt);
                            continue;
                        }
                    } else if top_level {
                        // Global mode — a *top-level* function is
                        // CreateGlobalFunctionBinding: its value is present at
                        // instantiation. Rename it to a hidden binding and publish
                        // `void (orig = hidden)` at the top of the body (assembled
                        // in `apply_global_eval_hoist`); its own `orig` self-
                        // references need no rename — with no local `orig` left,
                        // they resolve to the published global. (test262
                        // `*/var-env-func-*`.)
                        let hidden = self.fresh_hidden();
                        fn_decl.ident.sym = hidden.as_str().into();
                        self.top_fn_publishes.push((orig, hidden));
                        out.push(stmt);
                        continue;
                    }
                    // A *nested* (block / `if` / `switch`-case) function gets the
                    // B.3.3.3 legacy hoist below (`undefined` at instantiation in
                    // global mode, value published when reached in both modes).
                    // Rename the declaration to a fresh hidden name so the value-
                    // transfer assignment `orig = hidden` resolves `orig` to the
                    // *enclosing* (global) variable environment rather than this
                    // block's own binding — and rename the function's self-
                    // references in its body too, so the block-scoped binding
                    // stays independent of the published global var.
                    let hidden = self.fresh_hidden();
                    // A parameter named `orig`, or a body-level `var orig` /
                    // top-level `let`/`const`/`function orig`, shadows the
                    // function-name binding throughout the body — its `orig`
                    // references are that inner binding, so don't rename them.
                    let mut param_names = std::collections::HashSet::new();
                    for p in &fn_decl.function.params {
                        collect_pattern_names(&p.pat, &mut param_names);
                    }
                    let body_shadows = param_names.contains(&orig)
                        || fn_decl
                            .function
                            .body
                            .as_ref()
                            .is_some_and(|b| binds_at_function_scope(&b.stmts, &orig));
                    fn_decl.ident.sym = hidden.as_str().into();
                    if !body_shadows {
                        if let Some(body) = fn_decl.function.body.as_mut() {
                            rename_ident_in_block(body, &orig, &hidden);
                        }
                    }
                    let Some(assign) = synth_ident_assign_stmt(&orig, &hidden) else {
                        self.ok = false;
                        out.push(stmt);
                        continue;
                    };
                    // Legacy block hoisting: the function value published when
                    // reached. Global mode also pre-creates the binding as
                    // `undefined` at instantiation (prelude); function-scope mode
                    // reuses the pre-existing enclosing binding, so no prelude.
                    out.push(stmt);
                    out.push(assign);
                    match &self.mode {
                        HoistMode::Global => self.prelude_names.push(orig),
                        HoistMode::Function { .. } => self.hoisted_any = true,
                    }
                }
                // A top-level `var` is CreateGlobalVarBinding: pre-create each
                // name (`undefined`, not reinitialized if it already exists) via
                // the prelude and rewrite `var x = init` to a completion-inert
                // `{ let <hidden> = (x = init); }` global publish (the lexical
                // declaration keeps the VariableStatement's empty completion
                // value). A non-simple declarator (destructuring) the rewrite
                // can't model bails the whole fold. (test262 `*/var-env-var-*`.)
                // A *nested* `var` and all `let`/`const` stay put — the IIFE
                // models the eval's own variable / lexical env.
                ast::Stmt::Decl(ast::Decl::Var(var_decl))
                    if top_level
                        && var_decl.kind == ast::VarDeclKind::Var
                        && matches!(&self.mode, HoistMode::Global) =>
                {
                    let mut publishes: Vec<ast::Stmt> = Vec::new();
                    for d in &var_decl.decls {
                        let ast::Pat::Ident(binding) = &d.name else {
                            self.ok = false;
                            break;
                        };
                        let name = binding.id.sym.to_string();
                        self.var_prelude_names.push(name.clone());
                        if let Some(init) = &d.init {
                            // A fresh hidden sink per publish: the lexical binding
                            // is in TDZ while `init` evaluates, so a name the
                            // user's `init` could reference would throw spuriously.
                            let sink = self.fresh_hidden();
                            match synth_inert_assign_stmt(&sink, &name, init.clone()) {
                                Some(s) => publishes.push(s),
                                None => {
                                    self.ok = false;
                                    break;
                                }
                            }
                        }
                    }
                    if self.ok {
                        out.extend(publishes);
                    } else {
                        out.push(stmt);
                    }
                }
                // A `class` would leak to module scope when lowered in the IIFE;
                // `let` / `const` stay put — the IIFE already models the eval's
                // own lexical environment for them.
                ast::Stmt::Decl(ast::Decl::Class(_)) => {
                    // Global mode bails (a `class` lowered in the IIFE would leak
                    // to module scope). Function-scope mode never rewrites a
                    // `class`, so it just leaves it in place.
                    if matches!(&self.mode, HoistMode::Global) {
                        self.ok = false;
                    }
                    out.push(stmt);
                }
                ast::Stmt::Decl(_) => out.push(stmt),
                ast::Stmt::Block(b) => {
                    self.rewrite_list(&mut b.stmts, false);
                    out.push(stmt);
                }
                ast::Stmt::If(i) => {
                    self.rewrite_single(&mut i.cons);
                    if let Some(alt) = i.alt.as_mut() {
                        self.rewrite_single(alt);
                    }
                    out.push(stmt);
                }
                ast::Stmt::Switch(s) => {
                    // A `switch` body is one lexical block — collect every
                    // case's `let`/`const`/`class` before rewriting any case.
                    let mut switch_lex = std::collections::HashSet::new();
                    for case in &s.cases {
                        block_lexical_names(&case.cons, &mut switch_lex);
                    }
                    let cases = &mut s.cases;
                    self.with_lexical_scope(switch_lex, |me| {
                        for case in cases.iter_mut() {
                            me.rewrite_list_inner(&mut case.cons, false);
                        }
                    });
                    out.push(stmt);
                }
                ast::Stmt::Labeled(l) => {
                    self.rewrite_single(&mut l.body);
                    out.push(stmt);
                }
                ast::Stmt::Try(t) => {
                    self.rewrite_list(&mut t.block.stmts, false);
                    if let Some(h) = t.handler.as_mut() {
                        // The `catch` parameter is lexically bound in the handler
                        // body — a same-named function inside it is an early-error
                        // skip (B.3.3.3).
                        let mut catch_lex = std::collections::HashSet::new();
                        if let Some(param) = &h.param {
                            collect_pattern_names(param, &mut catch_lex);
                        }
                        let body = &mut h.body.stmts;
                        self.with_lexical_scope(catch_lex, |me| me.rewrite_list(body, false));
                    }
                    if let Some(f) = t.finalizer.as_mut() {
                        self.rewrite_list(&mut f.stmts, false);
                    }
                    out.push(stmt);
                }
                // A `var`/`function` in a loop header or `with` head is rare in
                // practice and awkward to relocate safely — bail rather than
                // mis-hoist. A loop/`with` body with no own declaration is fine.
                ast::Stmt::For(f) if !matches!(f.init, Some(ast::VarDeclOrExpr::VarDecl(_))) => {
                    self.rewrite_single(&mut f.body);
                    out.push(stmt);
                }
                ast::Stmt::ForIn(f) if !matches!(f.left, ast::ForHead::VarDecl(_)) => {
                    self.rewrite_single(&mut f.body);
                    out.push(stmt);
                }
                ast::Stmt::ForOf(f) if !matches!(f.left, ast::ForHead::VarDecl(_)) => {
                    self.rewrite_single(&mut f.body);
                    out.push(stmt);
                }
                ast::Stmt::While(w) => {
                    self.rewrite_single(&mut w.body);
                    out.push(stmt);
                }
                ast::Stmt::DoWhile(d) => {
                    self.rewrite_single(&mut d.body);
                    out.push(stmt);
                }
                ast::Stmt::For(_)
                | ast::Stmt::ForIn(_)
                | ast::Stmt::ForOf(_)
                | ast::Stmt::With(_) => {
                    self.ok = false;
                    out.push(stmt);
                }
                _ => out.push(stmt),
            }
        }
        *stmts = out;
    }

    /// Rewrite a single nested statement (an `if` branch / labeled / loop body),
    /// re-wrapping in a block if the rewrite expanded it.
    fn rewrite_single(&mut self, stmt: &mut Box<ast::Stmt>) {
        let placeholder = ast::Stmt::Empty(ast::EmptyStmt {
            span: swc_common::DUMMY_SP,
        });
        let mut list = vec![std::mem::replace(stmt.as_mut(), placeholder)];
        self.rewrite_list(&mut list, false);
        if list.len() == 1 {
            **stmt = list.pop().unwrap();
        } else {
            **stmt = ast::Stmt::Block(ast::BlockStmt {
                span: swc_common::DUMMY_SP,
                ctxt: Default::default(),
                stmts: list,
            });
        }
    }
}

/// Rewrite a sloppy *global* eval body so its var-scoped (`var`/`function`)
/// declarations bind in the global variable environment rather than the
/// completion IIFE. Returns `Some(stmts)` (prelude + body) when at least one
/// var-scoped declaration was hoisted and the whole body was modeled safely;
/// `None` (nothing to hoist, or an unmodelable construct) leaves the caller on
/// the unmodified fold. Operates on a clone, so a mid-way bail never leaves a
/// partially rewritten body.
pub(super) fn apply_global_eval_hoist(stmts: &[ast::Stmt]) -> Option<Vec<ast::Stmt>> {
    // The prelude / publishes read `globalThis` and `Object` (the
    // create-if-absent slot and CreateGlobalFunctionBinding); if the eval body
    // rebinds either name at function scope (`var globalThis`, top-level
    // `let`/`function Object`, …), the prelude — prepended into the same IIFE —
    // would hit the shadow or its TDZ. Bail so the runtime fold preserves
    // semantics for that (pathological) case.
    if binds_at_function_scope(stmts, "globalThis") || binds_at_function_scope(stmts, "Object") {
        return None;
    }
    let mut hoist = GlobalEvalHoist {
        mode: HoistMode::Global,
        hoisted_any: false,
        counter: 0,
        prelude_names: Vec::new(),
        var_prelude_names: Vec::new(),
        top_fn_publishes: Vec::new(),
        // `rewrite_list` adds each block scope's lexical bindings as it descends,
        // starting from the eval body's own top level.
        lexical: std::collections::HashSet::new(),
        ok: true,
    };
    let mut body = stmts.to_vec();
    hoist.rewrite_list(&mut body, true);
    let nothing_to_hoist = hoist.prelude_names.is_empty()
        && hoist.var_prelude_names.is_empty()
        && hoist.top_fn_publishes.is_empty();
    if !hoist.ok || nothing_to_hoist {
        // Bailed, or no var-scoped declaration to publish (declaration-free, or
        // only `let`/`const`) — the caller keeps the unmodified fold.
        return None;
    }
    let mut result: Vec<ast::Stmt> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Create-if-absent slots (`undefined`) for nested block functions and
    // top-level `var`s — neither reinitializes an already-present global binding.
    for name in hoist
        .prelude_names
        .iter()
        .chain(hoist.var_prelude_names.iter())
    {
        if seen.insert(name.clone()) {
            result.push(synth_create_if_absent_stmt(name)?);
        }
    }
    // Top-level functions are published (CreateGlobalFunctionBinding) with their
    // value at instantiation, after the create-if-absent slots and before the
    // body — the renamed function declarations hoist to the top of the IIFE
    // arrow, so the value is ready.
    for (orig, hidden) in &hoist.top_fn_publishes {
        result.push(synth_create_global_fn_binding(orig, hidden)?);
    }
    result.append(&mut body);
    Some(result)
}

/// Rewrite a sloppy direct-eval body running *inside a function* (Annex B.3.3.3,
/// function variable environment) so each block/`if`/`switch`-nested function
/// declaration whose name is in `bound` — i.e. already binds in the enclosing
/// function (a parameter or outer `var`) — is republished to that binding rather
/// than trapped as an arrow-local of the completion IIFE. The declaration is
/// renamed to a hidden binding (its self-references too) and a bare `name =
/// hidden` assignment is emitted where it is reached, so the pre-existing value
/// is read before and the function value after (test262 annexB
/// `.../func-*-eval-func-no-skip-param`). Top-level declarations, `var`s, and a
/// `class` are left untouched — the IIFE already models them, and a name *not*
/// already bound in the enclosing scope must keep the IIFE's fresh binding (no
/// bare assignment, which would otherwise leak a sloppy global). Returns
/// `Some(stmts)` only when at least one nested function was republished.
pub(super) fn apply_function_eval_hoist(
    stmts: &[ast::Stmt],
    bound: std::collections::HashSet<String>,
) -> Option<Vec<ast::Stmt>> {
    if bound.is_empty() {
        return None;
    }
    let mut hoist = GlobalEvalHoist {
        mode: HoistMode::Function { bound },
        hoisted_any: false,
        counter: 0,
        prelude_names: Vec::new(),
        var_prelude_names: Vec::new(),
        top_fn_publishes: Vec::new(),
        lexical: std::collections::HashSet::new(),
        ok: true,
    };
    let mut body = stmts.to_vec();
    hoist.rewrite_list(&mut body, true);
    if !hoist.ok || !hoist.hoisted_any {
        // Bailed on an unmodelable construct, or nothing was republished — the
        // caller keeps the unmodified fold.
        return None;
    }
    Some(body)
}

/// Collect the names of *nested* (block / `if` / `switch`-case / loop / `try` /
/// labeled) function declarations in an eval body. The caller intersects these
/// with the enclosing function's bindings to drive [`apply_function_eval_hoist`].
/// Top-level function declarations are excluded — their EvalDeclarationInstantiation
/// is already modeled by the completion IIFE.
pub(super) fn collect_nested_fn_decl_names(
    stmts: &[ast::Stmt],
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    collect_nested_fn_decl_names_inner(stmts, true, &mut out);
    out
}

fn collect_nested_fn_decl_names_inner(
    stmts: &[ast::Stmt],
    top_level: bool,
    out: &mut std::collections::HashSet<String>,
) {
    let single = |s: &ast::Stmt, out: &mut std::collections::HashSet<String>| {
        collect_nested_fn_decl_names_inner(std::slice::from_ref(s), false, out);
    };
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Fn(f)) if f.function.body.is_some() => {
                if !top_level {
                    out.insert(f.ident.sym.to_string());
                }
            }
            ast::Stmt::Block(b) => collect_nested_fn_decl_names_inner(&b.stmts, false, out),
            ast::Stmt::If(i) => {
                single(&i.cons, out);
                if let Some(alt) = &i.alt {
                    single(alt, out);
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    collect_nested_fn_decl_names_inner(&case.cons, false, out);
                }
            }
            ast::Stmt::Labeled(l) => single(&l.body, out),
            ast::Stmt::Try(t) => {
                collect_nested_fn_decl_names_inner(&t.block.stmts, false, out);
                if let Some(h) = &t.handler {
                    collect_nested_fn_decl_names_inner(&h.body.stmts, false, out);
                }
                if let Some(f) = &t.finalizer {
                    collect_nested_fn_decl_names_inner(&f.stmts, false, out);
                }
            }
            ast::Stmt::While(w) => single(&w.body, out),
            ast::Stmt::DoWhile(d) => single(&d.body, out),
            ast::Stmt::For(f) => single(&f.body, out),
            ast::Stmt::ForIn(f) => single(&f.body, out),
            ast::Stmt::ForOf(f) => single(&f.body, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod global_eval_hoist_tests {
    use super::{apply_function_eval_hoist, apply_global_eval_hoist, collect_nested_fn_decl_names};
    use swc_ecma_ast as ast;

    fn bound_set(names: &[&str]) -> std::collections::HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn parse_body(src: &str) -> Vec<ast::Stmt> {
        let module = perry_parser::parse_typescript(src, "<test>.cjs").expect("parse");
        module
            .body
            .into_iter()
            .filter_map(|item| match item {
                ast::ModuleItem::Stmt(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// Collect every identifier that appears as the simple target of an
    /// assignment statement (`x = …;`) anywhere in `stmts`.
    fn assign_targets(stmts: &[ast::Stmt]) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(stmt: &ast::Stmt, out: &mut Vec<String>) {
            match stmt {
                ast::Stmt::Expr(e) => {
                    if let ast::Expr::Assign(a) = e.expr.as_ref() {
                        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(b)) =
                            &a.left
                        {
                            out.push(b.id.sym.to_string());
                        }
                    }
                }
                ast::Stmt::Block(b) => b.stmts.iter().for_each(|s| walk(s, out)),
                ast::Stmt::If(i) => {
                    walk(&i.cons, out);
                    if let Some(alt) = &i.alt {
                        walk(alt, out);
                    }
                }
                _ => {}
            }
        }
        for s in stmts {
            walk(s, &mut out);
        }
        out
    }

    fn fn_decl_names(stmts: &[ast::Stmt]) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(stmt: &ast::Stmt, out: &mut Vec<String>) {
            match stmt {
                ast::Stmt::Decl(ast::Decl::Fn(f)) => out.push(f.ident.sym.to_string()),
                ast::Stmt::Block(b) => b.stmts.iter().for_each(|s| walk(s, out)),
                ast::Stmt::If(i) => {
                    walk(&i.cons, out);
                    if let Some(alt) = &i.alt {
                        walk(alt, out);
                    }
                }
                _ => {}
            }
        }
        for s in stmts {
            walk(s, &mut out);
        }
        out
    }

    #[test]
    fn block_function_is_hoisted_with_rename_and_prelude() {
        let body = parse_body("{ function f() { return 1; } }");
        let out = apply_global_eval_hoist(&body).expect("hoists a block function");
        // A leading create-if-absent prelude (`if (...) { f = void 0; }`).
        assert!(matches!(out.first(), Some(ast::Stmt::If(_))), "prelude if");
        // The block function was renamed to a hidden binding...
        let fns = fn_decl_names(&out);
        assert!(
            fns.iter().any(|n| n.starts_with("__perry_ev_fn_")),
            "renamed fn decl, got {fns:?}"
        );
        assert!(
            !fns.iter().any(|n| n == "f"),
            "no `f` decl remains: {fns:?}"
        );
        // ...and its value published to the global var name `f`.
        assert!(
            assign_targets(&out).iter().any(|t| t == "f"),
            "publishes f = <hidden>"
        );
    }

    #[test]
    fn self_reference_in_loop_body_is_renamed() {
        // A function self-reference inside a `for` head/body must be renamed
        // along with the declaration, so a later `f = …` writes the renamed
        // block binding, not the published global var.
        let body = parse_body("{ function f() { for (f = 1; false; ) {} return f; } }");
        let out = apply_global_eval_hoist(&body).expect("hoists");
        // No bare `f` reference may survive inside the renamed function body.
        fn idents(stmt: &ast::Stmt, out: &mut Vec<String>) {
            fn expr(e: &ast::Expr, out: &mut Vec<String>) {
                match e {
                    ast::Expr::Ident(i) => out.push(i.sym.to_string()),
                    ast::Expr::Assign(a) => {
                        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(b)) =
                            &a.left
                        {
                            out.push(b.id.sym.to_string());
                        }
                        expr(&a.right, out);
                    }
                    _ => {}
                }
            }
            match stmt {
                ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                    if let Some(b) = &f.function.body {
                        for s in &b.stmts {
                            idents(s, out);
                        }
                    }
                }
                ast::Stmt::Block(b) => b.stmts.iter().for_each(|s| idents(s, out)),
                ast::Stmt::Return(r) => {
                    if let Some(a) = &r.arg {
                        expr(a, out);
                    }
                }
                ast::Stmt::For(s) => {
                    if let Some(ast::VarDeclOrExpr::Expr(e)) = &s.init {
                        expr(e, out);
                    }
                    idents(&s.body, out);
                }
                _ => {}
            }
        }
        let mut names = Vec::new();
        for s in &out {
            idents(s, &mut names);
        }
        assert!(
            !names.iter().any(|n| n == "f"),
            "function body still references bare `f`: {names:?}"
        );
    }

    /// Names assigned by a top-level completion-inert `var` publish —
    /// `{ let __perry_eval_void = (name = …); }`. The assignment publishes to the
    /// (global) variable environment while the enclosing lexical declaration
    /// keeps the `var` statement's empty completion value.
    fn inert_publish_targets(stmts: &[ast::Stmt]) -> Vec<String> {
        let mut out = Vec::new();
        for s in stmts {
            let ast::Stmt::Block(b) = s else { continue };
            for inner in &b.stmts {
                let ast::Stmt::Decl(ast::Decl::Var(v)) = inner else {
                    continue;
                };
                for d in &v.decls {
                    let Some(init) = &d.init else { continue };
                    if let ast::Expr::Assign(a) = init.as_ref() {
                        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(bind)) =
                            &a.left
                        {
                            out.push(bind.id.sym.to_string());
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether any statement mentions an `Object.defineProperty(...)` call.
    fn mentions_define_property(stmts: &[ast::Stmt]) -> bool {
        fn ident_names(stmt: &ast::Stmt, out: &mut Vec<String>) {
            fn expr(e: &ast::Expr, out: &mut Vec<String>) {
                match e {
                    ast::Expr::Ident(i) => out.push(i.sym.to_string()),
                    ast::Expr::Member(m) => {
                        expr(&m.obj, out);
                        if let ast::MemberProp::Ident(i) = &m.prop {
                            out.push(i.sym.to_string());
                        }
                    }
                    ast::Expr::Call(c) => {
                        if let ast::Callee::Expr(e) = &c.callee {
                            expr(e, out);
                        }
                    }
                    ast::Expr::Cond(c) => {
                        expr(&c.test, out);
                        expr(&c.cons, out);
                        expr(&c.alt, out);
                    }
                    // The function publish wraps each `Object.defineProperty(...)`
                    // call in `void (...)` to keep an empty completion value.
                    ast::Expr::Unary(u) => expr(&u.arg, out),
                    _ => {}
                }
            }
            match stmt {
                ast::Stmt::Block(b) => b.stmts.iter().for_each(|s| ident_names(s, out)),
                ast::Stmt::If(i) => {
                    ident_names(&i.cons, out);
                    if let Some(a) = &i.alt {
                        ident_names(a, out);
                    }
                }
                ast::Stmt::Expr(e) => expr(&e.expr, out),
                ast::Stmt::Decl(ast::Decl::Var(v)) => {
                    for d in &v.decls {
                        if let Some(init) = &d.init {
                            expr(init, out);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut names = Vec::new();
        for s in stmts {
            ident_names(s, &mut names);
        }
        names.iter().any(|n| n == "defineProperty")
    }

    #[test]
    fn top_level_function_is_published_to_the_global() {
        // A *top-level* function is CreateGlobalFunctionBinding: renamed to a
        // hidden binding and published with its value at instantiation via an
        // `Object.defineProperty(globalThis, …)` block (empty completion value).
        // (test262 language/eval-code/*/var-env-func-init-global-new.)
        let out = apply_global_eval_hoist(&parse_body("initial = f; function f() { return 234; }"))
            .expect("publishes the top-level function");
        assert!(
            mentions_define_property(&out),
            "expected an Object.defineProperty publish of `f`"
        );
        // The original name no longer appears as a function *declaration*.
        let fns = fn_decl_names(&out);
        assert!(
            !fns.iter().any(|n| n == "f"),
            "`f` should be renamed: {fns:?}"
        );
        assert!(
            fns.iter().any(|n| n.starts_with("__perry_ev_fn_")),
            "renamed fn decl, got {fns:?}"
        );
    }

    #[test]
    fn top_level_var_is_published_to_the_global() {
        // A *top-level* `var` is CreateGlobalVarBinding: a create-if-absent slot
        // (`if (...) { globalThis[x] = void 0 }`) plus a completion-inert
        // `{ let __perry_eval_void = (x = init); }` publish (the lexical
        // declaration keeps the VariableStatement's empty completion value).
        let out = apply_global_eval_hoist(&parse_body("initial = x; var x = 9;"))
            .expect("publishes the top-level var");
        assert!(
            matches!(out.first(), Some(ast::Stmt::If(_))),
            "create-if-absent prelude"
        );
        assert!(
            inert_publish_targets(&out).iter().any(|t| t == "x"),
            "expected an inert publish of `x`"
        );
        // No top-level `var` declaration may remain (it was rewritten to the
        // publish block).
        assert!(
            !out.iter()
                .any(|s| matches!(s, ast::Stmt::Decl(ast::Decl::Var(_)))),
            "`var x` should be rewritten away"
        );
    }

    #[test]
    fn bare_top_level_var_creates_slot_only() {
        // `var x;` (no initializer) only needs the create-if-absent slot — no
        // publish assignment, and no surviving `var` declaration.
        let out = apply_global_eval_hoist(&parse_body("initial = x; var x;"))
            .expect("creates the global slot");
        assert!(
            matches!(out.first(), Some(ast::Stmt::If(_))),
            "create-if-absent prelude"
        );
        assert!(
            inert_publish_targets(&out).is_empty(),
            "no publish for a bare var"
        );
        assert!(
            !out.iter()
                .any(|s| matches!(s, ast::Stmt::Decl(ast::Decl::Var(_)))),
            "`var x` should be rewritten away"
        );
    }

    #[test]
    fn globalthis_rebind_declines_fold() {
        // The prelude reads/writes `globalThis`; if the body rebinds that name,
        // the hoist bails so the prelude can't hit the shadow / its TDZ.
        for src in [
            "var globalThis; { function f() {} }",
            "let globalThis; { function f() {} }",
            "function globalThis() {} { function f() {} }",
        ] {
            assert!(
                apply_global_eval_hoist(&parse_body(src)).is_none(),
                "should decline: {src}"
            );
        }
    }

    #[test]
    fn body_var_shadow_keeps_self_reference() {
        // `function f(){ var f; return f; }` — the body's `f` is the inner
        // `var f`, not the function-name binding, so it must NOT be renamed.
        let body = parse_body("{ function f() { var f; return f; } }");
        let out = apply_global_eval_hoist(&body).expect("hoists");
        fn return_ident(stmt: &ast::Stmt, out: &mut Vec<String>) {
            match stmt {
                ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                    if let Some(b) = &f.function.body {
                        for s in &b.stmts {
                            if let ast::Stmt::Return(r) = s {
                                if let Some(ast::Expr::Ident(i)) = r.arg.as_deref() {
                                    out.push(i.sym.to_string());
                                }
                            }
                        }
                    }
                }
                ast::Stmt::Block(b) => b.stmts.iter().for_each(|s| return_ident(s, out)),
                _ => {}
            }
        }
        let mut names = Vec::new();
        for s in &out {
            return_ident(s, &mut names);
        }
        assert!(
            names.iter().any(|n| n == "f"),
            "shadowed self-reference must stay `f`: {names:?}"
        );
    }

    #[test]
    fn lexical_conflict_skips_hoisting() {
        // Annex B.3.3.3 early-error skip: an enclosing `let f` blocks legacy
        // hoisting of the inner `function f`, so there is nothing var-scoped to
        // hoist and the fold is declined.
        let body = parse_body("{ let f = 1; { function f() {} } }");
        assert!(apply_global_eval_hoist(&body).is_none());
    }

    #[test]
    fn declaration_free_body_is_declined() {
        // No var-scoped declaration → the caller keeps the unmodified fold.
        let body = parse_body("globalThis.x = 1; foo();");
        assert!(apply_global_eval_hoist(&body).is_none());
    }

    #[test]
    fn class_declaration_declines_fold() {
        // A `class` would leak to module scope when lowered in the IIFE; bail so
        // the caller defers to the runtime path.
        let body = parse_body("var x = 1; class C {}");
        assert!(apply_global_eval_hoist(&body).is_none());
    }

    #[test]
    fn function_scope_bound_nested_function_is_republished() {
        // Annex B.3.3.3 (direct eval inside a function): a block function whose
        // name already binds in the enclosing function (`f`, in `bound`) is
        // renamed and republished to that binding with a bare assignment — no
        // `globalThis` create-if-absent prelude, so the pre-existing value is
        // read before the declaration and the function value after (test262
        // `.../func-block-decl-eval-func-no-skip-param`).
        let body = parse_body("init = f;{ function f() {} }after = f;");
        let out = apply_function_eval_hoist(&body, bound_set(&["f"])).expect("republishes f");
        assert!(
            !out.iter().any(|s| matches!(s, ast::Stmt::If(_))),
            "no globalThis prelude"
        );
        let fns = fn_decl_names(&out);
        assert!(
            fns.iter().any(|n| n.starts_with("__perry_ev_fn_")),
            "renamed fn decl, got {fns:?}"
        );
        assert!(
            !fns.iter().any(|n| n == "f"),
            "no `f` decl remains: {fns:?}"
        );
        assert!(
            assign_targets(&out).iter().any(|t| t == "f"),
            "publishes f = <hidden>"
        );
    }

    #[test]
    fn function_scope_unbound_nested_function_is_left_untouched() {
        // A nested function whose name is NOT bound in the enclosing scope keeps
        // the completion IIFE's fresh binding — republishing via a bare
        // assignment would leak a sloppy global (test262 `.../func-init`).
        let body = parse_body("init = f;{ function f() {} }after = f;");
        assert!(apply_function_eval_hoist(&body, bound_set(&[])).is_none());
    }

    #[test]
    fn function_scope_top_level_function_is_left_untouched() {
        // Function-scope mode never republishes a *top-level* eval function — the
        // completion IIFE already models its enclosing-function binding.
        let body = parse_body("function f() {}");
        assert!(apply_function_eval_hoist(&body, bound_set(&["f"])).is_none());
    }

    #[test]
    fn function_scope_lexical_conflict_is_skipped() {
        // An enclosing eval-body `let f` makes the inner `function f` an
        // early-error skip (B.3.3.3) even though `f` is in `bound` — nothing is
        // republished.
        let body = parse_body("{ let f = 1; { function f() {} } }");
        assert!(apply_function_eval_hoist(&body, bound_set(&["f"])).is_none());
    }

    #[test]
    fn nested_fn_decl_names_collects_only_nested() {
        // Top-level function declarations are excluded; block / `if` / `switch` /
        // `try` nested ones are collected.
        let names = collect_nested_fn_decl_names(&parse_body(
            "function top() {}\
             { function a() {} }\
             if (x) function b() {}\
             switch (y) { case 1: function c() {} }\
             try { function d() {} } catch (e) { function g() {} }",
        ));
        for n in ["a", "b", "c", "d", "g"] {
            assert!(names.contains(n), "missing nested {n}: {names:?}");
        }
        assert!(
            !names.contains("top"),
            "top-level must be excluded: {names:?}"
        );
    }
}
