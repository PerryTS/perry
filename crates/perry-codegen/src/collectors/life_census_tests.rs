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
    let rows = classify(
        &census,
        &m.init,
        &HashSet::new(),
        &HashMap::new(),
        &scalar,
        false,
    );
    let verdicts: HashMap<&str, Vec<&str>> = rows.iter().fold(HashMap::new(), |mut acc, r| {
        acc.entry(r.3.as_str()).or_default().push(r.4.as_str());
        acc
    });
    assert_eq!(verdicts.get("frame").map(Vec::len), Some(1), "{rows:?}");
    assert_eq!(verdicts.get("scalar").map(Vec::len), Some(1), "{rows:?}");
    assert!(
        verdicts.contains_key("heap:call-returned:direct function"),
        "{rows:?}"
    );
    assert!(verdicts.contains_key("heap:no-facts"), "{rows:?}");
    assert_eq!(
        verdicts.get("none:argument"),
        Some(&vec!["direct function:Dropped"]),
        "{rows:?}"
    );
    // The same body in an async function: the creating function blocks first.
    let rows = classify(
        &census,
        &m.init,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        true,
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.3 == "heap:async-or-generator")
            .count(),
        4,
        "{rows:?}"
    );
}
