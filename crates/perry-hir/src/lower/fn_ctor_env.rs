//! #1679 follow-up — compile-time argument resolution for `new Function` /
//! `Function(...)` beyond direct literals.
//!
//! Test262's Function-constructor suite passes arguments through local
//! variables and `toString`-bearing objects: `var body = {toString:
//! function() { return "return 1;"; }}; new Function(body)`. The values are
//! still compile-time constants — they just need (a) resolving the
//! identifier to its single, never-reassigned initializer and (b) running
//! the spec's `ToString` over that initializer, including calling a literal
//! `toString` method whose body is a constant `return`/`throw`.
//!
//! The pre-scan walks the whole module once and records, for every variable
//! name that is declared exactly once and never written again anywhere in
//! the module, a [`FnCtorShape`] describing its initializer. A second pass
//! marks "counter" variables (`var i = 0` mutated *only* by `++i` inside a
//! recorded object's `toString`) so the evaluator can model the
//! `"arg" + (++i)` pattern with the exact call-order semantics the runtime
//! would have.
//!
//! Everything here is best-effort and conservative: any shape outside the
//! recognized subset resolves to "not constant" and the call site falls
//! back to the Phase 0 classifier.

use std::collections::HashMap;

use swc_ecma_ast as ast;

/// What a single-assignment module variable's initializer looks like.
#[derive(Debug, Clone)]
pub(crate) enum FnCtorShape {
    /// Initializer coerces to this exact string (string/number/bool/null
    /// literals, substitution-free templates, `Object(<lit>)` wrappers).
    Str(String),
    /// `var x;` with no initializer and no assignment anywhere — reading it
    /// yields `undefined` (`ToString` → `"undefined"`).
    UndefinedVar,
    /// Object literal whose only property is a literal `toString` method.
    /// The body (one `return <expr>` or `throw <expr>` statement) is kept
    /// for the partial evaluator.
    ObjToString(ToStringBody),
    /// `var AsyncFunction = (async function () {}).constructor;` — a dynamic
    /// function constructor obtained off a function literal. Calling it with
    /// constant args folds like `Function(...)` with the matching prefix.
    DynCtor(DynFnCtorKind),
    /// `var f = async function () {};` — a function-literal var, recorded so
    /// a later `f.constructor` resolves to the right dynamic ctor kind.
    FnLiteral(DynFnCtorKind),
}

/// Which dynamic-function intrinsic a `<fn literal>.constructor` read names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DynFnCtorKind {
    Plain,
    Async,
    Generator,
    AsyncGenerator,
}

impl DynFnCtorKind {
    /// The `function` keyword form for the spec-assembled source.
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            DynFnCtorKind::Plain => "function",
            DynFnCtorKind::Async => "async function",
            DynFnCtorKind::Generator => "function*",
            DynFnCtorKind::AsyncGenerator => "async function*",
        }
    }
}

/// Classify a function LITERAL by its dynamic-ctor kind.
pub(crate) fn fn_literal_kind_of(expr: &ast::Expr) -> Option<DynFnCtorKind> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Fn(f) => Some(match (f.function.is_async, f.function.is_generator) {
            (false, false) => DynFnCtorKind::Plain,
            (true, false) => DynFnCtorKind::Async,
            (false, true) => DynFnCtorKind::Generator,
            (true, true) => DynFnCtorKind::AsyncGenerator,
        }),
        ast::Expr::Arrow(a) => Some(if a.is_async {
            DynFnCtorKind::Async
        } else {
            DynFnCtorKind::Plain
        }),
        _ => None,
    }
}

/// Recognize `<function literal>.constructor` (or `<fn-literal var>
/// .constructor`, resolved through `known_literals`) and classify which
/// dynamic function constructor it denotes.
pub(crate) fn dyn_fn_ctor_kind_of(
    expr: &ast::Expr,
    known_literals: &HashMap<String, DynFnCtorKind>,
) -> Option<DynFnCtorKind> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    let ast::Expr::Member(m) = e else {
        return None;
    };
    if !matches!(&m.prop, ast::MemberProp::Ident(id) if id.sym.as_ref() == "constructor") {
        return None;
    }
    let mut obj = m.obj.as_ref();
    while let ast::Expr::Paren(p) = obj {
        obj = p.expr.as_ref();
    }
    // `Object.getPrototypeOf(<fn-literal>).constructor` — the standard idiom
    // for reaching the hidden `AsyncFunction`/`GeneratorFunction`/
    // `AsyncGeneratorFunction` global constructors, which (unlike `Function`)
    // aren't reachable as named globals. `Object.getPrototypeOf(fnExpr)`
    // yields fnExpr's `[[Prototype]]` (e.g. `%GeneratorFunction.prototype%`
    // for a generator literal), and `.constructor` on THAT is the matching
    // hidden constructor — same kind as `fnExpr` itself (test262
    // toString/GeneratorFunction.js).
    if let Some(kind) = dyn_fn_ctor_kind_via_get_prototype_of(obj, known_literals) {
        return Some(kind);
    }
    if let Some(kind) = fn_literal_kind_of(obj) {
        return Some(kind);
    }
    if let ast::Expr::Ident(id) = obj {
        return known_literals.get(id.sym.as_str()).copied();
    }
    None
}

/// Match `Object.getPrototypeOf(<arg>)` and resolve `<arg>`'s dynamic-ctor
/// kind (function literal or known single-assignment variable).
fn dyn_fn_ctor_kind_via_get_prototype_of(
    obj: &ast::Expr,
    known_literals: &HashMap<String, DynFnCtorKind>,
) -> Option<DynFnCtorKind> {
    let ast::Expr::Call(call) = obj else {
        return None;
    };
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let mut c = callee.as_ref();
    while let ast::Expr::Paren(p) = c {
        c = p.expr.as_ref();
    }
    let ast::Expr::Member(gm) = c else {
        return None;
    };
    let is_get_proto = matches!(&gm.prop, ast::MemberProp::Ident(id) if id.sym.as_ref() == "getPrototypeOf")
        && matches!(gm.obj.as_ref(), ast::Expr::Ident(oid) if oid.sym.as_ref() == "Object");
    if !is_get_proto {
        return None;
    }
    let mut inner = call.args[0].expr.as_ref();
    while let ast::Expr::Paren(p) = inner {
        inner = p.expr.as_ref();
    }
    if let Some(kind) = fn_literal_kind_of(inner) {
        return Some(kind);
    }
    if let ast::Expr::Ident(id) = inner {
        return known_literals.get(id.sym.as_str()).copied();
    }
    None
}

/// The retained `return`/`throw` statement of a recorded `toString` method.
#[derive(Debug, Clone)]
pub(crate) struct ToStringBody {
    pub(crate) is_throw: bool,
    pub(crate) expr: ast::Expr,
    /// Names assigned by leading side-effect statements in the body
    /// (`toString: function() { p = 1; return "a"; }`). Evaluating the body
    /// poisons these in the env — a later read of a reassigned name must
    /// not see the stale recorded shape.
    pub(crate) poisoned: Vec<String>,
    /// The same leading assignments rendered as JS statements (`"p = 1;"`)
    /// so the synthesized lowering can replay them at runtime — they are
    /// real observable side effects (Test262 asserts `p === 1` afterwards).
    pub(crate) assigns: Vec<String>,
}

/// Pre-scanned constant environment for `Function(...)` argument resolution.
#[derive(Debug, Default)]
pub(crate) struct FnCtorEnv {
    pub(crate) entries: HashMap<String, FnCtorShape>,
    /// Counter variables: name → current compile-time value. Mutated by the
    /// evaluator as it models successive `toString` calls.
    pub(crate) counters: HashMap<String, f64>,
    /// Side-effect assignment statements (rendered JS) performed by the
    /// `toString` bodies evaluated so far for the CURRENT call site, in
    /// execution order. The fold prepends these to a synthesized throw so
    /// the effects stay observable at runtime.
    pub(crate) pending_side_effects: Vec<String>,
}

/// A resolved `Function(...)` argument: either the string `ToString` would
/// produce, or the constant value its `toString` would throw.
pub(crate) enum ResolvedArg {
    Str(String),
    Thrown(ConstVal),
}

/// Values the partial evaluator can produce.
#[derive(Debug, Clone)]
pub(crate) enum ConstVal {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
}

impl ConstVal {
    fn to_js_string(&self) -> String {
        match self {
            ConstVal::Str(s) => s.clone(),
            ConstVal::Num(n) => super::const_fold_fn::js_number_to_string(*n),
            ConstVal::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            ConstVal::Null => "null".to_string(),
            ConstVal::Undefined => "undefined".to_string(),
        }
    }

    /// Render as a JavaScript expression that evaluates to this value, for
    /// splicing into a synthesized `throw <value>;` statement.
    pub(crate) fn to_js_literal(&self) -> String {
        match self {
            ConstVal::Str(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\u{2028}' => out.push_str("\\u2028"),
                        '\u{2029}' => out.push_str("\\u2029"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            ConstVal::Num(n) if n.is_nan() => "NaN".to_string(),
            ConstVal::Num(n) => super::const_fold_fn::js_number_to_string(*n),
            ConstVal::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            ConstVal::Null => "null".to_string(),
            ConstVal::Undefined => "undefined".to_string(),
        }
    }
}

/// Build the constant environment for a module. Called once from
/// `lower_module_full` before statement lowering.
pub(crate) fn build_fn_ctor_env(module: &ast::Module) -> FnCtorEnv {
    let mut decls: HashMap<String, (usize, Option<ast::Expr>)> = HashMap::new();
    let mut writes: HashMap<String, usize> = HashMap::new();

    let mut empty_shadow = Shadow::new();
    for item in &module.body {
        if let ast::ModuleItem::Stmt(stmt) = item {
            scan_stmt(stmt, &mut decls, &mut writes, &mut empty_shadow);
        }
    }

    let mut env = FnCtorEnv::default();

    // First gather every recorded-shape `toString` body so writes that occur
    // INSIDE those bodies (counter updates `++i`, leading poison assignments
    // `p = 1`) can be netted out of the disqualifying write counts.
    let mut tostring_update_counts: HashMap<String, usize> = HashMap::new();
    let mut tostring_poison_counts: HashMap<String, usize> = HashMap::new();
    let mut tostring_candidates: Vec<(String, ToStringBody)> = Vec::new();
    let mut numeric_candidates: Vec<(String, f64)> = Vec::new();

    for (name, (decl_count, init)) in &decls {
        if *decl_count != 1 {
            continue;
        }
        if let Some(expr) = init {
            if let Some(body) = object_tostring_body(expr) {
                count_counter_updates(&body.expr, &mut tostring_update_counts);
                for p in &body.poisoned {
                    *tostring_poison_counts.entry(p.clone()).or_insert(0) += 1;
                }
                tostring_candidates.push((name.clone(), body));
            } else if let Some(n) = numeric_literal_of(expr) {
                numeric_candidates.push((name.clone(), n));
            }
        }
    }

    let accounted = |name: &str,
                     update_counts: &HashMap<String, usize>,
                     poison_counts: &HashMap<String, usize>| {
        update_counts.get(name).copied().unwrap_or(0)
            + poison_counts.get(name).copied().unwrap_or(0)
    };

    // Function-literal vars first, so `var f = async function () {};
    // var AF = f.constructor;` resolves in declaration order regardless of
    // HashMap iteration.
    let mut fn_literal_vars: HashMap<String, DynFnCtorKind> = HashMap::new();
    for (name, (decl_count, init)) in &decls {
        if *decl_count != 1 || writes.get(name).copied().unwrap_or(0) != 0 {
            continue;
        }
        if let Some(expr) = init {
            if let Some(kind) = fn_literal_kind_of(expr) {
                fn_literal_vars.insert(name.clone(), kind);
            }
        }
    }
    // Top-level function DECLARATIONS too (`async function f() {}` then
    // `f.constructor`). The declaration itself counted as the name's one
    // write; any further write disqualifies.
    fn collect_fn_decl_kinds(
        stmts: &[ast::Stmt],
        decls: &HashMap<String, (usize, Option<ast::Expr>)>,
        writes: &HashMap<String, usize>,
        out: &mut HashMap<String, DynFnCtorKind>,
    ) {
        for stmt in stmts {
            match stmt {
                ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                    let name = f.ident.sym.to_string();
                    if writes.get(&name).copied().unwrap_or(0) == 1 && !decls.contains_key(&name) {
                        let kind = match (f.function.is_async, f.function.is_generator) {
                            (false, false) => DynFnCtorKind::Plain,
                            (true, false) => DynFnCtorKind::Async,
                            (false, true) => DynFnCtorKind::Generator,
                            (true, true) => DynFnCtorKind::AsyncGenerator,
                        };
                        out.insert(name, kind);
                    }
                }
                ast::Stmt::Block(b) => collect_fn_decl_kinds(&b.stmts, decls, writes, out),
                ast::Stmt::Try(t) => {
                    collect_fn_decl_kinds(&t.block.stmts, decls, writes, out);
                    if let Some(h) = &t.handler {
                        collect_fn_decl_kinds(&h.body.stmts, decls, writes, out);
                    }
                    if let Some(fin) = &t.finalizer {
                        collect_fn_decl_kinds(&fin.stmts, decls, writes, out);
                    }
                }
                _ => {}
            }
        }
    }
    let top_stmts: Vec<ast::Stmt> = module
        .body
        .iter()
        .filter_map(|item| match item {
            ast::ModuleItem::Stmt(stmt) => Some(stmt.clone()),
            _ => None,
        })
        .collect();
    collect_fn_decl_kinds(&top_stmts, &decls, &writes, &mut fn_literal_vars);

    for (name, (decl_count, init)) in &decls {
        if *decl_count != 1 {
            continue;
        }
        let write_count = writes.get(name).copied().unwrap_or(0);
        match init {
            None if write_count == 0 => {
                env.entries.insert(name.clone(), FnCtorShape::UndefinedVar);
            }
            Some(expr) if write_count == 0 => {
                if let Some(s) = wrapper_const_string(expr) {
                    env.entries.insert(name.clone(), FnCtorShape::Str(s));
                } else if let Some(kind) = dyn_fn_ctor_kind_of(expr, &fn_literal_vars) {
                    env.entries.insert(name.clone(), FnCtorShape::DynCtor(kind));
                } else if let Some(kind) = fn_literal_kind_of(expr) {
                    env.entries
                        .insert(name.clone(), FnCtorShape::FnLiteral(kind));
                }
            }
            _ => {}
        }
    }

    // An object-with-toString var qualifies when every write to it is one of
    // the poison assignments inside a recorded body (evaluation poisons it
    // at the right moment, so order stays faithful).
    for (name, body) in tostring_candidates {
        let write_count = writes.get(&name).copied().unwrap_or(0);
        if write_count == tostring_poison_counts.get(&name).copied().unwrap_or(0) {
            env.entries.insert(name, FnCtorShape::ObjToString(body));
        }
    }

    for (name, n) in numeric_candidates {
        let total_writes = writes.get(&name).copied().unwrap_or(0);
        if total_writes == 0 {
            env.entries.insert(
                name,
                FnCtorShape::Str(super::const_fold_fn::js_number_to_string(n)),
            );
        } else if total_writes == accounted(&name, &tostring_update_counts, &tostring_poison_counts)
        {
            env.counters.insert(name, n);
        }
    }

    env
}

fn numeric_literal_of(expr: &ast::Expr) -> Option<f64> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Lit(ast::Lit::Num(n)) => Some(n.value),
        _ => None,
    }
}

/// `ToString` of an initializer that is itself a compile-time constant:
/// literals, substitution-free templates, and the `Object(<primitive>)` /
/// `new Object(<primitive>)` wrappers Test262 uses (whose `toString` is the
/// wrapped primitive's).
pub(crate) fn wrapper_const_string(expr: &ast::Expr) -> Option<String> {
    let mut e = expr;
    loop {
        match e {
            ast::Expr::Paren(p) => e = p.expr.as_ref(),
            ast::Expr::TsAs(t) => e = t.expr.as_ref(),
            ast::Expr::TsTypeAssertion(t) => e = t.expr.as_ref(),
            _ => break,
        }
    }
    if let Some(v) = literal_const_val(e) {
        return Some(v.to_js_string());
    }
    // Object("...") / new Object(1) — a primitive wrapper whose ToString is
    // the wrapped primitive's string. With no (or undefined/null) argument
    // the result is a plain empty object → "[object Object]".
    let args = match e {
        ast::Expr::Call(call) => {
            let ast::Callee::Expr(callee) = &call.callee else {
                return None;
            };
            let ast::Expr::Ident(id) = callee.as_ref() else {
                return None;
            };
            if id.sym.as_ref() != "Object" {
                return None;
            }
            Some(&call.args)
        }
        ast::Expr::New(new_expr) => {
            let ast::Expr::Ident(id) = new_expr.callee.as_ref() else {
                return None;
            };
            if id.sym.as_ref() != "Object" {
                return None;
            }
            new_expr.args.as_ref()
        }
        _ => return None,
    };
    match args.map(|a| a.as_slice()).unwrap_or(&[]) {
        [] => Some("[object Object]".to_string()),
        [arg] if arg.spread.is_none() => match literal_const_val(&arg.expr) {
            Some(ConstVal::Null) | Some(ConstVal::Undefined) => Some("[object Object]".to_string()),
            Some(v) => Some(v.to_js_string()),
            None => None,
        },
        _ => None,
    }
}

fn literal_const_val(expr: &ast::Expr) -> Option<ConstVal> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            Some(ConstVal::Str(s.value.as_str().unwrap_or("").to_string()))
        }
        ast::Expr::Lit(ast::Lit::Num(n)) => Some(ConstVal::Num(n.value)),
        ast::Expr::Lit(ast::Lit::Bool(b)) => Some(ConstVal::Bool(b.value)),
        ast::Expr::Lit(ast::Lit::Null(_)) => Some(ConstVal::Null),
        ast::Expr::Ident(id) if id.sym.as_str() == "undefined" => Some(ConstVal::Undefined),
        ast::Expr::Tpl(tpl) if tpl.exprs.is_empty() => tpl.quasis.first().map(|q| {
            ConstVal::Str(
                q.cooked
                    .as_ref()
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| q.raw.as_str().to_string()),
            )
        }),
        _ => None,
    }
}

/// Recognize `{ toString: function() { return/throw <expr>; } }` (the only
/// property) and return its retained body statement.
pub(crate) fn object_tostring_body(expr: &ast::Expr) -> Option<ToStringBody> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    let ast::Expr::Object(obj) = e else {
        return None;
    };
    if obj.props.len() != 1 {
        return None;
    }
    let ast::PropOrSpread::Prop(prop) = &obj.props[0] else {
        return None;
    };
    let (key_is_tostring, function) = match prop.as_ref() {
        ast::Prop::KeyValue(kv) => {
            let is_ts = matches!(&kv.key, ast::PropName::Ident(id) if id.sym.as_ref() == "toString")
                || matches!(&kv.key, ast::PropName::Str(s) if s.value.as_str() == Some("toString"));
            let ast::Expr::Fn(fn_expr) = kv.value.as_ref() else {
                return None;
            };
            (is_ts, &fn_expr.function)
        }
        ast::Prop::Method(m) => {
            let is_ts = matches!(&m.key, ast::PropName::Ident(id) if id.sym.as_ref() == "toString")
                || matches!(&m.key, ast::PropName::Str(s) if s.value.as_str() == Some("toString"));
            (is_ts, &m.function)
        }
        _ => return None,
    };
    if !key_is_tostring || !function.params.is_empty() {
        return None;
    }
    let body = function.body.as_ref()?;
    // Leading statements may only be simple `ident = <expr>` side effects
    // (Test262's `toString(){ p = 1; return "a"; }`); the assigned names are
    // poisoned at evaluation time. The final statement is the return/throw.
    let (last, leading) = body.stmts.split_last()?;
    let mut poisoned = Vec::new();
    let mut assigns = Vec::new();
    for stmt in leading {
        let ast::Stmt::Expr(es) = stmt else {
            return None;
        };
        let ast::Expr::Assign(assign) = es.expr.as_ref() else {
            return None;
        };
        let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(b)) = &assign.left else {
            return None;
        };
        // The RHS must be side-effect-free for the skip to be sound.
        let rhs_src = match assign.right.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(st)) => {
                ConstVal::Str(st.value.as_str().unwrap_or("").to_string()).to_js_literal()
            }
            ast::Expr::Lit(ast::Lit::Num(n)) => ConstVal::Num(n.value).to_js_literal(),
            ast::Expr::Lit(ast::Lit::Bool(bl)) => ConstVal::Bool(bl.value).to_js_literal(),
            ast::Expr::Lit(ast::Lit::Null(_)) => "null".to_string(),
            ast::Expr::Ident(id) => id.sym.to_string(),
            _ => return None,
        };
        assigns.push(format!("{} = {};", b.id.sym, rhs_src));
        poisoned.push(b.id.sym.to_string());
    }
    match last {
        ast::Stmt::Return(ret) => ret.arg.as_ref().map(|arg| ToStringBody {
            is_throw: false,
            expr: (**arg).clone(),
            poisoned: poisoned.clone(),
            assigns: assigns.clone(),
        }),
        ast::Stmt::Throw(thr) => Some(ToStringBody {
            is_throw: true,
            expr: (*thr.arg).clone(),
            poisoned,
            assigns,
        }),
        _ => None,
    }
}

/// Count `++name` / `name++` / `--name` updates inside a recorded `toString`
/// body, for counter qualification.
fn count_counter_updates(expr: &ast::Expr, out: &mut HashMap<String, usize>) {
    match expr {
        ast::Expr::Update(u) => {
            if let ast::Expr::Ident(id) = u.arg.as_ref() {
                *out.entry(id.sym.to_string()).or_insert(0) += 1;
            }
            count_counter_updates(&u.arg, out);
        }
        ast::Expr::Bin(b) => {
            count_counter_updates(&b.left, out);
            count_counter_updates(&b.right, out);
        }
        ast::Expr::Paren(p) => count_counter_updates(&p.expr, out),
        _ => {}
    }
}

/// Evaluate a recorded `toString` body against the env's counter state.
/// Returns the produced value or `None` when the body falls outside the
/// modeled subset (in which case the whole fold is abandoned, so any counter
/// mutation already applied is irrelevant — folding won't happen).
pub(crate) fn eval_tostring(env: &mut FnCtorEnv, body: &ToStringBody) -> Option<ResolvedArg> {
    let val = eval_const_expr(env, &body.expr)?;
    env.pending_side_effects
        .extend(body.assigns.iter().cloned());
    for name in &body.poisoned {
        env.entries.remove(name);
        env.counters.remove(name);
    }
    if body.is_throw {
        Some(ResolvedArg::Thrown(val))
    } else {
        Some(ResolvedArg::Str(val.to_js_string()))
    }
}

/// Evaluate a whole `Function(...)` ARGUMENT expression (Bin-add chains over
/// env entries) to the string ToString would produce.
pub(crate) fn eval_arg_expr(env: &mut FnCtorEnv, expr: &ast::Expr) -> Option<String> {
    // Only composite expressions — bare idents/literals are handled (with
    // throw support) by the caller.
    if !matches!(expr, ast::Expr::Bin(_)) {
        return None;
    }
    eval_const_expr(env, expr).map(|v| v.to_js_string())
}

fn eval_const_expr(env: &mut FnCtorEnv, expr: &ast::Expr) -> Option<ConstVal> {
    if let Some(v) = literal_const_val(expr) {
        return Some(v);
    }
    match expr {
        ast::Expr::Paren(p) => eval_const_expr(env, &p.expr),
        ast::Expr::Ident(id) => {
            let name = id.sym.to_string();
            if let Some(v) = env.counters.get(&name) {
                return Some(ConstVal::Num(*v));
            }
            match env.entries.get(&name) {
                Some(FnCtorShape::Str(s)) => Some(ConstVal::Str(s.clone())),
                Some(FnCtorShape::UndefinedVar) => Some(ConstVal::Undefined),
                // An object-with-toString var inside a larger expression
                // (`Function(p + "," + p, …)`) — ToString runs its body,
                // counters and all. Throwing bodies bail (conservative).
                Some(FnCtorShape::ObjToString(body)) => {
                    let body = body.clone();
                    if body.is_throw {
                        return None;
                    }
                    let v = eval_const_expr(env, &body.expr)?;
                    env.pending_side_effects
                        .extend(body.assigns.iter().cloned());
                    for name in &body.poisoned {
                        env.entries.remove(name);
                        env.counters.remove(name);
                    }
                    Some(ConstVal::Str(v.to_js_string()))
                }
                _ => None,
            }
        }
        ast::Expr::Update(u) => {
            let ast::Expr::Ident(id) = u.arg.as_ref() else {
                return None;
            };
            let name = id.sym.to_string();
            let old = *env.counters.get(&name)?;
            let new = match u.op {
                ast::UpdateOp::PlusPlus => old + 1.0,
                ast::UpdateOp::MinusMinus => old - 1.0,
            };
            env.counters.insert(name, new);
            Some(ConstVal::Num(if u.prefix { new } else { old }))
        }
        ast::Expr::Bin(b) if b.op == ast::BinaryOp::Add => {
            let l = eval_const_expr(env, &b.left)?;
            let r = eval_const_expr(env, &b.right)?;
            match (&l, &r) {
                (ConstVal::Num(a), ConstVal::Num(c)) => Some(ConstVal::Num(a + c)),
                _ => Some(ConstVal::Str(format!(
                    "{}{}",
                    l.to_js_string(),
                    r.to_js_string()
                ))),
            }
        }
        _ => None,
    }
}

/// One declarator seen by the scan.
fn record_decl(
    name: &str,
    init: Option<&ast::Expr>,
    decls: &mut HashMap<String, (usize, Option<ast::Expr>)>,
) {
    let entry = decls.entry(name.to_string()).or_insert((0, None));
    entry.0 += 1;
    if entry.0 == 1 {
        entry.1 = init.cloned();
    } else {
        entry.1 = None;
    }
}

/// `var`/function declarations are FUNCTION-scoped: a `for (var i = …)`
/// inside a harness helper must not disqualify the test's module-level
/// `var i` counter. Each function walk extends the shadow set with its
/// params and every name it (re)declares; writes to shadowed names target
/// the inner binding and are not recorded against the module-level one.
type Shadow = std::collections::HashSet<String>;

fn record_write(name: &str, writes: &mut HashMap<String, usize>, shadow: &Shadow) {
    if shadow.contains(name) {
        return;
    }
    *writes.entry(name.to_string()).or_insert(0) += 1;
}

fn collect_pat_names(pat: &ast::Pat, out: &mut Shadow) {
    match pat {
        ast::Pat::Ident(b) => {
            out.insert(b.id.sym.to_string());
        }
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_pat_names(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(a) => {
                        out.insert(a.key.sym.to_string());
                    }
                    ast::ObjectPatProp::KeyValue(kv) => collect_pat_names(&kv.value, out),
                    ast::ObjectPatProp::Rest(r) => collect_pat_names(&r.arg, out),
                }
            }
        }
        ast::Pat::Rest(r) => collect_pat_names(&r.arg, out),
        ast::Pat::Assign(a) => collect_pat_names(&a.left, out),
        _ => {}
    }
}

/// Hoisted names a function body declares (var/function/class declarations,
/// at any block depth but NOT inside nested functions).
fn collect_fn_scope_names(stmts: &[ast::Stmt], out: &mut Shadow) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var)) => {
                for d in &var.decls {
                    collect_pat_names(&d.name, out);
                }
            }
            ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                out.insert(f.ident.sym.to_string());
            }
            ast::Stmt::Decl(ast::Decl::Class(c)) => {
                out.insert(c.ident.sym.to_string());
            }
            ast::Stmt::Block(b) => collect_fn_scope_names(&b.stmts, out),
            ast::Stmt::If(i) => {
                collect_fn_scope_names(std::slice::from_ref(&i.cons), out);
                if let Some(alt) = &i.alt {
                    collect_fn_scope_names(std::slice::from_ref(alt), out);
                }
            }
            ast::Stmt::Try(t) => {
                collect_fn_scope_names(&t.block.stmts, out);
                if let Some(h) = &t.handler {
                    if let Some(p) = &h.param {
                        collect_pat_names(p, out);
                    }
                    collect_fn_scope_names(&h.body.stmts, out);
                }
                if let Some(f) = &t.finalizer {
                    collect_fn_scope_names(&f.stmts, out);
                }
            }
            ast::Stmt::While(w) => collect_fn_scope_names(std::slice::from_ref(&w.body), out),
            ast::Stmt::DoWhile(w) => collect_fn_scope_names(std::slice::from_ref(&w.body), out),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(v)) = &f.init {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
                collect_fn_scope_names(std::slice::from_ref(&f.body), out);
            }
            ast::Stmt::ForIn(f) => {
                if let ast::ForHead::VarDecl(v) = &f.left {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
                collect_fn_scope_names(std::slice::from_ref(&f.body), out);
            }
            ast::Stmt::ForOf(f) => {
                if let ast::ForHead::VarDecl(v) = &f.left {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
                collect_fn_scope_names(std::slice::from_ref(&f.body), out);
            }
            ast::Stmt::Switch(sw) => {
                for case in &sw.cases {
                    collect_fn_scope_names(&case.cons, out);
                }
            }
            ast::Stmt::Labeled(l) => collect_fn_scope_names(std::slice::from_ref(&l.body), out),
            ast::Stmt::With(w) => collect_fn_scope_names(std::slice::from_ref(&w.body), out),
            _ => {}
        }
    }
}

/// Treat every binding identifier in a pattern as a write (catch clauses,
/// destructuring) — it shadows or mutates the name.
fn record_pat_bindings(pat: &ast::Pat, writes: &mut HashMap<String, usize>, shadow: &mut Shadow) {
    match pat {
        ast::Pat::Ident(b) => record_write(&b.id.sym, writes, shadow),
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                record_pat_bindings(elem, writes, shadow);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(a) => {
                        record_write(&a.key.sym, writes, shadow);
                        if let Some(v) = &a.value {
                            scan_expr_writes(v, writes, shadow);
                        }
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        record_pat_bindings(&kv.value, writes, shadow)
                    }
                    ast::ObjectPatProp::Rest(r) => record_pat_bindings(&r.arg, writes, shadow),
                }
            }
        }
        ast::Pat::Rest(r) => record_pat_bindings(&r.arg, writes, shadow),
        ast::Pat::Assign(a) => {
            record_pat_bindings(&a.left, writes, shadow);
            scan_expr_writes(&a.right, writes, shadow);
        }
        _ => {}
    }
}

fn scan_stmt(
    stmt: &ast::Stmt,
    decls: &mut HashMap<String, (usize, Option<ast::Expr>)>,
    writes: &mut HashMap<String, usize>,
    shadow: &mut Shadow,
) {
    match stmt {
        ast::Stmt::Decl(ast::Decl::Var(var)) => {
            for d in &var.decls {
                if let ast::Pat::Ident(b) = &d.name {
                    record_decl(&b.id.sym, d.init.as_deref(), decls);
                } else {
                    record_pat_bindings(&d.name, writes, shadow);
                }
                if let Some(init) = &d.init {
                    scan_expr_writes(init, writes, shadow);
                }
            }
        }
        ast::Stmt::Decl(ast::Decl::Fn(f)) => {
            // The function name itself is a declaration of that name.
            record_write(&f.ident.sym, writes, shadow);
            scan_function_writes(&f.function, writes, shadow);
        }
        ast::Stmt::Decl(ast::Decl::Class(c)) => {
            record_write(&c.ident.sym, writes, shadow);
            scan_class_writes(&c.class, writes, shadow);
        }
        ast::Stmt::Decl(_) => {}
        ast::Stmt::Block(b) => {
            for s in &b.stmts {
                scan_stmt(s, decls, writes, shadow);
            }
        }
        ast::Stmt::If(i) => {
            scan_expr_writes(&i.test, writes, shadow);
            scan_stmt(&i.cons, decls, writes, shadow);
            if let Some(alt) = &i.alt {
                scan_stmt(alt, decls, writes, shadow);
            }
        }
        ast::Stmt::Try(t) => {
            for s in &t.block.stmts {
                scan_stmt(s, decls, writes, shadow);
            }
            if let Some(h) = &t.handler {
                if let Some(p) = &h.param {
                    record_pat_bindings(p, writes, shadow);
                }
                for s in &h.body.stmts {
                    scan_stmt(s, decls, writes, shadow);
                }
            }
            if let Some(f) = &t.finalizer {
                for s in &f.stmts {
                    scan_stmt(s, decls, writes, shadow);
                }
            }
        }
        ast::Stmt::While(w) => {
            scan_expr_writes(&w.test, writes, shadow);
            scan_stmt(&w.body, decls, writes, shadow);
        }
        ast::Stmt::DoWhile(w) => {
            scan_expr_writes(&w.test, writes, shadow);
            scan_stmt(&w.body, decls, writes, shadow);
        }
        ast::Stmt::For(f) => {
            match &f.init {
                Some(ast::VarDeclOrExpr::VarDecl(v)) => scan_stmt(
                    &ast::Stmt::Decl(ast::Decl::Var(v.clone())),
                    decls,
                    writes,
                    shadow,
                ),
                Some(ast::VarDeclOrExpr::Expr(e)) => scan_expr_writes(e, writes, shadow),
                None => {}
            }
            if let Some(t) = &f.test {
                scan_expr_writes(t, writes, shadow);
            }
            if let Some(u) = &f.update {
                scan_expr_writes(u, writes, shadow);
            }
            scan_stmt(&f.body, decls, writes, shadow);
        }
        ast::Stmt::ForIn(f) => {
            scan_for_head_writes(&f.left, writes, shadow);
            scan_expr_writes(&f.right, writes, shadow);
            scan_stmt(&f.body, decls, writes, shadow);
        }
        ast::Stmt::ForOf(f) => {
            scan_for_head_writes(&f.left, writes, shadow);
            scan_expr_writes(&f.right, writes, shadow);
            scan_stmt(&f.body, decls, writes, shadow);
        }
        ast::Stmt::Switch(s) => {
            scan_expr_writes(&s.discriminant, writes, shadow);
            for case in &s.cases {
                if let Some(t) = &case.test {
                    scan_expr_writes(t, writes, shadow);
                }
                for st in &case.cons {
                    scan_stmt(st, decls, writes, shadow);
                }
            }
        }
        ast::Stmt::Labeled(l) => scan_stmt(&l.body, decls, writes, shadow),
        ast::Stmt::Return(r) => {
            if let Some(a) = &r.arg {
                scan_expr_writes(a, writes, shadow);
            }
        }
        ast::Stmt::Throw(t) => scan_expr_writes(&t.arg, writes, shadow),
        ast::Stmt::Expr(e) => scan_expr_writes(&e.expr, writes, shadow),
        ast::Stmt::With(w) => {
            scan_expr_writes(&w.obj, writes, shadow);
            scan_stmt(&w.body, decls, writes, shadow);
        }
        _ => {}
    }
}

fn scan_for_head_writes(
    head: &ast::ForHead,
    writes: &mut HashMap<String, usize>,
    shadow: &mut Shadow,
) {
    match head {
        ast::ForHead::VarDecl(v) => {
            for d in &v.decls {
                record_pat_bindings(&d.name, writes, shadow);
                if let Some(init) = &d.init {
                    scan_expr_writes(init, writes, shadow);
                }
            }
        }
        ast::ForHead::Pat(p) => record_pat_bindings(p, writes, shadow),
        ast::ForHead::UsingDecl(u) => {
            for d in &u.decls {
                record_pat_bindings(&d.name, writes, shadow);
            }
        }
    }
}

fn scan_assign_target_writes(
    target: &ast::AssignTarget,
    writes: &mut HashMap<String, usize>,
    shadow: &mut Shadow,
) {
    match target {
        ast::AssignTarget::Simple(simple) => match simple {
            ast::SimpleAssignTarget::Ident(b) => record_write(&b.id.sym, writes, shadow),
            ast::SimpleAssignTarget::Member(m) => {
                scan_expr_writes(&m.obj, writes, shadow);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    scan_expr_writes(&c.expr, writes, shadow);
                }
            }
            ast::SimpleAssignTarget::Paren(p) => scan_expr_writes(&p.expr, writes, shadow),
            _ => {}
        },
        ast::AssignTarget::Pat(pat) => match pat {
            ast::AssignTargetPat::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    record_pat_bindings(elem, writes, shadow);
                }
            }
            ast::AssignTargetPat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        ast::ObjectPatProp::Assign(a) => record_write(&a.key.sym, writes, shadow),
                        ast::ObjectPatProp::KeyValue(kv) => {
                            record_pat_bindings(&kv.value, writes, shadow)
                        }
                        ast::ObjectPatProp::Rest(r) => record_pat_bindings(&r.arg, writes, shadow),
                    }
                }
            }
            _ => {}
        },
    }
}

/// Walk a nested function for writes to NON-shadowed (module-level) names.
/// The function's params and its own hoisted declarations extend the shadow.
///
/// The shadow is threaded as a single shared `&mut Shadow` that we push the
/// function's own names onto and pop afterwards, instead of `clone()`ing the
/// whole enclosing shadow per nested function. The old clone made scanning a
/// scope of N sibling nested functions O(N²) (each clone copies the ~N
/// enclosing names) — pathological for modules/wrapper-IIFEs that declare many
/// sibling closures (the same class of perf bug as the capture-set rebuild).
/// To restore the shadow exactly, we only remove the names this frame newly
/// inserted (a name already shadowed by an outer scope must stay shadowed).
fn scan_fn_body_writes(
    params: &[&ast::Pat],
    stmts: &[ast::Stmt],
    writes: &mut HashMap<String, usize>,
    shadow: &mut Shadow,
) {
    // Gather the names this function frame introduces (params + hoisted
    // declarations) without disturbing the shared shadow.
    let mut frame_names = Shadow::new();
    for p in params {
        collect_pat_names(p, &mut frame_names);
    }
    collect_fn_scope_names(stmts, &mut frame_names);

    // Insert only the names not already shadowed, remembering them so we can
    // pop exactly this frame's additions and leave outer shadows intact.
    let mut added: Vec<String> = Vec::new();
    for name in frame_names {
        if shadow.insert(name.clone()) {
            added.push(name);
        }
    }

    let mut nested_decls: HashMap<String, (usize, Option<ast::Expr>)> = HashMap::new();
    for s in stmts {
        scan_stmt(s, &mut nested_decls, writes, shadow);
    }

    for name in added {
        shadow.remove(&name);
    }
}

fn scan_function_writes(
    function: &ast::Function,
    writes: &mut HashMap<String, usize>,
    shadow: &mut Shadow,
) {
    let params: Vec<&ast::Pat> = function.params.iter().map(|p| &p.pat).collect();
    let stmts: &[ast::Stmt] = function
        .body
        .as_ref()
        .map(|b| b.stmts.as_slice())
        .unwrap_or(&[]);
    scan_fn_body_writes(&params, stmts, writes, shadow);
}

fn scan_class_writes(class: &ast::Class, writes: &mut HashMap<String, usize>, shadow: &mut Shadow) {
    if let Some(sup) = &class.super_class {
        scan_expr_writes(sup, writes, shadow);
    }
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) => scan_function_writes(&m.function, writes, shadow),
            ast::ClassMember::PrivateMethod(m) => scan_function_writes(&m.function, writes, shadow),
            ast::ClassMember::Constructor(c) => {
                let params: Vec<&ast::Pat> = c
                    .params
                    .iter()
                    .filter_map(|p| match p {
                        ast::ParamOrTsParamProp::Param(p) => Some(&p.pat),
                        _ => None,
                    })
                    .collect();
                let stmts: &[ast::Stmt] =
                    c.body.as_ref().map(|b| b.stmts.as_slice()).unwrap_or(&[]);
                scan_fn_body_writes(&params, stmts, writes, shadow);
            }
            ast::ClassMember::ClassProp(p) => {
                if let Some(v) = &p.value {
                    scan_expr_writes(v, writes, shadow);
                }
            }
            ast::ClassMember::PrivateProp(p) => {
                if let Some(v) = &p.value {
                    scan_expr_writes(v, writes, shadow);
                }
            }
            ast::ClassMember::StaticBlock(b) => {
                scan_fn_body_writes(&[], &b.body.stmts, writes, shadow);
            }
            _ => {}
        }
    }
}

fn scan_expr_writes(expr: &ast::Expr, writes: &mut HashMap<String, usize>, shadow: &mut Shadow) {
    match expr {
        ast::Expr::Assign(a) => {
            scan_assign_target_writes(&a.left, writes, shadow);
            scan_expr_writes(&a.right, writes, shadow);
        }
        ast::Expr::Update(u) => {
            if let ast::Expr::Ident(id) = u.arg.as_ref() {
                record_write(&id.sym, writes, shadow);
            } else {
                scan_expr_writes(&u.arg, writes, shadow);
            }
        }
        ast::Expr::Bin(b) => {
            scan_expr_writes(&b.left, writes, shadow);
            scan_expr_writes(&b.right, writes, shadow);
        }
        ast::Expr::Unary(u) => scan_expr_writes(&u.arg, writes, shadow),
        ast::Expr::Cond(c) => {
            scan_expr_writes(&c.test, writes, shadow);
            scan_expr_writes(&c.cons, writes, shadow);
            scan_expr_writes(&c.alt, writes, shadow);
        }
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(callee) = &c.callee {
                scan_expr_writes(callee, writes, shadow);
            }
            for a in &c.args {
                scan_expr_writes(&a.expr, writes, shadow);
            }
        }
        ast::Expr::New(n) => {
            scan_expr_writes(&n.callee, writes, shadow);
            if let Some(args) = &n.args {
                for a in args {
                    scan_expr_writes(&a.expr, writes, shadow);
                }
            }
        }
        ast::Expr::Member(m) => {
            scan_expr_writes(&m.obj, writes, shadow);
            if let ast::MemberProp::Computed(c) = &m.prop {
                scan_expr_writes(&c.expr, writes, shadow);
            }
        }
        ast::Expr::OptChain(o) => {
            if let ast::OptChainBase::Member(m) = &*o.base {
                scan_expr_writes(&m.obj, writes, shadow);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    scan_expr_writes(&c.expr, writes, shadow);
                }
            } else if let ast::OptChainBase::Call(c) = &*o.base {
                scan_expr_writes(&c.callee, writes, shadow);
                for a in &c.args {
                    scan_expr_writes(&a.expr, writes, shadow);
                }
            }
        }
        ast::Expr::Paren(p) => scan_expr_writes(&p.expr, writes, shadow),
        ast::Expr::Seq(s) => {
            for e in &s.exprs {
                scan_expr_writes(e, writes, shadow);
            }
        }
        ast::Expr::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                scan_expr_writes(&elem.expr, writes, shadow);
            }
        }
        ast::Expr::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Spread(s) => scan_expr_writes(&s.expr, writes, shadow),
                    ast::PropOrSpread::Prop(p) => match p.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            if let ast::PropName::Computed(c) = &kv.key {
                                scan_expr_writes(&c.expr, writes, shadow);
                            }
                            scan_expr_writes(&kv.value, writes, shadow);
                        }
                        ast::Prop::Method(m) => scan_function_writes(&m.function, writes, shadow),
                        ast::Prop::Getter(g) => {
                            let stmts: &[ast::Stmt] =
                                g.body.as_ref().map(|b| b.stmts.as_slice()).unwrap_or(&[]);
                            scan_fn_body_writes(&[], stmts, writes, shadow);
                        }
                        ast::Prop::Setter(st) => {
                            let stmts: &[ast::Stmt] =
                                st.body.as_ref().map(|b| b.stmts.as_slice()).unwrap_or(&[]);
                            scan_fn_body_writes(&[&st.param], stmts, writes, shadow);
                        }
                        ast::Prop::Shorthand(_) => {}
                        ast::Prop::Assign(a) => scan_expr_writes(&a.value, writes, shadow),
                    },
                }
            }
        }
        ast::Expr::Fn(f) => scan_function_writes(&f.function, writes, shadow),
        ast::Expr::Arrow(a) => {
            let params: Vec<&ast::Pat> = a.params.iter().collect();
            match &*a.body {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    scan_fn_body_writes(&params, &b.stmts, writes, shadow);
                }
                ast::BlockStmtOrExpr::Expr(e) => {
                    // Arrow expression body: push the params, scan, then pop —
                    // mirrors `scan_fn_body_writes` so we don't clone the whole
                    // enclosing shadow per arrow.
                    let mut frame_names = Shadow::new();
                    for p in &params {
                        collect_pat_names(p, &mut frame_names);
                    }
                    let mut added: Vec<String> = Vec::new();
                    for name in frame_names {
                        if shadow.insert(name.clone()) {
                            added.push(name);
                        }
                    }
                    scan_expr_writes(e, writes, shadow);
                    for name in added {
                        shadow.remove(&name);
                    }
                }
            }
        }
        ast::Expr::Class(c) => scan_class_writes(&c.class, writes, shadow),
        ast::Expr::Tpl(t) => {
            for e in &t.exprs {
                scan_expr_writes(e, writes, shadow);
            }
        }
        ast::Expr::TaggedTpl(t) => {
            scan_expr_writes(&t.tag, writes, shadow);
            for e in &t.tpl.exprs {
                scan_expr_writes(e, writes, shadow);
            }
        }
        ast::Expr::Await(a) => scan_expr_writes(&a.arg, writes, shadow),
        ast::Expr::Yield(y) => {
            if let Some(a) = &y.arg {
                scan_expr_writes(a, writes, shadow);
            }
        }
        ast::Expr::TsAs(t) => scan_expr_writes(&t.expr, writes, shadow),
        ast::Expr::TsTypeAssertion(t) => scan_expr_writes(&t.expr, writes, shadow),
        ast::Expr::TsNonNull(t) => scan_expr_writes(&t.expr, writes, shadow),
        _ => {}
    }
}
