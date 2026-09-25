//! CommonJS local require observes the live module cache (#11249).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn successful(command: &mut Command, subject: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{subject}: {error}"));
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn commonjs_cache_invalidation_matches_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let entry = root.join("main.ts");
    std::fs::write(
        root.join("dynamic.cjs"),
        r#"globalThis.__dynamicRuns = (globalThis.__dynamicRuns || 0) + 1;
module.exports = { run: globalThis.__dynamicRuns, replace() { module.exports = { run: 42 }; } };
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("eager.cjs"),
        r#"globalThis.__eagerRuns = (globalThis.__eagerRuns || 0) + 1;
module.exports = { run: globalThis.__eagerRuns, replace() { module.exports = { run: 42 }; } };
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("empty.cjs"),
        r#"globalThis.__emptyRuns = (globalThis.__emptyRuns || 0) + 1; module.exports = undefined;
"#,
    )
    .unwrap();
    std::fs::write(root.join("entry.cjs"), r#"const eager = require('./eager.cjs');
const eagerKey = __dirname + '/eager.cjs';
console.log('eager-cache', eager.run, require('./eager.cjs') === eager);
eager.replace();
console.log('eager-exports', require('./eager.cjs').run);
delete require.cache[eagerKey];
const eager2 = require('./eager.cjs');
console.log('eager-reload', eager2.run, eager2 === eager, require.cache[eagerKey]?.exports === eager2);
require.cache[eagerKey] = { exports: { run: 99 }, loaded: true };
console.log('eager-replaced', require('./eager.cjs').run);
delete require.cache[eagerKey];
console.log('eager-again', require('./eager.cjs').run);
function load() { return require('./lazy.cjs'); }
const lazy = load();
const lazyKey = __dirname + '/lazy.cjs';
console.log('lazy-cache', lazy.run, load() === lazy);
delete require.cache[lazyKey];
const lazy2 = load();
console.log('lazy-reload', lazy2.run, lazy2 === lazy, require.cache[lazyKey]?.exports === lazy2);
console.log('parent', require.cache[lazyKey]?.parent === module, module.children.includes(require.cache[lazyKey]));
require.cache[lazyKey] = { exports: { run: 88 }, loaded: true };
console.log('lazy-replaced', load().run);
function dynamic(name) { return require('./' + name + '.cjs'); }
const dyn = dynamic('dynamic');
const dynKey = __dirname + '/dynamic.cjs';
console.log('dynamic-cache', dyn.run, dynamic('dynamic') === dyn);
delete require.cache[dynKey];
console.log('dynamic-reload', dynamic('dynamic').run);
function empty() { return require('./empty.cjs'); }
console.log('empty', typeof empty(), globalThis.__emptyRuns);
delete require.cache[__dirname + '/empty.cjs'];
console.log('empty-reload', typeof empty(), globalThis.__emptyRuns);
const { createRequire } = require('node:module');
const other = createRequire(__filename);
delete other.cache[eagerKey];
const viaOther = other('./eager.cjs');
console.log('createRequire', viaOther.run, require('./eager.cjs') === viaOther);
module.exports = {};
"#).unwrap();
    std::fs::write(
        root.join("lazy.cjs"),
        r#"globalThis.__lazyRuns = (globalThis.__lazyRuns || 0) + 1;
module.exports = { run: globalThis.__lazyRuns, replace() { module.exports = { run: 42 }; } };
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("main.ts"),
        r#"import './dynamic.cjs';
import './entry.cjs';
"#,
    )
    .unwrap();
    let node = successful(
        Command::new("node").current_dir(&root).arg(&entry),
        "Node oracle",
    );
    let expected = r#"eager-cache 1 true
eager-exports 42
eager-reload 2 false true
eager-replaced 99
eager-again 3
lazy-cache 1 true
lazy-reload 2 false true
parent true true
lazy-replaced 88
dynamic-cache 1 true
dynamic-reload 2
empty undefined 1
empty-reload undefined 2
createRequire 4 true
"#;
    assert_eq!(
        node.stdout,
        expected.as_bytes(),
        "cache invalidation fixture must execute every reload"
    );
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let executable = root.join(if cfg!(windows) {
        "app.exe"
    } else {
        "app-native"
    });
    successful(
        Command::new(compiler)
            .current_dir(&root)
            .env("PERRY_RUNTIME_DIR", runtime)
            .env("PERRY_WORKSPACE_ROOT", workspace)
            .args(["compile", "--no-cache", "--no-auto-optimize"])
            .arg(&entry)
            .arg("-o")
            .arg(&executable),
        "Perry compile",
    );
    let native = successful(
        Command::new(executable).current_dir(&root),
        "Perry executable",
    );
    assert_eq!(
        native.stdout, node.stdout,
        "CommonJS cache invalidation must match Node"
    );
}
