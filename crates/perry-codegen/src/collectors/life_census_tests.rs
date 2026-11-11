//! The census reports each verdict: `frame`, `scalar`, `heap:<reason>` and
//! `none:<position>`, on a lowered program with hand-made receiver facts.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::*;
use crate::codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv, FactsRep};

const SRC: &str = "class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }\n\
function score(q: P): number { return q.x + q.y; }\n\
function keep(r: P): P { return r; }\n\
let s = 0;\n\
for (let i = 0; i < 3; i++) {\n\
  const a = new P(i, 1);\n\
  s += score(a);\n\
  const b = new P(i, 2);\n\
  keep(b);\n\
  const c = new P(i, 3);\n\
  s += score(c);\n\
  const d = new P(i, 4);\n\
  s += d.x;\n\
}\n\
s += score(new P(1, 2));\n\
console.log(s);\n";

fn lower(src: &str) -> perry_hir::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "lc.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "lc", "lc.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The id of the binding (`Let` or `Param`) named `name`.
fn id_of(m: &perry_hir::Module, what: &str, name: &str) -> u32 {
    let text = format!("{m:?}");
    let pat = format!("{what} {{ id: ");
    let mut rest = text.as_str();
    while let Some(i) = rest.find(&pat) {
        rest = &rest[i + pat.len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let j = rest.find("name: \"").expect("named");
        if rest[j + 7..].starts_with(&format!("{name}\"")) {
            return num.parse().expect("id");
        }
    }
    panic!("no {what} {name}");
}

fn field(key: &str, slot: u32) -> FactsField {
    FactsField {
        key: key.into(),
        slot,
        readonly: false,
        kind_mask: 1,
        ta_stable: false,
        ta_kind: None,
        declared: false,
        reserved: false,
        fns: vec![],
        classes: vec![],
        nullable: false,
        rep: FactsRep::Boxed,
        rest: false,
    }
}

#[test]
fn census_reports_every_verdict() {
    let m = lower(SRC);
    let recv = FactsRecv {
        class_ids: vec![1],
        nullable: false,
        checked: false,
    };
    let mut facts = ElisionFacts::default();
    for (what, name) in [
        ("Param", "q"),
        ("Param", "r"),
        ("Let", "a"),
        ("Let", "b"),
        ("Let", "d"),
    ] {
        facts.locals.insert(id_of(&m, what, name), recv.clone());
    }
    facts.classes.insert(
        1,
        FactsClass {
            display: "P".into(),
            fixed_layout: true,
            fields: vec![field("x", 0), field("y", 1)],
        },
    );
    let census = ModuleCensus::new(&m, &Arc::new(facts));
    let d = id_of(&m, "Let", "d");
    let scalar: HashMap<u32, String> = [(d, "P".to_string())].into_iter().collect();
    let none_o = HashMap::new();
    let none_a = HashMap::new();
    let sc = Scalar {
        news: &scalar,
        objects: &none_o,
        arrays: &none_a,
    };
    let rows = classify(
        &census,
        &m.init,
        &HashSet::new(),
        &HashMap::new(),
        &sc,
        false,
    );
    let verdicts: HashMap<&str, Vec<&str>> = rows.iter().fold(HashMap::new(), |mut acc, r| {
        acc.entry(r.4.as_str()).or_default().push(r.5.as_str());
        acc
    });
    assert_eq!(verdicts.get("frame").map(Vec::len), Some(1), "{rows:?}");
    assert_eq!(verdicts.get("scalar").map(Vec::len), Some(1), "{rows:?}");
    assert!(
        verdicts.contains_key("heap:call-returned:direct function"),
        "{rows:?}"
    );
    assert!(verdicts.contains_key("heap:no-facts:binding"), "{rows:?}");
    assert_eq!(
        verdicts.get("none:argument"),
        Some(&vec!["direct function:Dropped"]),
        "{rows:?}"
    );
    // The same body in an async function: the creating function blocks first.
    let none_n = HashMap::new();
    let sc = Scalar {
        news: &none_n,
        objects: &none_o,
        arrays: &none_a,
    };
    let rows = classify(
        &census,
        &m.init,
        &HashSet::new(),
        &HashMap::new(),
        &sc,
        true,
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.4 == "heap:async-or-generator")
            .count(),
        4,
        "{rows:?}"
    );
}

/// The fate's read question is `facts_read` on the receiver binding: a bare
/// load is Proven, a guarded one a bet on its site, and a receiver without
/// facts (or not a local or `this`) is unknown, so the fate keeps it.
#[test]
fn codegen_proof_reads_are_facts_read() {
    use perry_transform::effects::{FateProof, Proof};
    let mut facts = ElisionFacts::default();
    facts.classes.insert(
        1,
        FactsClass {
            display: "P".into(),
            fixed_layout: true,
            fields: vec![field("x", 0)],
        },
    );
    let plain = FactsRecv {
        class_ids: vec![1],
        nullable: false,
        checked: false,
    };
    let checked = FactsRecv {
        checked: true,
        ..plain.clone()
    };
    facts.locals.insert(10, plain.clone());
    facts.locals.insert(11, checked);
    facts.this_of.insert(5, plain);
    let p = CodegenProof { facts: &facts };
    assert_eq!(p.read(0, &Expr::LocalGet(10), "x", 7), Some(Proof::Proven));
    assert_eq!(p.read(0, &Expr::LocalGet(11), "x", 7), Some(Proof::Bet(7)));
    assert_eq!(p.read(5, &Expr::This, "x", 7), Some(Proof::Proven));
    // Not a birth data field (accessor, inherited key): unknown.
    assert_eq!(p.read(0, &Expr::LocalGet(10), "y", 7), None);
    // No facts for the binding or for this function's `this`.
    assert_eq!(p.read(0, &Expr::LocalGet(12), "x", 7), None);
    assert_eq!(p.read(6, &Expr::This, "x", 7), None);
    // A receiver that is neither a local nor `this`.
    assert_eq!(p.read(0, &Expr::Undefined, "x", 7), None);
}

const LITS: &str = "class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }
function sink(v: any): void { (globalThis as any).k = v; }
let s = 0;
const o = { a: 1, b: 2 };
s += o.a + o.b;
const e = { a: 1 };
sink(e);
const arr = [1, 2, 3];
s += arr[0];
const f = () => 1;
s += f();
const g = () => 2;
sink(g);
sink({ z: 1 });
s += new P(1, 2).x;
new P(3, 4);
console.log(s);
";

/// Object and array literals and closures get rows of their own kind, with
/// `scalar` from the escape analyses themselves, and the refined positions.
#[test]
fn census_reports_literals_closures_and_positions() {
    let m = lower(LITS);
    let census = ModuleCensus::new(&m, &Arc::new(ElisionFacts::default()));
    let (boxed, globals) = (HashSet::new(), HashMap::new());
    let objects = super::super::escape_objects::collect_non_escaping_object_literals(
        &m.init, &boxed, &globals,
    );
    let arrays =
        super::super::escape_arrays::collect_non_escaping_arrays(&m.init, &boxed, &globals);
    // `o` is scalar by the news analysis (its literal is an anon-shape `new`).
    let news: HashMap<u32, String> = [(id_of(&m, "Let", "o"), String::new())]
        .into_iter()
        .collect();
    let sc = Scalar {
        news: &news,
        objects: &objects,
        arrays: &arrays,
    };
    let rows = classify(&census, &m.init, &boxed, &globals, &sc, false);
    let has = |k: Kind, class: &str, verdict: &str| {
        rows.iter()
            .any(|r| r.0 == k && r.1 == class && r.4 == verdict)
    };
    let obj = |verdict: &str| rows.iter().any(|r| r.0 == Kind::Object && r.4 == verdict);
    assert!(obj("scalar"), "{rows:?}");
    assert!(obj("heap:no-facts:module"), "{rows:?}");
    // Object literals take their ordinal, never the anon class's offset 0.
    assert!(
        rows.iter().filter(|r| r.0 == Kind::Object).all(|r| r.2 > 0),
        "{rows:?}"
    );
    assert!(has(Kind::Array, "[3]", "scalar"), "{rows:?}");
    assert!(
        rows.iter().any(|r| r.0 == Kind::Closure && r.4 == "frame"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.0 == Kind::Closure && r.4 == "heap:call-kept:direct function"),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.0 == Kind::Object
            && r.4 == "none:argument"
            && r.5 == "direct function:Kept"),
        "{rows:?}"
    );
    assert!(has(Kind::New, "P", "none:field-read"), "{rows:?}");
    assert!(has(Kind::New, "P", "none:expression-statement"), "{rows:?}");
}
