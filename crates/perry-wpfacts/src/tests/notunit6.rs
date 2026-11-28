//! NOTUNIT6 (rule NUM-ACC): a read under a Number key runs only the
//! accessors a Number key may name. ToPropertyKey of a Number is its
//! ToString (ECMA-262 7.1.19, 7.1.12.1, 6.1.6.1.20), a canonical numeric
//! String; OrdinaryGet (10.1.8.1) of that key meets an accessor only under
//! that name (or one defined under a computed key). Once any accessor has a
//! numeric name, `o[i]` with a Number `i` ran every getter of `o`'s chain
//! (`width` of a class among them) and joined their results into the read.
//! Controls: the named read still runs the getter, and a getter under a
//! numeric name still runs for a Number key.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the reads of `key`, in source order.
fn sites(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && !s.access.is_store())
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("NOTUNIT6 {key}: {v:?}");
    v
}

const SRC: &str = "const c = Math.random() > 0.5;\n\
    class V { x: any; constructor(x: any) { this.x = x; } get width(): any { return this.x; } }\n\
    class P { q: number; constructor() { this.q = 1; } }\n\
    const ta = new Float32Array(4);\n\
    const v = new V(ta);\n\
    const n: any = { get 0(): any { return ta; } };\n\
    const i = c ? 0 : 1;\n\
    const arr: any = c ? v : [new P()];\n";

/// RED on base: `arr[i]` with a Number `i` ran `V`'s `width` getter, so the
/// typed array met the read of `e.q`.
#[test]
fn a_number_key_runs_no_named_getter() {
    let pf = one(&format!("{SRC}const e: any = arr[i];\nconsole.log(e.q);\n"));
    let v = sites(&pf, "q");
    assert!(!v.is_empty() && v.iter().all(|v| !v.contains("TypedArray")), "{v:?}");
}

/// Control: the named read runs the getter.
#[test]
fn a_named_read_runs_the_getter() {
    let pf = one(&format!("{SRC}const w: any = v.width;\nconsole.log(w.k1);\n"));
    let v = sites(&pf, "k1");
    assert!(!v.is_empty() && v.iter().all(|v| v.contains("TypedArray")), "{v:?}");
}

/// Control: a getter under a numeric name runs for a Number key.
#[test]
fn a_number_key_runs_a_numeric_getter() {
    let pf = one(&format!("{SRC}const m: any = n[i];\nconsole.log(m.k2);\n"));
    let v = sites(&pf, "k2");
    assert!(!v.is_empty() && v.iter().all(|v| v.contains("TypedArray")), "{v:?}");
}
