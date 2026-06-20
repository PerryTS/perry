//! Regression test for #5458: `new Request(url, init)` must read `method`
//! (and `body`/`headers`) from a *runtime* init object, not only from inline
//! object literals. Codegen's `extract_options_fields` fast path recognized
//! inline `{...}` literals, recorded option-object locals, and `__AnonShape_`
//! synthesis; for any other init shape — a call-expression result
//! (`new Request(url, f())`), a spread literal (`{ ...e }`), or a dynamic
//! object — it evaluated and *discarded* the init, silently defaulting
//! `method` back to `"GET"`. Under Hono that mis-dispatched every POST to the
//! GET handler (or 404'd) because `request.method` read `"GET"`.
//!
//! The fix adds a runtime fallback (`js_request_new_from_init`) that reads the
//! fields off the init object at runtime. This test exercises the init shapes
//! that previously lost `method`, plus the inline forms that always worked,
//! and confirms body/headers survive the runtime path too.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn request_init_dynamic_method_survives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Inline literal (fast path) — always worked.
console.log("A=" + new Request("http://h/x", { method: "POST" }).method);
// Function-call result as init (was dropped → GET).
function f(): RequestInit { return { method: "POST" }; }
console.log("B=" + new Request("http://h/x", f()).method);
// Ternary-returning-literal call result (was dropped → GET).
function g(b: boolean): RequestInit { return b ? { method: "PUT" } : { method: "POST" }; }
console.log("C=" + new Request("http://h/x", g(false)).method);
// Plain const-bound literal (fast path via option_object_locals).
const d: RequestInit = { method: "POST" };
console.log("D=" + new Request("http://h/x", d).method);
// Spread literal (was dropped → GET).
const e = { method: "POST" };
console.log("E=" + new Request("http://h/x", { ...e }).method);
// Runtime init also carries body/headers through the fallback.
function h(): RequestInit { return { method: "PUT", body: "a=1", headers: { "x-test": "1" } }; }
const r = new Request("http://h/x", h());
console.log("F=" + r.method);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "A=POST\nB=POST\nC=POST\nD=POST\nE=POST\nF=PUT\n",
        "Request init method must survive every init shape (#5458)"
    );
}
