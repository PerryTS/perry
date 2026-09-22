//! Regression test for #10835: a `static get`/`static set` on a class that
//! closes over its factory's arguments read the capture from the class's
//! DECLARATION-site slot instead of the receiver it was invoked on.
//!
//! The capture slots are keyed by class name, so the last evaluation of a
//! class declaration wins: every earlier class produced by the same factory
//! silently answered with the last one's captured values. Static *methods*
//! already resolved per-receiver; only accessors took the decl-site path.
//!
//! This is Effect's `Context.Service` shape. `makeService(id, fields)` returns
//! a class whose `static get layer()` closes over `id`, and OpenCode builds
//! several services from it:
//!
//! ```ts
//! class Auth  extends makeService("tag-Auth",  { ... }) {}
//! class Flags extends makeService("tag-Flags", { ... }) {}
//! ```
//!
//! Pre-fix, `Auth.layer` answered `{"for":"tag-Flags"}` — the LAST class built
//! from the factory — while `Auth.key` (a plain static field, a different
//! path) stayed correct. That asymmetry is the tell, so this test pins both.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
function serviceBase(id: string) {
  return class {
    static key = id
  }
}
function makeService(id: string, fields: Record<string, any>) {
  class ConfigTag extends serviceBase(id) {
    static fields = fields
    static get layer() { return { for: id } }
  }
  return ConfigTag
}

class Auth extends makeService("tag-Auth", { password: 1 }) {}
class Flags extends makeService("tag-Flags", { autoShare: 2 }) {}

console.log("Auth.key    =", Auth.key)
console.log("Flags.key   =", Flags.key)
console.log("Auth.layer  =", JSON.stringify(Auth.layer))
console.log("Flags.layer =", JSON.stringify(Flags.layer))
console.log("Auth.fields =", JSON.stringify(Auth.fields))
console.log("distinct    =", Auth.key !== Flags.key)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "\
Auth.key    = tag-Auth
Flags.key   = tag-Flags
Auth.layer  = {\"for\":\"tag-Auth\"}
Flags.layer = {\"for\":\"tag-Flags\"}
Auth.fields = {\"password\":1}
distinct    = true
";

#[test]
fn static_accessor_reads_its_own_receivers_captures() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let output = root.join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(root)
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
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        EXPECTED,
        "pre-fix, Auth.layer answered the LAST factory evaluation's capture"
    );
}
