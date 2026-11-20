//! An exposed loader capability can obtain any native module by runtime name.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module};

fn module_loader(expr: &Expr) -> bool {
    match expr {
        Expr::NativeModuleRef(name) | Expr::NativeMethodCall { module: name, .. }
            if matches!(name.as_str(), "module" | "module.default") =>
        {
            return true
        }
        Expr::Closure { body, .. } if super::stmts_any_expr(body, &mut module_loader) => {
            return true
        }
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= module_loader(child));
    found
}

pub(super) fn may_escape(module: &Module) -> bool {
    super::module_uses_process_object(module)
        || super::global_object::may_escape(module)
        || super::module_any_expr(module, &mut module_loader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::Stmt;

    fn selected(expr: Expr) -> bool {
        let mut module = Module::new("constants-selection.ts");
        module.init.push(Stmt::Expr(expr));
        may_escape(&module)
    }

    #[test]
    fn exposed_loader_capabilities_retain_constants() {
        for name in [
            "process",
            "process.default",
            "process.namespace",
            "module",
            "module.default",
        ] {
            assert!(selected(Expr::NativeModuleRef(name.into())), "{name}");
        }
        assert!(selected(Expr::GlobalThisExpr));
    }

    #[test]
    fn dedicated_argv_and_an_unrelated_namespace_do_not_expose_a_loader() {
        assert!(!selected(Expr::ProcessArgv));
        assert!(!selected(Expr::NativeModuleRef("fs".into())));
    }
}
