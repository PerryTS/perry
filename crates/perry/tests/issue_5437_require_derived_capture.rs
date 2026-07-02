//! Regression test for #5437 (Next.js W6, OTel `trace.getSpan` wall): a local
//! bound to a PROPERTY of a `require(...)` result (`const h = require(s).x` or
//! the destructured `const { x: h } = require(s)`), CAPTURED by a class method,
//! read `undefined` inside the method even though it was correct at module/
//! function scope.
//!
//! This was the next wall after the W6 member-new capture fix. The render
//! threw `TypeError: Cannot read properties of undefined (reading 'getSpan')`
//! from Next's tracer `getActiveScopeSpan() { return trace.getSpan(...) }`,
//! where `trace` is destructured from the (require'd) `@opentelemetry/api`
//! module and captured by the tracer class.
//!
//! Root: the W6 fix made a bare-identifier `new C(localCaptures...)` fill the
//! synthesized `__perry_cap_*` params from the class's DECL-SITE capture
//! snapshot (`js_class_capture_value(cid, slot)`) and discard the appended
//! `LocalGet` cap arg — the snapshot being authoritative because the bundle's
//! multi-level capture chain can materialize a mis-boxed value into the
//! appended arg. But the snapshot only exists for classes that reach the
//! `RegisterClassCaptures` decl-site. An inline anonymous class capturing a
//! `require(...)`-derived local has NO registered snapshot, so the snapshot
//! read returned `undefined`, dropping the (correct) appended cap value.
//!
//! Fix: `js_class_capture_value_or(cid, slot, fallback)` returns the snapshot
//! slot when a snapshot is registered for `cid`, else the `new`-site appended
//! cap arg (`fallback`). Keeps W6 (snapshot wins when present) while restoring
//! the appended value for the snapshot-less case.
//!
//! Mirrors the minimal repros that pinned the wall (`require().prop` captured
//! by a class method fails; `obj.prop` (no require) and capturing the whole
//! `require()` result both pass).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn require_derived_local_captured_by_class_method_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");

    // A sibling CJS module exporting a `trace`-like object with getter props,
    // mirroring the compiled `@opentelemetry/api` shape (a defineProperty
    // getter returning a class-instance-ish object with a method).
    let api = dir.path().join("api.js");
    std::fs::write(
        &api,
        r#"
"use strict";
var r = {};
Object.defineProperty(r, "__esModule", { value: true });
var TraceAPI = { getSpan: function () { return "SPAN_OK"; } };
Object.defineProperty(r, "trace", { enumerable: true, get: function () { return TraceAPI; } });
Object.defineProperty(r, "context", { enumerable: true, get: function () { return { active: function () { return {}; } }; } });
module.exports = r;
"#,
    )
    .expect("write api");

    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
"use strict";
// 1. const h = require(spec).prop  (member, not destructured) captured by a class method
const h = require("./api.js").trace;
const impl1 = new class { m() { return typeof h + ":" + h.getSpan(); } };

// 2. destructured const { trace: h2 } = require(spec) captured by a class method
const { trace: h2, context: c2 } = require("./api.js");
const impl2 = new class { m() { return typeof h2 + ":" + h2.getSpan(); } };

// 3. require-derived local captured inside a FUNCTION-level class
function mk() {
  const h3 = require("./api.js").trace;
  const i = new class { m() { return typeof h3 + ":" + h3.getSpan(); } };
  return i.m();
}

// 4. control: capturing the WHOLE require() result still works
const m4 = require("./api.js");
const impl4 = new class { m() { return typeof m4 + ":" + m4.trace.getSpan(); } };

// 5. control: capturing a plain (non-require) object property still works
const obj = { trace: { getSpan: function () { return "PLAIN_OK"; } } };
const h5 = obj.trace;
const impl5 = new class { m() { return typeof h5 + ":" + h5.getSpan(); } };

console.log("1=" + impl1.m());
console.log("2=" + impl2.m());
console.log("3=" + mk());
console.log("4=" + impl4.m());
console.log("5=" + impl5.m());
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
        stdout,
        "1=object:SPAN_OK\n2=object:SPAN_OK\n3=object:SPAN_OK\n4=object:SPAN_OK\n5=object:PLAIN_OK\n",
        "a require()-derived local captured by a class method must resolve to \
         the captured object (not undefined) — #5437 OTel getSpan wall"
    );
}
