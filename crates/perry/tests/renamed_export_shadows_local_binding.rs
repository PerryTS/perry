//! Regression: a renamed value export must be backed by its local binding even
//! when an unrelated local already owns the public export name.
//!
//! Effect's bundled `Effectable.js` declares a non-callable `Prototype` object
//! and later exports its callable `Prototype2` binding as `Prototype`. The old
//! module-global pass treated both names in `exported_objects` as local storage
//! owners, so the unrelated object claimed the public getter first and callers
//! failed with `TypeError: value is not a function`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn renamed_value_export_ignores_same_named_unexported_local() {
    let dir = tempfile::tempdir().expect("tempdir");
    let producer = dir.path().join("producer.mjs");
    let entry = dir.path().join("main.mjs");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &producer,
        r#"
const Prototype = { wrong: true };
const Prototype2 = (options) => ({ ...options, ok: true });
function Record(value) { return () => value; }
function Record3(value) { return { value, pipe: true }; }
function resolveAt(key) { return (value) => value[key]; }
const resolveAt2 = resolveAt;
export {
  Prototype as Other,
  Prototype2 as Prototype,
  Record as OtherRecord,
  Record3 as Record,
  resolveAt2 as resolveAt
};
"#,
    )
    .expect("write producer");
    std::fs::write(
        &entry,
        r#"
import { Prototype, Record, resolveAt } from "./producer.mjs";
const result = Prototype({ label: "right" });
const record = Record("record3");
const getAnswer = resolveAt("answer");
console.log(result.label, result.ok, record.value, record.pipe, getAnswer({ answer: 42 }));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "right true record3 true 42"
    );
}

#[test]
fn renamed_closure_export_ignores_same_named_private_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dep = dir.path().join("dep.ts");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main.exe");

    std::fs::write(
        &dep,
        r#"
        function optionalKey(ast: string): string {
            return "private:" + ast;
        }
        const optionalKey2 = (schema: string): string => "public:" + schema;
        export { optionalKey2 as optionalKey };
        "#,
    )
    .expect("write dependency");
    std::fs::write(
        &entry,
        r#"
        import { optionalKey } from "./dep.ts";
        console.log(optionalKey("schema"));
        "#,
    )
    .expect("write entry");

    let compile = std::process::Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = std::process::Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(run.status.success(), "run failed: {run:?}");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "public:schema\n");
}

#[test]
fn class_ref_set_prototype_supplies_inherited_static_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main.exe");

    std::fs::write(
        &entry,
        r#"
        const schema = { ast: { tag: "Objects", sentinel: "InvalidRequest" } };
        function makeClass() {
            class Generated {}
            return Object.setPrototypeOf(Generated, schema);
        }
        class InvalidRequestReason extends makeClass() {}
        console.log(InvalidRequestReason.ast.tag, InvalidRequestReason.ast.sentinel);
        const Generated = Object.getPrototypeOf(InvalidRequestReason);
        console.log(Object.getPrototypeOf(Generated) === schema);
        "#,
    )
    .expect("write entry");

    let compile = std::process::Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = std::process::Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "Objects InvalidRequest\ntrue\n"
    );
}

#[test]
fn dynamically_created_parent_supplies_inherited_static_symbol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main.exe");

    std::fs::write(
        &entry,
        r#"
        const TypeId = Symbol.for("schema-type");
        function makeClass() {
            return class Generated {
                static [TypeId] = TypeId;
            };
        }
        class SchemaClass extends makeClass() {}
        console.log(TypeId in SchemaClass, SchemaClass[TypeId] === TypeId);
        console.log(Object.hasOwn(SchemaClass, TypeId));
        "#,
    )
    .expect("write entry");

    let compile = std::process::Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = std::process::Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true true\nfalse\n");
}

#[test]
fn capture_carrying_parent_supplies_computed_string_static_presence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main.exe");

    std::fs::write(
        &entry,
        r#"
        const TypeId = "~effect/Schema/Schema";
        class Root {}
        function makeClass(Inherited: typeof Root, identifier: string) {
            const captured = identifier;
            const out = class extends Inherited {
                static [TypeId] = TypeId;
                static get identifier() { return captured; }
            };
            return out;
        }
        class SchemaClass extends makeClass(Root, "SchemaClass") {}
        const parent = Object.getPrototypeOf(SchemaClass);
        console.log(TypeId in parent, parent[TypeId] === TypeId, parent.identifier);
        console.log(TypeId in SchemaClass, SchemaClass[TypeId] === TypeId);
        "#,
    )
    .expect("write entry");

    let compile = std::process::Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = std::process::Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true true SchemaClass\ntrue true\n"
    );
}
