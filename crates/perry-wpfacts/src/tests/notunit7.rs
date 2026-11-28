//! NOTUNIT7 (rule NUM-ACC, store side): a store under a Number key runs
//! only the setters a Number key may name. ToPropertyKey of a Number is its
//! ToString (ECMA-262 7.1.19, 6.1.6.1.20), a canonical numeric String;
//! OrdinarySet (10.1.9.2 OrdinarySetWithOwnDescriptor) of that key meets a
//! setter only under that name (or one defined under a computed key). An
//! element store `o[i] = v` with a Number `i` ran every setter of `o`'s
//! chain (`width` of a class among them) with `v`.
//! Controls: the named store still runs the setter, and a setter under a
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
    eprintln!("NOTUNIT7 {key}: {v:?}");
    v
}

const SRC: &str = "const c = Math.random() > 0.5;\n\
    class P { q: number; constructor() { this.q = 1; } }\n\
    let sink: any;\n\
    let sink0: any;\n\
    class S { set width(v: any) { sink = v; } }\n\
    const ta = new Float32Array(4);\n\
    const s = new S();\n\
    const n: any = { set 0(v: any) { sink0 = v; } };\n\
    const i = c ? 0 : 1;\n\
    const arr: any = c ? s : [new P()];\n";

/// RED on base: `arr[i] = ta` with a Number `i` ran `S`'s `width` setter,
/// so the typed array met the read of `sink.q`.
#[test]
fn a_number_key_store_runs_no_named_setter() {
    let pf = one(&format!("{SRC}arr[i] = ta;\nconsole.log(sink.q);\n"));
    let v = sites(&pf, "q");
    assert!(!v.is_empty() && v.iter().all(|v| !v.contains("TypedArray")), "{v:?}");
}

/// Control: the named store runs the setter.
#[test]
fn a_named_store_runs_the_setter() {
    let pf = one(&format!("{SRC}s.width = ta;\nconsole.log(sink.k1);\n"));
    let v = sites(&pf, "k1");
    assert!(!v.is_empty() && v.iter().all(|v| v.contains("TypedArray")), "{v:?}");
}

/// Control: a setter under a numeric name runs for a Number key.
#[test]
fn a_number_key_store_runs_a_numeric_setter() {
    let pf = one(&format!("{SRC}n[i] = ta;\nconsole.log(sink0.k2);\n"));
    let v = sites(&pf, "k2");
    assert!(!v.is_empty() && v.iter().all(|v| v.contains("TypedArray")), "{v:?}");
}
