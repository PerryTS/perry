//! `class D extends L`, with L a class created per evaluation whose name the
//! lowering had to scope-rename (another `class L` is declared elsewhere in the
//! file), takes its parent from the evaluated class bound to `L`, as a dynamic
//! `extends` does. Resolved statically it named the template every evaluation
//! of L shares, so two evaluations of the enclosing function gave D the same
//! parent. Without a same-named class elsewhere the heritage was dynamic
//! already; the rename is what hid the local from the heritage check.

use crate::ir::{Class, Module};

fn lower(source: &str) -> Module {
    let module = perry_parser::parse_typescript(source, "fresh-extends.ts").expect("source parses");
    crate::lower::lower_module(&module, "fresh-extends", "fresh-extends.ts").expect("source lowers")
}

fn class<'a>(hir: &'a Module, name: &str) -> &'a Class {
    hir.classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("class `{name}` must be lowered: {:?}", hir.classes))
}

const RENAMED_FRESH_PARENT: &str = r#"
    export const other = () => { class L {} return L; };
    export function make(n: number) {
        const k = n;
        class L { v() { return k; } }
        class D extends L {}
        return D;
    }
"#;

#[test]
fn a_renamed_fresh_parent_is_read_from_its_binding() {
    // #11759 (c′): L may be evaluated more than once, and so may D. D's
    // template extends the renamed L's template (not the other module-level
    // `L`), and each evaluation of D after its first pins the evaluated `L`
    // its binding holds.
    let hir = lower(RENAMED_FRESH_PARENT);
    let d = class(&hir, "D");
    let parent = d
        .extends_name
        .clone()
        .expect("D's template extends L's template");
    assert!(
        parent != "L" && parent.starts_with('L'),
        "D extends the scope-renamed L of its own body: {parent}"
    );
    assert!(d.extends_expr.is_none(), "{:?}", d.extends_expr);
    let make = make_body(&hir);
    assert!(
        make.contains("evaluated_parent: Some(LocalGet("),
        "D's later evaluations take their parent from the evaluated `L`: {make}"
    );
}

/// The `make` function's body, as debug text.
fn make_body(hir: &Module) -> String {
    let body = &hir
        .functions
        .iter()
        .find(|f| f.name == "make")
        .expect("`make` must be lowered")
        .body;
    format!("{body:?}")
}

#[test]
fn a_renamed_shared_parent_stays_static() {
    // Two same-named classes, neither per evaluation (the body declaring the
    // parent runs once): the rename resolves the heritage statically to the
    // right one, as before.
    let hir = lower(
        r#"
        export const other = () => { class L {} return L; };
        export const made = (function make() {
            class L { v() { return 1; } }
            class D extends L {}
            return D;
        })();
        "#,
    );
    let d = class(&hir, "D");
    assert!(
        d.extends_expr.is_none(),
        "a shared parent is static: {:?}",
        d.extends_expr
    );
    assert!(d.extends.is_some(), "a shared parent is static");
}

#[test]
fn a_renamed_repeatable_parent_follows_its_evaluation() {
    // #11759 (c′): the same parent in a function that may run more than once
    // has fresh later evaluations, so the subclass's later evaluations take
    // their parent from the evaluated class bound to `L`.
    let hir = lower(
        r#"
        export const other = () => { class L {} return L; };
        export function make() {
            class L { v() { return 1; } }
            class D extends L {}
            return D;
        }
        "#,
    );
    let d = class(&hir, "D");
    assert!(d.extends.is_some(), "D's template extends L's template");
    let make = make_body(&hir);
    assert!(
        make.contains("evaluated_parent: Some(LocalGet("),
        "D's later evaluations take their parent from the evaluated `L`: {make}"
    );
}
