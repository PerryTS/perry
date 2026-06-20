//! Regression test for #5268 — `Object.getOwnPropertyNames` on a native-module
//! object (`fs`, `path`, …) must enumerate the module's export surface, not the
//! internal `__module__` sentinel.
//!
//! graceful-fs's `clone.js` clones the `fs` module via
//! `getOwnPropertyNames(fs).forEach(k => defineProperty(copy, k,
//! getOwnPropertyDescriptor(fs, k)))`. Before the fix, `getOwnPropertyNames(fs)`
//! returned only `["__module__"]` (the generic field walk saw just that
//! sentinel), so the clone dropped every fs method — `gfs.readFileSync` resolved
//! to `undefined`, and `fs-extra`/`graceful-fs` were unusable. (The separate
//! `Object prototype may only be an Object or null: undefined` crash from the
//! same issue was fixed earlier by #5269.)
//!
//! Fix: route native-module objects through the same `vt_own_keys_array`
//! (`native_module_enumerable_keys`) path `Object.keys` already used.

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
fn native_module_get_own_property_names_lists_exports_and_clones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
import fs from "fs"
import path from "path"

const names = Object.getOwnPropertyNames(fs)
console.log("fs.gOPN.includes(readFileSync):", names.includes("readFileSync"))
console.log("fs.gOPN.includes(__module__):", names.includes("__module__"))
console.log("fs.gOPN.count>50:", names.length > 50)
console.log("path.gOPN.includes(join):", Object.getOwnPropertyNames(path).includes("join"))

// graceful-fs clone.js shape: copy fs via getOwnPropertyNames + descriptors
const copy: any = {}
for (const key of Object.getOwnPropertyNames(fs)) {
  const d = Object.getOwnPropertyDescriptor(fs as any, key)
  if (d) Object.defineProperty(copy, key, d)
}
console.log("clone.readFileSync:", typeof copy.readFileSync)
console.log("clone.writeFileSync:", typeof copy.writeFileSync)

// non-module objects are unchanged (getOwnPropertyNames keeps non-enumerable)
const o: any = { a: 1 }
Object.defineProperty(o, "hidden", { value: 2, enumerable: false })
console.log("plain.gOPN:", JSON.stringify(Object.getOwnPropertyNames(o)))
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary crashed\nstdout:\n{stdout}");
    for needle in [
        "fs.gOPN.includes(readFileSync): true",
        "fs.gOPN.includes(__module__): false",
        "fs.gOPN.count>50: true",
        "path.gOPN.includes(join): true",
        "clone.readFileSync: function",
        "clone.writeFileSync: function",
        r#"plain.gOPN: ["a","hidden"]"#,
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
