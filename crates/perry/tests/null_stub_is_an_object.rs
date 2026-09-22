//! #10917 / #10821 row 4 -- the unresolved-namespace stub is an ordinary
//! object.
//!
//! The stub was `NULL_OBJECT_BYTES`, a `.rodata` byte array laid out like an
//! `ObjectHeader` and handed to JS under `POINTER_TAG`, with no `GcHeader`.
//! `try_read_gc_header` read the 8 bytes the linker placed before it as the
//! header -- in the v0.5.1631 binary the tail of a string literal, `"nts]"`,
//! i.e. `obj_type == 110`, a kind that does not exist. So the stub, which is
//! meant to be an empty object, answered `JSON.stringify` with `""` and threw
//! on `String()`, and did so differently on a build whose literal layout
//! differed.
//!
//! The expected strings are what an empty object gives -- node's answer for
//! `JSON.stringify({})` and `String({})` -- because that is what the stub has
//! always claimed to be.

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

/// `handle.constructor` on a common-registry handle falls through to the stub
/// (`ic_miss.rs` / `get_field_by_name*.rs`), which makes it reachable from an
/// ordinary program. Pre-fix: `json ""` and `String()` threw
/// `TypeError: Cannot convert object to primitive value`.
#[test]
fn the_unresolved_namespace_stub_answers_as_an_empty_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import * as crypto from "node:crypto";
const c: any = crypto.createHash("sha256").constructor;
console.log("typeof", typeof c);
console.log("json", JSON.stringify(c));
console.log("nested", JSON.stringify({ a: c }));
console.log("keys", JSON.stringify(Object.keys(c)));
console.log("brand", Object.prototype.toString.call(c));
console.log("string", String(c));
console.log("stable", c === crypto.createHash("md5").constructor);
const m = new Map<any, number>([[c, 1]]);
console.log("map", m.get(crypto.createHash("sha1").constructor));
"#,
    );
    assert_eq!(
        stdout,
        "typeof object\n\
         json {}\n\
         nested {\"a\":{}}\n\
         keys []\n\
         brand [object Object]\n\
         string [object Object]\n\
         stable true\n\
         map 1\n"
    );
}
