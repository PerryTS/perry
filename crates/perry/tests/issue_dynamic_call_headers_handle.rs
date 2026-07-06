//! A fused dynamic method call (`v.append(...)` where `v`'s static Web-Fetch
//! type was erased — stored in an `any`/untyped local, or destructured) routes
//! through `js_native_call_method`. Its #5961 URLSearchParams fast-path matches
//! the method names `append`/`set`/`get`/`has`/`delete`/`toString` and derefs
//! the receiver as a heap `ObjectHeader` to sniff the `_entries` shape.
//!
//! But a `Headers` instance is a native *handle* — nanbox-pointer-tagged, yet
//! its "pointer" is a small integer id in the low handle band, not a heap
//! object. Dereferencing it as an `ObjectHeader` SIGSEGV'd (fault address in
//! the low handle band). The WHATWG NullableHeaders builder does exactly this:
//!
//! ```js
//! for (const [name, value] of entries) {   // destructured -> type-erased
//!   if (value === null) K.delete(name);
//!   else K.append(name, value);
//! }
//! ```
//!
//! The fast-path now skips small handles, so a Headers handle falls through to
//! the fetch/Headers method dispatcher.

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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn headers_methods_dispatch_on_type_erased_receiver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// The WHATWG NullableHeaders builder shape: destructured (type-erased) names
// flow into Headers `delete`/`append`, fused to dynamic method calls.
function build(lists: Array<Array<[string, string | null]>>): Headers {
  const K = new Headers();
  for (const z of lists) {
    for (const [name, value] of z) {
      if (value === null) K.delete(name);
      else K.append(name, value);
    }
  }
  return K;
}

const h = build([
  [["Content-Type", "application/json"], ["x-beta", null]],
  [["x-beta", "on"]],
]);

// Read back through an untyped local — also a type-erased dynamic call.
const v: any = h;
process.stdout.write(
  "ct=" + v.get("content-type") +
  " beta=" + v.get("x-beta") +
  " hasCT=" + v.has("Content-Type") + "\n"
);
"#,
    );
    assert_eq!(
        out, "ct=application/json beta=on hasCT=true",
        "Headers methods on a type-erased receiver"
    );
}
