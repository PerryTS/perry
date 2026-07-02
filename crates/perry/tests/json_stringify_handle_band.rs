//! Regression (#5437, Next.js dynamic-page render): `JSON.stringify` with a
//! replacer (function or array-of-keys whitelist) over an object/array holding
//! a value in the small-handle band must not SIGSEGV. Next.js render reaches
//! `JSON.stringify(value, replacer)` over a render object that contains a
//! revocable-Proxy id (in the `[0xF0000, 0x100000)` band). The replacer/plain
//! stringify walk classified that id as a heap pointer and dereferenced it as a
//! GcHeader / ObjectHeader / ArrayHeader, crashing the bundle on `/posts/[id]`
//! and `/fetcher`. The fix rejects the handle band (emitting `null`) before any
//! deref, across every stringify deref primitive.

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
        .env("PERRY_ALLOW_PERRY_FEATURES", "1")
        .env("PERRY_ALLOW_EVAL", "1")
        .env("PERRY_ALLOW_UNIMPLEMENTED", "1")
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
        "compiled binary crashed (handle-band JSON.stringify regression?)\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn stringify_with_replacer_over_handle_band_value_does_not_segfault() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// A revocable / plain Proxy is stored as a small-handle-band id. Holding it in
// an object/array and stringifying through each replacer path must not crash.
const target = { a: 1, b: "x" };
const p = new Proxy(target, {});
const obj = { name: "root", child: p, list: [p, 2, p] };

// 1. Function replacer, pretty (3-arg) — the path that crashed in
//    dispatch_pointer_with_replacer.
const a = JSON.stringify(obj, (_k: string, v: any) => v, 2);
// 2. Function replacer, compact.
const b = JSON.stringify(obj, (_k: string, v: any) => v);
// 3. Array-of-keys whitelist replacer — the path that crashed in
//    is_object_pointer / stringify_value(_pretty).
const c = JSON.stringify(obj, ["name", "child", "list"]);
// 4. No replacer at all (plain stringify_value_depth path).
const d = JSON.stringify(obj);

// The handle is not a serializable object; each form must still produce a
// well-formed string that round-trips through JSON.parse and keeps "root".
for (const s of [a, b, c, d]) {
  const parsed = JSON.parse(s);
  if (parsed.name !== "root") throw new Error("lost name in: " + s);
  // A handle-band (Proxy) value is not introspectable during stringify, so the
  // contract is that each serializes to `null` — assert it directly (not just
  // "no crash / name survives"): a regression that dropped or mangled the
  // field would otherwise pass silently.
  if (parsed.child !== null) throw new Error("child not null in: " + s);
  if (!Array.isArray(parsed.list) || parsed.list.length !== 3) {
    throw new Error("list shape changed in: " + s);
  }
  if (parsed.list[0] !== null || parsed.list[1] !== 2 || parsed.list[2] !== null) {
    throw new Error("list contents changed in: " + s);
  }
}
console.log("ok");
"#,
    );
    assert_eq!(stdout, "ok\n");
}
