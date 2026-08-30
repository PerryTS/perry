//! Regression coverage for #9098: user-installed `return` methods on Map/Set
//! iterators must win over the native iterator dispatcher and participate in
//! IteratorClose.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static")
            .output()
            .expect("build runtime archives");
        assert!(
            build.status.success(),
            "runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("create tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write fixture");
    ensure_runtime_archive();

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn collection_iterator_overrides_are_called_directly_and_by_iterator_close() {
    let stdout = compile_and_run(
        r#"
const directMap: any = new Map([[1, "a"]]).values();
directMap.return = () => ({ value: 42, done: true });
const directSet: any = new Set([1]).values();
directSet.return = () => ({ value: 84, done: true });
console.log("direct", directMap.return().value, directSet.return().value);

const inherited: any = new Map([[1, "a"]]).values();
const inheritedPrototype = Object.create(Object.getPrototypeOf(inherited));
let inheritedThis = false;
inheritedPrototype.return = function () {
  inheritedThis = this === inherited;
  return { value: 21, done: true };
};
Object.setPrototypeOf(inherited, inheritedPrototype);
console.log("inherited", inherited.return().value, inheritedThis);

const unpatched: any = new Set([1]).values();
let missingThrows = false;
try {
  unpatched.return();
} catch (error) {
  missingThrows = error instanceof TypeError;
}
console.log("missing", missingThrows);

const breakIter: any = new Set([1, 2, 3]).values();
let breakClosed = 0;
breakIter.return = function () { breakClosed++; return { done: true }; };
const seen: number[] = [];
for (const value of breakIter) {
  seen.push(value);
  if (value === 2) break;
}
console.log("break", seen.join(","), breakClosed);

const throwIter: any = new Map([[1, "a"], [2, "b"]]).values();
let throwClosed = 0;
throwIter.return = function () { throwClosed++; return { done: true }; };
let thrown = "";
try {
  for (const value of throwIter) {
    if (value === "a") throw new Error("boom");
  }
} catch (error: any) {
  thrown = error.message;
}
console.log("throw", thrown, throwClosed);

const returnIter: any = new Set([7, 8]).values();
let returnClosed = 0;
returnIter.return = function () { returnClosed++; return { done: true }; };
function leaveLoop(): number {
  for (const value of returnIter) return value;
  return -1;
}
console.log("return", leaveLoop(), returnClosed);

const destructureIter: any = new Map([[1, 5], [2, 6], [3, 7]]).values();
let destructureClosed = 0;
destructureIter.return = function () { destructureClosed++; return { done: true }; };
const [a, b] = destructureIter;
console.log("destructure", a, b, destructureClosed);
"#,
    );

    assert_eq!(
        stdout,
        "direct 42 84\n\
         inherited 21 true\n\
         missing true\n\
         break 1,2 1\n\
         throw boom 1\n\
         return 7 1\n\
         destructure 5 6 1\n"
    );
}
