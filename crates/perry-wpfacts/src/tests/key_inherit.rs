//! KEYINH: which computed-key reads `o[k]` put built-in prototype members
//! into their result.
//! (K1) A Number key: ToPropertyKey gives a canonical numeric string
//! (ECMA-262 7.1.19 ToPropertyKey, 7.1.17 ToString of a Number, 6.1.7
//! array index / canonical numeric string): no built-in prototype member
//! has such a name (ECMA-262 20.1.3, 23.1.3) and a locked prototype cannot
//! gain one (perry-abi lock.rs), so the read yields the receiver's numeric
//! properties and `undefined`, never an `Array.prototype` method.
//! (K2) A for-in key (14.7.5.9 EnumerateObjectProperties, 14.7.5.10
//! %ForInIteratorPrototype%.next): the names of enumerable properties of
//! the enumerated object and its chain; built-in prototype members are not
//! enumerable (ECMA-262 17), so another object read under such a key
//! reaches a built-in member only under a NAME the enumerated object holds
//! as an enumerable property (an own `toString`).
//! (K3) A key of unknown string content (a JSON value): any name, so the
//! members are admitted.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The receivers of the property sites of `key`, joined.
fn recv(pf: &ProgramFacts, key: &str) -> String {
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("KEYINH {key}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v.join(" | ")
}

fn has_member(r: &str) -> bool {
    r.contains("Array.prototype") || r.contains("ObjectProtoMethod")
}

const NUM: &str = "const i = Math.floor(Math.random() * 2);\n\
    const a: any = Math.random() < 0.5 ? [{ x: 1 }] : [{ x: 2 }];\n";

#[test]
fn k1_number_key_reads_no_prototype_member() {
    let pf = one(&format!("{NUM}const v = a[i];\nconsole.log(v.x);\n"));
    let r = recv(&pf, "x");
    assert!(!has_member(&r), "{r}");
}

#[test]
fn k1_control_string_or_number_key_admits_members() {
    let pf = one(&format!(
        "{NUM}const k: any = Math.random() < 0.5 ? i : String(Math.random());\n\
         const v = a[k];\nconsole.log(v.x);\n"
    ));
    let r = recv(&pf, "x");
    assert!(r.contains("Array.prototype"), "{r}");
}

#[test]
fn k2_for_in_key_names_only_enumerated_names() {
    let pf = one(
        "const o = { a: 1, b: 2 };\n\
         const o2: any = { a: { x: 1 }, b: { x: 2 } };\n\
         for (const p in o) { const v = o2[p]; console.log(v.x); }\n",
    );
    let r = recv(&pf, "x");
    assert!(!has_member(&r), "{r}");
}

#[test]
fn k2_control_own_tostring_name_reaches_object_prototype() {
    let pf = one(
        "const o = { a: 1, toString: 2 };\n\
         const o2: any = { a: { x: 1 } };\n\
         for (const p in o) { const v = o2[p]; console.log(v.x); }\n",
    );
    let r = recv(&pf, "x");
    // The run under "toString" reads the member itself (10.1.8.1).
    assert!(r.contains("Object.prototype.toString"), "{r}");
    assert!(!r.starts_with("Throws"), "{r}");
}

#[test]
fn k2_control_enumerable_prototype_add_is_enumerated() {
    let pf = one(
        "class C { m() { return 1; } }\n\
         (C.prototype as any).extra = { x: 3 };\n\
         const c: any = new C();\n\
         for (const p in c) { const v = c[p]; console.log(v.x); }\n",
    );
    let r = recv(&pf, "x");
    assert!(r.contains("AnonShape"), "{r}");
    assert!(!r.contains("ObjectProtoMethod"), "{r}");
}

#[test]
fn k3_control_json_key_admits_members() {
    let pf = one(
        "const j: any = JSON.parse(String(Math.random()));\n\
         const a: any = Math.random() < 0.5 ? [{ x: 1 }] : [{ x: 2 }];\n\
         const v = a[j.k];\nconsole.log(v.x);\n",
    );
    let r = recv(&pf, "x");
    assert!(r.contains("Array.prototype"), "{r}");
}

#[test]
fn k2_diag_variants() {
    for src in [
        "const o = { a: 1, toString: 2 };\nconst o2: any = { a: { x: 1 } };\nfor (const p in o) { const v = o2[p]; console.log(v.x); }\n",
        "const o = { a: 1, c: 2 };\nconst o2: any = { a: { x: 1 } };\nfor (const p in o) { const v = o2[p]; console.log(v.x); }\n",
        "const o = { a: 1, toString: 2 };\nconst o2: any = { a: { x: 1 } };\nfor (const p in o) { const v = o2[p]; console.log(typeof v, v.y); }\n",
        "const o2: any = { a: { x: 1 } };\nconst k: any = Math.random() < 0.5 ? \"a\" : \"toString\";\nconst v = o2[k]; console.log(v.x);\n",
    ] {
        let pf = one(src);
        for key in ["x", "y"] {
            let v: Vec<String> = pf.sites.iter().filter(|s| s.key == key).map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
            eprintln!("KDIAG {key}: {v:?}");
        }
    }
}
