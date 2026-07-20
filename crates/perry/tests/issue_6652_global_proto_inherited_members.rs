//! Regression tests for #6652 (pi wall #6): bare identifiers that resolve to
//! Object.prototype-INHERITED members of the global object.
//!
//! Node resolves a bare `hasOwnProperty` through the scope chain to the
//! global object, which inherits `Object.prototype.hasOwnProperty` — so
//! `hasOwnProperty.call(o, k)` works. Perry's unknown-identifier-assume-
//! global lowering collapsed the ident in member-OBJECT position to the
//! bare `GlobalGet(0)` sentinel (which IS globalThis), discarding the
//! identifier name entirely: `hasOwnProperty.call(o, k)` read
//! `globalThis.call` (undefined) and threw "TypeError: value is not a
//! function". Trigger in the wild: @babel/types/lib/definitions/
//! placeholders.js (`hasOwnProperty.call(o, t4) || (o[t4] = [])`, 14 sites
//! in the pi bundle) during pi-native module init.
//!
//! The fix routes ALL unknown identifiers — member-object position included —
//! through the `js_global_get_or_throw_unresolved` by-name runtime lookup,
//! which serves globalThis' own AND inherited members with identity
//! preserved, and still throws the spec ReferenceError on a true miss.
//!
//! Receiver semantics (verified against node v26, both module/strict and
//! sloppy CJS): a bare CALL gets `this = undefined` (the global environment
//! record's WithBaseObject is undefined) — `toString()` is
//! "[object Undefined]", `hasOwnProperty("x")` throws "Cannot convert
//! undefined or null to object".

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

/// The @babel/types placeholders.js shape plus every access form: member
/// call on the bare ident, extraction with identity, typeof, bare calls
/// with their `this = undefined` semantics, other Object.prototype members,
/// use from inside a function body, and the spec ReferenceError on a true
/// miss. Expected output is node v26's, byte for byte.
#[test]
fn object_prototype_inherited_globals_match_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const o: any = { x: 1 };
console.log("call:", hasOwnProperty.call(o, "x"), hasOwnProperty.call(o, "y"));
const h: any = hasOwnProperty;
console.log("identity:", h === Object.prototype.hasOwnProperty);
console.log("extracted:", h.call({ y: 2 }, "y"), h.call({ y: 2 }, "z"));
console.log("typeof:", typeof hasOwnProperty, typeof isPrototypeOf);
console.log("toString():", String(toString()));
try {
  (hasOwnProperty as any)("x");
  console.log("bare-call: no throw");
} catch (e: any) {
  console.log("bare-call threw:", e.constructor.name + ": " + e.message);
}
console.log("isPrototypeOf:", isPrototypeOf === Object.prototype.isPrototypeOf);
function usesInherited(obj: any, key: string): boolean {
  return hasOwnProperty.call(obj, key);
}
console.log("in-function:", usesInherited({ k: 0 }, "k"), usesInherited({}, "k"));
try {
  // @ts-ignore -- deliberately unresolvable
  issue6652NeverDefined.foo;
  console.log("missing: no throw");
} catch (e: any) {
  console.log("missing threw:", e.constructor.name + ": " + e.message);
}
"#,
    );
    assert_eq!(
        stdout,
        "call: true false\n\
         identity: true\n\
         extracted: true false\n\
         typeof: function function\n\
         toString(): [object Undefined]\n\
         bare-call threw: TypeError: Cannot convert undefined or null to object\n\
         isPrototypeOf: true\n\
         in-function: true false\n\
         missing threw: ReferenceError: issue6652NeverDefined is not defined\n"
    );
}

/// The by-name runtime lookup must also serve runtime-CREATED globals in
/// member position — pre-fix `myGlobal.prop` read `globalThis.prop`
/// (undefined) and `myGlobal.method()` threw "value is not a function".
#[test]
fn runtime_created_global_member_access_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
(globalThis as any).issue6652RuntimeGlobal = { prop: 42, greet: () => "hi" };
// @ts-ignore -- deliberately unresolvable at compile time
console.log("member:", issue6652RuntimeGlobal.prop, issue6652RuntimeGlobal.greet());
"#,
    );
    assert_eq!(stdout, "member: 42 hi\n");
}
