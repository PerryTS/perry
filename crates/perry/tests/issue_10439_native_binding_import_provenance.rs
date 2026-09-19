//! Regression test for #10439: `new Command()` / `new LRUCache()` /
//! `new Decimal()` were intercepted by CLASS NAME and routed to Perry's
//! native binding regardless of `perry.compilePackages` — a user could not
//! opt out of the (broken) native handle by asking for real-source
//! compilation. Fixed by `detect_native_instance_expr`
//! (`crates/perry-hir/src/lower_patterns.rs`) consulting `lookup_native_module`
//! (the same compilePackages-aware provenance table `is_native_module`
//! populates at import-lowering time) instead of matching on the bare
//! identifier spelling.
//!
//! The hijack only manifested for a method CHAINED DIRECTLY onto `new
//! X(...)` (`new Command().name(...)`, `new LRUCache(...).set(...)`, `new
//! Decimal(...).dividedBy(...)`) — that shape short-circuits straight to
//! `Expr::NativeMethodCall` in `expr_call/static_and_instance.rs`, bypassing
//! every other (already provenance-aware) construction-site gate. A
//! `let`/`const`-bound receiver was never affected, which is why each fixture
//! below exercises the chained form specifically.
//!
//! Modeled on `issue_8749_compiled_package_builtin_import.rs`'s temp
//! `compilePackages` fixture pattern: a fake `node_modules/<pkg>` with a real
//! ES class shaped like the collision, so compiling it from source is
//! unambiguous (no real npm registry access needed).

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `node_modules/<pkg_name>/{package.json,index.mjs}` under `root`,
/// exporting the given real class source (`.mjs`, so no extra transpile step
/// leaks into HIR that the real npm packages wouldn't have either).
fn write_fake_package(root: &Path, pkg_name: &str, class_source: &str) {
    let pkg = root.join("node_modules").join(pkg_name);
    std::fs::create_dir_all(&pkg).expect("mkdir fake package");
    std::fs::write(
        pkg.join("package.json"),
        format!(
            r#"{{
  "name": "{pkg_name}",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.mjs"
}}"#
        ),
    )
    .expect("write fake package.json");
    std::fs::write(pkg.join("index.mjs"), class_source).expect("write fake package source");
}

fn write_compile_packages_manifest(root: &Path, pkg_name: &str) {
    std::fs::write(
        root.join("package.json"),
        format!(
            r#"{{
  "name": "issue-10439-consumer",
  "private": true,
  "type": "module",
  "perry": {{
    "compilePackages": ["{pkg_name}"],
    "allow": {{ "compilePackages": ["{pkg_name}"] }}
  }}
}}"#
        ),
    )
    .expect("write consumer package.json");
}

/// Compile `entry` (already written under `root`) and return its stdout,
/// asserting both the compile and the run succeeded.
fn compile_and_run(root: &Path, entry_name: &str) -> String {
    let entry = root.join(entry_name);
    let output = root.join(format!("{entry_name}.bin"));
    let compile = Command::new(perry_bin())
        .current_dir(root)
        // Auto-optimize triggers a full profile-guided workspace rebuild on
        // its first invocation — expensive, and irrelevant to this test
        // (which is about construction/dispatch routing, not optimization).
        // Every manual repro of #10439 used the same no-auto path.
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed for {entry_name}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .output()
        .unwrap_or_else(|e| panic!("run compiled binary for {entry_name}: {e}"));
    assert!(
        run.status.success(),
        "compiled binary failed for {entry_name}\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// #10439 case: commander's `Command`. Real source, default import name,
/// chained directly onto `new` — the exact shape `builtin.rs:405`'s
/// unconditional `"Command"` arm used to reach via the unguarded
/// `detect_native_instance_expr` match, regardless of `compilePackages`.
#[test]
fn commander_default_name_reaches_real_source_under_compile_packages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_packages_manifest(root, "commander");
    write_fake_package(
        root,
        "commander",
        r#"
export class Command {
  constructor() { this.__name = ""; }
  name(n) {
    if (n === undefined) return this.__name;
    this.__name = n;
    return this;
  }
}
"#,
    );

    // Default spelling, chained directly on `new` — previously hijacked.
    std::fs::write(
        root.join("main.ts"),
        r#"
import { Command } from "commander";
console.log(new Command().name("real-commander-source").name());
"#,
    )
    .expect("write main.ts");
    assert_eq!(
        compile_and_run(root, "main.ts"),
        "real-commander-source\n",
        "new Command().name(...).name() must run the compiled real source, not the native handle"
    );

    // Renamed-import control: this already worked before the fix (the
    // hardcoded match is keyed on the literal spelling "Command"), and must
    // keep working after it.
    std::fs::write(
        root.join("renamed.ts"),
        r#"
import { Command as Cmd } from "commander";
console.log(new Cmd().name("renamed-control").name());
"#,
    )
    .expect("write renamed.ts");
    assert_eq!(
        compile_and_run(root, "renamed.ts"),
        "renamed-control\n",
        "the renamed-import workaround must still work unchanged"
    );
}

/// #10439 case: lru-cache's `LRUCache`, chained directly onto `new`.
#[test]
fn lru_cache_default_name_reaches_real_source_under_compile_packages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_packages_manifest(root, "lru-cache");
    write_fake_package(
        root,
        "lru-cache",
        r#"
export class LRUCache {
  constructor(opts) { this.__store = new Map(); this.__max = opts && opts.max || 0; }
  set(k, v) { this.__store.set(k, v); return this; }
  get(k) { return this.__store.get(k); }
}
"#,
    );

    std::fs::write(
        root.join("main.ts"),
        r#"
import { LRUCache } from "lru-cache";
console.log(new LRUCache({ max: 3 }).set("a", 1).get("a"));
"#,
    )
    .expect("write main.ts");
    assert_eq!(
        compile_and_run(root, "main.ts"),
        "1\n",
        "new LRUCache(...).set(...).get(...) must run the compiled real source, not the native handle"
    );

    std::fs::write(
        root.join("renamed.ts"),
        r#"
import { LRUCache as Cache } from "lru-cache";
console.log(new Cache({ max: 3 }).set("a", 2).get("a"));
"#,
    )
    .expect("write renamed.ts");
    assert_eq!(
        compile_and_run(root, "renamed.ts"),
        "2\n",
        "the renamed-import workaround must still work unchanged"
    );
}

/// #10439 case: decimal.js's `Decimal`, chained directly onto `new`. This is
/// the shape #10684 (division/large-multiplication corruption) depends on:
/// the native handle's `dividedBy`/`times` are broken, and the interception
/// prevented compilePackages from ever reaching the real, correct source.
#[test]
fn decimal_default_name_reaches_real_source_under_compile_packages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_packages_manifest(root, "decimal.js");
    write_fake_package(
        root,
        "decimal.js",
        r#"
export default class Decimal {
  constructor(v) { this.__v = typeof v === "string" ? parseFloat(v) : v; }
  dividedBy(n) { return new Decimal(this.__v / (n instanceof Decimal ? n.__v : n)); }
  times(n) { return new Decimal(this.__v * (n instanceof Decimal ? n.__v : n)); }
  toString() { return String(this.__v); }
}
"#,
    );

    std::fs::write(
        root.join("main.ts"),
        r#"
import Decimal from "decimal.js";
console.log(new Decimal(1).dividedBy(4).toString());
"#,
    )
    .expect("write main.ts");
    assert_eq!(
        compile_and_run(root, "main.ts"),
        "0.25\n",
        "new Decimal(1).dividedBy(4).toString() must run the compiled real source, not the native handle"
    );

    std::fs::write(
        root.join("renamed.ts"),
        r#"
import Dec from "decimal.js";
console.log(new Dec(1).dividedBy(4).toString());
"#,
    )
    .expect("write renamed.ts");
    assert_eq!(
        compile_and_run(root, "renamed.ts"),
        "0.25\n",
        "the renamed-import workaround must still work unchanged"
    );
}

/// The legitimate case this issue explicitly warns against regressing: with
/// NO `perry.compilePackages` entry (and no real package installed at all —
/// there is nothing else it COULD mean), `new Command()...` must still route
/// to the native binding exactly as before. Values asserted here are the
/// native binding's own pre-existing (documented-limited) behavior, captured
/// against this same commit's pre-fix binary — this test exists to prove the
/// fix does not change them, not to bless them as correct.
#[test]
fn commander_default_name_still_uses_native_binding_without_compile_packages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    // No package.json, no node_modules: "commander" can only resolve to
    // Perry's bundled native shim.
    std::fs::write(
        root.join("main.ts"),
        r#"
import { Command } from "commander";
const program = new Command();
console.log(new Command().name("x").name());
console.log(program.constructor.name);
"#,
    )
    .expect("write main.ts");
    assert_eq!(
        compile_and_run(root, "main.ts"),
        "{}\nundefined\n",
        "the native-binding path (no compilePackages) must be byte-for-byte unchanged"
    );
}

// `lru_cache_default_name_still_uses_native_binding_without_compile_packages`
// removed here -- it guarded the "legitimate native case" (no
// compilePackages, native binding still handles `new LRUCache(...)`), which
// no longer exists: #10685 deletes the native lru-cache binding entirely.
// Without compilePackages AND without a real installed package, this shape
// now falls through to the generic unresolved-import handling instead
// (compiles, but throws at the first call that isn't actually there) --
// correctly reflecting that there is no source of truth for `LRUCache`
// left to reach, exactly as removing the binding intends.
