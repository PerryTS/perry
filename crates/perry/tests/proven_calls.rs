//! FVA: a call through a value is a call of the bodies the value is.
//!
//! End to end, whole program (`PERRY_FACTS_UNITS=0`) and per unit: a call
//! whose callee set the facts prove makes no runtime dispatch, and every
//! program prints node's output (pinned below).
//!
//! * a member call through a literal field holding one function per layout
//!   (`o.f(v)` over two literals): the slot's one load and one indirect
//!   call through the body's code word, no `js_closure_call`;
//! * virtual dispatch over two classes with a never-nullish receiver: one
//!   class-id load and a switch of direct method calls, no generic arm;
//! * the same call on a receiver that may be `undefined`: the class-id
//!   switch behind a pointer test, whose other arm reads the method (and
//!   throws) before the argument runs;
//! * calls through locals holding one or two closures.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `source` (whole program when `whole`) and run it; return the
/// saved LLVM IR of the entry module and the program's stdout.
fn compile_and_run(dir: &Path, source: &str, whole: bool) -> (String, String) {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join("main_bin");
    let ll_dir = dir.join(if whole { "ll_w" } else { "ll_u" });
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll_dir);
    if whole {
        cmd.env("PERRY_FACTS_UNITS", "0");
    }
    let out = cmd
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed (whole={whole})\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ll = std::fs::read_to_string(ll_dir.join("main_ts.ll")).expect("saved main_ts.ll");
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    (ll, String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// The body of the function `perry_fn_main_ts__<name>`.
fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let head = format!("@perry_fn_main_ts__{name}(");
    let at = ir
        .find(&head)
        .and_then(|i| ir[..i].rfind("define "))
        .unwrap_or_else(|| panic!("no function {name}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("perry_proven_calls_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const FIELDS: &str = r#"
function fa(x: number) { return x + 1; }
function fb(x: number) { return x * 2; }
function run(o: any, v: number) { return o.f(v); }
const p: any = { f: fa, tag: 1 };
const q: any = { f: fb, tag: 2 };
let s = 0;
for (let i = 0; i < 10; i++) s += run(i % 2 ? p : q, i);
console.log("fields", s, run(p, 5), run(q, 5));
"#;

#[test]
fn a_member_call_through_a_proven_field_runs_its_bodies() {
    let d = tmp("fields");
    for whole in [true, false] {
        let (ll, out) = compile_and_run(&d, FIELDS, whole);
        assert_eq!(out, "fields 70 6 10\n", "whole={whole}");
        if whole {
            let run = body(&ll, "run");
            assert!(!run.contains("js_closure_call"), "{run}");
            assert!(run.contains("call double %"), "an indirect call:\n{run}");
        }
    }
}

const TOWER: &str = r#"
class A { k = 1; m(x: number) { return x + this.k; } }
class B { k = 2; m(x: number) { return x * this.k; } }
function call(r: A | B, x: number) { return r.m(x); }
const a = new A(), b = new B();
let s = 0;
for (let i = 0; i < 10; i++) s += call(i % 2 ? a : b, i);
console.log("tower", s);
function callN(r: any) { let hit = 0; try { return r.m((hit = 1)); } catch (e) { return "threw:" + hit; } }
console.log("nullable", callN(a), callN(undefined), callN(b));
"#;

#[test]
fn virtual_dispatch_over_proven_classes_is_a_class_id_switch() {
    let d = tmp("tower");
    for whole in [true, false] {
        let (ll, out) = compile_and_run(&d, TOWER, whole);
        assert_eq!(out, "tower 70\nnullable 2 threw:0 2\n", "whole={whole}");
        if !whole {
            continue;
        }
        let call = body(&ll, "call");
        let switch = call
            .lines()
            .find(|l| l.contains("switch i32"))
            .unwrap_or_else(|| panic!("a switch:\n{call}"));
        // The last class is the default arm: no generic arm is reached.
        assert!(switch.contains("label %idispatch.case"), "{switch}");
        for gone in [
            "perry_probe",
            "js_method_call_slow",
            "js_native_call_method",
            "pconst.",
        ] {
            assert!(!call.contains(gone), "{gone} in\n{call}");
        }
        let call_n = body(&ll, "callN");
        assert!(call_n.contains("proven_recv.class_id"), "{call_n}");
        assert!(call_n.contains("idispatch.generic"), "{call_n}");
    }
}

const VALUES: &str = r#"
const inc = (x: number) => x + 1;
const dec = (x: number) => x - 1;
function pick(n: number) { return n > 3 ? inc : dec; }
let s = 0;
for (let i = 0; i < 6; i++) { const h = pick(i); s += h(i); }
console.log("values", s);
"#;

#[test]
fn a_call_through_a_local_holding_two_closures_is_one_indirect_call() {
    let d = tmp("values");
    for whole in [true, false] {
        let (ll, out) = compile_and_run(&d, VALUES, whole);
        assert_eq!(out, "values 13\n", "whole={whole}");
        if whole {
            assert!(!ll.contains("call double @js_closure_call"), "{ll}");
        }
    }
}

const STATICS: &str = r#"
class Body { m: number; constructor(m: number) { this.m = m; } static sun() { return new Body(1); } static jupiter() { return new Body(2); } }
class Pk { static of(n: number) { return n * 3; } static x = 5; }
function make() { return [Body.sun(), Body.jupiter()]; }
function viaValue(k: any) { return k.of(2) + k.x; }
let s = 0;
for (let i = 0; i < 3; i++) s += viaValue(Pk);
console.log("statics", make().map((b) => b.m).join(","), s, Pk.of(1));
class C { static k() { return 1; } }
function callC() { return C.k(); }
console.log("replaced", callC());
(C as any).k = () => 2;
console.log("replaced", callC());
"#;

#[test]
fn a_proven_static_is_called_without_its_guard() {
    // FVA 3.4: a static that holds its declaration for the program's life
    // (`Body.sun`, `Pk.of` through a value) is its body's direct call; one
    // replaced after its definition (`C.k`) keeps the guarded call.
    let d = tmp("statics");
    for whole in [true, false] {
        let (ll, out) = compile_and_run(&d, STATICS, whole);
        assert_eq!(
            out, "statics 1,2 33 3\nreplaced 1\nreplaced 2\n",
            "whole={whole}"
        );
        if !whole {
            continue;
        }
        let make = body(&ll, "make");
        assert!(!make.contains("js_class_static_call_guard("), "{make}");
        assert!(make.contains("@perry_static_main_ts__Body__"), "{make}");
        let call_c = body(&ll, "callC");
        assert!(call_c.contains("js_class_static_call_guard("), "{call_c}");
    }
}

const ACCESSORS: &str = r#"
class A { a = 1; }
const ns: any = { tag: 1 };
Object.defineProperty(ns, "k", { get: () => new A(), enumerable: true });
function rd(o: any) { return o.k.a + o.tag; }
let s = 0;
for (let i = 0; i < 5; i++) s += rd(ns);
console.log("accessors", s, Object.keys(ns).join(","));
const m: any = { tag: 2 };
Object.defineProperty(m, "tag", { get: () => 7 });
function rd2(o: any) { return o.tag; }
console.log("redefined", rd2(m), rd2({ tag: 3 }));
"#;

#[test]
fn a_getter_on_a_new_key_keeps_the_layout() {
    // FVA 4.3a: `defineProperty(ns, "k", { get })` adds an accessor key to
    // the literal's layout (not E5), and a getter over an existing data key
    // (`m.tag`) stays E5. Output pinned to node in both modes.
    let d = tmp("accessors");
    for whole in [true, false] {
        let (_, out) = compile_and_run(&d, ACCESSORS, whole);
        assert_eq!(out, "accessors 10 tag,k\nredefined 7 3\n", "whole={whole}");
    }
}
