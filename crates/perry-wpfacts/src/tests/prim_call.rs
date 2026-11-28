//! NOTUNIT7 (R9READ10 without an object): a call whose receivers are only
//! primitives, on whose families the key is absent, throws a TypeError on
//! every kind: a String / Number / Boolean reads `undefined` there
//! (ECMA-262 10.1.8.1) and calling it throws (13.3.6.2 EvaluateCall,
//! 7.3.14); a nullish one throws in ToObject (7.1.18). Beside an object
//! these are the TypeError arms of PerKind; with the object gone (less
//! flow) the site was refused ("primitive receiver").
//! Controls: a nullish-only call stays `Throws`; a call of a member the
//! Number family has (`toFixed`) stays refused here.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the calls of `key`, in source order.
fn calls(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("PRIMCALL {key}: {v:?}");
    v
}

const SRC: &str = "const c = Math.random() > 0.5;\n\
    const d = Math.random() > 0.5;\n\
    const p: any = c ? 1 : d ? true : undefined;\n\
    const u: any = c ? null : undefined;\n";

/// RED on base: Number / Boolean / undefined receivers only, `copy` absent
/// on their families: every kind throws.
#[test]
fn an_absent_key_call_on_primitives_is_per_kind() {
    let pf = one(&format!("{SRC}p.copy(2);\n"));
    let v = calls(&pf, "copy");
    assert!(!v.is_empty() && v.iter().all(|v| v == "PerKind"), "{v:?}");
}

/// Control: nullish receivers only stay `Throws`.
#[test]
fn a_nullish_only_call_throws() {
    let pf = one(&format!("{SRC}u.copy(2);\n"));
    let v = calls(&pf, "copy");
    assert!(!v.is_empty() && v.iter().all(|v| v == "Throws"), "{v:?}");
}

/// Control: a member the Number family has is no TypeError arm.
#[test]
fn a_family_member_call_is_not_per_kind() {
    let pf = one(&format!("{SRC}p.toFixed(2);\n"));
    let v = calls(&pf, "toFixed");
    assert!(!v.is_empty() && v.iter().all(|v| v != "PerKind"), "{v:?}");
}
