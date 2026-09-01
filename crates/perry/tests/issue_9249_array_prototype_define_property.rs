//! Regression coverage for #9249: indexed accessors installed on
//! `Array.prototype` through the descriptor APIs must invalidate array-store
//! fast paths just like a plain indexed assignment to the prototype does.

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
Object.defineProperty(Array.prototype, 7, {
  set(value) { hits++; },
  get() { return "P"; },
  configurable: true
});
const nums = [1, 2, 3];
nums[7] = 42;
console.log(hits, nums.length, nums[7]);
"#,
        "1 3 P\n",
        "define_property",
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
Object.defineProperties(Array.prototype, descriptors);
const flags = [true, false];
flags[9] = false;
console.log(hits, flags.length, flags[9]);
"#,
        "1 2 P\n",
        "define_properties",
    );
}

#[test]
fn define_property_object_prototype_index_setter_intercepts_array_store() {
    compile_and_run(
        r#"
let hits = 0;
Object.defineProperty(Object.prototype, 5, {
  set(value) { hits++; },
  get() { return "P"; },
  configurable: true
});
const values = [1];
values[5] = 99;
console.log(hits, values.length, values[5]);
"#,
        "1 1 P\n",
        "object_prototype",
    );
}

#[test]
fn reflect_define_property_non_writable_prototype_index_blocks_array_store() {
    compile_and_run(
        r#"
Reflect.defineProperty(Array.prototype, 11, {
  value: "P",
  writable: false,
  configurable: true
});
const values = [1];
let result = "no error";
try {
  values[11] = 99;
} catch (error) {
  result = error.name;
}
console.log(result, values.length, values[11]);
"#,
        "TypeError 1 P\n",
        "reflect_define_property",
    );
}
