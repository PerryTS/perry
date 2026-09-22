//! #10477: which `instanceof` right-hand sides carry a runtime value.
//!
//! An identifier RHS only reaches `js_instanceof_dynamic` when lowering
//! attaches the binding's value as `ty_expr`; otherwise codegen resolves the
//! bare NAME to a class id, and a name with no class entry folds to
//! `js_instanceof(v, 0)` — always `false`. An imported binding was in exactly
//! that position: `import { Plain } from "./lib.js"; x instanceof Plain` was
//! false for every non-class constructor, in every import form, while
//! `ns.Plain`, `const A = Plain` and the same check inside `lib.js` all worked.
//!
//! The builtin case is the other half: `x instanceof Date` must NOT grow a
//! value, because the reserved class id is what brand-checks a native Date.

use super::*;

fn instanceof_rhs_value(source: &str, function: &str) -> Option<Expr> {
    let module =
        perry_parser::parse_typescript(source, "instanceof-rhs.ts").expect("source parses");
    let hir = crate::lower::lower_module(&module, "instanceof-rhs", "instanceof-rhs.ts")
        .expect("source lowers");
    let body = hir
        .functions
        .iter()
        .find(|f| f.name == function)
        .unwrap_or_else(|| panic!("`{function}` must be lowered: {hir:?}"))
        .body
        .clone();
    match body.first() {
        Some(Stmt::Return(Some(Expr::InstanceOf { ty_expr, .. }))) => {
            ty_expr.as_ref().map(|value| (**value).clone())
        }
        other => panic!("`{function}` must lower to a returned instanceof: {other:?}"),
    }
}

#[test]
fn imported_binding_instanceof_rhs_carries_its_value() {
    let rhs = instanceof_rhs_value(
        r#"
            import { Plain } from "./lib.js";
            export function check(x: unknown) { return x instanceof Plain; }
        "#,
        "check",
    );
    assert!(
        matches!(&rhs, Some(Expr::ExternFuncRef { name, .. }) if name == "Plain"),
        "an imported RHS must be lowered to its value so the dynamic path can \
         resolve the constructor (#10477): {rhs:?}"
    );
}

#[test]
fn builtin_instanceof_rhs_stays_a_static_name() {
    let rhs = instanceof_rhs_value(
        r#"
            export function check(x: unknown) { return x instanceof Date; }
        "#,
        "check",
    );
    assert!(
        rhs.is_none(),
        "an unshadowed builtin RHS must keep the reserved-class-id check: {rhs:?}"
    );
}

#[test]
fn local_function_constructor_instanceof_rhs_carries_its_value() {
    let rhs = instanceof_rhs_value(
        r#"
            function Plain(this: any) {}
            export function check(x: unknown) { return x instanceof Plain; }
        "#,
        "check",
    );
    assert!(
        matches!(&rhs, Some(Expr::FuncRef(_))),
        "a module-local function constructor keeps its pre-#10477 value RHS: {rhs:?}"
    );
}
