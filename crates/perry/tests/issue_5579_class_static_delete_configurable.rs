//! Regression (#5579): `delete C.m` for a `static m()` must actually unregister
//! the static method so it stops being an own property of the class constructor.
//!
//! Root cause: #5490 rerouted the codegen `delete` arms through the value-form
//! wrappers `js_object_delete_field_value` / `js_object_delete_dynamic_value`,
//! whose `is_pointer` guard (added to make `delete (5).x` a no-op) silently
//! dropped class-reference receivers — a class ref is INT32-tagged, not a heap
//! pointer. So `delete C.m` returned `true` without ever calling
//! `class_mark_key_deleted`, leaving `hasOwnProperty(C, "m")` true after the
//! delete. test262's `verifyProperty` configurable check (delete-then-assert the
//! key is gone) then failed "m descriptor should be configurable" across ~296
//! generated `language/.../class/elements/*-static-method-rs-*` cases.
//!
//! Fix: the value-form wrappers detect a class-ref receiver via `class_ref_id`
//! and forward the class id to the existing class-delete path (which
//! `js_object_delete_field` already handles for sub-0x10000 "pointers").

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn delete_class_static_method_is_configurable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// Mirrors test262 propertyHelper.verifyProperty's `configurable` check:
// delete the property through a parameter receiver, then assert it is gone.
function verifyConfigurable(obj: any, name: string): boolean {
  delete obj[name];
  return !Object.prototype.hasOwnProperty.call(obj, name);
}

class C {
  static m() { return 1; }
}
// Descriptor must report configurable BEFORE the delete...
const d = Object.getOwnPropertyDescriptor(C, "m");
console.log("static.configurable:", d ? d.configurable : "(undefined)");
// ...and the delete must actually remove it.
console.log("static.deleted:", verifyConfigurable(C, "m"));
console.log("static.descAfter:", JSON.stringify(Object.getOwnPropertyDescriptor(C, "m")));

// Dynamic-key form: `delete C["m"]`.
class C2 {
  static m() { return 2; }
}
console.log("static.dyn.deleted:", verifyConfigurable(C2, "m"));

// Instance methods on the reflective prototype were already correct — keep
// them covered so the static fix doesn't regress them.
class D {
  m() { return 3; }
}
console.log("proto.deleted:", verifyConfigurable(D.prototype, "m"));

console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("static.configurable: true"),
        "static method descriptor must be configurable\n{out}"
    );
    assert!(
        out.contains("static.deleted: true"),
        "delete C.m must remove the static method (hasOwnProperty false after)\n{out}"
    );
    assert!(
        out.contains("static.descAfter: undefined"),
        "getOwnPropertyDescriptor(C, \"m\") must be undefined after delete\n{out}"
    );
    assert!(
        out.contains("static.dyn.deleted: true"),
        "delete C[\"m\"] (dynamic key) must remove the static method\n{out}"
    );
    assert!(
        out.contains("proto.deleted: true"),
        "delete D.prototype.m must still remove the instance method\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
