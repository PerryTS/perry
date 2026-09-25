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
pub(super) fn deferred_require_specs(source: &str) -> HashSet<String> {
    let Ok(module) = perry_parser::parse_typescript(source, "requires.cjs") else {
        return super::extract_requires::function_local_specs(source);
    };
    let mut visitor = Requires::default();
    module.visit_with(&mut visitor);
    visitor
        .sites
        .into_iter()
        .filter_map(|(specifier, deferred)| deferred.then_some(specifier))
        .collect()
}

#[derive(Default)]
struct Requires {
    deferred: bool,
    sites: HashMap<String, bool>,
}

impl Requires {
    fn defer(&mut self, visit: impl FnOnce(&mut Self)) {
        let previous = self.deferred;
        self.deferred = true;
        visit(self);
        self.deferred = previous;
    }
}

impl Visit for Requires {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            if matches!(callee.as_ref(), ast::Expr::Ident(name) if name.sym == *"require") {
                if let [arg] = call.args.as_slice() {
                    if arg.spread.is_none() {
                        if let ast::Expr::Lit(ast::Lit::Str(specifier)) = arg.expr.as_ref() {
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
            function.params.visit_with(visitor);
            function.body.visit_with(visitor);
        });
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.defer(|visitor| arrow.visit_children_with(visitor));
    }

    fn visit_constructor(&mut self, constructor: &ast::Constructor) {
        self.defer(|visitor| constructor.visit_children_with(visitor));
    }

    fn visit_getter_prop(&mut self, getter: &ast::GetterProp) {
        getter.key.visit_with(self);
        self.defer(|visitor| getter.body.visit_with(visitor));
    }

    fn visit_setter_prop(&mut self, setter: &ast::SetterProp) {
        setter.key.visit_with(self);
        self.defer(|visitor| setter.body.visit_with(visitor));
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

    /// A deferred require must not re-enter the path registry on every call.
    ///
    /// Deferring a conditional require routes it through
    /// `__perry_require_path_module`, which is a registry lookup plus a
    /// `globalThis` write pair inside a `try`/`finally`. That is needed only until
    /// the target is loaded; without the memo it ran on EVERY call and cost 3.4x on
    /// a hot require (1.43 B -> 4.86 B instructions over 300k calls).
    ///
    /// Nothing else pins the memo — delete it and every other test still passes,
    /// the only symptom being that hot requires get slow again. So assert the
    /// emitted shape: the cache slot is declared, the case is fronted by the
    /// short-circuit, and the record is only cached once `loaded === true`.
    #[test]
    fn a_deferred_require_case_is_fronted_by_its_memo() {
        let src = r#"
function get(flag) {
  if (flag) { return require("./dep.js").v; }
  return 0;
}
module.exports = { get };
"#;
        // The target must RESOLVE: the memo lives in the runtime-record arm, which
        // is only emitted for a specifier that resolves to a real file.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dep.js"), "module.exports = { v: 1 };\n").unwrap();
        let wrapped = super::super::wrap::wrap_commonjs(src, &dir.path().join("entry.js"));

        for (needle, why) in [
            ("__rec;", "the per-site memo slot declaration"),
            (
                "__rec !== undefined && require.cache[",
                "the short-circuit that skips the registry",
            ),
            (
                ".loaded === true",
                "the guard that refuses to cache a half-loaded target",
            ),
        ] {
            assert!(
                wrapped.contains(needle),
                "the deferred-require memo no longer emits `{needle}` ({why}).\n\
                 Without it every call to a deferred require re-enters \
                 `__perry_require_path_module`, which measured 3.4x slower on a hot \
                 require. If the memo moved, update this test; if it was removed on \
                 purpose, delete this test and say why in the PR.\n{wrapped}"
            );
        }

        // The record, not the exports: a module that replaces `module.exports`
        // after evaluation must still read through, as it does in Node.
        assert!(
            wrapped.contains("__rec.exports"),
            "the memo must return `record.exports`, not a cached exports value, or a \
             post-evaluation `module.exports = X` would be invisible to later \
             requires.\n{wrapped}"
        );
    }
}
