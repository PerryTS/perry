// GUARDS3 (2026-11-26): the per-class shadow-write blockers
// (`ProgramFacts::shadow_writes`) codegen's prototype-census binding sites
// read instead of the shadow-bit tests. A program that shadows a method (an
// own store, a definition, a computed-key store, a relink, a prototype
// store) blocks that class; one that does not leaves it free.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h: Vec<(String, Module)> = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

const BASE: &str = "class A { foo(): number { return 1; } bar(): number { return 10; } }
class B extends A { foo(): number { return 2; } }
class C extends B { baz(): number { return 3; } }
function run(xs: A[]): number { let s = 0; for (let i = 0; i < xs.length; i++) { s += xs[i].foo() * 7 + xs[i].bar(); } return s; }
const keys = JSON.parse('[\"zz\",\"yy\"]') as string[];
const plain: any = {}; for (const k of keys) plain[k] = k.length;
const other: any = { q: 1 }; Object.setPrototypeOf(other, { r: 2 });
const xs: A[] = [new A(), new B(), new C(), new B()];
console.log(run(xs), plain.zz, other.r);
";

fn class(pf: &crate::ProgramFacts, name: &str) -> crate::ShadowWrites {
    let id = pf
        .templates
        .iter()
        .filter(|t| t.class_id.is_some())
        .find(|t| t.display.split(|c: char| !c.is_alphanumeric()).any(|w| w == name))
        .and_then(|t| t.class_id)
        .unwrap_or_else(|| panic!("class {name}"));
    pf.shadow_writes.get(&id).cloned().unwrap_or_else(|| panic!("no entry for {name}"))
}

fn inst_free(w: &crate::ShadowWrites, m: &str) -> bool {
    w.inst_any.is_none() && !w.inst_names.contains(m)
}

fn proto_free(w: &crate::ShadowWrites, m: &str) -> bool {
    w.proto_any.is_none() && w.proto_relinked.is_none() && !w.proto_names.contains(m)
}

#[test]
fn guards3_plain_program_shadows_nothing() {
    let pf = facts(BASE);
    for c in ["A", "B", "C"] {
        let w = class(&pf, c);
        for m in ["foo", "bar"] {
            assert!(inst_free(&w, m), "{c}.{m}: {w:?}");
            assert!(proto_free(&w, m), "{c}.{m}: {w:?}");
        }
    }
}

#[test]
fn guards3_own_store_blocks_instance() {
    let pf = facts(&format!("{BASE} (xs[1] as any).foo = function () {{ return 50; }};"));
    let w = class(&pf, "B");
    assert!(!inst_free(&w, "foo"), "{w:?}");
}

#[test]
fn guards3_define_property_blocks_instance() {
    let pf = facts(&format!(
        "{BASE} Object.defineProperty(xs[2], \"foo\", {{ value: function () {{ return 60; }} }});"
    ));
    let w = class(&pf, "C");
    assert!(!inst_free(&w, "foo"), "{w:?}");
}

#[test]
fn guards3_unknown_key_store_blocks_instance() {
    let pf = facts(&format!(
        "{BASE} const names = JSON.parse('[\"foo\"]') as string[]; for (const n of names) (xs[3] as any)[n] = function () {{ return 70; }};"
    ));
    let w = class(&pf, "B");
    assert!(!inst_free(&w, "foo"), "{w:?}");
    // The store wrote an instance, not the prototype (a different object).
    assert!(proto_free(&w, "foo"), "{w:?}");
}

#[test]
fn guards3_set_prototype_of_blocks_instance() {
    let pf = facts(&format!(
        "{BASE} class D {{ foo(): number {{ return 4; }} }} Object.setPrototypeOf(xs[0], D.prototype);"
    ));
    let w = class(&pf, "A");
    assert!(w.inst_any.is_some(), "{w:?}");
}

#[test]
fn guards3_prototype_computed_store_blocks_prototype() {
    let pf = facts(&format!(
        "{BASE} const m = JSON.parse('[\"bar\"]') as string[]; (B.prototype as any)[m[0]] = function () {{ return 90; }};"
    ));
    let w = class(&pf, "B");
    assert!(!proto_free(&w, "bar"), "{w:?}");
}

#[test]
fn guards3_prototype_relink_blocks_prototype() {
    let pf = facts(&format!("{BASE} Object.setPrototypeOf(B.prototype, {{ bar() {{ return 5; }} }});"));
    let w = class(&pf, "B");
    assert!(w.proto_relinked.is_some(), "{w:?}");
}
