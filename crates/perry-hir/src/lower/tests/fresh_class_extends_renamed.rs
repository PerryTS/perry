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
    let hir = lower(RENAMED_FRESH_PARENT);
    let d = class(&hir, "D");
    assert!(
        d.extends_expr.is_some(),
        "D must take its parent from the evaluated `L`, got extends={:?} extends_name={:?}",
        d.extends,
        d.extends_name
    );
    assert!(
        d.extends.is_none(),
        "no static parent class: {:?}",
        d.extends
    );
    assert!(
        d.extends_name.is_none(),
        "no retained parent name for the static chain walks: {:?}",
        d.extends_name
    );
}

#[test]
fn a_renamed_shared_parent_stays_static() {
    // Two same-named classes, neither per evaluation: the rename resolves the
    // heritage statically to the right one, as before.
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
    assert!(
        d.extends_expr.is_none(),
        "a shared parent is static: {:?}",
        d.extends_expr
    );
    assert!(d.extends.is_some(), "a shared parent is static");
}
