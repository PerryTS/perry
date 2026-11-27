//! SITEB1: a read of a global name off the global object (`globalThis.Math`,
//! and under PN151 a bare `Math`: 9.1.1.4.6 GetBindingValue of the global
//! object record is [[Get]] of the global object). The realm gives the
//! global object its declared own data properties (ECMA-262 19.1-19.4,
//! `%Global%`): while no write the solve sees reaches the key on the global
//! object or through ⊤ (a store, a define, a delete, an escape), 10.1.8.1
//! OrdinaryGet finds the realm's value. The arm decides its receiver; the
//! site stays generic (PerKind).
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the reads of `key` off the global object, in source order.
fn at(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s
        .iter()
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("SITEB1 {key}: {v:?}");
    v
}

#[test]
fn siteb1_declared_global_read_decided() {
    for k in ["Math", "Map", "JSON", "Number"] {
        let pf = one(&format!(
            "const v: any = (globalThis as any).{k};\nconsole.log(typeof v);\n"
        ));
        let v = at(&pf, k);
        assert!(
            !v.is_empty() && v.iter().all(|v| v.starts_with("PerKind") && v.contains("GlobalThis")),
            "{k}: {v:?}"
        );
    }
}

#[test]
fn siteb1_written_global_not_decided() {
    for w in [
        "(globalThis as any).Math = { abs: (x: number) => x };\n",
        "delete (globalThis as any).Math;\n",
        "Object.defineProperty(globalThis, \"Math\", { value: 1 });\n",
        "const k: string = String(Date.now());\n(globalThis as any)[k] = 1;\n",
        "const k: string = String(Date.now());\nObject.defineProperty(globalThis, k, { value: 1 });\n",
        "(globalThis as any)[\"Ma\" + \"th\"] = { abs: (x: number) => x };\n",
    ] {
        let pf = one(&format!(
            "{w}const v: any = (globalThis as any).Math;\nconsole.log(typeof v);\n"
        ));
        let v = at(&pf, "Math");
        assert!(
            !v.is_empty() && v.iter().all(|v| !v.starts_with("PerKind")),
            "{w}: {v:?}"
        );
    }
}

#[test]
fn siteb1_undeclared_global_not_by_this_rule() {
    let pf = one("const v: any = (globalThis as any).__siteb1;\nconsole.log(typeof v);\n");
    let v = at(&pf, "__siteb1");
    assert!(v.iter().all(|v| !v.starts_with("PerKind")), "{v:?}");
}

/// A Number key names an index property of the global object (7.1.19:
/// ToString of a Number is a canonical numeric String), never `Math`, and
/// writes no other object: the read keeps the global object as its only
/// receiver. On this base the Number-key store shares the global's shape-0
/// any-key field, which every global binding read takes, so the read is
/// not decided here; under W10-IDX (`global_num_field`, its own field that
/// only a numeric name reads) the same rule decides it: then assert
/// `PerKind`.
#[test]
fn siteb1_number_key_store_not_a_write() {
    let pf = one(
        "const n: number = Date.now();\n(globalThis as any)[n] = 1;\nconst v: any = (globalThis as any).Math;\nconsole.log(typeof v);\n",
    );
    let v = at(&pf, "Math");
    assert!(
        !v.is_empty() && v.iter().all(|v| v.starts_with("PerKind") && v.contains("GlobalThis")),
        "{v:?}"
    );
}
