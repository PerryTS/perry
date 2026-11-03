//! A package's TypeScript sources stand in for its published JavaScript only
//! when they are self-contained (`package_sources_self_contained`): the
//! Supabase CLI linked `@posthog/core` and `@supabase/supabase-js` from their
//! sources, whose `@/utils` alias and unpublished `@supabase/tracing` import
//! were left unbound.

use super::resolve_package_source_entry;
use std::fs;
use std::path::{Path, PathBuf};

/// A package shipping `dist/index.js` with a source map to `src/index.ts`,
/// whose text is `source`; plus a `src/util.ts` it may import.
fn package(root: &Path, name: &str, source: &str) -> (PathBuf, PathBuf) {
    let dir = root.join("node_modules").join(name);
    fs::create_dir_all(dir.join("dist")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("dist/index.js"), "export const x = 1;\n").unwrap();
    fs::write(
        dir.join("dist/index.js.map"),
        r#"{"version":3,"file":"index.js","sources":["../src/index.ts"],"mappings":""}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), source).unwrap();
    fs::write(dir.join("src/util.ts"), "export const u = 1;\n").unwrap();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({ "name": name, "type": "module", "module": "./dist/index.js" })
            .to_string(),
    )
    .unwrap();
    let source = dir.join("src/index.ts").canonicalize().unwrap();
    (dir, source)
}

#[test]
fn a_path_alias_keeps_the_package_on_its_javascript() {
    let root = tempfile::tempdir().unwrap();
    let (dir, _) = package(
        root.path(),
        "aliased",
        "import { u } from '@/utils';\nexport const x = u;\n",
    );
    assert_eq!(resolve_package_source_entry(&dir, None), None);
}

#[test]
fn an_unpublished_workspace_import_keeps_the_package_on_its_javascript() {
    let root = tempfile::tempdir().unwrap();
    let (dir, _) = package(
        root.path(),
        "bundles-tracing",
        "import { u } from './util.js';\nexport { parse } from '@scope/not-installed';\nexport const x = u;\n",
    );
    assert_eq!(resolve_package_source_entry(&dir, None), None);
}

#[test]
fn self_contained_sources_are_still_used() {
    let root = tempfile::tempdir().unwrap();
    let dep = root.path().join("node_modules/dep");
    fs::create_dir_all(&dep).unwrap();
    fs::write(dep.join("package.json"), r#"{"name":"dep"}"#).unwrap();
    let (dir, source) = package(
        root.path(),
        "contained",
        "import type { T } from '@types/not-installed';\nimport { u } from './util.js';\nimport d from 'dep';\nimport fs from 'node:fs';\nexport const x: T = [u, d, fs];\n",
    );
    assert_eq!(resolve_package_source_entry(&dir, None), Some(source));
}
