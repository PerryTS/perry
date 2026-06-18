use anyhow::{bail, Result};
use swc_ecma_ast as ast;

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

/// Resolve a non-computed PropName (`Ident` / string / numeric literal) to its
/// property-key string. Returns `None` for computed (`[expr]`) keys — those are
/// runtime values and never trigger the static-semantics name checks below.
fn static_prop_name(key: &ast::PropName) -> Option<String> {
    match key {
        ast::PropName::Ident(i) => Some(i.sym.to_string()),
        ast::PropName::Str(s) => Some(s.value.as_str().unwrap_or("").to_string()),
        ast::PropName::Num(n) => Some(n.value.to_string()),
        _ => None,
    }
}

/// ECMA-262 Class Definitions / Static Semantics: Early Errors for the public
/// (non-private) class element surface. These must be rejected at compile time
/// with a SyntaxError. SWC already rejects several (async constructor, a field
/// named `constructor`, a static field named `constructor`); this covers the
/// ones it parses cleanly:
///   - more than one constructor,
///   - a constructor that is a getter / setter / generator (a SpecialMethod),
///   - a static method or accessor named `prototype`,
///   - a static field named `prototype`.
/// Test262 language/.../class/elements/syntax/early-errors + definition/*.
pub fn validate_class_element_early_errors(class: &ast::Class, class_name: &str) -> Result<()> {
    let mut constructor_count = 0usize;
    for member in &class.body {
        match member {
            // `constructor(){}` (the ordinary one) parses as a dedicated
            // Constructor node; count them to catch duplicates. TypeScript
            // overload SIGNATURES (`constructor(kind: 'N');` with no body)
            // are type-only declarations, not constructors — only the
            // implementation (the one with a body) counts (#4872, rxjs's
            // `Notification` has 3 overload signatures + 1 implementation).
            ast::ClassMember::Constructor(c) if c.body.is_some() => {
                constructor_count += 1;
            }
            ast::ClassMember::Method(m) => {
                let Some(name) = static_prop_name(&m.key) else {
                    continue;
                };
                // A getter/setter/generator/async method named `constructor`
                // (a SpecialMethod) is a Syntax Error. SWC routes these through
                // ClassMethod (not Constructor), so the duplicate count above
                // doesn't see them — reject here.
                if !m.is_static && name == "constructor" {
                    let is_special =
                        matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter)
                            || m.function.is_generator
                            || m.function.is_async;
                    if is_special {
                        bail!(
                            "SyntaxError: class `{class_name}` constructor may not be an \
                             accessor, generator, or async method",
                        );
                    }
                    // A plain non-static method literally named "constructor"
                    // (e.g. `\"constructor\"(){}`) is the class constructor too.
                    constructor_count += 1;
                }
                // `static prototype` in any method form is forbidden.
                if m.is_static && name == "prototype" {
                    bail!(
                        "SyntaxError: class `{class_name}` may not have a static method \
                         named 'prototype'",
                    );
                }
            }
            ast::ClassMember::ClassProp(p) => {
                let Some(name) = static_prop_name(&p.key) else {
                    continue;
                };
                if p.is_static && name == "prototype" {
                    bail!(
                        "SyntaxError: class `{class_name}` may not have a static field \
                         named 'prototype'",
                    );
                }
            }
            _ => {}
        }
    }
    if constructor_count > 1 {
        bail!("SyntaxError: class `{class_name}` may only have one constructor");
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
