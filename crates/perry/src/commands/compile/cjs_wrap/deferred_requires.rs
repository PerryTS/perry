//! Preserve the evaluation boundary of conditional and function-local requires.

use std::collections::{HashMap, HashSet};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Synthetic imports collect the target, but must not evaluate it before a
/// conditional branch or a function actually calls `require`. A specifier with
/// any unconditional occurrence keeps the existing eager/alias-adoption path.
/// Use the AST: brace scanning misses concise arrows, unbraced branches, and
/// short-circuit expressions. On a parse failure retain the existing scanner's
/// function-local classification (some CJS sources need wrapping to parse).
#[cfg(test)]
fn deferred_require_specs(source: &str) -> HashSet<String> {
    analyze_require_specs(source, std::path::Path::new(""), &mut Vec::new(), None)
}

/// Recoverable parse warnings go to `messages` and computed-require warnings
/// to `computed_require_warnings` (not looked for when `None`) for the caller
/// to print: this runs on source-preparation worker threads, and stderr must
/// keep the module walk's order.
pub(super) fn deferred_require_specs_for_path(
    source: &str,
    path: &std::path::Path,
    messages: &mut Vec<String>,
    computed_require_warnings: Option<&mut Vec<String>>,
) -> RequireSites {
    analyze(source, path, messages, computed_require_warnings)
}

/// What one parse of a CommonJS source says about its literal requires.
#[derive(Default)]
pub(super) struct RequireSites {
    /// Specifiers whose every call is conditional or inside a function.
    pub(super) deferred: HashSet<String>,
    /// `require('<literal>')` calls whose callee is the wrapper's own
    /// `require` (no binding of that name in any enclosing scope): byte
    /// range of the call in `source`, and its specifier. The wrapper turns
    /// these into static edges (design CJS_LINK §2.2). Empty on a parse
    /// failure: every call then keeps the `require` function.
    pub(super) calls: Vec<(usize, usize, String)>,
}

fn analyze_require_specs(
    source: &str,
    path: &std::path::Path,
    messages: &mut Vec<String>,
    computed_require_warnings: Option<&mut Vec<String>>,
) -> HashSet<String> {
    analyze(source, path, messages, computed_require_warnings).deferred
}

fn analyze(
    source: &str,
    path: &std::path::Path,
    messages: &mut Vec<String>,
    computed_require_warnings: Option<&mut Vec<String>>,
) -> RequireSites {
    let Ok(module) = perry_parser::parse_typescript_with_warnings(source, "requires.cjs", messages)
    else {
        return RequireSites {
            deferred: super::extract_requires::function_local_specs(source),
            calls: Vec::new(),
        };
    };
    let mut visitor = Requires::default();
    module.visit_with(&mut visitor);
    // Reuse the mandatory dependency walk as a zero-allocation admission
    // check. Literal-only CJS modules need no alias/scope diagnostic analysis.
    if let Some(warnings) = computed_require_warnings.filter(|_| visitor.computed_call_candidate) {
        for offset in super::computed_requires::computed_require_offsets(&module) {
            let prefix = &source[..offset.min(source.len())];
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            warnings.push(format!(
                "warning: {}:{}:{}: computed CommonJS require does not automatically include target files; add static imports for the possible targets and use explicit file extensions in computed requests (#10438)",
                path.display(), line, column,
            ));
        }
    }
    let calls = if visitor.shadowed_at_top {
        Vec::new()
    } else {
        let shadows = &visitor.shadowing_scopes;
        visitor
            .calls
            .into_iter()
            .filter(|(lo, hi, _)| !shadows.iter().any(|(s, e)| s <= lo && hi <= e))
            .map(|(lo, hi, spec)| {
                (
                    lo.saturating_sub(1) as usize,
                    hi.saturating_sub(1) as usize,
                    spec,
                )
            })
            .collect()
    };
    RequireSites {
        deferred: visitor
            .sites
            .into_iter()
            .filter_map(|(specifier, deferred)| deferred.then_some(specifier))
            .collect(),
        calls,
    }
}

#[derive(Default)]
struct Requires {
    deferred: bool,
    sites: HashMap<String, bool>,
    computed_call_candidate: bool,
    /// Literal require calls (SWC span) for [`RequireSites::calls`].
    calls: Vec<(u32, u32, String)>,
    /// Enclosing function scopes (SWC span), innermost last.
    scopes: Vec<(u32, u32)>,
    /// Function scopes that bind `require` anywhere in them (a parameter, a
    /// declaration in any block, a catch parameter): conservative, a call
    /// anywhere inside keeps the `require` function.
    shadowing_scopes: Vec<(u32, u32)>,
    /// The module scope binds `require`: no call is the wrapper's.
    shadowed_at_top: bool,
}

impl Requires {
    fn defer(&mut self, visit: impl FnOnce(&mut Self)) {
        let previous = self.deferred;
        self.deferred = true;
        visit(self);
        self.deferred = previous;
    }

    fn scope(&mut self, span: swc_common::Span, visit: impl FnOnce(&mut Self)) {
        self.scopes.push((span.lo.0, span.hi.0));
        visit(self);
        self.scopes.pop();
    }

    fn binds(&mut self, name: &str) {
        if name != "require" {
            return;
        }
        match self.scopes.last() {
            Some(scope) => self.shadowing_scopes.push(*scope),
            None => self.shadowed_at_top = true,
        }
    }
}

impl Visit for Requires {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            if matches!(callee.as_ref(), ast::Expr::Ident(_)) {
                self.computed_call_candidate |= call.args.first().is_some_and(|arg| {
                    arg.spread.is_some()
                        || !matches!(arg.expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(_)))
                });
            }
            if matches!(callee.as_ref(), ast::Expr::Ident(name) if name.sym == *"require") {
                if let [arg] = call.args.as_slice() {
                    if arg.spread.is_none() {
                        if let ast::Expr::Lit(ast::Lit::Str(specifier)) = arg.expr.as_ref() {
                            self.calls.push((
                                call.span.lo.0,
                                call.span.hi.0,
                                specifier.value.to_string_lossy().into_owned(),
                            ));
                            self.sites
                                .entry(specifier.value.to_string_lossy().into_owned())
                                .and_modify(|deferred| *deferred &= self.deferred)
                                .or_insert(self.deferred);
                        }
                    }
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_function(&mut self, function: &ast::Function) {
        function.decorators.visit_with(self);
        self.defer(|visitor| {
            visitor.scope(function.span, |visitor| {
                function.params.visit_with(visitor);
                function.body.visit_with(visitor);
            });
        });
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.defer(|visitor| visitor.scope(arrow.span, |v| arrow.visit_children_with(v)));
    }

    fn visit_constructor(&mut self, constructor: &ast::Constructor) {
        self.defer(|visitor| {
            visitor.scope(constructor.span, |v| constructor.visit_children_with(v))
        });
    }

    fn visit_getter_prop(&mut self, getter: &ast::GetterProp) {
        getter.key.visit_with(self);
        self.defer(|visitor| visitor.scope(getter.span, |v| getter.function.body.visit_with(v)));
    }

    fn visit_setter_prop(&mut self, setter: &ast::SetterProp) {
        setter.key.visit_with(self);
        self.defer(|visitor| {
            visitor.scope(setter.span, |v| {
                setter.function.params.visit_with(v);
                setter.function.body.visit_with(v);
            })
        });
    }

    fn visit_binding_ident(&mut self, id: &ast::BindingIdent) {
        self.binds(id.id.sym.as_ref());
        id.visit_children_with(self);
    }

    // Declaration names are plain identifiers, bound in the enclosing scope.
    fn visit_fn_decl(&mut self, decl: &ast::FnDecl) {
        self.binds(decl.ident.sym.as_ref());
        decl.visit_children_with(self);
    }

    fn visit_class_decl(&mut self, decl: &ast::ClassDecl) {
        self.binds(decl.ident.sym.as_ref());
        decl.visit_children_with(self);
    }

    fn visit_if_stmt(&mut self, stmt: &ast::IfStmt) {
        stmt.test.visit_with(self);
        self.defer(|visitor| {
            stmt.cons.visit_with(visitor);
            stmt.alt.visit_with(visitor);
        });
    }

    fn visit_cond_expr(&mut self, expr: &ast::CondExpr) {
        expr.test.visit_with(self);
        self.defer(|visitor| {
            expr.cons.visit_with(visitor);
            expr.alt.visit_with(visitor);
        });
    }

    fn visit_bin_expr(&mut self, expr: &ast::BinExpr) {
        expr.left.visit_with(self);
        if matches!(
            expr.op,
            ast::BinaryOp::LogicalAnd | ast::BinaryOp::LogicalOr | ast::BinaryOp::NullishCoalescing
        ) {
            self.defer(|visitor| expr.right.visit_with(visitor));
        } else {
            expr.right.visit_with(self);
        }
    }

    fn visit_try_stmt(&mut self, stmt: &ast::TryStmt) {
        // In particular, a throwing require must stay inside its try/catch.
        self.defer(|visitor| stmt.visit_children_with(visitor));
    }

    fn visit_assign_expr(&mut self, expr: &ast::AssignExpr) {
        expr.left.visit_with(self);
        if matches!(
            expr.op,
            ast::AssignOp::AndAssign | ast::AssignOp::OrAssign | ast::AssignOp::NullishAssign
        ) {
            self.defer(|visitor| expr.right.visit_with(visitor));
        } else {
            expr.right.visit_with(self);
        }
    }

    fn visit_switch_stmt(&mut self, stmt: &ast::SwitchStmt) {
        stmt.discriminant.visit_with(self);
        self.defer(|visitor| stmt.cases.visit_with(visitor));
    }

    fn visit_while_stmt(&mut self, stmt: &ast::WhileStmt) {
        stmt.test.visit_with(self);
        self.defer(|visitor| stmt.body.visit_with(visitor));
    }

    fn visit_do_while_stmt(&mut self, stmt: &ast::DoWhileStmt) {
        // Both halves are conditional: the body can `break` or `return` before
        // the test runs, so `do { break } while (require("dep"))` never
        // evaluates the require in Node.
        self.defer(|visitor| {
            stmt.body.visit_with(visitor);
            stmt.test.visit_with(visitor);
        });
    }

    fn visit_for_stmt(&mut self, stmt: &ast::ForStmt) {
        stmt.init.visit_with(self);
        stmt.test.visit_with(self);
        self.defer(|visitor| {
            stmt.update.visit_with(visitor);
            stmt.body.visit_with(visitor);
        });
    }

    fn visit_for_in_stmt(&mut self, stmt: &ast::ForInStmt) {
        stmt.right.visit_with(self);
        self.defer(|visitor| {
            stmt.left.visit_with(visitor);
            stmt.body.visit_with(visitor);
        });
    }

    fn visit_for_of_stmt(&mut self, stmt: &ast::ForOfStmt) {
        stmt.right.visit_with(self);
        self.defer(|visitor| {
            stmt.left.visit_with(visitor);
            stmt.body.visit_with(visitor);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::deferred_require_specs;
    use swc_ecma_visit::VisitWith;

    #[test]
    fn literal_only_modules_skip_scoped_diagnostic_analysis() {
        for (source, expected) in [
            (
                "const dep = require('./dep.js'); function f(value) { return value + 1; }",
                false,
            ),
            ("const r = require; r('./' + name);", true),
            ("require(name);", true),
            ("require(...names);", true),
        ] {
            let module = perry_parser::parse_typescript(source, "admission.cjs").unwrap();
            let mut visitor = super::Requires::default();
            module.visit_with(&mut visitor);
            assert_eq!(visitor.computed_call_candidate, expected, "{source}");
        }
    }

    #[test]
    fn preserves_conditional_and_function_evaluation_boundaries() {
        for source in [
            "if (enabled) require('dep');",
            "if (enabled) { const dep = require('dep'); }",
            "enabled ? require('dep') : 0;",
            "enabled && require('dep');",
            "enabled || require('dep');",
            "enabled ?? require('dep');",
            "value &&= require('dep');",
            "value ||= require('dep');",
            "value ??= require('dep');",
            "try { require('dep'); } catch (e) {}",
            "switch (value) { case 1: require('dep'); }",
            "while (enabled) require('dep');",
            "do { break; } while (require('dep'));",
            "do { require('dep'); } while (enabled);",
            "for (; enabled;) require('dep');",
            "for (const item of items) require('dep');",
            "for (const key in object) require('dep');",
            "module.exports = () => require('dep');",
            "function load(dep = require('dep',)) { return dep; }",
            "module.exports = {get value() { return require('dep'); }};",
        ] {
            assert!(deferred_require_specs(source).contains("dep"), "{source}");
        }
    }

    #[test]
    fn unconditional_occurrences_keep_existing_eager_classification() {
        for source in [
            "const dep = require('dep');",
            "if (require('dep')) {}",
            "require('dep') && enabled;",
            "const x = require('dep') + 1;",
            "value = require('dep');",
            "{ require('dep'); }",
            "if (enabled) require('dep'); require('dep');",
            "require('dep'); module.exports = () => require('dep');",
        ] {
            assert!(!deferred_require_specs(source).contains("dep"), "{source}");
        }
    }

    #[test]
    fn ignores_comments_strings_and_member_calls() {
        assert!(deferred_require_specs(
            "// require('dep')\nconst text = \"require('dep')\";\nif (enabled) other.require('dep');"
        ).is_empty());
    }

    #[test]
    fn wrapping_keeps_conditional_aliases_and_exports_inside_the_body() {
        let source = "class Unrelated {}\nif (enabled) {\nconst dep = require('dep');\nexports.value = require('dep');\nconsole.log(dep);\n}\n";
        let wrapped =
            super::super::wrap::wrap_commonjs(source, std::path::Path::new("/fixture/index.cjs"));
        assert!(
            wrapped.contains("import _lazyreq_0 from 'dep';"),
            "{wrapped}"
        );
        assert!(wrapped.contains("const dep = require('dep');"), "{wrapped}");
        assert!(!wrapped.contains("const dep = _lazyreq_0;"), "{wrapped}");
        assert!(
            wrapped.contains("export const value = _cjs.value;"),
            "{wrapped}"
        );
        assert!(
            !wrapped.contains("export { _lazyreq_0 as value };"),
            "{wrapped}"
        );
        perry_parser::parse_typescript(&wrapped, "wrapped.cjs").unwrap();
    }

    /// A conditional or function-local require of a compiled CommonJS module
    /// is the same static edge as an unconditional one (design CJS_LINK
    /// §2.6): no path-registry call, no per-site memo, no parent handoff
    /// through the support module in a closed program. The target evaluates
    /// when the edge first runs.
    #[test]
    fn a_deferred_require_of_a_commonjs_module_is_its_static_edge() {
        let src = r#"
function get(flag) {
  if (flag) { return require("./dep.js").v; }
  return 0;
}
module.exports = { get };
"#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dep.js"), "module.exports = { v: 1 };\n").unwrap();
        let wrapped = super::super::wrap::wrap_commonjs(src, &dir.path().join("entry.js"));
        assert!(
            wrapped.contains("return __perry_require_static_0().v;"),
            "{wrapped}"
        );
        assert!(
            wrapped.contains("function __perry_require_static_0() {"),
            "{wrapped}"
        );
        for absent in [
            "__perry_cjs_require_deferred",
            "__rec",
            "__perry_require_path_module(",
        ] {
            assert!(
                !wrapped.contains(absent),
                "`{absent}` is the registry path:\n{wrapped}"
            );
        }
        perry_parser::parse_typescript(&wrapped, "wrapped.cjs").unwrap();
    }

    /// No `try` inside `require(specifier)`, whatever the specifier count.
    ///
    /// On wasm32 a `try` makes the enclosing function a returns-twice frame, and
    /// LLVM's sjlj lowering threads the values of every case in that function
    /// through one dispatch loop: one per specifier made `require` grow with
    /// the square of the specifier count (a 3 KB barrel module produced a
    /// 17 MB function). An edge's handoff `try` lives in its own small function.
    #[test]
    fn the_require_function_has_no_try_at_any_specifier_count() {
        let dir = tempfile::tempdir().unwrap();
        for count in [1usize, 16] {
            let mut src = String::new();
            for i in 0..count {
                std::fs::write(
                    dir.path().join(format!("dep{i}.js")),
                    "module.exports = { v: 1 };\n",
                )
                .unwrap();
                src.push_str(&format!(
                    "function get{i}(flag) {{ if (flag) {{ return require(\"./dep{i}.js\").v; }} return 0; }}\n"
                ));
            }
            src.push_str("module.exports = {};\n");
            let wrapped = super::super::wrap::wrap_commonjs(&src, &dir.path().join("entry.js"));
            let start = wrapped
                .find("function require(specifier) {")
                .expect("require function");
            let end = start + wrapped[start..].find("\n    }\n").expect("its end");
            assert!(
                !wrapped[start..end].contains("try"),
                "{count}: {}",
                &wrapped[start..end]
            );
            assert_eq!(
                wrapped.matches("__perry_require_path_module(").count(),
                0,
                "{wrapped}"
            );
        }
    }
}
