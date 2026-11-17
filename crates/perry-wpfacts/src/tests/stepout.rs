//! R-STEPOUT (GLOBS18, 2026-11-17): a native `iteratorStep` writes its own
//! value binding, `LocalSet(value, iteratorStepOutput())` (IteratorStep then
//! IteratorValue, ECMA-262 7.4.8/7.4.4): the value of that step's result,
//! or `undefined` at done. One step never reads what another loop or
//! destructuring of the same function yielded.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the call sites of method `key`.
fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn all(v: &[String], want: &[&str]) -> bool {
    v.iter().all(|x| want.contains(&x.as_str()))
}

/// Two nested for-of loops in ONE live function: the outer one yields
/// `[key, strings]` entries of a Map, the inner one the strings.
fn program(inner: &str) -> String {
    format!(
        "const m = new Map<string, string[]>([[\"a\", [\"x\", \"y\"]]]);\n\
         function f(): number {{\n\
           let n = 0;\n\
           for (const [k, list] of m) {{\n\
             for (const s of list) {{ n += {inner}; }}\n\
             n += k.length;\n\
           }}\n\
           return n;\n\
         }}\n\
         console.log(f());\n"
    )
}

/// RED: the inner loop variable holds only strings, never the `[k, list]`
/// entry arrays, the Map or the string array the outer steps yield.
#[test]
fn stepout_loops_do_not_share_values() {
    let v = calls(&program("s.charCodeAt(0)"), "charCodeAt");
    assert!(all(&v, &["NativeMember"]), "s holds only strings: {v:?}");
}

/// KEEP: a value the inner loop itself yields is still seen by its variable.
#[test]
fn stepout_own_value_is_seen() {
    let src = "const o: any = { go() { return 1; } };\n\
               function f(): number {\n\
                 let n = 0;\n\
                 for (const [k, list] of new Map<string, any[]>([[\"a\", [o, \"x\"]]])) {\n\
                   for (const s of list) { if (typeof s !== \"string\") n += s.go(); }\n\
                   n += k.length;\n\
                 }\n\
                 return n;\n\
               }\n\
               console.log(f());\n";
    let v = calls(src, "go");
    assert!(all(&v, &["T0Mono", "T0Set"]), "s.go() reaches o.go: {v:?}");
}
