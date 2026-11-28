//! SITEB7: an accessor a class template declares (a getter or setter on its
//! prototype) under one key does not decide a read of another key. OrdinaryGet
//! (ECMA-262 10.1.8.1) of `k` asks the object and its chain for `k` only: an
//! instance key a store may add after birth reads that own data property or
//! `undefined` (`OwnDataOrAbsent`), whatever other keys are accessors.
//! Controls: the accessor under the read key (on the class or on its parent),
//! and an accessor defined at birth under a key not known.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the reads (or calls) of `key`, no stores, in source order.
fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call && !s.access.is_store())
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("SITEB7 {key} call={call}: {v:?}");
    v
}

const OWN: &str = "OwnDataOrAbsent";
const B: &str = "const c = Math.random() > 0.5;\n\
    class B { y: number; constructor() { this.y = 0; } get g(): number { return this.y; } }\n\
    const b: any = new B();\n";

/// RED on base: `k` is added after birth by a store that may not run; the
/// class getter `g` is another key.
#[test]
fn a_late_key_beside_a_getter_reads_own_data_or_absent() {
    let pf = one(&format!("{B}if (c) b.k = 2;\nconsole.log(b.k, b.g);\n"));
    let v = sites(&pf, "k", false);
    assert!(!v.is_empty() && v.iter().all(|v| v == OWN), "{v:?}");
}

/// RED on base: a key no store names, on an instance whose layout grows
/// after birth (`z`): absent on the object and its chain.
#[test]
fn an_unwritten_key_beside_a_getter_reads_absent() {
    let pf = one(&format!("{B}if (c) b.z = 1;\nconsole.log(b.q, b.g);\n"));
    let v = sites(&pf, "q", false);
    assert!(!v.is_empty() && v.iter().all(|v| v == OWN), "{v:?}");
}

/// Control: the getter is under the read key.
#[test]
fn a_getter_under_the_read_key_is_not_own_data() {
    let pf = one(
        "const c = Math.random() > 0.5;\n\
         class B { y: number; constructor() { this.y = 0; } get k(): number { return this.y; } }\n\
         const b: any = new B();\nif (c) b.z = 1;\nconsole.log(b.k);\n",
    );
    let v = sites(&pf, "k", false);
    assert!(!v.is_empty() && v.iter().all(|v| v != OWN), "{v:?}");
}

/// Control: the parent class has the getter under the read key.
#[test]
fn a_parent_getter_under_the_read_key_is_not_own_data() {
    let pf = one(
        "const c = Math.random() > 0.5;\n\
         class P { get k(): number { return 1; } }\n\
         class B extends P { y: number; constructor() { super(); this.y = 0; } }\n\
         const b: any = new B();\nif (c) b.z = 1;\nconsole.log(b.k);\n",
    );
    let v = sites(&pf, "k", false);
    assert!(!v.is_empty() && v.iter().all(|v| v != OWN), "{v:?}");
}

/// Control: an accessor defined on the object at birth under a key not known.
#[test]
fn a_birth_accessor_under_an_unknown_key_is_not_own_data() {
    let pf = one(
        "const c = Math.random() > 0.5;\nconst n = process.argv[2] ?? \"k\";\n\
         class B { y: number; constructor() { this.y = 0; Object.defineProperty(this, n, { get() { return 1; } }); } }\n\
         const b: any = new B();\nif (c) b.z = 1;\nconsole.log(b.k);\n",
    );
    let v = sites(&pf, "k", false);
    assert!(!v.is_empty() && v.iter().all(|v| v != OWN), "{v:?}");
}
