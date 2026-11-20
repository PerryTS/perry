//! An `Array.prototype` method applied to a receiver that is no array
//! (`non_array_this`): what the method does to the receiver (Get `length`,
//! ToPrimitive of it, the length store, an index delete,
//! `@@isConcatSpreadable`) reads the receiver and the method only. It is
//! made once per (receiver, method) and serves every call site; before, every
//! call site made its own copy (r10a: 144,903 copies over 699 calls, every
//! `Array.prototype` method with every JSON object as `this`).
//! RED: twenty sites of one method on one plain object make one receiver
//! model. KEEP: a program `length` getter still runs from every site; the
//! `join` a plain object carries still runs for `toString` from each site,
//! and each site's result is its own call's.
use super::*;

fn solved(src: &str) -> crate::gen::Gen<'static> {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g
}

fn count_label(g: &crate::gen::Gen<'static>, label: &str) -> usize {
    g.m.labels.iter().filter(|l| **l == label).count()
}

/// Whether function `name` is an active body after the whole pipeline.
fn runs(src: &str, name: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == name)
        .expect("fn")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

fn sites(n: usize, line: &str) -> String {
    (0..n)
        .map(|i| line.replace("#", &i.to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an13_red_one_receiver_model_per_method_and_receiver() {
    let src = format!(
        "const o: any = {{ length: 2, 0: \"a\", 1: process.argv[2] ?? \"b\" }};\n{}\n",
        sites(20, "console.log(Array.prototype.indexOf.call(o, \"x#\"));")
    );
    let g = solved(&src);
    let n = count_label(&g, "array-like-this");
    assert!(n >= 1, "the method runs on the plain object: {n}");
    assert!(
        n <= 1,
        "one receiver model for (o, indexOf), not one per site: {n}"
    );
}

#[test]
fn an13_keep_length_getter_runs_from_every_site() {
    // Each of the two sites reaches its own getter body: one per object.
    let src = r#"function g1() { return 2; }
function g2() { return 1; }
const a: any = { get length() { return g1(); }, 0: "a", 1: "b" };
const b: any = { get length() { return g2(); }, 0: "c" };
console.log(Array.prototype.indexOf.call(a, "a"));
console.log(Array.prototype.includes.call(b, process.argv[2] ?? "c"));
"#;
    assert!(runs(src, "g1"), "a's getter runs");
    assert!(runs(src, "g2"), "b's getter runs");
}

#[test]
fn an13_keep_tostring_join_is_each_calls_own() {
    let src = r#"function j1() { return "j1"; }
const o: any = { length: 0, join: j1 };
const s1 = Array.prototype.toString.call(o);
const s2 = Array.prototype.toString.call(o);
console.log(s1, s2);
"#;
    assert!(runs(src, "j1"), "the program join runs");
    let g = solved(src);
    assert_eq!(count_label(&g, "array-like-this"), 1, "one receiver model");
    assert_eq!(
        count_label(&g, "array-like-join"),
        2,
        "one join call per site"
    );
}
