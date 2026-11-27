//! ENUM1 over `Object.keys(o)`: `o[k]` with `k` an element of the keys
//! Array a counted for-of walks reads an own enumerable String key of `o`
//! (ECMA-262 20.1.2.18 Object.keys, 7.3.23 EnumerableOwnProperties: the
//! keys whose [[Enumerable]] is true), never a built-in member: every
//! built-in property is non-enumerable (clause 18). storage.ts:
//! `for (const k of Object.keys(data)) String(data[k])`, `data` a
//! `JSON.parse` result.
//! Controls: the same read under an unknown key, and the keys of
//! `Object.getOwnPropertyNames` (non-enumerable keys included).
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
    eprintln!("ENUMK {key}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v.join(" | ")
}

fn has_member(r: &str) -> bool {
    r.contains("Array.prototype") || r.contains("ObjectProtoMethod")
}

const JSON_D: &str = "const d = JSON.parse(String(Math.random())) as Record<string, any>;\n";

#[test]
fn an_object_keys_read_of_a_json_value_reads_no_builtin_member() {
    let pf = one(&format!(
        "{JSON_D}for (const k of Object.keys(d)) {{ const v = d[k]; console.log(v.wq); }}\n"
    ));
    let r = recv(&pf, "wq");
    assert!(!has_member(&r), "{r}");
}

#[test]
fn control_an_unknown_key_admits_members() {
    let pf = one(&format!(
        "{JSON_D}const k: any = String(Math.random());\nconst v = d[k];\nconsole.log(v.wq);\n"
    ));
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}

#[test]
fn control_own_property_names_admit_members() {
    let pf = one(&format!(
        "{JSON_D}for (const k of Object.getOwnPropertyNames(d)) {{ const v = d[k]; console.log(v.wq); }}\n"
    ));
    let r = recv(&pf, "wq");
    assert!(has_member(&r), "{r}");
}
