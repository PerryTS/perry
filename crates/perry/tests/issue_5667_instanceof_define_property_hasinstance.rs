//! Regression tests for #5667: `Object.defineProperty(C, Symbol.hasInstance, …)`
//! must be honored by `instanceof`, on both class and function receivers.
//!
//! Spec `InstanceofOperator` consults the RHS's OWN `@@hasInstance` first:
//!   - a callable own hook is invoked and its `ToBoolean` result returned —
//!     this takes precedence over the ordinary class-chain walk, so even
//!     `new C() instanceof C` runs the hook (it may return `false`);
//!   - a present-but-non-callable own hook (e.g. `{ value: 1 }`) is a
//!     `TypeError`, not a silent fall-through;
//!   - an own value of `undefined`/`null` means "no hook" → ordinary instanceof;
//!   - a generic redefine (`{ enumerable: true }`) must not clobber the
//!     existing hook value.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> (bool, String, String) {
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[test]
fn define_property_hasinstance_overrides_class_chain_and_brand_checks() {
    let src = r#"
"use strict";
// Hook returning false wins over the ordinary class-chain fast path.
class C1 {}
Object.defineProperty(C1, Symbol.hasInstance, { value() { return false; } });
console.log("1:" + (new C1() instanceof C1));

// Brand-style hook (zod 4 installs its instanceof check exactly this way).
class D {}
Object.defineProperty(D, Symbol.hasInstance, { value: (x) => !!(x && x.__d) });
console.log("2a:" + (({ __d: 1 }) instanceof D));
console.log("2b:" + (({}) instanceof D));

// Explicit `value: undefined` is "no hook" → ordinary instanceof (no throw).
class C3 {}
Object.defineProperty(C3, Symbol.hasInstance, { value: undefined });
console.log("3:" + (new C3() instanceof C3));

// Function receiver, plus a generic redefine that must NOT clobber the hook.
function F() {}
Object.defineProperty(F, Symbol.hasInstance, { value: (x) => x === 42, configurable: true });
console.log("5a:" + ((42) instanceof F));
Object.defineProperty(F, Symbol.hasInstance, { enumerable: true });
console.log("5b:" + ((42) instanceof F));

// Plain inheritance still works (no own hook present).
class A {}
class B extends A {}
console.log("6a:" + (new B() instanceof A));
console.log("6b:" + (new A() instanceof B));
"#;
    let (ok, stdout, stderr) = compile_and_run(src);
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout, "1:false\n2a:true\n2b:false\n3:true\n5a:true\n5b:true\n6a:true\n6b:false\n",
        "defineProperty(@@hasInstance) must drive instanceof (own hook before class chain), \
         honor explicit value:undefined as no-hook, and survive a generic redefine"
    );
}

#[test]
fn non_callable_own_hasinstance_throws_type_error() {
    let src = r#"
"use strict";
class C {}
Object.defineProperty(C, Symbol.hasInstance, { value: 1 });
try {
    void (({}) instanceof C);
    console.log("no-throw");
} catch (e) {
    console.log(e && e.constructor ? e.constructor.name : String(e));
}
"#;
    let (ok, stdout, stderr) = compile_and_run(src);
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout, "TypeError\n",
        "a present-but-non-callable own @@hasInstance must throw TypeError, not fall through"
    );
}
