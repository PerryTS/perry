//! Regression test for #5009: `perry.define` of `process.env.<NAME>` must be
//! applied as a build-time substitution again.
//!
//! When `process` became a real runtime object (#4993), the issue author
//! suspected `process.env.NODE_ENV` had become a live property read that
//! bypassed the define. The actual root cause is broader: the define was only
//! ever consulted by the tree-shake-gated `env_fold` branch pruner, so a
//! `process.env.NODE_ENV` read produced a runtime env lookup (`undefined` when
//! unset) in every default build — and packages that branch on it (React /
//! react-reconciler / scheduler) selected their *development* builds.
//!
//! The fix folds a defined `process.env.<NAME>` read into its literal at the
//! single HIR lowering point that would otherwise emit `Expr::EnvGet`, so the
//! define is honored in every context and independent of tree-shaking. These
//! checks mirror the real failure: a `compilePackages` dependency selects its
//! build via `if (process.env.NODE_ENV === 'production')`, exactly like
//! react-reconciler's `index.js`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Build the react-reconciler-shaped fixture in `root`: a `compilePackages`
/// dependency whose entry picks a production/development sub-module based on
/// `process.env.NODE_ENV`, plus a user entry that prints the selected build
/// and the raw `process.env.NODE_ENV` value. `package_json` lets each test
/// supply its own `perry` config (with or without the define).
fn write_fixture(root: &std::path::Path, package_json: &str) {
    std::fs::write(root.join("package.json"), package_json).expect("write consumer package.json");

    let pkg = root.join("node_modules").join("fakelib");
    std::fs::create_dir_all(pkg.join("cjs")).expect("mkdir fakelib/cjs");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "fakelib", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write fakelib package.json");

    // The react-reconciler `index.js` shape verbatim: a top-level `if` that
    // selects the production or development build off `process.env.NODE_ENV`.
    std::fs::write(
        pkg.join("index.js"),
        r#""use strict";
if (process.env.NODE_ENV === 'production') {
  module.exports = require('./cjs/fakelib.production.js');
} else {
  module.exports = require('./cjs/fakelib.development.js');
}
"#,
    )
    .expect("write fakelib index.js");
    std::fs::write(
        pkg.join("cjs").join("fakelib.production.js"),
        "\"use strict\";\nmodule.exports = { which: 'production' };\n",
    )
    .expect("write production build");
    std::fs::write(
        pkg.join("cjs").join("fakelib.development.js"),
        "\"use strict\";\nmodule.exports = { which: 'development' };\n",
    )
    .expect("write development build");

    std::fs::write(
        root.join("main.ts"),
        r#"
import lib from 'fakelib';
console.log("NODE_ENV=" + process.env.NODE_ENV);
console.log("which=" + (lib as any).which);
"#,
    )
    .expect("write entry");
}

fn compile(root: &std::path::Path) -> PathBuf {
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    output
}

fn run(bin: &std::path::Path, env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let run = cmd.output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn define_node_env_selects_production_build_and_wins_over_runtime_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(
        root,
        r#"{
  "name": "define-process-env",
  "type": "module",
  "perry": {
    "compilePackages": ["fakelib"],
    "allow": { "compilePackages": ["fakelib"] },
    "define": { "process.env.NODE_ENV": "production" }
  }
}"#,
    );
    let bin = compile(root);

    // With the define and NO runtime env var set, the define must apply:
    // the dependency selects its production build and the user-source read
    // folds to the literal "production".
    assert_eq!(
        run(&bin, &[]),
        "NODE_ENV=production\nwhich=production\n",
        "perry.define of process.env.NODE_ENV must be applied (compile-time)"
    );

    // esbuild-style `define` semantics: the define is authoritative and wins
    // over the runtime environment, so a conflicting NODE_ENV is ignored.
    assert_eq!(
        run(&bin, &[("NODE_ENV", "development")]),
        "NODE_ENV=production\nwhich=production\n",
        "the define must win over a conflicting runtime NODE_ENV"
    );
}

#[test]
fn no_define_still_reads_runtime_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(
        root,
        r#"{
  "name": "no-define-process-env",
  "type": "module",
  "perry": {
    "compilePackages": ["fakelib"],
    "allow": { "compilePackages": ["fakelib"] }
  }
}"#,
    );
    let bin = compile(root);

    // No define ⇒ the `process.env.NODE_ENV` reads remain live runtime
    // lookups (no tree-shake `env_fold` here either). With the OS env var
    // unset they read `undefined`, so the dependency's runtime `if` falls to
    // its development branch.
    assert_eq!(
        run(&bin, &[]),
        "NODE_ENV=undefined\nwhich=development\n",
        "without a define, an unset NODE_ENV must read undefined at runtime"
    );
    // With the OS env var set, both the user read and the dependency's
    // runtime `if` observe it — so production is selected, proving the read
    // is a genuine runtime lookup (not a stale compile-time fold).
    assert_eq!(
        run(&bin, &[("NODE_ENV", "production")]),
        "NODE_ENV=production\nwhich=production\n",
        "without a define, the runtime NODE_ENV must flow through"
    );
}
