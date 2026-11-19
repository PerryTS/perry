//! `super.m()` in a method of a class with a static prototype image calls the
//! holder's method body directly: the home prototype's shape names the
//! parent's prototype, and the parent's names the ConstFn holder. The get
//! stays a read of the holders' shadow bits where the program writes `m`.
use crate::native_root_coverage::ir_opts;

const SOURCE: &str = "class Parent { m(a, b) { return a + b; } }
class Child extends Parent { m(a, b) { return super.m(a, b) * 2; } }
console.log(new Child().m(1, 2));";

fn emit(source: &str) -> String {
    emit_for(source, "x86_64-unknown-linux-gnu")
}

fn emit_for(source: &str, triple: &str) -> String {
    let path = "/thr7_super.ts";
    let ast = perry_parser::parse_typescript(source, path).unwrap();
    let m = perry_hir::lower_module(&ast, path, path).unwrap();
    let mut opts = ir_opts(triple, true);
    let births = crate::module_birth_shapes(&m, opts.clone()).unwrap();
    let ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    opts.static_shape_ids = ids.into_iter().collect();
    String::from_utf8(crate::compile_module(&m, opts).unwrap()).expect("LLVM IR is UTF-8")
}

/// The bodies of `Child.m` (every emitted variant).
fn child_bodies(ir: &str) -> Vec<&str> {
    ir.split("\ndefine ")
        .filter(|f| {
            f.lines()
                .next()
                .is_some_and(|h| h.contains("__Child__m(") && h.contains("@perry_method_"))
        })
        .collect()
}

fn calls_parent(body: &str) -> bool {
    body.lines()
        .any(|l| l.contains("call ") && l.contains("perry_method_") && l.contains("__Parent__m("))
}

#[test]
fn proven_super_method_call_is_a_direct_call() {
    let ir = emit(SOURCE);
    let bodies = child_bodies(&ir);
    assert!(!bodies.is_empty(), "no Child.m body in\n{ir}");
    for body in &bodies {
        assert!(calls_parent(body), "{body}");
        assert!(!body.contains("@js_object_super_get("), "{body}");
        assert!(!body.contains("@js_class_method_home("), "{body}");
        // A census-constant name in a program with no dynamic write: no test.
        assert!(!body.contains("holder.page"), "{body}");
        assert!(!body.contains("@js_super_accessor_get("), "{body}");
    }
}

#[test]
fn a_dynamic_write_guards_the_super_call_by_the_holder_bits() {
    let mut census = perry_hir::ProtoCensus::default();
    census.dynamic_write = true;
    let ir = crate::proto_constants::with_test_census(census, || emit(SOURCE));
    let bodies = child_bodies(&ir);
    assert!(!bodies.is_empty(), "no Child.m body in\n{ir}");
    for body in &bodies {
        assert!(calls_parent(body), "{body}");
        // One shadow test per prototype: the home (Child) and the holder.
        let pages = body
            .lines()
            .filter(|l| l.starts_with("holder.page") && l.trim_end().ends_with(':'))
            .count();
        assert_eq!(pages, 2, "{body}");
        assert!(body.contains("@js_super_accessor_get("), "{body}");
        assert!(body.contains("@js_method_call_value("), "{body}");
        assert!(!body.contains("@js_object_super_get("), "{body}");
    }
}

/// Out of the proof: an accessor named `m` keeps the generic get and call.
#[test]
fn an_accessor_holder_keeps_the_generic_super_call() {
    let ir = emit(
        "class Parent { get m() { return () => 1; } }
class Child extends Parent { m() { return super.m(); } }
console.log(new Child().m());",
    );
    let bodies = child_bodies(&ir);
    assert!(!bodies.is_empty(), "no Child.m body in\n{ir}");
    for body in &bodies {
        assert!(body.contains("@js_object_super_get("), "{body}");
    }
}

/// The bodies of static method `class.m` (`@perry_static_<module>__<class>__c<id>__<m>(`).
fn static_bodies<'a>(ir: &'a str, class: &str, m: &str) -> Vec<&'a str> {
    ir.split("\ndefine ")
        .filter(|f| {
            f.lines().next().is_some_and(|h| {
                h.contains("@perry_static_")
                    && h.contains(&format!("__{class}__c"))
                    && h.contains(&format!("__{m}("))
            })
        })
        .collect()
}

/// #12213: `super.m()` in a static method calls the parent's static body
/// directly behind the site's static-call memo (the home's and the owner's
/// shapes); only a miss reads the home's live parent and calls the value.
#[test]
fn static_super_method_call_is_a_guarded_direct_call() {
    for triple in ["x86_64-unknown-linux-gnu", "arm64-apple-macosx15.0.0"] {
        let ir = emit_for(
            "class Parent { static m(a, b) { return a + b; } }
class Child extends Parent { static m(a, b) { return super.m(a, b) * 2; } }
console.log(Child.m(1, 2));",
            triple,
        );
        let bodies = static_bodies(&ir, "Child", "m");
        assert!(!bodies.is_empty(), "no static Child.m body in\n{ir}");
        for body in &bodies {
            assert_static_super_direct(body);
            // The memo hit is inline: the runtime guard runs only on a miss.
            assert!(body.contains("static_guard.miss"), "{triple}: {body}");
        }
    }
}

fn assert_static_super_direct(body: &str) {
    assert!(
        body.lines().any(|l| l.contains("call ")
            && l.contains("@perry_static_")
            && l.contains("__Parent__c")
            && l.contains("__m(")),
        "{body}"
    );
    assert!(
        body.contains("@js_class_static_super_call_guard("),
        "{body}"
    );
    assert!(body.contains("@js_static_this_arm_value("), "{body}");
    // The home binding is elided: only the miss reads the home.
    let slow = body.find("super.get.slow").expect("miss block");
    assert!(!body[..slow].contains("@js_class_method_home("), "{body}");
    // The miss keeps the generic get and call.
    assert!(body.contains("@js_object_super_get("), "{body}");
    assert!(body.contains("@js_method_call_value("), "{body}");
}

/// Out of the proof: a parent static with a rest parameter keeps the generic
/// get and call.
#[test]
fn a_static_rest_holder_keeps_the_generic_super_call() {
    let ir = emit(
        "class Parent { static m(...xs) { return xs.length; } }
class Child extends Parent { static m() { return super.m(1, 2); } }
console.log(Child.m());",
    );
    let bodies = static_bodies(&ir, "Child", "m");
    assert!(!bodies.is_empty(), "no static Child.m body in\n{ir}");
    for body in &bodies {
        assert!(
            !body.contains("@js_class_static_super_call_guard("),
            "{body}"
        );
        assert!(body.contains("@js_object_super_get("), "{body}");
    }
}
