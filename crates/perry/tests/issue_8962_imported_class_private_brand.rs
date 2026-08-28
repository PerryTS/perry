//! Regression for #8962: constructing a class IMPORTED from another module
//! threw `TypeError: Cannot initialize private elements twice on the same
//! object` when that class declared a private method or accessor.
//!
//! `import { Hono } from "hono"; new Hono()` was the report. The importing
//! module sees the class only as the metadata-only stub `compile_module`
//! builds — a name table with no bodies and no initializers — but the stub
//! copies private METHOD names verbatim (it needs them to resolve dispatch
//! symbols), and `Class::has_private_instance_brand` is defined purely over
//! `#`-prefixed member names. So the `new` site emitted `js_private_brand_add`
//! for a brand it does not own, on top of the one the DEFINING module's
//! standalone `<prefix>__<class>_constructor` emits — and installing a class's
//! brand twice on one object is the error PrivateMethodOrAccessorAdd requires.
//!
//! Every case here calls the private member after construction, so a fix that
//! merely dropped the second install without leaving the first one standing
//! would fail these too: the brand check inside the private-member access
//! throws when no brand is present.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `files` (relative path -> source) with `entry` as the entry point
/// and return the binary's stdout. Panics with the compiler's or the program's
/// output on any failure.
fn compile_and_run(files: &[(&str, &str)], entry: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for (name, source) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, source).expect("write source");
    }
    let entry_path = root.join(entry);
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry_path)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run binary");
    assert!(
        run.status.success(),
        "binary failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

/// The reduced `new Hono()`: a class with a private METHOD, declared in one
/// module and constructed in another. No inheritance is needed to trigger it.
#[test]
fn imported_class_with_private_method_constructs_once() {
    let stdout = compile_and_run(
        &[
            (
                "base.ts",
                r#"export class BaseX {
  #m(): number { return 41; }
  call(): number { return this.#m() + 1; }
}
"#,
            ),
            (
                "main.ts",
                r#"import { BaseX } from "./base";
console.log(new BaseX().call());
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "42");
}

/// A private ACCESSOR carries the same brand as a private method, and the stub
/// copies getter/setter names the same way.
#[test]
fn imported_class_with_private_getter_constructs_once() {
    let stdout = compile_and_run(
        &[
            (
                "base.ts",
                r#"export class BaseX {
  v = 41;
  get #g(): number { return this.v + 1; }
  call(): number { return this.#g; }
}
"#,
            ),
            (
                "main.ts",
                r#"import { BaseX } from "./base";
console.log(new BaseX().call());
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "42");
}

/// A private-method class reached as an ANCESTOR: the leaf is local, so the
/// brand came from the `AncestorsOnly` walk at the `new` site rather than from
/// the leaf's own entry. Both spellings of the subclass — with and without an
/// explicit constructor — take different paths through `lower_new`.
#[test]
fn local_subclass_of_imported_private_method_class() {
    let base = r#"export class BaseX {
  #m(): number { return 41; }
  call(): number { return this.#m() + 1; }
}
"#;
    let with_ctor = compile_and_run(
        &[
            ("base.ts", base),
            (
                "main.ts",
                r#"import { BaseX } from "./base";
class D extends BaseX { constructor() { super(); } }
console.log(new D().call());
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(with_ctor, "42");

    let without_ctor = compile_and_run(
        &[
            ("base.ts", base),
            (
                "main.ts",
                r#"import { BaseX } from "./base";
class D extends BaseX {}
console.log(new D().call());
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(without_ctor, "42");
}

/// hono's own shape: the base with the private members is in one module, the
/// subclass that `super()`s into it is an anonymous class expression in a
/// second, and the `new` is in a third. Every link in the chain is an imported
/// stub at the site that constructs it.
#[test]
fn imported_subclass_of_imported_private_method_class() {
    let stdout = compile_and_run(
        &[
            (
                "base.ts",
                r#"const notFound = (x: string): string => "nf:" + x;
export class BaseX {
  pub: number;
  #path = "/";
  #nf = notFound;
  constructor(options: any = {}) {
    this.pub = 1;
  }
  #addRoute(m: string): string { return m + this.#path; }
  route(m: string): string { return this.#addRoute(m) + this.#nf("!"); }
}
"#,
            ),
            (
                "mid.ts",
                r#"import { BaseX } from "./base";
export const DerivedX = class extends BaseX {
  constructor(options: any = {}) { super(options); }
};
"#,
            ),
            (
                "main.ts",
                r#"import { DerivedX } from "./mid";
const a = new DerivedX();
console.log(a.route("GET") + "|" + a.pub);
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "GET/nf:!|1");
}

/// The same class constructed INSIDE its defining module never had the bug —
/// there the `new` site owns the field-initializer phase and installs the
/// brand itself. Pin it, so a fix that suppressed the install unconditionally
/// (rather than only where another module already performs it) fails here.
#[test]
fn same_module_construction_still_installs_the_brand() {
    let stdout = compile_and_run(
        &[
            (
                "base.ts",
                r#"export class BaseX {
  #m(): number { return 41; }
  call(): number { return this.#m() + 1; }
}
export function make(): BaseX { return new BaseX(); }
"#,
            ),
            (
                "main.ts",
                r#"import { make } from "./base";
console.log(make().call());
"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "42");
}

/// Double initialization must still be observable where the spec requires it:
/// a base constructor that returns an object the derived class has already
/// branded. This is the case `js_private_brand_add`'s duplicate check exists
/// for, and #8962's fix must not silence it.
#[test]
fn genuine_double_initialization_still_throws() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"const recycled: any = {};
class Base {
  constructor() { return recycled; }
}
class Derived extends Base {
  #m(): number { return 1; }
  call(): number { return this.#m(); }
}
new Derived();
new Derived();
console.log("no throw");
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
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run binary");
    let stderr = String::from_utf8_lossy(&run.stderr);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        !run.status.success() && stderr.contains("private elements twice"),
        "expected the second construction to throw the duplicate-brand \
         TypeError\nstatus: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status.code()
    );
}
