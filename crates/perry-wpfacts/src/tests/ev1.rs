//! EVT2 (2026-11-16): the object-URL natives, declared at their bodies.
//! `URL.createObjectURL(blob)` (perry-stdlib fetch_blob.rs) returns a String,
//! reads only the Blob's bytes and type, and throws a TypeError for a
//! non-Blob; `node:buffer.resolveObjectURL(url)` runs ToString on its
//! argument and returns `undefined` or a NEW Blob (node lib/internal/blob.js).
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("EVT2 {key}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

const HEAD: &str = "import { resolveObjectURL } from \"node:buffer\";\n\
class Box { v = 2; get() { return this.v; } }\n\
const box = new Box();\n\
const made = new Blob([String(Math.random())], { type: \"text/plain\" });\n";

/// RED: neither the URL nor the resolved Blob reaches unknown code, so a
/// program method called after them stays monomorphic.
#[test]
fn ev1_object_url_round_trip_keeps_box() {
    let src = format!(
        "{HEAD}const url = URL.createObjectURL(made);\n\
         const got = resolveObjectURL(process.argv[2] === \"m\" ? \"blob:nodedata:x\" : url);\n\
         if (got !== undefined) console.log(got.type);\n\
         box.get();\n"
    );
    let v = calls(&src, "get");
    assert!(mono(&v), "box.get() stays Box::get: {v:?}");
}

/// KEEP: a program `toString` on the URL argument runs (ToString), so what
/// it does is seen: it replaces `box.get`.
#[test]
fn ev1_url_argument_tostring_runs() {
    let src = format!(
        "{HEAD}const url = URL.createObjectURL(made);\n\
         const key = {{ toString() {{ (box as any).get = function () {{ return 5; }}; return url; }} }};\n\
         resolveObjectURL(key as any);\n\
         box.get();\n"
    );
    let v = calls(&src, "get");
    assert!(!mono(&v), "the toString replaced box.get: {v:?}");
}

/// KEEP: a field stored on the original Blob is not on the resolved one (a
/// new Blob): reading it there is no read of the stored function.
#[test]
fn ev1_resolved_blob_is_new() {
    let src = format!(
        "{HEAD}(made as any).f = function () {{ (box as any).get = function () {{ return 7; }}; }};\n\
         const got: any = resolveObjectURL(URL.createObjectURL(made));\n\
         if (got !== undefined && typeof got.f === \"function\") got.f();\n\
         box.get();\n"
    );
    let v = calls(&src, "get");
    assert!(mono(&v), "the resolved Blob holds no f: {v:?}");
}

/// KEEP: the original Blob still holds its field (no store is lost).
#[test]
fn ev1_original_blob_keeps_field() {
    let src = format!(
        "{HEAD}(made as any).f = function () {{ (box as any).get = function () {{ return 7; }}; }};\n\
         URL.createObjectURL(made);\n\
         (made as any).f();\n\
         box.get();\n"
    );
    let v = calls(&src, "get");
    assert!(!mono(&v), "made.f replaced box.get: {v:?}");
}

/// KEEP: a non-Blob argument (the TypeError arm) is not called or escaped:
/// its method is not run by createObjectURL.
#[test]
fn ev1_non_blob_not_called() {
    let src = format!(
        "{HEAD}const o = {{ size: 1, arrayBuffer() {{ (box as any).get = function () {{ return 9; }}; }} }};\n\
         try {{ URL.createObjectURL((process.argv[2] === \"b\" ? made : o) as any); }} catch (e) {{}}\n\
         box.get();\n"
    );
    let v = calls(&src, "get");
    assert!(
        mono(&v),
        "createObjectURL calls nothing of its argument: {v:?}"
    );
}
