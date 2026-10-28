//! Declaration-unit contracts preserve emitted callable identity.
use crate::{compute_units, FactsConfig};

fn lower(src: &str, path: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(src, path).unwrap();
    perry_hir::lower_module(&ast, "contract", path).unwrap()
}

const SOURCE: &str = r#"
function make(n: number) { return function(x: number) { return (x * 7 + n) % 101; }; }
function run(f: (x: number) => number, x: number) { return f(x); }
console.log(run(make(3), 4));
"#;

#[test]
fn a_captured_body_crosses_declaration_units_with_its_original_id() {
    let m = lower(SOURCE, "contract.ts");
    let id = perry_hir::emitted_closures(&m)[0].0;
    let param = m.functions.iter().find(|f| f.name == "run").unwrap().params[0].id;
    let pf = compute_units(&[("contract.ts".into(), &m)], &FactsConfig::default());
    assert!(pf.units.len() >= 3, "fixture must cross the unit boundary");
    let b = &pf
        .bindings_for("contract.ts")
        .unwrap()
        .locals
        .iter()
        .find(|(i, _)| *i == param)
        .unwrap()
        .1
        .bodies;
    assert_eq!(b.len(), 1, "{}", pf.dump_text());
    assert_eq!((b[0].module, b[0].func_id, b[0].params), (0, id, 1));
    assert!(b[0].closure && b[0].plain);
}

#[test]
fn a_primitive_or_builtin_mixed_with_a_body_refuses_the_contract() {
    for extra in ["run(1 as any, 4);", "run(Math.abs, 4);"] {
        let m = lower(&format!("{SOURCE} {extra}"), "contract.ts");
        let param = m.functions.iter().find(|f| f.name == "run").unwrap().params[0].id;
        let pf = compute_units(&[("contract.ts".into(), &m)], &FactsConfig::default());
        let b = &pf
            .bindings_for("contract.ts")
            .unwrap()
            .locals
            .iter()
            .find(|(i, _)| *i == param)
            .unwrap()
            .1
            .bodies;
        assert!(b.is_empty(), "{extra}: {b:?}");
    }
}

#[test]
fn a_foreign_module_cannot_reuse_a_same_numbered_local_body() {
    let a = lower(
        "export function make(n: number) { return (x: number) => x + n; }",
        "a.ts",
    );
    let b = lower(
        r#"
        import { make } from './a';
        function run(f: any, x: number) { return f(x); }
        const own = (x: number) => x * 9;
        console.log(run(make(3), 4), own(2));
    "#,
        "b.ts",
    );
    let param = b.functions.iter().find(|f| f.name == "run").unwrap().params[0].id;
    let pf = compute_units(
        &[("a.ts".into(), &a), ("b.ts".into(), &b)],
        &FactsConfig::default(),
    );
    let bodies = &pf
        .bindings_for("b.ts")
        .unwrap()
        .locals
        .iter()
        .find(|(i, _)| *i == param)
        .unwrap()
        .1
        .bodies;
    assert!(
        bodies.is_empty(),
        "foreign identities must remain generic: {bodies:?}"
    );
}

#[test]
fn repeated_factory_contexts_preserve_one_emitted_callback_body() {
    let m = lower(
        r#"
        function make(n: number) { return function(x: number) { return (x * 7 + n) % 101; }; }
        function run(f: (x: number) => number, count: number, x: number) {
            let y = x;
            for (let i = 0; i < count; ++i) y = f(y);
            return y;
        }
        let sum = 0;
        for (let k = 1; k < 20; ++k) sum += run(make(k % 3), 15, k);
        console.log(sum);
    "#,
        "contract.ts",
    );
    let id = perry_hir::emitted_closures(&m)[0].0;
    let param = m.functions.iter().find(|f| f.name == "run").unwrap().params[0].id;
    let pf = compute_units(&[("contract.ts".into(), &m)], &FactsConfig::default());
    let b = &pf
        .bindings_for("contract.ts")
        .unwrap()
        .locals
        .iter()
        .find(|(i, _)| *i == param)
        .unwrap()
        .1
        .bodies;
    assert_eq!(b.len(), 1, "{}", pf.dump_text());
    assert_eq!(b[0].func_id, id);
    assert!(b[0].closure && b[0].plain);
}
