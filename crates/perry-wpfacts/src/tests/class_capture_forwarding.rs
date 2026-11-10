//! Instance captures retain their forwarded constructor argument. Controls:
//! environment-backed captures and the direct/composed CJS capture bindings in
//! `unit_cjs::capture_parameters_bound_directly_or_composed_hold_the_class_environment`.
use super::*;

#[test]
fn a_nested_instance_capture_admits_the_forwarded_shared_cell() {
    let src = r#"
        function f() {
            var n = 1;
            class K { nested() { return new (class { r() { return n; } })().r(); } }
            n = 2;
            return new K().nested();
        }
        console.log(f());
    "#;
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let captures: Vec<_> = hir
        .classes
        .iter()
        .flat_map(|c| {
            c.fields
                .iter()
                .filter(|f| f.origin == perry_hir::ClassFieldOrigin::Capture)
                .map(move |f| (c.name.clone(), f.name.clone()))
        })
        .collect();
    assert!(
        !captures.is_empty(),
        "fixture must have an instance capture"
    );
    let pf = compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    for (class, key) in captures {
        let capture = field(tmpl(&pf, &class), &key);
        assert_ne!(
            capture.kind.mask() & (1 << 7),
            0,
            "forwarded shared cell is an object, not bottom: {}",
            pf.dump_text()
        );
    }
}

#[test]
fn environment_backed_capture_preserves_its_object_receiver_fact() {
    let pf = facts(
        r#"
        class Payload { value = 3; }
        function make() {
            const p = new Payload();
            class K { read() { return p.value; } }
            return new K().read();
        }
        console.log(make());
    "#,
    );
    assert_eq!(field(tmpl(&pf, "Payload"), "value").kind.mask(), 1);
    let read = site(&pf, "value");
    assert_eq!(read.verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(read.receivers.iter().any(|r| r.ends_with(":Payload")));
}
