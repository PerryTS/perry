//! R-ITERDONE (SHNODE1, 2026-11-18): a plain-async step keeps one
//! iter-result slot (`IterResultSet(value, done)`, read back by
//! `IterResultGetValue`). Its value is read in two places: under
//! `if (IterResultGetDone)`, where the last write was a `return`
//! (done = true), and after it, where the last write was an await's
//! operand (done = false). Each read takes only the writes with its own
//! `done` flag: the function's result never holds what it awaited, and an
//! await never resumes with what the function returns.
use super::*;

/// The object receivers of the call sites of method `key`, in source
/// order (the first resume's `undefined` is not what these tests watch).
fn receivers(src: &str, key: &str) -> Vec<Vec<String>> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_transform::transform_async_to_generator(&mut hir);
    perry_transform::transform_generators(&mut hir);
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<Vec<String>> = s
        .iter()
        .map(|s| {
            let mut r: Vec<String> = s
                .receivers
                .iter()
                .filter(|r| !r.starts_with("primitive:"))
                .cloned()
                .collect();
            r.sort();
            r
        })
        .collect();
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

const CLASSES: &str = "class A { m(): number { return 2; } }\n\
                       class B { m(): number { return 1; } }\n";

/// RED: `f` awaits an A and returns a B. The awaited A is not f's result
/// (`b.m()` reaches B.m only) and the returned B is not the await's result
/// (`a.m()` reaches A.m only).
#[test]
fn iterdone_await_and_return_kept_apart() {
    let src = format!(
        "{CLASSES}\
         async function f(p: Promise<A>): Promise<B> {{\n\
           const a = await p;\n\
           console.log(a.m());\n\
           return new B();\n\
         }}\n\
         async function main(): Promise<void> {{\n\
           const b = await f(Promise.resolve(new A()));\n\
           console.log(b.m());\n\
         }}\n\
         main();\n"
    );
    let v = receivers(&src, "m");
    assert_eq!(v.len(), 2, "{v:?}");
    assert_eq!(v[0], ["t.ts:A"], "a holds the awaited A only: {v:?}");
    assert_eq!(v[1], ["t.ts:B"], "b holds the returned B only: {v:?}");
}

/// KEEP: every `return` of the function still reaches its result: the
/// caller's `r.m()` sees both A and B.
#[test]
fn iterdone_every_return_is_seen() {
    let src = format!(
        "{CLASSES}\
         async function f(k: number): Promise<A | B> {{\n\
           await null;\n\
           if (k > 1) return new A();\n\
           return new B();\n\
         }}\n\
         async function main(): Promise<void> {{\n\
           const r = await f(Date.now());\n\
           console.log(r.m());\n\
         }}\n\
         main();\n"
    );
    let v = receivers(&src, "m");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0], ["t.ts:A", "t.ts:B"], "r holds both returns: {v:?}");
}
