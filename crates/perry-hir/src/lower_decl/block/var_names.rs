//! `var` / lexical binding-name collection and the Annex B B.3.3 scan.
//!
//! Moved verbatim out of `lower_decl/block.rs` (file-size gate); `block.rs`
//! re-exports the `pub(crate)` entry points so `crate::lower_decl::*` keeps
//! the exact same shape.

use swc_ecma_ast as ast;

pub(crate) fn collect_var_binding_names_from_pat(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(ident) => out.push(ident.id.sym.to_string()),
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_var_binding_names_from_pat(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => out.push(assign.key.sym.to_string()),
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_var_binding_names_from_pat(&kv.value, out)
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_var_binding_names_from_pat(&rest.arg, out)
                    }
                }
            }
        }
        ast::Pat::Assign(assign) => collect_var_binding_names_from_pat(&assign.left, out),
        ast::Pat::Rest(rest) => collect_var_binding_names_from_pat(&rest.arg, out),
        _ => {}
    }
}

fn collect_var_binding_names_from_var_decl(var_decl: &ast::VarDecl, out: &mut Vec<String>) {
    if var_decl.kind != ast::VarDeclKind::Var {
        return;
    }
    for decl in &var_decl.decls {
        collect_var_binding_names_from_pat(&decl.name, out);
    }
}

pub(crate) fn collect_var_binding_names_from_stmt(stmt: &ast::Stmt, out: &mut Vec<String>) {
    match stmt {
        ast::Stmt::Block(block) => {
            for stmt in &block.stmts {
                collect_var_binding_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
            collect_var_binding_names_from_var_decl(var_decl, out);
        }
        // Nested function/class bodies have their own var environments.
        ast::Stmt::Decl(ast::Decl::Fn(_)) | ast::Stmt::Decl(ast::Decl::Class(_)) => {}
        ast::Stmt::If(if_stmt) => {
            collect_var_binding_names_from_stmt(&if_stmt.cons, out);
            if let Some(alt) = &if_stmt.alt {
                collect_var_binding_names_from_stmt(alt, out);
            }
        }
        ast::Stmt::While(while_stmt) => collect_var_binding_names_from_stmt(&while_stmt.body, out),
        ast::Stmt::DoWhile(do_while) => collect_var_binding_names_from_stmt(&do_while.body, out),
        ast::Stmt::For(for_stmt) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(var_decl)) = &for_stmt.init {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_stmt.body, out);
        }
        ast::Stmt::ForIn(for_in) => {
            if let ast::ForHead::VarDecl(var_decl) = &for_in.left {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_in.body, out);
        }
        ast::Stmt::ForOf(for_of) => {
            if let ast::ForHead::VarDecl(var_decl) = &for_of.left {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_of.body, out);
        }
        ast::Stmt::Labeled(labeled) => collect_var_binding_names_from_stmt(&labeled.body, out),
        ast::Stmt::Switch(switch_stmt) => {
            for case in &switch_stmt.cases {
                for stmt in &case.cons {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
        }
        ast::Stmt::Try(try_stmt) => {
            for stmt in &try_stmt.block.stmts {
                collect_var_binding_names_from_stmt(stmt, out);
            }
            if let Some(handler) = &try_stmt.handler {
                for stmt in &handler.body.stmts {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                for stmt in &finalizer.stmts {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
        }
        ast::Stmt::With(with_stmt) => collect_var_binding_names_from_stmt(&with_stmt.body, out),
        _ => {}
    }
}

/// Collect the lexically-declared names (`let` / `const` / `class`) at the top
/// level of a statement list. A `var` or a `function` declaration is NOT
/// lexical and does not belong here. Used to build the Annex B "forbidden" set:
/// a block-level function declaration whose name collides with a lexical
/// binding in an enclosing scope would make the equivalent `var` an early
/// error, so B.3.3 skips creating the enclosing-scope `var`.
pub(crate) fn collect_lexical_decl_names(
    stmts: &[ast::Stmt],
    out: &mut std::collections::HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) if var_decl.kind != ast::VarDeclKind::Var => {
                for decl in &var_decl.decls {
                    let mut names = Vec::new();
                    collect_var_binding_names_from_pat(&decl.name, &mut names);
                    out.extend(names);
                }
            }
            ast::Stmt::Decl(ast::Decl::Class(class_decl)) => {
                out.insert(class_decl.ident.sym.to_string());
            }
            _ => {}
        }
    }
}

/// Annex B B.3.3 (#5297): collect the names of function declarations that
/// appear *inside a nested block* of a function/program body. In sloppy mode
/// such a legacy block-level function declaration ALSO creates a `var`-style
/// binding in the enclosing function/global scope (`f` is visible — as a `var`
/// initialised to `undefined` until the declaration runs — outside the block).
///
/// `body_stmts` are the body's own top-level statements: a `function f(){}`
/// directly among them is an ordinary FunctionDeclaration (already function-
/// scoped) and is NOT collected; every function declaration reached by
/// descending through a block / `if` branch / loop body / `switch` case /
/// `try` part / labeled / `with` body IS. `forbidden` seeds the names for which
/// the legacy `var` must be skipped — the spec gates B.3.3 on "replacing the
/// FunctionDeclaration with a `var` produces no early error and the name is not
/// a parameter": callers pass the parameter names, the body's own top-level
/// lexical names, and `"arguments"`. As we descend, each block contributes its
/// own `let`/`const`/`class` names to the forbidden set for everything nested
/// within it (so `{ let f; { function f(){} } }` is correctly skipped). Nested
/// function and class bodies own their own var environment and are not entered.
/// One traversal yields two results:
/// - `all_out`: EVERY block-nested function declaration name. Every block-level
///   function declaration is block-scoped (gets its own binding), so
///   `lower_nested_fn_decl` gives these a fresh local rather than clobbering an
///   enclosing same-named parameter/binding.
/// - `var_out`: the subset that ALSO gets the legacy enclosing-scope `var` —
///   names not in `forbidden` and not shadowed by an enclosing block's
///   `let`/`const`/`class` (which would make `var f` an early error).
pub(crate) fn collect_annexb_block_fn_decl_names(
    body_stmts: &[ast::Stmt],
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    for stmt in body_stmts {
        // A direct top-level function declaration is already function-scoped.
        if matches!(stmt, ast::Stmt::Decl(ast::Decl::Fn(_))) {
            continue;
        }
        annexb_nested_stmt(stmt, forbidden, all_out, var_out);
    }
}

fn annexb_nested_stmt(
    stmt: &ast::Stmt,
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    match stmt {
        ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
            let name = fn_decl.ident.sym.to_string();
            all_out.push(name.clone());
            if !forbidden.contains(&name) {
                var_out.push(name);
            }
        }
        // Nested function/class bodies have their own var environment.
        ast::Stmt::Decl(ast::Decl::Class(_)) => {}
        ast::Stmt::Block(block) => annexb_nested_block(&block.stmts, forbidden, all_out, var_out),
        ast::Stmt::If(if_stmt) => {
            annexb_nested_stmt(&if_stmt.cons, forbidden, all_out, var_out);
            if let Some(alt) = &if_stmt.alt {
                annexb_nested_stmt(alt, forbidden, all_out, var_out);
            }
        }
        ast::Stmt::While(while_stmt) => {
            annexb_nested_stmt(&while_stmt.body, forbidden, all_out, var_out)
        }
        ast::Stmt::DoWhile(do_while) => {
            annexb_nested_stmt(&do_while.body, forbidden, all_out, var_out)
        }
        // A `for`/`for-in`/`for-of` lexical head (`for (let f; ...)`,
        // `for (let f in/of ...)`) introduces a binding whose scope encloses
        // the loop body; an equivalent `var f` in the body is an early error
        // (14.7.4.1 / 14.7.5.1), so the AnnexB legacy `var` for a same-named
        // block function in the body must be skipped.
        ast::Stmt::For(for_stmt) => {
            let names = match &for_stmt.init {
                Some(ast::VarDeclOrExpr::VarDecl(vd)) => var_decl_lexical_names(vd),
                _ => Vec::new(),
            };
            annexb_nested_loop_body(&for_stmt.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::ForIn(for_in) => {
            let names = for_head_lexical_names(&for_in.left);
            annexb_nested_loop_body(&for_in.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::ForOf(for_of) => {
            let names = for_head_lexical_names(&for_of.left);
            annexb_nested_loop_body(&for_of.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::Labeled(labeled) => {
            annexb_nested_stmt(&labeled.body, forbidden, all_out, var_out)
        }
        ast::Stmt::Switch(switch_stmt) => {
            // All cases of a switch share one block scope, so their lexical
            // names contribute together to the forbidden set.
            let mut inner = forbidden.clone();
            for case in &switch_stmt.cases {
                collect_lexical_decl_names(&case.cons, &mut inner);
            }
            for case in &switch_stmt.cases {
                for stmt in &case.cons {
                    annexb_nested_stmt(stmt, &inner, all_out, var_out);
                }
            }
        }
        ast::Stmt::Try(try_stmt) => {
            annexb_nested_block(&try_stmt.block.stmts, forbidden, all_out, var_out);
            if let Some(handler) = &try_stmt.handler {
                // B.3.5: a `var` whose name is also a bound name of a
                // *destructuring* CatchParameter is an early error, so the
                // equivalent AnnexB legacy `var` for a same-named block
                // function in the handler body must be skipped. The B.3.5
                // exception only exempts a simple `catch (e)` BindingIdentifier
                // (where the var IS allowed), so only pattern catch params
                // (`catch ({ f })` / `catch ([f])`) contribute to `forbidden`.
                let mut handler_forbidden;
                let inner = match &handler.param {
                    Some(param) if !matches!(param, ast::Pat::Ident(_)) => {
                        handler_forbidden = forbidden.clone();
                        let mut names = Vec::new();
                        collect_var_binding_names_from_pat(param, &mut names);
                        handler_forbidden.extend(names);
                        &handler_forbidden
                    }
                    _ => forbidden,
                };
                annexb_nested_block(&handler.body.stmts, inner, all_out, var_out);
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                annexb_nested_block(&finalizer.stmts, forbidden, all_out, var_out);
            }
        }
        ast::Stmt::With(with_stmt) => {
            annexb_nested_stmt(&with_stmt.body, forbidden, all_out, var_out)
        }
        _ => {}
    }
}

/// Lexical (`let`/`const`) binding names introduced by a `VarDecl`. A `var`
/// declaration introduces no lexical names and yields an empty list.
fn var_decl_lexical_names(vd: &ast::VarDecl) -> Vec<String> {
    if vd.kind == ast::VarDeclKind::Var {
        return Vec::new();
    }
    let mut names = Vec::new();
    for decl in &vd.decls {
        collect_var_binding_names_from_pat(&decl.name, &mut names);
    }
    names
}

/// Lexical binding names of a `for-in` / `for-of` head (`for (let f in …)`).
/// A `var` head or a bare assignment-target pattern introduces no lexical
/// binding here and yields an empty list.
fn for_head_lexical_names(head: &ast::ForHead) -> Vec<String> {
    match head {
        ast::ForHead::VarDecl(vd) => var_decl_lexical_names(vd),
        _ => Vec::new(),
    }
}

/// Descend into a loop body, adding the loop head's lexical binding names to
/// the forbidden set so a same-named block function in the body skips its
/// AnnexB legacy `var` (the equivalent `var` would be an early error).
fn annexb_nested_loop_body(
    body: &ast::Stmt,
    lexical_names: Vec<String>,
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    if lexical_names.is_empty() {
        annexb_nested_stmt(body, forbidden, all_out, var_out);
    } else {
        let mut inner = forbidden.clone();
        inner.extend(lexical_names);
        annexb_nested_stmt(body, &inner, all_out, var_out);
    }
}

fn annexb_nested_block(
    stmts: &[ast::Stmt],
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    let mut inner = forbidden.clone();
    collect_lexical_decl_names(stmts, &mut inner);
    for stmt in stmts {
        annexb_nested_stmt(stmt, &inner, all_out, var_out);
    }
}
