//! #10222: namespace classes must evaluate and publish like class declarations.
//! Expected output is checked against Bun (namespaces require TS transforms).

use std::path::PathBuf;
use std::process::Command;

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"experimentalDecorators":true}}"#,
    )
    .expect("write tsconfig");
    let perry = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime_dir = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| perry.parent().expect("binary directory").to_path_buf());
    let compile = Command::new(&perry)
        .current_dir(dir.path())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir)
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "run failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

const ISSUE_REPRO: &str = r#"
const Proto: any = { of(self: any) { return self } }
const Key = function () { function K() {}; Object.setPrototypeOf(K, Proto); return function (key: string) { (K as any).key = key; return K } }
const t = (name: string, f: () => any) => { try { console.log(name, JSON.stringify(f())) } catch (e: any) { console.log(name, "THROW", e.message) } }
class Base { static of(x: any) { return x } static b = 1 }
export namespace N {
  export class Plain { static p = 7; static m() { return "m" } }
  export class FromBase extends Base {}
  export class FromCall extends (Key as any)()("@x/FromCall") {}
  export const inside = () => [typeof Plain, Plain.p, Plain.m(), typeof FromBase, FromBase.b, FromBase.of({ a: 1 }), typeof FromCall]
  export const ofInside = () => FromCall.of({ z: 1 })
  export const keyInside = () => (FromCall as any).key
}
t("C1 inside: Plain/FromBase/FromCall", () => N.inside())
t("C2 outside N.Plain", () => [typeof N.Plain, N.Plain.p, N.Plain.m()])
t("C3 outside N.FromBase", () => [typeof N.FromBase, N.FromBase.b, N.FromBase.of({ b: 2 })])
t("C4 outside N.FromCall", () => [typeof N.FromCall, (N.FromCall as any).key])
t("C5 outside N.FromCall.of", () => N.FromCall.of({ c: 3 }))
t("C6 keys of N", () => Object.keys(N))
t("C7 inside FromCall.of", () => N.ofInside())
t("C8 inside FromCall.key", () => N.keyInside())
"#;

#[test]
fn issue_repro_matches_bun() {
    assert_eq!(
        compile_and_run(ISSUE_REPRO),
        concat!(
            "C1 inside: Plain/FromBase/FromCall [\"function\",7,\"m\",\"function\",1,{\"a\":1},\"function\"]\n",
            "C2 outside N.Plain [\"function\",7,\"m\"]\n",
            "C3 outside N.FromBase [\"function\",1,{\"b\":2}]\n",
            "C4 outside N.FromCall [\"function\",\"@x/FromCall\"]\n",
            "C5 outside N.FromCall.of {\"c\":3}\n",
            "C6 keys of N [\"Plain\",\"FromBase\",\"FromCall\",\"inside\",\"ofInside\",\"keyInside\"]\n",
            "C7 inside FromCall.of {\"z\":1}\n",
            "C8 inside FromCall.key \"@x/FromCall\"\n",
        )
    );
}

const NESTED_AND_PRIVATE: &str = r#"
class C { static value = "top" }
export namespace Outer {
  export const first = 1;
  console.log("keys during", Object.keys(Outer).join(","));
  export class C { static value = "outer"; value = 9 }
  export function readPrivate() { return new ReleaseError().message }
  class ReleaseError { static prefix = "release"; message = ReleaseError.prefix + " failed" }
  export namespace Inner {
    export const before = 2;
    export class C { static value = "inner"; value = 11 }
    export const after = 3;
  }
  export const last = 4;
}
namespace Local {
  export class C { static value = "local" }
}
console.log(C.value, Outer.C.value, Outer.Inner.C.value, Local.C.value);
console.log(Outer.readPrivate(), typeof (Outer as any).ReleaseError);
const read = Outer.readPrivate;
console.log(read());
const outer = new Outer.C();
const inner = new Outer.Inner.C();
console.log(outer.value, inner.value, outer instanceof Outer.C, inner instanceof Outer.Inner.C, inner instanceof Outer.C);
console.log(Outer.C.name, Outer.Inner.C.name, Local.C.name);
console.log(JSON.stringify(Object.keys(Outer)));
console.log(JSON.stringify(Object.keys(Outer.Inner)));
"#;

#[test]
fn nested_and_private_classes_keep_their_bindings() {
    assert_eq!(
        compile_and_run(NESTED_AND_PRIVATE),
        concat!(
            "keys during first\n",
            "top outer inner local\n",
            "release failed undefined\n",
            "release failed\n",
            "9 11 true true false\n",
            "C C C\n",
            "[\"first\",\"C\",\"readPrivate\",\"Inner\",\"last\"]\n",
            "[\"before\",\"C\",\"after\"]\n",
        )
    );
}

const CLASS_EVALUATION: &str = r#"
const events: string[] = [];
function key(name: string) { events.push("key:" + name); return name }
function init(name: string, value: number) { events.push("init:" + name); return value }
function decorate(target: any) { events.push("decorate:" + target.name); target.decorated = target.a + target.b }
function parent() { events.push("heritage"); return class { static inherited = 5 } }
export namespace Evaluation {
  export class C extends parent() {
    static [key("a")] = init("a", 7);
    static { events.push("block:" + this.a); }
    static [key("b")] = init("b", this.inherited + this.a);
    [key("method")]() { return "method" }
  }
  @decorate
  export class D { static a = C.a; static b = C.b }
}
console.log(JSON.stringify(events));
console.log(Evaluation.C.a, Evaluation.C.b, Evaluation.D.decorated, new Evaluation.C().method());
"#;

#[test]
fn computed_names_static_blocks_and_decorators_run_in_order() {
    assert_eq!(
        compile_and_run(CLASS_EVALUATION),
        concat!(
            "[\"heritage\",\"key:a\",\"key:b\",\"key:method\",\"init:a\",\"block:7\",\"init:b\",\"decorate:D\"]\n",
            "7 12 19 method\n",
        )
    );
}

const TAGGED_ERROR: &str = r#"
const Schema = {
  TaggedErrorClass<T>() {
    return (tag: string, fields: any) => class extends Error {
      static tag = tag;
      static of(value: any) { return value }
      constructor(value: any) { super(value.message); this._tag = tag }
    }
  }
};
export namespace Errors {
  export class Failure extends Schema.TaggedErrorClass<Failure>()("Failure", {}) {}
  class ReleaseError extends Schema.TaggedErrorClass<ReleaseError>()("ReleaseError", {}) {}
  export function release() { return new ReleaseError({ message: "release" })._tag }
  export const inside = () => Failure.of({ value: 7 });
}
const failure = new Errors.Failure({ message: "failed" });
console.log(failure._tag, failure.message, failure instanceof Errors.Failure, failure instanceof Error);
console.log(Errors.Failure.tag, Errors.release());
console.log(JSON.stringify(Errors.inside()), JSON.stringify(Errors.Failure.of({ value: 9 })));
"#;

#[test]
fn tagged_error_factory_heritage_matches_bun() {
    assert_eq!(
        compile_and_run(TAGGED_ERROR),
        "Failure failed true true\nFailure ReleaseError\n{\"value\":7} {\"value\":9}\n"
    );
}
