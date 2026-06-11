//! Tests for the dynamic-import glob expansion + module collection driver.
//! Split out of `collect_modules.rs` to keep that file under the file-size gate.

use super::{collect_modules, env_defines_for_lowering, expand_dynamic_import_glob};
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
