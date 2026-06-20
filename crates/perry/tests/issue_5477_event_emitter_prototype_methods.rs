//! Regression test for #5477 — `EventEmitter.prototype` carries the emitter
//! methods, so the `Object.setPrototypeOf(x, EventEmitter.prototype)` mixin
//! pattern (pino's logger prototype) gives `x` a working `emit`/`on`.
//!
//! Before the fix, `EventEmitter.prototype.emit` was `undefined` (the #5269
//! synthetic prototype object existed but had no methods), even though
//! instances dispatched `emit` natively. pino's `lib/proto.js` does
//! `Object.setPrototypeOf(prototype, EventEmitter.prototype)` then
//! `lib/levels.js` calls `this.emit('level-change', …)` → "emit is not a
//! function".
//!
//! Fix: when the bound `events.EventEmitter` export's synthetic `.prototype`
//! is materialized, install the EventEmitter methods on it via
//! `install_event_emitter_prototype_methods` (the same dynamic-`this` closures
//! `Stream.prototype` already uses), so an object inheriting the prototype
//! dispatches against itself.

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
fn event_emitter_prototype_mixin_has_working_emit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { EventEmitter } from "node:events"

// EventEmitter.prototype now carries the methods.
console.log("proto.emit:", typeof (EventEmitter.prototype as any).emit)
console.log("proto.on:", typeof (EventEmitter.prototype as any).on)

// pino's mixin shape: a plain prototype whose __proto__ is EventEmitter.prototype.
const proto: any = { tag: "logger" }
Object.setPrototypeOf(proto, EventEmitter.prototype)
const inst: any = Object.create(proto)
console.log("mixin.emit:", typeof inst.emit)
let payload = ""
inst.on("level-change", (p: string) => { payload = p })
inst.emit("level-change", "PAYLOAD")
console.log("mixin emit dispatched:", payload)

// Direct instances and subclasses must still work (regression guard).
const e: any = new EventEmitter()
let direct = ""
e.on("x", (v: string) => { direct = v }); e.emit("x", "direct")
console.log("direct:", direct)

class Sub extends EventEmitter { fire() { this.emit("s", "sub") } }
const s: any = new Sub()
let sub = ""
s.on("s", (v: string) => { sub = v }); s.fire()
console.log("subclass:", sub)
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "proto.emit: function",
        "proto.on: function",
        "mixin.emit: function",
        "mixin emit dispatched: PAYLOAD",
        "direct: direct",
        "subclass: sub",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
