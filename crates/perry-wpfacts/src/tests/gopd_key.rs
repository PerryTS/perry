//! The key operand of `Object.getOwnPropertyDescriptor`,
//! `Reflect.getOwnPropertyDescriptor` and `Reflect.deleteProperty` is
//! converted with ToPropertyKey: an object key's `toString` runs (SOUND7 D3:
//! the key was lowered and dropped, so its `toString` was excluded while Node
//! ran it, node-suite object/reflect-proxy-abrupt).
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

#[test]
fn a_descriptor_key_object_is_converted() {
    for call in [
        "Reflect.getOwnPropertyDescriptor(t, k)",
        "Object.getOwnPropertyDescriptor(t, k)",
        "Reflect.deleteProperty(t, k)",
    ] {
        let src = format!(
            r#"
function hit() {{ return 1; }}
const k: any = {{ toString() {{ hit(); return "x"; }} }};
const t: any = {{ x: 1 }};
console.log({call});
"#
        );
        assert!(hit_runs(&src), "{call}");
    }
}

/// Control: a computed read converts its key (passes on the base).
#[test]
fn a_computed_read_key_object_is_converted() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
const k: any = { toString() { hit(); return "x"; } };
const t: any = { x: 1 };
console.log(t[k]);
"#
    ));
}
