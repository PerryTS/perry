//! INSP1 (2026-11-16): two natives the app's solves stopped at.
//! (A) The runtime formatter (`console.*`, perry-runtime
//! builtins/formatting.rs `format_jsvalue`) shows a realm object (the global
//! object, a namespace): its `[util.inspect.custom]` hook is an own entry
//! under the `Symbol.for("nodejs.util.inspect.custom")` key or the
//! `__perry_inspect_custom__` method of its class; no realm object has
//! either, so the hook is what the program stored there under a key that may
//! name it. (B) `fs.readSync` copies bytes into its buffer argument: no
//! program value is stored, no coercion runs (perry-runtime fs/fd_ops.rs).
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
    eprintln!("INSP1 {key}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

const SHOWN: &str = "class A { x = 1; get g() { return 2; } }\n\
class B extends A { y: any = globalThis; }\n\
class P extends B { z = \"s\"; }\n\
class C { m() { return 1; } }\n\
const q: any = new C();\n\
(globalThis as any)[String(Math.random())] = function () { return 3; };\n\
const b = new P();\n";

/// RED: a class instance with a getter and a three-class chain, one of
/// whose fields holds the global object, which the program wrote under a
/// String key: printing it calls no hook of the global object.
#[test]
fn in1_shown_global_object_no_hook() {
    let v = calls(&format!("{SHOWN}console.log(b);\nq.m();\n"), "m");
    assert!(mono(&v), "q.m() stays C::m: {v:?}");
}

/// KEEP: a hook the program stores on the global object under the inspect
/// symbol runs when the global object is shown (Node and perry call it).
#[test]
fn in1_global_hook_stored_runs() {
    let src = format!(
        "{SHOWN}(globalThis as any)[Symbol.for(\"nodejs.util.inspect.custom\")] = \
         function () {{ q.m = function () {{ return 2; }}; return \"G\"; }};\n\
         console.log(b);\nq.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "the stored hook may replace q.m: {v:?}");
}

/// KEEP: a getter with a side effect on the shown instance: the model may
/// run it (perry and Node run getters only under `{ getters }`); the
/// verdict stays sound.
#[test]
fn in1_getter_side_effect_sound() {
    let src = "class C { m() { return 1; } }\nconst q: any = new C();\n\
               class G { get g() { q.m = function () { return 2; }; return 1; } }\n\
               console.log(new G());\nif (Math.random() > 2) (new G()).g;\nq.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the getter may replace q.m: {v:?}");
}

/// KEEP: a class's `[util.inspect.custom]` method runs when an instance is
/// shown.
#[test]
fn in1_class_inspect_custom_runs() {
    let src = "class C { m() { return 1; } }\nconst q: any = new C();\n\
               class H { [Symbol.for(\"nodejs.util.inspect.custom\")]() { \
               q.m = function () { return 2; }; return \"H\"; } }\n\
               console.log(new H());\nq.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the hook may replace q.m: {v:?}");
}

/// KEEP: the printed object then handed to code read under an unknown key
/// of the global object: it escapes there, not at the print.
#[test]
fn in1_shown_then_unknown_code() {
    let src = format!(
        "{SHOWN}console.log(b);\nconst u: any = (globalThis as any)[process.argv[2]];\n\
         if (typeof u === \"function\") u(q);\nq.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "unknown code may replace q.m: {v:?}");
}

const FD: &str = "import * as fs from \"fs\";\n\
class C { m() { return 1; } }\n\
const c = new C();\n\
const fd = fs.openSync(process.argv[1], \"r\");\n";

/// RED: `fs.readSync` into a fresh Uint8Array keeps nothing and runs no
/// program code.
#[test]
fn in1_read_sync_fresh_u8() {
    let src = format!(
        "{FD}const buf = new Uint8Array(16);\nconst n = fs.readSync(fd, buf, 0, 16, 0);\n\
         console.log(n + buf[0]);\nc.m();\n"
    );
    let v = calls(&src, "m");
    assert!(mono(&v), "c.m() stays C::m: {v:?}");
}

/// KEEP: an object offset is the options form: its fields are read, its
/// `valueOf` is never called (no ToNumber).
#[test]
fn in1_read_sync_offset_value_of() {
    let src = format!(
        "{FD}const q: any = new C();\nconst buf = new Uint8Array(16);\n\
         const off: any = {{ valueOf() {{ q.m = function () {{ return 2; }}; return 0; }} }};\n\
         fs.readSync(fd, buf, off, 16, 0);\nq.m();\nc.m();\n"
    );
    let v = calls(&src, "m");
    assert!(mono(&v), "no valueOf runs: {v:?}");
}

/// KEEP: an options getter runs ([[Get]] of `offset`).
#[test]
fn in1_read_sync_options_getter() {
    let src = format!(
        "{FD}const q: any = new C();\nconst buf = new Uint8Array(16);\n\
         const o: any = {{ get offset() {{ q.m = function () {{ return 2; }}; return 0; }} }};\n\
         fs.readSync(fd, buf, o);\nq.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "the getter may replace q.m: {v:?}");
}

/// KEEP: a buffer unknown code also reaches: it escapes there.
#[test]
fn in1_read_sync_buffer_unknown_code() {
    let src = format!(
        "{FD}const o: any = {{ m() {{ return 1; }} }};\n\
         const u: any = (globalThis as any)[process.argv[2]];\n\
         if (typeof u === \"function\") u(o);\nfs.readSync(fd, o, 0, 1, 0);\no.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "unknown code may replace o.m: {v:?}");
}
