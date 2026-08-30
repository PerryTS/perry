//! Characterization coverage for variable-shaped exports reached through a
//! namespace binding.  The export ABI is a zero-argument getter returning the
//! closure, not a function symbol with the closure's user-facing arity.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_variable_exports_use_their_getter_then_call_the_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "namespace-variable-export-abi",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-cjs"],
    "allow": { "compilePackages": ["mini-cjs"] }
  }
}"#,
    )
    .expect("write package manifest");

    std::fs::write(
        root.join("vars.ts"),
        r#"
export const make = (value: string) => "make:" + value;
export const alsoMake = (value: string) => "also:" + value;
export { alsoMake as aliasedMake };
export function declared(value: string) { return "declared:" + value; }
export default (value: string) => "default:" + value;
"#,
    )
    .expect("write vars");
    std::fs::write(
        root.join("barrel.ts"),
        r#"export * as Reexported from "./vars.js";"#,
    )
    .expect("write barrel");
    // The cycle must retain module-init ordering: evaluating the imported
    // closure is deferred until after both modules finish initialization.
    std::fs::write(
        root.join("cycle-a.ts"),
        r#"
import * as CycleB from "./cycle-b.js";
export const make = (value: string) => CycleB.prefix(value) + ":a";
export const ready = () => "ready";
"#,
    )
    .expect("write cycle-a");
    std::fs::write(
        root.join("cycle-b.ts"),
        r#"
import * as CycleA from "./cycle-a.js";
export const prefix = (value: string) => "b:" + value;
export const readReady = () => CycleA.ready();
"#,
    )
    .expect("write cycle-b");
    let cjs = root.join("node_modules/mini-cjs");
    std::fs::create_dir_all(&cjs).expect("create cjs package");
    std::fs::write(
        cjs.join("package.json"),
        r#"{ "name": "mini-cjs", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write cjs manifest");
    std::fs::write(
        cjs.join("index.js"),
        "var make = require('./make'); module.exports = { make: make };\n",
    )
    .expect("write cjs barrel");
    std::fs::write(
        cjs.join("make.js"),
        "module.exports = function (value) { return 'cjs:' + value; };\n",
    )
    .expect("write cjs function");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as API from "./vars.js";
import { Reexported } from "./barrel.js";
import * as CycleA from "./cycle-a.js";
import * as CycleB from "./cycle-b.js";
import * as CJS from "mini-cjs";

console.log(API.make("one"));
console.log(API.aliasedMake("two"));
console.log(API.declared("three"));
console.log(API.default("four"));
console.log(Reexported.make("five"));
console.log(CycleA.make("six"));
console.log(CycleB.readReady());
console.log(CJS.make("seven"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "make:one\nalso:two\ndeclared:three\ndefault:four\nmake:five\nb:six:a\nready\ncjs:seven\n"
    );
}

/// A variable export in one namespace must not make an equal-named declared
/// function in another namespace use the variable getter ABI. Effect's
/// `Array.ts` imports many namespace modules that export `make`; before this
/// regression was fixed, `Reducer.make` evaluated to the object returned by an
/// accidental zero-argument invocation rather than to the function itself.
#[test]
fn namespace_variable_classification_is_scoped_to_the_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("declared.ts"),
        r#"
export function make(value: string) {
  return "declared:" + value;
}
"#,
    )
    .expect("write declared module");
    std::fs::write(
        root.join("variable.ts"),
        r#"
export const make = (value: string) => "variable:" + value;
"#,
    )
    .expect("write variable module");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as Declared from "./declared.ts";
import * as Variable from "./variable.ts";
import * as declared from "./declared.ts";
import * as variable from "./variable.ts";

console.log("upper-declared-type:", typeof Declared.make);
console.log("upper-declared-call:", Declared.make("one"));
console.log("upper-variable-type:", typeof Variable.make);
console.log("upper-variable-call:", Variable.make("two"));
console.log("lower-declared-type:", typeof declared.make);
console.log("lower-declared-call:", declared.make("three"));
console.log("lower-variable-type:", typeof variable.make);
console.log("lower-variable-call:", variable.make("four"));
"#,
    )
    .expect("write entry");

    let output = root.join("collision_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "upper-declared-type: function\n",
            "upper-declared-call: declared:one\n",
            "upper-variable-type: function\n",
            "upper-variable-call: variable:two\n",
            "lower-declared-type: function\n",
            "lower-declared-call: declared:three\n",
            "lower-variable-type: function\n",
            "lower-variable-call: variable:four\n",
        )
    );
}

/// A class in one namespace must not make an equal-named value in another
/// namespace lower as that class. Effect exercises this with the
/// `SchemaAST.Boolean` class and the `Schema.Boolean` schema object.
#[test]
fn namespace_class_classification_is_scoped_to_the_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("classes.ts"),
        r#"
export class Token {}
"#,
    )
    .expect("write class module");
    std::fs::write(
        root.join("values.ts"),
        r#"
export const Token = { encoding: "value-token" };
"#,
    )
    .expect("write value module");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as Classes from "./classes.ts";
import * as Values from "./values.ts";

console.log("class-type:", typeof Classes.Token);
console.log("value-field:", Values.Token.encoding);
"#,
    )
    .expect("write entry");

    let output = root.join("class_collision_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!("class-type: function\n", "value-field: value-token\n",)
    );
}

/// A namespace binding may have the same name as a class exported by that
/// namespace. The binding itself must remain a namespace: `Sharding.layer`
/// reads the exported variable, rather than looking for a static `layer`
/// property on the exported `Sharding` class.
#[test]
fn namespace_binding_is_not_replaced_by_an_equal_named_exported_class() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("sharding.ts"),
        r#"
export class Sharding {}
export const layer: { pipe: (value: string) => string } = {
  pipe: (value: string) => "layer:" + value
};
"#,
    )
    .expect("write sharding module");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as Sharding from "./sharding.ts";

console.log("class-type:", typeof Sharding.Sharding);
console.log(Sharding.layer.pipe("ok"));
"#,
    )
    .expect("write entry");

    let output = root.join("namespace_class_name_collision_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "class-type: function\nlayer:ok\n"
    );
}

/// Function ABI metadata is namespace-local too. A rest export in one
/// namespace must not make an equal-named ordinary function in another bundle
/// its arguments into a synthetic rest array. Effect exercises this with
/// fast-check's `tuple(...arbs)` and `SchemaAST.tuple(elements, checks)`.
#[test]
fn namespace_function_abi_is_scoped_to_the_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("plain.ts"),
        r#"
export function tuple(elements: Array<string>, checks: string | undefined = undefined) {
  return "plain:" + elements.length + ":" + checks;
}
"#,
    )
    .expect("write plain module");
    std::fs::write(
        root.join("rest.ts"),
        r#"
export function tuple(...values: Array<string>) {
  return "rest:" + values.length;
}
"#,
    )
    .expect("write rest module");
    std::fs::write(
        root.join("barrel.ts"),
        r#"
export * as Plain from "./plain.ts";
export * as Rest from "./rest.ts";
"#,
    )
    .expect("write barrel module");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as Plain from "./plain.ts";
import * as Rest from "./rest.ts";
import { Plain as BarrelPlain, Rest as BarrelRest } from "./barrel.ts";

console.log(Plain.tuple(["a", "b"], "checked"));
console.log(Rest.tuple("a", "b", "c"));
console.log(BarrelPlain.tuple(["a", "b"], "checked"));
console.log(BarrelRest.tuple("a", "b", "c"));
"#,
    )
    .expect("write entry");

    let output = root.join("function_abi_collision_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "plain:2:checked\nrest:3\nplain:2:checked\nrest:3\n"
    );
}
