//! Regression coverage for the Array.prototype.shift exotic-index cluster in
//! #5898. Shift must use live ordinary-property operations and observe indexed
//! getter side effects before setting the final array length.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn shift_observes_inherited_indices_holes_and_length_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    let runtime_dir = perry_bin()
        .parent()
        .expect("perry binary directory")
        .to_path_buf();
    std::fs::write(
        &entry,
        r#"
(Array.prototype as any)[1] = 1;
const inherited = [0];
inherited.length = 2;
console.log("inherited", inherited.shift(), inherited[0], inherited[1]);
delete (Array.prototype as any)[1];

const holey: any[] = [];
holey[0] = 0;
holey[3] = 3;
console.log("holey-first", holey.shift(), holey.length, holey[0], holey[2]);
holey.length = 1;
console.log("holey-second", holey.shift(), holey.length);

const frozen: any[] = new Array(1);
let frozenGetterCalls = 0;
Object.defineProperty(Array.prototype, "0", {
  configurable: true,
  get() {
    Object.freeze(frozen);
    frozenGetterCalls++;
  }
});
try {
  frozen.shift();
  console.log("frozen no throw");
} catch (error) {
  console.log("frozen", error instanceof TypeError, frozen.length, frozenGetterCalls);
}
delete (Array.prototype as any)[0];

const readonlyLength: any[] = new Array(1);
let readonlyGetterCalls = 0;
Object.defineProperty(Array.prototype, "0", {
  configurable: true,
  get() {
    Object.defineProperty(readonlyLength, "length", { writable: false });
    readonlyGetterCalls++;
  }
});
try {
  readonlyLength.shift();
  console.log("readonly no throw");
} catch (error) {
  console.log(
    "readonly",
    error instanceof TypeError,
    readonlyLength.length,
    readonlyGetterCalls
  );
}
delete (Array.prototype as any)[0];
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_LIB_DIR", &runtime_dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RS4GC", "0")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "inherited 0 1 1\n",
            "holey-first 0 3 undefined 3\n",
            "holey-second undefined 0\n",
            "frozen true 1 1\n",
            "readonly true 1 1\n",
        )
    );
}
