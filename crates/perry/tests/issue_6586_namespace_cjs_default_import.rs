//! Regression test for #6586: a namespace import of a CommonJS module whose
//! `module.exports` value is itself the export
//! (`module.exports = function equal(){}`) — TypeScript's
//! `esModuleInterop=false` interop shape — must bind the namespace local to the
//! default export so a DIRECT call of it (`equal(a, b)`) links.
//!
//! This is the wall that blocks `fast-json-stringify` (via `ajv`): ajv's
//! `lib/compile/resolve.ts` does
//!
//! ```ts
//! import * as equal from "fast-deep-equal"
//! import * as traverse from "json-schema-traverse"
//! // ...
//! traverse(schema, {allKeys: true}, (sch) => { /* ... */ })
//! if (!equal(sch1, sch2)) throw ambiguos(ref)
//! ```
//!
//! where both deps are pure CJS `module.exports = function`. Pre-fix, the
//! namespace binding had no `import_function_prefixes` entry, so the direct
//! calls fell through to bare `equal` / `traverse` externs and the link died
//! with `Undefined symbols: "_equal", "_traverse"`. A DEFAULT import of the
//! same module (`import equal from "..."`) always linked — the fix routes the
//! namespace whole-value binding to the module's `default` symbol the same way.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_import_of_cjs_default_function_links_and_calls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "ns-cjs-default",
  "private": true,
  "perry": {
    "compilePackages": ["fakeequal", "faketraverse", "fakecls"],
    "allow": { "compilePackages": ["fakeequal", "faketraverse", "fakecls"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    // fast-deep-equal shape: a bare `module.exports = function`.
    let equal_pkg = root.join("node_modules").join("fakeequal");
    std::fs::create_dir_all(&equal_pkg).expect("mkdir fakeequal");
    std::fs::write(
        equal_pkg.join("package.json"),
        r#"{ "name": "fakeequal", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write fakeequal package.json");
    std::fs::write(
        equal_pkg.join("index.js"),
        "'use strict';\nmodule.exports = function equal(a, b) { return a === b; };\n",
    )
    .expect("write fakeequal index.js");

    // json-schema-traverse shape: `module.exports = fn` PLUS a
    // `module.exports.default = fn` self-reference (a named `default` on top of
    // the CJS default value).
    let traverse_pkg = root.join("node_modules").join("faketraverse");
    std::fs::create_dir_all(&traverse_pkg).expect("mkdir faketraverse");
    std::fs::write(
        traverse_pkg.join("package.json"),
        r#"{ "name": "faketraverse", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write faketraverse package.json");
    std::fs::write(
        traverse_pkg.join("index.js"),
        "'use strict';\nfunction traverse(schema, cb) { cb(schema); return schema.n * 2; }\nmodule.exports = traverse;\nmodule.exports.default = traverse;\n",
    )
    .expect("write faketraverse index.js");

    // A CJS default that is a CLASS (`module.exports = class Foo {}`) —
    // exercises the class/metadata propagation for the namespace binding so
    // `new ns(...)` resolves an ImportedClass entry instead of a phantom
    // `perry_fn_<mod>__default` function wrapper.
    let cls_pkg = root.join("node_modules").join("fakecls");
    std::fs::create_dir_all(&cls_pkg).expect("mkdir fakecls");
    std::fs::write(
        cls_pkg.join("package.json"),
        r#"{ "name": "fakecls", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write fakecls package.json");
    std::fs::write(
        cls_pkg.join("index.js"),
        "'use strict';\nmodule.exports = class Box { constructor(x) { this.x = x; } doubled() { return this.x * 2; } };\n",
    )
    .expect("write fakecls index.js");

    // Consumer imports both as namespaces and CALLS them directly — the exact
    // ajv `resolve.ts` shape.
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import * as equal from "fakeequal";
import * as traverse from "faketraverse";
import * as Box from "fakecls";

const eq: boolean = (equal as any)({ a: 1 }, { a: 1 } as any) === false;
console.log("equal:", (equal as any)(1, 1), (equal as any)(1, 2));

let seen = 0;
const doubled: number = (traverse as any)({ n: 21 }, (_s: any) => { seen++; });
console.log("traverse:", doubled, "seen:", seen);
console.log("eq_ref:", eq);

// `new` through a namespace binding whose CJS default is a class.
const b: any = new (Box as any)(21);
console.log("box:", b.doubled());
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (namespace-import-of-CJS-default link wall regressed?)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, "equal: true false\ntraverse: 42 seen: 1\neq_ref: true\nbox: 42\n",
        "namespace import of a CJS default function/class must resolve the default export"
    );
}
