//! R-INSPKEY (GLOBS17, 2026-11-17): a computed store under an object key
//! stores under ToPropertyKey of that key, for every receiver (ECMA-262
//! 7.1.19: ToPrimitive(key, string), 7.1.1, then ToString unless the
//! result is a Symbol). A String result takes the any-String shape, which
//! never writes a symbol-keyed property such as the `[util.inspect.custom]`
//! hook the console formatter calls. A Symbol result, ⊤ or a key from an
//! unknown source keeps the any-key store, which may write it.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the `.m()` call sites.
fn calls(src: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    assert!(!v.is_empty(), "no call site of m");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// `key` names the key, then a hook that would replace `q.m` is stored on
/// `o` under it, `o` is shown, `q.m()` runs.
fn program(key: &str) -> String {
    format!(
        "class C {{ m() {{ return 1; }} }}\n\
         const q: any = new C();\n\
         const o: any = {{ a: 1 }};\n\
         {key}\n\
         for (const k of ks) o[k] = function () {{ q.m = function () {{ return 2; }}; return \"H\"; }};\n\
         console.log(o);\n\
         q.m();\n"
    )
}

/// RED: arrays as keys (Array.prototype.toString: a String) never write
/// the hook: showing `o` calls nothing the store wrote.
#[test]
fn inspkey_array_key_writes_no_hook() {
    let v = calls(&program("const ks: any[] = [[\"a\"], [\"b\", \"c\"]];"));
    assert!(mono(&v), "q.m() stays C::m: {v:?}");
}

/// KEEP 1: an object key whose `toString` or `@@toPrimitive` returns the
/// inspect symbol writes the hook; it runs when `o` is shown.
#[test]
fn inspkey_symbol_result_writes_hook() {
    for key in [
        "class S { toString(): any { return Symbol.for(\"nodejs.util.inspect.custom\"); } }\n\
         const ks: any[] = [new S()];",
        "const ks: any[] = [{ [Symbol.toPrimitive]() { return Symbol.for(\"nodejs.util.inspect.custom\"); } }];",
    ] {
        let v = calls(&program(key));
        assert!(!mono(&v), "{key}: the stored hook may replace q.m: {v:?}");
    }
}

/// KEEP 2: a key from an unknown source (an unmodeled native's result), or
/// an object key whose `toString` returns one (⊤ in the ToPrimitive result,
/// which carries no primitive bits), keeps the any-key store.
#[test]
fn inspkey_unknown_key_keeps_any() {
    for key in [
        "const ks: any[] = [(ReadableStream as any).from([1])];",
        "const u: any = (ReadableStream as any).from([1]);\n\
         class S { toString(): any { return u; } }\n\
         const ks: any[] = [new S()];",
    ] {
        let v = calls(&program(key));
        assert!(!mono(&v), "{key}: the key may be the hook's symbol: {v:?}");
    }
}

/// KEEP 3: a key whose `toString` returns a value the solver holds as ⊤
/// alone (no primitive bits: `add_top_inner`), here a generator object,
/// may be the inspect symbol: the hook may run, on the global object (the
/// GLOBK path) and on any other object.
fn top_result(recv: &str, shown: &str) -> String {
    format!(
        "class C {{ m() {{ return 1; }} }}\n\
         const q: any = new C();\n\
         const o: any = {{ a: 1 }};\n\
         const gen = function* (): any {{ yield 1; }};\n\
         class S {{ toString(): any {{ return gen(); }} }}\n\
         {recv}[new S() as any] = function () {{ q.m = function () {{ return 2; }}; return \"G\"; }};\n\
         console.log({shown});\n\
         q.m();\n"
    )
}

#[test]
fn inspkey_top_result_keeps_any_global() {
    let v = calls(&top_result("(globalThis as any)", "globalThis"));
    assert!(!mono(&v), "the key may be the hook's symbol: {v:?}");
}

#[test]
fn inspkey_top_result_keeps_any_object() {
    let v = calls(&top_result("o", "o"));
    assert!(!mono(&v), "the key may be the hook's symbol: {v:?}");
}
