use super::{compile, contains, fixture, perry_bin, runtime_dir, write};
use std::path::Path;
use std::process::Command;

fn node_output(root: &Path) -> String {
    let output = Command::new("node")
        .current_dir(root)
        .args(["--experimental-strip-types", "main.ts"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

#[test]
fn inferred_pure_siblings_are_not_compiled_without_package_metadata() {
    let dir = fixture(None);
    write(dir.path(), "node_modules/fixture/unused.js", "export function unused() { console.log('only when called'); } export const data = [1, { value: 2 }];");
    let (on, output) = compile(dir.path(), false, false);
    let (off, baseline) = compile(dir.path(), true, false);
    assert_eq!(output, node_output(dir.path()));
    assert_eq!(output, baseline);
    assert!(!contains(&on, "/fixture/unused.js"));
    assert!(contains(&off, "/fixture/unused.js"));
    assert_eq!(off.len(), on.len() + 1);
}

#[test]
fn inferred_purity_applies_to_first_party_forwarding_barrels() {
    let dir = fixture(None);
    write(
        dir.path(),
        "main.ts",
        "import { used } from './barrel.js'; console.log(used);",
    );
    write(dir.path(), "barrel.js", "import { used } from './used.js'; import { unused } from './unused.js'; export { used, unused };");
    write(dir.path(), "used.js", "export const used = 42;");
    write(dir.path(), "unused.js", "export const unused = 99;");
    let (on, output) = compile(dir.path(), false, false);
    assert!(!contains(&on, "/unused.js"));
    assert_eq!(output, node_output(dir.path()));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[cfg(any(unix, windows))]
#[test]
fn inferred_purity_resolves_dependencies_from_the_source_visible_path() {
    let dir = fixture(None);
    write(
        dir.path(),
        "main.ts",
        "import { used } from './visible/barrel/index.js'; console.log(used);",
    );
    write(
        dir.path(),
        "actual/barrel/index.js",
        "export { used } from './used.js'; export { unused } from './unused.js';",
    );
    write(
        dir.path(),
        "actual/barrel/used.js",
        "export const used = 42;",
    );
    write(
        dir.path(),
        "actual/barrel/unused.js",
        "import '../effect.js'; export const unused = 99;",
    );
    write(dir.path(), "actual/effect.js", "export const inert = 0;");
    write(dir.path(), "visible/effect.js", "console.log('visible');");
    let actual = dir.path().join("actual/barrel");
    let alias = dir.path().join("visible/barrel");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    #[cfg(windows)]
    {
        // Directory junctions need no administrator/developer-mode privilege.
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(alias.to_string_lossy().replace('/', "\\"))
            .arg(actual.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
    }
    let (on, output) = compile(dir.path(), false, false);
    assert!(contains(&on, "/actual/barrel/unused.js"));
    assert!(contains(&on, "/visible/effect.js"));
    assert!(!contains(&on, "/actual/effect.js"));
    assert_eq!(output, "visible\n42\n");
    assert_eq!(output, compile(dir.path(), true, false).1);
    // Perry deliberately uses the lexical import base through symlinks.
    let node = Command::new("node")
        .current_dir(dir.path())
        .args([
            "--preserve-symlinks",
            "--experimental-strip-types",
            "main.ts",
        ])
        .output()
        .unwrap();
    assert!(node.status.success(), "{:?}", node);
    assert_eq!(
        output,
        String::from_utf8(node.stdout)
            .unwrap()
            .replace("\r\n", "\n")
    );
}

#[test]
fn mixed_forwarding_barrels_prune_unused_siblings_and_keep_bare_dependencies() {
    for contract in [None, Some(false.into())] {
        let dir = fixture(contract);
        write(dir.path(), "node_modules/fixture/index.js", "import { used } from './used.js'; import { unused } from './unused.js'; import './shared.js'; export { used, unused }; export * from './extra.js';");
        write(
            dir.path(),
            "node_modules/fixture/shared.js",
            "export const shared = 7;",
        );
        write(
            dir.path(),
            "node_modules/fixture/extra.js",
            "export const extra = 8;",
        );
        let (on, output) = compile(dir.path(), false, false);
        assert!(!contains(&on, "/fixture/unused.js"));
        assert!(!contains(&on, "/fixture/extra.js"));
        assert!(contains(&on, "/fixture/shared.js"));
        assert_eq!(output, node_output(dir.path()));
        assert_eq!(output, compile(dir.path(), true, false).1);
    }
}

#[test]
fn mixed_forwarding_barrels_preserve_the_entry_into_a_cycle() {
    for contract in [None, Some(false.into())] {
        let dir = fixture(contract);
        write(
            dir.path(),
            "node_modules/fixture/index.js",
            "import { a } from './a.js'; import './b.js'; export { a };",
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
            "import { a } from 'fixture'; console.log(a);",
        );
        let (_, output) = compile(dir.path(), false, false);
        assert_eq!(output, "2\n");
        assert_eq!(output, node_output(dir.path()));
        assert_eq!(output, compile(dir.path(), true, false).1);
    }
}

#[test]
fn inferred_purity_retains_effectful_sibling_in_esm_order() {
    let dir = fixture(None);
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
    write(dir.path(), "node_modules/fixture/index.js", "export { used } from './used.js'; export { unused } from './unused.js'; export * from './inert.js';");
    write(
        dir.path(),
        "node_modules/fixture/inert.js",
        "export const inert = 0;",
    );
    let (on, output) = compile(dir.path(), false, false);
    assert!(contains(&on, "/fixture/unused.js"));
    assert!(!contains(&on, "/fixture/inert.js"));
    assert_eq!(output, "used\nunused\n42\n");
    assert_eq!(output, node_output(dir.path()));
    assert_eq!(output, compile(dir.path(), true, false).1);
}

#[test]
fn inferred_purity_checks_dependencies_and_honors_explicit_contracts() {
    let dir = fixture(None);
    write(
        dir.path(),
        "node_modules/external/package.json",
        r#"{"name":"external","type":"module","main":"index.js"}"#,
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
    let (on, output) = compile(dir.path(), false, false);
    assert!(contains(&on, "/fixture/unused.js"));
    assert!(contains(&on, "/external/index.js"));
    assert_eq!(output, "external\n42\n");
    assert_eq!(output, node_output(dir.path()));
    assert_eq!(output, compile(dir.path(), true, false).1);

    for contract in [serde_json::json!(true), serde_json::json!(["[ab].js"])] {
        let dir = fixture(Some(contract));
        let (on, _) = compile(dir.path(), false, true);
        assert!(contains(&on, "/fixture/unused.js"));
    }
}

#[test]
fn inferred_pure_dynamic_target_is_available_but_not_eagerly_initialized() {
    let dir = fixture(None);
    write(
        dir.path(),
        "node_modules/fixture/lazy.js",
        "console.log('lazy init'); export const lazy = 7;",
    );
    write(dir.path(), "main.ts", "import { used } from 'fixture'; export async function load() { return import('fixture/lazy.js'); } console.log(used);");
    let (on, output) = compile(dir.path(), false, false);
    assert!(!contains(&on, "/fixture/unused.js"));
    assert!(contains(&on, "/fixture/lazy.js"));
    assert_eq!(output, "42\n");
    assert_eq!(output, node_output(dir.path()));
    assert_eq!(output, compile(dir.path(), true, false).1);
    compile(dir.path(), false, true);
    let graph: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("cache-on/module-graph.json")).unwrap(),
    )
    .unwrap();
    let lazy = graph["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| {
            module["path"]
                .as_str()
                .unwrap()
                .replace('\\', "/")
                .ends_with("/fixture/lazy.js")
        })
        .unwrap();
    assert_eq!(lazy["init"], "deferred");
}

#[test]
fn cached_build_rechecks_inferred_purity_when_omitted_source_gains_an_effect() {
    let dir = fixture(None);
    let binary = dir.path().join("cached-inferred");
    let run = || {
        let output = Command::new(perry_bin())
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
        assert!(output.status.success(), "{:?}", output);
        let output = Command::new(&binary).output().unwrap();
        assert!(output.status.success(), "{:?}", output);
        String::from_utf8(output.stdout).unwrap()
    };
    assert_eq!(run(), "42\n");
    assert_eq!(run(), "42\n");
    write(
        dir.path(),
        "node_modules/fixture/unused.js",
        "console.log('restored'); export const unused = 99;",
    );
    assert_eq!(run(), "restored\n42\n");
    assert_eq!(run(), node_output(dir.path()));
}
