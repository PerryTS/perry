//! Enum objects born from a constant layout image.
//!
//! tsc and esbuild write a JS enum as an IIFE over the enum object:
//!
//! ```js
//! var E = ((E2) => { E2[E2["A"] = 0] = "A"; E2["S"] = "s"; return E2; })(E || {});  // esbuild
//! var E; (function (E) { E[E["A"] = 0] = "A"; })(E || (E = {}));                    // tsc
//! ```
//!
//! When the object is FRESH the result is one object whose keys and values
//! the compiler knows: the IIFE is replaced by [`Expr::EnumImage`], which
//! codegen births from the keys' literal layout in one call (the runtime
//! replays the stores instead when `%Object.prototype%` was written, so a
//! user accessor still sees its `[[Set]]`).
//!
//! The object is fresh when `E || {}` can only take the `{}`: `E` is a `var`
//! of the function (or module) the IIFE runs in, whose ONLY writes there are
//! this one (the declarator, or tsc's `var E;` plus the `E = {}` in the
//! argument), and the statement is not in a loop of that function, so every
//! activation reaches it with `E` still `undefined`. Code outside the
//! function cannot name its `var`, so writes are counted in the function's
//! subtree, by name without scope resolution (as `run_once` does), which can
//! only refuse more. A module with direct `eval` or `with` admits nothing.
//!
//! The body must be exactly enum stores with constant keys and values:
//! `P[P["k"] = n] = "k"` (a number member and its reverse entry) and
//! `P["k"] = v` (a string member), then optionally `return P`. A number
//! must be an integer (its reverse key is then its decimal spelling); the key
//! `__proto__` is refused. Everything else keeps the IIFE and is counted
//! by reason (`PERRY_ENUM_IMAGE_CENSUS=1`).

use std::collections::{BTreeMap, HashMap};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use crate::ir::{EnumValue, Expr};
use crate::types::Type;

/// One admitted IIFE: its stores in source order.
#[derive(Debug, Clone)]
pub(crate) struct EnumImagePlan {
    pub(crate) stores: Vec<(String, EnumValue)>,
    /// tsc form: the result is assigned by the argument's `E = {}`.
    pub(crate) assigns: bool,
}

#[derive(Debug, Default)]
pub(crate) struct EnumImageSites {
    /// Keyed by the call's span.
    pub(crate) sites: HashMap<(u32, u32), EnumImagePlan>,
    pub(crate) refused: BTreeMap<&'static str, u32>,
}

pub(crate) fn enum_image_sites(module: &ast::Module) -> EnumImageSites {
    let mut walk = Walk {
        scope: MODULE_SCOPE,
        in_loop: false,
        pending: Vec::new(),
        out: EnumImageSites::default(),
    };
    module.visit_with(&mut walk);
    let mut out = walk.out;
    let mut writes = Writes {
        needed: walk.pending.iter().map(|c| c.scope).collect(),
        stack: Vec::new(),
        scopes: HashMap::new(),
        dynamic_scope: false,
    };
    writes.enter(MODULE_SCOPE);
    module.visit_with(&mut writes);
    writes.stack.pop();
    let mut not_fresh = Vec::new();
    for c in walk.pending {
        let scope = writes.scopes.get(&c.scope);
        let count = scope
            .and_then(|s| s.writes.get(&c.object))
            .copied()
            .unwrap_or(0);
        let is_var = scope.is_some_and(|s| s.vars.contains(&c.object));
        if writes.dynamic_scope {
            *out.refused.entry("eval_or_with").or_default() += 1;
        } else if !is_var || count != c.expected {
            *out.refused.entry("not_fresh").or_default() += 1;
            not_fresh.push(c.object);
        } else if out.sites.insert(c.span, c.plan).is_some() {
            // A span seen twice is a synthesized copy: claim neither.
            out.sites.remove(&c.span);
            *out.refused.entry("span").or_default() += 1;
        }
    }
    if std::env::var_os("PERRY_ENUM_IMAGE_CENSUS").is_some() {
        let refused: Vec<String> = out
            .refused
            .iter()
            .map(|(reason, n)| format!("{reason}={n}"))
            .collect();
        eprintln!(
            "perry enum-image census: admitted={} refused[{}] not_fresh[{}]",
            out.sites.len(),
            refused.join(" "),
            not_fresh.join(",")
        );
    }
    out.sites.shrink_to_fit();
    out
}

/// The span of a function-like node: the scope its `var`s belong to.
type Scope = (u32, u32);
const MODULE_SCOPE: Scope = (u32::MAX, u32::MAX);

fn scope_of(span: swc_common::Span) -> Scope {
    (span.lo.0, span.hi.0)
}

/// An IIFE whose freshness is decided by its scope's writes.
struct Candidate {
    span: (u32, u32),
    plan: EnumImagePlan,
    object: String,
    scope: Scope,
    expected: u32,
}

#[derive(Default)]
struct ScopeWrites {
    /// Per name, the positions in the scope's subtree that can write it.
    writes: HashMap<String, u32>,
    /// Names declared by `var` in the scope itself.
    vars: std::collections::HashSet<String>,
}

/// Counts writes in the subtree of each needed scope.
struct Writes {
    needed: std::collections::HashSet<Scope>,
    /// Every function-like node being visited; `true` when it is needed.
    stack: Vec<(Scope, bool)>,
    scopes: HashMap<Scope, ScopeWrites>,
    dynamic_scope: bool,
}

impl Writes {
    fn enter(&mut self, scope: Scope) {
        let needed = self.needed.contains(&scope);
        if needed {
            self.scopes.entry(scope).or_default();
        }
        self.stack.push((scope, needed));
    }

    fn scoped<F: FnOnce(&mut Self)>(&mut self, scope: Scope, f: F) {
        self.enter(scope);
        f(self);
        self.stack.pop();
    }

    fn add(&mut self, name: &str) {
        for (scope, needed) in &self.stack {
            if *needed {
                let writes = &mut self.scopes.get_mut(scope).unwrap().writes;
                *writes.entry(name.to_string()).or_default() += 1;
            }
        }
    }
}

impl Visit for Writes {
    fn visit_function(&mut self, function: &ast::Function) {
        self.scoped(scope_of(function.span), |w| function.visit_children_with(w));
    }
    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.scoped(scope_of(arrow.span), |w| arrow.visit_children_with(w));
    }
    fn visit_getter_prop(&mut self, prop: &ast::GetterProp) {
        self.scoped(scope_of(prop.span), |w| prop.visit_children_with(w));
    }
    fn visit_setter_prop(&mut self, prop: &ast::SetterProp) {
        self.scoped(scope_of(prop.span), |w| prop.visit_children_with(w));
    }
    fn visit_constructor(&mut self, ctor: &ast::Constructor) {
        self.scoped(scope_of(ctor.span), |w| ctor.visit_children_with(w));
    }
    fn visit_static_block(&mut self, block: &ast::StaticBlock) {
        self.scoped(scope_of(block.span), |w| block.visit_children_with(w));
    }
    fn visit_var_decl(&mut self, decl: &ast::VarDecl) {
        if decl.kind == ast::VarDeclKind::Var {
            if let Some(&(scope, true)) = self.stack.last() {
                let vars = &mut self.scopes.get_mut(&scope).unwrap().vars;
                for d in &decl.decls {
                    if let ast::Pat::Ident(id) = &d.name {
                        vars.insert(id.id.sym.to_string());
                    }
                }
            }
        }
        decl.visit_children_with(self);
    }
    fn visit_binding_ident(&mut self, id: &ast::BindingIdent) {
        // Declarators, parameters, catch bindings, assignment targets and
        // every destructuring pattern.
        self.add(&id.id.sym);
    }
    fn visit_update_expr(&mut self, update: &ast::UpdateExpr) {
        if let ast::Expr::Ident(id) = strip_parens(&update.arg) {
            self.add(&id.sym);
        }
        update.visit_children_with(self);
    }
    fn visit_fn_decl(&mut self, decl: &ast::FnDecl) {
        self.add(&decl.ident.sym);
        decl.visit_children_with(self);
    }
    fn visit_fn_expr(&mut self, expr: &ast::FnExpr) {
        if let Some(id) = &expr.ident {
            self.add(&id.sym);
        }
        expr.visit_children_with(self);
    }
    fn visit_class_decl(&mut self, decl: &ast::ClassDecl) {
        self.add(&decl.ident.sym);
        decl.visit_children_with(self);
    }
    fn visit_class_expr(&mut self, expr: &ast::ClassExpr) {
        if let Some(id) = &expr.ident {
            self.add(&id.sym);
        }
        expr.visit_children_with(self);
    }
    fn visit_import_named_specifier(&mut self, s: &ast::ImportNamedSpecifier) {
        self.add(&s.local.sym);
    }
    fn visit_import_default_specifier(&mut self, s: &ast::ImportDefaultSpecifier) {
        self.add(&s.local.sym);
    }
    fn visit_import_star_as_specifier(&mut self, s: &ast::ImportStarAsSpecifier) {
        self.add(&s.local.sym);
    }
    fn visit_ts_enum_decl(&mut self, decl: &ast::TsEnumDecl) {
        self.add(&decl.id.sym);
        decl.visit_children_with(self);
    }
    fn visit_ts_module_decl(&mut self, decl: &ast::TsModuleDecl) {
        if let ast::TsModuleName::Ident(id) = &decl.id {
            self.add(&id.sym);
        }
        decl.visit_children_with(self);
    }
    fn visit_ident(&mut self, id: &ast::Ident) {
        if &*id.sym == "eval" {
            self.dynamic_scope = true;
        }
    }
    fn visit_with_stmt(&mut self, stmt: &ast::WithStmt) {
        self.dynamic_scope = true;
        stmt.visit_children_with(self);
    }
}

struct Walk {
    scope: Scope,
    in_loop: bool,
    pending: Vec<Candidate>,
    out: EnumImageSites,
}

impl Walk {
    fn with<F: FnOnce(&mut Self)>(&mut self, in_loop: bool, f: F) {
        let saved = self.in_loop;
        self.in_loop = in_loop;
        f(self);
        self.in_loop = saved;
    }

    fn function<F: FnOnce(&mut Self)>(&mut self, scope: Scope, f: F) {
        let saved = self.scope;
        self.scope = scope;
        self.with(false, f);
        self.scope = saved;
    }

    fn refuse(&mut self, reason: &'static str) {
        *self.out.refused.entry(reason).or_default() += 1;
    }

    /// `call` is an enum IIFE whose object is `name`: esbuild's `name || {}`
    /// under a `var name =` declarator, or (no `name`) tsc's
    /// `E || (E = {})` statement.
    fn consider(&mut self, call: &ast::CallExpr, name: Option<&str>) {
        let Some((param, body)) = enum_iife(call) else {
            return;
        };
        let stores = match enum_stores(param, body) {
            Ok(stores) => stores,
            Err(reason) => return self.refuse(reason),
        };
        let Some((object, assigns)) = enum_argument(call) else {
            return self.refuse("argument");
        };
        if name.is_some_and(|name| name != object) || name.is_some() == assigns {
            return self.refuse("argument");
        }
        if self.in_loop {
            return self.refuse("in_loop");
        }
        // esbuild: the declarator. tsc: `var E;` and the argument's
        // `E = {}`, plus the IIFE's own parameter when it reuses the name.
        let expected = if assigns { 2 } else { 1 } + u32::from(param == object);
        self.pending.push(Candidate {
            span: (call.span.lo.0, call.span.hi.0),
            plan: EnumImagePlan { stores, assigns },
            object: object.to_string(),
            scope: self.scope,
            expected,
        });
    }
}

impl Visit for Walk {
    fn visit_function(&mut self, function: &ast::Function) {
        self.function(scope_of(function.span), |w| function.visit_children_with(w));
    }
    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.function(scope_of(arrow.span), |w| arrow.visit_children_with(w));
    }
    fn visit_getter_prop(&mut self, prop: &ast::GetterProp) {
        self.function(scope_of(prop.span), |w| prop.visit_children_with(w));
    }
    fn visit_setter_prop(&mut self, prop: &ast::SetterProp) {
        self.function(scope_of(prop.span), |w| prop.visit_children_with(w));
    }
    fn visit_constructor(&mut self, ctor: &ast::Constructor) {
        self.function(scope_of(ctor.span), |w| ctor.visit_children_with(w));
    }
    fn visit_static_block(&mut self, block: &ast::StaticBlock) {
        self.function(scope_of(block.span), |w| block.visit_children_with(w));
    }
    fn visit_for_stmt(&mut self, node: &ast::ForStmt) {
        self.with(true, |w| node.visit_children_with(w));
    }
    fn visit_for_in_stmt(&mut self, node: &ast::ForInStmt) {
        self.with(true, |w| node.visit_children_with(w));
    }
    fn visit_for_of_stmt(&mut self, node: &ast::ForOfStmt) {
        self.with(true, |w| node.visit_children_with(w));
    }
    fn visit_while_stmt(&mut self, node: &ast::WhileStmt) {
        self.with(true, |w| node.visit_children_with(w));
    }
    fn visit_do_while_stmt(&mut self, node: &ast::DoWhileStmt) {
        self.with(true, |w| node.visit_children_with(w));
    }

    fn visit_var_decl(&mut self, decl: &ast::VarDecl) {
        // Only `var`: `let E = (...)(E || {})` reads `E` in its dead zone.
        if decl.kind == ast::VarDeclKind::Var {
            for d in &decl.decls {
                if let (ast::Pat::Ident(name), Some(init)) = (&d.name, d.init.as_deref()) {
                    if let ast::Expr::Call(call) = strip_parens(init) {
                        self.consider(call, Some(&name.id.sym));
                    }
                }
            }
        }
        decl.visit_children_with(self);
    }

    fn visit_expr_stmt(&mut self, stmt: &ast::ExprStmt) {
        if let ast::Expr::Call(call) = strip_parens(&stmt.expr) {
            self.consider(call, None);
        }
        stmt.visit_children_with(self);
    }
}

fn strip_parens(mut expr: &ast::Expr) -> &ast::Expr {
    while let ast::Expr::Paren(paren) = expr {
        expr = &paren.expr;
    }
    expr
}

/// The IIFE's single parameter and body statements: a plain arrow or
/// function expression of one identifier parameter, called with one argument.
fn enum_iife(call: &ast::CallExpr) -> Option<(&str, &[ast::Stmt])> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let (param, body) = match strip_parens(callee) {
        ast::Expr::Arrow(arrow) if !arrow.is_async && !arrow.is_generator => {
            let [ast::Pat::Ident(param)] = arrow.params.as_slice() else {
                return None;
            };
            let ast::ArrowFunctionBody::FunctionBody(body) = arrow.body.as_ref() else {
                return None;
            };
            (param, body.stmts.as_slice())
        }
        ast::Expr::Fn(f)
            if !f.function.is_async && !f.function.is_generator && f.ident.is_none() =>
        {
            let [param] = f.function.params.as_slice() else {
                return None;
            };
            if !param.decorators.is_empty() {
                return None;
            }
            let (ast::Pat::Ident(param), Some(body)) = (&param.pat, f.function.body.as_ref())
            else {
                return None;
            };
            (param, body.stmts.as_slice())
        }
        _ => return None,
    };
    // Only an IIFE that starts like an enum is a candidate (and counted).
    let first = body.first()?;
    let ast::Stmt::Expr(first) = first else {
        return None;
    };
    let ast::Expr::Assign(assign) = strip_parens(&first.expr) else {
        return None;
    };
    member_of(&assign.left, &param.id.sym)?;
    Some((&param.id.sym, body))
}

/// `P[key]` / `P.key` as an assignment target: the member's object is `P`.
fn member_of<'a>(target: &'a ast::AssignTarget, param: &str) -> Option<&'a ast::MemberProp> {
    let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) = target else {
        return None;
    };
    match strip_parens(&member.obj) {
        ast::Expr::Ident(id) if &*id.sym == param => Some(&member.prop),
        _ => None,
    }
}

fn enum_stores(param: &str, body: &[ast::Stmt]) -> Result<Vec<(String, EnumValue)>, &'static str> {
    let mut stores = Vec::new();
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            ast::Stmt::Return(ret) if i + 1 == body.len() => {
                match ret.arg.as_deref().map(strip_parens) {
                    Some(ast::Expr::Ident(id)) if &*id.sym == param => continue,
                    _ => return Err("body"),
                }
            }
            ast::Stmt::Expr(e) => {
                let ast::Expr::Assign(outer) = strip_parens(&e.expr) else {
                    return Err("body");
                };
                if outer.op != ast::AssignOp::Assign {
                    return Err("body");
                }
                let prop = member_of(&outer.left, param).ok_or("body")?;
                // `P[P["k"] = n] = "k"`: the member and its reverse entry.
                if let ast::MemberProp::Computed(c) = prop {
                    if let ast::Expr::Assign(inner) = strip_parens(&c.expr) {
                        if inner.op != ast::AssignOp::Assign {
                            return Err("body");
                        }
                        let key = static_key(member_of(&inner.left, param).ok_or("body")?)
                            .ok_or("body")?;
                        let n = number(&inner.right).ok_or("value")?;
                        match strip_parens(&outer.right) {
                            ast::Expr::Lit(ast::Lit::Str(s))
                                if s.value.as_str() == Some(key.as_str()) => {}
                            _ => return Err("value"),
                        }
                        if key == "__proto__" {
                            return Err("proto_key");
                        }
                        stores.push((key.clone(), EnumValue::Number(n)));
                        stores.push((crate::js_number_to_string(n), EnumValue::String(key)));
                        continue;
                    }
                }
                // `P["k"] = v`.
                let key = static_key(prop).ok_or("body")?;
                if key == "__proto__" {
                    return Err("proto_key");
                }
                let value = match strip_parens(&outer.right) {
                    ast::Expr::Lit(ast::Lit::Str(s)) => {
                        EnumValue::String(s.value.as_str().ok_or("value")?.to_string())
                    }
                    other => EnumValue::Number(number(other).ok_or("value")?),
                };
                stores.push((key, value));
            }
            _ => return Err("body"),
        }
    }
    if stores.is_empty() {
        return Err("body");
    }
    Ok(stores)
}

/// A constant property key: `["k"]`, `.k`, or a numeric `[n]`.
fn static_key(prop: &ast::MemberProp) -> Option<String> {
    match prop {
        ast::MemberProp::Ident(id) => Some(id.sym.to_string()),
        ast::MemberProp::Computed(c) => match strip_parens(&c.expr) {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().map(str::to_string),
            other => number(other).map(|n| crate::js_number_to_string(n)),
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

/// A number literal, optionally signed. Keep its exact IEEE-754 value.
fn number(expr: &ast::Expr) -> Option<f64> {
    match strip_parens(expr) {
        ast::Expr::Lit(ast::Lit::Num(n)) => Some(n.value),
        ast::Expr::Unary(u) if u.op == ast::UnaryOp::Minus => Some(-number(&u.arg)?),
        ast::Expr::Unary(u) if u.op == ast::UnaryOp::Plus => number(&u.arg),
        _ => None,
    }
}

/// The argument's object name and whether it assigns: `E || {}` (false) or
/// `E || (E = {})` (true), `{}` empty.
fn enum_argument(call: &ast::CallExpr) -> Option<(&str, bool)> {
    let ast::Expr::Bin(bin) = strip_parens(&call.args[0].expr) else {
        return None;
    };
    if bin.op != ast::BinaryOp::LogicalOr {
        return None;
    }
    let ast::Expr::Ident(object) = strip_parens(&bin.left) else {
        return None;
    };
    let empty =
        |e: &ast::Expr| matches!(strip_parens(e), ast::Expr::Object(o) if o.props.is_empty());
    match strip_parens(&bin.right) {
        e if empty(e) => Some((&object.sym, false)),
        ast::Expr::Assign(assign) if assign.op == ast::AssignOp::Assign && empty(&assign.right) => {
            match &assign.left {
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(id))
                    if id.id.sym == object.sym =>
                {
                    Some((&object.sym, true))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The admitted IIFE `call` as [`Expr::EnumImage`] (tsc's form: assigned
/// to its variable by the argument's own `E = {}`), or `None` to lower it as
/// written.
pub(crate) fn lower_enum_image(
    ctx: &mut super::LoweringContext,
    call: &ast::CallExpr,
) -> anyhow::Result<Option<Expr>> {
    let Some(plan) = ctx.enum_image_sites.get(&(call.span.lo.0, call.span.hi.0)) else {
        return Ok(None);
    };
    let plan = plan.clone();
    // The keys the stores leave, in first-store order, typed by last value.
    let mut keys: Vec<(String, Type)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (key, value) in &plan.stores {
        let ty = match value {
            EnumValue::Number(_) | EnumValue::Computed => Type::Number,
            EnumValue::String(_) => Type::String,
        };
        match at.get(key.as_str()) {
            Some(&i) => keys[i].1 = ty,
            None => {
                at.insert(key, keys.len());
                keys.push((key.clone(), ty));
            }
        }
    }
    let class_name = ctx.synthesize_anon_shape_class(&keys);
    let mut image = Some(Expr::EnumImage {
        class_name,
        stores: plan.stores,
        // The body is stores only (no directive): the call site's mode.
        strict: ctx.current_strict_mode(),
    });
    if !plan.assigns {
        return Ok(image);
    }
    // `E || (E = {})`: lower the assignment and put the image where its
    // `{}` went, so `E` is written exactly as the argument writes it.
    let ast::Expr::Bin(bin) = strip_parens(&call.args[0].expr) else {
        return Ok(None);
    };
    let ast::Expr::Assign(assign) = strip_parens(&bin.right) else {
        return Ok(None);
    };
    let mut lowered = super::expr_assign::lower_assign(ctx, assign)?;
    let mut replaced = 0;
    fn put(expr: &mut Expr, image: &mut Option<Expr>, replaced: &mut u32) {
        let empty = match expr {
            Expr::New {
                class_name, args, ..
            } => args.is_empty() && class_name.starts_with("__AnonShape_"),
            Expr::Object(fields) => fields.is_empty(),
            _ => false,
        };
        if empty {
            if let Some(image) = image.take() {
                *expr = image;
            }
            *replaced += 1;
            return;
        }
        crate::walker::walk_expr_children_mut(expr, &mut |child| put(child, image, replaced));
    }
    put(&mut lowered, &mut image, &mut replaced);
    Ok((replaced == 1 && image.is_none()).then_some(lowered))
}

#[cfg(test)]
mod tests;
