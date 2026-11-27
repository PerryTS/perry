//! Iterating a builtin function reads no element: it has no own
//! `@@iterator` and no internal list of entries, and neither has
//! `%Function.prototype%` (ECMA-262 20.2.3, 20.2.4; `GetIterator` throws a
//! TypeError unless the program installs an `@@iterator` on its chain, a
//! protocol call `Gen::iterate` makes beside the element read). What the
//! program stored on the function itself under an index is no element of
//! `[...Math.max]`, as for a program function.
//! Control: an Array beside it still gives its element.
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
    eprintln!("BFNIT {key}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v.join(" | ")
}

#[test]
fn a_spread_builtin_function_yields_no_own_element() {
    let pf = one("(Math.max as any)[0] = { wq: 2, zz: 1 };\n\
         const m: any = Math.random() < 0.5 ? Math.max : [{ wq: 1 }];\n\
         const arr = [...m];\n\
         console.log(arr[0].wq);\n");
    let r = recv(&pf, "wq");
    // Only the Array's element `{ wq: 1 }`, not the function's `{ wq: 2, zz: 1 }`.
    assert_eq!(r.matches("AnonShape").count(), 1, "{r}");
}
