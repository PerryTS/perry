//! Closed-world boundary for source the linked interpreter evaluates.
//!
//! A source site whose text is a compile-time constant is not dynamic: the
//! constant fold (or `static_dynamic_code::specialize`) parses and lowers it as
//! ordinary program code. Only a site whose source is not a constant reaches
//! the interpreter, and its reach is what its code can NAME:
//!
//! - `new Function`/`Function(...)`, indirect eval, `vm.*`, data-URL import:
//!   the global scope only (builtin namespaces, builtin prototypes, the global
//!   object and what is stored on it);
//! - direct `eval`: its lexical scope chain plus the global scope. Bindings of
//!   the enclosing module and functions are visible to the evaluated code, and
//!   a sloppy direct eval may declare `var`s in the enclosing function (that
//!   function's scope is lowered as dynamic, see `lower::dynamic_eval`).
//!
//! Module-private classes, prototypes and families that none of those names
//! lead to keep their facts. The global scope always includes the builtin
//! prototypes, so any site withdraws the builtin-constant facts.
//!
//! A `Function`/`eval` VALUE held as data (get-intrinsic's `%Function%` entry)
//! is not a site; it keeps the interpreter linked (#10421) and nothing else.
use std::collections::BTreeSet;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DynamicCodeReach {
    /// Every non-constant source site, with the scope its code can name.
    pub sites: BTreeSet<String>,
    /// Some site's code can name the global scope (every site can).
    pub globals: bool,
    /// Modules whose bindings a direct eval can name.
    pub lexical_modules: BTreeSet<String>,
}

impl DynamicCodeReach {
    pub fn is_empty(&self) -> bool {
        !self.globals && self.sites.is_empty()
    }
    /// Record a site whose code names only the global scope.
    pub fn record_global(&mut self, site: String) {
        self.globals = true;
        self.sites.insert(site);
    }
    /// Record a direct eval in `module`: its lexical chain plus globals.
    pub fn record_direct(&mut self, module: &str, site: String) {
        self.globals = true;
        self.lexical_modules.insert(module.to_string());
        self.sites.insert(site);
    }
    pub fn merge(&mut self, other: &DynamicCodeReach) {
        self.globals |= other.globals;
        self.sites.extend(other.sites.iter().cloned());
        self.lexical_modules
            .extend(other.lexical_modules.iter().cloned());
    }
    /// A builtin prototype member (`Array.prototype.push`) or namespace member
    /// (`Math.max`) may have been replaced by dynamic code.
    pub fn disables_builtin_constants(&self) -> bool {
        self.globals
    }
    /// A direct eval in `module` names that module's bindings (its classes
    /// and their prototypes among them).
    pub fn reaches_module_bindings(&self, module: &str) -> bool {
        self.lexical_modules.contains(module)
    }
}

/// The spellings of the global object.
fn is_global_object_name(name: &str) -> bool {
    matches!(name, "globalThis" | "global" | "window" | "self")
}

fn site_name(kind: &str, path: &str, span: swc_common::Span) -> String {
    // The scan runs before the module's line table exists: name the byte.
    format!("{kind} @ {path} (byte {})", span.lo.0)
}

fn peel(mut e: &ast::Expr) -> &ast::Expr {
    loop {
        match e {
            ast::Expr::Paren(p) => e = &p.expr,
            ast::Expr::TsAs(x) => e = &x.expr,
            ast::Expr::TsNonNull(x) => e = &x.expr,
            ast::Expr::TsTypeAssertion(x) => e = &x.expr,
            ast::Expr::TsConstAssertion(x) => e = &x.expr,
            ast::Expr::TsSatisfies(x) => e = &x.expr,
            _ => return e,
        }
    }
}

/// The source text of a `Function`/`eval` call when every piece is constant.
pub(crate) fn constant_source(args: &[ast::ExprOrSpread], eval: bool) -> Option<String> {
    let parts = args
        .iter()
        .map(|a| {
            if a.spread.is_some() {
                None
            } else {
                crate::eval_classifier::const_string_of(&a.expr)
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(if eval {
        parts.first().cloned().unwrap_or_default()
    } else {
        let (body, params) = parts
            .split_last()
            .map(|(b, p)| (b.as_str(), p.join(",")))
            .unwrap_or(("", String::new()));
        format!("(function({params}){{\n{body}\n}})")
    })
}

/// Which evaluator a callee names, when it is a fixed spelling of one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Evaluator {
    Function,
    /// `eval` called by its own name: a direct eval.
    DirectEval,
    /// `eval` reached any other way: an indirect eval.
    IndirectEval,
}

/// Names ever bound to `Function`/`eval`. Over-approximate: a reassigned or
/// redeclared name still counts, so a call through it is never missed.
#[derive(Default)]
struct Aliases {
    function: BTreeSet<String>,
    eval: BTreeSet<String>,
    /// Names bound to the global object (`var g = globalThis`).
    global: BTreeSet<String>,
}
impl Visit for Aliases {
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(i), Some(init)) = (&d.name, &d.init) {
            self.bind(&i.id.sym, init);
        }
        d.visit_children_with(self);
    }
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(i)) = &a.left {
            self.bind(&i.id.sym, &a.right);
        }
        a.visit_children_with(self);
    }
}
impl Aliases {
    fn bind(&mut self, name: &str, value: &ast::Expr) {
        match peel(value) {
            ast::Expr::Ident(f) if f.sym.as_ref() == "Function" => {
                self.function.insert(name.to_string());
            }
            ast::Expr::Ident(f) if f.sym.as_ref() == "eval" => {
                self.eval.insert(name.to_string());
            }
            ast::Expr::Ident(f) if is_global_object_name(f.sym.as_ref()) => {
                self.global.insert(name.to_string());
            }
            _ => {}
        }
    }
}

/// Collect the non-constant source sites of `module` into `reach`.
pub fn scan(module: &ast::Module, path: &str, reach: &mut DynamicCodeReach) {
    let mut aliases = Aliases::default();
    module.visit_with(&mut aliases);

    module.visit_with(&mut Scan {
        path,
        reach,
        aliases: &aliases,
        bodies: None,
    });
}

/// The constant source text of every evaluator call in `module` (any
/// spelling, nested sources excluded): program code the fold compiles, whose
/// writes the builtin-write scan must see.
pub fn constant_source_bodies(module: &ast::Module) -> Vec<String> {
    let mut aliases = Aliases::default();
    module.visit_with(&mut aliases);
    let mut reach = DynamicCodeReach::default();
    let mut bodies = Vec::new();
    module.visit_with(&mut Scan {
        path: "",
        reach: &mut reach,
        aliases: &aliases,
        bodies: Some(&mut bodies),
    });
    bodies
}

struct Scan<'a> {
    path: &'a str,
    reach: &'a mut DynamicCodeReach,
    aliases: &'a Aliases,
    /// Collect constant bodies instead of scanning them.
    bodies: Option<&'a mut Vec<String>>,
}

impl Scan<'_> {
    fn evaluator(&self, callee: &ast::Expr) -> Option<Evaluator> {
        match peel(callee) {
            ast::Expr::Ident(i) => {
                let n = i.sym.as_ref();
                if n == "Function" || self.aliases.function.contains(n) {
                    Some(Evaluator::Function)
                } else if n == "eval" {
                    Some(Evaluator::DirectEval)
                } else if self.aliases.eval.contains(n) {
                    Some(Evaluator::IndirectEval)
                } else {
                    None
                }
            }
            // `x.Function` is the constructor on any receiver (lodash's
            // `context.Function`); `x.eval` only on the global object, since
            // any other object's `eval` is its own method (`redis.eval(lua)`).
            ast::Expr::Member(m) => {
                let on_global = matches!(peel(&m.obj), ast::Expr::Ident(o)
                    if is_global_object_name(o.sym.as_ref())
                        || self.aliases.global.contains(o.sym.as_ref()));
                let name = match &m.prop {
                    ast::MemberProp::Ident(p) => Some(p.sym.to_string()),
                    ast::MemberProp::Computed(c) => {
                        crate::eval_classifier::const_string_of(&c.expr)
                    }
                    _ => None,
                };
                match name.as_deref() {
                    Some("Function") => Some(Evaluator::Function),
                    Some("eval") if on_global => Some(Evaluator::IndirectEval),
                    _ => None,
                }
            }
            // `(0, eval)(s)` is an indirect eval.
            ast::Expr::Seq(s) => s.exprs.last().and_then(|e| self.evaluator(e)).map(|e| {
                if e == Evaluator::DirectEval {
                    Evaluator::IndirectEval
                } else {
                    e
                }
            }),
            _ => None,
        }
    }
    fn site(&mut self, evaluator: Evaluator, kind: &str, span: swc_common::Span) {
        let name = site_name(kind, self.path, span);
        if evaluator == Evaluator::DirectEval {
            self.reach.record_direct(
                self.path,
                format!("{name} (names its lexical scope chain and globals)"),
            );
        } else {
            self.reach.record_global(format!("{name} (names globals)"));
        }
    }
    /// A source call: constant pieces are compiled statically, so only
    /// the parsed body is scanned; anything else is a site.
    fn source(
        &mut self,
        evaluator: Evaluator,
        kind: &str,
        args: &[ast::ExprOrSpread],
        span: swc_common::Span,
    ) {
        let eval = evaluator != Evaluator::Function;
        if eval && args.is_empty() {
            return;
        }
        if let Some(source) = constant_source(args, eval) {
            if let Some(bodies) = self.bodies.as_deref_mut() {
                bodies.push(source);
                return;
            }
            if let Ok(parsed) = perry_parser::parse_typescript(&source, "<static source>.cjs") {
                let mut inner_aliases = Aliases::default();
                parsed.visit_with(&mut inner_aliases);
                let mut inner = Scan {
                    path: self.path,
                    reach: &mut *self.reach,
                    aliases: &inner_aliases,
                    bodies: None,
                };
                parsed.visit_with(&mut inner);
                return;
            }
        }
        self.site(evaluator, kind, span);
    }
}
impl Visit for Scan<'_> {
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        match &c.callee {
            ast::Callee::Expr(e) => {
                if let Some(ev) = self.evaluator(e) {
                    let kind = match ev {
                        Evaluator::Function => "Function(...)",
                        Evaluator::DirectEval => "eval(...)",
                        Evaluator::IndirectEval => "indirect eval(...)",
                    };
                    self.source(ev, kind, &c.args, c.span);
                } else if let ast::Expr::Member(m) = peel(e) {
                    let prop = match &m.prop {
                        ast::MemberProp::Ident(p) => Some(p.sym.as_ref()),
                        _ => None,
                    };
                    match prop {
                        // `Function.call(t, src)`, `Function.apply(t, srcs)`,
                        // `eval.call(t, src)`.
                        Some("call" | "apply" | "bind") if self.evaluator(&m.obj).is_some() => self
                            .site(
                                Evaluator::IndirectEval,
                                "Function/eval via call/apply/bind",
                                c.span,
                            ),
                        // `fn.constructor(src)`: a function's constructor is
                        // Function (or AsyncFunction/GeneratorFunction).
                        Some("constructor") if !c.args.is_empty() => {
                            self.site(Evaluator::Function, "x.constructor(...)", c.span)
                        }
                        Some(
                            "runInThisContext" | "runInNewContext" | "runInContext"
                            | "compileFunction",
                        ) => self.site(Evaluator::Function, "vm", c.span),
                        // `Reflect.construct(Function, …)`, `Reflect.apply(eval, …)`.
                        Some("construct" | "apply")
                            if matches!(peel(&m.obj), ast::Expr::Ident(r) if r.sym.as_ref() == "Reflect")
                                && c.args
                                    .first()
                                    .is_some_and(|a| self.evaluator(&a.expr).is_some()) =>
                        {
                            self.site(Evaluator::Function, "Reflect with Function/eval", c.span)
                        }
                        _ => {}
                    }
                }
            }
            ast::Callee::Import(_) => {
                // A runtime specifier can select a data: URL.
                if c.args
                    .first()
                    .and_then(|a| crate::eval_classifier::const_string_of(&a.expr))
                    .is_none_or(|s| s.starts_with("data:"))
                {
                    self.site(Evaluator::Function, "import(...)", c.span);
                }
            }
            ast::Callee::Super(_) => {}
        }
        c.visit_children_with(self);
    }
    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
        let args = n.args.as_deref().unwrap_or(&[]);
        match self.evaluator(&n.callee) {
            Some(Evaluator::Function) => {
                self.source(Evaluator::Function, "new Function(...)", args, n.span)
            }
            Some(_) => {} // `new eval()` throws: eval is not a constructor.
            None => {
                if let ast::Expr::Member(m) = peel(&n.callee) {
                    let prop = match &m.prop {
                        ast::MemberProp::Ident(p) => Some(p.sym.as_ref()),
                        _ => None,
                    };
                    match prop {
                        // `new vm.Script(src)`.
                        Some("Script") if !args.is_empty() => {
                            self.site(Evaluator::Function, "vm.Script", n.span)
                        }
                        // `new (async function () {}).constructor(src)`.
                        Some("constructor")
                            if matches!(peel(&m.obj), ast::Expr::Fn(_) | ast::Expr::Arrow(_))
                                && !args.is_empty() =>
                        {
                            self.site(Evaluator::Function, "new fn.constructor(...)", n.span)
                        }
                        _ => {}
                    }
                }
            }
        }
        n.visit_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reach(source: &str) -> DynamicCodeReach {
        let module = perry_parser::parse_typescript(source, "m.cjs").unwrap();
        let mut r = DynamicCodeReach::default();
        scan(&module, "m.cjs", &mut r);
        r
    }
    #[test]
    fn constant_sources_and_evaluator_values_are_not_sites() {
        for s in [
            "Function('return this')();",
            "new Function('a', 'b', 'return a + b');",
            "eval('1 + 1');",
            "Function('\"use strict\"; return (' + 'async function () {}' + ').constructor;')();",
            "var $Function = Function; var table = { f: $Function };",
            "const F = Function; F('return 1');",
            "var x = (async function () {}).constructor;",
            "declare const redis: any, s: string; redis.eval(s);",
        ] {
            assert!(reach(s).is_empty(), "{s}: {:?}", reach(s));
        }
    }
    #[test]
    fn non_constant_sources_are_sites_with_their_scope() {
        let r = reach("declare const s: string; new Function('a', s);");
        assert!(r.globals && r.lexical_modules.is_empty());
        let r = reach("declare const s: string; const F = Function; F(s);");
        assert!(r.globals && r.lexical_modules.is_empty());
        let r = reach("declare const s: string; (0, eval)(s);");
        assert!(r.globals && r.lexical_modules.is_empty());
        let r = reach("declare const s: string; const e = eval; e(s);");
        assert!(r.globals && r.lexical_modules.is_empty());
        let r = reach("declare const s: string; function f() { return eval(s); }");
        assert!(r.globals && r.lexical_modules.contains("m.cjs"));
        let r = reach("Function(\"return eval(globalThis.s)\");");
        assert!(r.globals, "a site inside constant source counts");
        let r = reach("import vm from 'node:vm'; vm.runInThisContext('1');");
        assert!(r.globals);
        let r = reach("declare const s: string; await import(s);");
        assert!(r.globals);
        let r = reach("declare const s: string; Function.apply(null, ['a', s]);");
        assert!(r.globals);
        let r = reach("declare const s: string; globalThis.eval(s);");
        assert!(r.globals && r.lexical_modules.is_empty());
        let r = reach("declare const s: string; const g = global; g['Function'](s);");
        assert!(r.globals);
    }
}
