//! RP2-false: the array arm of a guarded for-of reads only an Array.
//! `perry-hir` lowers `for (x of t)` to
//! `if (arrayRecordNeedsIterator(t) === false) { a = t; x = a[k] … } else
//! { GetIterator(t) … }` (`lower/for_of_guard.rs`). The runtime intrinsic
//! (`js_array_record_needs_iterator`, `array_record_enter`) answers false
//! only for an object whose header is an array (`GC_TYPE_ARRAY`, an
//! `arguments` object among them) or a lazy JSON array; it refuses every
//! other value: a primitive, a function, a Set or Map, an ordinary object.
//! So the array arm's `a[k]` never reads a Set's or a builtin function's
//! chain: what the program stored on `Set.prototype` or
//! `Function.prototype` under a key that may be an index never reaches the
//! loop variable (c09 audio/context.ts:766 `for (const c of contexts)`,
//! engine.worker.ts:10 `for (const uci of moves)`).
//! Controls: an Array literal, and an `arguments` object (an array header),
//! still reach the element read.
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
        .filter(|s| s.key == key)
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("RP2F {key}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v.join(" | ")
}

fn has_builtin(r: &str) -> bool {
    r.contains("builtin")
}

#[test]
fn a_set_iterated_by_for_of_reads_no_set_prototype_member() {
    let pf = one("const s = new Set<any>();\n\
         s.add({ wq: 1 });\n\
         (Set.prototype as any)[String(Math.random())] = Math.max;\n\
         for (const c of s) { console.log(c.wq); }\n");
    let r = recv(&pf, "wq");
    assert!(!has_builtin(&r), "{r}");
    assert!(r.contains("AnonShape"), "control: the Set's element: {r}");
}

#[test]
fn a_builtin_function_iterated_by_for_of_reads_no_function_member() {
    let pf = one(
        "function f(m: any) { for (const u of m) { console.log(u.wq); } }\n\
         f([{ wq: 1 }]);\n\
         f(Math.max);\n\
         (Function.prototype as any)[Math.floor(Math.random() * 4)] = Math.min;\n",
    );
    let r = recv(&pf, "wq");
    // The array arm never reads Math.max's elements (`undefined`); its
    // protocol arm throws (no `@@iterator`).
    assert!(!has_builtin(&r) && !r.contains("undefined"), "{r}");
    assert!(r.contains("AnonShape"), "control: the array's element: {r}");
}

#[test]
fn control_arguments_reach_the_array_arm() {
    let pf = one(
        "function g(..._r: any[]) { for (const u of arguments) { console.log(u.wq); } }\n\
         g({ wq: 1 });\n",
    );
    let r = recv(&pf, "wq");
    assert!(r.contains("AnonShape"), "{r}");
}
