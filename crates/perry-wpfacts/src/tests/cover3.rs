// COVER3 (2026-11-18): a named store whose receivers are objects that reach
// ONE setter (10.1.9.2 step 3 / 7) beside `undefined` / `null` is
// `AccessorSet` with the TypeError arm: PutValue's ToObject throws for the
// nullish base after the value was evaluated (6.2.5.6 step 3.a, 13.15.2),
// and codegen keeps the one receiver test in front of the direct setter
// call. Any other primitive receiver keeps the refusal.
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

/// The (verdict, receivers) of the sites of `key`, stores or reads.
fn sites(pf: &crate::ProgramFacts, key: &str, store: bool) -> Vec<(String, Vec<String>)> {
    let v: Vec<(String, Vec<String>)> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && s.access.is_store() == store)
        .map(|s| (format!("{:?}", s.verdict), s.receivers.clone()))
        .collect();
    eprintln!("COVER3 .{key} store={store} {v:?}");
    v
}

const SHAPE: &str = "class Base {\n  version = 0;\n  _alphaTest = 0;\n\
  get alphaTest(): number { return this._alphaTest; }\n\
  set alphaTest(v: number) { if (this._alphaTest > 0 !== v > 0) this.version++; this._alphaTest = v; }\n}\n\
class Mid extends Base { isMid = true; }\n\
class A extends Mid { isA = true; }\nclass B extends Mid { isB = true; }\nclass C extends Mid { isC = true; }\n\
const objs: any[] = [];\n\
for (let i = 0; i < 64; i++) objs.push(i % 3 === 0 ? new A() : i % 3 === 1 ? new B() : new C());\n";

// RED: the PTP-04a micro shape, `objs[i & 63].alphaTest = v` on a 64-slot
// `any[]` (the element read can be `undefined`).
#[test]
fn cover3_accessor_set_beside_undefined() {
    let pf = facts(&format!(
        "{SHAPE}function set(n: number) {{ for (let i = 0; i < n; i++) objs[i & 63].alphaTest = i & 1; return objs[0].version; }}\n\
         console.log(set(Number(process.argv[2] ?? 100)));\n"
    ));
    let s = sites(&pf, "alphaTest", true);
    assert!(!s.is_empty(), "no alphaTest store");
    for (v, r) in &s {
        assert_eq!(v, "AccessorSet", "store verdict {v} recv {r:?}");
        assert!(
            r.iter().any(|x| x == "primitive:undefined (TypeError)"),
            "store has no TypeError arm: {r:?}"
        );
    }
}

// KEEP: a receiver that can be a string beside the objects is no setter
// call (a string base runs [[Set]] on its wrapper, 6.2.5.6 step 3.b).
#[test]
fn cover3_string_receiver_keeps_refusal() {
    let pf = facts(&format!(
        "{SHAPE}function set(n: number, k: boolean) {{ for (let i = 0; i < n; i++) (k ? (\"s\" as any) : objs[i & 63]).alphaTest = i & 1; return objs[0].version; }}\n\
         console.log(set(Number(process.argv[2] ?? 100), process.argv[3] === \"k\"));\n"
    ));
    let s = sites(&pf, "alphaTest", true);
    assert!(!s.is_empty(), "no alphaTest store");
    for (v, r) in &s {
        assert!(
            v.contains("store: primitive receiver"),
            "string receiver store verdict {v} recv {r:?}"
        );
    }
}

// KEEP: a number receiver too.
#[test]
fn cover3_number_receiver_keeps_refusal() {
    let pf = facts(&format!(
        "{SHAPE}function set(n: number, k: boolean) {{ for (let i = 0; i < n; i++) (k ? (i as any) : objs[i & 63]).alphaTest = i & 1; return objs[0].version; }}\n\
         console.log(set(Number(process.argv[2] ?? 100), process.argv[3] === \"k\"));\n"
    ));
    let s = sites(&pf, "alphaTest", true);
    assert!(!s.is_empty(), "no alphaTest store");
    for (v, r) in &s {
        assert!(
            v.contains("store: primitive receiver"),
            "number receiver store verdict {v} recv {r:?}"
        );
    }
}
