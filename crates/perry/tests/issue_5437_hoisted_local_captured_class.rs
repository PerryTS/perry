//! Regression test for #5437 (Next.js ResponseCache `handleGet` wall): a
//! function-nested class that captures an enclosing local which is assigned
//! LATER in the same function body (after the class's hoisted declaration)
//! read `undefined` for that capture inside its methods.
//!
//! The render threw `TypeError: Cannot read properties of undefined (reading
//! 'get')` from `r.incrementalCache.get(...)` in the minified `nh.handleGet`,
//! where `r` is a `class f` instance whose `incrementalCache` field is a
//! CAPTURE of the hoisted `const incrementalCache = … || await
//! getIncrementalCache(…)` local.
//!
//! Root: the W6 / getSpan capture fix makes a bare-identifier
//! `new C(localCaptures...)` fill its synthesized `__perry_cap_*` params from
//! the class's DECL-SITE capture snapshot (`js_class_capture_value_or`), the
//! snapshot being authoritative because the bundle's multi-level capture chain
//! can materialize a mis-boxed value into the appended cap arg. But the
//! snapshot is registered at the class's DECLARATION position, and class
//! declarations hoist to the top of the function body — so the
//! `RegisterClassCaptures` statement runs BEFORE the captured local is
//! assigned (TDZ), recording `undefined`. The `new C` site then appended the
//! CORRECT post-assignment local, but the (undefined) snapshot won and
//! dropped it.
//!
//! Fix: `js_class_capture_value_or` falls back to the `new`-site appended cap
//! value when the snapshot SLOT holds `undefined` (not only when the whole
//! snapshot is absent). A snapshot slot holding a real value stays
//! authoritative (keeps W6); a require-derived class with no snapshot still
//! falls back (keeps getSpan).
//!
//! Pinned by the minimal repro (`w6-repro/classf/cf2.js`): class declared
//! BEFORE the await-assigned local fails on the pre-fix compiler with the
//! exact `.get`-on-undefined error; class declared AFTER (`cf1`/`cf3`) passes.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, src).expect("write entry");

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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// The failing shape: `class f` is HOISTED above the assignment of the
/// captured local (`incrementalCache`), so its decl-site snapshot records
/// `undefined` while the `new f` site appends the correct post-assignment
/// value. Pre-fix this read `undefined` inside `handleGet` → `.get` on
/// undefined.
#[test]
fn hoisted_class_captures_locally_assigned_local_resolves() {
    let out = compile_and_run(
        r#"
"use strict";
async function getIC(n) { return { get: (k) => ({ hit: k, n }) }; }
class RC {
  async handleResponse(n) {
    // class `f` hoists to the top of the body, ABOVE the assignment below.
    class f {
      handleGet(e) { return incrementalCache.get(e); }
    }
    const incrementalCache = (this.ic) || (await getIC(n));
    const r = new f();
    return r.handleGet(n);
  }
}
(async () => {
  const rc = new RC();
  console.log(JSON.stringify(await rc.handleResponse(1)));
  console.log(JSON.stringify(await rc.handleResponse(2)));
})();
"#,
    );
    assert_eq!(
        out, "{\"hit\":1,\"n\":1}\n{\"hit\":2,\"n\":2}\n",
        "a function-nested class capturing a later-assigned local must read \
         the live value, not the undefined decl-site snapshot — #5437 handleGet"
    );
}

/// Control: class declared AFTER the captured local is assigned. The snapshot
/// is correct here; must keep working (and did on the pre-fix compiler).
#[test]
fn class_declared_after_local_assignment_still_resolves() {
    let out = compile_and_run(
        r#"
"use strict";
async function getIC(n) { return { get: (k) => ({ hit: k, n }) }; }
class RC {
  async handleResponse(n) {
    const incrementalCache = (this.ic) || (await getIC(n));
    class f {
      handleGet(e) { return incrementalCache.get(e); }
    }
    const r = new f();
    return r.handleGet(n);
  }
}
(async () => {
  const rc = new RC();
  console.log(JSON.stringify(await rc.handleResponse(7)));
})();
"#,
    );
    assert_eq!(out, "{\"hit\":7,\"n\":7}\n");
}
