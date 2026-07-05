//! Regression test for #5961: native URLSearchParams methods were unreachable
//! through dynamically-typed access. The instance is an ordinary object
//! (class_id == 0, `_entries`/`_owner` slots) whose method surface existed
//! only via static type-directed lowering — the moment the receiver's static
//! type was lost (`any`, containers, bundled/minified code), `sp.append(...)`
//! threw "append is not a function" and `typeof sp.append` read `undefined`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
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

/// The #5961 shape: a type-erased receiver must still expose the callable
/// method surface, both as a fused call and as a property read.
#[test]
fn type_erased_searchparams_methods_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const u = new URL("https://x.com/p");
const o: any = [u.searchParams][0]; // launder the static type

console.log("T", typeof o.append);

o.append("a", "1");
o.append("b", "two words");
o.set("a", "2");
console.log("GET", o.get("a"), o.get("missing"));
console.log("HAS", o.has("b"), o.has("nope"));
console.log("STR", o.toString());
console.log("SIZE", o.size);
o.delete("b");
console.log("AFTER", u.toString());

// Property-read-then-call (non-fused) must work too.
const m: any = o.append;
m("c", "3");
console.log("BOUND", o.has("c"));
"#,
    );
    assert!(stdout.contains("T function"), "typeof: {stdout}");
    assert!(stdout.contains("GET 2 null"), "get: {stdout}");
    assert!(stdout.contains("HAS true false"), "has: {stdout}");
    assert!(stdout.contains("STR a=2&b=two+words"), "toString: {stdout}");
    assert!(stdout.contains("SIZE 2"), "size: {stdout}");
    assert!(
        stdout.contains("AFTER https://x.com/p?a=2"),
        "delete/owner-sync: {stdout}"
    );
    assert!(stdout.contains("BOUND true"), "bound method: {stdout}");
}
