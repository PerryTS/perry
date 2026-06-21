//! `class X extends <runtime value holding events.EventEmitter>` — an import
//! alias (`import { EventEmitter as E } from "events"`) or a local
//! `const E = EventEmitter` — must inherit the EventEmitter instance methods
//! (`setMaxListeners`/`on`/`emit`/`getMaxListeners`/…), exactly like the direct
//! `class X extends EventEmitter` form.
//!
//! The direct form lowers through codegen's compile-time extends-NAME machinery
//! (which emits `js_event_emitter_subclass_init` in `super()`). The aliased /
//! indirect form has a runtime-VALUE parent, so codegen lowers `super()` through
//! the dynamic-parent path — `js_native_call_value(parentValue, …)` with
//! IMPLICIT_THIS bound to the fresh instance — and
//! `js_register_class_parent_dynamic` early-returns for bound-native parents.
//! Before the fix, `super()` installed nothing and `this.setMaxListeners(0)` in
//! the subclass constructor threw `TypeError: value is not a function`.
//!
//! Fix: `js_native_call_value` detects a bound-native `events.EventEmitter`
//! invoked with IMPLICIT_THIS set (the dynamic-`super()` shape) and installs the
//! EventEmitter methods onto the receiver instance, mirroring the direct form.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn aliased_import_extends_eventemitter_inherits_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// Import alias — the common minified shape (`import { EventEmitter as nL_ }`).
import { EventEmitter as EE } from "events"

// Named class expression assigned to a var, with an `emit` override that calls
// `super.emit` — mirrors ink's internal event emitter, the real-world trigger.
let M6: any
M6 = class M6 extends EE {
  constructor() {
    super()
    this.setMaxListeners(0) // must NOT throw "value is not a function"
  }
  emit(event: any, ...args: any[]) {
    if (event === "error") return super.emit(event, ...args)
    return super.emit(event, ...args)
  }
}

const m: any = new M6()
console.log("ctor-ok")
console.log("setMaxListeners:", typeof m.setMaxListeners)
console.log("getMaxListeners:", typeof m.getMaxListeners, m.getMaxListeners())
let payload = ""
m.on("evt", (p: string) => { payload = p })
m.emit("evt", "PAYLOAD")
console.log("dispatched:", payload)
console.log("DONE")
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "ctor-ok",
        "setMaxListeners: function",
        "getMaxListeners: function 0",
        "dispatched: PAYLOAD",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}

#[test]
fn local_const_alias_extends_eventemitter_inherits_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// A local indirection (not an import alias) must behave identically: the parent
// is still a runtime VALUE holding the bound-native EventEmitter export.
import { EventEmitter } from "events"
const Base = EventEmitter
class Widget extends Base {
  constructor() { super(); this.setMaxListeners(11) }
}
const w: any = new Widget()
console.log("ctor-ok")
console.log("setMaxListeners:", typeof w.setMaxListeners)
console.log("maxListeners:", w.getMaxListeners())
let hit = false
w.on("ping", () => { hit = true })
w.emit("ping")
console.log("dispatched:", hit)
console.log("DONE")
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "ctor-ok",
        "setMaxListeners: function",
        "maxListeners: 11",
        "dispatched: true",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
