//! #10486: a class extending a capture-bearing class EXPRESSION held in a
//! local (`const Base = class { m() { return cap; } }; class Sub extends
//! Base {}`) must forward the base's captured locals to the synthesized
//! subclass constructor, even though `extends_name` is deliberately left
//! `None` for this lexically-local heritage shape (see the #5437 PQueue
//! fix). Split from `tests.rs` for the 2000-line file cap.

/// The minimal repro from #10486: a subclass EXPRESSION with no own
/// constructor extending a base class EXPRESSION, both capturing distinct
/// enclosing-function locals. The `new Sub()` construction site must
/// forward BOTH captured ids (the base's and the subclass's own), not just
/// the subclass's own capture.
#[test]
fn subclass_of_local_class_expr_forwards_base_captures() {
    let source = r#"
        function outer() {
            const baseCap = "base-capture";
            const subCap = "sub-capture";
            const Base = class { m() { return baseCap; } };
            const Sub = class extends Base { n() { return subCap; } };
            return new Sub().m();
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let outer = hir
        .functions
        .iter()
        .find(|f| f.name == "outer")
        .expect("fixture declares function outer");
    let compact: String = format!("{:?}", outer.body)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    // The `new Sub()` site must append TWO forwarded capture args (the
    // subclass's own `subCap` plus the inherited `baseCap`), not one.
    assert!(
        compact.contains("cap_args_appended:2"),
        "expected new Sub() to forward both the base and subclass captures \
         (cap_args_appended: 2); got body: {compact}"
    );
}

// NOTE: a subclass DECLARATION (as opposed to expression) extending a local
// class expression is a separate lowering path (`Expr::NewDynamic` with a
// `RegisterClassCaptures`/`RefreshClassExprCaptures`-based shared-box
// capture mechanism, not `Expr::New{cap_args_appended}`) that this fix does
// not cover — left as a known gap, see the PR body for #10486.

/// A subclass that captures nothing of its own, extending a capture-bearing
/// local class expression, must still forward the base's capture (a
/// regression the naive "skip if the child's own union is empty" shape
/// would have reintroduced).
#[test]
fn subclass_with_no_own_captures_still_forwards_base_captures() {
    let source = r#"
        function outer() {
            const cap = "only-base-cap";
            const Base = class { m() { return cap; } };
            const Sub = class extends Base {};
            return new Sub().m();
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let outer = hir
        .functions
        .iter()
        .find(|f| f.name == "outer")
        .expect("fixture declares function outer");
    let compact: String = format!("{:?}", outer.body)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    assert!(
        compact.contains("cap_args_appended:1"),
        "expected new Sub() to forward the base's capture even though Sub \
         itself captures nothing; got body: {compact}"
    );
}
