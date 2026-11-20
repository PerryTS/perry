//! A for-in binds one key per iteration and `v[k]` in it reads that key
//! only (ECMA-262 14.7.5.6 ForIn/OfBodyEvaluation, 14.7.5.9
//! EnumerateObjectProperties). Over a subject whose enumerable keys the
//! solve knows (ENUM3's subject: the names the program wrote on it,
//! nothing under an unknown key) the body runs once per key, also when the
//! keys are written after birth (three.js `Material.setValues(materialParams)`,
//! GLTFLoader): `this[k] = v[k]` then gives `m.a` only `v.a`'s value. A
//! subject from unmodeled code, one written under an unknown key, or a
//! body that reassigns its key keeps the run with every key.
use super::*;

fn src(subject: &str, body: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ q = 2; }}
         class M {{
           a: any = 0;
           b: any = 0;
           set(v: any) {{ for (let k in v) {{ {body} }} }}
         }}
         const m: any = new M();
         {subject}
         console.log(m.a.p, m.b.q);"
    )
}

const BODY: &str = "const n = v[k]; this[k] = n;";

/// The classes the read of `key` sees.
fn read_classes(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn for_in_over_birth_keys_runs_per_key() {
    let pf = facts(&src("m.set({ a: new A(), b: new B() });", BODY));
    assert_eq!(read_classes(&pf, "p"), vec!["A"]);
    assert_eq!(read_classes(&pf, "q"), vec!["B"]);
}

#[test]
fn for_in_over_keys_written_after_birth_runs_per_key() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o.b = new B(); m.set(o);",
        BODY,
    ));
    assert_eq!(read_classes(&pf, "p"), vec!["A"]);
    assert_eq!(read_classes(&pf, "q"), vec!["B"]);
}

#[test]
fn for_in_over_a_json_subject_keeps_every_key() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o.b = new B(); m.set(o); m.set(JSON.parse(process.argv[2]));",
        BODY,
    ));
    assert!(read_classes(&pf, "p").contains(&"B".to_string()));
}

#[test]
fn for_in_over_an_unknown_subject_keeps_every_key() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o.b = new B(); m.set(o); m.set(new Function(process.argv[2])());",
        BODY,
    ));
    // `m.a` may hold anything unknown code stores: the site lists no
    // receiver set, never A alone.
    assert_ne!(read_classes(&pf, "p"), vec!["A"]);
}

#[test]
fn for_in_over_a_subject_written_under_an_unknown_key_keeps_every_key() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o[process.argv[2]] = new B(); m.set(o);",
        BODY,
    ));
    assert!(read_classes(&pf, "p").contains(&"B".to_string()));
}

#[test]
fn for_in_whose_body_reassigns_its_key_keeps_its_stores() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o.b = new B(); m.set(o);",
        "const n = v[k]; k = \"a\"; this[k] = n;",
    ));
    assert!(read_classes(&pf, "p").contains(&"B".to_string()));
}

/// `for (k in undefined)` and `for (k in null)` run no iteration (14.7.5.6
/// ForIn/OfHeadEvaluation step 6.a): they keep the per-key runs.
#[test]
fn for_in_over_undefined_or_null_runs_no_iteration() {
    let pf = facts(&src(
        "const o: any = {}; o.a = new A(); o.b = new B(); m.set(o); m.set(undefined); m.set(null);",
        BODY,
    ));
    assert_eq!(read_classes(&pf, "p"), vec!["A"]);
    assert_eq!(read_classes(&pf, "q"), vec!["B"]);
}
