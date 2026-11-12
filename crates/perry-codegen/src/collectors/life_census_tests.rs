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
        None,
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
        Some(&Body {
            label: "t".into(),
            suspends: true,
            func: 0,
            direct: false,
        }),
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
    let rows = classify(&census, &m.init, &boxed, &globals, &sc, None);
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

const SIDES: &str = "class Box { v: any; constructor(v: any) { this.v = v; } }
class SBox { _y: any; constructor(v: any) { this.y = v; } set y(x: any) { this._y = x; } get y(): any { return this._y; } }
class M { k: number; constructor() { this.k = 1; } run(o: any): number { return 1; } static make(o: any): number { return 2; } }
function drop1(o: any): number { return 1; }
function keep1(o: any): void { (globalThis as any).g = o; }
function mk(): any { return { r: 1 }; }
function mk2(): any { return { r: 2 }; }
export function mk3(): any { return { r: 3 }; }
function mk4(): any { return { r: 4 }; }
function mk5(): any { return { r: 5 }; }
const hold: any = mk5;
let s = 0;
s += drop1({ a: { n: 1 } });
keep1({ b: 1 });
console.log({ c: 1 });
console.log([7, 8]);
s += Object.keys({ o: 1 }).length;
s += Array.from([4, 5]).length;
const arr2 = [3, 4];
s += arr2.map((x: number) => x + 1).length;
const bx = new Box({ z: 1 });
const sb = new SBox({ y: 1 });
const mm = new M();
s += mm.run({ d: 1 });
const mg = new M();
s += mg.run({ d2: 1 });
s += M.make({ e: 1 });
const f2 = (o: any) => 1;
s += f2({ h: 1 });
const u: any = (globalThis as any).u;
s += u.go({ i: 1 });
setTimeout(() => 1, 0);
mk();
const x2 = mk2();
keep1(x2);
console.log(s, bx, sb, hold);
";

/// Every body of `m` through the census, then the return sides.
fn census_rows(census: &ModuleCensus, m: &perry_hir::Module) -> Vec<Row> {
    let (boxed, globals) = (HashSet::new(), HashMap::new());
    let (n, o, a) = (HashMap::new(), HashMap::new(), HashMap::new());
    let sc = Scalar {
        news: &n,
        objects: &o,
        arrays: &a,
    };
    let mut rows = classify(census, &m.init, &boxed, &globals, &sc, None);
    let mut bodies: Vec<&[Stmt]> = m.functions.iter().map(|f| f.body.as_slice()).collect();
    for c in &m.classes {
        bodies.extend(c.constructor.iter().map(|f| f.body.as_slice()));
        bodies.extend(c.methods.iter().map(|f| f.body.as_slice()));
    }
    for b in bodies {
        let body = census.bodies.get(&(b.as_ptr() as usize)).cloned();
        rows.extend(classify(census, b, &boxed, &globals, &sc, body.as_ref()));
    }
    census.finish(rows)
}

fn class_id_of(m: &perry_hir::Module, name: &str) -> (u32, FuncId) {
    let c = m.classes.iter().find(|c| c.name == name).expect("class");
    (c.id, c.constructor.as_ref().expect("ctor").id)
}

/// Column 9 takes every value of its vocabulary on one program: compiled
/// callees resolved as the lowering resolves them (direct, local bodies,
/// field bodies, static), unresolved targets with their reason, builtins
/// (a global object's method and dedicated HIR nodes), literal fields with
/// the literal's verdict in column 10, constructor fates (a storing
/// constructor keeps by the new object; a setter keeps), and return sides.
#[test]
fn census_reports_receiving_sides() {
    let m = lower(SIDES);
    let (box_id, box_ctor) = class_id_of(&m, "Box");
    let (sbox_id, sbox_ctor) = class_id_of(&m, "SBox");
    let (m_id, _) = class_id_of(&m, "M");
    let run = m.classes.iter().find(|c| c.name == "M").expect("M").methods[0].id;
    let recv = |cid: u32| FactsRecv {
        class_ids: vec![cid],
        nullable: false,
        checked: false,
    };
    let mut facts = ElisionFacts::default();
    facts.this_of.insert(box_ctor, recv(box_id));
    facts.this_of.insert(sbox_ctor, recv(sbox_id));
    facts.classes.insert(
        box_id,
        FactsClass {
            display: "Box".into(),
            fixed_layout: true,
            fields: vec![field("v", 0)],
        },
    );
    facts.classes.insert(
        sbox_id,
        FactsClass {
            display: "SBox".into(),
            fixed_layout: true,
            fields: vec![field("_y", 0)],
        },
    );
    let mut runf = field("run", 1);
    runf.fns = vec![crate::codegen::FactsBody {
        func_id: run,
        params: 1,
        closure: false,
    }];
    facts.classes.insert(
        m_id,
        FactsClass {
            display: "M".into(),
            fixed_layout: true,
            fields: vec![field("k", 0), runf],
        },
    );
    facts.locals.insert(id_of(&m, "Let", "mm"), recv(m_id));
    facts.locals.insert(
        id_of(&m, "Let", "mg"),
        FactsRecv {
            checked: true,
            ..recv(m_id)
        },
    );
    // The closure `f2` is bound to.
    let closure_id = |name: &str| -> u32 {
        m.init
            .iter()
            .find_map(|s| match s {
                Stmt::Let {
                    name: n,
                    init: Some(Expr::Closure { func_id, .. }),
                    ..
                } if n == name => Some(*func_id),
                _ => None,
            })
            .expect("closure binding")
    };
    facts.local_bodies.insert(
        id_of(&m, "Let", "f2"),
        vec![crate::codegen::FactsBody {
            func_id: closure_id("f2"),
            params: 1,
            closure: true,
        }],
    );
    let census = ModuleCensus::new(&m, &Arc::new(facts));
    let rows = census_rows(&census, &m);
    let sides: HashSet<&str> = rows.iter().map(|r| r.6.as_str()).collect();
    for want in [
        "compiled:direct:Dropped",
        "compiled:direct:Kept:use(PutValueSet.value)",
        "compiled:field-bodies:Dropped",
        "compiled:local-bodies:Dropped",
        "builtin:global.log",
        "builtin:ObjectKeys",
        "builtin:ArrayFrom",
        "new:kept-by-new-object",
        "new:Kept:use(PutValueSet.value-to-this)",
        "unknown-target:receiver-no-facts",
        "unknown-target:receiver-guarded",
        "unknown-target:other-module",
        "return:all-drop",
        "return:kept:local:heap:call-kept:direct function",
        "return:exported",
        "return:no-visible-caller",
        "return:function-escapes",
    ] {
        assert!(sides.contains(want), "missing {want}: {rows:#?}");
    }
    // `<census>.params.tsv`: one row per parameter and `this`, with the
    // first keeping use of a `Kept` one.
    let mut functions: Vec<&perry_hir::Function> = m.functions.iter().collect();
    functions.extend(census.callers.closures.iter());
    let classes: Vec<&perry_hir::Class> = m.classes.iter().collect();
    let params = super::params_text("m", &functions, &classes, &census.fates, &census.callers);
    let row = |body: &str, param: &str| -> String {
        params
            .lines()
            .find(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                c[1] == body && c[3] == param
            })
            .unwrap_or_else(|| panic!("no row {body} {param}: {params}"))
            .to_string()
    };
    assert!(row("drop1", "0:o").ends_with("\tdirect\tDropped"), "{params}");
    assert!(row("keep1", "0:o").ends_with("\tdirect\tKept:use(PutValueSet.value)"), "{params}");
    assert!(row("Box.constructor", "0:v").ends_with("\tKeptByReceiver"), "{params}");
    assert!(row("M.run", "this").contains("\tthis\t"), "{params}");
    assert!(
        sides.iter().any(|s| s.starts_with("compiled:static")),
        "{rows:#?}"
    );
    // An array literal and a closure handed to runtime code.
    assert!(
        rows.iter()
            .any(|r| r.0 == Kind::Array && r.6 == "builtin:global.log"),
        "{rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.0 == Kind::Closure && r.6.starts_with("builtin:")),
        "{rows:#?}"
    );
    // `{n: 1}` is a field of `{a: ...}`, itself an argument.
    assert!(
        rows.iter()
            .any(|r| r.6 == "literal-field" && r.7 == "none:argument"),
        "{rows:#?}"
    );
}

/// The store question is `wpfacts_access::facts_store` on `this`'s
/// receiver fact: the read rule plus an unguarded receiver and no
/// interpreted code.
#[test]
fn codegen_proof_stores_are_facts_store() {
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
    facts.this_of.insert(5, plain.clone());
    facts.this_of.insert(
        6,
        FactsRecv {
            checked: true,
            ..plain.clone()
        },
    );
    {
        let p = CodegenProof { facts: &facts };
        assert_eq!(p.store(5, "x"), Some(Proof::Proven));
        // Guarded: a store is never a bet.
        assert_eq!(p.store(6, "x"), None);
        // Not a birth data field (an accessor, an inherited key).
        assert_eq!(p.store(5, "y"), None);
        // No `this` facts.
        assert_eq!(p.store(7, "x"), None);
    }
    assert!(crate::expr::wpfacts_access::facts_store(&facts, &plain, "x").is_ok());
    facts.interp_guard = true;
    let p = CodegenProof { facts: &facts };
    assert_eq!(p.store(5, "x"), None);
}

/// A binding without a receiver fact: `no-facts:module` (no facts at all),
/// `escaped` (facts, but no receiver or class facts in the module),
/// `unreached-body` (the solve gave other bindings a representation, not
/// this one), else `no-facts:binding`.
#[test]
fn census_tells_unreached_and_escaped_from_no_facts() {
    let m = lower(SRC);
    let (n, o, a) = (HashMap::new(), HashMap::new(), HashMap::new());
    let sc = Scalar {
        news: &n,
        objects: &o,
        arrays: &a,
    };
    let count = |facts: ElisionFacts, verdict: &str| {
        let census = ModuleCensus::new(&m, &Arc::new(facts));
        let rows = classify(&census, &m.init, &HashSet::new(), &HashMap::new(), &sc, None);
        rows.iter().filter(|r| r.4 == verdict).count()
    };
    assert_eq!(count(ElisionFacts::default(), "heap:no-facts:module"), 4);
    let mut reps = ElisionFacts::default();
    reps.local_reps.insert(id_of(&m, "Let", "a"), FactsRep::Boxed);
    assert_eq!(count(reps.clone(), "heap:escaped"), 4);
    let recv = FactsRecv {
        class_ids: vec![1],
        nullable: false,
        checked: false,
    };
    let mut facts = reps;
    for name in ["a", "b", "d"] {
        facts.locals.insert(id_of(&m, "Let", name), recv.clone());
    }
    facts.classes.insert(
        1,
        FactsClass {
            display: "P".into(),
            fixed_layout: true,
            fields: vec![field("x", 0), field("y", 1)],
        },
    );
    assert_eq!(count(facts.clone(), "heap:unreached-body"), 1);
    assert_eq!(count(facts.clone(), "heap:no-facts:binding"), 0);
    facts
        .local_reps
        .insert(id_of(&m, "Let", "c"), FactsRep::Boxed);
    assert_eq!(count(facts.clone(), "heap:unreached-body"), 0);
    assert_eq!(count(facts, "heap:no-facts:binding"), 1);
}
