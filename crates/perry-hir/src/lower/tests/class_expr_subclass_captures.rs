//! #10486: a class extending a capture-bearing class EXPRESSION held in a
//! local (`const Base = class { m() { return cap; } }; class Sub extends
//! Base {}`) must forward the base's captured locals to the synthesized
//! subclass constructor, even though `extends_name` is deliberately left
//! `None` for this lexically-local heritage shape (see the #5437 PQueue
//! fix). Split from `tests.rs` for the 2000-line file cap.

/// The minimal repro from #10486: a subclass EXPRESSION with no own
/// constructor extending a base class EXPRESSION, both capturing distinct
/// enclosing-function locals. The evaluated subclass must carry BOTH
/// captured ids (the base's and the subclass's own), and construction must
/// use that evaluation's prototype and captures.
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
    assert_eq!(fresh_captures(&outer.body, "Sub"), Some(2));
    assert!(
        compact.contains("NewDynamic{callee:LocalGet("),
        "new Sub() must construct the evaluated class carrying both captures: {compact}"
    );
}

/// Captures belong to the evaluated class object when construction is by
/// value; they must not be lost just because no static New appends them.
fn fresh_captures(body: &[crate::ir::Stmt], template: &str) -> Option<usize> {
    fn visit(expr: &crate::ir::Expr, template: &str, found: &mut Option<usize>) {
        if let crate::ir::Expr::ClassExprFresh {
            template: name,
            captured_args,
            ..
        } = expr
        {
            if name == template {
                *found = Some(captured_args.len());
            }
        }
        crate::walker::walk_expr_children(expr, &mut |child| visit(child, template, found));
    }
    let mut found = None;
    for stmt in body {
        crate::walker::stmt_any_expr(stmt, &mut |expr| {
            visit(expr, template, &mut found);
            false
        });
    }
    found
}

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
    assert_eq!(fresh_captures(&outer.body, "Sub"), Some(1));
    assert!(
        compact.contains("NewDynamic{callee:LocalGet("),
        "new Sub() must use the evaluated class even without own captures: {compact}"
    );
}
