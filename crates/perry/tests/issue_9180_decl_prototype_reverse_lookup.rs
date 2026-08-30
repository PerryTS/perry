//! Regression test for #9180: the reverse "which declared class's `.prototype`
//! is this heap object?" lookup.
//!
//! The lookup used to be a linear scan of `CLASS_DECL_PROTOTYPE_OBJECTS`; it is
//! now answered from an index maintained alongside that table. The failure mode
//! of getting an index like this wrong is SILENT — it does not crash, it reports
//! "not a prototype", and the class's vtable accessors stop being surfaced as
//! own properties of `C.prototype`. A first attempt at a pointer-keyed cache
//! shipped exactly that: `Object.getOwnPropertyDescriptor(C.prototype, "g")`
//! returned `undefined` where node returns a getter/setter descriptor.
//!
//! So this pins the observable surface rather than the data structure:
//! descriptor shapes for accessor and data properties, `hasOwnProperty`,
//! prototype identity, and `delete` — each of which routes through the reverse
//! lookup — with enough sibling classes materialized that a size-one table
//! cannot make a broken index look right, and repeated across allocation churn
//! because the collector rewrites the very addresses the index is keyed by.

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
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).arg("build");
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let build = command
            .args(["-p", "perry-runtime-static"])
            .output()
            .expect("build static runtime archive");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });

    perry_bin()
        .parent()
        .expect("Perry binary directory")
        .to_path_buf()
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .arg("--no-cache")
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
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Sibling classes whose prototypes are materialized before the assertions, so
/// the table under test holds more than the one entry being asked about.
const FILLERS: &str = r#"
class F0 { f0() { return 0; } get g0() { return 0; } }
class F1 { f1() { return 1; } get g1() { return 1; } }
class F2 { f2() { return 2; } get g2() { return 2; } }
class F3 { f3() { return 3; } get g3() { return 3; } }
class F4 { f4() { return 4; } get g4() { return 4; } }
class F5 { f5() { return 5; } get g5() { return 5; } }
const FILL: any[] = [
  F0.prototype, F1.prototype, F2.prototype,
  F3.prototype, F4.prototype, F5.prototype,
];
function churn(n: number): number {
  let sink = 0;
  for (let i = 0; i < n; i++) {
    const a: any[] = [];
    for (let j = 0; j < 40; j++) a.push({ i, j, s: "x" + j });
    sink += a.length;
  }
  return sink;
}
"#;

#[test]
fn class_prototype_accessors_reflect_as_own_descriptors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &format!(
            r#"{FILLERS}
class Base {{ bm() {{ return "bm"; }} }}
class C extends Base {{
  m() {{ return "m"; }}
  get g() {{ return "getter-g"; }}
  set g(v: string) {{ (this as any)._g = v; }}
}}

function report(tag: string) {{
  const acc: any = Object.getOwnPropertyDescriptor(C.prototype, "g");
  console.log(tag + ".accessor.present=" + (acc !== undefined));
  console.log(tag + ".accessor.hasGet=" + (acc !== undefined && typeof acc.get === "function"));
  console.log(tag + ".accessor.hasSet=" + (acc !== undefined && typeof acc.set === "function"));
  console.log(tag + ".accessor.get()=" + (acc !== undefined && acc.get ? acc.get.call({{}}) : "MISSING"));
  console.log(tag + ".accessor.hasValue=" + (acc !== undefined && "value" in acc));
  const data: any = Object.getOwnPropertyDescriptor(C.prototype, "m");
  console.log(tag + ".data.present=" + (data !== undefined));
  console.log(tag + ".data.value()=" + (data !== undefined && data.value ? data.value.call({{}}) : "MISSING"));
  console.log(tag + ".hasOwn.g=" + Object.prototype.hasOwnProperty.call(C.prototype, "g"));
  console.log(tag + ".identity=" + (Object.getPrototypeOf(new C()) === C.prototype));
  console.log(tag + ".isProtoOf=" + C.prototype.isPrototypeOf(new C()));
  // A miss must stay a miss: a plain object is not any class's prototype.
  console.log(tag + ".plainMiss=" + (Object.getOwnPropertyDescriptor({{ a: 1 }}, "g") === undefined));
}}

console.log("fillers=" + FILL.length);
report("fresh");
churn(2000);
report("afterChurn");

// A class whose prototype is first demanded AFTER the churn — the table grows
// again once addresses have already moved.
class Late {{ lm() {{ return "lm"; }} get lg() {{ return "lg"; }} }}
const late: any = Object.getOwnPropertyDescriptor(Late.prototype, "lg");
console.log("late.hasGet=" + (late !== undefined && typeof late.get === "function"));
churn(1000);
const late2: any = Object.getOwnPropertyDescriptor(Late.prototype, "lg");
console.log("late.afterChurn.hasGet=" + (late2 !== undefined && typeof late2.get === "function"));
"#
        ),
    );

    assert!(stdout.contains("fillers=6"), "stdout:\n{stdout}");
    for tag in ["fresh", "afterChurn"] {
        for (line, why) in [
            (
                format!("{tag}.accessor.present=true"),
                "accessor descriptor",
            ),
            (format!("{tag}.accessor.hasGet=true"), "getter"),
            (format!("{tag}.accessor.hasSet=true"), "setter"),
            (format!("{tag}.accessor.get()=getter-g"), "getter call"),
            (
                format!("{tag}.accessor.hasValue=false"),
                "accessor is not a data descriptor",
            ),
            (format!("{tag}.data.present=true"), "method descriptor"),
            (format!("{tag}.data.value()=m"), "method call"),
            (format!("{tag}.hasOwn.g=true"), "hasOwnProperty"),
            (format!("{tag}.identity=true"), "prototype identity"),
            (format!("{tag}.isProtoOf=true"), "isPrototypeOf"),
            (format!("{tag}.plainMiss=true"), "non-prototype receiver"),
        ] {
            assert!(
                stdout.contains(&line),
                "missing {why} ({line})\nstdout:\n{stdout}"
            );
        }
    }
    assert!(stdout.contains("late.hasGet=true"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("late.afterChurn.hasGet=true"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn deleting_a_prototype_method_removes_it_from_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &format!(
            r#"{FILLERS}
class E {{ em() {{ return "em"; }} get eg() {{ return "eg"; }} }}
const proto: any = E.prototype;
console.log("fillers=" + FILL.length);
console.log("before=" + (typeof new E().em));
console.log("delete.em=" + (delete proto.em));
console.log("descAfter=" + (Object.getOwnPropertyDescriptor(proto, "em") === undefined));
try {{
  (new E() as any).em();
  console.log("call=NO_THROW");
}} catch (e: any) {{
  console.log("call=throw:" + (e && e.constructor ? e.constructor.name : "?"));
}}
console.log("delete.eg=" + (delete proto.eg));
console.log("descEgAfter=" + (Object.getOwnPropertyDescriptor(proto, "eg") === undefined));
churn(1500);
console.log("descAfterChurn=" + (Object.getOwnPropertyDescriptor(proto, "em") === undefined));
"#
        ),
    );

    for line in [
        "before=function",
        "delete.em=true",
        "descAfter=true",
        "call=throw:TypeError",
        "delete.eg=true",
        "descEgAfter=true",
        "descAfterChurn=true",
    ] {
        assert!(stdout.contains(line), "missing {line}\nstdout:\n{stdout}");
    }
}
