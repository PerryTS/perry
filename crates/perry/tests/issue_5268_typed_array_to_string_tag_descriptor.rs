//! Regression test for #5268 (pino follow-up wall): reading
//! `Object.getOwnPropertyDescriptor(%TypedArray%.prototype, Symbol.toStringTag)`
//! must return an accessor descriptor whose `.get` is a callable getter — not
//! `undefined`.
//!
//! After PR #5269 removed the `Object prototype may only be an Object or null:
//! undefined` throw for graceful-fs / fs-extra / pino, pino's next wall was a
//! `TypeError: Cannot read properties of undefined (reading 'get')` raised by
//! its transitive dependency `safe-stable-stringify`, which does:
//!
//! ```js
//! const typedArrayPrototypeGetSymbolToStringTag =
//!   Object.getOwnPropertyDescriptor(
//!     Object.getPrototypeOf(Object.getPrototypeOf(new Int8Array())),
//!     Symbol.toStringTag
//!   ).get
//! ```
//!
//! Perry returned `undefined` for that descriptor (no `@@toStringTag` accessor
//! existed on the shared `%TypedArray%.prototype`), so `.get` threw. The fix
//! installs the spec accessor (ES2024 23.2.3.38): a non-enumerable, configurable
//! getter that returns the receiver's `[[TypedArrayName]]` ("Int8Array", …) for
//! a typed-array receiver and `undefined` for any other receiver (no throw).

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
        "compiled binary failed (pre-fix: 'TypeError: Cannot read properties of \
         undefined (reading 'get')')\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn typed_array_proto_to_string_tag_accessor_descriptor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// safe-stable-stringify's exact shape: grab the %TypedArray%.prototype
// @@toStringTag getter off the descriptor and brand-check with it.
const taProto = Object.getPrototypeOf(Object.getPrototypeOf(new Int8Array()));
const desc = Object.getOwnPropertyDescriptor(taProto, Symbol.toStringTag);

console.log("desc_is_object:", typeof desc === "object" && desc !== null);
console.log("get_is_function:", typeof (desc as any).get === "function");
console.log("set_is_undefined:", (desc as any).set === undefined);
console.log("enumerable:", (desc as any).enumerable);
console.log("configurable:", (desc as any).configurable);

const getter = (desc as any).get as Function;
// Returns the [[TypedArrayName]] for a typed-array receiver.
console.log("tag_int8:", getter.call(new Int8Array()));
console.log("tag_f64:", getter.call(new Float64Array()));
// Returns undefined (does NOT throw) for a non-typed-array receiver.
console.log("tag_plain:", getter.call({}));

// Object.prototype.toString still tags typed arrays correctly.
console.log("tostring:", Object.prototype.toString.call(new Uint8Array()));
"#,
    );
    assert_eq!(
        stdout,
        "desc_is_object: true\n\
         get_is_function: true\n\
         set_is_undefined: true\n\
         enumerable: false\n\
         configurable: true\n\
         tag_int8: Int8Array\n\
         tag_f64: Float64Array\n\
         tag_plain: undefined\n\
         tostring: [object Uint8Array]\n"
    );
}
