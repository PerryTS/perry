//! Which class definitions are evaluated AT MOST ONCE.
//!
//! A class nested in a function captures the enclosing function's locals.
//! Each evaluation of the class definition closes over its own environment,
//! so in general the captured values must be reachable per evaluation — Perry
//! stores them on every instance as hidden `__perry_cap_*` fields. When the
//! definition can evaluate only once there is exactly one environment, and it
//! can live with the class instead (`Expr::ClassEnvGet`/`ClassEnvSet`), the way
//! V8 keeps it in the closure context. Instances then carry no hidden capture
//! keys, and identically-declared classes get identically-slotted fields.
//!
//! A position is RUN-ONCE when it executes at most once per program run:
//!
//! * module top level;
//! * inside `if`/`switch`/`try`/blocks/labels and expressions of a run-once
//!   position (each executes at most once);
//! * the body of a function immediately invoked from a run-once position —
//!   `(function(){…})()`, `(() => {…})()`, `(function(){…}).call(…)` — or of
//!   a function DECLARATION whose name occurs exactly twice in the module:
//!   its declaration and one call from a run-once position. The CJS wrapper's
//!   `function __perry_cjs_factory(){…} return __perry_cjs_factory();` and
//!   bundles' `((module) => {…})(…)` are both this shape.
//!
//! Not run-once: loop bodies and heads, class members (a method runs once
//! per call), every other function body, and async/generator bodies (their
//! lowering re-enters the body through a step function). A function whose
//! own `arguments` object is referenced can reach itself through
//! `arguments.callee` and is never treated as invoked once. Names are compared module-wide without scope
//! resolution, which can only make the answer more conservative.
//!
//! The result is keyed by the class node's span. A span seen twice (a
//! synthesized copy) is dropped, as are dummy spans.

use std::collections::{HashMap, HashSet};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Spans `(lo, hi)` of every class (declaration or expression) whose
/// definition is evaluated at most once.
pub(crate) fn run_once_class_spans(module: &ast::Module) -> HashSet<(u32, u32)> {
    let mut counts = IdentCounts::default();
    module.visit_with(&mut counts);
    let mut candidates: HashSet<String> = HashSet::new();
    let mut decls = FnDecls::default();
    module.visit_with(&mut decls);
    for (name, eligible) in decls.eligible {
        if eligible && counts.counts.get(&name).copied() == Some(2) {
            candidates.insert(name);
        }
    }
    // Fixpoint: a function invoked once from a run-once position makes its
    // own body run-once, which can admit further once-called declarations.
    // Monotone (the set only grows), bounded by the nesting depth.
    let mut once_fns: HashSet<String> = HashSet::new();
    for _ in 0..32 {
        let mut walk = Walk::new(&once_fns, &candidates, &counts.counts);
        module.visit_with(&mut walk);
        let next: HashSet<String> = std::mem::take(&mut walk.called_once)
            .into_iter()
            .filter(|n| candidates.contains(n))
            .collect();
        if next == once_fns {
            return walk.spans();
        }
        once_fns = next;
    }
    // Did not converge (cannot happen for a finite nesting depth): claim
    // nothing rather than something unproven.
    HashSet::new()
}

#[derive(Default)]
struct IdentCounts {
    counts: HashMap<String, u32>,
}

impl Visit for IdentCounts {
    fn visit_ident(&mut self, ident: &ast::Ident) {
        *self.counts.entry(ident.sym.to_string()).or_default() += 1;
    }
}

/// Function declarations, and whether each is structurally eligible to be a
/// once-called body (plain, not async/generator, no `callee` reference). A
/// name declared twice is ineligible.
#[derive(Default)]
struct FnDecls {
    eligible: HashMap<String, bool>,
}

impl Visit for FnDecls {
    fn visit_fn_decl(&mut self, decl: &ast::FnDecl) {
        let ok = function_is_plain(&decl.function);
        self.eligible
            .entry(decl.ident.sym.to_string())
            .and_modify(|e| *e = false)
            .or_insert(ok);
        decl.visit_children_with(self);
    }
}

fn function_is_plain(function: &ast::Function) -> bool {
    // Visit the parts, not the `Function` node: `ArgumentsRef` stops at
    // function boundaries, which would skip this function's own body.
    !function.is_async
        && !function.is_generator
        && !names_callee(&function.params)
        && !function.body.as_ref().is_some_and(names_callee)
}

/// Whether `node` can reach its own function object through
/// `arguments.callee`: it names `arguments` outside any nested non-arrow
/// function (those bind their own). Any such reference counts, since the
/// object can escape before `.callee` is read.
fn names_callee<N: VisitWith<ArgumentsRef>>(node: &N) -> bool {
    let mut finder = ArgumentsRef(false);
    node.visit_with(&mut finder);
    finder.0
}

struct ArgumentsRef(bool);

impl Visit for ArgumentsRef {
    fn visit_ident(&mut self, ident: &ast::Ident) {
        if &*ident.sym == "arguments" {
            self.0 = true;
        }
    }
    // Every construct below binds its own `arguments`.
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_constructor(&mut self, _: &ast::Constructor) {}
    fn visit_getter_prop(&mut self, _: &ast::GetterProp) {}
    fn visit_setter_prop(&mut self, _: &ast::SetterProp) {}
}

struct Walk<'a> {
    run_once: bool,
    once_fns: &'a HashSet<String>,
    candidates: &'a HashSet<String>,
    counts: &'a HashMap<String, u32>,
    called_once: HashSet<String>,
    seen: HashSet<(u32, u32)>,
    duplicated: HashSet<(u32, u32)>,
}

impl<'a> Walk<'a> {
    fn new(
        once_fns: &'a HashSet<String>,
        candidates: &'a HashSet<String>,
        counts: &'a HashMap<String, u32>,
    ) -> Self {
        Walk {
            run_once: true,
            once_fns,
            candidates,
            counts,
            called_once: HashSet::new(),
            seen: HashSet::new(),
            duplicated: HashSet::new(),
        }
    }

    fn spans(self) -> HashSet<(u32, u32)> {
        let duplicated = self.duplicated;
        self.seen
            .into_iter()
            .filter(|s| !duplicated.contains(s))
            .collect()
    }

    fn with<F: FnOnce(&mut Self)>(&mut self, run_once: bool, f: F) {
        let saved = self.run_once;
        self.run_once = run_once;
        f(self);
        self.run_once = saved;
    }

    /// The function an immediately-invoked callee evaluates, if the callee is
    /// one: `fn`, `(fn)`, `fn.call`, `(fn).apply`.
    fn iife_target<'e>(&self, callee: &'e ast::Expr) -> Option<Iife<'e>> {
        let callee = strip_parens(callee);
        let target = match callee {
            ast::Expr::Member(member) => match &member.prop {
                ast::MemberProp::Ident(prop) if &*prop.sym == "call" || &*prop.sym == "apply" => {
                    strip_parens(&member.obj)
                }
                _ => return None,
            },
            other => other,
        };
        match target {
            ast::Expr::Fn(fn_expr) => {
                let named_once = fn_expr
                    .ident
                    .as_ref()
                    .is_none_or(|id| self.counts.get(id.sym.as_str()).copied() == Some(1));
                (named_once && function_is_plain(&fn_expr.function))
                    .then_some(Iife::Function(&fn_expr.function))
            }
            ast::Expr::Arrow(arrow) => {
                (!arrow.is_async && !arrow.is_generator && !names_callee(arrow.body.as_ref()))
                    .then_some(Iife::Arrow(arrow))
            }
            _ => None,
        }
    }
}

enum Iife<'e> {
    Function(&'e ast::Function),
    Arrow(&'e ast::ArrowExpr),
}

fn strip_parens(mut expr: &ast::Expr) -> &ast::Expr {
    while let ast::Expr::Paren(paren) = expr {
        expr = &paren.expr;
    }
    expr
}

impl Visit for Walk<'_> {
    fn visit_class(&mut self, class: &ast::Class) {
        if self.run_once && !(class.span.lo.0 == 0 && class.span.hi.0 == 0) {
            let key = (class.span.lo.0, class.span.hi.0);
            if !self.seen.insert(key) {
                self.duplicated.insert(key);
            }
        }
        // Heritage and computed keys evaluate with the definition, but every
        // member body runs once per call: stay conservative for the lot.
        self.with(false, |w| class.visit_children_with(w));
    }

    fn visit_function(&mut self, function: &ast::Function) {
        self.with(false, |w| function.visit_children_with(w));
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.with(false, |w| arrow.visit_children_with(w));
    }

    fn visit_getter_prop(&mut self, prop: &ast::GetterProp) {
        self.with(false, |w| prop.visit_children_with(w));
    }

    fn visit_setter_prop(&mut self, prop: &ast::SetterProp) {
        self.with(false, |w| prop.visit_children_with(w));
    }

    fn visit_fn_decl(&mut self, decl: &ast::FnDecl) {
        let once = self.run_once && self.once_fns.contains(decl.ident.sym.as_str());
        // Bypass `visit_function` so the body keeps the run-once flag.
        self.with(once, |w| decl.function.visit_children_with(w));
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        let ast::Callee::Expr(callee) = &call.callee else {
            call.visit_children_with(self);
            return;
        };
        if self.run_once {
            if let ast::Expr::Ident(id) = strip_parens(callee) {
                if self.candidates.contains(id.sym.as_str()) {
                    self.called_once.insert(id.sym.to_string());
                }
            }
        }
        match self.iife_target(callee) {
            Some(target) if self.run_once => {
                match target {
                    Iife::Function(function) => function.visit_children_with(self),
                    Iife::Arrow(arrow) => arrow.visit_children_with(self),
                }
                // `.call`/`.apply` receivers and the arguments are ordinary
                // expressions of this position.
                call.args.visit_with(self);
            }
            _ => call.visit_children_with(self),
        }
    }

    fn visit_for_stmt(&mut self, node: &ast::ForStmt) {
        self.with(false, |w| node.visit_children_with(w));
    }

    fn visit_for_in_stmt(&mut self, node: &ast::ForInStmt) {
        self.with(false, |w| node.visit_children_with(w));
    }

    fn visit_for_of_stmt(&mut self, node: &ast::ForOfStmt) {
        self.with(false, |w| node.visit_children_with(w));
    }

    fn visit_while_stmt(&mut self, node: &ast::WhileStmt) {
        self.with(false, |w| node.visit_children_with(w));
    }

    fn visit_do_while_stmt(&mut self, node: &ast::DoWhileStmt) {
        self.with(false, |w| node.visit_children_with(w));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_of(src: &str) -> (HashSet<(u32, u32)>, Vec<(String, (u32, u32))>) {
        let module = perry_parser::parse_typescript(src, "run-once.ts").expect("source parses");
        struct Names(Vec<(String, (u32, u32))>);
        impl Visit for Names {
            fn visit_class_decl(&mut self, d: &ast::ClassDecl) {
                self.0.push((
                    d.ident.sym.to_string(),
                    (d.class.span.lo.0, d.class.span.hi.0),
                ));
                d.visit_children_with(self);
            }
            fn visit_class_expr(&mut self, e: &ast::ClassExpr) {
                let name = e
                    .ident
                    .as_ref()
                    .map(|i| i.sym.to_string())
                    .unwrap_or_default();
                self.0.push((name, (e.class.span.lo.0, e.class.span.hi.0)));
                e.visit_children_with(self);
            }
        }
        let mut names = Names(Vec::new());
        module.visit_with(&mut names);
        (run_once_class_spans(&module), names.0)
    }

    fn once(src: &str) -> Vec<String> {
        let (spans, names) = spans_of(src);
        let mut out: Vec<String> = names
            .into_iter()
            .filter(|(_, s)| spans.contains(s))
            .map(|(n, _)| n)
            .collect();
        out.sort();
        out
    }

    #[test]
    fn iife_and_once_called_declaration_bodies_are_run_once() {
        assert_eq!(
            once(
                "(function(){ class A {} })();
                 (() => { class B {} })();
                 (function(){ class C {} }).call(this);
                 (function(){ const callee = 1; function g(){ arguments; } class E {} })();
                 const x = (function(){ function f(){ class D {} } return f(); })();"
            ),
            vec!["A", "B", "C", "D", "E"]
        );
    }

    #[test]
    fn repeatable_positions_are_not_run_once() {
        assert_eq!(
            once(
                "function g(){ class A {} } g(); g();
             function h(){ class B {} } const k = h;
             for (;;) { (function(){ class C {} })(); }
             while (1) { class D {} }
             (async function(){ class E {} })();
             (function*(){ class F {} })();
             (function r(){ class G {} r; })();
             (function(){ arguments.callee; class H {} })();
             (function(){ const a = arguments; class M {} })();
             class K { m(){ (function(){ class I {} })(); } }
             const o = { get p(){ class J {} return 1; } };
             function once(){ class L {} } [1].map(once);"
            ),
            // K itself sits at module top; only its method body repeats.
            vec!["K"]
        );
    }

    #[test]
    fn module_top_and_nested_blocks_are_run_once() {
        assert_eq!(
            once("class A {} if (x) { class B {} } try { class C {} } catch { class D {} }"),
            vec!["A", "B", "C", "D"]
        );
    }
}
