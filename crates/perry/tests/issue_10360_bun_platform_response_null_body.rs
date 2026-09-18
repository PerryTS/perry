//! #10360 — a body under a null-body status (204/205/304) throws in Node for
//! both `new Response(...)` and `Response.json(...)`, but Bun accepts it.
//! Perry follows Node by default (gap test
//! `test_gap_response_null_body_status_10360.ts`) and Bun under
//! `--platform bun`, including in a dependency's top-level code.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const DEPENDENCY: &str = r#"
export const depStatus = (() => {
  try {
    return String(new Response("", { status: 204 }).status);
  } catch (e: any) {
    return "THREW " + e.constructor.name;
  }
})();
"#;

const MAIN: &str = r#"
import { depStatus } from "./dependency.ts";

const t = (label: string, f: () => any) => {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e: any) {
    console.log(label, "THREW " + e.constructor.name);
  }
};
console.log("dep '' 204", depStatus);
t("ctor '' 204", () => new Response("", { status: 204 }).status);
t("ctor 'x' 205", () => new Response("x", { status: 205 }).status);
t("ctor 'x' 304", () => new Response("x", { status: 304 }).status);
t("ctor null 204", () => new Response(null, { status: 204 }).status);
t("json 204", () => Response.json({ a: 1 }, { status: 204 }).status);
t("json 600", () => Response.json({}, { status: 600 }).status);
"#;

fn compile_and_run(platform: Option<&str>) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("dependency.ts"), DEPENDENCY).expect("write dependency");
    std::fs::write(dir.path().join("main.ts"), MAIN).expect("write entry");
    let output = dir.path().join("main_bin");
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join("main.ts"))
        .arg("-o")
        .arg(&output);
    if let Some(platform) = platform {
        command.arg("--platform").arg(platform);
    }
    let compile = command.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    run(&output, dir.path())
}

fn run(output: &Path, dir: &Path) -> String {
    let run = Command::new(output)
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

#[test]
fn bun_platform_accepts_a_body_with_a_null_body_status() {
    // Bun 1.3.14 prints exactly this. The status range check is not a
    // null-body rule and still throws in Bun (error text is covered by the
    // gap test, and Bun words it differently, so only the class is printed).
    let expected = "\
dep '' 204 204
ctor '' 204 204
ctor 'x' 205 205
ctor 'x' 304 304
ctor null 204 204
json 204 204
json 600 THREW RangeError
";
    assert_eq!(compile_and_run(Some("bun")), expected);
}

#[test]
fn node_platform_rejects_a_body_with_a_null_body_status() {
    // Control for the test above: the same program without `--platform bun`
    // follows Node, so the Bun result is the platform switch, not a lost check.
    let expected = "\
dep '' 204 THREW TypeError
ctor '' 204 THREW TypeError
ctor 'x' 205 THREW TypeError
ctor 'x' 304 THREW TypeError
ctor null 204 204
json 204 THREW TypeError
json 600 THREW RangeError
";
    assert_eq!(compile_and_run(None), expected);
}
