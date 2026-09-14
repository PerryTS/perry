//! `new Headers(init)` must accept a Proxy wrapping a record, reading the init's
//! own keys and values through the proxy's traps. perry took the record path only
//! for plain heap objects, so a proxied header record raised
//! "Headers constructor: init is not iterable" — OpenCode's request path hit this
//! on every `run` (tracker #10107).
//! Two neighbours are deliberately out of scope here: a proxy wrapping an *array*
//! (`Array.from` over such a value segfaults, #10270) and a proxied init passed
//! through `new Request(url, { headers })`, which takes a different path (#10274).

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
const dump = (h: any) => { const out: string[] = []; h.forEach((v: string, k: string) => out.push(k + "=" + v)); return out.sort() }
t("P1 plain proxy", () => dump(new Headers(new Proxy({ "x-a": "1", "x-b": "2" }, {}) as any)))
t("P2 proxy with get trap", () => dump(new Headers(new Proxy({ "x-a": "1" }, { get: (t: any, k: any) => (typeof k === "string" && k in t ? "trapped" : (t as any)[k]) }) as any)))
t("P3 proxy with ownKeys trap hiding a key", () => dump(new Headers(new Proxy({ "x-a": "1", "x-b": "2" }, { ownKeys: () => ["x-a"], getOwnPropertyDescriptor: () => ({ configurable: true, enumerable: true, value: "1" }) }) as any)))
t("P4 proxy over empty object", () => dump(new Headers(new Proxy({}, {}) as any)))
t("P6 nested proxy", () => dump(new Headers(new Proxy(new Proxy({ "x-a": "1" }, {}), {}) as any)))
t("P8 plain object still works", () => dump(new Headers({ "x-a": "1" })))
t("P9 array still works", () => dump(new Headers([["x-a", "1"]])))
t("P10 map still works", () => dump(new Headers(new Map([["x-a", "1"]]) as any)))
"#;

const EXPECTED: &str = "P1 plain proxy [\"x-a=1\",\"x-b=2\"]\nP2 proxy with get trap [\"x-a=trapped\"]\nP3 proxy with ownKeys trap hiding a key [\"x-a=1\"]\nP4 proxy over empty object []\nP6 nested proxy [\"x-a=1\"]\nP8 plain object still works [\"x-a=1\"]\nP9 array still works [\"x-a=1\"]\nP10 map still works [\"x-a=1\"]\n";

#[test]
fn headers_accepts_a_proxied_record_init() {
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
