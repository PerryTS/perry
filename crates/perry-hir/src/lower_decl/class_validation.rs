use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

pub fn validate_legacy_decorator_surface(class: &ast::Class, class_name: &str) -> Result<()> {
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) => {
                // SWC models getters/setters as Method with kind != Method.
                // Their decorators would expect descriptor replacement, which
                // Perry does not implement; reject rather than drop silently.
                if matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) {
                    if let Some(dec) = m.function.decorators.first() {
                        let name = decorator_name_hint(dec);
                        let key = method_key_hint(&m.key);
                        let kind = match m.kind {
                            ast::MethodKind::Getter => "getter",
                            ast::MethodKind::Setter => "setter",
                            _ => "accessor",
                        };
                        bail!(
                            "TypeScript {kind} decorators are not supported (found `@{name}` on `{class_name}.{key}`). \
                             See docs/src/language/decorators.md — accessor descriptor replacement is not implemented.",
                        );
                    }
                }
            }
            ast::ClassMember::PrivateMethod(m) => {
                if let Some(dec) = m.function.decorators.first() {
                    let name = decorator_name_hint(dec);
                    bail!(
                        "TypeScript private method decorators are not supported yet (found `@{name}` on private method of `{class_name}`).",
                    );
                }
            }
            ast::ClassMember::ClassProp(_) => {}
            ast::ClassMember::PrivateProp(p) => {
                if let Some(dec) = p.decorators.first() {
                    let name = decorator_name_hint(dec);
                    bail!(
                        "TypeScript private property decorators are not supported yet (found `@{name}` on a private property of `{class_name}`).",
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn method_key_hint(key: &ast::PropName) -> String {
    match key {
        ast::PropName::Ident(i) => i.sym.to_string(),
        ast::PropName::Str(s) => format!("{:?}", s.value),
        ast::PropName::Num(n) => n.value.to_string(),
        _ => "<method>".to_string(),
    }
}

fn decorator_name_hint(dec: &ast::Decorator) -> String {
    match dec.expr.as_ref() {
        ast::Expr::Ident(i) => i.sym.to_string(),
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(e) = &c.callee {
                if let ast::Expr::Ident(i) = e.as_ref() {
                    return i.sym.to_string();
                }
            }
            "<decorator>".to_string()
        }
        _ => "<decorator>".to_string(),
    }
}
