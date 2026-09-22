//! Regression test for #10911: `this` inside a STATIC getter resolved to the
//! class that DECLARED the getter instead of the class it was read from.
//!
//! `js_object_get_field_by_name` walks the static side by re-entering ITSELF
//! with the parent class object as the receiver. That walk was written when
//! effect's `ast` was a static DATA field, where the object does not matter.
//! effect now exposes `ast` as a static GETTER, so the getter ran with
//! `this === <the parent>`; spec `OrdinaryGet` threads the original Receiver
//! through the chain unchanged.
//!
//! The runtime already had the device for this — `accessor_receiver_override`,
//! which `resolve_proto_chain_field_inner` uses so an INHERITED INSTANCE getter
//! binds the original instance. The static side simply never produced or
//! consumed it.
//!
//! `this` and the capture/private OWNER are deliberately different here, which
//! is what makes the test pin both: `this` is the class the read started from,
//! while the owner is the evaluation the getter was FOUND on — the object whose
//! `__perry_ctor_caps` hold its captured variables. Binding the owner to the
//! subclass loses every capture (an earlier version of the fix did exactly
//! that, turning `A.tagv` from "a" into undefined), so a correct fix has to
//! keep them apart.
//!
//! Downstream this is #10891: effect's
//! `static get ast() { return getClassSchema(this).ast }` memoised the schema
//! against the base class, so `Schema.decodeUnknownSync` built decoded values
//! from the base and they were not `instanceof` their own class.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
function factory(tag: string) {
  return class Out {
    static get who() { return this }      // captures nothing
    static get tagv() { return tag }      // captures `tag`
  }
}
class A extends factory("a") {}
class B extends factory("b") {}

// `this` is the class the read started from, not the declaring class.
console.log("1", (A as any).who === A)
console.log("2", (B as any).who === B)
console.log("3", ((A as any).who || {}).name)

// ...while captures still come from the evaluation the getter lives on.
console.log("4", (A as any).tagv)
console.log("5", (B as any).tagv)

// A class with no captures at all was always fine; keep it that way.
function plain() { return class { static get who() { return this } } }
class C extends plain() {}
console.log("6", (C as any).who === C)

// Reading through a dynamic receiver must agree with the direct form.
const holder: any = { A }
console.log("7", holder.A.who === A)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "1 true\n2 true\n3 A\n4 a\n5 b\n6 true\n7 true\n";

#[test]
fn static_getter_binds_this_to_the_class_it_was_read_from() {
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, EXPECTED,
        "pre-fix rows 1-3 answered with the DECLARING class (`Out`); an \
         over-eager fix instead broke rows 4-5 by binding the capture owner \
         to the subclass"
    );
}
