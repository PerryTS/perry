//! #6084 item 6: one `Object.freeze` must not disable the dynamic-write fast
//! path process-wide.
//!
//! Both dynamic-write fast paths (`js_object_set_field_by_name_transition_fast`
//! and the transition-cache branch inside `js_object_set_field_by_name`) used to
//! be gated on the process-global `GLOBAL_DESCRIPTORS_IN_USE` latch, which flips
//! on ANY descriptor install anywhere — including the ones the runtime itself
//! performs (an Error's `stack` accessor, `arguments` objects, tagged-template
//! `raw`, typed-array props) and a userland `Object.freeze` on a completely
//! unrelated object. Once flipped it never reverts, so every dynamic property
//! write in the process fell back to the O(own-key-count) `[[Set]]` walk.
//!
//! The gate is now per-receiver: an own descriptor is visible in the object's
//! `OBJ_FLAG_HAS_DESCRIPTORS` GcHeader bit, and only prototype-level installs
//! (`Object.prototype`, a recorded `setPrototypeOf` target, a class prototype)
//! need a chain walk — mirroring what `ordinary_set`'s #5054 fast path already
//! did per receiver.
//!
//! These tests pin the CORRECTNESS half: every interception source must still
//! intercept a plain-data write after the fast path is re-enabled. The write
//! fast path was effectively dead on `main` (the latch is set before user code
//! runs), so this re-enables a path that needs its semantics nailed down.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");
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
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        run.status.success(),
        "binary failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

fn assert_lines(stdout: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}

/// An unrelated `Object.freeze` must not change the semantics of writes to
/// other objects — and every interception source must still fire.
#[test]
fn unrelated_freeze_keeps_write_semantics_for_every_interception_source() {
    let stdout = compile_and_run(
        r#"
// Flip the process-global descriptor latch via an object nobody writes to.
Object.freeze({ unrelated: 1 })

// 1. Plain fresh object: own data writes still land.
const plain: any = {}
plain.a = 1
plain.b = 2
plain.a = 3
console.log("plain:", plain.a, plain.b, Object.keys(plain).join(","))

// 2. Inherited SETTER on Object.prototype must still intercept.
Object.defineProperty(Object.prototype, "viaProto", {
  set(v: any) { (this as any)._proto_set = v },
  get() { return (this as any)._proto_set },
  configurable: true,
})
const p2: any = {}
p2.viaProto = 42
console.log("protoSetter:", p2._proto_set, Object.prototype.hasOwnProperty.call(p2, "viaProto"))

// 3. Inherited NON-WRITABLE data on Object.prototype must still block.
Object.defineProperty(Object.prototype, "roProto", { value: 7, writable: false, configurable: true })
const p3: any = {}
p3.roProto = 99
console.log("protoRO:", p3.roProto, Object.prototype.hasOwnProperty.call(p3, "roProto"))

// 4. OWN accessor on the receiver must still intercept.
const p4: any = {}
Object.defineProperty(p4, "own", {
  set(v: any) { (this as any)._own = v * 2 },
  get() { return (this as any)._own },
  configurable: true,
})
p4.own = 21
console.log("ownSetter:", p4._own)

// 5. OWN non-writable data on the receiver must still block.
const p5: any = {}
Object.defineProperty(p5, "ro", { value: 5, writable: false, configurable: true })
p5.ro = 123
console.log("ownRO:", p5.ro)

// 6. A frozen object itself still rejects writes (own + new keys).
const frozen: any = { x: 1 }
Object.freeze(frozen)
try { frozen.x = 2 } catch (e) {}
try { frozen.fresh = 1 } catch (e) {}
console.log("frozen:", frozen.x, frozen.fresh === undefined)

// 7. A sealed object rejects NEW keys but allows existing ones.
const sealed: any = { y: 1 }
Object.seal(sealed)
sealed.y = 2
try { sealed.brand = 1 } catch (e) {}
console.log("sealed:", sealed.y, sealed.brand === undefined)

console.log("DONE")
"#,
    );

    assert_lines(
        &stdout,
        &[
            "plain: 3 2 a,b",
            "protoSetter: 42 false",
            "protoRO: 7 false",
            "ownSetter: 42",
            "ownRO: 5",
            "frozen: 1 true",
            "sealed: 2 true",
            "DONE",
        ],
    );
}

/// Prototype-level interception reached through `setPrototypeOf` / `Object.create`
/// and through a class prototype — the per-receiver flag cannot see these, so the
/// gate has to walk the chain.
#[test]
fn prototype_chain_setters_still_intercept_after_unrelated_freeze() {
    let stdout = compile_and_run(
        r#"
Object.freeze({ unrelated: 1 })

// setPrototypeOf onto a proto carrying a setter.
const protoA: any = {}
Object.defineProperty(protoA, "hooked", {
  set(v: any) { (this as any)._a = v + 1 },
  get() { return (this as any)._a },
  configurable: true,
})
const viaSet: any = {}
Object.setPrototypeOf(viaSet, protoA)
viaSet.hooked = 10
console.log("setProtoOf:", viaSet._a, Object.prototype.hasOwnProperty.call(viaSet, "hooked"))

// Object.create with the same proto.
const viaCreate: any = Object.create(protoA)
viaCreate.hooked = 20
console.log("objectCreate:", viaCreate._a, Object.prototype.hasOwnProperty.call(viaCreate, "hooked"))

// Class accessor on the prototype (vtable, not the address-keyed tables).
class Base {
  _v = 0
  set tracked(v: number) { this._v = v * 3 }
  get tracked(): number { return this._v }
}
const inst: any = new Base()
inst.tracked = 5
console.log("classSetter:", inst._v)

// Object.defineProperty on a class prototype intercepts instance writes.
class Other { z = 0 }
Object.defineProperty(Other.prototype, "onProto", {
  set(v: any) { (this as any)._z = v - 1 },
  get() { return (this as any)._z },
  configurable: true,
})
const o2: any = new Other()
o2.onProto = 8
console.log("classProtoDesc:", o2._z, Object.prototype.hasOwnProperty.call(o2, "onProto"))

// A plain data write on the same class instance must still be a plain write.
o2.plainField = 99
console.log("classPlainWrite:", o2.plainField)

console.log("DONE")
"#,
    );

    assert_lines(
        &stdout,
        &[
            "setProtoOf: 11 false",
            "objectCreate: 21 false",
            "classSetter: 15",
            "classProtoDesc: 7 false",
            "classPlainWrite: 99",
            "DONE",
        ],
    );
}

/// A descriptor installed on one object must not leak onto a different object —
/// the point of a per-object gate. Also pins that a freeze on object A does not
/// make writes to object B throw or silently drop.
#[test]
fn descriptors_do_not_leak_across_objects() {
    let stdout = compile_and_run(
        r#"
const a: any = { locked: 1 }
Object.defineProperty(a, "locked", { value: 1, writable: false, configurable: true })
Object.freeze(a)

// A different object with the SAME key name must be freely writable.
const b: any = {}
b.locked = 2
b.locked = 3
console.log("b:", b.locked, Object.keys(b).join(","))

// And a whole batch of fresh objects keeps taking plain data writes.
let total = 0
for (let i = 0; i < 2000; i++) {
  const o: any = {}
  o.locked = i
  o.other = i + 1
  total += o.locked + o.other
}
console.log("batch:", total)
console.log("a-still-frozen:", a.locked, Object.isFrozen(a))
console.log("DONE")
"#,
    );

    assert_lines(
        &stdout,
        &[
            "b: 3 locked",
            "batch: 4000000",
            "a-still-frozen: 1 true",
            "DONE",
        ],
    );
}

/// The perf half: the identical write loop must not get materially slower just
/// because an unrelated object was frozen. On `main` the freeze flipped the
/// global latch and pushed every write onto the slow `[[Set]]` walk.
///
/// Pinned as a RATIO of two runs of the same loop in the same process, so it is
/// insensitive to machine speed and CI contention (both halves are perturbed
/// together). The regression this guards against is unbounded — the slow path is
/// O(own-key-count) per write — so a generous ceiling still catches it.
#[test]
fn unrelated_freeze_does_not_slow_the_write_loop() {
    let stdout = compile_and_run(
        r#"
let sink = 0
function build(n: number): number {
  const t0 = Date.now()
  for (let i = 0; i < n; i++) {
    const o: any = {}
    o.a = i
    o.b = i + 1
    o.c = i + 2
    sink += o.c
  }
  return Date.now() - t0
}

const N = 200000
build(20000)                      // warm
const before = build(N)
Object.freeze({ unrelated: 1 })   // <- must NOT poison the fast path
const after = build(N)

// Guard against a zero-length window on a very fast machine.
const ratio = after / Math.max(before, 1)
console.log("before=" + before + " after=" + after + " ratio=" + ratio.toFixed(2))
console.log("REGRESSED:" + (before >= 4 && ratio > 2.0))
console.log("sink=" + (sink > 0))
"#,
    );

    assert_lines(&stdout, &["REGRESSED:false", "sink=true"]);
}
