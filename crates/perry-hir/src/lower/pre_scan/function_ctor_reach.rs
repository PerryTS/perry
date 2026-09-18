//! #10421: can this module reach the `Function` constructor as a VALUE?
//!
//! The auto-optimized runtime links the `dyn-eval` interpreter only when the
//! compile records a reason to (`eval_classifier::has_deferred_dynamic_code_sites`).
//! Literal `new Function(...)` / `Function(...)` sites record it from their own
//! lowering. A constructor reached any other way never passed through that
//! lowering, so the program compiled, linked a runtime without the interpreter,
//! and threw `dynamic code generation ... is not supported` at the first call —
//! while a `PERRY_NO_AUTO_OPTIMIZE=1` build of the same program worked.
//!
//! This scan names those other ways. It is deliberately over-approximate: a
//! false positive links an interpreter the program never calls (binary size),
//! a false negative breaks the program at runtime.
//!
//! - any value use of the identifier `Function` (`const F = Function`,
//!   `Reflect.construct(Function, …)`, `module.exports = Function`,
//!   `Function.bind(…)`, `class X extends Function`), except the uses that
//!   cannot construct: `Function.prototype` / `.name` / `.length`,
//!   `typeof Function`, `x instanceof Function`, `x === Function`. The literal
//!   `Function(…)`, `Function.call(…)`, `Function.apply(…)` and
//!   `new Function(…)` callees are left to their lowering, which records the
//!   site only when the constant fold did not compile it;
//! - a property named `Function` on anything (`globalThis.Function`,
//!   lodash's `var Function = context.Function`, `{ Function: F } = …`);
//! - `globalThis[key]` / `global[key]` with a key that is not a literal;
//! - `.constructor` read on something that is statically a function (a
//!   function, arrow or class expression, `Object.getPrototypeOf` of one, or a
//!   name declared as one), and any `x.constructor(…)` call — calling a
//!   constructor without `new` only produces a value for a plain function,
//!   whose `constructor` is `Function`.

use std::collections::HashSet;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Record the module for the auto-optimize `dyn-eval` decision when it can
/// reach the `Function` constructor through a value.
pub(crate) fn pre_scan_function_ctor_reach(ast_module: &ast::Module) {
    // Once any module needs the interpreter the answer cannot change.
    if crate::eval_classifier::has_deferred_dynamic_code_sites() {
        return;
    }
    if module_reaches_function_ctor_value(ast_module) {
        crate::eval_classifier::note_dynamic_function_reachable();
    }
}

pub(crate) fn module_reaches_function_ctor_value(ast_module: &ast::Module) -> bool {
    let mut scan = Scan::default();
    ast_module.visit_with(&mut scan);
    scan.found
        || scan
            .constructor_receivers
            .iter()
            .any(|name| scan.function_names.contains(name))
}

#[derive(Default)]
struct Scan {
    found: bool,
    /// Names declared as a function / class, or initialized with a function,
    /// arrow or class expression.
    function_names: HashSet<String>,
    /// Identifiers whose `.constructor` is read. Resolved against
    /// `function_names` after the walk, since declarations hoist.
    constructor_receivers: Vec<String>,
}

fn peel(mut e: &ast::Expr) -> &ast::Expr {
    loop {
        match e {
            ast::Expr::Paren(p) => e = &p.expr,
            ast::Expr::TsAs(x) => e = &x.expr,
            ast::Expr::TsTypeAssertion(x) => e = &x.expr,
            ast::Expr::TsNonNull(x) => e = &x.expr,
            ast::Expr::TsConstAssertion(x) => e = &x.expr,
            ast::Expr::TsSatisfies(x) => e = &x.expr,
            _ => return e,
        }
    }
}

fn is_ident(e: &ast::Expr, name: &str) -> bool {
    matches!(peel(e), ast::Expr::Ident(id) if id.sym.as_ref() == name)
}

/// The static name of a member property: `.name` or `["name"]`.
fn member_prop_name(m: &ast::MemberExpr) -> Option<&str> {
    match &m.prop {
        ast::MemberProp::Ident(id) => Some(id.sym.as_ref()),
        ast::MemberProp::Computed(c) => match peel(&c.expr) {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

/// `recv` when `e` is a plain `recv.constructor` read.
fn constructor_read_receiver(e: &ast::Expr) -> Option<&ast::Expr> {
    match peel(e) {
        ast::Expr::Member(m) if member_prop_name(m) == Some("constructor") => Some(m.obj.as_ref()),
        _ => None,
    }
}

fn is_function_like(e: &ast::Expr) -> bool {
    match peel(e) {
        ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_) => true,
        // `Function.constructor`, `Function.prototype.constructor`.
        ast::Expr::Ident(id) => id.sym.as_ref() == "Function",
        ast::Expr::Member(m) => match member_prop_name(m) {
            Some("prototype") => is_ident(&m.obj, "Function"),
            Some("__proto__") => is_function_like(&m.obj),
            _ => false,
        },
        // `Object.getPrototypeOf(async function () {})` — the AsyncFunction /
        // GeneratorFunction constructor idiom.
        ast::Expr::Call(call) => {
            let ast::Callee::Expr(callee) = &call.callee else {
                return false;
            };
            let ast::Expr::Member(m) = peel(callee) else {
                return false;
            };
            member_prop_name(m) == Some("getPrototypeOf")
                && (is_ident(&m.obj, "Object") || is_ident(&m.obj, "Reflect"))
                && call.args.first().is_some_and(|a| is_function_like(&a.expr))
        }
        _ => false,
    }
}

impl Visit for Scan {
    fn visit_expr(&mut self, expr: &ast::Expr) {
        if self.found {
            return;
        }
        if let ast::Expr::Ident(id) = expr {
            if id.sym.as_ref() == "Function" {
                self.found = true;
            }
            return;
        }
        expr.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            let callee = peel(callee);
            let literal_ctor_call = is_ident(callee, "Function")
                || matches!(callee, ast::Expr::Member(m)
                    if is_ident(&m.obj, "Function")
                        && matches!(member_prop_name(m), Some("call" | "apply")));
            if literal_ctor_call {
                call.args.visit_with(self);
                return;
            }
            if let ast::Expr::Member(m) = callee {
                if member_prop_name(m) == Some("constructor") && !call.args.is_empty() {
                    self.found = true;
                    return;
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, new_expr: &ast::NewExpr) {
        if is_ident(&new_expr.callee, "Function") {
            new_expr.args.visit_with(self);
            return;
        }
        new_expr.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        match member_prop_name(m) {
            Some("Function") => {
                self.found = true;
                return;
            }
            Some("constructor") => {
                if is_function_like(&m.obj) {
                    self.found = true;
                    return;
                }
                if let ast::Expr::Ident(id) = peel(&m.obj) {
                    self.constructor_receivers.push(id.sym.to_string());
                }
            }
            Some("prototype" | "name" | "length") if is_ident(&m.obj, "Function") => return,
            // `x.constructor.name` only inspects the constructor.
            Some("name") => {
                if let Some(receiver) = constructor_read_receiver(&m.obj) {
                    receiver.visit_with(self);
                    return;
                }
            }
            _ => {}
        }
        if let ast::MemberProp::Computed(c) = &m.prop {
            if !matches!(peel(&c.expr), ast::Expr::Lit(_))
                && (is_ident(&m.obj, "globalThis") || is_ident(&m.obj, "global"))
            {
                self.found = true;
                return;
            }
        }
        m.visit_children_with(self);
    }

    fn visit_unary_expr(&mut self, u: &ast::UnaryExpr) {
        if u.op == ast::UnaryOp::TypeOf && is_ident(&u.arg, "Function") {
            return;
        }
        u.visit_children_with(self);
    }

    fn visit_bin_expr(&mut self, b: &ast::BinExpr) {
        if matches!(
            b.op,
            ast::BinaryOp::InstanceOf
                | ast::BinaryOp::EqEq
                | ast::BinaryOp::NotEq
                | ast::BinaryOp::EqEqEq
                | ast::BinaryOp::NotEqEq
        ) {
            for side in [&b.left, &b.right] {
                if is_ident(side, "Function") {
                    continue;
                }
                // `x.constructor === Y` compares the constructor, it does not
                // hand it on.
                match constructor_read_receiver(side) {
                    Some(receiver) => receiver.visit_with(self),
                    None => side.as_ref().visit_with(self),
                }
            }
            return;
        }
        b.visit_children_with(self);
    }

    fn visit_prop(&mut self, p: &ast::Prop) {
        if let ast::Prop::Shorthand(id) = p {
            if id.sym.as_ref() == "Function" {
                self.found = true;
                return;
            }
        }
        p.visit_children_with(self);
    }

    fn visit_object_pat_prop(&mut self, p: &ast::ObjectPatProp) {
        let key_is_function = match p {
            ast::ObjectPatProp::KeyValue(kv) => match &kv.key {
                ast::PropName::Ident(id) => id.sym.as_ref() == "Function",
                ast::PropName::Str(s) => s.value.as_str() == Some("Function"),
                _ => false,
            },
            ast::ObjectPatProp::Assign(a) => a.key.id.sym.as_ref() == "Function",
            ast::ObjectPatProp::Rest(_) => false,
        };
        if key_is_function {
            self.found = true;
            return;
        }
        p.visit_children_with(self);
    }

    fn visit_fn_decl(&mut self, f: &ast::FnDecl) {
        self.function_names.insert(f.ident.sym.to_string());
        f.visit_children_with(self);
    }

    fn visit_class_decl(&mut self, c: &ast::ClassDecl) {
        self.function_names.insert(c.ident.sym.to_string());
        c.visit_children_with(self);
    }

    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(binding), Some(init)) = (&d.name, &d.init) {
            if matches!(
                peel(init),
                ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_)
            ) {
                self.function_names.insert(binding.id.sym.to_string());
            }
        }
        d.visit_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use super::module_reaches_function_ctor_value;

    fn reaches(source: &str) -> bool {
        let module = perry_parser::parse_typescript(source, "function-ctor-reach.ts")
            .unwrap_or_else(|e| panic!("fixture must parse: {e}\n{source}"));
        module_reaches_function_ctor_value(&module)
    }

    #[test]
    fn value_uses_of_the_constructor_reach_it() {
        for source in [
            "const F = Function; new F('a', 'return a');",
            "const F: any = Function; F('return 1');",
            "Reflect.construct(Function, ['return 1']);",
            "module.exports = Function;",
            "Function.bind(null, 'a')('return a');",
            "new (globalThis.Function)('return 1');",
            "new globalThis['Function']('return 1');",
            "function lodash(context: any) { var Function = context.Function; }",
            "const { Function: F } = globalThis;",
            "const { Function } = globalThis as any;",
            "const o = { Function };",
            "new (globalThis['Func' + 'tion'])('return 1');",
            "const k = 'Function'; global[k];",
            "class X extends Function {}",
            "(function () {}).constructor('return 1');",
            "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;",
            "const G = (function* () {}).constructor;",
            "function f() {} const F = f.constructor;",
            "const g = () => 1; const F = (g as any).constructor;",
            "declare const x: any; x.constructor('return 1');",
            "const F = Function.prototype.constructor;",
            "Function.call.bind(Function);",
            "(Function as any)?.bind(null);",
        ] {
            assert!(
                reaches(source),
                "must reach the Function constructor: {source}"
            );
        }
    }

    /// The common non-constructing uses must stay free: they are everywhere in
    /// npm code, and each one linking the interpreter would grow the binary.
    #[test]
    fn non_constructing_uses_do_not_reach_it() {
        for source in [
            "console.log('hello');",
            "const s = Function.prototype.toString.call(() => 1);",
            "const bind = Function.prototype.bind;",
            "const n = Function.name + Function.length;",
            "const t = typeof Function;",
            "declare const v: unknown; const ok = v instanceof Function;",
            "declare const c: unknown; const same = c === Function || Function !== c;",
            "class A { clone() { return new (this.constructor as any)(); } }",
            "declare const o: any; const C = o.constructor; const k = o.constructor.name;",
            "const f = () => 1; const same = f.constructor === Function;",
            "function g() {} const n = g.constructor.name;",
            "const n = (async function () {}).constructor.name;",
            "const g = globalThis['process'];",
            "declare const self: any, key: string; self[key];",
            "let x: Function = () => 1;",
            // Literal constructor sites are recorded by their own lowering.
            "new Function('a', 'return a');",
            "Function('return 1');",
            "declare const body: string; Function.apply(null, ['a', body]);",
            "declare const body: string; Function.call(null, 'a', body);",
        ] {
            assert!(
                !reaches(source),
                "must not reach the Function constructor: {source}"
            );
        }
    }

    #[test]
    fn literal_constructor_arguments_are_still_scanned() {
        assert!(reaches("new Function(Function.bind(null), 'return 1');"));
        assert!(reaches("Function('a', (globalThis as any).Function);"));
    }
}
