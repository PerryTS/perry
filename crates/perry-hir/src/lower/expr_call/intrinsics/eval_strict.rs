use super::*;

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use super::super::super::LoweringContext;

/// #1678 (Phase 0 of #1677) — classify a bare `Function(...)` /
/// `eval(...)` call. The `Function('return this')()` globalThis fold runs
/// before this (in `lower_call_inner`) and short-circuits, so its inner
/// `Function('return this')` never reaches here.
///
/// In strict-eval mode returns `Err` (span-tagged) for the runtime-unknown
/// bucket — const-foldable (string-literal body) and known-codegen-library
/// sites log under `PERRY_EVAL_DIAG` and fall through (`Ok(None)`) to the
/// existing lowering, to be picked up by later phases. Under the default
/// (defer) mode a runtime-unknown site returns `Ok(Some(throw_value))`
/// (#5206): the caller uses that expression in place of the call so it
/// throws a descriptive `Error` only if reached. `Ok(None)` means proceed.
pub(crate) fn check_eval_function_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    // `Function.apply(thisArg, argsArray)` / `Function.call(thisArg, …, body)` reach
    // CreateDynamicFunction indirectly — the same surface as a direct `Function(body)`,
    // just spelled through the constructor's own `apply`/`call`. Recognize them here so
    // an unfoldable body is classified. Otherwise the site fell through to the generic
    // call lowering and evaluated to `undefined`; the caller then invoked `.apply` on
    // that `undefined` and failed several frames away with a misleading
    // "Function.prototype.apply was called on a value that is not a function", naming
    // neither eval nor the real cause. mysql2's row-parser codegen is exactly this
    // shape: `Function.apply(null, argNames.concat(body)).apply(null, argValues)`.
    //
    // A const-foldable spelling never reaches here — `try_eval_function_member_call_fold`
    // runs first and compiles it.
    let mut member_apply = false;
    let ident = match callee {
        ast::Expr::Ident(id) => id,
        ast::Expr::Member(m) => {
            let ast::MemberProp::Ident(prop) = &m.prop else {
                return Ok(None);
            };
            match prop.sym.as_ref() {
                "apply" => member_apply = true,
                "call" => {}
                _ => return Ok(None),
            }
            let mut obj = m.obj.as_ref();
            while let ast::Expr::Paren(p) = obj {
                obj = p.expr.as_ref();
            }
            let ast::Expr::Ident(id) = obj else {
                return Ok(None);
            };
            if id.sym.as_ref() != "Function" {
                return Ok(None);
            }
            id
        }
        _ => return Ok(None),
    };
    let name = ident.sym.as_ref();
    let surface = match name {
        "eval" if !member_apply && !matches!(callee, ast::Expr::Member(_)) => {
            crate::eval_classifier::EvalSurface::Eval
        }
        "Function" => crate::eval_classifier::EvalSurface::FunctionCall,
        _ => return Ok(None),
    };
    // A local/func/imported binding named `eval`/`Function` shadows the
    // builtin — leave those alone.
    if ctx.lookup_local(name).is_some()
        || ctx.lookup_func(name).is_some()
        || ctx.lookup_imported_func(name).is_some()
    {
        return Ok(None);
    }
    // Body argument: the only arg for `eval(code)`, the last arg for
    // `Function(p1, p2, body)`. A spread in the body position yields a
    // non-constant inner expr → the classifier buckets it runtime-unknown.
    //
    // `Function.call(thisArg, p1, …, body)` — CreateDynamicFunction ignores its `this`,
    // so the body is still the last argument. `Function.apply(thisArg, argsArray)` —
    // the body is the array's last element when the array is a literal; when the list is
    // assembled at runtime the array expression itself stands in, which is by
    // construction non-constant, so the classifier buckets the site runtime-unknown.
    let body_arg = if member_apply {
        match call.args.get(1).map(|a| a.expr.as_ref()) {
            Some(ast::Expr::Array(arr)) => arr
                .elems
                .last()
                .and_then(|e| e.as_ref())
                .map(|e| e.expr.as_ref()),
            other => other,
        }
    } else {
        match surface {
            crate::eval_classifier::EvalSurface::Eval => call.args.first(),
            _ => call.args.last(),
        }
        .map(|a| a.expr.as_ref())
    };
    match crate::eval_classifier::check_site(surface, body_arg, &ctx.source_file_path, call.span)? {
        crate::eval_classifier::EvalDecision::Proceed => Ok(None),
        crate::eval_classifier::EvalDecision::DeferToRuntimeError(message) => Ok(Some(
            super::super::super::const_fold_fn::synth_deferred_eval_value(
                ctx, surface, &message, call.span,
            )?,
        )),
    }
}

pub(crate) fn try_strict_eval_arguments_assignment(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Option<Expr> {
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Ident(ident) = callee else {
        return None;
    };
    if ident.sym.as_ref() != "eval"
        || ctx.lookup_local("eval").is_some()
        || ctx.lookup_func("eval").is_some()
        || ctx.lookup_imported_func("eval").is_some()
    {
        return None;
    }
    let ast::Expr::Lit(ast::Lit::Str(source)) = call.args[0].expr.as_ref() else {
        return None;
    };
    let source = source.value.as_str().unwrap_or("");
    let outer_strict = ctx.current_strict_mode() || ctx.current_strict;

    // Spec early errors for eval code: in strict-mode code (inherited from
    // the calling context for direct eval, or introduced by a directive in
    // the eval source itself), binding, assigning, or naming a function
    // `eval` / `arguments` is a SyntaxError thrown by the eval call.
    // Parse the source and scan; fall back to the older substring heuristic
    // when the source doesn't parse here.
    let parses = perry_parser::parse_typescript(source, "<eval body>.cjs");
    let violation = match &parses {
        Ok(module) => eval_module_has_strict_eval_arguments_violation(module, outer_strict),
        // SWC enforces some strict early errors at parse time (e.g.
        // `eval = 42` inside a 'use strict' function body). A source that
        // fails to parse while strict-mode is in play is a SyntaxError at
        // the eval call. Keep sloppy parse failures on the existing path —
        // SWC's TS grammar rejects some legal sloppy JS (legacy octal etc.).
        Err(_) => outer_strict || source.contains("use strict"),
    };
    if !violation {
        return None;
    }
    Some(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_strict_eval_arguments_syntax_error".to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

fn is_restricted_name(name: &str) -> bool {
    name == "eval" || name == "arguments"
}

fn stmts_start_with_use_strict(stmts: &[ast::Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Expr(expr_stmt) => match expr_stmt.expr.as_ref() {
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    if s.value.as_str() == Some("use strict") {
                        return true;
                    }
                    // Other directive-prologue strings — keep scanning.
                }
                _ => return false,
            },
            _ => return false,
        }
    }
    false
}

fn pat_binds_restricted_name(pat: &ast::Pat) -> bool {
    match pat {
        ast::Pat::Ident(ident) => is_restricted_name(ident.id.sym.as_ref()),
        ast::Pat::Array(arr) => arr.elems.iter().flatten().any(pat_binds_restricted_name),
        ast::Pat::Object(obj) => obj.props.iter().any(|p| match p {
            ast::ObjectPatProp::Assign(a) => is_restricted_name(a.key.sym.as_ref()),
            ast::ObjectPatProp::KeyValue(kv) => pat_binds_restricted_name(&kv.value),
            ast::ObjectPatProp::Rest(r) => pat_binds_restricted_name(&r.arg),
        }),
        ast::Pat::Assign(a) => pat_binds_restricted_name(&a.left),
        ast::Pat::Rest(r) => pat_binds_restricted_name(&r.arg),
        _ => false,
    }
}

fn collect_param_names(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(ident) => out.push(ident.id.sym.to_string()),
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_param_names(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for p in &obj.props {
                match p {
                    ast::ObjectPatProp::Assign(a) => out.push(a.key.sym.to_string()),
                    ast::ObjectPatProp::KeyValue(kv) => collect_param_names(&kv.value, out),
                    ast::ObjectPatProp::Rest(r) => collect_param_names(&r.arg, out),
                }
            }
        }
        ast::Pat::Assign(a) => collect_param_names(&a.left, out),
        ast::Pat::Rest(r) => collect_param_names(&r.arg, out),
        _ => {}
    }
}

fn function_has_violation(func: &ast::Function, name: Option<&str>, strict: bool) -> bool {
    let body_strict = strict
        || func
            .body
            .as_ref()
            .is_some_and(|b| stmts_start_with_use_strict(&b.stmts));
    if body_strict {
        if let Some(n) = name {
            if is_restricted_name(n) {
                return true;
            }
        }
        if func
            .params
            .iter()
            .any(|p| pat_binds_restricted_name(&p.pat))
        {
            return true;
        }
        // Duplicate parameter names are a strict-mode early error
        // (`function f(param, param) {}` — test262 13.1-2x-s).
        let mut names = Vec::new();
        for p in &func.params {
            collect_param_names(&p.pat, &mut names);
        }
        names.sort();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return true;
        }
    }
    func.body
        .as_ref()
        .is_some_and(|b| b.stmts.iter().any(|s| stmt_has_violation(s, body_strict)))
}

fn expr_has_violation(expr: &ast::Expr, strict: bool) -> bool {
    use ast::Expr as E;
    match expr {
        E::Assign(assign) => {
            if strict {
                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(id)) = &assign.left
                {
                    if is_restricted_name(id.id.sym.as_ref()) {
                        return true;
                    }
                }
            }
            expr_has_violation(&assign.right, strict)
        }
        E::Update(update) => {
            if strict {
                if let E::Ident(id) = update.arg.as_ref() {
                    if is_restricted_name(id.sym.as_ref()) {
                        return true;
                    }
                }
            }
            expr_has_violation(&update.arg, strict)
        }
        E::Fn(fn_expr) => function_has_violation(
            &fn_expr.function,
            fn_expr.ident.as_ref().map(|i| i.sym.as_ref()),
            strict,
        ),
        E::Arrow(arrow) => {
            if strict && arrow.params.iter().any(pat_binds_restricted_name) {
                return true;
            }
            match arrow.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    let body_strict = strict || stmts_start_with_use_strict(&b.stmts);
                    b.stmts.iter().any(|s| stmt_has_violation(s, body_strict))
                }
                ast::BlockStmtOrExpr::Expr(e) => expr_has_violation(e, strict),
            }
        }
        E::Call(call) => {
            if let ast::Callee::Expr(c) = &call.callee {
                if matches!(c.as_ref(), E::Ident(i) if i.sym.as_ref() == "Function")
                    && function_ctor_body_has_violation(call.args.last())
                {
                    return true;
                }
            }
            call.args
                .iter()
                .any(|a| expr_has_violation(&a.expr, strict))
                || matches!(&call.callee, ast::Callee::Expr(c) if expr_has_violation(c, strict))
        }
        E::New(new_expr) => {
            // `new Function(p1, …, body)` with a literal body that carries
            // its own strict directive + violation — the ctor throws the
            // SyntaxError when the eval body runs (13.0-13/14-s).
            if matches!(new_expr.callee.as_ref(), E::Ident(i) if i.sym.as_ref() == "Function")
                && function_ctor_body_has_violation(new_expr.args.as_ref().and_then(|a| a.last()))
            {
                return true;
            }
            expr_has_violation(&new_expr.callee, strict)
                || new_expr
                    .args
                    .iter()
                    .flatten()
                    .any(|a| expr_has_violation(&a.expr, strict))
        }
        E::Paren(p) => expr_has_violation(&p.expr, strict),
        E::Seq(seq) => seq.exprs.iter().any(|e| expr_has_violation(e, strict)),
        E::Bin(b) => expr_has_violation(&b.left, strict) || expr_has_violation(&b.right, strict),
        E::Unary(u) => expr_has_violation(&u.arg, strict),
        E::Cond(c) => {
            expr_has_violation(&c.test, strict)
                || expr_has_violation(&c.cons, strict)
                || expr_has_violation(&c.alt, strict)
        }
        E::Member(m) => expr_has_violation(&m.obj, strict),
        E::Array(arr) => arr
            .elems
            .iter()
            .flatten()
            .any(|el| expr_has_violation(&el.expr, strict)),
        E::Object(obj) => obj.props.iter().any(|p| match p {
            ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                ast::Prop::KeyValue(kv) => expr_has_violation(&kv.value, strict),
                ast::Prop::Method(m) => function_has_violation(&m.function, None, strict),
                _ => false,
            },
            ast::PropOrSpread::Spread(s) => expr_has_violation(&s.expr, strict),
        }),
        _ => false,
    }
}

/// `Function(p…, body)` / `new Function(p…, body)` with a literal body whose
/// own directive prologue is 'use strict' and which contains a restricted
/// eval/arguments binding or assignment. Function-constructor bodies do NOT
/// inherit outer strictness, so only the body's own directive counts.
fn function_ctor_body_has_violation(body_arg: Option<&ast::ExprOrSpread>) -> bool {
    let Some(arg) = body_arg else { return false };
    let ast::Expr::Lit(ast::Lit::Str(s)) = arg.expr.as_ref() else {
        return false;
    };
    let src = s.value.as_str().unwrap_or("");
    match perry_parser::parse_typescript(src, "<fn ctor body>.cjs") {
        Ok(module) => {
            let owned: Vec<ast::Stmt> = module
                .body
                .iter()
                .filter_map(|item| match item {
                    ast::ModuleItem::Stmt(stmt) => Some(stmt.clone()),
                    _ => None,
                })
                .collect();
            let body_strict = stmts_start_with_use_strict(&owned);
            body_strict && owned.iter().any(|s| stmt_has_violation(s, true))
        }
        Err(_) => src.contains("use strict"),
    }
}

fn var_decl_has_violation(var_decl: &ast::VarDecl, strict: bool) -> bool {
    var_decl.decls.iter().any(|d| {
        (strict && pat_binds_restricted_name(&d.name))
            || d.init
                .as_ref()
                .is_some_and(|e| expr_has_violation(e, strict))
    })
}

fn stmt_has_violation(stmt: &ast::Stmt, strict: bool) -> bool {
    use ast::Stmt as S;
    match stmt {
        S::Expr(e) => expr_has_violation(&e.expr, strict),
        S::Decl(ast::Decl::Var(v)) => var_decl_has_violation(v, strict),
        S::Decl(ast::Decl::Fn(f)) => {
            function_has_violation(&f.function, Some(f.ident.sym.as_ref()), strict)
        }
        S::Block(b) => b.stmts.iter().any(|s| stmt_has_violation(s, strict)),
        S::If(i) => {
            expr_has_violation(&i.test, strict)
                || stmt_has_violation(&i.cons, strict)
                || i.alt
                    .as_ref()
                    .is_some_and(|a| stmt_has_violation(a, strict))
        }
        S::While(w) => expr_has_violation(&w.test, strict) || stmt_has_violation(&w.body, strict),
        S::DoWhile(w) => expr_has_violation(&w.test, strict) || stmt_has_violation(&w.body, strict),
        S::For(f) => {
            f.init.as_ref().is_some_and(|i| match i {
                ast::VarDeclOrExpr::VarDecl(v) => var_decl_has_violation(v, strict),
                ast::VarDeclOrExpr::Expr(e) => expr_has_violation(e, strict),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_has_violation(e, strict))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_violation(e, strict))
                || stmt_has_violation(&f.body, strict)
        }
        S::ForIn(f) => stmt_has_violation(&f.body, strict),
        S::ForOf(f) => stmt_has_violation(&f.body, strict),
        S::Try(t) => {
            t.block.stmts.iter().any(|s| stmt_has_violation(s, strict))
                || t.handler.as_ref().is_some_and(|h| {
                    (strict && h.param.as_ref().is_some_and(pat_binds_restricted_name))
                        || h.body.stmts.iter().any(|s| stmt_has_violation(s, strict))
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.stmts.iter().any(|s| stmt_has_violation(s, strict)))
        }
        S::Switch(sw) => sw.cases.iter().any(|c| {
            c.test
                .as_ref()
                .is_some_and(|e| expr_has_violation(e, strict))
                || c.cons.iter().any(|s| stmt_has_violation(s, strict))
        }),
        S::Return(r) => r
            .arg
            .as_ref()
            .is_some_and(|e| expr_has_violation(e, strict)),
        S::Throw(t) => expr_has_violation(&t.arg, strict),
        S::Labeled(l) => stmt_has_violation(&l.body, strict),
        S::With(w) => expr_has_violation(&w.obj, strict) || stmt_has_violation(&w.body, strict),
        _ => false,
    }
}

fn eval_module_has_strict_eval_arguments_violation(
    module: &ast::Module,
    outer_strict: bool,
) -> bool {
    let stmts: Vec<&ast::Stmt> = module
        .body
        .iter()
        .filter_map(|item| match item {
            ast::ModuleItem::Stmt(s) => Some(s),
            _ => None,
        })
        .collect();
    let top_strict = outer_strict || {
        // Directive prologue of the eval source itself.
        let mut prologue_strict = false;
        for s in &stmts {
            match s {
                ast::Stmt::Expr(e) => match e.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(lit)) => {
                        if lit.value.as_str() == Some("use strict") {
                            prologue_strict = true;
                            break;
                        }
                    }
                    _ => break,
                },
                _ => break,
            }
        }
        prologue_strict
    };
    stmts.iter().any(|s| stmt_has_violation(s, top_strict))
}

fn strict_eval_source_assigns_arguments(source: &str) -> bool {
    let bytes = source.as_bytes();
    let needle = b"arguments";
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        let before_ok = i == 0 || !is_ident_continue(bytes[i - 1]);
        let after = i + needle.len();
        let after_ok = after == bytes.len() || !is_ident_continue(bytes[after]);
        if before_ok && after_ok {
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len()
                && bytes[j] == b'='
                && bytes.get(j + 1).copied() != Some(b'=')
                && bytes.get(j + 1).copied() != Some(b'>')
            {
                return true;
            }
        }
        i = after;
    }
    false
}

fn is_ident_continue(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}
