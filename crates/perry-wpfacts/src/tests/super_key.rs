//! The computed key of `super[key]` (a store in a class method, and a read,
//! store or call in an object-literal method) is converted with
//! ToPropertyKey: an object key's `@@toPrimitive` runs (SOUND10 S,
//! test262 12211_super_key_order: the key was lowered and dropped, so its
//! `@@toPrimitive` was excluded while Node ran it).
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const KEY: &str = r#"
function hit() { return 1; }
const k: any = { [Symbol.toPrimitive]() { hit(); return "x"; } };
"#;

#[test]
fn a_super_key_object_is_converted() {
    let missed = [
        (
            "class store",
            "class A { set x(v: any) {} } class B extends A { w(v: any) { super[k] = v; } } new B().w(1);",
        ),
        (
            "object read",
            "const o: any = { __proto__: { x: 1 }, r() { return super[k]; } }; console.log(o.r());",
        ),
        (
            "object store",
            "const o: any = { __proto__: { x: 1 }, w(v: any) { super[k] = v; } }; o.w(1);",
        ),
        (
            "object call",
            "const o: any = { __proto__: { x() { return 1; } }, c() { return super[k](); } }; console.log(o.c());",
        ),
    ]
    .into_iter()
    .filter(|(_, body)| !hit_runs(&format!("{KEY}{body}")))
    .map(|(what, _)| what)
    .collect::<Vec<_>>();
    assert!(missed.is_empty(), "{missed:?}");
}

/// Control: a key object never used as a key is never converted.
#[test]
fn an_unused_key_object_is_not_converted() {
    assert!(!hit_runs(&format!(
        "{KEY}class A {{ set x(v: any) {{}} }} class B extends A {{ w(v: any) {{ super.x = v; }} }} new B().w(k === k);"
    )));
}
