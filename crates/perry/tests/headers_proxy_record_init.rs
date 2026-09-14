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

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    if let Some(runtime) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(runtime);
    }
    // Use the same profile as this integration executable. Building archives
    // here would change Cargo feature unification beneath the running suite.
    perry_bin()
        .parent()
        .expect("compiler directory")
        .to_path_buf()
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
t("P11 non-enumerable key", () => { const o: any = {"x-a":"1"}; Object.defineProperty(o,"x-hidden",{value:"secret"}); return dump(new Headers(new Proxy(o,{}))) })
t("P12 missing descriptor", () => dump(new Headers(new Proxy({}, {ownKeys:()=>["x-ghost"]}))))
t("P13 enumerable symbol", () => { const o:any={}; o[Symbol("header")]="value"; try { new Headers(new Proxy(o,{})); return "no throw"; } catch(e:any) { return e.name; } })
t("P14 hidden symbol", () => { const o:any={}; Object.defineProperty(o,Symbol("hidden"),{value:"secret",enumerable:false}); return dump(new Headers(new Proxy(o,{}))); })
t("P15 descriptor/get order", () => { const order:string[]=[]; const o:any={"x-a":"1","x-b":"2"}; const h=new Headers(new Proxy(o,{getOwnPropertyDescriptor(t:any,k:any){order.push("desc:"+String(k));return Object.getOwnPropertyDescriptor(t,k);},get(t:any,k:any){if(typeof k==="string")order.push("get:"+k);return t[k];}})); return [dump(h),order]; })
"#;

const EXPECTED: &str = "P1 plain proxy [\"x-a=1\",\"x-b=2\"]\nP2 proxy with get trap [\"x-a=trapped\"]\nP3 proxy with ownKeys trap hiding a key [\"x-a=1\"]\nP4 proxy over empty object []\nP6 nested proxy [\"x-a=1\"]\nP8 plain object still works [\"x-a=1\"]\nP9 array still works [\"x-a=1\"]\nP10 map still works [\"x-a=1\"]\nP11 non-enumerable key [\"x-a=1\"]\nP12 missing descriptor []\nP13 enumerable symbol \"TypeError\"\nP14 hidden symbol []\nP15 descriptor/get order [[\"x-a=1\",\"x-b=2\"],[\"desc:x-a\",\"get:x-a\",\"desc:x-b\",\"get:x-b\"]]\n";

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-codegen")
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
    String::from_utf8(run.stdout).expect("UTF-8 output")
}

#[test]
fn headers_accepts_a_proxied_record_init() {
    assert_eq!(compile_and_run(SOURCE), EXPECTED);
}

#[test]
fn rejected_headers_init_diagnostic_identifies_the_value() {
    let source = r#"
for (const value of [123, true]) {
    try { new Headers(value as any); console.log("accepted"); }
    catch (e: any) { console.log(e.name, e.message); }
}
"#;
    assert_eq!(
        compile_and_run(source),
        "TypeError Headers constructor: init is not iterable (received 0x405ec00000000000)\nTypeError Headers constructor: init is not iterable (received 0x7ffc000000000004)\n"
    );
}
