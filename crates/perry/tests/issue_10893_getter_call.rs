//! Direct calls through class getters invoke the value returned by the getter.
//!
//! #10893's hard case is the static getter reached through a RUNTIME-resolved
//! heritage (`class G extends make() {}`), whose value is keyed on `this`.
//! It is resolved in the runtime's class-id parent-chain walk
//! (`js_class_static_method_call` -> `try_static_accessor_value_call`), not in
//! codegen: codegen only sees the static `extends_name` chain and cannot know
//! what a runtime parent carries.
//!
//! The `MyArr` rows are the CONTROL for that decision and are not incidental.
//! `class MyArr extends Array {}` also lowers to a dynamic parent (`Array` is
//! not a user class, so `lookup_class` misses and `extends_expr` is captured),
//! so a codegen-side "dynamic parent => read the property and call it" rule
//! passes the `G`/`H` rows above while silently breaking every inherited
//! Array static (#7541) — `MyArr.from`'s property-GET form is still
//! `undefined`, so the read-then-call lowering yields "value is not a
//! function". Keep both halves in one test so that trade cannot be made again
//! without going red here.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

/// The fix lives in `perry-runtime`, so this fixture is only meaningful when it
/// links an archive built from the tree under test. `perry-runtime` is
/// rlib-only; `libperry_{runtime,stdlib}.a` come from the `-static` wrappers.
fn runtime_dir() -> PathBuf {
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
            .expect("build static runtime archives");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    target.join("debug")
}

const SOURCE: &str = r#"
let reads = 0;
class C {
  get g() { reads++; return (n: number) => n + 2; }
}
const c = new C();
console.log("instance", c.g(1), reads);
const instanceFn = c.g;
console.log("read then call", instanceFn(1), reads);

function make() {
  const cache = new Map<any, any>();
  return class {
    static get g() {
      if (!cache.has(this)) cache.set(this, (n: number) => n + 8);
      return cache.get(this);
    }
  };
}
class G extends make() {}
class H extends G {}
const staticFn = G.g;
console.log("static read", staticFn(1));
console.log("static direct", G.g(1), H.g(2));

// #7541 control: a dynamic parent that carries no such accessor must keep
// reaching the runtime's inherited-builtin-static arms.
class MyArr extends Array {}
class Indirect extends MyArr {}
console.log("array from", MyArr.from([1, 2, 3]).join(","));
console.log("array of", MyArr.of(7, 8).join(","));
console.log("array isArray", MyArr.isArray([]), MyArr.isArray(1));
console.log("array indirect", Indirect.from([1, 2]).join(","));
"#;

const EXPECTED: &str = "instance 3 1
read then call 3 2
static read 9
static direct 9 10
array from 1,2,3
array of 7,8
array isArray true false
array indirect 1,2
";

#[test]
fn direct_calls_through_instance_and_static_getters() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write fixture");

    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
}
