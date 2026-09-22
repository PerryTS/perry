//! #10485/#10489: class members share the enclosing bindings they capture.
//!
//! Two lowering properties are asserted here, both invisible to a runtime
//! probe until the miscompiled program prints a stale value:
//!
//! 1. a `var` captured and mutated across the class boundary becomes ONE
//!    shared cell (`crate::lower::shared_mutable_capture`), even though a
//!    `var` is declared twice in HIR (body-entry slot + declaration), and the
//!    re-declaration writes that cell instead of minting a second one;
//! 2. a `new K()` resolves to the class the BINDING holds, not to whatever
//!    class first claimed the name `K` in the module.

use crate::ir::{Expr, Stmt};

fn lower(source: &str) -> crate::ir::Module {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::super::lower_module(&module, "t", "t.ts").expect("source lowers")
}

fn function<'a>(module: &'a crate::ir::Module, name: &str) -> &'a crate::ir::Function {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("fixture declares function {name}"))
}

/// Statements of `body` (recursing into nested statement bodies) that declare
/// `name`, rendered compactly.
fn declarations_of<'a>(body: &'a [Stmt], name: &str) -> Vec<&'a Stmt> {
    let mut out = Vec::new();
    for stmt in body {
        if matches!(stmt, Stmt::Let { name: n, .. } if n == name) {
            out.push(stmt);
        }
        if let Stmt::For { body, .. } | Stmt::While { body, .. } = stmt {
            out.extend(declarations_of(body, name));
        }
    }
    out
}

fn compact(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

/// A `var` mutated from a constructor is the same binding on both sides: it
/// lowers to a one-element cell, and every use — the declaring function's read
/// and the member's write — goes through it. Before #10489 the two HIR `Let`s a
/// `var` produces read as two bindings, so the desugar skipped the id and the
/// class kept a private copy (`declCtor` returned 0, not 3).
#[test]
fn var_mutated_from_a_constructor_becomes_one_shared_cell() {
    let module = lower(
        r#"
        function declCtor() {
            var a = 0;
            class A { constructor() { a++; } }
            new A(); new A();
            return a;
        }
        declCtor();
        "#,
    );
    let decls = declarations_of(&function(&module, "declCtor").body, "a");
    assert_eq!(
        decls.len(),
        1,
        "the `var` re-declaration must write the cell, not re-declare it: {decls:#?}"
    );
    assert!(
        matches!(decls[0], Stmt::Let { init: Some(Expr::Array(items)), .. } if items.len() == 1),
        "the captured `var` must lower to a one-element cell: {:#?}",
        decls[0]
    );
    let body = compact(&function(&module, "declCtor").body);
    assert!(
        body.contains("IndexSet{object:LocalGet"),
        "the `var a = 0` re-declaration must write the existing cell: {body}"
    );
    let class = module
        .classes
        .iter()
        .find(|c| c.name == "A")
        .expect("fixture declares class A");
    let ctor = compact(&class.constructor.as_ref().expect("class A has a ctor").body);
    assert!(
        ctor.contains("IndexUpdate{"),
        "the constructor's `a++` must update the shared cell: {ctor}"
    );
}

/// The control: a capture nobody mutates keeps the cheap value snapshot, so
/// the cell rewrite cannot quietly become universal (it costs an indirection
/// on every read).
#[test]
fn an_unmutated_var_capture_keeps_its_value_snapshot() {
    let module = lower(
        r#"
        function readOnly() {
            var a = 0;
            class A { value() { return a; } }
            return new A().value();
        }
        readOnly();
        "#,
    );
    let body = compact(&function(&module, "readOnly").body);
    assert!(
        !body.contains("Array([Undefined])") && !body.contains("IndexSet{object:LocalGet"),
        "an unmutated capture must not be boxed into a cell: {body}"
    );
}

/// A class nested in a member body captures the member's rebind local, which
/// holds the cell — so its own members must index through it too. The
/// immediately-constructed form has no `RegisterClassCaptures` node at all
/// (`lower_new` emits a bare `Expr::New`), which is how it was missed.
#[test]
fn a_class_nested_in_a_member_shares_the_same_cell() {
    let module = lower(
        r#"
        function outerFn() {
            var made = 0;
            class Outer {
                make() { return class Inner { constructor() { made++; } }; }
                immediate() { return new (class { read() { return made; } })().read(); }
            }
            const Inner = new Outer().make();
            new Inner();
            return [made, new Outer().immediate()];
        }
        outerFn();
        "#,
    );
    // A named class EXPRESSION registers under a disambiguated key, so match
    // the source name as a prefix.
    let inner = module
        .classes
        .iter()
        .find(|c| c.name.starts_with("Inner"))
        .expect("fixture declares the nested class Inner");
    let inner_ctor = compact(&inner.constructor.as_ref().expect("Inner has a ctor").body);
    assert!(
        inner_ctor.contains("IndexUpdate{"),
        "a nested class's write must reach the shared cell: {inner_ctor}"
    );
    let anon = module
        .classes
        .iter()
        .find(|c| c.name.starts_with("__anon_class_"))
        .expect("fixture declares an immediately-constructed anonymous class");
    let read = compact(
        &anon
            .methods
            .first()
            .expect("the anon class has read()")
            .body,
    );
    assert!(
        read.contains("IndexGet{"),
        "an immediately-constructed nested class must read the cell's value: {read}"
    );
}

/// Two sibling functions that each bind `const K = class {…}`: the second
/// binding holds its OWN class (registered under a disambiguated key), so
/// `new K()` inside it must not construct the first function's class with the
/// first function's capture ids (#10489's `twinTwo` returned 0).
#[test]
fn sibling_same_named_class_bindings_construct_their_own_class() {
    let module = lower(
        r#"
        function twinOne() { var n = 0; const K = class { constructor() { n++; } }; new K(); return n; }
        function twinTwo() { var n = 0; const K = class { constructor() { n += 10; } }; new K(); return n; }
        twinOne(); twinTwo();
        "#,
    );
    let one = compact(&function(&module, "twinOne").body);
    let two = compact(&function(&module, "twinTwo").body);
    let key_of = |body: &str| -> String {
        let at = body
            .find("New{class_name:\"")
            .expect("the fixture constructs its class statically");
        let rest = &body[at + "New{class_name:\"".len()..];
        rest[..rest.find('"').expect("class name is terminated")].to_string()
    };
    let (one_key, two_key) = (key_of(&one), key_of(&two));
    assert_ne!(
        one_key, two_key,
        "sibling functions must construct distinct classes, both built {one_key}"
    );
    for (key, body) in [(&one_key, &one), (&two_key, &two)] {
        let class = module
            .classes
            .iter()
            .find(|c| &c.name == key)
            .unwrap_or_else(|| panic!("the constructed class {key} exists"));
        let ctor = compact(&class.constructor.as_ref().expect("has a ctor").body);
        assert!(
            ctor.contains("IndexUpdate{") || ctor.contains("IndexSet{object:LocalGet"),
            "{key}'s constructor must write its own function's cell: {ctor}"
        );
        assert!(
            body.contains("init:Some(Array([Undefined]))")
                && body.contains("IndexSet{object:LocalGet"),
            "each function declares its own counter cell and initializes it: {body}"
        );
    }
}
