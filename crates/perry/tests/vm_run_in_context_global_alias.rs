//! Regression test (#5437, Next.js dynamic-page cluster): `vm.runInNewContext`
//! / `vm.runInContext` must run the supplied code with the context object AS
//! the sandbox global, so that `globalThis.X = …`, bare `var Y = …`, and
//! top-level `this.Z = …` writes land on the context object — and bare reads
//! inside the code resolve against it.
//!
//! The original gap was not the global aliasing (that already worked for simple
//! assignments) but the toy VM interpreter's inability to evaluate the *shapes*
//! Next.js feeds it: the RSC client-reference manifest is two statements —
//! `globalThis.__RSC_MANIFEST = globalThis.__RSC_MANIFEST || {};` and
//! `globalThis.__RSC_MANIFEST["/page"] = { …JSON.stringify payload… };`. That
//! exercises the `||` operator, an object/array literal right-hand side, and a
//! computed-member (`obj["key"]`) assignment target — none of which the
//! interpreter handled, so the manifest write was silently dropped and Next's
//! `evalManifest` read back `undefined`, tripping the manifests-singleton
//! invariant and 500ing `/posts/[id]` + `/fetcher`.

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
        .env("PERRY_ALLOW_PERRY_FEATURES", "1")
        .env("PERRY_ALLOW_EVAL", "1")
        .env("PERRY_ALLOW_UNIMPLEMENTED", "1")
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
fn run_in_new_context_aliases_context_object_as_global() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const vm = require("vm");

// Case 1: the context object is the sandbox global — globalThis/var/this all
// land on it. (node: 42 7 9)
const ctx1: any = {};
vm.runInNewContext("globalThis.X = 42; var Y = 7; this.Z = 9", ctx1);
if (ctx1.X !== 42 || ctx1.Y !== 7 || ctx1.Z !== 9) {
  throw new Error("case1: " + ctx1.X + " " + ctx1.Y + " " + ctx1.Z);
}
console.log("case1-ok");

// Case 2: createContext + runInContext share the same context object.
const ctx2: any = {};
vm.createContext(ctx2);
vm.runInContext("globalThis.X = 1", ctx2);
if (ctx2.X !== 1) throw new Error("case2: " + ctx2.X);
console.log("case2-ok");

// Case 3: bare reads inside the code resolve against the context object.
const ctx3: any = { pre: 5 };
vm.runInNewContext("globalThis.out = pre + 1", ctx3);
if (ctx3.out !== 6) throw new Error("case3: " + ctx3.out);
console.log("case3-ok");

// Case 4: the exact Next.js RSC-manifest shape — `|| {}`, an object-literal
// right-hand side, and a computed-member assignment target.
const ctx4: any = {};
vm.runInNewContext(
  'globalThis.__RSC_MANIFEST = globalThis.__RSC_MANIFEST || {};' +
    'globalThis.__RSC_MANIFEST["/fetcher/page"] = {"moduleLoading":{"prefix":"","crossOrigin":null},"clientModules":{}};',
  ctx4,
);
const entry = ctx4.__RSC_MANIFEST && ctx4.__RSC_MANIFEST["/fetcher/page"];
if (!entry) throw new Error("case4: manifest entry missing");
if (entry.moduleLoading.prefix !== "") throw new Error("case4: prefix " + entry.moduleLoading.prefix);
if (entry.moduleLoading.crossOrigin !== null) throw new Error("case4: crossOrigin not null");
if (typeof entry.clientModules !== "object") throw new Error("case4: clientModules shape");
console.log("case4-ok");

// Case 5: plain JS object literal with an unquoted key (not strict JSON) still
// builds, and computed-member read-back works.
const ctx5: any = {};
vm.runInNewContext('globalThis.M = {}; globalThis.M["x"] = {a: 1};', ctx5);
if (!ctx5.M || ctx5.M.x.a !== 1) throw new Error("case5: " + JSON.stringify(ctx5.M));
console.log("case5-ok");

console.log("ok");
"#,
    );
    assert_eq!(
        stdout, "case1-ok\ncase2-ok\ncase3-ok\ncase4-ok\ncase5-ok\nok\n",
        "vm.runIn*Context must alias the context object as the sandbox global and evaluate manifest-shaped code"
    );
}
