//! Regression test for #5268: native-module constructor *class* exports must
//! expose a real `.prototype` object so the userland subclass idioms used by
//! graceful-fs / fs-extra / pino don't throw
//! `TypeError: Object prototype may only be an Object or null: undefined`.
//!
//! graceful-fs `graceful-fs.js`:
//! ```js
//! var fs$ReadStream = fs.ReadStream
//! if (fs$ReadStream) {
//!   ReadStream.prototype = Object.create(fs$ReadStream.prototype) // throws pre-fix
//! }
//! ```
//! pino `lib/proto.js`:
//! ```js
//! const { EventEmitter } = require('node:events')
//! Object.setPrototypeOf(prototype, EventEmitter.prototype) // throws pre-fix
//! ```
//!
//! Pre-fix, `fs.ReadStream`/`fs.WriteStream`/`events.EventEmitter` were truthy
//! callable closures (bound-native exports) whose `.prototype` resolved to
//! `undefined`: `ordinary_function_prototype_value_for_read`
//! (crates/perry-runtime/src/object/class_registry.rs) returned `None` for
//! every bound-native export except a hardcoded http/https whitelist. Reading
//! `Object.create(undefined)` / `Object.setPrototypeOf(x, undefined)` then hit
//! the spec TypeError. Fix: recognize constructor-cased bound-native exports
//! (leading uppercase, not flagged non-constructable) and let the
//! synthetic-class path materialize a stable `.prototype` object — while
//! keeping non-constructor exports (`fs.readFile`, …) at `prototype ===
//! undefined`, matching Node's built-in non-constructor functions.

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
        "compiled binary failed (pre-fix: 'TypeError: Object prototype may \
         only be an Object or null: undefined')\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn native_ctor_class_exports_expose_real_prototype() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import * as fs from "node:fs";
import { EventEmitter } from "node:events";

// Native constructor classes expose an object .prototype (not undefined).
const rsProto = (fs as any).ReadStream.prototype;
const wsProto = (fs as any).WriteStream.prototype;
const eeProto = (EventEmitter as any).prototype;
console.log("rs:", typeof rsProto === "object" && rsProto !== null);
console.log("ws:", typeof wsProto === "object" && wsProto !== null);
console.log("ee:", typeof eeProto === "object" && eeProto !== null);

// graceful-fs's ReadStream pattern: Object.create(Ctor.prototype).
const sub = Object.create((fs as any).ReadStream.prototype);
console.log("create:", typeof sub === "object" && sub !== null);
console.log("chain:", Object.getPrototypeOf(sub) === (fs as any).ReadStream.prototype);

// pino's proto.js pattern: setPrototypeOf(prototype, EventEmitter.prototype).
const prototype: any = { child() { return null; } };
Object.setPrototypeOf(prototype, (EventEmitter as any).prototype);
console.log("setproto:", true);

// Non-constructor native exports keep prototype === undefined (no spurious
// synthesis), matching Node's built-in non-constructor functions.
console.log("nonctor:", (fs as any).readFile.prototype === undefined);
"#,
    );
    assert_eq!(
        stdout,
        "rs: true\nws: true\nee: true\ncreate: true\nchain: true\nsetproto: true\nnonctor: true\n"
    );
}
