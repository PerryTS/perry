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
fn a_prototype_member_is_read_at_the_call() {
    let hir = hir_of("function f(o) { return Array.prototype.slice.call(o, 1); } f({});");
    assert!(hir.contains("ArrayLikeMethod"), "{hir}");
    assert!(hir.contains("source: Prototype"), "{hir}");
    let hir = hir_of("function f(o) { return [].slice.call(o); } f({});");
    assert!(hir.contains("source: Prototype"), "{hir}");
}

#[test]
fn a_captured_binding_keeps_its_value() {
    let hir = hir_of(
        "const slice = Array.prototype.slice; function f(o) { return slice.call(o); } f({});",
    );
    assert!(hir.contains("source: Binding"), "{hir}");
}

#[test]
fn object_prototype_borrowed_calls_go_through_the_checked_entries() {
    let hir = hir_of(
        "function f(o, k) { return Object.prototype.hasOwnProperty.call(o, k) \
         && Object.hasOwnProperty.call(o, k); } f({}, 'k');",
    );
    assert!(hir.contains("js_borrowed_object_has_own"), "{hir}");
    assert!(!hir.contains("\"js_object_has_own\""), "{hir}");
}
