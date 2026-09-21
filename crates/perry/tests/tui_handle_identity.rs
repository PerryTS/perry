//! #10821 row 3 -- object identity and surface for `perry/tui` handles.
//!
//! Every value the module handed TypeScript was a small registry integer under
//! `POINTER_TAG`, and SIX id spaces shared that encoding: three registries
//! (widget tree, state slots, hook slots) plus three constants
//! (`useApp()` = 1, `useStdout()` = 2, `useFocusManager()` = 3). The widget
//! tree and `useRef` both count from 1, so `useApp() === Text("hi")` was
//! `true` and a `Set` of two widgets and the App handle had SIZE 2. The first
//! `state(0)` of a program was slot 0, i.e. `POINTER_TAG | 0` -- a null
//! pointer wearing the pointer tag.
//!
//! These tests are the identity contract every migrated family must satisfy,
//! written against BEHAVIOUR rather than representation: two distinct
//! resources are `!==`, the same resource reached twice is `===`, both hold as
//! `Map` / `Set` keys, and all of it survives a collection -- a widget is a
//! movable heap object now, so a probe that only checked values would not
//! cover the axis that changed.
//!
//! `perry/tui` has no node equivalent, so the parity bar here is the ORDINARY
//! OBJECT surface node gives every one of its own native classes: `typeof`
//! `"object"`, `Object.keys` `[]`, `JSON.stringify` `{}` (it was `null`).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The collision, and the object surface that removes it. Measured on the
/// pre-change binary (v0.5.1631), every one of these lines read the other way:
/// `app-eq-widget true`, `set-size 2`, and `{}` was `null`.
#[test]
fn tui_handles_of_different_kinds_are_different_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Text, state, useApp, useStdout, useFocusManager } from "perry/tui";
const a: any = Text("alpha");
const b: any = Text("beta");
const app: any = useApp();
const so: any = useStdout();
const fm: any = useFocusManager();
const s: any = state(0);
console.log("typeof", typeof a, typeof app, typeof s);
console.log("collide", app === a, app === so, so === fm, s === a);
console.log("stable", useApp() === app, useStdout() === so, a === a);
console.log("distinct", a === b);
const set = new Set<any>([a, b, app, so, fm, s]);
console.log("setsize", set.size);
const m = new Map<any, string>([[a, "one"], [b, "two"]]);
console.log("map", m.size, m.get(a), m.get(b));
console.log("surface", JSON.stringify(a), JSON.stringify(s), JSON.stringify(app));
console.log("keys", JSON.stringify(Object.keys(a)), JSON.stringify(Object.getOwnPropertyNames(a)));
"#,
    );
    assert_eq!(
        stdout,
        "typeof object object object\n\
         collide false false false false\n\
         stable true true true\n\
         distinct false\n\
         setsize 6\n\
         map 2 one two\n\
         surface {} {} {}\n\
         keys [] []\n"
    );
}

/// Identity and state must survive a collection: a handle is a movable
/// `GC_TYPE_OBJECT` now, held from a `Map` key, from a hook slot and from a
/// realm singleton slot. A probe that only checked `s.get()` would pass a
/// change that left the singleton slot or the `Map` key unrewritten.
#[test]
fn tui_handles_survive_a_collection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Text, state, useApp } from "perry/tui";
const a: any = Text("alpha");
const b: any = Text("beta");
const app: any = useApp();
const s = state(0);
s.set(7);
const m = new Map<any, string>([[a, "one"], [b, "two"]]);
let sink: any[] = [];
for (let i = 0; i < 200000; i++) { sink.push({ i: i, j: i + 1 }); }
sink = [];
console.log("state", s.get());
console.log("map", m.get(a), m.get(b), m.size);
console.log("identity", a === a, a === b, useApp() === app);
console.log("surface", typeof a, JSON.stringify(a));
"#,
    );
    assert_eq!(
        stdout,
        "state 7\n\
         map one two 2\n\
         identity true false true\n\
         surface object {}\n"
    );
}

/// The whole method surface used to exist ONLY as `class_filter` lowerings, so
/// a handle reached through an untyped value answered `undefined` for every
/// one of them -- measured on v0.5.1631, all four of these read `undefined`
/// and `typeof sAny.get === "function"` was `false`. They are prototype
/// methods now, so the ordinary read path finds them.
#[test]
fn tui_handle_methods_resolve_through_the_prototype() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { state, useApp, useStdout, useFocusManager } from "perry/tui";
const s: any = state(1);
const app: any = useApp();
const so: any = useStdout();
const fm: any = useFocusManager();
console.log("types", typeof s.get, typeof s.set, typeof app.exit, typeof so.columns, typeof fm.focusNext);
// A dynamic read of a method value, then a call through it (the #8133 shape).
const g: any = s.get;
console.log("read-then-call", typeof g === "function");
s.set(42);
console.log("dynamic-set-then-get", s.get());
console.log("columns-positive", so.columns() > 0, so.rows() > 0);
// A foreign receiver is answered leniently, never read as an id of this kind:
// perry/tui is not WebIDL and node has no equivalent to copy a policy from.
console.log("foreign", s.get.call({}), app.exit.call({}));
"#,
    );
    assert_eq!(
        stdout,
        "types function function function function function\n\
         read-then-call true\n\
         dynamic-set-then-get 42\n\
         columns-positive true true\n\
         foreign undefined undefined\n"
    );
}
