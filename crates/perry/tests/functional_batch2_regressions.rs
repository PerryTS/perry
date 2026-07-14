//! Regression tests for the functional-correctness batch-2 fixes.
//!
//! Each fix was localized from a real npm package in the differential
//! functional corpus and reduced to a minimal multi-module / single-module
//! fixture here so the behavior is pinned without depending on the corpus.
//!
//! Fixes covered:
//!   1. Cross-module constructor with a `...rest` param dropped all but the
//!      first argument (mime: `new Mime(standardTypes, otherTypes)`).
//!   2. `Object.freeze` on a Map/Set corrupted its backing so later
//!      `get`/`values` over object-valued entries faulted (mime `_freeze`).
//!   3. Optional-call `obj.method?.(args)` on a string builtin short-circuited
//!      to `undefined` (mime `type?.split?.(';')`).
//!   4. `new ImportedClass()` discarded an ECMAScript constructor
//!      return-override (chalk `class Chalk { constructor(){ return factory; } }`).
//!   5. `instanceof` with a namespace/import member RHS (`x instanceof ns.C`)
//!      always returned false (semver's SemVer-clone guard).

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `files` (relative path -> contents) into `dir`, compile `entry`
/// with `--no-cache`, run it, and return stdout. Asserts compile + run succeed.
fn compile_and_run(dir: &Path, files: &[(&str, &str)], entry: &str) -> String {
    for (rel, contents) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, contents).expect("write fixture");
    }
    let entry_path = dir.join(entry);
    let output = dir.join("main_bin");

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

#[test]
fn cross_module_constructor_rest_param_keeps_all_args() {
    // Pre-fix: `new C("x","y","z")` on an imported class with a `...args`
    // ctor only captured "x" (rest slot got the first arg raw). Node: all 3.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[
            (
                "cls.ts",
                r#"export class C { constructor(...args: any[]){ (this as any).n = args.length; (this as any).a = args; } }"#,
            ),
            (
                "main.ts",
                r#"import { C } from "./cls.ts";
const c: any = new C("x", "y", "z");
console.log(c.n, JSON.stringify(c.a));"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "3 [\"x\",\"y\",\"z\"]\n");
}

#[test]
fn object_freeze_on_map_of_sets_preserves_entries() {
    // Pre-fix: Object.freeze(map) ran the keys-array walk over the Map's
    // backing and corrupted it; a later get/values then SIGSEGV'd.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[(
            "main.ts",
            r#"const m = new Map<string, Set<string>>();
m.set("a", new Set(["x", "y"]));
m.set("b", new Set(["z"]));
Object.freeze(m);
let total = 0;
for (const v of m.values()) { total += v.size; Object.freeze(v); }
console.log("get a:", (m.get("a") as Set<string>).size);
console.log("total:", total);"#,
        )],
        "main.ts",
    );
    assert_eq!(stdout, "get a: 2\ntotal: 3\n");
}

#[test]
fn optional_call_on_string_builtin_invokes_method() {
    // Pre-fix: `s.split?.(...)` / `s?.split?.(...)` returned undefined because
    // the function-value guard saw `string.split` read back as undefined.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[(
            "main.ts",
            r#"const s = "a/b";
console.log("A:", JSON.stringify(s.split?.("/")));
console.log("B:", JSON.stringify(s?.split?.("/")));
const t: string | undefined = "x;y";
console.log("C:", JSON.stringify(t?.split?.(";")[0]));
// A user object missing the method still short-circuits.
const o: any = {};
console.log("D:", o.missing?.());"#,
        )],
        "main.ts",
    );
    assert_eq!(
        stdout,
        "A: [\"a\",\"b\"]\nB: [\"a\",\"b\"]\nC: \"x\"\nD: undefined\n"
    );
}

#[test]
fn imported_class_constructor_return_override_honored() {
    // Pre-fix: `new ImportedClass()` whose ctor `return <function>` yielded the
    // empty allocated instance instead of the returned function.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[
            (
                "factory.ts",
                r#"function make(tag: string) { return (s: string) => tag + ":" + s; }
export class Wrapper {
  constructor(tag: string) {
    // eslint-disable-next-line no-constructor-return
    return make(tag) as any;
  }
}"#,
            ),
            (
                "main.ts",
                r#"import { Wrapper } from "./factory.ts";
const w: any = new Wrapper("hi");
console.log("typeof:", typeof w);
console.log("call:", w("there"));"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "typeof: function\ncall: hi:there\n");
}

#[test]
fn instanceof_namespace_member_rhs() {
    // Pre-fix: `x instanceof ns.Class` (member RHS over a default/namespace
    // import) always returned false (only native modules took the dynamic path).
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[
            (
                "ns.ts",
                r#"export class Thing { constructor(public v: number) {} }"#,
            ),
            (
                "main.ts",
                r#"import * as ns from "./ns.ts";
const a = new ns.Thing(5);
console.log("member:", a instanceof ns.Thing);
const C = ns.Thing;
console.log("local:", a instanceof C);
console.log("neg:", ({} as any) instanceof ns.Thing);"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "member: true\nlocal: true\nneg: false\n");
}

/// Characterizes the complete metadata surface assembled for an imported
/// class. The class travels through a renamed re-export and named alias, so its
/// source class id must remain the defining module's id rather than a fresh
/// importer-local id.
#[test]
fn imported_class_metadata_survives_named_alias_reexport() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[
            ("base.ts", r#"export class Parent { base = "parent"; }"#),
            (
                "model.ts",
                r#"import { Parent } from "./base.ts";
const computedName = "not-a-static-global";
export class Child extends Parent {
  next: Parent = new Parent();
  private saved = "";
  static plain = "static";
  static [computedName] = "computed";
  constructor(...parts: string[]) { super(); this.saved = parts.join("|"); }
  get value() { return this.saved; }
  set value(value: string) { this.saved = "set:" + value; }
  describe(first: string, ...tail: string[]) { return this.value + ":" + first + ":" + tail.length; }
}"#,
            ),
            (
                "barrel.ts",
                r#"export { Child as PublicChild } from "./model.ts";"#,
            ),
            (
                "main.ts",
                r#"import { PublicChild as LocalChild } from "./barrel.ts";
const value: any = new LocalChild("one", "two");
value.value = "changed";
console.log("method:", value.describe("head", "tail-a", "tail-b"));
console.log("parent:", value.next.base);
console.log("static:", LocalChild.plain);
console.log("computed:", LocalChild["not-a-static-global"]);
console.log("named:", value instanceof LocalChild);"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(
        stdout,
        "method: set:changed:head:2\nparent: parent\nstatic: static\ncomputed: computed\nnamed: true\n"
    );
}

/// A renamed re-export must retain its visible export name when consumed only
/// through a namespace. Keeping this entry point separate prevents named-import
/// metadata from masking a missing namespace registration.
#[test]
fn imported_class_namespace_reexport_uses_visible_alias() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[
            (
                "model.ts",
                r#"export class Child { constructor(public value: string) {} }"#,
            ),
            (
                "barrel.ts",
                r#"export { Child as PublicChild } from "./model.ts";"#,
            ),
            (
                "main.ts",
                r#"import * as barrel from "./barrel.ts";
const value: any = new barrel.PublicChild("namespace");
console.log("value:", value.value);
console.log("instanceof:", value instanceof barrel.PublicChild);"#,
            ),
        ],
        "main.ts",
    );
    assert_eq!(stdout, "value: namespace\ninstanceof: true\n");
}
