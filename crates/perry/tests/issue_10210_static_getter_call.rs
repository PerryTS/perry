//! Regression for #10210: a static getter on a class whose `extends` clause is a
//! call expression (effect v4's `Context.Service<..>()(id)` returns a plain
//! `function KeyClass(){}` whose `[[Prototype]]` was swapped with
//! `Object.setPrototypeOf`) must be able to call `this.of(x)` — the CALL path
//! has to resolve the name wherever the READ path does (static accessors,
//! per-evaluation parent class objects, and the function-valued ancestor's
//! swapped prototype), and the closure-parent edge must be found on an
//! ancestor of the receiver's class, not only on the class itself.

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
const Proto: any = { of(self: any) { return self } }
const Key = function () {
  function K() {}
  Object.setPrototypeOf(K, Proto)
  ;(K as any).isK = true
  return function (key: string) { (K as any).key = key; return K }
}
const t = (name: string, f: () => any) => {
  try { console.log(name, JSON.stringify(f())) } catch (e: any) { console.log(name, "THROW", e.message) }
}
// nested class + subclass, static getter calls this.of(x)
const mk1 = (id: string) => { class T extends (Key as any)()(id) { static get g() { return (this as any).of({ n: 1 }) } }; return T as any }
class S1 extends mk1("S1") {}
t("N1", () => S1.g)
// same receiver, read first then call
const mk2 = (id: string) => { class T extends (Key as any)()(id) { static get g() { const f = (this as any).of; return [typeof f, f.call(this, { n: 2 })] } }; return T as any }
class S2 extends mk2("S2") {}
t("N2", () => S2.g)
// alias `this` into a local first (opencode's `const tag = this`)
const mk4 = (id: string) => { class T extends (Key as any)()(id) { static get g() { const tag = this as any; return tag.of({ n: 4 }) } }; return T as any }
class S4 extends mk4("S4") {}
t("N4", () => S4.g)
// nested, no subclass
t("N5", () => mk1("T5").g)
// top-level + subclass
class T6 extends (Key as any)()("T6") { static get g() { return (this as any).of({ n: 6 }) } }
class S6 extends T6 {}
t("N6", () => S6.g)
// top-level, no subclass
t("N7", () => T6.g)
// the closure-parent edge sits on an ancestor: inherited reads on a grandchild
class T8 extends (Key as any)()("T8") {}
class S8 extends T8 {}
t("N8", () => [S8.key, (S8 as any).isK, typeof (S8 as any).of])
// static METHOD with the same body
const mk10 = (id: string) => { class T extends (Key as any)()(id) { static m() { return (this as any).of({ n: 10 }) } }; return T as any }
class S10 extends mk10("S10") {}
t("N10", () => S10.m())
// calling the function a static getter returns, on a class extending a call expression
const mk11 = (id: string) => { class T extends (Key as any)()(id) { static get g() { return () => 11 } }; return T as any }
t("M1", () => mk11("T11").g())
// the closure returned by a static getter calls tag.of (config-service.ts shape)
const mk12 = (id: string) => { class T extends (Key as any)()(id) { static get layer() { const tag = this as any; return () => tag.of({ n: 12 }) } }; return T as any }
class S12 extends mk12("S12") {}
t("M2", () => { const thunk = S12.layer; return thunk() })
"#;

const EXPECTED: &str = "N1 {\"n\":1}
N2 [\"function\",{\"n\":2}]
N4 {\"n\":4}
N5 {\"n\":1}
N6 {\"n\":6}
N7 {\"n\":6}
N8 [\"T8\",true,\"function\"]
N10 {\"n\":10}
M1 11
M2 {\"n\":12}
";

#[test]
fn static_getter_calls_this_of_on_call_expression_heritage() {
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
