//! Regression coverage for #9249: indexed accessors installed on
//! `Array.prototype` through the descriptor APIs must invalidate array-store
//! fast paths just like a plain indexed assignment to the prototype does.
//!
//! DEC 250 (Q2): a program adds no index key to `Array.prototype` or
//! `Object.prototype`. Each descriptor API must still reach the same index
//! note a plain indexed assignment does, which on those two is now the lock's
//! TypeError (a funnel that skipped the note would install silently); the
//! interception itself is shown on the array's own prototype, installed after
//! the array is linked to it.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str, expected: &str, label: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join(format!("{label}.ts"));
    let output = dir.path().join(format!("{label}_bin"));
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed for {label}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed for {label}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected,
        "{label} output must match Node\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn define_property_array_prototype_index_setter_intercepts_numeric_store() {
    compile_and_run(
        r#"
let hits = 0;
let refused = "none";
try {
  Object.defineProperty(Array.prototype, 7, { get() { return "AP"; }, configurable: true });
} catch (e) { refused = (e as Error).constructor.name; }
const proto: any = Object.create(Array.prototype);
const nums = [1, 2, 3];
Object.setPrototypeOf(nums, proto);
nums[5] = 0;
Object.defineProperty(proto, 7, {
  set(value) { hits++; },
  get() { return "P"; },
  configurable: true
});
nums[7] = 42;
console.log(refused, hits, nums.length, nums[7], ([] as any)[7]);
"#,
        "TypeError 1 6 P undefined\n",
        "define_property",
    );
}

#[test]
fn default_array_prototype_setter_intercepts_in_bounds_holes() {
    compile_and_run(
        include_str!(
            "../../../tests/parity/language/array/test_gap_9787_array_hole_inherited_setter.ts"
        ),
        "true:31 false array-proto-three 1\n\
         true:31,true:37 false array-proto-three 5\n\
         own 2 true 41 4\n\
         deleted true:31,true:37,true:43 false array-proto-three 4\n\
         removed true 47 5\n",
        "array_prototype_holes",
    );
}

#[test]
fn default_object_prototype_descriptors_intercept_in_bounds_holes() {
    compile_and_run(
        include_str!(
            "../../../tests/parity/language/object/test_gap_9787_object_prototype_hole_setter.ts"
        ),
        "1 true 53 false object-proto-eight 10\n\
         TypeError false getter-only 10\n\
         TypeError false locked 10\n",
        "object_prototype_holes",
    );
}

#[test]
fn define_properties_array_prototype_index_setter_intercepts_boolean_store() {
    compile_and_run(
        r#"
let hits = 0;
const descriptors: any = {};
descriptors[9] = {
  set(value) { hits++; },
  get() { return "P"; },
  configurable: true
};
let refused = "none";
try { Object.defineProperties(Array.prototype, descriptors); } catch (e) { refused = (e as Error).constructor.name; }
const proto: any = Object.create(Array.prototype);
const flags = [true, false];
Object.setPrototypeOf(flags, proto);
flags[1] = true;
Object.defineProperties(proto, descriptors);
flags[9] = false;
console.log(refused, hits, flags.length, flags[9], ([] as any)[9]);
"#,
        "TypeError 1 2 P undefined\n",
        "define_properties",
    );
}

#[test]
fn define_property_object_prototype_index_setter_intercepts_array_store() {
    compile_and_run(
        r#"
let hits = 0;
let refused = "none";
try {
  Object.defineProperty(Object.prototype, 5, { get() { return "OP"; }, configurable: true });
} catch (e) { refused = (e as Error).constructor.name; }
// The accessor sits at the END of the array's chain (a plain object above
// the array's own prototype), as Object.prototype did.
const base: any = {};
const proto: any = Object.create(base);
const values = [1];
Object.setPrototypeOf(values, proto);
values[0] = 2;
Object.defineProperty(base, 5, {
  set(value) { hits++; },
  get() { return "P"; },
  configurable: true
});
values[5] = 99;
console.log(refused, hits, values.length, values[5], ({} as any)[5]);
"#,
        "TypeError 1 1 P undefined\n",
        "object_prototype",
    );
}

#[test]
fn reflect_define_property_non_writable_prototype_index_blocks_array_store() {
    compile_and_run(
        r#""use strict";
// #9426 made a rejected array-element write throw ONLY in strict mode, which
// is what node does. This file is a bare .ts — the oracle
// (`node --experimental-strip-types`) runs it as a SCRIPT, i.e. sloppy, where
// the rejected store silently no-ops in node too:
//   $ node --experimental-strip-types <this program without "use strict">
//   no error 1 P
// The point of this test is that a non-writable inherited index BLOCKS the
// store, so it opts into strict mode rather than asserting the sloppy no-op.
let refused = "none";
try {
  Reflect.defineProperty(Array.prototype, 11, { value: "AP", writable: false, configurable: true });
} catch (e) { refused = (e as Error).constructor.name; }
const proto: any = Object.create(Array.prototype);
const values = [1];
Object.setPrototypeOf(values, proto);
values[0] = 2;
Reflect.defineProperty(proto, 11, {
  value: "P",
  writable: false,
  configurable: true
});
let result = "no error";
try {
  values[11] = 99;
} catch (error) {
  result = error.name;
}
console.log(refused, result, values.length, values[11], ([] as any)[11]);
"#,
        "TypeError TypeError 1 P undefined\n",
        "reflect_define_property",
    );
}
