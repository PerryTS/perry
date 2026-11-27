//! B-del: the enumerable read `o[k]` of a for-in (ENUM1 / F-A) or of the
//! counted walk of `Object.keys(o)` (rule B) reads only enumerable keys of
//! `o`'s chain. A key the walk yields may be deleted before the read (by
//! the body, a call, an alias: ECMA-262 14.7.5.9 skips a key deleted before
//! it is processed, not one deleted after; `Object.keys` snapshots, 20.1.2.18).
//! `o[k]` is then OrdinaryGet (10.1.8.1): the own key is gone and the read
//! finds the prototype's non-enumerable builtin member
//! (`Object.prototype.toString`). The enumerable read holds only for an
//! object no delete reaches (`Model::enum_kept`).
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn recv(pf: &ProgramFacts, key: &str) -> String {
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("ENUMDEL {key}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v.join(" | ")
}

fn has_member(r: &str) -> bool {
    r.contains("ObjectProtoMethod")
}

const JSON_D: &str = "const d: any = JSON.parse(String(Math.random()) + '{\"toString\": 1}');\n";

#[test]
fn an_object_keys_read_after_a_delete_reads_the_chain() {
    let pf = one(&format!(
        "{JSON_D}for (const k of Object.keys(d)) {{ delete d[k]; const v = d[k]; console.log(v.wq); }}\n"
    ));
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}

#[test]
fn a_for_in_read_after_a_delete_reads_the_chain() {
    let pf = one(
        "const d: any = { toString: 1 };\nfor (const k in d) { delete d[k]; const v = d[k]; console.log(v.wq); }\n",
    );
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}

#[test]
fn a_delete_in_a_callee_reaches_the_read() {
    let pf = one(&format!(
        "function del(o: any, k: string) {{ delete o[k]; }}\n{JSON_D}for (const k of Object.keys(d)) {{ del(d, k); const v = d[k]; console.log(v.wq); }}\n"
    ));
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}

#[test]
fn a_delete_after_the_read_still_counts() {
    // The delete of the first iteration precedes the read of the second
    // (flow-insensitive per object): the walk's read sees the chain.
    let pf = one(
        "const d: any = { toString: 1, a: 2 };\nfor (const k in d) { const v = d[k]; console.log(v.wq); delete d.toString; }\n",
    );
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}

#[test]
fn control_no_delete_keeps_the_enumerable_read() {
    let pf = one("const d: any = { toString: 1 };\nfor (const k in d) { const v = d[k]; console.log(v.wq); }\n");
    let r = recv(&pf, "wq");
    assert!(!has_member(&r), "{r}");
}

#[test]
fn control_a_delete_of_another_object_keeps_the_enumerable_read() {
    let pf = one(&format!(
        "const e: any = {{ q: 1 }};\n{JSON_D}for (const k of Object.keys(d)) {{ delete e[k]; const v = d[k]; console.log(v.wq); }}\n"
    ));
    let r = recv(&pf, "wq");
    assert!(!has_member(&r), "{r}");
}
