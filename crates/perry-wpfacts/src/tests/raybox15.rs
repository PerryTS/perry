// RAYBOX15 (2026-11-16): rule A, a key absent from every instance of a
// class and from its whole prototype chain for the program's life is a fact
// of the class (`FieldFacts::absent`): OrdinaryGet (ECMA-262 10.1.8.1) of
// it gives `undefined`, so codegen lowers the read to the constant.
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// `Some(absent)` of the row `key` on class template `class`; `None` when
/// the template has no such row.
fn absent(pf: &crate::ProgramFacts, class: &str, key: &str) -> Option<bool> {
    let t = pf
        .templates
        .iter()
        .find(|t| t.display.ends_with(&format!(":{class}")))
        .expect("template");
    let r = t.fields.iter().find(|f| f.key == key).map(|f| f.absent);
    for st in pf
        .sites
        .iter()
        .filter(|st| st.key == key && !st.access.is_store())
    {
        eprintln!("RB15 site .{key} verdict={:?}", st.verdict);
    }
    if let Some(f) = t.fields.iter().find(|f| f.key == key) {
        eprintln!("RB15 row {f:?}");
    }
    eprintln!(
        "RB15 {class}.{key} absent={r:?} fixed_layout={}",
        t.fixed_layout
    );
    r
}

const CLASSES: &str = r#"
class Base { a: number; constructor() { this.a = 1; } raycast(): number { return 0; } }
class Leaf extends Base { g: number; constructor() { super(); this.g = 2; } raycast(): number { return this.g; } }
function probe(o: Base): number {
  if ((o as any).bs !== undefined) return 1;
  return o.raycast();
}
const xs: Base[] = [new Base(), new Leaf()];
let n = 0;
for (const o of xs) n += probe(o);
"#;

fn src(extra: &str) -> String {
    format!("{CLASSES}{extra}\nconsole.log(n, process.argv[2] ?? \"x\");\n")
}

#[test]
fn rb15_absent_key_on_class_family_is_a_class_fact() {
    let pf = facts(&src(""));
    assert_eq!(absent(&pf, "Leaf", "bs"), Some(true));
    assert_eq!(absent(&pf, "Base", "bs"), Some(true));
}

#[test]
fn rb15_keep_prototype_method_is_not_absent() {
    let pf = facts(&src(""));
    assert_ne!(absent(&pf, "Leaf", "raycast"), Some(true));
}

#[test]
fn rb15_keep_birth_field_is_not_absent() {
    let pf = facts(&src(""));
    assert_ne!(absent(&pf, "Leaf", "g"), Some(true));
}

#[test]
fn rb15_keep_store_after_birth_refuses() {
    let pf = facts(&src(
        "if (process.argv[2] === \"y\") (xs[1] as any).bs = 3;",
    ));
    assert_ne!(absent(&pf, "Leaf", "bs"), Some(true));
}

#[test]
fn rb15_keep_computed_key_store_refuses() {
    let pf = facts(&src("(xs[1] as any)[process.argv[2] ?? \"q\"] = 3;"));
    assert_ne!(absent(&pf, "Leaf", "bs"), Some(true));
}

/// A store onto %Object.prototype% is refused by the runtime while DEC 250
/// locks the builtin prototypes (`object_chain_locked`); the class fact
/// follows the read verdict's `absent_key` walk, so the two agree: when the
/// lock goes, both refuse together.
#[test]
fn rb15_object_prototype_store_agrees_with_the_read_verdict() {
    let pf = facts(&src(
        "if (process.argv[2] === \"y\") (Object.prototype as any).bs = 3;",
    ));
    let read_absent = pf
        .sites
        .iter()
        .filter(|st| st.key == "bs" && !st.access.is_store())
        .all(|st| matches!(st.verdict, Verdict::T0Mono | Verdict::T0Set));
    assert_eq!(absent(&pf, "Leaf", "bs"), Some(read_absent));
}

#[test]
fn rb15_keep_class_prototype_store_refuses() {
    let pf = facts(&src(
        "if (process.argv[2] === \"y\") (Base.prototype as any).bs = 3;",
    ));
    assert_ne!(absent(&pf, "Leaf", "bs"), Some(true));
}

#[test]
fn rb15_keep_define_property_getter_refuses() {
    let pf = facts(&src(
        "if (process.argv[2] === \"y\") Object.defineProperty(Base.prototype, \"bs\", { get() { return 5; } });",
    ));
    assert_ne!(absent(&pf, "Leaf", "bs"), Some(true));
}

#[test]
fn rb15_keep_set_prototype_of_refuses() {
    let pf = facts(&src(
        "if (process.argv[2] === \"y\") Object.setPrototypeOf(xs[1], { bs: 7 });",
    ));
    assert_ne!(absent(&pf, "Leaf", "bs"), Some(true));
}

#[test]
fn rb15_keep_accessor_in_class_body_is_not_absent() {
    let pf = facts(
        "class G { get bs(): number { return 4; } }\nfunction r(o: G): number { return (o as any).bs; }\nconsole.log(r(new G()), process.argv[2]);\n",
    );
    assert_ne!(absent(&pf, "G", "bs"), Some(true));
}
