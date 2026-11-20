//! A value read out of `Object.fromEntries` or `Map.groupBy` holds what
//! the program put in (BUFEQ19). Patching `Buffer.prototype` through such
//! a value is a write of it: a Buffer member call is then no NativeMember.
//! Each program has its control: the same flow of a plain object, no
//! patch, keeps the NativeMember.
//! - W1: an entry's `[0]`/`[1]` (a read under some Number, `NUM_KEY`) of
//!   an array reads its elements (`Model::field`, `is_num_atom`).
//! - W2: `Map.groupBy` bears a Map: `get` reads its groups.
use crate::model::{Cons, Key, Model};
use crate::{ProgramFacts, Verdict};

fn parse(src: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers")
}

fn facts(src: &str) -> ProgramFacts {
    let hir = parse(src);
    crate::compute(
        &[("t.ts".to_string(), &hir)],
        &crate::FactsConfig::default(),
    )
}

fn model(src: &str) -> Model {
    let hir = parse(src);
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// `probe` calls `a.equals(b)` and `a.readInt32BE(0)` on Buffers; `patch`
/// runs once before it.
fn program(patch: &str) -> String {
    format!(
        "const P: any = function () {{ return \"PATCHED\"; }};\n\
         function probe() {{ const a = Buffer.alloc(16); const b = Buffer.alloc(16); b[3] = 1;\n\
           console.log(String(a.equals(b)), String(a.readInt32BE(0))); }}\n\
         {patch}\n\
         probe();"
    )
}

const PATCH: &str = "p.equals = P; p.readInt32BE = P;";

/// The fixtures of /root/BUFEQ/fx-gap (g08, g08e, g08h, g08i, g08l, g08g).
fn shapes() -> Vec<(&'static str, String)> {
    let via = |v: &str| format!("((p: any) => {{ {PATCH} }})({v});");
    vec![
        ("g08", format!("const w: any = Object.fromEntries([[\"p\", Buffer.prototype]]).p; {}", via("w"))),
        ("g08e", format!("const o: any = Object.fromEntries([[\"p\", Buffer.prototype]]); const w = o.p; {}", via("w"))),
        ("g08h", format!("const o: any = Object.fromEntries(Object.entries({{ p: Buffer.prototype }})); {}", via("o.p"))),
        ("g08i", format!("const o: any = Object.assign({{}}, Object.fromEntries([[\"p\", Buffer.prototype]])); {}", via("o.p"))),
        ("g08l", "const o: any = Object.fromEntries([[\"equals\", P], [\"readInt32BE\", P]]); Object.assign(Buffer.prototype, o);".to_string()),
        ("g08g", via("(Map.groupBy([Buffer.prototype], () => \"p\") as any).get(\"p\")[0]")),
    ]
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn native(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

#[test]
fn a_patch_through_from_entries_or_map_group_by_refuses_the_buffer_native_member() {
    // Control: the same reads of an object holding no prototype.
    let pf = facts(&program(
        "const o: any = Object.fromEntries([[\"p\", {}]]); ((p: any) => { p.equals = P; })(o.p);",
    ));
    for key in ["equals", "readInt32BE"] {
        let v = verdicts(&pf, key);
        assert!(native(&v), "control {key}: {v:?}");
    }
    for (name, patch) in shapes() {
        let src = program(&patch);
        let pf = facts(&src);
        for key in ["equals", "readInt32BE"] {
            let v = verdicts(&pf, key);
            assert!(
                !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_))),
                "{name} {key}: {v:?}\n{src}"
            );
        }
    }
}

/// The tokens (by kind) the receiver of each `<recv>.equals = ...` store
/// may be.
fn store_targets(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for n in &m.nodes {
        let equals = n.cons.iter().any(|c| {
            matches!(c, Cons::Store { key: Key::Named(a), .. } if m.atoms[*a as usize] == "equals")
        });
        if equals {
            for t in n.pts.to_sorted() {
                out.push(format!("{:?}", m.toks[t as usize]));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn a_value_read_out_of_from_entries_or_map_group_by_points_to_the_prototype() {
    // What `Buffer.prototype` is as a store target, read straight.
    let direct = store_targets(&model(&program(&format!(
        "((p: any) => {{ {PATCH} }})(Buffer.prototype);"
    ))));
    assert!(!direct.is_empty(), "direct: no store target");
    for (name, patch) in shapes() {
        if name == "g08l" {
            continue; // its store target is `Buffer.prototype` itself
        }
        let src = program(&patch);
        let got = store_targets(&model(&src));
        for t in &direct {
            assert!(got.contains(t), "{name}: {t} missing from {got:?}\n{src}");
        }
    }
}
