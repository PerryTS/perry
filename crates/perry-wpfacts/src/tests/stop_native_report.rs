//! STOP report lines (PERRY_FACTS_STOP_ON_TOP only): when a native hands a
//! value to unknown code (`escape_at` inside a native call), the S block
//! names the native call and its callee (`native:`) and the value handed
//! with the escape reason (`hands:`); a copy edge with no program position
//! names its source and the solver line that made it (`nopos:`).
use super::*;

fn first_block(src: &str) -> String {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let mut v = crate::topstop::take_captured();
    assert!(!v.is_empty(), "no STOP block");
    v.swap_remove(0)
}

/// B.2.2.2 `__defineGetter__` given a Proxy getter: the Proxy has no model
/// for the accessor rule (PXESC), so the native hands it to unknown code.
const NATIVE_HANDS: &str =
    "const h = {};\n(h as any).__defineGetter__(\"g\", new Proxy(function () {}, {}));\n";

#[test]
fn stop_report_names_the_native_and_the_handed_value() {
    let b = first_block(NATIVE_HANDS);
    assert!(b.contains("a native hands a value to unknown code"), "{b}");
    assert!(
        b.lines().any(|l| l.starts_with("  native:  call #")
            && l.ends_with(" of builtin-fn:Object.prototype.__defineGetter__")),
        "no native line:\n{b}"
    );
    assert!(
        b.lines().any(|l| l.starts_with("  hands:   proxy:0 (Proxy operand of rule ")),
        "no hands line:\n{b}"
    );
}

/// The same native call inside a function: the getter is its parameter, and
/// the binding of a parameter (`run_unsummarized`'s `param-bind` edge) stands
/// at the function's declaration, so the edge into the getter's node has a
/// program position.
#[test]
fn stop_report_parameter_binding_has_a_position() {
    let b = first_block(
        "function k(h: any, y: any): void { h.__defineGetter__(\"g\", y); }\nk({}, new Proxy(function () {}, {}));\n",
    );
    assert!(b.contains("a native hands a value to unknown code"), "{b}");
    assert!(b.contains("  fed by:  "), "no fed-by line:\n{b}");
    assert!(!b.contains("(no program position recorded)"), "edge without a position:\n{b}");
    assert!(!b.contains("  nopos:   "), "edge without a position:\n{b}");
}
