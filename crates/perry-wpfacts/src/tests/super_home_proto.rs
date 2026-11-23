//! `super.key` starts at the home object's [[GetPrototypeOf]] at the
//! access: a prototype `Object.setPrototypeOf` gave the home (the class
//! prototype, or the class for a static method) is where the lookup goes,
//! not only the declared parent (SOUND10 S, test262 12211_super_key_order:
//! the setter of the prototype set while the store ran was claimed
//! inactive while Node ran it).
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

const B: &str = r#"
function hit() { return 1; }
const B: any = { set x(v: any) { hit(); }, get y() { return hit(); }, m() { return hit(); } };
"#;

#[test]
fn super_starts_at_the_home_prototype() {
    let missed = [
        (
            "store",
            "class H { w(v: any) { super.x = v; } } Object.setPrototypeOf(H.prototype, B); H.prototype.w.call({}, 1);",
        ),
        (
            "read",
            "class H { r() { return super.y; } } Object.setPrototypeOf(H.prototype, B); console.log(new H().r());",
        ),
        (
            "call",
            "class H { c() { return super.m(); } } Object.setPrototypeOf(H.prototype, B); console.log(new H().c());",
        ),
        (
            "arrow store",
            "class H { w(v: any) { const f = () => { super.x = v; }; f(); } } Object.setPrototypeOf(H.prototype, B); H.prototype.w.call({}, 1);",
        ),
        (
            "static call",
            "class H { static c() { return super.m(); } } Object.setPrototypeOf(H, B); console.log(H.c());",
        ),
    ]
    .into_iter()
    .filter(|(_, body)| !hit_runs(&format!("{B}{body}")))
    .map(|(what, _)| what)
    .collect::<Vec<_>>();
    assert!(missed.is_empty(), "{missed:?}");
}

/// Control: a prototype given to another object is not the home's.
#[test]
fn another_objects_prototype_is_not_the_home() {
    assert!(!hit_runs(&format!(
        "{B}class H {{ c() {{ return super.m(); }} }} Object.setPrototypeOf({{}}, B); console.log(new H().c());"
    )));
}
