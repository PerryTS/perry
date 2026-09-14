//! Symbol-keyed properties added to a declared class's `.prototype` after the
//! declaration (`C.prototype[S] = f`, `Object.defineProperty(C.prototype, S, ...)`,
//! `Object.assign(C.prototype, { [Symbol.iterator]() {} })`) must be visible on
//! instances and subclass instances, exactly like string keys. drizzle-orm's
//! `applyEffectWrapper` installs effect's `Effectable.Prototype` this way, and
//! `yield* query` needs the copied `[Symbol.iterator]`.

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
    if let Some(dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
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
const fn = function (this: any) { return "called" }
const custom = Symbol("custom")
class A1 {}; (A1.prototype as any)[Symbol.iterator] = fn
t("P1", () => typeof (new A1() as any)[Symbol.iterator])
class A2 {}; Object.assign(A2.prototype, { [Symbol.iterator]: fn })
t("P2", () => typeof (new A2() as any)[Symbol.iterator])
class A3 {}; Object.defineProperty(A3.prototype, Symbol.iterator, { value: fn, writable: true, configurable: true })
t("P3", () => typeof (new A3() as any)[Symbol.iterator])
class A4 {}; Object.assign(A4.prototype, { [custom]: fn })
t("P4", () => (new A4() as any)[custom]())
class A5 {}; (A5.prototype as any)[Symbol.iterator] = fn
class A5s extends A5 {}
t("P5", () => typeof (new A5s() as any)[Symbol.iterator])
t("P6", () => Symbol.iterator in (new A2() as any))
class A7 {}; Object.defineProperty(A7.prototype, custom, { get(this: any) { return this.v }, configurable: true })
t("P7", () => { const a: any = new A7(); a.v = 7; return a[custom] })
class A8 { [Symbol.iterator]() { return "declared" } }
t("P8", () => (new A8() as any)[Symbol.iterator]())
class SingleShotGen { called = false; self: any; constructor(self: any) { this.self = self } next(a?: any) { return this.called ? { value: a, done: true } : ((this.called = true), { value: this.self, done: false }) } }
const EffectProto: any = { op: "E", [Symbol.iterator]() { return new SingleShotGen(this) } }
class Raw { execute: () => number; constructor(execute: () => number) { this.execute = execute } }
Object.assign(Raw.prototype, { ...EffectProto })
class SubRaw extends Raw {}
t("Y1", () => { const r = new Raw(() => 1); function* g(): Generator<any, any, any> { const x = yield* r; return x } const it = g(); const a = it.next(); const b = it.next("sent"); return [a.value === r, b.value, b.done] })
t("Y2", () => { const r = new SubRaw(() => 2); function* g(): Generator<any, any, any> { return yield* r } const it = g(); it.next(); return it.next("sub").value })
class It {}; (It.prototype as any)[Symbol.iterator] = function* () { yield 1; yield 2 }
t("Y3", () => { const out: number[] = []; for (const v of new It() as any) out.push(v); return [out, [...(new It() as any)]] })
let getterCalls = 0
class AccessorBase {}
Object.defineProperty(AccessorBase.prototype, custom, { get() { getterCalls++; return 7 }, configurable: true })
class AccessorChild extends AccessorBase {}
const accessor: any = new AccessorChild()
t("H1", () => [custom in accessor, getterCalls])
t("H2", () => [accessor[custom], getterCalls])
class UndefinedBase {}
Object.defineProperty(UndefinedBase.prototype, custom, { value: undefined })
t("H3", () => custom in new UndefinedBase())
class Overridden {}
Object.defineProperty(Overridden.prototype, custom, { value: 9 })
const noPrototype: any = new Overridden()
Object.setPrototypeOf(noPrototype, null)
t("H4", () => [noPrototype[custom], custom in noPrototype])
const replacementPrototype: any = new Overridden()
Object.setPrototypeOf(replacementPrototype, {})
t("H5", () => [replacementPrototype[custom], custom in replacementPrototype])
"#;

const EXPECTED: &str = "P1 \"function\"
P2 \"function\"
P3 \"function\"
P4 \"called\"
P5 \"function\"
P6 true
P7 7
P8 \"declared\"
Y1 [true,\"sent\",true]
Y2 \"sub\"
Y3 [[1,2],[1,2]]
H1 [true,0]
H2 [7,1]
H3 true
H4 [null,false]
H5 [null,false]
";

#[test]
fn symbol_props_written_to_declared_class_prototypes_reach_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let oracle = Command::new("node")
        .arg(&entry)
        .output()
        .expect("run Node oracle");
    assert!(oracle.status.success(), "Node oracle failed: {:?}", oracle);
    assert_eq!(String::from_utf8_lossy(&oracle.stdout), EXPECTED);
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
