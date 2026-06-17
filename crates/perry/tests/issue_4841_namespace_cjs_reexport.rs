//! Regression test for #4841: `import * as ns from "<cjs-pkg>"` where a
//! namespace member is a re-export of a CommonJS submodule's `default`
//! (`var sfy = require('./sfy'); module.exports = { sfy }`, and `./sfy` is
//! `module.exports = function (...) {...}`).
//!
//! The submodule records its export under the synthetic `"default"` suffix —
//! not the consumer-visible member name `sfy`. The namespace-import
//! var-vs-function classifier keyed only on `(origin_path, "sfy")`, missed the
//! `(origin_path, "default")` entry, and so classified `sfy` as a FUNCTION.
//! That routed `ns.sfy` through the singleton-closure wrap of the default
//! getter, so `ns.sfy(args)` RETURNED the function value instead of being it
//! — `typeof ns.sfy(...) === "function"`, length 0.
//!
//! This was the real root cause of the Stripe SDK failure: Stripe's
//! `utils.js` does `import * as qs from 'qs'` and `qs.stringify(...).replace(...)`.
//! `qs.stringify(...)` returned the qs stringify *function* (not a string), so
//! `.replace` was undefined → `TypeError: replace is not a function`.
//!
//! Fix (crates/perry/src/commands/compile.rs): the namespace arm now probes
//! both `(origin_path, member)` and `(origin_path, origin_name)` against
//! `exported_var_names`, mirroring the named-import arm. With the member
//! correctly classified as a var, `ns.sfy` reads the getter's value.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_import_of_cjs_default_reexport_resolves_member_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Consumer package.json: native-compile the `mini` package.
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "ns-cjs-reexport",
  "type": "module",
  "perry": {
    "compilePackages": ["mini"],
    "allow": { "compilePackages": ["mini"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    // node_modules/mini — a CJS barrel that re-exports a function submodule
    // and an object submodule (mirrors qs/lib/index.js's shape).
    let mini = root.join("node_modules").join("mini");
    std::fs::create_dir_all(&mini).expect("mkdir mini");
    std::fs::write(
        mini.join("package.json"),
        r#"{ "name": "mini", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write mini package.json");
    std::fs::write(
        mini.join("index.js"),
        r#"'use strict';
var sfy = require('./sfy');
var helper = require('./helper');
module.exports = { sfy: sfy, helper: helper };
"#,
    )
    .expect("write index.js");
    // module.exports is a FUNCTION (the qs.stringify shape).
    std::fs::write(
        mini.join("sfy.js"),
        r#"'use strict';
module.exports = function (object, opts) { return "S:" + object; };
"#,
    )
    .expect("write sfy.js");
    // module.exports is an OBJECT.
    std::fs::write(
        mini.join("helper.js"),
        r#"'use strict';
module.exports = { ALPHA: 1, doit: function doit(x) { return "H:" + x; } };
"#,
    )
    .expect("write helper.js");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import * as m from "mini";
const ns: any = m;
// The function re-export must BE the function, not a wrapper that returns it.
console.log(typeof ns.sfy, ns.sfy.length, ns.sfy("X", {}));
// The object re-export must BE the object, not a function.
console.log(typeof ns.helper, JSON.stringify(ns.helper));
// Chained `.replace` on a string result — the exact Stripe failure shape.
const q: string = ns.sfy("ab");
console.log(q.replace(/:/g, "="));
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "function 2 S:X\nobject {\"ALPHA\":1}\nS=ab\n",
        "namespace member of a CJS default re-export must resolve to the \
         underlying value (function / object), not a wrapper closure"
    );
}
