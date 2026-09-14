//! #10222 follow-up: an exported function of a TypeScript namespace referenced as a
//! VALUE from inside the namespace body (`const f = g`, `call(g)`, `Effect.gen(g)`)
//! must be the same callable that `NS.g` yields from outside. Direct calls were
//! already redirected to the namespace's static method; value references still
//! lowered to a module-function reference that had no body behind it.

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
const t = (name: string, f: () => any) => { try { console.log(name, JSON.stringify(f())) } catch (e: any) { console.log(name, "THROW", e.message) } }
const call = (f: any) => f()
const gen = (body: () => Generator<any, any, any>) => { const it = body(); let r = it.next(); while (!r.done) r = it.next(r.value); return r.value }
export namespace NS {
  export function g() { return 1 }
  function h() { return 2 }
  export function* gg() { const a: any = yield 1; return a + 1 }
  export const v1 = () => { const f = g; return f() }
  export const v2 = () => [g].map((f) => f())
  export const v3 = () => call(g)
  export const v4 = () => call(h)
  export const v5 = () => [typeof g, typeof h, g === NS.g]
  export const v6 = () => call(gg).next().value
  export const v7 = () => gen(gg)
  export const v8 = () => g.name
  export const v9 = () => call(v1)
  export const v10 = () => g()
}
t("V1", () => NS.v1())
t("V2", () => NS.v2())
t("V3", () => NS.v3())
t("V4", () => NS.v4())
t("V5", () => NS.v5())
t("V6", () => NS.v6())
t("V7", () => NS.v7())
t("V8", () => NS.v8())
t("V9", () => NS.v9())
t("V10", () => NS.v10())
t("O1", () => { const f = NS.g; return f() })
t("O2", () => call(NS.gg).next().value)
"#;

const EXPECTED: &str = "V1 1
V2 [1]
V3 1
V4 2
V5 [\"function\",\"function\",true]
V6 1
V7 2
V8 \"g\"
V9 1
V10 1
O1 1
O2 1
";

#[test]
fn exported_namespace_functions_are_first_class_inside_the_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
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
    assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
}
