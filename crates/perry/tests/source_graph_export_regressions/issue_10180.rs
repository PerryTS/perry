//! Collection and runtime regressions for unused re-export pruning. Each
//! assertion reads the actual collection audit, so a successful executable
//! alone cannot turn the pruning checks into vacuous parity tests.

use std::path::Path;
use std::process::Command;

use super::{perry_bin, runtime_dir};

#[path = "issue_10180/purity.rs"]
mod purity;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn fixture(side_effects: Option<serde_json::Value>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "package.json",
        r#"{"perry":{"compilePackages":["fixture","external"],"allow":{"compilePackages":["fixture","external"]}}}"#,
    );
    let mut package = serde_json::json!({"name":"fixture","type":"module","main":"index.js"});
    if let Some(value) = side_effects {
        package["sideEffects"] = value;
    }
    write(
        dir.path(),
        "node_modules/fixture/package.json",
        &package.to_string(),
    );
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export { used } from './used.js'; export { unused } from './unused.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/used.js",
        "export const used = 42;",
    );
    write(
        dir.path(),
        "node_modules/fixture/unused.js",
        "export const unused = 99;",
    );
    write(
        dir.path(),
        "main.ts",
        "import { used } from 'fixture'; console.log(used);",
    );
    dir
}

fn compile(root: &Path, disabled: bool, collect_only: bool) -> (Vec<String>, String) {
    let cache = root.join(if disabled { "cache-off" } else { "cache-on" });
    let binary = root.join(if disabled { "main-off" } else { "main-on" });
    // A collect-only invocation may follow a complete compile in this fixture.
    // Its output assertion must observe this invocation, not the earlier binary.
    if binary.exists() {
        std::fs::remove_file(&binary).unwrap();
    }
    let mut command = Command::new(perry_bin());
    command
        .current_dir(root)
        .args(["compile", "main.ts", "--no-cache", "--cache-dir"])
        .arg(&cache)
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_NO_REEXPORT_PRUNE", if disabled { "1" } else { "0" })
        .env("PERRY_COLLECT_ONLY", if collect_only { "1" } else { "0" });
    if !collect_only {
        command.env("PERRY_RUNTIME_DIR", runtime_dir());
    }
    let result = command.output().unwrap();
    let output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "{output}");
    assert!(
        output.contains("pruned as unreferenced side-effect-free re-exports"),
        "{output}"
    );
    let audit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("audit.json")).unwrap()).unwrap();
    let paths = audit["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["source"].as_str().unwrap().to_owned())
        .collect();
    if collect_only {
        assert!(
            !binary.exists(),
            "collect-only must stop before code generation"
        );
        return (paths, output);
    }
    let result = Command::new(binary).current_dir(root).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    (paths, String::from_utf8(result.stdout).unwrap())
}

fn contains(paths: &[String], file: &str) -> bool {
    paths.iter().any(|p| p.replace('\\', "/").ends_with(file))
}

#[test]
fn unused_sibling_is_never_collected_and_disable_switch_restores_it() {
    let dir = fixture(Some(false.into()));
    let (on, stdout) = compile(dir.path(), false, false);
    assert_eq!(stdout, "42\n");
    assert!(contains(&on, "/fixture/used.js"));
    assert!(!contains(&on, "/fixture/unused.js"));
    let (off, stdout_off) = compile(dir.path(), true, false);
    assert_eq!(stdout, stdout_off);
    assert_eq!(off.len(), on.len() + 1);
    assert!(contains(&off, "/fixture/unused.js"));
}

#[test]
fn missing_or_effectful_contract_keeps_side_effect_order() {
    for contract in [None, Some(true.into())] {
        let dir = fixture(contract);
        write(
            dir.path(),
            "node_modules/fixture/used.js",
            "console.log('used'); export const used = 42;",
        );
        write(
            dir.path(),
            "node_modules/fixture/unused.js",
            "console.log('unused'); export const unused = 99;",
        );
        write(dir.path(), "node_modules/fixture/index.js", "export { used } from './used.js'; export { unused } from './unused.js'; console.log('barrel');");
        let (paths, output) = compile(dir.path(), false, false);
        assert!(contains(&paths, "/fixture/unused.js"));
        assert_eq!(output, "used\nunused\nbarrel\n42\n");
    }
}

#[test]
fn export_star_chain_and_renamed_binding_prune_transitive_siblings() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export * from './middle.js'; export * from './unused.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/middle.js",
        "export { used as answer } from './used.js'; export * from './extra.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/extra.js",
        "export const extra = 7;",
    );
    write(
        dir.path(),
        "main.ts",
        "import { answer } from 'fixture'; console.log(answer);",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "42\n");
    assert!(contains(&paths, "/fixture/middle.js"));
    assert!(!contains(&paths, "/fixture/unused.js"));
    assert!(!contains(&paths, "/fixture/extra.js"));
}

#[test]
fn later_direct_import_restores_a_previously_unused_module() {
    let dir = fixture(Some(false.into()));
    write(dir.path(), "main.ts", "import { used } from 'fixture'; import { unused } from 'fixture/unused.js'; console.log(used, unused);");
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "42 99\n");
}

#[test]
fn later_importer_adds_demand_to_already_visited_barrel() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "later.ts",
        "import { unused } from 'fixture'; export const later = unused;",
    );
    write(dir.path(), "main.ts", "import { used } from 'fixture'; import { later } from './later'; console.log(used, later);");
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "42 99\n");
}

#[test]
fn dynamic_import_retains_namespace_and_defers_initialization() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/unused.js",
        "console.log('dynamic'); export const unused = 99;",
    );
    write(dir.path(), "later.ts", "export async function load() { const ns = await import('fixture/unused.js'); console.log(ns.unused); }");
    write(dir.path(), "main.ts", "import { used } from 'fixture'; import { load } from './later'; console.log(used); load();");
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "42\ndynamic\n99\n");
}

#[test]
fn dynamic_import_of_a_static_barrel_restores_all_exports() {
    let dir = fixture(Some(false.into()));
    write(dir.path(), "main.ts", "import { used } from 'fixture'; console.log(used); async function load() { const ns = await import('fixture'); console.log(ns.unused); } load();");
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "42\n99\n");
}

#[test]
fn live_binding_is_read_after_mutation_through_reexport() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/used.js",
        "export let counter = 0; export function increment() { counter++; }",
    );
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export { counter, increment } from './used.js'; export * from './unused.js';",
    );
    write(dir.path(), "main.ts", "import { counter, increment } from 'fixture'; console.log(counter); increment(); console.log(counter);");
    let (paths, output) = compile(dir.path(), false, false);
    assert!(!contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "0\n1\n");
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn side_effect_globs_preserve_matches_and_external_effect_dependencies() {
    let dir = fixture(Some(serde_json::json!(["**/effect*.js"])));
    write(dir.path(), "node_modules/fixture/index.js", "export { used } from './used.js'; export * from './unused.js'; export * from './effect.js';");
    write(
        dir.path(),
        "node_modules/fixture/effect.js",
        "console.log('effect'); export const effect = 1;",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "effect\n42\n");
    assert!(contains(&paths, "/fixture/effect.js"));
    assert!(!contains(&paths, "/fixture/unused.js"));

    write(
        dir.path(),
        "node_modules/external/package.json",
        r#"{"name":"external","main":"index.js"}"#,
    );
    write(
        dir.path(),
        "node_modules/external/index.js",
        "console.log('external');",
    );
    write(
        dir.path(),
        "node_modules/fixture/unused.js",
        "import 'external'; export const unused = 99;",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/external/index.js"));
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, "external\neffect\n42\n");
}

#[test]
fn collect_only_handles_star_cycles_and_unknown_globs_conservatively() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export * from './cycle.js'; export * from './used.js'; export * from './unused.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/cycle.js",
        "export * from './index.js';",
    );
    let (paths, _) = compile(dir.path(), false, true);
    assert!(contains(&paths, "/fixture/unused.js"));
    write(
        dir.path(),
        "node_modules/fixture/package.json",
        r#"{"name":"fixture","main":"index.js","sideEffects":["[ab].js"]}"#,
    );
    let (paths, _) = compile(dir.path(), false, true);
    assert!(contains(&paths, "/fixture/unused.js"));
}

#[test]
fn cached_build_rechecks_omitted_sources_and_new_package_contracts() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export { used } from './used.js'; export * from './dead/index.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/dead/index.js",
        "console.log('restored'); export const unused = 99;",
    );
    let binary = dir.path().join("cached-bin");
    let run = || {
        let compile = Command::new(perry_bin())
            .current_dir(dir.path())
            .args(["compile", "main.ts", "--cache-dir", "cache", "-o"])
            .arg(&binary)
            .env("PERRY_NO_AUTO_OPTIMIZE", "1")
            .env("PERRY_RUNTIME_DIR", runtime_dir())
            .env("PERRY_NO_REEXPORT_PRUNE", "0")
            .env_remove("PERRY_NO_CACHE")
            .env_remove("PERRY_COLLECT_ONLY")
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };
    assert_eq!(run(), "42\n");
    assert_eq!(run(), "42\n");
    // No previously fingerprinted file changed: the new manifest must itself
    // invalidate the cached proof that dead/index.js has no side effects.
    write(
        dir.path(),
        "node_modules/fixture/dead/package.json",
        r#"{"sideEffects":true}"#,
    );
    assert_eq!(run(), "restored\n42\n");
    std::fs::remove_file(dir.path().join("node_modules/fixture/dead/package.json")).unwrap();
    assert_eq!(run(), "42\n");
    // Changing only an omitted source introduces an effectful dependency.
    write(
        dir.path(),
        "node_modules/external/package.json",
        r#"{"name":"external","main":"index.js"}"#,
    );
    write(
        dir.path(),
        "node_modules/external/index.js",
        "console.log('source changed');",
    );
    write(
        dir.path(),
        "node_modules/fixture/dead/index.js",
        "import 'external'; export const unused = 99;",
    );
    assert_eq!(run(), "source changed\n42\n");
}

#[test]
fn namespace_keeps_values_but_star_does_not_forward_types_or_default() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export * from './used.js'; export * from './types.ts'; export * from './default.js';",
    );
    write(
        dir.path(),
        "node_modules/fixture/types.ts",
        "export interface Shape { x: number }; export type Alias = string;",
    );
    write(
        dir.path(),
        "node_modules/fixture/default.js",
        "export default 99;",
    );
    write(
        dir.path(),
        "main.ts",
        "import * as ns from 'fixture'; console.log(ns.used, Object.keys(ns).join(','));",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert!(!contains(&paths, "/fixture/types.ts"));
    assert!(!contains(&paths, "/fixture/default.js"));
    assert_eq!(output, "42 used\n");
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn imported_bindings_used_only_by_export_lists_are_reexports() {
    let dir = fixture(Some(false.into()));
    write(dir.path(), "node_modules/fixture/index.js", "import { used as value } from './used.js'; import { unused as other } from './unused.js'; export { value as used, other as unused };");
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "42\n");
    assert!(!contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn forwarding_normalization_preserves_live_aliases() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/used.js",
        "export let counter = 0; export function increment() { counter++; }",
    );
    write(dir.path(), "node_modules/fixture/index.js", "import { counter as localCounter, increment } from './used.js'; import { unused } from './unused.js'; export { localCounter as counter, increment, unused };");
    write(dir.path(), "main.ts", "import { counter, increment } from 'fixture'; console.log(counter); increment(); console.log(counter);");
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "0\n1\n");
    assert!(!contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn imports_used_by_module_code_are_not_normalized() {
    let dir = fixture(Some(false.into()));
    write(dir.path(), "node_modules/fixture/index.js", "import { used } from './used.js'; import { unused } from './unused.js'; console.log(unused); export { used, unused };");
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "99\n42\n");
    assert!(contains(&paths, "/fixture/unused.js"));
}

#[test]
fn import_attributes_keep_forwarding_imports_and_their_loader() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/payload.js",
        "export default 99;",
    );
    write(dir.path(), "node_modules/fixture/index.js", "import { default as asset } from './payload.js' with { type: 'file' }; import { used } from './used.js'; import { unused } from './unused.js'; export { asset, used, unused };");
    write(dir.path(), "main.ts", "import { asset, used } from 'fixture'; console.log(used, typeof asset, asset.endsWith('payload.js'));");
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "42 string true\n");
    assert!(contains(&paths, "/fixture/unused.js"));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn forwarding_barrels_keep_effectful_dependencies_and_bare_imports() {
    let dir = fixture(Some(false.into()));
    write(dir.path(), "node_modules/fixture/index.js", "import { used } from './used.js'; import { unused } from './unused.js'; import './bare.js'; export { used, unused };");
    write(
        dir.path(),
        "node_modules/fixture/bare.js",
        "export const bare = 1;",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "42\n");
    assert!(!contains(&paths, "/fixture/unused.js"));
    assert!(contains(&paths, "/fixture/bare.js"));

    write(
        dir.path(),
        "node_modules/external/package.json",
        r#"{"name":"external","main":"index.js","sideEffects":true}"#,
    );
    write(
        dir.path(),
        "node_modules/external/index.js",
        "console.log('effect');",
    );
    write(
        dir.path(),
        "node_modules/fixture/unused.js",
        "import 'external'; export const unused = 99;",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert_eq!(output, "effect\n42\n");
    assert!(contains(&paths, "/fixture/unused.js"));
    assert!(contains(&paths, "/external/index.js"));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn cyclic_initialization_order_survives_forwarding_and_pruning() {
    let dir = fixture(Some(false.into()));
    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "import { a } from './a.js'; import * as bns from './b.js'; export { a, bns };",
    );
    write(
        dir.path(),
        "node_modules/fixture/a.js",
        "import { b } from './b.js'; export var a = (b ?? 0) + 1;",
    );
    write(
        dir.path(),
        "node_modules/fixture/b.js",
        "import { a } from './a.js'; export var b = (a ?? 0) + 1;",
    );
    write(
        dir.path(),
        "main.ts",
        "import { a, bns } from 'fixture'; console.log(a, bns.b);",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/a.js"));
    assert!(contains(&paths, "/fixture/b.js"));
    assert_eq!(output, "2 1\n");
    assert_eq!(output, compile(dir.path(), true, false).1);

    write(
        dir.path(),
        "node_modules/fixture/index.js",
        "export { a } from './a.js'; export { b } from './b.js';",
    );
    write(
        dir.path(),
        "main.ts",
        "import { b } from 'fixture'; console.log(b);",
    );
    let (paths, output) = compile(dir.path(), false, false);
    assert!(contains(&paths, "/fixture/a.js"));
    assert!(contains(&paths, "/fixture/b.js"));
    assert_eq!(output, "1\n");
    assert_eq!(output, compile(dir.path(), true, false).1);
}
