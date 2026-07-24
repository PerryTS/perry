//! Tests for the dynamic-import glob expansion + module collection driver.
//! Split out of `collect_modules.rs` to keep that file under the file-size gate.

use super::{
    collect_js_module_imports, collect_modules, env_defines_for_lowering,
    expand_dynamic_import_glob, refuse_compile_package_native_addon,
};
use crate::commands::compile::{CompilationContext, DefineValue};
use crate::commands::progress::VerboseProgress;
use crate::OutputFormat;
use std::collections::HashSet;

#[test]
fn env_defines_for_lowering_strips_prefix_and_maps_kinds() {
    // #5009: only `process.env.*` define keys are honored, the prefix is
    // stripped to the bare env var name, and each DefineValue kind maps to the
    // matching perry_hir::EnvDefine.
    let mut define = std::collections::HashMap::new();
    define.insert(
        "process.env.NODE_ENV".to_string(),
        DefineValue::Str("production".into()),
    );
    define.insert("process.env.DEBUG".to_string(), DefineValue::Bool(false));
    define.insert("process.env.LEVEL".to_string(), DefineValue::Number(3.0));
    define.insert("process.env.MISSING".to_string(), DefineValue::Null);
    // A non-`process.env.*` key is ignored (no other define namespace today).
    define.insert("__DEV__".to_string(), DefineValue::Bool(true));

    let mapped = env_defines_for_lowering(&define);
    assert_eq!(mapped.len(), 4, "the non-process.env key is dropped");
    assert!(!mapped.contains_key("__DEV__"));
    assert!(matches!(
        mapped.get("NODE_ENV"),
        Some(perry_hir::EnvDefine::Str(s)) if s == "production"
    ));
    assert!(matches!(
        mapped.get("DEBUG"),
        Some(perry_hir::EnvDefine::Bool(false))
    ));
    assert!(matches!(
        mapped.get("LEVEL"),
        Some(perry_hir::EnvDefine::Num(n)) if *n == 3.0
    ));
    assert!(matches!(
        mapped.get("MISSING"),
        Some(perry_hir::EnvDefine::Null)
    ));
}

#[test]
fn expands_directory_files_matching_suffix() {
    // #1674 sub-B: glob `./plugins/*.ts` against the importing module's dir.
    let base = std::env::temp_dir().join(format!("perry_glob_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let plugins = base.join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("alpha.ts"), "export const x=1;").unwrap();
    std::fs::write(plugins.join("beta.ts"), "export const x=2;").unwrap();
    std::fs::write(plugins.join("notes.md"), "ignored: wrong suffix").unwrap();
    let importing = base.join("main.ts");
    std::fs::write(&importing, "").unwrap();

    let got = expand_dynamic_import_glob(importing.to_str().unwrap(), "./plugins/", ".ts", 64);
    assert_eq!(
        got,
        vec![
            "./plugins/alpha.ts".to_string(),
            "./plugins/beta.ts".to_string()
        ]
    );

    // A directory with no matches yields nothing (→ rejected promise).
    let none = expand_dynamic_import_glob(importing.to_str().unwrap(), "./plugins/", ".mjs", 64);
    assert!(none.is_empty());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn dependency_is_transformed_before_importer_for_cross_module_inline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let dep = root.join("dep.ts");
    let entry = root.join("entry.ts");

    std::fs::write(
        &dep,
        r#"
export class Dep {
  marker(): number {
return 424242;
  }
}
"#,
    )
    .expect("write dep");
    std::fs::write(
        &entry,
        r#"
import { Dep } from "./dep";

const dep = new Dep();
const got = dep.marker();
console.log(got);
"#,
    )
    .expect("write entry");

    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("collect modules");

    let entry_hir = ctx
        .native_modules
        .get(&entry.canonicalize().unwrap())
        .expect("entry module collected");
    let entry_debug = format!("{entry_hir:?}");

    assert!(
        entry_debug.contains("424242"),
        "entry HIR should contain the dependency method literal after cross-module inlining:\n{entry_debug}"
    );
}

#[test]
fn fs_promises_named_glob_in_class_enables_regex_engine() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("entry.ts");
    std::fs::write(
        &entry,
        r#"
import { glob as findFiles } from "node:fs/promises";
class Scanner {
  static scan() {
    return findFiles("**/*.ts");
  }
}
const iterator = Scanner.scan();
void iterator;
"#,
    )
    .expect("write entry");

    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("collect modules");

    assert!(
        ctx.uses_regex,
        "aliased fs/promises.glob in a class body must retain the regex-backed glob engine"
    );
}

#[test]
fn unrelated_named_glob_does_not_enable_regex_engine() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("entry.ts");
    std::fs::write(
        dir.path().join("util.ts"),
        r#"
export function glob(pattern: string): string {
  return pattern;
}
"#,
    )
    .expect("write dependency");
    std::fs::write(
        &entry,
        r#"
import { glob } from "./util";
const value = glob("not-a-runtime-glob");
void value;
"#,
    )
    .expect("write entry");

    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("collect modules");

    assert!(
        !ctx.uses_regex,
        "an unrelated external named glob binding must not retain the regex engine"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_entry_resolves_relative_imports_from_lexical_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let real_parent = root.join("real-parent");
    let alias_parent = root.join("alias-parent");
    let real_app = real_parent.join("app");
    let alias_app = alias_parent.join("app");
    let alias_outside = alias_parent.join("outside");
    let real_outside = real_parent.join("outside");

    std::fs::create_dir_all(&real_app).expect("mkdir real app");
    std::fs::create_dir_all(&alias_outside).expect("mkdir alias outside");
    std::fs::create_dir_all(&real_outside).expect("mkdir real outside");
    std::os::unix::fs::symlink(&real_app, &alias_app).expect("symlink app");

    let dep = alias_outside.join("dep.ts");
    let decoy_dep = real_outside.join("dep.ts");
    let child = alias_app.join("child.ts");
    let entry = alias_app.join("entry.ts");

    std::fs::write(
        &dep,
        r#"
export class ExternalCtor {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
  marker(): string {
    return this.value;
  }
}
"#,
    )
    .expect("write dep");
    std::fs::write(
        &decoy_dep,
        r#"
export class ExternalCtor {
  value: string;
  constructor(value: string) {
    this.value = "decoy:" + value;
  }
  marker(): string {
    return this.value;
  }
}
"#,
    )
    .expect("write decoy dep");
    std::fs::write(
        &child,
        r#"
import { ExternalCtor } from "../outside/dep";

export function makeValue(): string {
  const value = new ExternalCtor("ready");
  return value.marker();
}
"#,
    )
    .expect("write child");
    std::fs::write(
        &entry,
        r#"
import { makeValue } from "./child";

console.log(makeValue());
"#,
    )
    .expect("write entry");

    let mut ctx = CompilationContext::new(alias_parent.to_path_buf());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("collect modules");

    let dep_canonical = dep.canonicalize().expect("canonical dep");
    let decoy_canonical = decoy_dep.canonicalize().expect("canonical decoy dep");
    assert!(
        ctx.native_modules.contains_key(&dep_canonical),
        "relative imports from a symlinked entry must resolve from the lexical path; collected modules: {:?}",
        ctx.native_modules.keys().collect::<Vec<_>>()
    );
    assert!(
        !ctx.native_modules.contains_key(&decoy_canonical),
        "source-visible lexical imports must win over the canonical sibling decoy; collected modules: {:?}",
        ctx.native_modules.keys().collect::<Vec<_>>()
    );
}

#[test]
fn js_import_scan_follows_bare_dot_and_dotdot_specifiers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let app = root.join("app");
    std::fs::create_dir_all(&app).expect("mkdir app");
    std::fs::write(root.join("index.js"), "export const parent = 1;\n").expect("write parent");
    std::fs::write(app.join("index.js"), "export const current = 1;\n").expect("write current");
    let child = app.join("child.js");
    std::fs::write(&child, "import '.';\nexport * from '..';\n").expect("write child");

    let imports = collect_js_module_imports(&child, "import '.';\nexport * from '..';\n");
    let collected = imports
        .into_iter()
        .map(|path| path.canonicalize().expect("canonical import"))
        .collect::<HashSet<_>>();

    assert!(collected.contains(&app.join("index.js").canonicalize().unwrap()));
    assert!(collected.contains(&root.join("index.js").canonicalize().unwrap()));
}

fn write_compile_package_fixture(
    root: &std::path::Path,
    package_name: &str,
    package_json_extra: &str,
) -> std::path::PathBuf {
    let package = root.join("node_modules").join(package_name);
    let lib = package.join("lib");
    std::fs::create_dir_all(&lib).expect("create package lib");
    let package_json = format!(
        r#"{{
  "name": "{package_name}",
  "version": "1.0.0",
  "main": "./lib/index.js"{package_json_extra}
}}"#
    );
    std::fs::write(package.join("package.json"), package_json).expect("write package json");
    std::fs::write(
        lib.join("index.js"),
        r#"
exports.value = 42;
"#,
    )
    .expect("write package entry");

    let entry = root.join("entry.ts");
    std::fs::write(
        &entry,
        format!(
            r#"
import * as pkg from "{package_name}";
console.log(typeof pkg);
"#
        ),
    )
    .expect("write entry");
    entry
}

fn collect_compile_package(
    root: &std::path::Path,
    entry: &std::path::Path,
    package_name: &str,
) -> anyhow::Result<()> {
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.compile_packages.insert(package_name.to_string());
    ctx.allow_native_library.push(package_name.to_string());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry.to_path_buf(),
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .map(|_| ())
}

fn guard_compile_package(
    root: &std::path::Path,
    package_name: &str,
    entry: &std::path::Path,
) -> anyhow::Result<()> {
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.compile_packages.insert(package_name.to_string());
    ctx.compile_package_dirs.insert(
        package_name.to_string(),
        root.join("node_modules")
            .join(package_name)
            .canonicalize()
            .expect("package root"),
    );
    refuse_compile_package_native_addon(&mut ctx, entry)
}

fn assert_compile_package_native_addon_rejected(
    marker_setup: impl FnOnce(&std::path::Path),
    expected_marker: &str,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = write_compile_package_fixture(root, "node-pty", "");
    let package = root.join("node_modules/node-pty");
    marker_setup(&package);

    let err = collect_compile_package(root, &entry, "node-pty")
        .expect_err("node native addon packages should not enter compilePackages");
    let message = err.to_string();
    assert!(message.contains("node-pty"), "got: {message}");
    assert!(message.contains("Node native addon"), "got: {message}");
    assert!(message.contains(expected_marker), "got: {message}");
    assert!(message.contains("perry.compilePackages"), "got: {message}");
    assert!(message.contains("perry.nativeLibrary"), "got: {message}");
}

#[test]
fn compile_package_with_binding_gyp_is_rejected() {
    assert_compile_package_native_addon_rejected(
        |package| {
            std::fs::write(package.join("binding.gyp"), "{}\n").expect("write binding.gyp");
        },
        "binding.gyp",
    );
}

#[test]
fn compile_package_with_prebuilds_dir_is_rejected() {
    assert_compile_package_native_addon_rejected(
        |package| {
            std::fs::create_dir_all(package.join("prebuilds/win32-x64")).expect("create prebuilds");
        },
        "prebuilds/",
    );
}

#[test]
fn compile_package_with_node_file_is_rejected() {
    assert_compile_package_native_addon_rejected(
        |package| {
            let dir = package.join("build/Release");
            std::fs::create_dir_all(&dir).expect("create build dir");
            std::fs::write(dir.join("addon.node"), b"not a real addon")
                .expect("write addon marker");
        },
        "*.node",
    );
}

#[test]
fn compile_package_with_node_directory_is_allowed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(root, "directory-node", "");
    let package = root.join("node_modules/directory-node");
    std::fs::create_dir_all(package.join("build/not-an-addon.node")).expect("create .node dir");
    let entry = package
        .join("lib/index.js")
        .canonicalize()
        .expect("entry path");

    guard_compile_package(root, "directory-node", &entry)
        .expect(".node directories should not be treated as native addon files");
}

#[test]
fn compile_package_with_gypfile_package_json_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = write_compile_package_fixture(
        root,
        "nativeish",
        r#",
  "gypfile": true"#,
    );

    let err = collect_compile_package(root, &entry, "nativeish")
        .expect_err("gypfile packages should not enter compilePackages");
    let message = err.to_string();
    assert!(message.contains("nativeish"), "got: {message}");
    assert!(message.contains("package.json gypfile"), "got: {message}");
    assert!(message.contains("perry.nativeLibrary"), "got: {message}");
}

#[test]
fn compile_package_with_gypfile_false_is_allowed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(
        root,
        "not-gyp",
        r#",
  "gypfile": false"#,
    );
    let entry = root
        .join("node_modules/not-gyp/lib/index.js")
        .canonicalize()
        .expect("entry path");

    guard_compile_package(root, "not-gyp", &entry)
        .expect("gypfile false should not be treated as a native addon marker");
}

#[test]
fn compile_package_with_native_addon_loader_dependency_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = write_compile_package_fixture(
        root,
        "loader-native",
        r#",
  "dependencies": {
    "node-gyp-build": "^4.8.0"
  }"#,
    );

    let err = collect_compile_package(root, &entry, "loader-native")
        .expect_err("node-gyp-build packages should not enter compilePackages");
    let message = err.to_string();
    assert!(message.contains("loader-native"), "got: {message}");
    assert!(
        message.contains("native addon loader dependency"),
        "got: {message}"
    );
    assert!(message.contains("perry.nativeLibrary"), "got: {message}");
}

#[test]
fn compile_package_with_loader_dev_dependency_is_allowed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(
        root,
        "loader-dev-only",
        r#",
  "devDependencies": {
    "bindings": "^1.5.0"
  }"#,
    );
    let entry = root
        .join("node_modules/loader-dev-only/lib/index.js")
        .canonicalize()
        .expect("entry path");

    guard_compile_package(root, "loader-dev-only", &entry)
        .expect("dev-only native addon loader dependencies should not be hard rejected");
}

#[test]
fn compile_package_nested_manifest_uses_compile_package_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(root, "nested-native", "");
    let package = root.join("node_modules/nested-native");
    let nested = package.join("lib/esm");
    std::fs::create_dir_all(&nested).expect("nested dir");
    std::fs::write(nested.join("package.json"), r#"{ "type": "module" }"#)
        .expect("nested package json");
    std::fs::write(nested.join("index.js"), "export const value = 42;\n").expect("nested entry");
    std::fs::write(package.join("binding.gyp"), "{}\n").expect("write binding.gyp");
    let entry = nested.join("index.js").canonicalize().expect("entry path");

    let err = guard_compile_package(root, "nested-native", &entry)
        .expect_err("root native marker should be detected past nested package.json");
    let message = err.to_string();
    assert!(message.contains("nested-native"), "got: {message}");
    assert!(message.contains("binding.gyp"), "got: {message}");
}

#[test]
fn normal_compile_package_without_native_addon_is_allowed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(root, "pure-js", "");
    let entry = root
        .join("node_modules/pure-js/lib/index.js")
        .canonicalize()
        .expect("entry path");

    guard_compile_package(root, "pure-js", &entry).expect("pure JS package should not be rejected");
}

#[test]
fn perry_native_library_package_is_not_rejected_by_node_addon_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_compile_package_fixture(
        root,
        "native-lib",
        r#",
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        { "name": "js_native_lib_value", "params": [], "returns": "number" }
      ]
    }
  }"#,
    );
    let package = root.join("node_modules/native-lib");
    std::fs::write(package.join("binding.gyp"), "{}\n").expect("write binding.gyp");
    let entry = package
        .join("lib/index.js")
        .canonicalize()
        .expect("entry path");

    guard_compile_package(root, "native-lib", &entry)
        .expect("perry.nativeLibrary package should not be rejected by the Node addon guard");
}

#[cfg(unix)]
#[test]
fn bun_compile_package_js_esm_realpath_parses_as_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let node_modules = root.join("node_modules");
    let glob_pkg = node_modules.join(".bun/glob@13.0.5/node_modules/glob");
    let esm_dir = glob_pkg.join("dist/esm");
    std::fs::create_dir_all(&esm_dir).expect("create glob esm dir");
    std::fs::write(
        glob_pkg.join("package.json"),
        r#"{
  "name": "glob",
  "version": "13.0.5",
  "type": "module",
  "exports": {
".": {
  "import": {
    "default": "./dist/esm/index.min.js"
  },
  "require": {
    "default": "./dist/commonjs/index.min.js"
  }
}
  },
  "module": "./dist/esm/index.min.js",
  "main": "./dist/commonjs/index.min.js"
}"#,
    )
    .expect("write package json");
    std::fs::write(esm_dir.join("package.json"), r#"{ "type": "module" }"#)
        .expect("write esm package json");
    std::fs::write(esm_dir.join("dep.js"), "export const dep=41;\n").expect("write dep");
    std::fs::write(
        esm_dir.join("index.min.js"),
        r#"import{dep}from"./dep.js";const value=dep+1;export{value};"#,
    )
    .expect("write index");

    std::os::unix::fs::symlink(
        ".bun/glob@13.0.5/node_modules/glob",
        node_modules.join("glob"),
    )
    .expect("symlink glob");

    let entry = root.join("entry.ts");
    std::fs::write(
        &entry,
        r#"
import { value } from "glob";
console.log(value);
"#,
    )
    .expect("write entry");

    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.compile_packages.insert("glob".to_string());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);

    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("collect modules");

    let canonical_index = esm_dir.join("index.min.js").canonicalize().unwrap();
    let canonical_dep = esm_dir.join("dep.js").canonicalize().unwrap();
    assert!(
        ctx.native_modules.contains_key(&canonical_index),
        "glob ESM entry should be compiled natively from Bun realpath"
    );
    assert!(
        ctx.native_modules.contains_key(&canonical_dep),
        "glob ESM dependency should be compiled natively from Bun realpath"
    );
    assert!(
        ctx.js_modules.is_empty(),
        "compilePackages ESM files should not route through JS runtime"
    );
}
