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
    // The collapsed member token, or (a named read) the member itself.
    r.contains("ObjectProtoMethod") || r.contains("Object.prototype.")
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

// E7DEL2: the read after a delete is OrdinaryGet under a key the walk
// gives (14.7.5.9): a key the program wrote on the object. A Number-keyed
// map's keys are numeric strings and a Boolean-keyed one's are "true" and
// "false": neither names a builtin member of the chain.

#[test]
fn a_numeric_keyed_walk_after_a_delete_reads_no_builtin_member() {
    let pf = one(
        "const m: any = {};\nfunction put(id: number) { const s: any = { v: id }; m[id] = s; }\nput(1); put(2);\nfor (const k in m) { const s = m[k]; delete m[k]; console.log(s.wq); }\n",
    );
    let r = recv(&pf, "wq");
    assert!(!has_member(&r), "{r}");
}

#[test]
fn nested_walks_deleting_their_keys_read_only_the_maps() {
    let pf = one(
        "const outer: any = {};\nfunction put(id: number, w: boolean) { let inner = outer[id]; if (inner === undefined) { inner = {}; outer[id] = inner; } inner[w] = { v: id }; }\nput(1, true); put(2, false);\nfor (const a in outer) { const inner = outer[a]; for (const b in inner) { const st = inner[b]; console.log(st.wq); delete inner[b]; } delete outer[a]; }\n",
    );
    let r = recv(&pf, "wq");
    assert!(!has_member(&r) && !r.contains("Function"), "{r}");
}

#[test]
fn a_named_walk_key_after_a_delete_reads_that_member() {
    // The walk gives "constructor": after the delete, OrdinaryGet finds
    // `Object.prototype.constructor`.
    let pf = one(
        "const m: any = {};\nm.constructor = { v: 1 };\nm[Number(Math.random())] = { v: 2 };\nfor (const k in m) { delete m[k]; const s = m[k]; console.log(s.wq); }\n",
    );
    let r = recv(&pf, "wq");
    assert!(r.contains("Object"), "{r}");
}
