//! COVER3: a named access the whole-program facts prove calls ONE class
//! accessor, whose receiver can also be `undefined`, keeps the one receiver
//! test in front of the direct call. The nullish base takes the complete
//! operation: [[Get]] / PutValue's ToObject throws the TypeError (6.2.5.5,
//! 6.2.5.6 step 3.a, 7.1.18), the store after its value was evaluated
//! (13.15.2), and the accessor body never runs with `this` undefined.
//! (Before: the getter ran with `this` = undefined and the binary crashed.)
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

/// `PERRY_RUNTIME_DIR` when the caller names prebuilt runtime archives, else
/// the workspace's debug archives, built once.
fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .args([
                "build",
                "-p",
                "perry-runtime-static",
                "-p",
                "perry-stdlib-static",
            ])
            .output()
            .expect("build static runtime archives");
        assert!(
            build.status.success(),
            "static runtime build failed\nstderr:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        assert!(
            Command::new("bash")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../scripts/gc_call_effects/sidecars.sh"
                ))
                .args(["--cargo-profile", "debug"])
                .status()
                .is_ok_and(|status| status.success()),
            "scripts/gc_call_effects/sidecars.sh failed"
        );
    });
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    target.join("debug")
}

const CLASSES: &str = r#"
let getRan = 0;
let setRan = 0;
let evals = 0;
class Base {
  _a = 0;
  get alphaTest(): number { getRan++; return this._a; }
  set alphaTest(v: number) { setRan++; this._a = v; }
}
class A extends Base { isA = true; }
class B extends Base { isB = true; }
const objs: any[] = [];
for (let i = 0; i < 4; i++) objs.push(i % 2 === 0 ? new A() : new B());
function val(i: number): number { evals++; return i; }
function store(i: number): void { objs[i & 7].alphaTest = val(i); }
function load(i: number): number { return objs[i & 7].alphaTest; }
let sum = 0;
for (let i = 0; i < 4; i++) { store(i); sum += load(i); }
"#;

fn compile_and_run(name: &str, tail: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join(format!("{name}.ts"));
    let out = dir.path().join(name);
    std::fs::write(&src, format!("{CLASSES}{tail}")).expect("write source");
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .env("PERRY_NO_CACHE", "1")
        .env_remove("PERRY_FACTS_EXPLAIN")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&out).output().expect("run binary");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstdout:\n{stdout}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

#[test]
fn proven_setter_nullish_receiver_throws_after_value() {
    let out = compile_and_run(
        "set_nullish",
        "try { store(5); console.log(\"set no throw\"); } catch (e) { console.log(\"set\", e instanceof TypeError, setRan, evals); }\nconsole.log(\"sum\", sum);\n",
    );
    assert_eq!(out, "set true 4 5\nsum 6\n");
}

#[test]
fn proven_getter_nullish_receiver_throws_before_getter() {
    let out = compile_and_run(
        "get_nullish",
        "try { sum += load(5); console.log(\"get no throw\"); } catch (e) { console.log(\"get\", e instanceof TypeError, getRan); }\nconsole.log(\"sum\", sum);\n",
    );
    assert_eq!(out, "get true 4\nsum 6\n");
}
