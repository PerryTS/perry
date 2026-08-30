//! Regression tests for the setPrototypeOf cycle-walk chain-end bug
//! (regression introduced by 1f4a2c5bd / #5763, bisected via the Next.js
//! boot failure).
//!
//! The OrdinarySetPrototypeOf cycle check uses Floyd's tortoise-and-hare.
//! Two chain-end bugs made `Object.setPrototypeOf(obj, proto)` throw
//! "Cannot convert undefined or null to object" on perfectly ordinary,
//! acyclic prototype chains:
//!
//! 1. Only the hare's SECOND step was null-guarded. On any acyclic chain
//!    longer than one link (every function/class proto:
//!    fn -> Function.prototype -> Object.prototype -> null) the hare reached
//!    null first and the next iteration called advance(null) ->
//!    js_object_get_prototype_of(null) -> TypeError. comment-json's
//!    `__extends` hit this on every transpiled subclass, killing Next.js
//!    server boot.
//!
//! 2. `js_object_get_prototype_of` returns undefined (not spec's
//!    object-or-null) for some exotic receivers; the walk fed that back into
//!    the next advance and threw. comment-json's `__extends` feature-test
//!    `{ __proto__: [] }` hit this at boot. Undefined can never be part of a
//!    genuine cycle, so it must end the walk like null.
//!
//! All expected outputs are byte-for-byte what `node
//! --experimental-strip-types` prints.

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
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Bug 1 guard: setPrototypeOf onto acyclic chains of length 2 and 4 must
/// succeed and establish inheritance. Pre-fix the hare walked past the chain
/// end and every one of these threw "Cannot convert undefined or null to
/// object".
#[test]
fn set_prototype_of_acyclic_chain_longer_than_one_link() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const base = {
  greet() {
    return "hi";
  },
};

// Chain length 2: base -> Object.prototype -> null.
const obj: any = {};
Object.setPrototypeOf(obj, base);
console.log(obj.greet());

// Chain length 4: deep2 -> deep1 -> base -> Object.prototype -> null.
const deep1 = Object.create(base);
const deep2 = Object.create(deep1);
const target: any = {};
Object.setPrototypeOf(target, deep2);
console.log(target.greet());
console.log("ok");
"#,
    );
    assert_eq!(
        stdout, "hi\nhi\nok\n",
        "setPrototypeOf onto an acyclic chain of length >= 2 must not throw \
         (hare must freeze at chain end)"
    );
}

/// The `__extends` statics link that killed Next.js boot: setPrototypeOf of
/// one function onto another. The proto chain of a function is
/// fn -> Function.prototype -> Object.prototype -> null (three links), so
/// pre-fix this threw on every transpiled subclass.
#[test]
fn set_prototype_of_function_statics_link() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function Base() {}
function Derived() {}
Object.setPrototypeOf(Derived, Base);
console.log(Object.getPrototypeOf(Derived) === Base);
console.log("statics linked");
"#,
    );
    assert_eq!(
        stdout, "true\nstatics linked\n",
        "setPrototypeOf(fn, fn) — a 3-link proto chain — must not throw"
    );
}

/// Perry represents a declared class as an INT32 ClassRef, not as the heap
/// function object used for an ordinary function declaration. The ClassRef
/// therefore needs its own static-prototype recording path. Effect's
/// `Schema.Opaque` uses this pattern and subclasses must inherit the schema
/// object's `ast` field through the generated base class.
#[test]
fn set_prototype_of_class_ref_links_static_object_properties() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const schema = { ast: { marker: "schema-ast" } };

function opaque() {
  class Opaque {}
  return Object.setPrototypeOf(Opaque, schema);
}

const Opaque = opaque();
class PartialRequest extends Opaque {}
console.log(PartialRequest.ast.marker);

Object.setPrototypeOf(Opaque, null);
console.log(PartialRequest.ast);
"#,
    );
    assert_eq!(
        stdout, "schema-ast\nundefined\n",
        "ClassRef Object.setPrototypeOf must link and clear inherited static object fields"
    );
}

/// Bug 2 guard: comment-json's `__extends` feature test. The object-literal
/// `{ __proto__: [] }` routes through the same cycle walk with an exotic
/// receiver whose getPrototypeOf reports undefined mid-walk; undefined must
/// terminate the walk like null instead of being fed back into the next
/// advance. Pre-fix, merely EVALUATING the literal threw and killed boot.
///
/// NOTE: this deliberately does not assert `probe instanceof Array` — that
/// is `true` in node but still `false` in perry (a separate, pre-existing
/// instanceof/literal-`__proto__` gap unchanged by this fix). This test
/// guards only the throw.
#[test]
fn object_literal_proto_array_feature_test() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const probe: any = { __proto__: [] };
console.log(typeof probe);
console.log("feature test ok");
"#,
    );
    assert_eq!(
        stdout, "object\nfeature test ok\n",
        "evaluating {{ __proto__: [] }} must not throw \
         (undefined getPrototypeOf ends the walk)"
    );
}

/// The cycle check itself must keep working: setting a prototype that would
/// form a cycle still throws a TypeError, and the object stays usable.
#[test]
fn cycle_detection_still_throws() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const a: any = {};
const b: any = {};
Object.setPrototypeOf(b, a);
let threw = false;
try {
  Object.setPrototypeOf(a, b);
} catch (e) {
  threw = true;
}
console.log("cycle detected:", threw);
"#,
    );
    assert_eq!(
        stdout, "cycle detected: true\n",
        "a genuine prototype cycle must still be rejected with a TypeError"
    );
}

/// `Object.setPrototypeOf(Ctor, obj)` sets the CONSTRUCTOR's [[Prototype]].
/// It must not put `obj` on the chain instances inherit from, and it must not
/// mutate `obj` itself. Perry records the link in CLASS_STATIC_PROTOTYPES for
/// exactly that reason: `CLASS_PROTOTYPE_OBJECTS` means "what instances inherit
/// from", so storing it there made `new Opaque().ast` resolve the static and
/// made prototype-method mirroring write into the user's object.
#[test]
fn set_prototype_of_class_ref_is_invisible_to_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const schema = { ast: "STATIC-ONLY", greet() { return "static-only"; } };
class Opaque {}
Object.setPrototypeOf(Opaque, schema);

const inst = new Opaque();
console.log(inst.ast);
console.log(typeof inst.greet);
try { inst.greet(); console.log("NO THROW"); } catch (e) { console.log("throws " + e.constructor.name); }
console.log("ast" in inst, "greet" in inst);
console.log(JSON.stringify(inst));

// The constructor side still sees them.
console.log(Opaque.ast, typeof Opaque.greet, Opaque.greet());

// Installing a prototype method must not write into `schema`.
Opaque.prototype.added = function () { return "proto-method"; };
console.log(JSON.stringify(Object.keys(schema)));
console.log(Object.prototype.hasOwnProperty.call(schema, "added"));
"#,
    );
    assert_eq!(
        stdout,
        concat!(
            "undefined\n",
            "undefined\n",
            "throws TypeError\n",
            "false false\n",
            "{}\n",
            "STATIC-ONLY function static-only\n",
            "[\"ast\",\"greet\"]\n",
            "false\n",
        ),
        "the constructor's [[Prototype]] must stay off the instance chain and \
         out of the user's object"
    );
}

/// The static side of the same link: an inherited static DATA read, a direct
/// static METHOD call on the class ref (which used to throw "is not a
/// function"), and `Object.getPrototypeOf` all resolve through it.
#[test]
fn set_prototype_of_class_ref_serves_the_whole_static_side() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const schema = { ast: 1, make(x) { return x + 1; } };
class Opaque {}
Object.setPrototypeOf(Opaque, schema);
class Derived extends Opaque {}

console.log(Object.getPrototypeOf(Opaque) === schema);
console.log(Opaque.ast, Derived.ast);
console.log(Opaque.make(1), Derived.make(2));
console.log("ast" in Opaque, "make" in Derived, "nope" in Derived);

const alias = Opaque;
console.log(alias.make(10));

Object.setPrototypeOf(Opaque, null);
console.log(Opaque.ast, Derived.ast, Object.getPrototypeOf(Opaque));
"#,
    );
    assert_eq!(
        stdout,
        concat!(
            "true\n",
            "1 1\n",
            "2 3\n",
            "true true false\n",
            "11\n",
            "undefined undefined null\n",
        ),
        "static reads, static calls, `in`, and getPrototypeOf must all route \
         through the recorded constructor prototype, and clear with it"
    );
}
