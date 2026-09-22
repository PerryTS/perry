//! Regression: reading an INHERITED member of a non-escaping object as a
//! VALUE must resolve through the prototype chain, not fold to `undefined`.
//!
//! Issue #10689. `const o = { a: 1 }; typeof o.toString` answered `undefined`
//! while `o.toString()` answered correctly, and the divergence was
//! order-dependent: adding `JSON.stringify(o)` earlier in the function
//! "repaired" it.
//!
//! The mechanism is escape analysis, not the lazy `globalThis` realm the
//! report guessed at. `collectors/escape_check.rs`'s `PropertyGet` arm treated
//! EVERY read on a scalar-replacement candidate as a "plain field read — safe",
//! including reads of keys the class chain does not declare. The local then
//! stayed scalar-replaced (no heap object at all) and
//! `expr/property_get.rs`'s scalar arm folded the slot-less read to the
//! constant `undefined`. `JSON.stringify(o)` only appeared to fix it because
//! passing `o` to a call makes it escape; `JSON.stringify` of an UNRELATED
//! object — which forces the realm just the same — does not, and that case is
//! pinned below.
//!
//! The three WRITE arms of the same analysis already carried this rule
//! (#9024 `PropertySet`/`PutValueSet`, #9460 `PropertyUpdate`); only the read
//! arm was missing it.
//!
//! Fixtures are `.js`, not `.ts`, so `Object.prototype.zz = 7` and
//! `o.nope` are valid source without `as any` casts — a cast would route the
//! read through the dynamic path and miss the statically-shaped lowering the
//! bug lived in.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `entry` into `dir`, compile it with `--no-cache`
/// `PERRY_NO_AUTO_OPTIMIZE=1` (links the prebuilt runtime archive), run it, and
/// return stdout. Mirrors the helper in `builtin_namespace_unknown_member.rs`.
fn compile_and_run_js(dir: &Path, entry: &str, source: &str) -> String {
    let entry_path = dir.join(entry);
    std::fs::write(&entry_path, source).expect("write fixture");
    let output = dir.join(format!("{entry}.bin"));

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry_path)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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

/// The core case. `o` is read but never passed anywhere, so nothing makes it
/// escape and nothing forces the realm — the exact program shape #10689
/// reported. Every line was `undefined` / `false` before the fix.
#[test]
fn inherited_object_prototype_members_read_as_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
const o = { a: 1 };
console.log("typeof-constructor:", typeof o.constructor);
console.log("typeof-toString:", typeof o.toString);
console.log("typeof-hasOwnProperty:", typeof o.hasOwnProperty);
console.log("typeof-valueOf:", typeof o.valueOf);
console.log("typeof-isPrototypeOf:", typeof o.isPrototypeOf);
console.log("typeof-propertyIsEnumerable:", typeof o.propertyIsEnumerable);
console.log("ctor-is-Object:", o.constructor === Object);
console.log("toString-is-proto-toString:", o.toString === Object.prototype.toString);
"#,
    );
    for expected in [
        "typeof-constructor: function",
        "typeof-toString: function",
        "typeof-hasOwnProperty: function",
        "typeof-valueOf: function",
        "typeof-isPrototypeOf: function",
        "typeof-propertyIsEnumerable: function",
        "ctor-is-Object: true",
        "toString-is-proto-toString: true",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}

/// The ordering half of #10689, and the case that names the real mechanism.
///
/// `JSON.stringify` is what the report found "repaired" the read — but only
/// when it was handed `o` ITSELF, which makes `o` escape. Stringifying an
/// UNRELATED object forces exactly the same lazy realm and must not change the
/// answer, and the read before it must agree with the read after it. Before the
/// fix all four reads here were `undefined`; a fix that merely forced the realm
/// from the read path would leave this test passing for the wrong reason, so it
/// asserts the invariant (before == after == "function"), not the repair.
#[test]
fn inherited_value_read_does_not_depend_on_evaluation_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
const o = { a: 1 };
console.log("before-toString:", typeof o.toString);
console.log("before-constructor:", typeof o.constructor);
// Forces `populate_global_this_builtins` without `o` escaping.
JSON.stringify({ unrelated: 2 });
console.log("after-toString:", typeof o.toString);
console.log("after-constructor:", typeof o.constructor);
"#,
    );
    for expected in [
        "before-toString: function",
        "before-constructor: function",
        "after-toString: function",
        "after-constructor: function",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}

/// The other direction of the same pair: calls through the inherited chain were
/// always correct and must STAY correct, so a future change cannot "fix" reads
/// by routing them through something that breaks the call path.
#[test]
fn inherited_object_prototype_members_are_still_callable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
const o = { a: 1 };
console.log("call-toString:", o.toString());
console.log("call-hasOwnProperty-present:", o.hasOwnProperty("a"));
console.log("call-hasOwnProperty-absent:", o.hasOwnProperty("b"));
console.log("concat:", "" + o);
console.log("template:", `${o}`);
console.log("in-operator:", "constructor" in o);
console.log("proto-identity:", Object.getPrototypeOf(o) === Object.prototype);
// Read-then-call through a local, the form that needs a real function value.
const f = o.toString;
console.log("read-then-call:", f.call(o));
"#,
    );
    for expected in [
        "call-toString: [object Object]",
        "call-hasOwnProperty-present: true",
        "call-hasOwnProperty-absent: false",
        "concat: [object Object]",
        "template: [object Object]",
        "in-operator: true",
        "proto-identity: true",
        "read-then-call: [object Object]",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}

/// The same hole on a declared class: a PROTOTYPE METHOD read as a value off a
/// non-escaping `new` answered `undefined` while the fused call answered
/// correctly. Same arm, same fold — `m` is not a declared FIELD, so scalar
/// replacement had no slot for it.
#[test]
fn prototype_method_read_as_value_on_non_escaping_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
// `m` is body-summarizable, which is what keeps `c` scalar-replaced across
// the fused call below. A method whose body the summary rejects escapes its
// receiver for that reason alone and would not exercise the fold.
class C {
  m() { return 1; }
}
const c = new C();
console.log("typeof-method:", typeof c.m);
console.log("call-method:", c.m());
"#,
    );
    for expected in ["typeof-method: function", "call-method: 1"] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}

/// A user-added `Object.prototype` member is inherited by a plain object. The
/// folded read could not see it at all, which is the form the report warned
/// about: a library feature-detects and silently takes the other branch.
#[test]
fn user_added_object_prototype_member_is_inherited() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
Object.prototype.perryInherited = 7;
const o = { a: 1 };
console.log("inherited-value:", o.perryInherited);
console.log("own-value:", o.a);
"#,
    );
    for expected in ["inherited-value: 7", "own-value: 1"] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}

/// The opposite failure the fix must not cause: a key that is on neither the
/// object nor its prototype chain still reads `undefined`, and own fields still
/// come from their scalar slots. Without this, "make every miss escape" would
/// pass the tests above while answering some non-`undefined` value here.
#[test]
fn genuinely_absent_key_is_still_undefined_and_own_fields_are_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
const o = { a: 1, b: "x" };
console.log("absent-typeof:", typeof o.definitelyNotThere);
console.log("absent-is-undefined:", o.definitelyNotThere === undefined);
console.log("own-a:", o.a);
console.log("own-b:", o.b);
const n = { count: 0 };
n.count = n.count + 41;
n.count++;
console.log("own-updated:", n.count);
"#,
    );
    for expected in [
        "absent-typeof: undefined",
        "absent-is-undefined: true",
        "own-a: 1",
        "own-b: x",
        "own-updated: 42",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}\nstdout:\n{stdout}"
        );
    }
}
