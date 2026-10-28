//! Literal builtin requires use hoisted ordinary ESM default bindings.
//! Mutable/reassigned aliases and destructuring retain their runtime locals,
//! while the synthetic require returns the imported canonical builtin value.
//! Computed specifiers continue through the createRequire-backed escape.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile a CJS entry (`.cjs` so the wrap is applied) and run it, returning
/// stdout. Asserts both the compile and the run succeed.
fn compile_and_run_cjs(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.cjs");
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

/// The simplest witness: a bare `const p = require("process")` in a CJS-wrapped
/// module. Pre-fix this threw `ReferenceError: p is not defined` when the body
/// read `p.platform`.
#[test]
fn cjs_wrap_bare_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const p = require("process");
const os = require("os");
const path = require("path");
console.log("platform:", p.platform);
console.log("cpus:", typeof os.cpus);
console.log("join:", typeof path.join, path.join("a", "b"));
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!(
            "platform: {expected_platform}\ncpus: function\njoin: function a{sep}b\n",
            sep = std::path::MAIN_SEPARATOR
        )
    );
}

/// The exact sdxgen/rolldown shape: `let node_process = require("process");
/// node_process = __toESM(node_process, 1)` — the alias is REASSIGNED, so the
/// wrap can't adopt it, and the HIR's destructuring pass stole it into a
/// native-module namespace binding (dropping the runtime local). Pre-fix:
/// `ReferenceError: node_process is not defined` on every invocation.
#[test]
fn cjs_wrap_rolldown_toesm_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
var __toESM = (mod, isNodeMode) => {
  if (mod && typeof mod === "object" && mod.__esModule) return mod;
  var target = {};
  Object.defineProperty(target, "default", { value: mod, enumerable: true });
  return Object.assign(target, mod);
};
let node_process = require("process");
node_process = __toESM(node_process, 1);
let node_os = require("os");
node_os = __toESM(node_os, 1);
console.log("node_process.platform:", node_process.platform);
console.log("node_os.cpus:", typeof node_os.cpus);
module.exports = { platform: node_process.platform };
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!("node_process.platform: {expected_platform}\nnode_os.cpus: function\n")
    );
}

/// Destructured built-in require: `const { platform } = require("process")`
/// in a CJS-wrapped module. The destructuring native-require fast path must
/// also respect the wrap's synthetic require, so `platform` binds from its
/// result and ordinary user-defined require bindings keep their semantics.
#[test]
fn cjs_wrap_destructured_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const { platform, arch } = require("process");
const { join } = require("path");
console.log("platform:", platform);
console.log("arch:", typeof arch);
console.log("join:", join("a", "b"));
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!(
            "platform: {expected_platform}\narch: string\njoin: a{sep}b\n",
            sep = std::path::MAIN_SEPARATOR
        )
    );
}

/// #8343 followup: `Object.create(require("process"))` stamps the result with
/// a synthetic class_id and — pre-fix — registered `NATIVE_MODULE_CLASS_ID`
/// (0xFFFFFFFE, the `-2` sentinel) as that synthetic class's parent via
/// `register_class`.  Later, `Object.getPrototypeOf` on the synthetic class
/// ref (returned by `instance.constructor`) walked the parent chain and
/// returned the raw sentinel as an INT32-tagged class ref (`-2`).
/// `Object.create(-2)` then threw
/// `TypeError: Object prototype may only be an Object or null: -2` — the
/// blocker for `sdxgen --help` (rolldown's `__toESM` calls
/// `Object.create(Object.getPrototypeOf(mod))`).
///
/// This witness creates the same `Object.create(builtin_namespace)` shape,
/// reads `.constructor` to obtain the synthetic class ref, and then runs the
/// `Object.create(Object.getPrototypeOf(ctor))` chain.  Pre-fix it threw the
/// `-2` TypeError; post-fix it succeeds.
#[test]
fn cjs_wrap_object_create_on_builtin_namespace_get_prototype_of_not_sentinel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const ns = require("process");
const obj = Object.create(ns);
const ctor = obj.constructor;
const proto = Object.getPrototypeOf(ctor);
const obj2 = Object.create(proto);
console.log("ok");
"#,
    );
    assert_eq!(stdout, "ok\n");
}

/// The full rolldown `__toESM` shape (the real one using
/// `Object.create(Object.getPrototypeOf(mod))`) must not throw the `-2`
/// TypeError when `mod` is a built-in module namespace AND a prior
/// `Object.create(builtin_namespace)` has registered the sentinel parent.
/// This mirrors the `external-pack.js` startup sequence: `__toESM` calls on
/// `require("process")` etc. interleave with `isPlainObject`/`deepMerge`
/// prototype-chain walks that hit `getPrototypeOf` on synthetic class refs.
#[test]
fn cjs_wrap_rolldown_toesm_after_object_create_on_builtin_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  isNodeMode || !mod || !mod.__esModule || !__hasOwnProp.call(mod, "default") ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
// First: Object.create on a built-in namespace registers the sentinel parent.
const ns = require("process");
const obj = Object.create(ns);
const ctor = obj.constructor;
// Route __toESM through the synthetic class ref (ctor) so
// Object.getPrototypeOf(ctor) takes the class-id-tagged branch in
// js_object_get_prototype_of — the path the sentinel-suppression fix
// changed. Without this the heap-pointer path (for node_os below) hides a
// regression.
__toESM(ctor, 1);
// Now run __toESM on another built-in — its Object.create(getPrototypeOf(mod))
// must not trip on the registered sentinel.
let node_os = require("os");
node_os = __toESM(node_os, 1);
console.log("os.cpus:", typeof node_os.cpus);
"#,
    );
    assert_eq!(stdout, "os.cpus: function\n");
}

/// Regression for the computed/dynamic `require` arm: a specifier held in a
/// variable (not a string literal) must route through the synthetic require's
/// `createRequire` arm for Node.js built-ins. Pre-fix the
/// `__perry_cjs_require_is_builtin` predicate was a hardcoded switch that
/// omitted 16 entries from the shared `NODE_BUILTIN_MODULES` table (`tls`,
/// `dgram`, `diagnostics_channel`, `domain`, `fs/promises`, `inspector`,
/// `repl`, `stream/web`, `v8`, `vm`, `wasi`, …), so `require(spec)` for one of
/// those fell through to compiled-module resolution and raised
/// `MODULE_NOT_FOUND`. `domain` is a stable, simple built-in that was missing.
#[test]
fn cjs_wrap_computed_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const spec = "domain";
const mod = require(spec);
console.log("typeof:", typeof mod);
"#,
    );
    assert_eq!(stdout, "typeof: object\n");
}

#[test]
fn cjs_wrap_callable_builtin_defaults_keep_their_commonjs_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
console.log(typeof require("assert"));
console.log(typeof require("assert/strict"));
console.log(typeof require("events"));
console.log(typeof require("stream"));
"#,
    );
    assert_eq!(stdout, "function\nfunction\nfunction\nfunction\n");
}

#[test]
fn cjs_wrap_destructured_fs_promises_is_available_without_another_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const { promises } = require("fs");
console.log(typeof promises.readFile);
"#,
    );
    assert_eq!(stdout, "function\n");
}

/// The same sources emitted by tsc as CommonJS and ES2022 must select the
/// same optional installers. This catches collection-time feature leakage,
/// which merely checking the final loader call sites would miss.
#[test]
fn cjs_wrap_closed_samples_select_their_esm_installers() {
    fn installs(source: &str, extension: &str) -> std::collections::BTreeSet<String> {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join(format!("main.{extension}"));
        std::fs::write(&input, source).unwrap();
        let result = Command::new(perry_bin())
            .current_dir(dir.path())
            .arg("compile")
            .arg(&input)
            .arg("-v")
            .arg("--no-cache")
            .arg("-o")
            .arg(dir.path().join("app"))
            .output()
            .unwrap();
        let log = format!(
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.status.success(), "{log}");
        assert!(
            log.contains("runtime installs:"),
            "no install evidence: {log}"
        );
        log.lines()
            .filter_map(|line| {
                line.split_once("runtime installs:")
                    .or_else(|| line.split_once("stdlib installs:"))
            })
            .flat_map(|(_, names)| names.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }
    for (name, cjs, esm) in [
        (
            "02-wc",
            include_str!("fixtures/cjs_closed_support/02-wc.cjs"),
            include_str!("fixtures/cjs_closed_support/02-wc.mjs"),
        ),
        (
            "03-todo",
            include_str!("fixtures/cjs_closed_support/03-todo.cjs"),
            include_str!("fixtures/cjs_closed_support/03-todo.mjs"),
        ),
        (
            "04-json-query",
            include_str!("fixtures/cjs_closed_support/04-json-query.cjs"),
            include_str!("fixtures/cjs_closed_support/04-json-query.mjs"),
        ),
        (
            "05-csv-stats",
            include_str!("fixtures/cjs_closed_support/05-csv-stats.cjs"),
            include_str!("fixtures/cjs_closed_support/05-csv-stats.mjs"),
        ),
    ] {
        assert_eq!(installs(cjs, "cjs"), installs(esm, "mjs"), "{name}");
    }
}

#[test]
fn cjs_wrap_computed_require_and_cache_writer_keep_create_require() {
    for source in [
        "const name = process.argv[2] || 'fs'; console.log(typeof require(name).readFileSync);",
        "require.cache['/a'] = { exports: 42 }; console.log(require.cache['/a'].exports);",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.cjs");
        std::fs::write(&input, source).unwrap();
        let result = Command::new(perry_bin())
            .current_dir(dir.path())
            .env("PERRY_LAYOUT_REPORT", "2")
            .arg("compile")
            .arg(&input)
            .arg("--print-hir")
            .arg("--no-link")
            .arg("--no-cache")
            .arg("-o")
            .arg(dir.path().join("app.o"))
            .output()
            .unwrap();
        let log = format!(
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.status.success(), "{log}");
        assert!(log.contains("CommonJS not closed:"), "{log}");
        assert!(log.contains("method: \"createRequire\""), "{log}");
    }
}
