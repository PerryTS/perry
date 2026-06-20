//! Regression test for the #5024 follow-up: methods added to a class prototype
//! by plain ASSIGNMENT (`Class.prototype.m = fn`) must be enumerable own
//! properties of the reflective `Class.prototype` object, so `for…in` /
//! `Object.keys` / `in` / `hasOwnProperty` see them — while methods added via
//! `Object.defineProperty(Class.prototype, m, { value })` WITHOUT an explicit
//! `enumerable: true` stay non-enumerable (the spec default).
//!
//! This is the root cause of the claude-code `--help` wall: zod's trait factory
//! `b1` builds a constructor whose instances inherit base methods, then copies
//! them onto each instance with
//! ```js
//! for (let H in O.prototype)
//!   if (!(H in w)) Object.defineProperty(w, H, { value: O.prototype[H].bind(w) });
//! ```
//! Assignment-registered prototype methods (e.g. `ZodType`'s `optional`) were
//! dispatchable via the per-class side table but INVISIBLE to `for…in` on the
//! reflective prototype object, because the side-table mirror (#5024) targeted
//! the synthetic `CLASS_PROTOTYPE_OBJECTS` cache, not the
//! `CLASS_DECL_PROTOTYPE_OBJECTS` object that reflective enumeration reads. The
//! `for…in` enumerated nothing, no methods were copied onto the instance, and
//! `z.number().optional()` threw `TypeError: Cannot read properties of
//! undefined (reading 'optional')`.

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

#[test]
fn assignment_registered_prototype_methods_are_enumerable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// Base/derived classes with a method added by assignment on each prototype —
// the shape zod's `b1` trait factory produces (`class A extends Y` + proto
// method assignment).
class Base { }
(Base as any).prototype.optional = function () { return "OPT"; };
class Derived extends Base { }
(Derived as any).prototype.int = function () { return "INT"; };

// `for…in` over the prototype must enumerate own + inherited assignment-added
// methods (Node: ["int","optional"]).
const forin: string[] = [];
for (const k in (Derived as any).prototype) forin.push(k);
console.log("forin:", forin.sort().join(","));

// And over a real instance.
const inst: any = new Derived();
const instKeys: string[] = [];
for (const k in inst) instKeys.push(k);
console.log("inst:", instKeys.sort().join(","));

// Object.keys / `in` / hasOwnProperty agree.
console.log("keys:", JSON.stringify(Object.keys((Derived as any).prototype).sort()));
console.log("in:", ("int" in (Derived as any).prototype) && ("optional" in (Derived as any).prototype));
console.log("hasOwn:", (Derived as any).prototype.hasOwnProperty("int"));
"#,
    );
    assert_eq!(
        stdout,
        "forin: int,optional\ninst: int,optional\nkeys: [\"int\"]\nin: true\nhasOwn: true\n",
        "assignment-registered prototype methods must enumerate; \
         `keys` shows only Derived's own `int` (inherited `optional` is not an \
         own key of Derived.prototype, matching Node)"
    );
}

#[test]
fn define_property_without_enumerable_stays_non_enumerable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class C { }
// defineProperty data descriptor WITHOUT enumerable:true -> non-enumerable.
Object.defineProperty((C as any).prototype, "m1", { value: () => 1 });
// plain assignment -> enumerable.
(C as any).prototype.m2 = () => 2;
// defineProperty WITH enumerable:true -> enumerable.
Object.defineProperty((C as any).prototype, "m3", { value: () => 3, enumerable: true });

const forin: string[] = [];
for (const k in (C as any).prototype) forin.push(k);
console.log("forin:", forin.sort().join(","));          // m2,m3
console.log("keys:", JSON.stringify(Object.keys((C as any).prototype).sort()));
console.log("m1enum:", Object.getOwnPropertyDescriptor((C as any).prototype, "m1")?.enumerable);
console.log("m2enum:", Object.getOwnPropertyDescriptor((C as any).prototype, "m2")?.enumerable);
console.log("m3enum:", Object.getOwnPropertyDescriptor((C as any).prototype, "m3")?.enumerable);
// All three are still readable / dispatchable regardless of enumerability.
console.log("vals:", (C as any).prototype.m1() + (C as any).prototype.m2() + (C as any).prototype.m3());
"#,
    );
    assert_eq!(
        stdout,
        "forin: m2,m3\nkeys: [\"m2\",\"m3\"]\nm1enum: false\nm2enum: true\nm3enum: true\nvals: 6\n"
    );
}
