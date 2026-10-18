//! A closed-world refusal census over ORIGINAL source. A loader value,
//! module record, global object or dynamic-code capability can expose the
//! CommonJS cache/factory; such programs keep per-evaluation classes.
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};
fn name(m: &ast::MemberExpr) -> Option<String> {
    perry_hir::patched_builtins::static_member_name(&m.prop)
}
#[derive(Default)]
struct Uses(u32);
impl Visit for Uses {
    fn visit_import_decl(&mut self, i: &ast::ImportDecl) {
        if matches!(
            i.src.value.to_string_lossy().as_ref(),
            "module"
                | "node:module"
                | "vm"
                | "node:vm"
                | "process"
                | "node:process"
                | "inspector"
                | "node:inspector"
        ) {
            self.0 += 1;
        }
        i.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        // The only unescaped require value: an ordinary literal call. Its
        // module result cannot expose the loader except the module/vm APIs.
        if matches!(&c.callee, ast::Callee::Expr(e) if matches!(e.as_ref(), ast::Expr::Ident(i) if i.sym == *"require"))
        {
            if let [arg] = c.args.as_slice() {
                if arg.spread.is_none() {
                    if let ast::Expr::Lit(ast::Lit::Str(s)) = arg.expr.as_ref() {
                        if matches!(
                            s.value.to_string_lossy().as_ref(),
                            "module"
                                | "node:module"
                                | "vm"
                                | "node:vm"
                                | "process"
                                | "node:process"
                                | "inspector"
                                | "node:inspector"
                        ) {
                            self.0 += 1;
                        }
                        return;
                    }
                }
            }
        }
        c.visit_children_with(self);
    }
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        // Ordinary CJS publication and ordinary argv/timer/output reads do
        // not expose a loader. Accessing their constructor still refuses at
        // the outer member below.
        if matches!(m.obj.as_ref(), ast::Expr::Ident(i) if i.sym == *"module")
            && name(m).as_deref() == Some("exports")
        {
            return;
        }
        if matches!(m.obj.as_ref(), ast::Expr::Ident(i) if i.sym == *"process")
            && matches!(
                name(m).as_deref(),
                Some("argv" | "env" | "hrtime" | "stdout" | "stderr" | "exit")
            )
        {
            return;
        }
        if matches!(
            name(m).as_deref(),
            Some(
                "cache"
                    | "_cache"
                    | "require"
                    | "mainModule"
                    | "getBuiltinModule"
                    | "binding"
                    | "_linkedBinding"
                    | "constructor"
            )
        ) {
            self.0 += 1;
        }
        m.visit_children_with(self);
    }
    fn visit_ident(&mut self, i: &ast::Ident) {
        if matches!(
            i.sym.as_ref(),
            "require"
                | "module"
                | "process"
                | "globalThis"
                | "global"
                | "window"
                | "self"
                | "eval"
                | "Function"
        ) {
            self.0 += 1;
        }
    }
}
pub(super) fn module_capability_uses(module: &ast::Module) -> u32 {
    let mut uses = Uses::default();
    module.visit_with(&mut uses);
    uses.0
}
#[cfg(test)]
mod tests {
    use super::*;
    fn uses(s: &str) -> u32 {
        module_capability_uses(&perry_parser::parse_typescript(s, "proof.cjs").unwrap())
    }
    #[test]
    fn closed_calls_and_cache_or_loader_escape_are_distinct() {
        assert_eq!(uses("const {Base} = require('./base'); class C extends Base {} module.exports = C; console.log(process.argv[2]);"), 0);
        for source in [
            "delete require.cache[require.resolve('./child')];",
            "const r = require; r('./child');",
            "require['cache'];",
            "const {cache} = require;",
            "require('module');",
            "import {createRequire} from 'node:module';",
            "module.parent.require('./child');",
            "process.getBuiltinModule('module');",
            "globalThis.process;",
        ] {
            assert!(uses(source) > 0, "{source}");
        }
    }
}
