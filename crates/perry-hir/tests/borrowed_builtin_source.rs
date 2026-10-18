//! #11915: a borrowed builtin call keeps WHERE its method value comes from, so
//! a failed builtin-prototype proof reads the right holder.

use perry_hir::lower_module;
use perry_parser::parse_typescript;

fn hir_of(source: &str) -> String {
    let parsed = parse_typescript(source, "borrowed.js").unwrap();
    format!(
        "{:?}",
        lower_module(&parsed, "test", "borrowed.js").unwrap()
    )
}

#[test]
fn borrowed_prototype_member_is_read_at_the_call() {
    let hir = hir_of("function f(o) { return Array.prototype.slice.call(o, 1); } f({});");
    assert!(!hir.contains("ArrayLikeMethod"), "{hir}");
    assert!(hir.contains("property: \"prototype\""), "{hir}");
    assert!(hir.contains("property: \"slice\""), "{hir}");
    assert!(hir.contains("property: \"call\""), "{hir}");
    let hir = hir_of("function f(o) { return [].slice.call(o); } f({});");
    assert!(!hir.contains("ArrayLikeMethod"), "{hir}");
    assert!(hir.contains("property: \"slice\""), "{hir}");
    assert!(hir.contains("property: \"call\""), "{hir}");
}

#[test]
fn borrowed_captured_binding_keeps_its_value() {
    let hir = hir_of(
        "const slice = Array.prototype.slice; function f(o) { return slice.call(o); } f({});",
    );
    assert!(!hir.contains("ArrayLikeMethod"), "{hir}");
    assert!(
        hir.contains("callee: PropertyGet { object: LocalGet("),
        "{hir}"
    );
    assert!(hir.contains("property: \"call\""), "{hir}");
}

#[test]
fn borrowed_object_prototype_calls_keep_the_reflective_source() {
    let hir = hir_of(
        "function f(o, k) { return Object.prototype.hasOwnProperty.call(o, k) \
         && Object.hasOwnProperty.call(o, k); } f({}, 'k');",
    );
    assert!(!hir.contains("js_borrowed_object_has_own"), "{hir}");
    assert_eq!(hir.matches("property: \"call\"").count(), 2, "{hir}");
    assert_eq!(
        hir.matches("property: \"hasOwnProperty\"").count(),
        2,
        "{hir}"
    );
    assert!(hir.contains("GlobalThisExpr"), "{hir}");
    assert!(!hir.contains("\"js_object_has_own\""), "{hir}");
}
