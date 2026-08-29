//! Regression coverage for #9086: ordinary collection/string iterators do not
//! expose the generator-only `return` and `throw` methods.

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
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build runtime archives");
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
fn ordinary_iterators_omit_generator_control_methods() {
    let stdout = compile_and_run(
        r#"
function check(label: string, iterator: any) {
  console.log(
    label,
    typeof iterator.return,
    typeof iterator.throw,
    "return" in iterator,
    "throw" in iterator,
    typeof iterator[Symbol.iterator],
    iterator[Symbol.iterator]() === iterator,
  );
}

check("map", new Map([[1, "a"]]).entries());
check("set", new Set([1, 2]).values());
check("string", "ab"[Symbol.iterator]());

const patched: any = new Set([1, 2]).values();
const ownReturn = function () {};
const ownThrow = function () {};
patched.return = ownReturn;
patched.throw = ownThrow;
console.log(
  "own methods",
  patched.return === ownReturn,
  patched.throw === ownThrow,
  "return" in patched,
  "throw" in patched,
);

const inherited: any = new Map([[1, "a"]]).entries();
const inheritedReturn = function () {};
const inheritedThrow = function () {};
const customPrototype = Object.create(Object.getPrototypeOf(inherited));
customPrototype.return = inheritedReturn;
customPrototype.throw = inheritedThrow;
Object.setPrototypeOf(inherited, customPrototype);
console.log(
  "inherited methods",
  inherited.return === inheritedReturn,
  inherited.throw === inheritedThrow,
  "return" in inherited,
  "throw" in inherited,
);

function* values() { yield 1; }
const generator: any = values();
console.log("generator", typeof generator.return, typeof generator.throw);
"#,
    );

    assert_eq!(
        stdout,
        "map undefined undefined false false function true\n\
         set undefined undefined false false function true\n\
         string undefined undefined false false function true\n\
         own methods true true true true\n\
         inherited methods true true true true\n\
         generator function function\n"
    );
}
